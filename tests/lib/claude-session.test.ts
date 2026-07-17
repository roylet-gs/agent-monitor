import { describe, it, expect, vi, beforeEach } from "vitest";
import { appendFileSync, mkdirSync, readFileSync, writeFileSync, rmSync, utimesSync } from "fs";
import { dirname, join } from "path";
import { getTestDir } from "../setup.js";
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

// Mock homedir so claudeTranscriptPath (~/.claude/projects/...) stays in the test dir
const home = vi.hoisted(() => ({ dir: "/nonexistent" }));
vi.mock("os", async (importOriginal) => {
  const actual = await importOriginal<typeof import("os")>();
  return { ...actual, homedir: () => home.dir };
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
    home.dir = getTestDir();
    spawnMock.mockReset().mockReturnValue(fakeChild());
    cs = await import("../../src/lib/claude-session.js");
    db = await import("../../src/lib/db.js");
    const repo = db.addRepository("/tmp/am-test-repo", "test-repo");
    worktree = db.upsertWorktree(repo.id, "/tmp/am-test-repo/wt", "feature/x", "wt");
  });

  function writeClaudeTranscript(cwd: string, sessionId: string, lines: unknown[]): string {
    const file = cs.claudeTranscriptPath(cwd, sessionId);
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, lines.map((l) => JSON.stringify(l)).join("\n") + "\n");
    return file;
  }

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

  describe("external session adoption", () => {
    const EXTERNAL_ID = "dddddddd-eeee-ffff-0000-111111111111";

    it("adopts a hook-observed session whose transcript exists and resumes it", () => {
      db.upsertAgentStatus(worktree.id, "idle", EXTERNAL_ID);
      writeClaudeTranscript(worktree.path, EXTERNAL_ID, [
        { type: "user", message: { role: "user", content: "hi from terminal" } },
      ]);

      const session = cs.startTurn(worktree, "continue please", SETTINGS);
      expect(session.id).toBe(EXTERNAL_ID);

      const [, args] = spawnMock.mock.calls[0]!;
      expect(args).toContain("--resume");
      expect(args).toContain(EXTERNAL_ID);
      expect(args).not.toContain("--session-id");
    });

    it("starts fresh when the hook-observed session has no transcript on disk", () => {
      db.upsertAgentStatus(worktree.id, "idle", EXTERNAL_ID);

      const session = cs.startTurn(worktree, "go", SETTINGS);
      expect(session.id).not.toBe(EXTERNAL_ID);

      const [, args] = spawnMock.mock.calls[0]!;
      expect(args).toContain("--session-id");
    });
  });

  describe("claude project transcripts", () => {
    it("parses user, assistant, and tool entries; skips meta, sidechain, and wrappers", () => {
      const lines = [
        { type: "queue-operation", timestamp: "t" },
        { type: "user", isMeta: true, message: { role: "user", content: "injected context" } },
        { type: "user", message: { role: "user", content: "fix the bug" }, timestamp: "t1" },
        { type: "user", message: { role: "user", content: "<command-name>/clear</command-name>" } },
        { type: "user", message: { role: "user", content: [{ type: "tool_result", content: "stuff" }] } },
        {
          type: "assistant",
          message: {
            content: [
              { type: "thinking", thinking: "hmm" },
              { type: "text", text: "On it." },
              { type: "tool_use", name: "Bash", input: { command: "ls" } },
            ],
          },
          timestamp: "t2",
        },
        { type: "assistant", isSidechain: true, message: { content: [{ type: "text", text: "subagent noise" }] } },
      ];
      const messages = lines.flatMap((l) => cs.parseClaudeTranscriptLine(JSON.stringify(l)));
      expect(messages).toEqual([
        { role: "user", text: "fix the bug", ts: "t1" },
        { role: "assistant", text: "On it.", ts: "t2" },
        { role: "tool", text: "Bash: ls", ts: "t2" },
      ]);
    });

    it("loadTranscript prefers the claude project file and falls back to the am log", async () => {
      const session = cs.startTurn(worktree, "from am", SETTINGS);

      // Only the am log exists → fallback shows the am prompt
      expect(cs.loadTranscript(worktree.path, session.id).map((m) => m.text)).toContain("from am");

      // Claude project file appears → it wins
      const file = writeClaudeTranscript(worktree.path, session.id, [
        { type: "user", message: { role: "user", content: "from claude file" } },
      ]);
      const preferred = cs.loadTranscript(worktree.path, session.id).map((m) => m.text);
      expect(preferred).toContain("from claude file");
      expect(preferred).not.toContain("from am");

      // File removed → fallback again
      rmSync(file);
      expect(cs.loadTranscript(worktree.path, session.id).map((m) => m.text)).toContain("from am");
    });
  });

  describe("session discovery (worktree root + subdirectories)", () => {
    const ROOT_ID = "11111111-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    const SUBDIR_ID = "22222222-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    const SIBLING_ID = "33333333-cccc-cccc-cccc-cccccccccccc";

    beforeEach(() => {
      // Session at the worktree root
      const rootFile = writeClaudeTranscript(worktree.path, ROOT_ID, [
        { type: "user", cwd: worktree.path, message: { role: "user", content: "root prompt" } },
      ]);
      utimesSync(rootFile, new Date(2000), new Date(2000));
      // Session started in a subdirectory of the worktree
      const subFile = writeClaudeTranscript(`${worktree.path}/src/app`, SUBDIR_ID, [
        { type: "user", cwd: `${worktree.path}/src/app`, message: { role: "user", content: "subdir prompt" } },
      ]);
      utimesSync(subFile, new Date(9000), new Date(9000));
      // Sibling dir whose encoded name collides with the worktree prefix — must be excluded
      writeClaudeTranscript(`${worktree.path}.bak`, SIBLING_ID, [
        { type: "user", cwd: `${worktree.path}.bak`, message: { role: "user", content: "sibling prompt" } },
      ]);
    });

    it("discovers root and subdirectory sessions, newest first, excluding encoded-name collisions", () => {
      const sessions = cs.discoverWorktreeSessions(worktree.path);
      expect(sessions.map((s) => s.id)).toEqual([SUBDIR_ID, ROOT_ID]);
      expect(sessions[0]).toMatchObject({ cwd: `${worktree.path}/src/app`, lastPrompt: "subdir prompt" });
      expect(sessions[1]).toMatchObject({ cwd: worktree.path, lastPrompt: "root prompt" });
    });

    it("findClaudeTranscript locates a subdirectory session by id", () => {
      const file = cs.findClaudeTranscript(worktree.path, SUBDIR_ID);
      expect(file).toBe(cs.claudeTranscriptPath(`${worktree.path}/src/app`, SUBDIR_ID));
      expect(cs.findClaudeTranscript(worktree.path, "99999999-0000-0000-0000-000000000000")).toBeNull();
    });

    it("startTurn with an explicit session switches the managed session and resumes from its cwd", () => {
      // Existing managed session at the root
      cs.startTurn(worktree, "first", SETTINGS);
      spawnMock.mockClear();

      const session = cs.startTurn(worktree, "into the subdir one", SETTINGS, SUBDIR_ID);
      expect(session.id).toBe(SUBDIR_ID);
      expect(session.cwd).toBe(`${worktree.path}/src/app`);
      expect(db.getManagedSession(worktree.id)!.id).toBe(SUBDIR_ID);

      const [, args, opts] = spawnMock.mock.calls[0]!;
      expect(args).toContain("--resume");
      expect(args).toContain(SUBDIR_ID);
      expect(opts.cwd).toBe(`${worktree.path}/src/app`);
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

  describe("isSubdirectory", () => {
    it("is true for a strict subdirectory", () => {
      expect(cs.isSubdirectory("/wt/sub", "/wt")).toBe(true);
      expect(cs.isSubdirectory("/wt/a/b", "/wt")).toBe(true);
    });

    it("is false for the same directory", () => {
      expect(cs.isSubdirectory("/wt", "/wt")).toBe(false);
    });

    it("is false for a sibling with a shared prefix", () => {
      expect(cs.isSubdirectory("/wt-bak/x", "/wt")).toBe(false);
    });

    it("is false for an unrelated path", () => {
      expect(cs.isSubdirectory("/other", "/wt")).toBe(false);
    });
  });
});
