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
  openFileInEditor: vi.fn(),
  removeStartupScript: vi.fn(),
}));

const mockRules = vi.fn(() => [] as import("../../src/lib/types.js").Rule[]);
vi.mock("../../src/lib/rules.js", () => ({
  loadRules: (...args: unknown[]) => mockRules(...args),
  removeRule: vi.fn(),
  clearAllRules: vi.fn(() => ({ removed: 0 })),
  clearLearnedRules: vi.fn(() => ({ removed: 0 })),
  syncLearnedRules: vi.fn(() => ({ added: 0 })),
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

  it("shows Open settings.json as the first field", () => {
    const { lastFrame } = render(<SettingsPanel {...defaultProps} />);
    const frame = lastFrame()!;
    expect(frame).toContain("Open settings.json");
    // It should be selected by default (first field)
    expect(frame).toContain("▸ Open settings.json");
  });

  it("calls openFileInEditor when Enter is pressed on Open settings.json", async () => {
    const { openFileInEditor } = await import("../../src/lib/scripts.js");
    const { stdin } = render(<SettingsPanel {...defaultProps} />);
    await waitForFrame();
    // First field is openSettingsJson, press Enter
    stdin.write("\r");
    expect(openFileInEditor).toHaveBeenCalled();
  });

  describe("Manage Rules view", () => {
    const DOWN = "\u001B[B";
    const ENTER = "\r";
    // manageRules is field index 21 in FIELDS array
    const MANAGE_RULES_INDEX = 21;

    async function navigateToManageRules(stdin: { write: (s: string) => void }) {
      for (let i = 0; i < MANAGE_RULES_INDEX; i++) {
        stdin.write(DOWN);
      }
      await waitForFrame(100);
    }

    it("opens manage rules view and shows empty state", async () => {
      mockRules.mockReturnValue([]);
      const { stdin, lastFrame } = render(<SettingsPanel {...defaultProps} />);
      await waitForFrame();
      await navigateToManageRules(stdin);
      // Verify we're on Manage Rules
      expect(lastFrame()!).toContain("▸ Manage Rules");
      // Open it
      stdin.write(ENTER);
      await waitForFrame();
      const frame = lastFrame()!;
      expect(frame).toContain("Manage Rules");
      expect(frame).toContain("No rules");
    });

    it("shows rules with truncated long patterns", async () => {
      mockRules.mockReturnValue([
        { id: "r1", tool: "Bash", input_pattern: "git status*", decision: "allow", source: "learned", created_at: "" },
        { id: "r2", tool: "Bash", input_pattern: "/very/long/path/to/some/deeply/nested/file/that/should/be/truncated/and/not/overflow.ts", decision: "allow", source: "learned", created_at: "" },
        { id: "r3", tool: "Read", decision: "allow", source: "manual", created_at: "" },
      ]);
      const { stdin, lastFrame } = render(<SettingsPanel {...defaultProps} />);
      await waitForFrame();
      await navigateToManageRules(stdin);
      stdin.write(ENTER);
      await waitForFrame();
      const frame = lastFrame()!;
      // Should show rules count
      expect(frame).toContain("3 rules");
      // Short rules should appear normally
      expect(frame).toContain("Bash(git status*)");
      expect(frame).toContain("Read");
      // Long rule should be truncated (contains ellipsis)
      expect(frame).toContain("…");
      // Should show [learned] tag
      expect(frame).toContain("[learned]");
      // Should NOT show [learned] for manual rules
      // The "Read" rule is manual, so just "Read" without [learned]
      expect(frame).toContain("allow  Read");
    });

    it("navigates rules with arrow keys", async () => {
      mockRules.mockReturnValue([
        { id: "r1", tool: "Bash", input_pattern: "git status*", decision: "allow", source: "learned", created_at: "" },
        { id: "r2", tool: "Read", decision: "allow", source: "manual", created_at: "" },
        { id: "r3", tool: "Write", decision: "deny", source: "learned", created_at: "" },
      ]);
      const { stdin, lastFrame } = render(<SettingsPanel {...defaultProps} />);
      await waitForFrame();
      await navigateToManageRules(stdin);
      stdin.write(ENTER);
      await waitForFrame();
      // First rule should be selected
      expect(lastFrame()!).toContain("▸ allow  Bash(git status*)");
      // Move down
      stdin.write(DOWN);
      await waitForFrame();
      expect(lastFrame()!).toContain("▸ allow  Read");
      // Move down again
      stdin.write(DOWN);
      await waitForFrame();
      expect(lastFrame()!).toContain("▸ deny  Write");
    });

    it("removes a rule with d key", async () => {
      const initialRules = [
        { id: "r1", tool: "Bash", input_pattern: "git status*", decision: "allow" as const, source: "learned" as const, created_at: "" },
        { id: "r2", tool: "Read", decision: "allow" as const, source: "manual" as const, created_at: "" },
      ];
      mockRules.mockReturnValue([...initialRules]);
      const { removeRule } = await import("../../src/lib/rules.js");

      const { stdin, lastFrame } = render(<SettingsPanel {...defaultProps} />);
      await waitForFrame();
      await navigateToManageRules(stdin);
      stdin.write(ENTER);
      await waitForFrame();
      expect(lastFrame()!).toContain("2 rules");

      // After removing, loadRules returns updated list
      mockRules.mockReturnValue([initialRules[1]!]);
      stdin.write("d");
      await waitForFrame();
      expect(removeRule).toHaveBeenCalledWith("r1");
    });

    it("returns to settings with Esc", async () => {
      mockRules.mockReturnValue([
        { id: "r1", tool: "Bash", decision: "allow", source: "manual", created_at: "" },
      ]);
      const { stdin, lastFrame } = render(<SettingsPanel {...defaultProps} />);
      await waitForFrame();
      await navigateToManageRules(stdin);
      stdin.write(ENTER);
      await waitForFrame();
      expect(lastFrame()!).toContain("Manage Rules");
      expect(lastFrame()!).toContain("1 rule");
      // Press Esc to go back
      stdin.write(ESCAPE);
      await waitForFrame();
      // Should be back to settings
      expect(lastFrame()!).toContain("Settings");
      expect(lastFrame()!).toContain("IDE / Editor");
    });
  });
});
