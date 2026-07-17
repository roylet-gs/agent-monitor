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

  it("shows the Sorting & Display section with the Sort Order row", () => {
    const { lastFrame } = render(<SettingsPanel {...defaultProps} />);
    const frame = lastFrame()!;
    expect(frame).toContain("Sorting & Display");
    expect(frame).toContain("Sort Order:");
  });

  it("shows the Resume Last Session toggle, on by default", () => {
    const { lastFrame } = render(<SettingsPanel {...defaultProps} />);
    const frame = lastFrame()!;
    expect(frame).toContain("Resume Last Session");
    // default is on → rendered as a checked box
    expect(DEFAULT_SETTINGS.resumeLastSession).toBe(true);
    expect(frame).toContain("Resume Last Session: [✓]");
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

  it("opens the full-page sort editor and reorders via grab-to-move", async () => {
    const onSave = vi.fn();
    const { stdin, lastFrame } = render(<SettingsPanel {...defaultProps} onSave={onSave} />);
    await waitForFrame();

    // Navigate down to the Sort Order field. The two audio-sound fields are
    // skipped while audio notifications are off, so this takes 12 presses.
    const DOWN = ESCAPE + "[B";
    for (let i = 0; i < 12; i++) {
      stdin.write(DOWN);
      await waitForFrame(10);
    }
    expect(lastFrame()!).toContain("▸ Sort Order");

    // Enter opens the dedicated editor page: criteria list + example preview.
    stdin.write("\r");
    await waitForFrame();
    const editorFrame = lastFrame()!;
    expect(editorFrame).toContain("Edit Sort Order");
    expect(editorFrame).toContain("Dedicated vs main");
    expect(editorFrame).toContain("Example");
    expect(editorFrame).toContain("Grab to move");

    // Grab the first criterion (isMain), move it down, drop it.
    stdin.write("\r"); // grab
    await waitForFrame();
    stdin.write(DOWN); // move item down
    await waitForFrame();
    stdin.write("\r"); // drop
    await waitForFrame();

    stdin.write(ESCAPE); // back to Settings
    await waitForFrame();
    stdin.write(ESCAPE); // save & close
    await waitForFrame();

    expect(onSave).toHaveBeenCalled();
    const saved = onSave.mock.calls[0][0];
    // isMain moved below linearTicket
    expect(saved.worktreeSort[0].key).toBe("linearTicket");
    expect(saved.worktreeSort[1].key).toBe("isMain");
  });

});
