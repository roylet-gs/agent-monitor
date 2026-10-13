import type { Page } from "@playwright/test";

/**
 * Page Object Model wrapping Playwright's Page with TUI-specific helpers.
 * Interacts with the ttyd terminal via xterm.js DOM elements.
 */
export class TuiPage {
  constructor(private page: Page) {}

  /** Navigate to ttyd and wait for the terminal to initialize. */
  async goto(): Promise<void> {
    await this.page.goto("/");
    // Wait for xterm.js terminal to render (canvas or DOM)
    await this.page.waitForSelector(".xterm-screen", { timeout: 15_000 });
    // Wait for the terminal buffer to have content
    await this.page.waitForFunction(
      () => {
        const term =
          (window as any).term?.terminal ??
          (window as any).terminal ??
          (window as any).term;
        if (!term?.buffer?.active) return false;
        const buf = term.buffer.active;
        for (let i = 0; i < buf.length; i++) {
          const line = buf.getLine(i);
          if (line && line.translateToString(true).trim().length > 0) return true;
        }
        return false;
      },
      { timeout: 15_000 }
    );
    // No fixed settle time: callers wait for the text they need (RustTui.open
    // waits for the first data frame).
  }

  /** Extract all visible text from the terminal buffer. */
  async getScreenText(): Promise<string> {
    return this.page.evaluate(() => {
      // ttyd exposes the xterm.js Terminal instance; try multiple access paths
      const term =
        (window as any).term?.terminal ??  // ttyd >=1.7
        (window as any).terminal ??         // fallback
        (window as any).term;               // older ttyd
      if (term?.buffer?.active) {
        const buf = term.buffer.active;
        const lines: string[] = [];
        for (let i = 0; i < buf.length; i++) {
          const line = buf.getLine(i);
          if (line) lines.push(line.translateToString(true));
        }
        return lines.join("\n");
      }
      // Fallback: try DOM-based extraction (works with DOM renderer)
      const rows = document.querySelectorAll(".xterm-rows > div");
      return Array.from(rows)
        .map((row) => (row as HTMLElement).textContent ?? "")
        .join("\n");
    });
  }

  /**
   * Poll terminal content until the given text appears.
   * @param text - The text to wait for (case-sensitive substring match).
   * @param timeout - Maximum time to wait in ms (default 10s).
   */
  async waitForText(text: string, timeout = 10_000): Promise<void> {
    const start = Date.now();
    while (Date.now() - start < timeout) {
      const content = await this.getScreenText();
      if (content.includes(text)) return;
      await this.page.waitForTimeout(250);
    }
    const finalContent = await this.getScreenText();
    throw new Error(
      `Timed out waiting for text "${text}" after ${timeout}ms.\nScreen content:\n${finalContent}`
    );
  }

  /**
   * Send a key press to the terminal.
   * For special keys, uses Playwright key names (e.g. "Escape", "Enter", "ArrowUp").
   */
  async sendKey(key: string): Promise<void> {
    await this.focus();
    const before = await this.getScreenText();
    await this.page.keyboard.press(key);
    await this.settle(before);
  }

  /** Type a string into the terminal. */
  async type(text: string): Promise<void> {
    await this.focus();
    const before = await this.getScreenText();
    await this.page.keyboard.type(text, { delay: 50 });
    await this.settle(before);
  }

  /**
   * Wait for the TUI to redraw after input: return as soon as the screen
   * differs from `before`, or after `max` ms for keys that change nothing
   * (the old fixed delay). Callers that need a specific state still wait
   * for its text.
   */
  private async settle(before: string, max = 300): Promise<void> {
    const start = Date.now();
    while (Date.now() - start < max) {
      await this.page.waitForTimeout(20);
      if ((await this.getScreenText()) !== before) return;
    }
  }

  /**
   * Give the terminal keyboard focus without clicking it: am-tui captures
   * the mouse, so a focus click would reach the app (select a row, close
   * the help overlay…).
   */
  async focus(): Promise<void> {
    await this.page.evaluate(() => {
      const term =
        (window as any).term?.terminal ??
        (window as any).terminal ??
        (window as any).term;
      if (term?.focus) term.focus();
      else (document.querySelector(".xterm-helper-textarea") as HTMLElement | null)?.focus();
    });
  }

  /** Save a screenshot to the tmp directory. */
  async screenshot(name: string): Promise<void> {
    await this.page.screenshot({
      path: `tests/e2e/tmp/${name}.png`,
      fullPage: true,
    });
  }
}
