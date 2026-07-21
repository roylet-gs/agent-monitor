import { describe, it, expect, vi, beforeEach, afterAll } from "vitest";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

// Mock child_process and fs before importing
const mockExecSync = vi.fn();
const mockRealpathSync = vi.fn((p: string) => p);

vi.mock("child_process", () => ({
  execSync: (...args: unknown[]) => mockExecSync(...args),
}));

vi.mock("fs", () => ({
  realpathSync: (p: string) => mockRealpathSync(p),
}));

import {
  getTerminalPaths,
  getIdePaths,
  isTerminalOpenAt,
  killClaudeAtPath,
  parsePsArgs,
  friendlyCommandLabel,
  parseWorktreeProcesses,
  processesForWorktree,
} from "../../src/lib/process.js";
import type { RunningProcess } from "../../src/lib/types.js";

const originalProcessKill = process.kill;
const mockProcessKill = vi.fn(() => true);

beforeEach(() => {
  vi.clearAllMocks();
  mockRealpathSync.mockImplementation((p: string) => p);
  mockProcessKill.mockImplementation(() => true);
  process.kill = mockProcessKill as unknown as typeof process.kill;
});

afterAll(() => {
  process.kill = originalProcessKill;
});

describe("getTerminalPaths", () => {
  it("parses lsof output into a set of paths", () => {
    mockExecSync.mockReturnValue(
      "p1234\nfcwd\nn/Users/dev/project\np5678\nfcwd\nn/Users/dev/other\n"
    );
    const paths = getTerminalPaths();
    expect(paths.size).toBe(2);
    expect(paths.has("/Users/dev/project")).toBe(true);
    expect(paths.has("/Users/dev/other")).toBe(true);
  });

  it("ignores non-path lines", () => {
    mockExecSync.mockReturnValue("p1234\nfcwd\nn/Users/dev/project\n");
    const paths = getTerminalPaths();
    expect(paths.size).toBe(1);
    expect(paths.has("/Users/dev/project")).toBe(true);
  });

  it("returns empty set when lsof fails", () => {
    mockExecSync.mockImplementation(() => {
      throw new Error("lsof not found");
    });
    const paths = getTerminalPaths();
    expect(paths.size).toBe(0);
  });
});

describe("getIdePaths", () => {
  it("detects Cursor.app workspace path", () => {
    mockExecSync.mockReturnValue(
      "/Applications/Cursor.app/Contents/MacOS/Cursor /Users/dev/my-project\n"
    );
    const paths = getIdePaths();
    expect(paths.size).toBe(1);
    expect(paths.get("/Users/dev/my-project")).toBe("cursor");
  });

  it("detects VS Code (Code.app) workspace path", () => {
    mockExecSync.mockReturnValue(
      "/Applications/Visual Studio Code.app/Contents/MacOS/Code.app/Electron /Users/dev/vscode-project\n"
    );
    const paths = getIdePaths();
    expect(paths.size).toBe(1);
    expect(paths.get("/Users/dev/vscode-project")).toBe("vscode");
  });

  it("detects cursor CLI command", () => {
    mockExecSync.mockReturnValue("cursor /Users/dev/project\n");
    const paths = getIdePaths();
    expect(paths.get("/Users/dev/project")).toBe("cursor");
  });

  it("detects code CLI command", () => {
    mockExecSync.mockReturnValue("code /Users/dev/project\n");
    const paths = getIdePaths();
    expect(paths.get("/Users/dev/project")).toBe("vscode");
  });

  it("ignores lines without IDE processes", () => {
    mockExecSync.mockReturnValue(
      "/usr/bin/node /some/script.js\n/bin/bash\nps -eo args\n"
    );
    const paths = getIdePaths();
    expect(paths.size).toBe(0);
  });

  it("skips paths that don't exist (realpathSync throws)", () => {
    mockExecSync.mockReturnValue(
      "/Applications/Cursor.app/Contents/MacOS/Cursor /nonexistent/path\n"
    );
    mockRealpathSync.mockImplementation(() => {
      throw new Error("ENOENT");
    });
    const paths = getIdePaths();
    expect(paths.size).toBe(0);
  });

  it("handles multiple IDE processes", () => {
    mockExecSync.mockReturnValue(
      [
        "/Applications/Cursor.app/Contents/MacOS/Cursor /Users/dev/project-a",
        "/Applications/Visual Studio Code.app/Contents/Code.app/Electron /Users/dev/project-b",
        "cursor /Users/dev/project-c",
      ].join("\n")
    );
    const paths = getIdePaths();
    expect(paths.size).toBe(3);
    expect(paths.get("/Users/dev/project-a")).toBe("cursor");
    expect(paths.get("/Users/dev/project-b")).toBe("vscode");
    expect(paths.get("/Users/dev/project-c")).toBe("cursor");
  });

  it("returns empty map when ps fails", () => {
    mockExecSync.mockImplementation(() => {
      throw new Error("ps failed");
    });
    const paths = getIdePaths();
    expect(paths.size).toBe(0);
  });

  it("skips arguments starting with /--", () => {
    mockExecSync.mockReturnValue(
      "/Applications/Cursor.app/Contents/MacOS/Cursor /--some-flag /Users/dev/project\n"
    );
    const paths = getIdePaths();
    expect(paths.get("/Users/dev/project")).toBe("cursor");
  });

  it("resolves symlinks via realpathSync", () => {
    mockExecSync.mockReturnValue(
      "cursor /Users/dev/symlink-project\n"
    );
    mockRealpathSync.mockReturnValue("/Users/dev/real-project");
    const paths = getIdePaths();
    expect(paths.get("/Users/dev/real-project")).toBe("cursor");
  });
});

