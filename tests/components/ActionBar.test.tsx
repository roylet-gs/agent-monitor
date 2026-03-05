import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { ActionBar } from "../../src/components/ActionBar.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

describe("ActionBar", () => {
  it("shows action keys when worktrees exist", () => {
    const { lastFrame } = render(
      <ActionBar busy={null} hasWorktrees={true} escHint={false} />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Open");
    expect(frame).toContain("ew");
    expect(frame).toContain("elete");
    expect(frame).toContain("ettings");
    expect(frame).toContain("uit");
  });

  it("shows fewer actions when no worktrees", () => {
    const { lastFrame } = render(
      <ActionBar busy={null} hasWorktrees={false} escHint={false} />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("ew");
    expect(frame).toContain("ettings");
    expect(frame).not.toContain("Open");
    expect(frame).not.toContain("elete");
  });

  it("shows busy spinner", () => {
    const { lastFrame } = render(
      <ActionBar busy="Syncing worktrees..." hasWorktrees={true} escHint={false} />
    );
    expect(lastFrame()!).toContain("Syncing worktrees");
  });

  it("shows escape hint", () => {
    const { lastFrame } = render(
      <ActionBar busy={null} hasWorktrees={true} escHint={true} />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Esc");
    expect(frame).toContain("again to quit");
  });

  it("shows github shortcut when ghPrStatus is enabled", () => {
    const { lastFrame } = render(
      <ActionBar busy={null} hasWorktrees={true} escHint={false} ghPrStatus={true} />
    );
    expect(lastFrame()!).toContain("ithub");
  });

  it("shows linear shortcut when enabled", () => {
    const { lastFrame } = render(
      <ActionBar busy={null} hasWorktrees={true} escHint={false} linearEnabled={true} />
    );
    expect(lastFrame()!).toContain("inear");
  });
});
