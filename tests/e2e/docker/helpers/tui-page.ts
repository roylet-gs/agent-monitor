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
    // Give the TUI a moment to finish rendering
    await this.page.waitForTimeout(2000);
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
    // Focus the terminal
    await this.page.click(".xterm-screen", { force: true });
    await this.page.keyboard.press(key);
    // Small delay for the TUI to process the keystroke
    await this.page.waitForTimeout(300);
  }

  /** Type a string into the terminal. */
  async type(text: string): Promise<void> {
    await this.page.click(".xterm-screen", { force: true });
    await this.page.keyboard.type(text, { delay: 50 });
    await this.page.waitForTimeout(300);
  }

  /** Save a screenshot to the tmp directory. */
  async screenshot(name: string): Promise<void> {
    await this.page.screenshot({
      path: `tests/e2e/tmp/${name}.png`,
      fullPage: true,
    });
  }
}
