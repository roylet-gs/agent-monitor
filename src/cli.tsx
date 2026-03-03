#!/usr/bin/env node
import meow from "meow";
import { render } from "ink";
import React from "react";
import { initLogger } from "./lib/logger.js";
import { loadSettings } from "./lib/settings.js";
import { handleHookEvent } from "./commands/hook-event.js";
import { printStatus } from "./commands/status.js";
import { installHooks } from "./lib/hooks-installer.js";
import { App } from "./app.js";

const cli = meow(
  `
  Usage
    $ am                          Launch TUI
    $ am hook-event               Receive hook event from stdin
    $ am status --worktree <path> Print agent status
    $ am install-hooks <path>     Install Claude hooks into worktree

  Options
    --worktree, -w  Worktree path (for hook-event and status)
    --event, -e     Event name override (for hook-event)
`,
  {
    importMeta: import.meta,
    flags: {
      worktree: { type: "string", shortFlag: "w" },
      event: { type: "string", shortFlag: "e" },
    },
  }
);

const [command] = cli.input;
const settings = loadSettings();
initLogger(settings.logLevel);

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
    printStatus(cli.flags.worktree);
    break;
  }

  case "install-hooks": {
    const path = cli.input[1];
    if (!path) {
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
    const { waitUntilExit } = render(<App />);
    waitUntilExit().catch(() => process.exit(0));
  }
}
