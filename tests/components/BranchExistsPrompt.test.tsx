import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { BranchExistsPrompt } from "../../src/components/BranchExistsPrompt.js";

const ESCAPE = "\u001B";
const ENTER = "\r";

function waitForFrame(ms = 150): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

describe("BranchExistsPrompt", () => {
  it("renders branch name and key hints", () => {
    const { lastFrame } = render(
      <BranchExistsPrompt
        branchName="feature/test"
        onReuse={vi.fn()}
        onDeleteAndRecreate={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain('Branch "feature/test" already exists');
    expect(frame).toContain("[Enter/y]");
    expect(frame).toContain("[d]");
    expect(frame).toContain("[Esc/n]");
  });

  it("calls onReuse when Enter is pressed", async () => {
    const onReuse = vi.fn();
    const { stdin } = render(
      <BranchExistsPrompt
        branchName="feature/test"
        onReuse={onReuse}
        onDeleteAndRecreate={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    await waitForFrame(100);
    stdin.write(ENTER);
    await waitForFrame();
    expect(onReuse).toHaveBeenCalled();
  });

  it("calls onCancel when Escape is pressed", async () => {
    const onCancel = vi.fn();
    const { stdin } = render(
      <BranchExistsPrompt
        branchName="feature/test"
        onReuse={vi.fn()}
        onDeleteAndRecreate={vi.fn()}
        onCancel={onCancel}
      />
    );
    await waitForFrame(100);
    stdin.write(ESCAPE);
    await waitForFrame();
    expect(onCancel).toHaveBeenCalled();
  });

  it("pressing d shows remote deletion prompt", async () => {
    const { stdin, lastFrame } = render(
      <BranchExistsPrompt
        branchName="feature/test"
        onReuse={vi.fn()}
        onDeleteAndRecreate={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    await waitForFrame(100);
    stdin.write("d");
    await waitForFrame();
    const frame = lastFrame()!;
    expect(frame).toContain("Also delete remote branch?");
    expect(frame).toContain("[y]");
    expect(frame).toContain("[Enter/n]");
  });

  it("pressing Enter on remote prompt calls onDeleteAndRecreate with false", async () => {
    const onDelete = vi.fn();
    const { stdin } = render(
      <BranchExistsPrompt
        branchName="feature/test"
        onReuse={vi.fn()}
        onDeleteAndRecreate={onDelete}
        onCancel={vi.fn()}
      />
    );
    await waitForFrame(100);
    stdin.write("d");
    await waitForFrame(100);
    stdin.write(ENTER);
    await waitForFrame();
    expect(onDelete).toHaveBeenCalledWith(false);
  });

  it("pressing y on remote prompt calls onDeleteAndRecreate with true", async () => {
    const onDelete = vi.fn();
    const { stdin } = render(
      <BranchExistsPrompt
        branchName="feature/test"
        onReuse={vi.fn()}
        onDeleteAndRecreate={onDelete}
        onCancel={vi.fn()}
      />
    );
    await waitForFrame(100);
    stdin.write("d");
    await waitForFrame(100);
    stdin.write("y");
    await waitForFrame();
    expect(onDelete).toHaveBeenCalledWith(true);
  });

  it("pressing Escape on remote prompt goes back to choose phase", async () => {
    const { stdin, lastFrame } = render(
      <BranchExistsPrompt
        branchName="feature/test"
        onReuse={vi.fn()}
        onDeleteAndRecreate={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    await waitForFrame(100);
    stdin.write("d");
    await waitForFrame(100);
    expect(lastFrame()!).toContain("Also delete remote branch?");
    stdin.write(ESCAPE);
    await waitForFrame(100);
    expect(lastFrame()!).toContain('Branch "feature/test" already exists');
  });

  it("ignores input during initial ready delay", () => {
    const onReuse = vi.fn();
    const onDelete = vi.fn();
    const onCancel = vi.fn();
    const { stdin } = render(
      <BranchExistsPrompt
        branchName="feature/test"
        onReuse={onReuse}
        onDeleteAndRecreate={onDelete}
        onCancel={onCancel}
      />
    );
    // Fire immediately without waiting for ready delay
    stdin.write(ENTER);
    stdin.write("d");
    stdin.write(ESCAPE);
    expect(onReuse).not.toHaveBeenCalled();
    expect(onDelete).not.toHaveBeenCalled();
    expect(onCancel).not.toHaveBeenCalled();
  });
});
