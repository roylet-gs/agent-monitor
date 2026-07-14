import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { SessionPicker } from "../../src/components/SessionPicker.js";
import type { DiscoveredSession } from "../../src/lib/claude-session.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

const SESSIONS: DiscoveredSession[] = [
  {
    id: "aaaaaaaa-1111-2222-3333-444444444444",
    cwd: "/repo/wt/src/app",
    file: "/x/a.jsonl",
    mtimeMs: Date.now() - 60_000,
    lastPrompt: "fix the login bug",
  },
  {
    id: "bbbbbbbb-1111-2222-3333-444444444444",
    cwd: "/repo/wt",
    file: "/x/b.jsonl",
    mtimeMs: Date.now() - 3_600_000,
    lastPrompt: "write the README",
  },
];

function waitForFrame(ms = 50): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

describe("SessionPicker", () => {
  it("lists sessions with start dir, age, prompt, and active marker", () => {
    const { lastFrame } = render(
      <SessionPicker
        worktreeName="feature/x"
        worktreePath="/repo/wt"
        sessions={SESSIONS}
        activeSessionId={SESSIONS[1]!.id}
        onSelect={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Claude Sessions — feature/x");
    expect(frame).toContain("aaaaaaaa");
    expect(frame).toContain("src/app");
    expect(frame).toContain("fix the login bug");
    expect(frame).toContain("(active)");
  });

  it("navigates and selects a session with Enter", async () => {
    const onSelect = vi.fn();
    const { stdin } = render(
      <SessionPicker
        worktreeName="feature/x"
        worktreePath="/repo/wt"
        sessions={SESSIONS}
        activeSessionId={null}
        onSelect={onSelect}
        onCancel={vi.fn()}
      />
    );
    await waitForFrame();
    stdin.write("j");
    await waitForFrame();
    stdin.write("\r");
    expect(onSelect).toHaveBeenCalledWith(SESSIONS[1]);
  });

  it("cancels on Esc", async () => {
    const onCancel = vi.fn();
    const { stdin } = render(
      <SessionPicker
        worktreeName="feature/x"
        worktreePath="/repo/wt"
        sessions={SESSIONS}
        activeSessionId={null}
        onSelect={vi.fn()}
        onCancel={onCancel}
      />
    );
    await waitForFrame();
    stdin.write("\u001B");
    expect(onCancel).toHaveBeenCalled();
  });
});
