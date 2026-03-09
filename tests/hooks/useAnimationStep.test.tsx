import React from "react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render } from "ink-testing-library";
import { Text } from "ink";
import { useAnimationStep, _resetAnimationTimer } from "../../src/hooks/useAnimationStep.js";

function StepDisplay() {
  const step = useAnimationStep();
  return <Text>step:{step}</Text>;
}

describe("useAnimationStep", () => {
  beforeEach(() => {
    _resetAnimationTimer();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    _resetAnimationTimer();
  });

  it("starts at step 0", () => {
    const { lastFrame } = render(<StepDisplay />);
    expect(lastFrame()!).toContain("step:0");
  });

  it("advances step on interval tick", async () => {
    const { lastFrame } = render(<StepDisplay />);
    await vi.advanceTimersByTimeAsync(200);
    expect(lastFrame()!).toContain("step:1");
  });

  it("wraps around after full cycle (12 steps)", async () => {
    const { lastFrame } = render(<StepDisplay />);
    await vi.advanceTimersByTimeAsync(12 * 200);
    expect(lastFrame()!).toContain("step:0");
  });

  it("multiple subscribers share the same step value", async () => {
    function TwoDots() {
      const step1 = useAnimationStep();
      const step2 = useAnimationStep();
      return <Text>a:{step1} b:{step2}</Text>;
    }
    const { lastFrame } = render(<TwoDots />);
    await vi.advanceTimersByTimeAsync(600); // 3 ticks
    expect(lastFrame()!).toContain("a:3 b:3");
  });

  it("cleans up timer when all subscribers unmount", () => {
    const { unmount } = render(<StepDisplay />);
    unmount();
    // After unmount, advancing timers should not throw
    vi.advanceTimersByTime(1000);
  });
});
