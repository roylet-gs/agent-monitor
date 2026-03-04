#!/usr/bin/env node
import meow from "meow";
import { render } from "ink";
import React from "react";
import { initLogger } from "./lib/logger.js";
import { loadSettings } from "./lib/settings.js";
import { handleHookEvent } from "./commands/hook-event.js";
import { printStatus } from "./commands/status.js";
import { installGlobalHooks, uninstallGlobalHooks } from "./lib/hooks-installer.js";
import { App } from "./app.js";
import { runScript, waitForEnter } from "./lib/run-script.js";

const cli = meow(
  `
  Agent Monitor — TUI dashboard for git worktrees & Claude Code agents

  Usage
    $ am                            Launch the dashboard
    $ am status -w <path>           Print agent status for a worktree
    $ am status -w <path> --set <s> Set agent status for a worktree
    $ am install-hooks              Install Claude hooks into ~/.claude/settings.json
    $ am uninstall-hooks            Remove agent-monitor hooks from ~/.claude/settings.json
    $ am hook-event -w <path>       Receive hook event from stdin (used by hooks)

  Options
    --worktree, -w  Worktree path
    --event, -e     Event name override (for hook-event)
    --set           Set agent status (idle, executing, planning, waiting)
    --help          Show this help
    --version       Show version

  Dashboard Keys
    j/k ↑/↓  Navigate        n  New worktree     d  Delete
    Enter    Open in IDE      s  Settings         r  Refresh
    g        Open PR          l  Open Linear      q  Quit
`,
  {
    description: false,
    importMeta: import.meta,
    flags: {
      worktree: { type: "string", shortFlag: "w" },
      event: { type: "string", shortFlag: "e" },
      set: { type: "string" },
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
    const instance = render(<App onRunScript={onRunScript} />, { patchConsole: true });

    try {
      await instance.waitUntilExit();
    } catch {
      // App exited
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
      console.error("Error: --worktree is required for hook-event");
      process.exit(1);
    }
    handleHookEvent(worktreePath, cli.flags.event).catch((err) => {
      console.error("hook-event error:", err);
      process.exit(1);
    });
    break;
  }

  case "status": {
    printStatus(cli.flags.worktree, cli.flags.set).catch((err) => {
      console.error("status error:", err);
      process.exit(1);
    });
    break;
  }

  case "install-hooks": {
    installGlobalHooks();
    console.log("Hooks installed into ~/.claude/settings.json");
    break;
  }

  case "uninstall-hooks": {
    uninstallGlobalHooks();
    console.log("Hooks removed from ~/.claude/settings.json");
    break;
  }

  default: {
    // Clear console before launching TUI
    process.stdout.write("\x1B[2J\x1B[3J\x1B[H");
    launchTui();
  }
}
