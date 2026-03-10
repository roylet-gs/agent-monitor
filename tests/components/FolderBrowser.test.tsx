import React from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render } from "ink-testing-library";

vi.mock("fs", async () => {
  const actual = await vi.importActual<typeof import("fs")>("fs");
  return {
    ...actual,
    readdirSync: vi.fn(),
    statSync: vi.fn(),
    existsSync: vi.fn(() => true),
  };
});

vi.mock("../../src/lib/git.js", () => ({
  isGitRepo: vi.fn(() => false),
}));

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

import { readdirSync, statSync } from "fs";
import { FolderBrowser } from "../../src/components/FolderBrowser.js";

const ARROW_UP = "\u001B[A";
const ARROW_DOWN = "\u001B[B";
const ESCAPE = "\u001B";
const ENTER = "\r";

function waitForFrame(ms = 50): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

function setupFakeDirs(count: number) {
  const names = Array.from({ length: count }, (_, i) => `folder-${String(i + 1).padStart(2, "0")}`);

  vi.mocked(readdirSync).mockReturnValue(names as unknown as ReturnType<typeof readdirSync>);
  vi.mocked(statSync).mockReturnValue({
    isDirectory: () => true,
  } as ReturnType<typeof statSync>);

  return names;
}

describe("FolderBrowser", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("displays up to 15 entries from a directory with many folders", async () => {
    const names = setupFakeDirs(20);

    const { lastFrame } = render(
      <FolderBrowser onSelect={vi.fn()} onCancel={vi.fn()} />
    );
    await waitForFrame();

    const frame = lastFrame()!;
    // ".." parent entry plus first 14 folders = 15 visible
    expect(frame).toContain("..");
    for (let i = 0; i < 14; i++) {
      expect(frame).toContain(names[i]);
    }
    // The 15th folder (index 14) should NOT be visible since ".." takes one slot
    expect(frame).not.toContain(names[14]);
  });

  it('shows "... N more" when there are more than 15 entries', async () => {
    setupFakeDirs(20);

    const { lastFrame } = render(
      <FolderBrowser onSelect={vi.fn()} onCancel={vi.fn()} />
    );
    await waitForFrame();

    const frame = lastFrame()!;
    // 21 total entries (1 ".." + 20 folders), 15 visible, so 6 more
    expect(frame).toContain("6 more");
  });

  it("arrow down past the 15th item scrolls the view", async () => {
    const names = setupFakeDirs(20);

    const { stdin, lastFrame } = render(
      <FolderBrowser onSelect={vi.fn()} onCancel={vi.fn()} />
    );
    await waitForFrame();

    // Press down 15 times to move past the 15th visible item (index 0 -> 15)
    for (let i = 0; i < 15; i++) {
      stdin.write(ARROW_DOWN);
    }
    await waitForFrame();

    const frame = lastFrame()!;
    // After scrolling, the ".." entry at index 0 should no longer be visible
    // and a later folder should now be visible
    expect(frame).toContain(names[14]);
    // Should show "above" indicator since we scrolled down
    expect(frame).toContain("above ...");
  });

  it("arrow up from a scrolled position shifts the window back", async () => {
    const names = setupFakeDirs(20);

    const { stdin, lastFrame } = render(
      <FolderBrowser onSelect={vi.fn()} onCancel={vi.fn()} />
    );
    await waitForFrame();

    // Scroll down past the visible window
    for (let i = 0; i < 16; i++) {
      stdin.write(ARROW_DOWN);
    }
    await waitForFrame();

    // Verify we've scrolled (above indicator visible)
    expect(lastFrame()!).toContain("above ...");

    // Now arrow up enough to scroll the window back to the top
    for (let i = 0; i < 16; i++) {
      stdin.write(ARROW_UP);
    }
    await waitForFrame();

    const frame = lastFrame()!;
    // ".." should be visible again at the top
    expect(frame).toContain("..");
    // "above" indicator should be gone
    expect(frame).not.toContain("above ...");
  });

  it('shows "N above ..." indicator when scrolled down', async () => {
    setupFakeDirs(20);

    const { stdin, lastFrame } = render(
      <FolderBrowser onSelect={vi.fn()} onCancel={vi.fn()} />
    );
    await waitForFrame();

    // Scroll down enough to trigger the above indicator
    for (let i = 0; i < 15; i++) {
      stdin.write(ARROW_DOWN);
    }
    await waitForFrame();

    const frame = lastFrame()!;
    // scrollOffset should be 1, so "1 above ..."
    expect(frame).toMatch(/\d+ above \.\.\./);
  });

  it("calls onCancel when Escape is pressed", async () => {
    setupFakeDirs(5);

    const onCancel = vi.fn();
    const { stdin } = render(
      <FolderBrowser onSelect={vi.fn()} onCancel={onCancel} />
    );
    await waitForFrame();

    stdin.write(ESCAPE);
    expect(onCancel).toHaveBeenCalled();
  });

  it("shows no 'more' indicator when entries fit within 15", async () => {
    setupFakeDirs(10);

    const { lastFrame } = render(
      <FolderBrowser onSelect={vi.fn()} onCancel={vi.fn()} />
    );
    await waitForFrame();

    const frame = lastFrame()!;
    expect(frame).not.toContain("more");
    expect(frame).not.toContain("above");
  });
});
