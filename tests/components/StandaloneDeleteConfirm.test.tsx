import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { StandaloneDeleteConfirm } from "../../src/components/StandaloneDeleteConfirm.js";
import type { StandaloneSession } from "../../src/lib/types.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

function makeSession(overrides: Partial<StandaloneSession> = {}): StandaloneSession {
  return {
    id: "sess-1",
    path: "/Users/dev/my-project",
    status: "idle",
    session_id: null,
    last_response: null,
    transcript_summary: null,
    is_open: 1,
    created_at: "2024-01-01",
    updated_at: "2024-01-01",
    ...overrides,
  };
}

const ESCAPE = "\u001B";
const ENTER = "\r";

function waitForFrame(ms = 50): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

describe("StandaloneDeleteConfirm", () => {
  it("renders with session path", () => {
    const { lastFrame } = render(
      <StandaloneDeleteConfirm
        session={makeSession()}
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("/Users/dev/my-project");
    expect(frame).toContain("Remove this session from the dashboard");
  });

  it("shows active warning for running session", () => {
    const { lastFrame } = render(
      <StandaloneDeleteConfirm
        session={makeSession({ status: "executing" })}
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("actively running");
    expect(frame).toContain("executing");
    expect(frame).toContain("terminated");
  });

  it("shows active warning for planning status", () => {
    const { lastFrame } = render(
      <StandaloneDeleteConfirm
        session={makeSession({ status: "planning" })}
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    expect(lastFrame()!).toContain("actively running");
  });

  it("does not show active warning for idle session", () => {
    const { lastFrame } = render(
      <StandaloneDeleteConfirm
        session={makeSession({ status: "idle" })}
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    expect(lastFrame()!).not.toContain("actively running");
  });

  it("does not show active warning for done session", () => {
    const { lastFrame } = render(
      <StandaloneDeleteConfirm
        session={makeSession({ status: "done" })}
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    expect(lastFrame()!).not.toContain("actively running");
  });

  it("calls onConfirm on Enter", async () => {
    const onConfirm = vi.fn();
    const { stdin } = render(
      <StandaloneDeleteConfirm
        session={makeSession()}
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />
    );
    await waitForFrame();
    stdin.write(ENTER);
    expect(onConfirm).toHaveBeenCalled();
  });

  it("calls onConfirm on y", async () => {
    const onConfirm = vi.fn();
    const { stdin } = render(
      <StandaloneDeleteConfirm
        session={makeSession()}
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />
    );
    await waitForFrame();
    stdin.write("y");
    expect(onConfirm).toHaveBeenCalled();
  });

  it("calls onCancel on Escape", async () => {
    const onCancel = vi.fn();
    const { stdin } = render(
      <StandaloneDeleteConfirm
        session={makeSession()}
        onConfirm={vi.fn()}
        onCancel={onCancel}
      />
    );
    await waitForFrame();
    stdin.write(ESCAPE);
    expect(onCancel).toHaveBeenCalled();
  });

  it("calls onCancel on n", async () => {
    const onCancel = vi.fn();
    const { stdin } = render(
      <StandaloneDeleteConfirm
        session={makeSession()}
        onConfirm={vi.fn()}
        onCancel={onCancel}
      />
    );
    await waitForFrame();
    stdin.write("n");
    expect(onCancel).toHaveBeenCalled();
  });
});
