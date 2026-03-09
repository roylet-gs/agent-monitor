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
    // Wait for xterm.js terminal to render rows
    await this.page.waitForSelector(".xterm-rows", { timeout: 15_000 });
    // Give the TUI a moment to render initial content
    await this.page.waitForTimeout(2000);
  }

  /** Extract all visible text from the terminal. */
  async getScreenText(): Promise<string> {
    return this.page.evaluate(() => {
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
    await this.page.click(".xterm-rows", { force: true });
    await this.page.keyboard.press(key);
    // Small delay for the TUI to process the keystroke
    await this.page.waitForTimeout(300);
  }

  /** Type a string into the terminal. */
  async type(text: string): Promise<void> {
    await this.page.click(".xterm-rows", { force: true });
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
