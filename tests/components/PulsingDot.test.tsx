import React from "react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render } from "ink-testing-library";
import { PulsingDot } from "../../src/components/PulsingDot.js";

describe("PulsingDot", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders the dot character", () => {
    const { lastFrame } = render(<PulsingDot color="green" />);
    expect(lastFrame()!).toContain("●");
  });

  it("continues rendering dot after interval ticks", () => {
    const { lastFrame } = render(<PulsingDot color="green" />);
    vi.advanceTimersByTime(500);
    expect(lastFrame()!).toContain("●");
  });

  it("completes a full pulse cycle without errors", () => {
    const { lastFrame } = render(<PulsingDot color="green" />);
    // Advance through a full cycle (24 steps * 100ms)
    vi.advanceTimersByTime(2400);
    expect(lastFrame()!).toContain("●");
  });
});
