import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { execSync } from "child_process";

const mockSpawnProcess = { unref: vi.fn() };
vi.mock("child_process", () => ({
  execSync: vi.fn(),
  spawn: vi.fn(() => mockSpawnProcess),
}));

vi.mock("fs", () => ({
  realpathSync: vi.fn((p: string) => p),
}));

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
}));

const mockedExecSync = vi.mocked(execSync);

describe("ide-launcher", () => {
  let originalPlatform: PropertyDescriptor | undefined;
  let originalTermProgram: string | undefined;

  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    originalPlatform = Object.getOwnPropertyDescriptor(process, "platform");
    originalTermProgram = process.env.TERM_PROGRAM;
    Object.defineProperty(process, "platform", { value: "darwin", configurable: true });
    process.env.TERM_PROGRAM = "Apple_Terminal";
  });

  afterEach(() => {
    if (originalPlatform) {
      Object.defineProperty(process, "platform", originalPlatform);
    }
    if (originalTermProgram !== undefined) {
      process.env.TERM_PROGRAM = originalTermProgram;
    } else {
      delete process.env.TERM_PROGRAM;
    }
  });

  describe("openTerminal", () => {
    it("tries to focus existing window before opening new one", async () => {
      // First call: tryFocusTerminalWindow osascript → returns "not_found"
      // Second call: open new terminal window
      // Third call: setTerminalTitle
      mockedExecSync
        .mockReturnValueOnce("not_found\n") // tryFocusTerminalWindow
        .mockReturnValueOnce("" as any)     // open new window
        .mockReturnValueOnce("" as any);    // setTerminalTitle

      const { openTerminal } = await import("../../src/lib/ide-launcher.js");
      const windowId = openTerminal("/tmp/worktrees/my-feature", "my-feature");

      expect(windowId).toBeDefined();
      expect(typeof windowId).toBe("string");
      expect(windowId!.length).toBe(6); // 3 random bytes = 6 hex chars

      // First call should be the focus attempt (osascript heredoc)
      const focusCall = mockedExecSync.mock.calls[0][0] as string;
      expect(focusCall).toContain("APPLESCRIPT");
      expect(focusCall).toContain("[am] my-feature");

      // Second call should open new Terminal window
      const openCall = mockedExecSync.mock.calls[1][0] as string;
      expect(openCall).toContain('tell app "Terminal" to do script');
      expect(openCall).toContain("/tmp/worktrees/my-feature");

      // Third call should set the window title
      const titleCall = mockedExecSync.mock.calls[2][0] as string;
      expect(titleCall).toContain("custom title");
      expect(titleCall).toContain(`[am] my-feature #${windowId}`);
    });

    it("returns undefined and skips new window when focus succeeds", async () => {
      mockedExecSync.mockReturnValueOnce("found\n"); // tryFocusTerminalWindow succeeds

      const { openTerminal } = await import("../../src/lib/ide-launcher.js");
      const windowId = openTerminal("/tmp/worktrees/my-feature", "my-feature");

      expect(windowId).toBeUndefined();
      // Should only have the one focus call, no open/title calls
      expect(mockedExecSync).toHaveBeenCalledTimes(1);
    });

    it("falls back to opening new window when focus throws", async () => {
      mockedExecSync
        .mockImplementationOnce(() => { throw new Error("osascript failed"); }) // focus fails
        .mockReturnValueOnce("" as any)  // open new window
        .mockReturnValueOnce("" as any); // setTerminalTitle

      const { openTerminal } = await import("../../src/lib/ide-launcher.js");
      const windowId = openTerminal("/tmp/worktrees/my-feature", "my-feature");

      expect(windowId).toBeDefined();
      expect(mockedExecSync).toHaveBeenCalledTimes(3);
    });

    it("uses path basename when no title provided", async () => {
      mockedExecSync.mockReturnValueOnce("found\n");

      const { openTerminal } = await import("../../src/lib/ide-launcher.js");
      openTerminal("/tmp/worktrees/my-feature");

      const focusCall = mockedExecSync.mock.calls[0][0] as string;
      expect(focusCall).toContain("[am] my-feature");
    });

    it("includes [am] prefix in window title", async () => {
      mockedExecSync
        .mockReturnValueOnce("not_found\n")
        .mockReturnValueOnce("" as any)
        .mockReturnValueOnce("" as any);

      const { openTerminal } = await import("../../src/lib/ide-launcher.js");
      openTerminal("/tmp/worktrees/my-feature", "custom-name");

      const titleCall = mockedExecSync.mock.calls[2][0] as string;
      expect(titleCall).toContain("[am] custom-name #");
    });
  });

  describe("openTerminal with iTerm2", () => {
    it("uses iTerm2-specific AppleScript for focus and title", async () => {
      process.env.TERM_PROGRAM = "iTerm.app";

      mockedExecSync
        .mockReturnValueOnce("not_found\n") // focus attempt
        .mockReturnValueOnce("" as any)     // create window
        .mockReturnValueOnce("" as any);    // set title

      const { openTerminal } = await import("../../src/lib/ide-launcher.js");
      openTerminal("/tmp/worktrees/feat", "feat");

      // Focus script should use iTerm2 session names
      const focusCall = mockedExecSync.mock.calls[0][0] as string;
      expect(focusCall).toContain("iTerm2");
      expect(focusCall).toContain("sessions");

      // Open should use iTerm2 create window
      const openCall = mockedExecSync.mock.calls[1][0] as string;
      expect(openCall).toContain("iTerm2");
      expect(openCall).toContain("create window with default profile");

      // Title should use iTerm2 session name
      const titleCall = mockedExecSync.mock.calls[2][0] as string;
      expect(titleCall).toContain("set name to");
    });
  });

  describe("openTerminal with Ghostty", () => {
    it("uses spawn with --title and --working-directory flags", async () => {
      process.env.TERM_PROGRAM = "ghostty";
      const { spawn } = await import("child_process");
      const mockedSpawn = vi.mocked(spawn);

      mockedExecSync
        .mockReturnValueOnce("not_found\n"); // focus attempt

      const { openTerminal } = await import("../../src/lib/ide-launcher.js");
      const windowId = openTerminal("/tmp/worktrees/feat", "feat");

      // Focus should use System Events
      const focusCall = mockedExecSync.mock.calls[0][0] as string;
      expect(focusCall).toContain("System Events");
      expect(focusCall).toContain("Ghostty");
      expect(focusCall).toContain("AXRaise");

      // Open should use spawn with Ghostty binary directly
      expect(mockedSpawn).toHaveBeenCalledWith(
        "/Applications/Ghostty.app/Contents/MacOS/ghostty",
        expect.arrayContaining([
          expect.stringContaining("--working-directory=/tmp/worktrees/feat"),
          expect.stringContaining("--title=[am] feat"),
        ]),
        expect.objectContaining({ stdio: "ignore", detached: true }),
      );

      // Should return a window ID (not undefined)
      expect(windowId).toBeDefined();
    });
  });

  describe("openInIde", () => {
    it("passes title through to openTerminal for terminal IDE", async () => {
      mockedExecSync
        .mockReturnValueOnce("not_found\n")
        .mockReturnValueOnce("" as any)
        .mockReturnValueOnce("" as any);

      const { openInIde } = await import("../../src/lib/ide-launcher.js");
      const result = openInIde("/tmp/worktrees/feat", "terminal", "my-branch");

      expect(result).toBeDefined();
      const titleCall = mockedExecSync.mock.calls[2][0] as string;
      expect(titleCall).toContain("[am] my-branch");
    });

    it("does not use terminal logic for cursor", async () => {
      mockedExecSync.mockReturnValueOnce("" as any);

      const { openInIde } = await import("../../src/lib/ide-launcher.js");
      openInIde("/tmp/worktrees/feat", "cursor", "my-branch");

      expect(mockedExecSync).toHaveBeenCalledTimes(1);
      const call = mockedExecSync.mock.calls[0][0] as string;
      expect(call).toContain("cursor");
      expect(call).not.toContain("[am]");
    });

    it("does not use terminal logic for vscode", async () => {
      mockedExecSync.mockReturnValueOnce("" as any);

      const { openInIde } = await import("../../src/lib/ide-launcher.js");
      openInIde("/tmp/worktrees/feat", "vscode", "my-branch");

      expect(mockedExecSync).toHaveBeenCalledTimes(1);
      const call = mockedExecSync.mock.calls[0][0] as string;
      expect(call).toContain("code");
    });
  });

  describe("openClaudeInTerminal", () => {
    it("opens a new terminal with claude command and sets title", async () => {
      mockedExecSync
        .mockReturnValueOnce("" as any)  // open terminal
        .mockReturnValueOnce("" as any); // set title

      const { openClaudeInTerminal } = await import("../../src/lib/ide-launcher.js");
      const windowId = openClaudeInTerminal("/tmp/worktrees/feat", false, "feat");

      expect(windowId).toBeDefined();
      expect(windowId.length).toBe(6);

      const openCall = mockedExecSync.mock.calls[0][0] as string;
      expect(openCall).toContain("claude");
      expect(openCall).not.toContain("claude -c");

      const titleCall = mockedExecSync.mock.calls[1][0] as string;
      expect(titleCall).toContain(`[am] feat #${windowId}`);
    });

    it("uses claude -c for continue session", async () => {
      mockedExecSync
        .mockReturnValueOnce("" as any)
        .mockReturnValueOnce("" as any);

      const { openClaudeInTerminal } = await import("../../src/lib/ide-launcher.js");
      openClaudeInTerminal("/tmp/worktrees/feat", true, "feat");

      const openCall = mockedExecSync.mock.calls[0][0] as string;
      expect(openCall).toContain("claude -c");
    });
  });
});
