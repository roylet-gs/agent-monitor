import type { Page } from "@playwright/test";
import { TuiPage } from "./tui-page";

export { setupMock, resetMock } from "./mock-api-client";

/** ttyd serving am-tui with a fresh, empty data dir per connection. */
export const FRESH_URL = process.env.TUI_FRESH_URL || "http://localhost:7682";

/**
 * TuiPage plus helpers for the native (ratatui) dashboard. Layout reminder:
 * one full-width `─Worktrees` pane (bottom-right `N of M`), then the key bar
 * on the last terminal row (` ⏎ open  t terminal … ? help  q quit`) with any
 * transient notice / offline hint right-aligned on it. The selected worktree
 * row starts with `▌` just inside the pane border (`│▌<tree>◆ main …`).
 * While a create/delete job runs, a `─Jobs` pane sits to the right of the
 * list; Tab moves focus between them.
 */
export class RustTui extends TuiPage {
  constructor(private readonly pg: Page) {
    super(pg);
  }

  /** Open the seeded dashboard and wait for its first data frame. */
  async open(): Promise<void> {
    await this.goto();
    await this.waitForText("─Worktrees", 10_000);
    await this.waitForText(" of ", 10_000); // "N of M" footer = rows loaded
  }

  /** Open an absolute ttyd URL (e.g. FRESH_URL) and wait for content. */
  async openUrl(url: string): Promise<void> {
    await this.pg.goto(url);
    await this.pg.waitForSelector(".xterm-screen", { timeout: 15_000 });
    await this.pg.waitForFunction(
      () => {
        const term = (window as any).term?.terminal ?? (window as any).terminal ?? (window as any).term;
        const buf = term?.buffer?.active;
        if (!buf) return false;
        for (let i = 0; i < buf.length; i++) {
          if (buf.getLine(i)?.translateToString(true).trim()) return true;
        }
        return false;
      },
      { timeout: 15_000 }
    );
  }

  async lines(): Promise<string[]> {
    const lines = (await this.getScreenText()).split("\n");
    while (lines.length && lines[lines.length - 1].trim() === "") lines.pop();
    return lines;
  }

  /** The key bar: the last non-empty row (also carries notices / offline hint). */
  async keyBar(): Promise<string> {
    const l = await this.lines();
    return l[l.length - 1] ?? "";
  }

  /** The selected worktree row (the one marked `│▌`), or "" if none. */
  async selectedRow(): Promise<string> {
    return (await this.lines()).find((l) => l.startsWith("│▌")) ?? "";
  }

  /**
   * Name (branch / nickname / path) on the selected row: after the `▌`, any
   * stack-tree prefix (`├─`/`└─`/`│`), then the status glyph and a space.
   */
  async selectedName(): Promise<string> {
    const m = /│▌[│├└─\s]*\S (\S+)/.exec(await this.selectedRow());
    return m ? m[1] : "";
  }

  /** Move the worktree selection to the row whose name starts with `branch`. */
  async selectRow(branch: string, max = 10): Promise<void> {
    // Several passes: a row can move while we walk the list (e.g. a new
    // worktree is re-sorted once its Linear ticket arrives).
    for (let pass = 0; pass < 3; pass++) {
      await this.sendKey("Home");
      for (let i = 0; i < max; i++) {
        const name = await this.selectedName();
        if (name.startsWith(branch)) return;
        await this.sendKey("j");
        // A redraw elsewhere (a job spinner) can end sendKey's wait before
        // the selection moved, so wait for the selection itself. At the
        // bottom of the list `j` changes nothing; give up after 500 ms.
        const start = Date.now();
        while (Date.now() - start < 500 && (await this.selectedName()) === name) {
          await this.pg.waitForTimeout(20);
        }
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
   * wait until the new row exists. The jobs pane and the placeholder row
   * also show the branch name, so first wait for the create to finish (it
   * asks about the seeded startup script, which is declined).
   */
  async createWorktree(suffix: string): Promise<string> {
    const branch = `feature/${suffix}`;
    await this.sendKey("n");
    await this.waitForText("New worktree");
    await this.type(suffix);
    await this.waitForGone("checking…", 10_000);
    await this.sendKey("Enter");
    await this.waitForGone("New worktree", 10_000);
    await this.skipStartupScript();
    await this.waitForMatch(new RegExp(`│[▌ ][│├└─ ]*\\S ${branch.replace(/[/.]/g, "\\$&")}\\s`), 15_000);
    return branch;
  }

  /**
   * After a create: wait for the jobs pane to ask about the seeded startup
   * script and decline (Tab focuses the pane, `n` skips). The placeholder
   * row (`creating…`) is gone by then.
   */
  async skipStartupScript(): Promise<void> {
    await this.waitForText("Run the repo's startup script?", 30_000);
    await this.sendKey("Tab");
    await this.sendKey("n");
    await this.waitForGone("Run the repo's startup script?", 5_000);
  }

  /**
   * `s` → walk down the settings list until the field whose help line
   * contains `help` is selected (only the selected field shows its help).
   * Robust to fields being added above it, unlike a fixed `j` count.
   */
  async openSetting(help: string, max = 40): Promise<void> {
    await this.sendKey("s");
    await this.waitForText("esc close");
    for (let i = 0; i < max && !(await this.getScreenText()).includes(help); i++) {
      await this.sendKey("j");
    }
    await this.waitForText(help);
  }

  /** Press a key `n` times. */
  async press(key: string, n = 1): Promise<void> {
    for (let i = 0; i < n; i++) await this.sendKey(key);
  }
}
