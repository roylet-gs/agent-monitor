import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { NewWorktreeForm } from "../../src/components/NewWorktreeForm.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

const ESCAPE = "\u001B";

function waitForFrame(ms = 50): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

describe("NewWorktreeForm", () => {
  it("renders form with default values", () => {
    const { lastFrame } = render(
      <NewWorktreeForm
        defaultPrefix="feature/"
        defaultBaseBranch="main"
        onSubmit={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("New Worktree");
    expect(frame).toContain("Branch name");
    expect(frame).toContain("main");
  });

  it("shows navigation hints", () => {
    const { lastFrame } = render(
      <NewWorktreeForm
        defaultPrefix="feature/"
        defaultBaseBranch="main"
        onSubmit={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Tab");
    expect(frame).toContain("Create");
    expect(frame).toContain("Cancel");
  });

  it("calls onCancel when Esc is pressed", async () => {
    const onCancel = vi.fn();
    const { stdin } = render(
      <NewWorktreeForm
        defaultPrefix="feature/"
        defaultBaseBranch="main"
        onSubmit={vi.fn()}
        onCancel={onCancel}
      />
    );
    await waitForFrame();
    stdin.write(ESCAPE);
    expect(onCancel).toHaveBeenCalled();
  });
});
