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
    await this.pg.waitForTimeout(1500);
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
    return (await this.lines()).find((l) => l.includes("│▌")) ?? "";
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
        if ((await this.selectedName()).startsWith(branch)) return;
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

  /** Left-click (or double-click) the first occurrence of `text`. */
  async clickText(text: string, opts: { double?: boolean } = {}): Promise<void> {
    const { col, row } = await this.findCell(text);
    const { x, y } = await this.cellCenter(col, row);
    if (opts.double) await this.pg.mouse.dblclick(x, y);
    else await this.pg.mouse.click(x, y);
    await this.pg.waitForTimeout(300);
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

  /** Press a key `n` times. */
  async press(key: string, n = 1): Promise<void> {
    for (let i = 0; i < n; i++) await this.sendKey(key);
  }
}
