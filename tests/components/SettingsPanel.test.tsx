import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { SettingsPanel } from "../../src/components/SettingsPanel.js";
import { DEFAULT_SETTINGS } from "../../src/lib/settings.js";
import type { Repository } from "../../src/lib/types.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/linear.js", () => ({
  verifyLinearApiKey: vi.fn().mockResolvedValue({ ok: true }),
  getLinearStatusColor: vi.fn(() => "cyan"),
}));

vi.mock("../../src/lib/scripts.js", () => ({
  hasStartupScript: vi.fn(() => false),
  openScriptInEditor: vi.fn(),
  removeStartupScript: vi.fn(),
}));

vi.mock("../../src/lib/rules.js", () => ({
  loadRules: vi.fn(() => []),
  removeRule: vi.fn(),
  clearLearnedRules: vi.fn(() => ({ removed: 0 })),
  applyRulesToClaudeSettings: vi.fn(() => ({ added: 0, total: 0 })),
  removeAmPermissionsFromClaudeSettings: vi.fn(),
}));

const ESCAPE = "\u001B";

function waitForFrame(ms = 50): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

const repo: Repository = { id: "r1", path: "/tmp/repo", name: "my-repo", last_used_at: "2024-01-01" };

describe("SettingsPanel", () => {
  const defaultProps = {
    settings: DEFAULT_SETTINGS,
    repositories: [repo],
    onSave: vi.fn(),
    onClose: vi.fn(),
    onAddRepo: vi.fn(),
    onRemoveRepo: vi.fn(),
    onSettingsReset: vi.fn(),
    onFactoryReset: vi.fn(),
    onCheckForUpdates: vi.fn().mockResolvedValue(null),
  };

  it("renders settings panel with title", () => {
    const { lastFrame } = render(<SettingsPanel {...defaultProps} />);
    expect(lastFrame()!).toContain("Settings");
  });

  it("shows current IDE setting", () => {
    const { lastFrame } = render(<SettingsPanel {...defaultProps} />);
    expect(lastFrame()!).toContain("cursor");
  });

  it("shows repositories section", () => {
    const { lastFrame } = render(<SettingsPanel {...defaultProps} />);
    expect(lastFrame()!).toContain("my-repo");
  });

  it("shows navigation hints", () => {
    const { lastFrame } = render(<SettingsPanel {...defaultProps} />);
    const frame = lastFrame()!;
    expect(frame).toContain("Navigate");
    expect(frame).toContain("Save & Close");
  });

  it("calls onSave and onClose when Esc is pressed", async () => {
    const onSave = vi.fn();
    const onClose = vi.fn();
    const { stdin } = render(
      <SettingsPanel {...defaultProps} onSave={onSave} onClose={onClose} />
    );
    await waitForFrame();
    stdin.write(ESCAPE);
    expect(onSave).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("shows danger zone options", () => {
    const { lastFrame } = render(<SettingsPanel {...defaultProps} />);
    const frame = lastFrame()!;
    expect(frame).toContain("Reset Settings");
    expect(frame).toContain("Factory Reset");
  });
});
