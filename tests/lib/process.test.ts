import { describe, it, expect, vi, beforeEach } from "vitest";

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

import { getTerminalPaths, getIdePaths, isTerminalOpenAt } from "../../src/lib/process.js";

beforeEach(() => {
  vi.clearAllMocks();
  mockRealpathSync.mockImplementation((p: string) => p);
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
