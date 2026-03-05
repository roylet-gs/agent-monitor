import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { SetupWizard } from "../../src/components/SetupWizard.js";
import { DEFAULT_SETTINGS } from "../../src/lib/settings.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/github.js", () => ({
  isGhAvailable: vi.fn(() => true),
}));

vi.mock("../../src/lib/linear.js", () => ({
  fetchLinearInfo: vi.fn().mockResolvedValue(null),
  verifyLinearApiKey: vi.fn().mockResolvedValue({ ok: true, name: "Test" }),
  getLinearStatusColor: vi.fn(() => "cyan"),
}));

vi.mock("../../src/lib/hooks-installer.js", () => ({
  installGlobalHooks: vi.fn(),
  isGlobalHooksInstalled: vi.fn(() => false),
}));

const ESCAPE = "\u001B";
const ENTER = "\r";

function waitForFrame(ms = 50): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

describe("SetupWizard", () => {
  it("renders welcome screen", () => {
    const { lastFrame } = render(
      <SetupWizard
        initialSettings={DEFAULT_SETTINGS}
        onComplete={vi.fn()}
        onSkip={vi.fn()}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Welcome to Agent Monitor");
    expect(frame).toContain("Start setup");
  });

  it("calls onSkip when Esc is pressed", async () => {
    const onSkip = vi.fn();
    const { stdin } = render(
      <SetupWizard
        initialSettings={DEFAULT_SETTINGS}
        onComplete={vi.fn()}
        onSkip={onSkip}
      />
    );
    await waitForFrame();
    stdin.write(ESCAPE);
    expect(onSkip).toHaveBeenCalled();
  });

  it("advances to IDE step on Enter", async () => {
    const { stdin, lastFrame } = render(
      <SetupWizard
        initialSettings={DEFAULT_SETTINGS}
        onComplete={vi.fn()}
        onSkip={vi.fn()}
      />
    );
    await waitForFrame();
    stdin.write(ENTER);
    const frame = lastFrame()!;
    expect(frame).toContain("IDE");
    expect(frame).toContain("editor");
  });

  it("shows IDE options after advancing", async () => {
    const { stdin, lastFrame } = render(
      <SetupWizard
        initialSettings={DEFAULT_SETTINGS}
        onComplete={vi.fn()}
        onSkip={vi.fn()}
      />
    );
    await waitForFrame();
    stdin.write(ENTER);
    const frame = lastFrame()!;
    expect(frame).toContain("Cursor");
    expect(frame).toContain("VS Code");
    expect(frame).toContain("Terminal");
  });
});
