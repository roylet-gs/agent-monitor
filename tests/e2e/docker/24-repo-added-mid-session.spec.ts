import { test, expect } from "@playwright/test";
import { LATE_SESSION_URL, RustTui, resetMock } from "./helpers/rust-tui";

// Port 7683 fires `am hook-event --worktree /home/user/late-repo` (an
// untracked git repo → standalone session) before each am-tui it serves
// (e2e/entrypoint.sh). Adding the repo must move that running agent onto the
// new main row rather than losing it, even with hideMainBranch on.

test.beforeEach(async () => {
  await resetMock();
});

const REPOS_HELP = "Add or stop tracking repositories, edit their startup and review scripts";
const HIDE_MAIN_HELP = "Hide the main/master checkout; always applied";
const HIDE_MAIN = /Hide main branch\s+(on|off)/;
const STANDALONE = /● \/home\/user\/late-repo/;
const GROUP = "late-repo ─";

async function openLate(tui: RustTui): Promise<void> {
  await tui.openUrl(LATE_SESSION_URL);
  await tui.waitForText("· Worktrees", 10_000);
  await tui.waitForText(" of ", 10_000);
}

/** Settings → Hide main branch → `want`, then back to the dashboard. */
async function setHideMain(tui: RustTui, want: "on" | "off"): Promise<void> {
  await tui.openSetting(HIDE_MAIN_HELP);
  const m = HIDE_MAIN.exec(await tui.getScreenText());
  if (!m) throw new Error(`Hide main branch row not found:\n${await tui.getScreenText()}`);
  if (m[1] !== want) {
    await tui.sendKey(" ");
    await tui.waitForMatch(new RegExp(`Hide main branch\\s+${want}`));
  }
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
}

/** Settings → Repositories… → stop tracking late-repo (when tracked). */
async function untrackLateRepo(tui: RustTui): Promise<void> {
  await tui.openSetting(REPOS_HELP);
  await tui.sendKey("Enter");
  await tui.waitForText("Repositories — a add · x remove");
  if (/late-repo\s+\S*late-repo\s+\d+ worktree/.test(await tui.getScreenText())) {
    for (let i = 0; i < 10 && !(await tui.lines()).some((l) => l.includes("▌") && l.includes("late-repo")); i++) {
      await tui.sendKey("j");
    }
    await tui.sendKey("x");
    await tui.waitForText("Stop tracking late-repo?");
    await tui.sendKey("y");
    await tui.waitForText("Stopped tracking late-repo");
  }
  await tui.sendKey("Escape");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
}

test("an agent already running in a repo stays on the dashboard when the repo is added", async ({ page }) => {
  const tui = new RustTui(page);
  await openLate(tui);

  // A retry after a failed attempt may find the repo still tracked: stop
  // tracking it and reconnect, which fires the hook again (standalone now).
  if ((await tui.getScreenText()).includes(GROUP)) {
    await untrackLateRepo(tui);
    await openLate(tui);
  }

  await setHideMain(tui, "on");

  // Before: the agent is a standalone session under "Other sessions".
  await tui.waitForMatch(/│Other sessions ─+/);
  await tui.waitForMatch(STANDALONE);
  expect(await tui.getScreenText()).not.toContain(GROUP);
  await tui.screenshot("repo-added-mid-session-before");

  // Add the repo by typed path in the add-repository browser.
  await tui.sendKey("A");
  await tui.waitForText("Add repository ");
  await tui.type("/home/user/late-repo");
  await tui.sendKey("Enter");
  await tui.waitForGone("Add repository ", 10_000);

  // After: a late-repo group whose main row carries the executing agent
  // (shown despite hideMainBranch, because its session is open), and the
  // standalone row is gone.
  await tui.waitForText(GROUP);
  await tui.waitForMatch(/│[▌ ]● main\b/);
  const lines = await tui.lines();
  const header = lines.findIndex((l) => l.includes(GROUP));
  const main = lines.findIndex((l, i) => i > header && /│[▌ ]● main\b/.test(l));
  expect(header).toBeGreaterThanOrEqual(0);
  expect(main).toBeGreaterThan(header);
  // The main row sits in late-repo's group, not under the next header.
  expect(lines.slice(header + 1, main).some((l) => /│\S.* ─{3,}/.test(l))).toBe(false);
  expect(await tui.getScreenText()).not.toMatch(STANDALONE);
  await tui.screenshot("repo-added-mid-session-after");

  // Restore: stop tracking late-repo, hideMainBranch off.
  await untrackLateRepo(tui);
  await tui.waitForGone(GROUP);
  await setHideMain(tui, "off");
});
