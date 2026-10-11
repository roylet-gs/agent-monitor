import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

test("delete dialog for a worktree can be cancelled", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("feature/to-delete");

  await tui.sendKey("d");
  await tui.waitForText('Delete worktree "feature/to-delete"');
  await tui.waitForText("Remove this worktree from disk?");
  const text = await tui.getScreenText();
  expect(text).toMatch(/⏎ \/ y\s+Yes/);
  expect(text).toMatch(/n \/ esc\s+Cancel/);
  await tui.screenshot("delete-dialog");

  await tui.sendKey("Escape");
  await tui.waitForGone("Remove this worktree from disk?");
  await tui.waitForText("feature/to-delete");
});

test("the main checkout can't be deleted", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");
  await tui.sendKey("d");
  await tui.waitForText("The main checkout can't be deleted", 3_000);
  expect(await tui.getScreenText()).not.toContain("Remove this worktree from disk?");
});

test("create then delete a worktree end to end (incl. its local branch)", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  const branch = await tui.createWorktree(`e2e-del-${Date.now() % 1_000_000}`);

  await tui.selectRow(branch);
  await tui.sendKey("d");
  await tui.waitForText(`Delete worktree "${branch}"`);
  await tui.sendKey("y");
  await tui.waitForText(`Also delete local branch ${branch}?`);
  await tui.sendKey("y");
  // Gone from the list, the "Deleting …" modal and the "Deleted …" notice.
  await tui.waitForGone(branch, 20_000);
  await tui.screenshot("worktree-deleted");
});

test("long name wraps in the Working box; any key continues the delete in the background", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  const id = Date.now() % 1_000_000;
  const branch = `feature/e2e-wrap-${id}`;
  const first = `Wrap${id}`;
  const nickname = `${first} a deliberately long nickname so the Working box must wrap it onto another line zebrafinale`;
  expect(nickname.length).toBeGreaterThan(72);

  // New worktree with a custom Name (Tab moves from Branch to Name).
  await tui.sendKey("n");
  await tui.waitForText("New worktree");
  await tui.type(`e2e-wrap-${id}`);
  await tui.waitForGone("checking…", 10_000);
  await tui.sendKey("Tab");
  await tui.type(nickname);
  await tui.sendKey("Enter");
  await tui.waitForGone("New worktree", 10_000);
  await tui.waitForGone("Creating ", 30_000);
  await tui.waitForMatch(new RegExp(`│[▌ ][│├└─ ]*\\S ${first}\\s`), 15_000);

  await tui.selectRow(first);
  await tui.sendKey("d");
  await tui.waitForText("Remove this worktree from disk?");
  await tui.sendKey("y");
  await tui.waitForText(`Also delete local branch ${branch}?`);

  // A local `git worktree remove` can finish before a 250 ms poll sees the
  // busy box, so watch xterm's write path: once a complete frame shows the
  // Working box, keep that frame's text and hold later writes (test side
  // only, released right after) so the screenshot shows it too.
  await page.evaluate(() => {
    const w = window as any;
    const term = w.term?.terminal ?? w.terminal ?? w.term;
    const orig = term.write.bind(term);
    w.__e2e = { orig, held: [] as unknown[][], hold: false, frame: "" };
    term.write = (data: unknown, cb?: () => void) => {
      if (w.__e2e.hold) {
        w.__e2e.held.push([data, cb]);
        return;
      }
      // Only stop after a chunk that ends a frame (synchronized output end,
      // DEC 2026), so the held screen is never a half-drawn frame.
      const chunk = typeof data === "string" ? data : new TextDecoder().decode(data as Uint8Array);
      const frameEnd = chunk.includes("\x1b[?2026l");
      orig(data, () => {
        cb?.();
        if (w.__e2e.frame || !frameEnd) return;
        const buf = term.buffer.active;
        const lines: string[] = [];
        for (let i = 0; i < buf.length; i++) lines.push(buf.getLine(i)?.translateToString(true) ?? "");
        const text = lines.join("\n");
        if (text.includes("─Working") && text.includes("Deleting")) {
          w.__e2e.frame = text;
          w.__e2e.hold = true;
        }
      });
    };
  });
  const release = () =>
    page.evaluate(() => {
      const w = window as any;
      const term = w.term?.terminal ?? w.terminal ?? w.term;
      term.write = w.__e2e.orig;
      w.__e2e.hold = false;
      for (const [d, cb] of w.__e2e.held) w.__e2e.orig(d, cb);
      w.__e2e.held = [];
    });

  await tui.sendKey("y");
  await page.waitForFunction(() => Boolean((window as any).__e2e.frame), undefined, { timeout: 10_000 });
  const busy: string = await page.evaluate(() => (window as any).__e2e.frame);
  // The whole name is there, wrapped: its last word made it into the box.
  expect(busy).toContain(`Deleting ${first}`);
  expect(busy).toContain("zebrafinale");
  expect(busy).toContain("any key: continue in background");
  expect(busy.split("\n").some((l) => l.includes("zebrafinale") && !l.includes(first))).toBe(true);
  await tui.screenshot("delete-busy-wrapped");
  await release();

  // Any key hides the box (if the delete is still running); it carries on.
  await tui.sendKey("x");
  await tui.waitForGone("─Working", 5_000);
  // Only worktree rows (inside the pane border); the key bar's right-aligned
  // "Deleted <name>" notice also carries the name once the delete is done.
  const rowWith = async () => (await tui.lines()).find((l) => /^│[▌ ]/.test(l) && l.includes(first));
  const rowLine = await rowWith();
  // Either still running (row badged `deleting…`) or already gone.
  if (rowLine) expect(rowLine).toContain("deleting…");
  await tui.screenshot("delete-in-background");

  await expect.poll(rowWith, { timeout: 30_000 }).toBeUndefined();
});
