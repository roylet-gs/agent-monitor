#!/usr/bin/env node
import meow from "meow";
import { render } from "ink";
import React from "react";
import { initLogger } from "./lib/logger.js";
import { loadSettings } from "./lib/settings.js";
import { handleHookEvent } from "./commands/hook-event.js";
import { runStatus } from "./commands/status.js";
import { runList } from "./commands/list.js";
import { runCreate } from "./commands/create.js";
import { runDelete } from "./commands/delete.js";
import { runOpen } from "./commands/open.js";
import { runOpenPr } from "./commands/open-pr.js";
import { runOpenLinear } from "./commands/open-linear.js";
import { runSync } from "./commands/sync.js";
import { runConfig } from "./commands/config.js";
import { runRepoList, runRepoAdd, runRepoRemove } from "./commands/repo.js";
import { installHooks } from "./lib/hooks-installer.js";
import { App } from "./app.js";
import { runScript, waitForEnter } from "./lib/run-script.js";

const cli = meow(
  `
  Agent Monitor — TUI dashboard for git worktrees & Claude Code agents

  Usage
    $ am                                  Launch the dashboard
    $ am list [--repo <path>] [--json]    List all worktrees with status
    $ am status [-w <path>] [-b <branch>] [--json]
                                          Show detailed worktree status
    $ am create <branch> [--repo <path>] [--name <n>] [--reuse] [--no-hooks] [--open]
                                          Create a new worktree
    $ am delete [-w <path>] [-b <branch>] [--delete-branch] [--force] [--yes]
                                          Delete a worktree
    $ am open [-w <path>] [-b <branch>] [--ide <cursor|vscode|terminal>]
                                          Open worktree in IDE
    $ am open-pr [-w <path>] [-b <branch>]
                                          Open PR in browser
    $ am open-linear [-w <path>] [-b <branch>]
                                          Open Linear ticket in browser
    $ am sync [--repo <path>]             Sync git worktrees to DB
    $ am config [<key>] [<value>] [--json] [--reset]
                                          Get/set configuration
    $ am repo list [--json]               List repositories
    $ am repo add <path>                  Add a repository
    $ am repo remove <path> [--yes]       Remove a repository
    $ am install-hooks <path>             Install Claude hooks into a worktree
    $ am hook-event -w <path>             Receive hook event from stdin

  Options
    --worktree, -w      Worktree path
    --branch, -b        Branch name
    --repo              Repository path (for disambiguation)
    --json              Output as JSON
    --ide               IDE to open with (cursor, vscode, terminal)
    --name              Custom name for worktree
    --reuse             Reuse existing branch
    --no-hooks          Skip hook installation
    --open              Open in IDE after creating
    --delete-branch     Also delete the git branch
    --force             Force operation
    --yes, -y           Skip confirmation prompts
    --reset             Reset settings to defaults
    --help              Show this help
    --version           Show version

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
      branch: { type: "string", shortFlag: "b" },
      event: { type: "string", shortFlag: "e" },
      repo: { type: "string" },
      json: { type: "boolean", default: false },
      ide: { type: "string" },
      name: { type: "string" },
      reuse: { type: "boolean", default: false },
      noHooks: { type: "boolean", default: false },
      open: { type: "boolean", default: false },
      deleteBranch: { type: "boolean", default: false },
      force: { type: "boolean", default: false },
      yes: { type: "boolean", shortFlag: "y", default: false },
      reset: { type: "boolean", default: false },
    },
  }
);

const [command, ...restArgs] = cli.input;
const settings = loadSettings();
initLogger(settings.logLevel);

const resolveOpts = {
  worktree: cli.flags.worktree,
  branch: cli.flags.branch,
  repo: cli.flags.repo,
};

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

async function main(): Promise<void> {
  switch (command) {
    case "hook-event": {
      const worktreePath = cli.flags.worktree;
      if (!worktreePath) {
        console.error("Error: --worktree is required for hook-event");
        process.exit(1);
      }
      await handleHookEvent(worktreePath, cli.flags.event);
      break;
    }

    case "status": {
      await runStatus(resolveOpts, { json: cli.flags.json });
      break;
    }

    case "list": {
      await runList({ repo: cli.flags.repo, json: cli.flags.json });
      break;
    }

    case "create": {
      const branch = restArgs[0];
      if (!branch) {
        console.error("Usage: am create <branch> [--repo <path>] [--name <n>] [--reuse] [--no-hooks] [--open]");
        process.exit(1);
      }
      await runCreate(branch, {
        repo: cli.flags.repo,
        name: cli.flags.name,
        reuse: cli.flags.reuse,
        noHooks: cli.flags.noHooks,
        open: cli.flags.open,
      });
      break;
    }

    case "delete": {
      await runDelete(resolveOpts, {
        deleteBranch: cli.flags.deleteBranch,
        force: cli.flags.force,
        yes: cli.flags.yes,
      });
      break;
    }

    case "open": {
      runOpen(resolveOpts, {
        ide: cli.flags.ide as "cursor" | "vscode" | "terminal" | undefined,
      });
      break;
    }

    case "open-pr": {
      await runOpenPr(resolveOpts);
      break;
    }

    case "open-linear": {
      await runOpenLinear(resolveOpts);
      break;
    }

    case "sync": {
      await runSync({ repo: cli.flags.repo });
      break;
    }

    case "config": {
      runConfig(restArgs, { json: cli.flags.json, reset: cli.flags.reset });
      break;
    }

    case "repo": {
      const subcommand = restArgs[0];
      switch (subcommand) {
        case "list":
          runRepoList({ json: cli.flags.json });
          break;
        case "add": {
          const path = restArgs[1];
          if (!path) {
            console.error("Usage: am repo add <path>");
            process.exit(1);
          }
          await runRepoAdd(path);
          break;
        }
        case "remove": {
          const path = restArgs[1];
          if (!path) {
            console.error("Usage: am repo remove <path>");
            process.exit(1);
          }
          await runRepoRemove(path, { yes: cli.flags.yes });
          break;
        }
        default:
          // Default to list if no subcommand
          runRepoList({ json: cli.flags.json });
          break;
      }
      break;
    }

    case "install-hooks": {
      const path = restArgs[0];
      if (!path) {
        console.error("Usage: am install-hooks <path>");
        process.exit(1);
      }
      installHooks(path);
      console.log(`Hooks installed into ${path}/.claude/settings.json`);
      break;
    }

    default: {
      // Clear console before launching TUI
      process.stdout.write("\x1B[2J\x1B[3J\x1B[H");
      launchTui();
    }
  }
}

main().catch((err) => {
  console.error("Error:", err.message ?? err);
  process.exit(1);
});
