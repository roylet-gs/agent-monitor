import type { Page } from "@playwright/test";
import { TuiPage } from "../../docker/helpers/tui-page";

export { setupMock, resetMock } from "../../docker/helpers/mock-api-client";

/** ttyd serving am-tui with a fresh, empty data dir per connection. */
export const FRESH_URL = process.env.TUI_FRESH_URL || "http://localhost:7692";

/**
 * TuiPage plus helpers for the native (ratatui) dashboard. Layout reminder:
 * the last terminal row is the status bar, the one above it the context line
 * (` <branch> · ENG-123 … · PR #42 open · session …`) for the selection.
 */
export class RustTui extends TuiPage {
  constructor(private readonly pg: Page) {
    super(pg);
  }

  /** Open the seeded dashboard and wait for its first data frame. */
  async open(): Promise<void> {
    await this.goto();
    await this.waitForText("[1] Worktrees", 10_000);
    await this.waitForText(" of ", 10_000); // "N of M" footer = rows loaded
  }

  /** Open an absolute ttyd URL (e.g. FRESH_URL) and wait for content. */
  async openUrl(url: string): Promise<void> {
    await this.pg.goto(url);
    await this.pg.waitForSelector(".xterm-screen", { timeout: 15_000 });
    await this.pg.waitForTimeout(1500);
  }

  async lines(): Promise<string[]> {
    const lines = (await this.getScreenText()).split("\n");
    while (lines.length && lines[lines.length - 1].trim() === "") lines.pop();
    return lines;
  }

  async statusBar(): Promise<string> {
    const l = await this.lines();
    return l[l.length - 1] ?? "";
  }

  async contextLine(): Promise<string> {
    const l = await this.lines();
    return l[l.length - 2] ?? "";
  }

  /** Move the worktree selection to the row whose branch starts with `branch`. */
  async selectRow(branch: string, max = 10): Promise<void> {
    // Several passes: a row can move while we walk the list (e.g. a new
    // worktree is re-sorted once its Linear ticket arrives).
    for (let pass = 0; pass < 3; pass++) {
      await this.sendKey("Home");
      for (let i = 0; i < max; i++) {
        if ((await this.contextLine()).trim().startsWith(branch)) return;
        await this.sendKey("j");
      }
    }
    throw new Error(`Row "${branch}" not found.\n${await this.getScreenText()}`);
  }

  /** Poll until `text` is gone from the screen. */
  async waitForGone(text: string, timeout = 10_000): Promise<void> {
    const start = Date.now();
    while (Date.now() - start < timeout) {
      if (!(await this.getScreenText()).includes(text)) return;
      await this.pg.waitForTimeout(250);
    }
    throw new Error(`"${text}" still on screen after ${timeout}ms.\n${await this.getScreenText()}`);
  }

  /** Poll until `re` matches the screen. */
  async waitForMatch(re: RegExp, timeout = 10_000): Promise<void> {
    const start = Date.now();
    while (Date.now() - start < timeout) {
      if (re.test(await this.getScreenText())) return;
      await this.pg.waitForTimeout(250);
    }
    throw new Error(`No match for ${re} after ${timeout}ms.\n${await this.getScreenText()}`);
  }

  /**
   * `n` → type the branch (after the default `feature/` prefix) → Enter, and
   * wait until the new row exists. The "Working" modal (`Creating <branch>…`)
   * also shows the branch name, so wait for it to close first.
   */
  async createWorktree(suffix: string): Promise<string> {
    const branch = `feature/${suffix}`;
    await this.sendKey("n");
    await this.waitForText("New worktree");
    await this.type(suffix);
    await this.waitForGone("checking…", 10_000);
    await this.sendKey("Enter");
    await this.waitForGone("New worktree", 10_000);
    await this.waitForGone("Creating ", 30_000);
    await this.waitForMatch(new RegExp(`│[▌▏ ]\\S ${branch.replace(/[/.]/g, "\\$&")}\\s`), 15_000);
    return branch;
  }

  /** Press a key `n` times. */
  async press(key: string, n = 1): Promise<void> {
    for (let i = 0; i < n; i++) await this.sendKey(key);
  }
}