describe("killClaudeAtPath", () => {
  it("finds and kills a claude process at the target path", () => {
    // lsof output: pid 1234 with cwd at /Users/dev/project
    mockExecSync
      .mockReturnValueOnce("p1234\nn/Users/dev/project\n") // lsof
      .mockReturnValueOnce("node /Users/dev/.claude/local/claude\n"); // ps
    const killed = killClaudeAtPath("/Users/dev/project");
    expect(killed).toBe(1);
    expect(mockProcessKill).toHaveBeenCalledWith(1234, "SIGTERM");
  });

  it("skips non-claude node processes", () => {
    mockExecSync
      .mockReturnValueOnce("p1234\nn/Users/dev/project\n") // lsof
      .mockReturnValueOnce("node /Users/dev/server.js\n"); // ps — not claude
    const killed = killClaudeAtPath("/Users/dev/project");
    expect(killed).toBe(0);
    expect(mockProcessKill).not.toHaveBeenCalled();
  });

  it("handles lsof failure gracefully and returns 0", () => {
    mockExecSync.mockImplementation(() => {
      throw new Error("lsof not found");
    });
    const killed = killClaudeAtPath("/Users/dev/project");
    expect(killed).toBe(0);
    expect(mockProcessKill).not.toHaveBeenCalled();
  });

  it("returns 0 when no processes match the target path", () => {
    // lsof shows a process at a different path
    mockExecSync.mockReturnValueOnce("p1234\nn/Users/dev/other-project\n");
    const killed = killClaudeAtPath("/Users/dev/project");
    expect(killed).toBe(0);
    expect(mockProcessKill).not.toHaveBeenCalled();
  });

  it("kills multiple claude processes at the same path", () => {
    mockExecSync
      .mockReturnValueOnce("p1234\nn/Users/dev/project\np5678\nn/Users/dev/project\n")
      .mockReturnValueOnce("node claude\n") // ps for 1234
      .mockReturnValueOnce("node claude\n"); // ps for 5678
    const killed = killClaudeAtPath("/Users/dev/project");
    expect(killed).toBe(2);
    expect(mockProcessKill).toHaveBeenCalledWith(1234, "SIGTERM");
    expect(mockProcessKill).toHaveBeenCalledWith(5678, "SIGTERM");
  });

  it("handles ps failure gracefully (process already exited)", () => {
    mockExecSync
      .mockReturnValueOnce("p1234\nn/Users/dev/project\n") // lsof
      .mockImplementationOnce(() => { throw new Error("No such process"); }); // ps fails
    const killed = killClaudeAtPath("/Users/dev/project");
    expect(killed).toBe(0);
    expect(mockProcessKill).not.toHaveBeenCalled();
  });

  it("resolves symlinks via realpathSync before matching", () => {
    mockRealpathSync.mockReturnValue("/Users/dev/real-project");
    mockExecSync
      .mockReturnValueOnce("p1234\nn/Users/dev/real-project\n")
      .mockReturnValueOnce("node claude\n");
    const killed = killClaudeAtPath("/Users/dev/symlink-project");
    expect(killed).toBe(1);
    expect(mockRealpathSync).toHaveBeenCalledWith("/Users/dev/symlink-project");
  });
});

