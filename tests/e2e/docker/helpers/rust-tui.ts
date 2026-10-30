import type { Page } from "@playwright/test";
import { TuiPage } from "./tui-page";

export { setupMock, resetMock } from "./mock-api-client";

/** ttyd serving am-tui with a fresh, empty data dir per connection. */
export const FRESH_URL = process.env.TUI_FRESH_URL || "http://localhost:7682";

/**
 * ttyd serving the seeded dashboard, after a hook (fired per connection) from
 * an agent running in the untracked git repo /home/user/late-repo.
 */
export const LATE_SESSION_URL = process.env.TUI_LATE_SESSION_URL || "http://localhost:7683";

/** ttyd serving the seeded dashboard as an npm install (`AM_INSTALL_METHOD=npm`) with a fake `npm`. */
export const NPM_URL = process.env.TUI_NPM_URL || "http://localhost:7684";

/**
 * TuiPage plus helpers for the native (ratatui) dashboard. Layout reminder:
 * one full-width `am v… · Worktrees` pane (bottom-right `N of M`), then the key bar
 * on the last terminal row (` ⏎ open  │ g PR … h help  q quit`) with any
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
    await this.waitForText("· Worktrees", 10_000);
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

  /** Pixel centre of terminal cell (col, row), from the screen size ÷ cols/rows. */
  async cellCenter(col: number, row: number): Promise<{ x: number; y: number }> {
    const box = await this.pg.locator(".xterm-screen").boundingBox();
    if (!box) throw new Error("No .xterm-screen");
    const { cols, rows } = await this.pg.evaluate(() => {
      const term = (window as any).term?.terminal ?? (window as any).terminal ?? (window as any).term;
      return { cols: term.cols as number, rows: term.rows as number };
    });
    return { x: box.x + ((col + 0.5) * box.width) / cols, y: box.y + ((row + 0.5) * box.height) / rows };
  }

  /** Cell (col, row) of the first occurrence of `text` on screen. */
  async findCell(text: string): Promise<{ col: number; row: number }> {
    const lines = await this.lines();
    for (let row = 0; row < lines.length; row++) {
      const i = lines[row].indexOf(text);
      if (i >= 0) return { col: [...lines[row].slice(0, i)].length, row };
    }
    throw new Error(`"${text}" not on screen.\n${lines.join("\n")}`);
  }

  /**
   * Wait until the screen has not changed for `quiet` ms (at most `max` ms).
   * Rows can still shift just after `open()` (e.g. a `— ENG-123 —` header
   * appears when Linear data lands), which would move a click target.
   */
  async waitForStableScreen(quiet = 300, max = 3_000): Promise<void> {
    const start = Date.now();
    let last = await this.getScreenText();
    let since = Date.now();
    while (Date.now() - start < max && Date.now() - since < quiet) {
      await this.pg.waitForTimeout(50);
      const now = await this.getScreenText();
      if (now !== last) {
        last = now;
        since = Date.now();
      }
    }
  }

  /** Left-click (or double-click) the first occurrence of `text`. */
  async clickText(text: string, opts: { double?: boolean } = {}): Promise<void> {
    // Clicks are aimed by position, so don't aim at a layout that's moving.
    await this.waitForStableScreen();
    const { col, row } = await this.findCell(text);
    const { x, y } = await this.cellCenter(col, row);
    if (opts.double) await this.pg.mouse.dblclick(x, y);
    else await this.pg.mouse.click(x, y);
    await this.pg.waitForTimeout(300);
  }

  /** Move the pointer over the first occurrence of `text` (no click). */
  async hoverText(text: string): Promise<{ col: number; row: number }> {
    await this.waitForStableScreen();
    const cell = await this.findCell(text);
    const { x, y } = await this.cellCenter(cell.col, cell.row);
    await this.pg.mouse.move(x, y);
    await this.pg.waitForTimeout(300);
    return cell;
  }

  /** Whether cell (col, row) has the terminal's default background. */
  async bgIsDefault(col: number, row: number): Promise<boolean> {
    return this.pg.evaluate(
      ([c, r]) => {
        const term = (window as any).term?.terminal ?? (window as any).terminal ?? (window as any).term;
        const buf = term.buffer.active;
        return buf.getLine(buf.viewportY + r).getCell(c).isBgDefault() === true;
      },
      [col, row],
    );
  }

  /** Scroll the wheel over the middle of the terminal (`ticks` > 0 = down). */
  async wheel(ticks: number): Promise<void> {
    const box = await this.pg.locator(".xterm-screen").boundingBox();
    if (!box) throw new Error("No .xterm-screen");
    await this.pg.mouse.move(box.x + box.width / 2, box.y + box.height / 3);
    for (let i = 0; i < Math.abs(ticks); i++) {
      await this.pg.mouse.wheel(0, ticks > 0 ? 100 : -100);
      await this.pg.waitForTimeout(150);
    }
    await this.pg.waitForTimeout(200);
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

  /** Like `openSetting`, for rows near the end of Settings (Privacy, Setup):
   *  End, then up, instead of walking down the whole list. */
  async openSettingFromEnd(help: string, max = 20): Promise<void> {
    await this.sendKey("s");
    await this.waitForText("esc close");
    await this.sendKey("End");
    for (let i = 0; i < max && !(await this.getScreenText()).includes(help); i++) {
      await this.sendKey("k");
    }
    await this.waitForText(help);
  }

  /** Press a key `n` times. */
  async press(key: string, n = 1): Promise<void> {
    for (let i = 0; i < n; i++) await this.sendKey(key);
  }
}
