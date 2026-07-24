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

describe("NewWorktreeForm live branch check", () => {
  const ENTER = "\r";

  function renderForm(overrides: Partial<React.ComponentProps<typeof NewWorktreeForm>> = {}) {
    const onSubmit = vi.fn();
    const utils = render(
      <NewWorktreeForm
        defaultPrefix="feature/"
        defaultBaseBranch="main"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        checkDebounceMs={10}
        {...overrides}
      />
    );
    return { ...utils, onSubmit };
  }

  it("shows checking then the remote indicator after typing", async () => {
    const checkBranch = vi.fn().mockResolvedValue({ local: false, remote: true });
    const { stdin, lastFrame } = renderForm({ checkBranch });
    await waitForFrame();
    stdin.write("x");
    await waitForFrame(5);
    expect(lastFrame()).toContain("checking origin");
    await waitForFrame(60);
    expect(checkBranch).toHaveBeenCalledWith("feature/x");
    expect(lastFrame()).toContain("✓ exists on origin");
    expect(lastFrame()).toContain("Enter will offer to pull it");
  });

  it("mentions the local branch when it exists on both", async () => {
    const checkBranch = vi.fn().mockResolvedValue({ local: true, remote: true });
    const { stdin, lastFrame } = renderForm({ checkBranch });
    await waitForFrame();
    stdin.write("x");
    await waitForFrame(60);
    expect(lastFrame()).toContain("✓ exists on origin and locally");
  });

  it("debounces rapid keystrokes into one check", async () => {
    const checkBranch = vi.fn().mockResolvedValue({ local: false, remote: true });
    const { stdin } = renderForm({ checkBranch, checkDebounceMs: 30 });
    await waitForFrame();
    stdin.write("a");
    await waitForFrame(5);
    stdin.write("b");
    await waitForFrame(5);
    stdin.write("c");
    await waitForFrame(80);
    expect(checkBranch).toHaveBeenCalledTimes(1);
    expect(checkBranch).toHaveBeenCalledWith("feature/abc");
  });

  it("drops a stale result that lands after a newer keystroke", async () => {
    let resolveFirst!: (r: { local: boolean; remote: boolean }) => void;
    const checkBranch = vi
      .fn()
      .mockImplementationOnce(
        () => new Promise((r) => { resolveFirst = r; })
      )
      .mockResolvedValue({ local: false, remote: false });
    const { stdin, lastFrame } = renderForm({ checkBranch });
    await waitForFrame();
    stdin.write("a");
    await waitForFrame(30); // first check fired, still pending
    stdin.write("b");
    await waitForFrame(30); // second check fired and resolved (no match)
    resolveFirst({ local: false, remote: true }); // stale result arrives late
    await waitForFrame(20);
    expect(lastFrame()).not.toContain("✓ exists on origin");
  });

  it("skips the base-branch field when the branch exists on origin", async () => {
    const checkBranch = vi.fn().mockResolvedValue({ local: false, remote: true });
    const { stdin, lastFrame, onSubmit } = renderForm({ checkBranch });
    await waitForFrame();
    stdin.write("x");
    await waitForFrame(60);
    expect(lastFrame()).toContain("(ignored — will track origin/feature/x)");
    stdin.write(ENTER); // branch -> name
    await waitForFrame();
    stdin.write(ENTER); // name -> submit (base skipped)
    await waitForFrame();
    expect(onSubmit).toHaveBeenCalledWith("feature/x", "", "main");
  });

  it("does not skip the base field when the branch exists locally only", async () => {
    const checkBranch = vi.fn().mockResolvedValue({ local: true, remote: false });
    const { stdin, lastFrame, onSubmit } = renderForm({ checkBranch });
    await waitForFrame();
    stdin.write("x");
    await waitForFrame(60);
    expect(lastFrame()).toContain("exists locally only");
    stdin.write(ENTER); // branch -> name
    await waitForFrame();
    stdin.write(ENTER); // name -> base
    await waitForFrame();
    expect(onSubmit).not.toHaveBeenCalled();
    stdin.write(ENTER); // base -> submit
    await waitForFrame();
    expect(onSubmit).toHaveBeenCalledWith("feature/x", "", "main");
  });

  it("follows the normal flow while the check is still in flight", async () => {
    const checkBranch = vi.fn().mockImplementation(() => new Promise(() => {}));
    const { stdin, onSubmit } = renderForm({ checkBranch });
    await waitForFrame();
    stdin.write("x");
    await waitForFrame();
    stdin.write(ENTER);
    await waitForFrame();
    stdin.write(ENTER);
    await waitForFrame();
    stdin.write(ENTER);
    await waitForFrame();
    expect(onSubmit).toHaveBeenCalledWith("feature/x", "", "main");
  });

  it("clears the indicator when the input is emptied back to the prefix", async () => {
    const checkBranch = vi.fn().mockResolvedValue({ local: false, remote: true });
    const { stdin, lastFrame } = renderForm({ checkBranch });
    await waitForFrame();
    stdin.write("x");
    await waitForFrame(60);
    expect(lastFrame()).toContain("✓ exists on origin");
    stdin.write("\u007F"); // backspace (DEL) -> "feature/"
    await waitForFrame(60);
    expect(lastFrame()).not.toContain("✓ exists on origin");
    expect(lastFrame()).not.toContain("checking origin");
  });

  it("stays silent and submits normally when checkBranch rejects", async () => {
    const checkBranch = vi.fn().mockRejectedValue(new Error("network down"));
    const { stdin, lastFrame, onSubmit } = renderForm({ checkBranch });
    await waitForFrame();
    stdin.write("x");
    await waitForFrame(60);
    expect(lastFrame()).not.toContain("✓ exists on origin");
    stdin.write(ENTER);
    await waitForFrame();
    stdin.write(ENTER);
    await waitForFrame();
    stdin.write(ENTER);
    await waitForFrame();
    expect(onSubmit).toHaveBeenCalledWith("feature/x", "", "main");
  });

  it("shows no indicator and never skips fields without checkBranch", async () => {
    const { stdin, lastFrame, onSubmit } = renderForm();
    await waitForFrame();
    stdin.write("x");
    await waitForFrame(60);
    expect(lastFrame()).not.toContain("checking origin");
    stdin.write(ENTER);
    await waitForFrame();
    stdin.write(ENTER);
    await waitForFrame();
    expect(onSubmit).not.toHaveBeenCalled(); // landed on base field
    stdin.write(ENTER);
    await waitForFrame();
    expect(onSubmit).toHaveBeenCalledWith("feature/x", "", "main");
  });
});
