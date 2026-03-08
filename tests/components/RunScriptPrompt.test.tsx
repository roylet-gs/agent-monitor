import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { RunScriptPrompt } from "../../src/components/RunScriptPrompt.js";

const ESCAPE = "\u001B";
const ENTER = "\r";

function waitForFrame(ms = 80): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

describe("RunScriptPrompt", () => {
  it("renders script path and key hints", () => {
    const { lastFrame } = render(
      <RunScriptPrompt
        scriptPath="/tmp/scripts/repo-1.sh"
        onRun={vi.fn()}
        onSkip={vi.fn()}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Run startup script?");
    expect(frame).toContain("/tmp/scripts/repo-1.sh");
    expect(frame).toContain("[Enter/y]");
    expect(frame).toContain("[Esc/n]");
  });

  it("calls onRun when Enter is pressed after ready delay", async () => {
    const onRun = vi.fn();
    const onSkip = vi.fn();
    const { stdin } = render(
      <RunScriptPrompt scriptPath="/tmp/s.sh" onRun={onRun} onSkip={onSkip} />
    );

    // Should ignore input before ready
    stdin.write(ENTER);
    expect(onRun).not.toHaveBeenCalled();

    await waitForFrame();
    stdin.write(ENTER);
    expect(onRun).toHaveBeenCalledTimes(1);
    expect(onSkip).not.toHaveBeenCalled();
  });

  it("calls onRun when y is pressed", async () => {
    const onRun = vi.fn();
    const { stdin } = render(
      <RunScriptPrompt scriptPath="/tmp/s.sh" onRun={onRun} onSkip={vi.fn()} />
    );
    await waitForFrame();
    stdin.write("y");
    expect(onRun).toHaveBeenCalledTimes(1);
  });

  it("calls onSkip when Escape is pressed", async () => {
    const onSkip = vi.fn();
    const { stdin } = render(
      <RunScriptPrompt scriptPath="/tmp/s.sh" onRun={vi.fn()} onSkip={onSkip} />
    );
    await waitForFrame();
    stdin.write(ESCAPE);
    expect(onSkip).toHaveBeenCalledTimes(1);
  });

  it("calls onSkip when n is pressed", async () => {
    const onSkip = vi.fn();
    const { stdin } = render(
      <RunScriptPrompt scriptPath="/tmp/s.sh" onRun={vi.fn()} onSkip={onSkip} />
    );
    await waitForFrame();
    stdin.write("n");
    expect(onSkip).toHaveBeenCalledTimes(1);
  });

  it("ignores input during ready delay", async () => {
    const onRun = vi.fn();
    const onSkip = vi.fn();
    const { stdin } = render(
      <RunScriptPrompt scriptPath="/tmp/s.sh" onRun={onRun} onSkip={onSkip} />
    );

    stdin.write("y");
    stdin.write("n");
    stdin.write(ENTER);
    stdin.write(ESCAPE);

    expect(onRun).not.toHaveBeenCalled();
    expect(onSkip).not.toHaveBeenCalled();
  });
});