describe("parsePsArgs", () => {
  it("maps pid to full command line", () => {
    const map = parsePsArgs("  1234 node /repo/.bin/vite\n 5678 pnpm dev\n");
    expect(map.get(1234)).toBe("node /repo/.bin/vite");
    expect(map.get(5678)).toBe("pnpm dev");
  });

  it("handles a pid with no args", () => {
    const map = parsePsArgs("1234\n");
    expect(map.get(1234)).toBe("");
  });

  it("skips blank lines and non-numeric pids", () => {
    const map = parsePsArgs("\n  \nnotapid foo\n42 real\n");
    expect(map.has(42)).toBe(true);
    expect(map.size).toBe(1);
  });
});

describe("friendlyCommandLabel", () => {
  it("strips a leading node interpreter and basenames the tool", () => {
    expect(friendlyCommandLabel("node /repo/node_modules/.bin/vite", "node")).toBe("vite");
  });

  it("keeps a subcommand after the tool", () => {
    expect(friendlyCommandLabel("/opt/homebrew/bin/node /repo/.bin/pnpm dev", "node")).toBe("pnpm dev");
  });

  it("leaves a non-interpreter command intact (basenamed)", () => {
    expect(friendlyCommandLabel("npm run dev", "npm")).toBe("npm run dev");
  });

  it("falls back to the lsof comm when args are empty", () => {
    expect(friendlyCommandLabel("", "node")).toBe("node");
  });

  it("truncates very long labels", () => {
    const label = friendlyCommandLabel("someverylongtoolname " + "x".repeat(80), "x");
    expect(label.length).toBeLessThanOrEqual(40);
    expect(label.endsWith("…")).toBe(true);
  });
});

