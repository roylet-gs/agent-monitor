#!/usr/bin/env node
import meow from "meow";
import { render } from "ink";
import React from "react";
import { initLogger, log } from "./lib/logger.js";
import { loadSettings } from "./lib/settings.js";
import { handleHookEvent } from "./commands/hook-event.js";
import { printStatus } from "./commands/status.js";
import { printLogs } from "./commands/logs.js";
import { installHooks } from "./lib/hooks-installer.js";
import { App } from "./app.js";
import { runScript, waitForEnter } from "./lib/run-script.js";

const cli = meow(
  `
  Agent Monitor — TUI dashboard for git worktrees & Claude Code agents

  Usage
    $ am                            Launch the dashboard
    $ am --watch                    Launch with log panel open
    $ am status -w <path>           Print agent status for a worktree
    $ am install-hooks <path>       Install Claude hooks into a worktree
    $ am hook-event -w <path>       Receive hook event from stdin (used by hooks)
    $ am logs                       Show recent logs
    $ am logs -f                    Follow log output
    $ am logs --level error         Filter by level
    $ am logs --module hook-event   Filter by module
    $ am logs --clear               Clear log file

  Options
    --worktree, -w  Worktree path
    --event, -e     Event name override (for hook-event)
    --lines, -n     Number of log lines to show (default: 50)
    --follow, -f    Follow log output
    --level         Filter logs by level (debug, info, warn, error)
    --module        Filter logs by module
    --clear         Clear the log file
    --watch         Launch with log panel open
    --help          Show this help
    --version       Show version

  Dashboard Keys
    j/k ↑/↓  Navigate        n  New worktree     d  Delete
    Enter    Open in IDE      s  Settings         r  Refresh
    g        Open PR          l  Open Linear      w  Watch logs
    q        Quit
`,
  {
    description: false,
    importMeta: import.meta,
    flags: {
      worktree: { type: "string", shortFlag: "w" },
      event: { type: "string", shortFlag: "e" },
      lines: { type: "number", shortFlag: "n", default: 50 },
      follow: { type: "boolean", shortFlag: "f", default: false },
      level: { type: "string" },
      module: { type: "string" },
      clear: { type: "boolean", default: false },
      watch: { type: "boolean", default: false },
    },
  }
);

const [command] = cli.input;
const settings = loadSettings();
initLogger(settings.logLevel);

async function launchTui(): Promise<void> {
  let pendingScript: { scriptPath: string; cwd: string } | null = null;

  const onRunScript = (scriptPath: string, cwd: string) => {
    pendingScript = { scriptPath, cwd };
  };

  // eslint-disable-next-line no-constant-condition
  while (true) {
    const instance = render(<App onRunScript={onRunScript} watch={cli.flags.watch} />, { patchConsole: true });

    try {
      await instance.waitUntilExit();
    } catch (err) {
      log("debug", "cli", `TUI exit: ${err}`);
    }

    // Always unmount + clear to remove stale Ink output
    instance.unmount();
    instance.clear();

    if (!pendingScript) {
      // Normal exit (user pressed q)
      process.exit(0);
    }

    // Run the script with full terminal access
    const { scriptPath, cwd } = pendingScript;
    pendingScript = null;

    runScript(scriptPath, cwd);
    waitForEnter();

    // Clear screen before re-launching TUI
    process.stdout.write("\x1B[2J\x1B[3J\x1B[H");
  }
}

switch (command) {
  case "hook-event": {
    const worktreePath = cli.flags.worktree;
    if (!worktreePath) {
      log("error", "cli", "Missing --worktree flag for hook-event");
      console.error("Error: --worktree is required for hook-event");
      process.exit(1);
    }
    handleHookEvent(worktreePath, cli.flags.event).catch((err) => {
      log("error", "cli", `hook-event error: ${err}`);
      console.error("hook-event error:", err);
      process.exit(1);
    });
    break;
  }

  case "status": {
    printStatus(cli.flags.worktree);
    break;
  }

  case "logs": {
    printLogs({
      lines: cli.flags.lines,
      follow: cli.flags.follow,
      level: cli.flags.level,
      module: cli.flags.module,
      clear: cli.flags.clear,
    });
    break;
  }

  case "install-hooks": {
    const path = cli.input[1];
    if (!path) {
      log("error", "cli", "Missing path for install-hooks");
      console.error("Usage: am install-hooks <path>");
      process.exit(1);
    }
    installHooks(path);
    console.log(`Hooks installed into ${path}/.claude/settings.local.json`);
    break;
  }

  default: {
    // Clear console before launching TUI
    process.stdout.write("\x1B[2J\x1B[3J\x1B[H");
    launchTui();
  }
}
