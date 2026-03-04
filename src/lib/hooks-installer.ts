import { existsSync, mkdirSync, readFileSync, writeFileSync } from "fs";
import { join } from "path";
import { log } from "./logger.js";

const HOOKS_CONFIG = {
  hooks: {
    PreToolUse: [
      {
        matcher: "",
        hooks: [
          {
            type: "command",
            command:
              'cat | am hook-event --worktree "$CLAUDE_PROJECT_DIR" --event PreToolUse',
            timeout: 5000,
          },
        ],
      },
    ],
    PostToolUse: [
      {
        matcher: "",
        hooks: [
          {
            type: "command",
            command:
              'cat | am hook-event --worktree "$CLAUDE_PROJECT_DIR" --event PostToolUse',
            timeout: 5000,
          },
        ],
      },
    ],
    Stop: [
      {
        matcher: "",
        hooks: [
          {
            type: "command",
            command:
              'cat | am hook-event --worktree "$CLAUDE_PROJECT_DIR" --event Stop',
            timeout: 5000,
          },
        ],
      },
    ],
    Notification: [
      {
        matcher: "",
        hooks: [
          {
            type: "command",
            command:
              'cat | am hook-event --worktree "$CLAUDE_PROJECT_DIR" --event Notification',
            timeout: 5000,
          },
        ],
      },
    ],
    SessionStart: [
      {
        matcher: "",
        hooks: [
          {
            type: "command",
            command:
              'cat | am hook-event --worktree "$CLAUDE_PROJECT_DIR" --event SessionStart',
            timeout: 5000,
          },
        ],
      },
    ],
  },
};

export function installHooks(worktreePath: string): void {
  const claudeDir = join(worktreePath, ".claude");
  const settingsPath = join(claudeDir, "settings.json");

  if (!existsSync(claudeDir)) {
    mkdirSync(claudeDir, { recursive: true });
  }

  // If settings.local.json exists, merge hooks into it
  let existing: Record<string, unknown> = {};
  if (existsSync(settingsPath)) {
    try {
      existing = JSON.parse(readFileSync(settingsPath, "utf-8"));
    } catch {
      existing = {};
    }
  }

  const merged = { ...existing, ...HOOKS_CONFIG };
  writeFileSync(settingsPath, JSON.stringify(merged, null, 2) + "\n");

  // Ensure .claude/ is in .gitignore
  ensureGitignore(worktreePath);

  log("info", "hooks", `Installed hooks into ${settingsPath}`);
}

function ensureGitignore(worktreePath: string): void {
  const gitignorePath = join(worktreePath, ".gitignore");
  if (existsSync(gitignorePath)) {
    const content = readFileSync(gitignorePath, "utf-8");
    if (content.includes(".claude/")) return;
    writeFileSync(gitignorePath, content.trimEnd() + "\n.claude/\n");
  }
  // Don't create .gitignore if it doesn't exist — .claude/ is already gitignored by Claude Code
}