describe("parseWorktreeProcesses", () => {
  // lsof -d cwd -Fpcn field output: p<pid>, c<comm>, f<fd>, n<cwd path>
  const lsof = [
    "p1000", "cnode", "fcwd", "n/wt/a",       // dev server -> keep
    "p1001", "czsh", "fcwd", "n/wt/a",        // shell -> drop
    "p1002", "cnode", "fcwd", "n/wt/b",       // claude -> drop
    "p1003", "cnode", "fcwd", "n/other",      // not a worktree cwd (kept in map, attributed elsewhere)
    "p1004", "cpython3", "fcwd", "n/wt/a",    // python server -> keep
  ].join("\n") + "\n";

  const psArgs = new Map<number, string>([
    [1000, "node /wt/a/node_modules/.bin/vite"],
    [1001, "-zsh"],
    [1002, "node /Users/dev/.claude/local/claude --resume abc"],
    [1003, "node /other/server.js"],
    [1004, "python3 -m http.server"],
  ]);

  it("keeps non-shell/non-claude processes and derives labels", () => {
    const map = parseWorktreeProcesses(lsof, psArgs);
    const a = map.get("/wt/a") ?? [];
    expect(a.map((p) => p.command).sort()).toEqual(["python3 -m http.server", "vite"]);
    expect(a.find((p) => p.command === "vite")?.pid).toBe(1000);
  });

  it("excludes shells", () => {
    const map = parseWorktreeProcesses(lsof, psArgs);
    const a = map.get("/wt/a") ?? [];
    expect(a.some((p) => p.pid === 1001)).toBe(false);
  });

  it("excludes the claude agent", () => {
    const map = parseWorktreeProcesses(lsof, psArgs);
    expect(map.has("/wt/b")).toBe(false);
  });

  it("honors excludePids", () => {
    const map = parseWorktreeProcesses(lsof, psArgs, new Set([1000]));
    const a = map.get("/wt/a") ?? [];
    expect(a.some((p) => p.pid === 1000)).toBe(false);
    expect(a.some((p) => p.pid === 1004)).toBe(true);
  });

  it("keeps only processes matching a filter (case-insensitive, full command)", () => {
    const map = parseWorktreeProcesses(lsof, psArgs, new Set(), "VITE");
    const a = map.get("/wt/a") ?? [];
    expect(a.map((p) => p.pid)).toEqual([1000]); // only the vite process
  });

  it("matches against the full command line, not just the label", () => {
    // "http.server" appears in pid 1004's args but not its friendly label
    const map = parseWorktreeProcesses(lsof, psArgs, new Set(), "http.server");
    const a = map.get("/wt/a") ?? [];
    expect(a.map((p) => p.pid)).toEqual([1004]);
  });

  it("returns nothing when the filter matches no process", () => {
    const map = parseWorktreeProcesses(lsof, psArgs, new Set(), "nomatch");
    expect(map.size).toBe(0);
  });

  it("treats a blank/whitespace filter as no filter", () => {
    const map = parseWorktreeProcesses(lsof, psArgs, new Set(), "   ");
    const a = map.get("/wt/a") ?? [];
    expect(a.length).toBe(2);
  });
});

describe("processesForWorktree", () => {
  const procMap = new Map<string, RunningProcess[]>([
    ["/wt/main", [{ pid: 1, command: "vite" }]],
    ["/wt/main/apps/api", [{ pid: 2, command: "pnpm dev" }]],           // subdir of main
    ["/wt/main/.claude/worktrees/feat", [{ pid: 3, command: "next" }]], // nested worktree root
    ["/wt/main/.claude/worktrees/feat/apps/web", [{ pid: 4, command: "webpack" }]], // subdir of nested
    ["/unrelated", [{ pid: 5, command: "server" }]],
  ]);
  const roots = ["/wt/main", "/wt/main/.claude/worktrees/feat"];

  it("attributes root and subdirectory processes to the worktree (monorepo)", () => {
    const procs = processesForWorktree(procMap, "/wt/main", roots);
    // pid 1 (root) and pid 2 (subdir) belong to main; nested worktree's do NOT
    expect(procs.map((p) => p.pid).sort()).toEqual([1, 2]);
  });

  it("attributes nested-worktree processes to the most-specific root only", () => {
    const procs = processesForWorktree(procMap, "/wt/main/.claude/worktrees/feat", roots);
    expect(procs.map((p) => p.pid).sort()).toEqual([3, 4]);
  });

  it("returns empty when no process is under the worktree", () => {
    const procs = processesForWorktree(procMap, "/nope", roots);
    expect(procs).toEqual([]);
  });
});

describe("isTerminalOpenAt", () => {
  it("returns true when path matches a terminal cwd", () => {
    mockExecSync.mockReturnValue("n/Users/dev/project\n");
    expect(isTerminalOpenAt("/Users/dev/project")).toBe(true);
  });

  it("returns false when path is not in terminal cwds", () => {
    mockExecSync.mockReturnValue("n/Users/dev/other\n");
    expect(isTerminalOpenAt("/Users/dev/project")).toBe(false);
  });

  it("returns false when realpathSync fails", () => {
    mockRealpathSync.mockImplementation(() => {
      throw new Error("ENOENT");
    });
    expect(isTerminalOpenAt("/nonexistent")).toBe(false);
  });
});
