import { expect, test } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// The entrypoint seeds a draft manifest plugin `toycli` and replays two of its
// native events (one with a tool the manifest doesn't map) on
// /home/user/toy-project, an "Other sessions" row.

test.beforeEach(async () => {
  await resetMock();
});

test("a draft plugin's session carries the ✎ draft badge", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForText("/home/user/toy-project ✦ toycli ✎ draft");
  // Built-in rows never get it.
  const gemini = (await tui.lines()).find((l) => l.includes("/home/user/gemini-project")) ?? "";
  expect(gemini).not.toContain("✎");
  await tui.screenshot("training-draft-row");

  await tui.sendKey("?");
  await tui.waitForText("✎ draft plugin still being trained");
  await tui.sendKey("?");
});

test("Settings → Agent plugins: train, publish and retrain", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.openSetting("Turn agent CLIs on/off, install their hooks, pick the default agent");
  await tui.sendKey("Enter");
  await tui.waitForText("Training — T train a new harness plugin · e retrain · p publish · x remove");
  await tui.waitForMatch(/\[x\] Toy CLI\s+toycli\.json.*✎ draft ⚠ 1/);
  await tui.waitForText("+ Train a new harness plugin…");

  // The draft's help line: what was captured and what went wrong.
  await tui.press("j", 2);
  await tui.waitForMatch(/toycli · draft · 2 capture\(s\) · 1 issue\(s\)/);
  await tui.screenshot("training-plugins-page");

  // Built-ins can't be retrained.
  await tui.sendKey("k");
  await tui.sendKey("e");
  await tui.waitForText("is built in; T trains a new harness plugin");
  await tui.sendKey("j");

  // T: type the command; there's no terminal in the container, so the popup
  // says what to run instead (the workspace and TRAINING.md are ready).
  await tui.sendKey("T");
  await tui.waitForText("Command that starts it:");
  await tui.type("toycli");
  await tui.screenshot("training-command");
  await tui.sendKey("Enter");
  await tui.waitForText("TRAINING.md");
  await tui.waitForText("/training/toycli");
  await tui.screenshot("training-popup");
  await tui.sendKey("Escape");

  // p asks first, then publishes: the draft tag goes.
  await tui.sendKey("p");
  await tui.waitForText("Publish toycli? Stops capturing, drops the draft badge");
  await tui.screenshot("training-confirm-publish");
  await tui.sendKey("y");
  await tui.waitForText("toycli published");
  await tui.waitForMatch(/\[x\] Toy CLI\s+toycli\.json\s+not set up\s+⚠ 1/);

  // e retrains it, which puts it back in draft (restores the seeded state).
  await tui.sendKey("e");
  await tui.waitForText("TRAINING.md");
  await tui.sendKey("Escape");
  await tui.sendKey("Escape");
  await tui.sendKey("Enter");
  await tui.waitForMatch(/\[x\] Toy CLI\s+toycli\.json.*✎ draft/);
  await tui.sendKey("Escape");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
});
