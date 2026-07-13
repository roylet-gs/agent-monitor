import { describe, it, expect, vi, beforeEach } from "vitest";
import { appendFileSync, existsSync, mkdirSync, readFileSync } from "fs";
import { join } from "path";
import type { ManagedSession, Settings, Worktree } from "../../src/lib/types.js";

// Mock logger to avoid file I/O during tests
vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

// Mock pubsub-client so we can verify published messages
vi.mock("../../src/lib/pubsub-client.js", () => ({
  publishMessage: vi.fn().mockResolvedValue(undefined),
}));

// Mock child_process.spawn — turns must never launch a real claude
const spawnMock = vi.fn();
vi.mock("child_process", async (importOriginal) => {
  const actual = await importOriginal<typeof import("child_process")>();
  return { ...actual, spawn: (...args: unknown[]) => spawnMock(...args) };
});

const SETTINGS = {
  agentPermissionMode: "acceptEdits",
  agentClaudeArgs: "",
} as Settings;

function fakeChild(pid: number | undefined = 4242) {
  return { pid, unref: vi.fn(), on: vi.fn() };
}

describe("claude-session", () => {
  let cs: typeof import("../../src/lib/claude-session.js");
  let db: typeof import("../../src/lib/db.js");
  let worktree: Worktree;

  beforeEach(async () => {
    spawnMock.mockReset().mockReturnValue(fakeChild());
    cs = await import("../../src/lib/claude-session.js");
    db = await import("../../src/lib/db.js");
    const repo = db.addRepository("/tmp/am-test-repo", "test-repo");
    worktree = db.upsertWorktree(repo.id, "/tmp/am-test-repo/wt", "feature/x", "wt");
  });

  describe("buildTurnArgs", () => {
    const session = (turnCount: number): ManagedSession => ({
      id: "11111111-2222-3333-4444-555555555555",
      worktree_id: "w1",
      cwd: "/tmp/wt",
      last_prompt: null,
      turn_pid: null,
      turn_count: turnCount,
      created_at: "",
      updated_at: "",
    });

    it("uses --session-id on the first turn", () => {
      const args = cs.buildTurnArgs(session(0), "hello", SETTINGS);
      expect(args).toContain("--session-id");
      expect(args).not.toContain("--resume");
      expect(args[args.length - 1]).toBe("hello");
    });

    it("uses --resume on later turns", () => {
      const args = cs.buildTurnArgs(session(3), "hello", SETTINGS);
      expect(args).toContain("--resume");
      expect(args).not.toContain("--session-id");
    });

    it("includes stream-json output and the permission mode", () => {
      const args = cs.buildTurnArgs(session(0), "hello", SETTINGS);
      expect(args.join(" ")).toContain("-p --output-format stream-json --verbose");
      expect(args.join(" ")).toContain("--permission-mode acceptEdits");
    });

    it("appends extra args from settings before the prompt", () => {
      const args = cs.buildTurnArgs(session(0), "hello", {
        ...SETTINGS,
        agentClaudeArgs: "--model sonnet",
      });
      const modelIdx = args.indexOf("--model");
      expect(modelIdx).toBeGreaterThan(-1);
      expect(args[modelIdx + 1]).toBe("sonnet");
      expect(args[args.length - 1]).toBe("hello");
    });
  });

  describe("startTurn", () => {
    it("creates a managed session, writes the prompt line, and spawns detached", () => {
      const session = cs.startTurn(worktree, "do the thing", SETTINGS);

      expect(session.worktree_id).toBe(worktree.id);
      expect(session.turn_count).toBe(1);
      expect(session.turn_pid).toBe(4242);
      expect(session.last_prompt).toBe("do the thing");

      const [cmd, args, opts] = spawnMock.mock.calls[0]!;
      expect(cmd).toBe("claude");
      expect(args).toContain("--session-id");
      expect(args).toContain(session.id);
      expect(opts.cwd).toBe(worktree.path);
      expect(opts.detached).toBe(true);

      const logRaw = readFileSync(cs.sessionLogPath(session.id), "utf-8");
      const first = JSON.parse(logRaw.split("\n")[0]!);
      expect(first).toMatchObject({ type: "am-user-prompt", text: "do the thing" });
    });

    it("resumes the session on the second turn", () => {
      cs.startTurn(worktree, "first", SETTINGS);
      cs.startTurn(worktree, "second", SETTINGS);

      const [, args] = spawnMock.mock.calls[1]!;
      expect(args).toContain("--resume");
      const session = db.getManagedSession(worktree.id)!;
      expect(session.turn_count).toBe(2);
    });

    it("publishes a managed-session-update message", async () => {
      const { publishMessage } = await import("../../src/lib/pubsub-client.js");
      const session = cs.startTurn(worktree, "go", SETTINGS);
      expect(publishMessage).toHaveBeenCalledWith({
        type: "managed-session-update",
        worktreeId: worktree.id,
        sessionId: session.id,
        state: "turn-started",
      });
    });

    it("throws when a turn is already running", () => {
      // Use our own pid so the liveness check (kill(pid, 0)) sees a live process
      spawnMock.mockReturnValue(fakeChild(process.pid));
      cs.startTurn(worktree, "first", SETTINGS);
      expect(() => cs.startTurn(worktree, "second", SETTINGS)).toThrow(/still working/);
    });

    it("throws when spawn fails to produce a pid", () => {
      spawnMock.mockReturnValue({ pid: undefined, unref: vi.fn(), on: vi.fn() });
      expect(() => cs.startTurn(worktree, "go", SETTINGS)).toThrow(/Failed to spawn/);
      // No turn recorded — retry is possible
      expect(db.getManagedSession(worktree.id)!.turn_count).toBe(0);
    });
  });

  describe("isTurnRunning", () => {
    it("is false with no pid and true for a live pid", () => {
      const session = cs.startTurn(worktree, "go", SETTINGS);
      // pid 4242 from the mock is (almost certainly) dead
      expect(cs.isTurnRunning(session)).toBe(false);
      expect(cs.isTurnRunning({ ...session, turn_pid: process.pid })).toBe(true);
      expect(cs.isTurnRunning({ ...session, turn_pid: null })).toBe(false);
    });
  });

  describe("parseTranscript", () => {
    it("parses am prompts, assistant text, tool use, and results", async () => {
      const { SESSIONS_DIR } = await import("../../src/lib/paths.js");
      mkdirSync(SESSIONS_DIR, { recursive: true });
      const sessionId = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
      const lines = [
        JSON.stringify({ type: "am-user-prompt", text: "fix the bug", ts: "2026-07-13T00:00:00Z" }),
        JSON.stringify({ type: "system", subtype: "init", session_id: sessionId }),
        JSON.stringify({
          type: "assistant",
          message: {
            content: [
              { type: "text", text: "Looking at the code." },
              { type: "tool_use", name: "Bash", input: { command: "git status" } },
            ],
          },
        }),
        JSON.stringify({ type: "user", message: { content: [{ type: "tool_result", content: "ok" }] } }),
        JSON.stringify({ type: "assistant", message: { content: [{ type: "text", text: "Fixed it." }] } }),
        JSON.stringify({ type: "result", subtype: "success", is_error: false, duration_ms: 12000, total_cost_usd: 0.42 }),
        "{ partial json garbage",
      ];
      appendFileSync(join(SESSIONS_DIR, `${sessionId}.jsonl`), lines.join("\n") + "\n");

      const transcript = cs.parseTranscript(sessionId);
      expect(transcript).toEqual([
        { role: "user", text: "fix the bug", ts: "2026-07-13T00:00:00Z" },
        { role: "assistant", text: "Looking at the code." },
        { role: "tool", text: "Bash: git status" },
        { role: "assistant", text: "Fixed it." },
        { role: "system", text: "turn complete · 12s · $0.42" },
      ]);
    });

    it("renders error results as errors", () => {
      const messages = cs.parseTranscriptLine(
        JSON.stringify({ type: "result", subtype: "error_during_execution", is_error: true, result: "boom" })
      );
      expect(messages).toEqual([{ role: "error", text: "boom" }]);
    });

    it("returns empty for a missing log file", () => {
      expect(cs.parseTranscript("no-such-session")).toEqual([]);
    });
  });

  describe("managed_sessions db", () => {
    it("cascades delete when the worktree is removed", () => {
      const session = cs.startTurn(worktree, "go", SETTINGS);
      expect(db.getManagedSessionById(session.id)).toBeDefined();
      db.removeWorktree(worktree.id);
      expect(db.getManagedSessionById(session.id)).toBeUndefined();
    });

    it("lists sessions and clears turn pid", () => {
      const session = cs.startTurn(worktree, "go", SETTINGS);
      expect(db.getManagedSessions()).toHaveLength(1);
      db.clearManagedSessionTurnPid(session.id);
      expect(db.getManagedSession(worktree.id)!.turn_pid).toBeNull();
      db.removeManagedSession(session.id);
      expect(db.getManagedSessions()).toHaveLength(0);
    });
  });
});
