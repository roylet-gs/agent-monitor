import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

const REPOS_HELP = "Add or stop tracking repositories, edit their startup scripts";
const PAGE_HEADER = "Repositories — a add · x remove";

/** Settings → Repositories… */
async function openReposPage(tui: RustTui): Promise<void> {
  await tui.openSetting(REPOS_HELP);
  await tui.sendKey("Enter");
  await tui.waitForText(PAGE_HEADER);
}

/** Select `name` on the repositories page and stop tracking it. */
async function removeRepo(tui: RustTui, name: string): Promise<void> {
  for (let i = 0; i < 10 && !(await tui.lines()).some((l) => l.includes("▌") && l.includes(name)); i++) {
    await tui.sendKey("j");
  }
  await tui.sendKey("x");
  await tui.waitForText(`Stop tracking ${name}?`);
  await tui.sendKey("y");
  await tui.waitForText(`Stopped tracking ${name}`);
}

test("Settings → Repositories… adds a repo through the folder browser and removes it", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await openReposPage(tui);

  // The seeded repo, with the startup script the entrypoint wrote.
  await tui.waitForMatch(/work\s+\/work\s+\d+ worktrees\s+script/);
  // A retry after a failed attempt may find the repo still tracked.
  if ((await tui.getScreenText()).includes("other-repo")) {
    await removeRepo(tui, "other-repo");
  }
  await tui.screenshot("repositories-page");

  // a → the browser opens next to the newest repo (/work → /).
  await tui.sendKey("a");
  await tui.waitForText("Add repository ");
  await tui.waitForMatch(/work\/\s+✓ tracked/);
  await tui.type("home");
  await tui.sendKey("Enter");
  await tui.waitForText("/home ");
  await tui.type("user");
  await tui.sendKey("Enter");
  // The git repo is listed first.
  await tui.waitForMatch(/other-repo\/\s+git/);
  await tui.screenshot("add-repo-browser");
  await tui.type("other");
  await tui.sendKey("Enter");

  // Back on the page with the new repo; then on the dashboard as a group.
  await tui.waitForText("Added other-repo");
  // The new repo is selected (am's HOME may differ, so the path isn't always ~/…).
  await tui.waitForMatch(/▌\s+other-repo\s+\S*other-repo\s+1 worktree/);
  await tui.screenshot("repositories-added");
  await tui.sendKey("Escape");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
  await tui.waitForText("other-repo ─");

  // Stop tracking it again (restores the seeded state).
  await openReposPage(tui);
  await removeRepo(tui, "other-repo");
  expect(await tui.getScreenText()).not.toMatch(/other-repo\s+\S*other-repo\s+1 worktree/);
  await tui.sendKey("Escape");
  await tui.sendKey("Escape");
  await tui.waitForGone("other-repo ─");
});

test("the folder browser takes a typed path and refuses a tracked repo", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.sendKey("A");
  await tui.waitForText("Add repository ");
  await tui.type("/work");
  await tui.sendKey("Enter");
  await tui.waitForText("work is already tracked");
  await tui.sendKey("Escape");
  await tui.waitForGone("Add repository ");
});
