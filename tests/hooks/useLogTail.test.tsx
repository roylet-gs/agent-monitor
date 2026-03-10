import React, { useRef } from "react";
import { Text } from "ink";
import { render } from "ink-testing-library";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("fs", () => ({
  existsSync: vi.fn(),
  readFileSync: vi.fn(),
}));

vi.mock("../../src/lib/paths.js", () => ({
  LOG_PATH: "/tmp/test.log",
}));

import { existsSync, readFileSync } from "fs";
import { useLogTail } from "../../src/hooks/useLogTail.js";

const mockExistsSync = existsSync as ReturnType<typeof vi.fn>;
const mockReadFileSync = readFileSync as ReturnType<typeof vi.fn>;

function LogTailHarness({ enabled, maxLines }: { enabled: boolean; maxLines: number }) {
  const lines = useLogTail(enabled, maxLines);
  const renderCountRef = useRef(0);
  renderCountRef.current++;
  return <Text>{`renders:${renderCountRef.current}|${lines.join(",")}`}</Text>;
}

async function flush() {
  // Let React process effects
  await new Promise((r) => setTimeout(r, 0));
}

describe("useLogTail", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockExistsSync.mockReturnValue(true);
    mockReadFileSync.mockReturnValue("line1\nline2\nline3\n");
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("returns log lines on initial poll", async () => {
    const { lastFrame } = render(<LogTailHarness enabled={true} maxLines={10} />);
    await vi.advanceTimersByTimeAsync(0);
    expect(lastFrame()!).toContain("line1,line2,line3");
  });

  it("does not re-render when content has not changed", async () => {
    const { lastFrame } = render(<LogTailHarness enabled={true} maxLines={10} />);
    await vi.advanceTimersByTimeAsync(0);
    expect(lastFrame()!).toContain("line1,line2,line3");

    const frameBeforePoll = lastFrame();
    await vi.advanceTimersByTimeAsync(500);
    // Frame should be identical — no re-render triggered
    expect(lastFrame()).toBe(frameBeforePoll);
  });

  it("re-renders when content changes", async () => {
    const { lastFrame } = render(<LogTailHarness enabled={true} maxLines={10} />);
    await vi.advanceTimersByTimeAsync(0);
    expect(lastFrame()!).toContain("line1,line2,line3");

    mockReadFileSync.mockReturnValue("line1\nline2\nline3\nline4\n");
    await vi.advanceTimersByTimeAsync(2000);

    expect(lastFrame()!).toContain("line1,line2,line3,line4");
  });

  it("returns empty when disabled", async () => {
    const { lastFrame } = render(<LogTailHarness enabled={false} maxLines={10} />);
    await vi.advanceTimersByTimeAsync(0);
    expect(lastFrame()!).not.toContain("line1");
  });

  it("returns empty when log file does not exist", async () => {
    mockExistsSync.mockReturnValue(false);
    const { lastFrame } = render(<LogTailHarness enabled={true} maxLines={10} />);
    await vi.advanceTimersByTimeAsync(0);
    expect(lastFrame()!).not.toContain("line1");
  });
});
