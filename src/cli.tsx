#!/usr/bin/env node
import { Command } from "commander";
import { render } from "ink";
import React from "react";
import { execFileSync } from "child_process";
import { initLogger, log } from "./lib/logger.js";
import { loadSettings, saveSettings } from "./lib/settings.js";
import { getVersion, detectPackageManager } from "./lib/version.js";
import { App } from "./app.js";
import { runScript, waitForEnter } from "./lib/run-script.js";

// --- CLI setup ---

const settings = loadSettings();
initLogger(settings.logLevel, getVersion(), settings.maxLogSizeMb);

let watchFlag = false;

const program = new Command()
  .name("am")
  .description("Agent Monitor — TUI dashboard for git worktrees & Claude Code agents")
  .version(getVersion())
  .option("--watch", "Launch with log panel open")
  .action((opts) => {
    watchFlag = opts.watch ?? false;
    // Default: launch TUI
    process.stdout.write("\x1B[2J\x1B[3J\x1B[H");
    launchTui();
  });

// --- Worktree commands ---

const worktreeCmd = program
  .command("worktree")
  .alias("wt")
  .description("Manage worktrees");

worktreeCmd
  .command("list")
  .description("List worktrees with status, PR, and Linear info")
  .option("--repo <path>", "Repository path (default: detect from CWD)")
  .option("--json", "Output as JSON")
  .action(async (opts) => {
    const { worktreeList } = await import("./commands/worktree/list.js");
    await worktreeList(opts);
  });

worktreeCmd
  .command("create <branch>")
  .description("Create a new worktree")
  .option("--repo <path>", "Repository path")
  .option("--name <name>", "Custom worktree name")
  .option("--base <branch>", "Base branch (default: main)")
  .option("--reuse", "Reuse existing branch instead of creating new")
  .option("--json", "Output as JSON")
  .action(async (branch, opts) => {
    const { worktreeCreate } = await import("./commands/worktree/create.js");
    await worktreeCreate(branch, opts);
  });

worktreeCmd
  .command("delete <target>")
  .description("Delete a worktree")
  .option("--repo <path>", "Repository path")
  .option("--force", "Force delete even with uncommitted changes")
  .option("--delete-branch", "Also delete the local branch")
  .option("--delete-remote", "Also delete the remote branch")
  .action(async (target, opts) => {
    const { worktreeDelete } = await import("./commands/worktree/delete.js");
    await worktreeDelete(target, opts);
  });

worktreeCmd
  .command("open <target>")
  .description("Open a worktree in IDE")
  .option("--repo <path>", "Repository path")
  .action(async (target, opts) => {
    const { worktreeOpen } = await import("./commands/worktree/open.js");
    await worktreeOpen(target, opts);
  });

worktreeCmd
  .command("sync")
  .description("Sync git worktree state with database")
  .option("--repo <path>", "Repository path")
  .action(async (opts) => {
    const { worktreeSync } = await import("./commands/worktree/sync.js");
    await worktreeSync(opts);
  });

worktreeCmd
  .command("info <target>")
  .description("Show detailed info for a worktree")
  .option("--repo <path>", "Repository path")
  .option("--json", "Output as JSON")
  .action(async (target, opts) => {
    const { worktreeInfo } = await import("./commands/worktree/info.js");
    await worktreeInfo(target, opts);
  });

// --- Repo commands ---

const repoCmd = program
  .command("repo")
  .description("Manage tracked repositories");

repoCmd
  .command("list")
  .description("List tracked repositories")
  .option("--json", "Output as JSON")
  .action(async (opts) => {
    const { repoList } = await import("./commands/repo/list.js");
    repoList(opts);
  });

repoCmd
  .command("add <path>")
  .description("Add a repository to track")
  .option("--json", "Output as JSON")
  .action(async (path, opts) => {
    const { repoAdd } = await import("./commands/repo/add.js");
    await repoAdd(path, opts);
  });

repoCmd
  .command("remove <name-or-path>")
  .description("Remove a tracked repository")
  .action(async (nameOrPath) => {
    const { repoRemove } = await import("./commands/repo/remove.js");
    repoRemove(nameOrPath);
  });

// --- Settings commands ---

const settingsCmd = program
  .command("settings")
  .description("View and modify settings");

settingsCmd
  .command("list")
  .description("Show all settings")
  .option("--json", "Output as JSON")
  .action(async (opts) => {
    const { settingsList } = await import("./commands/settings/list.js");
    settingsList(opts);
  });

settingsCmd
  .command("get <key>")
  .description("Get a setting value")
  .option("--json", "Output as JSON")
  .action(async (key, opts) => {
    const { settingsGet } = await import("./commands/settings/get.js");
    settingsGet(key, opts);
  });

settingsCmd
  .command("set <key> <value>")
  .description("Set a setting value")
  .action(async (key, value) => {
    const { settingsSet } = await import("./commands/settings/set.js");
    settingsSet(key, value);
  });

settingsCmd
  .command("reset")
  .description("Reset all settings to defaults")
  .action(async () => {
    const { settingsReset } = await import("./commands/settings/reset.js");
    settingsReset();
  });

// --- Hooks commands ---

const hooksCmd = program
  .command("hooks")
  .description("Manage Claude Code hooks");

hooksCmd
  .command("install")
  .description("Install Claude hooks into ~/.claude/settings.json")
  .action(async () => {
    const { hooksInstall } = await import("./commands/hooks.js");
    hooksInstall();
  });

hooksCmd
  .command("uninstall")
  .description("Remove agent-monitor hooks from ~/.claude/settings.json")
  .action(async () => {
    const { hooksUninstall } = await import("./commands/hooks.js");
    hooksUninstall();
  });

hooksCmd
  .command("status")
  .description("Check if hooks are installed")
  .option("--json", "Output as JSON")
  .action(async (opts) => {
    const { hooksStatus } = await import("./commands/hooks.js");
    hooksStatus(opts);
  });

// --- Backward compat aliases for hooks ---

program
  .command("install-hooks")
  .description("Install Claude hooks (alias for: hooks install)")
  .action(async () => {
    const { hooksInstall } = await import("./commands/hooks.js");
    hooksInstall();
  });

program
  .command("uninstall-hooks")
  .description("Remove Claude hooks (alias for: hooks uninstall)")
  .action(async () => {
    const { hooksUninstall } = await import("./commands/hooks.js");
    hooksUninstall();
  });

// --- PR commands ---

const prCmd = program
  .command("pr [target]")
  .description("Show PR info for a worktree branch")
  .option("--repo <path>", "Repository path")
  .option("--json", "Output as JSON")
  .action(async (target, opts) => {
    const { prShow } = await import("./commands/pr.js");
    await prShow(target, opts);
  });

prCmd
  .command("open [target]")
  .description("Open PR in browser")
  .option("--repo <path>", "Repository path")
  .action(async (target, opts) => {
    const { prOpen } = await import("./commands/pr.js");
    await prOpen(target, opts);
  });

// --- Linear commands ---

const linearCmd = program
  .command("linear [target]")
  .description("Show Linear ticket info for a worktree branch")
  .option("--repo <path>", "Repository path")
  .option("--json", "Output as JSON")
  .action(async (target, opts) => {
    const { linearShow } = await import("./commands/linear.js");
    await linearShow(target, opts);
  });

linearCmd
  .command("open [target]")
  .description("Open Linear ticket in browser")
  .option("--repo <path>", "Repository path")
  .action(async (target, opts) => {
    const { linearOpen } = await import("./commands/linear.js");
    await linearOpen(target, opts);
  });

// --- Script commands ---

const scriptCmd = program
  .command("script")
  .description("Manage startup scripts");

scriptCmd
  .command("edit")
  .description("Create/open startup script in IDE")
  .option("--repo <path>", "Repository path")
  .action(async (opts) => {
    const { scriptEdit } = await import("./commands/script.js");
    scriptEdit(opts);
  });

scriptCmd
  .command("remove")
  .description("Remove startup script")
  .option("--repo <path>", "Repository path")
  .action(async (opts) => {
    const { scriptRemove } = await import("./commands/script.js");
    scriptRemove(opts);
  });

scriptCmd
  .command("show")
  .description("Print startup script contents")
  .option("--repo <path>", "Repository path")
  .option("--json", "Output as JSON")
  .action(async (opts) => {
    const { scriptShow } = await import("./commands/script.js");
    scriptShow(opts);
  });

// --- Daemon commands ---

const daemonCmd = program
  .command("daemon")
  .description("Manage the background daemon");

daemonCmd
  .command("start")
  .description("Start the background daemon")
  .action(async () => {
    const { daemonStart } = await import("./commands/daemon.js");
    daemonStart();
  });

daemonCmd
  .command("stop")
  .description("Stop the background daemon")
  .action(async () => {
    const { daemonStop } = await import("./commands/daemon.js");
    daemonStop();
  });

daemonCmd
  .command("status")
  .description("Show daemon status")
  .action(async () => {
    const { daemonStatus } = await import("./commands/daemon.js");
    daemonStatus();
  });

// --- Doctor ---

program
  .command("doctor")
  .description("Check system health (hooks, gh CLI, DB, etc.)")
  .option("--json", "Output as JSON")
  .action(async (opts) => {
    const { doctor } = await import("./commands/doctor.js");
    doctor(opts);
  });

// --- Logs command (from main) ---

program
  .command("logs")
  .description("Show recent logs")
  .option("-n, --lines <number>", "Number of log lines to show", "50")
  .option("-f, --follow", "Follow log output")
  .option("--level <level>", "Filter by level (debug, info, warn, error)")
  .option("--module <module>", "Filter by module")
  .option("--clear", "Clear the log file")
  .action(async (opts) => {
    const { printLogs } = await import("./commands/logs.js");
    printLogs({
      lines: parseInt(opts.lines, 10),
      follow: opts.follow ?? false,
      level: opts.level,
      module: opts.module,
      clear: opts.clear ?? false,
    });
  });

// --- Setup wizard ---

program
  .command("setup")
  .description("Run the setup wizard")
  .action(() => {
    process.stdout.write("\x1B[2J\x1B[3J\x1B[H");
    launchTui(true);
  });

// --- Existing commands (backward compat) ---

program
  .command("status")
  .description("Get/set agent status for a worktree")
  .requiredOption("-w, --worktree <path>", "Worktree path")
  .option("--set <status>", "Set agent status (idle, executing, planning, waiting)")
  .action(async (opts) => {
    const { printStatus } = await import("./commands/status.js");
    await printStatus(opts.worktree, opts.set);
  });

program
  .command("hook-event")
  .description("Receive hook event from stdin (internal)")
  .requiredOption("-w, --worktree <path>", "Worktree path")
  .option("-e, --event <name>", "Event name override")
  .action(async (opts) => {
    const { handleHookEvent } = await import("./commands/hook-event.js");
    await handleHookEvent(opts.worktree, opts.event);
  });

// --- Short aliases ---

program
  .command("ls")
  .description("List worktrees (alias for: worktree list)")
  .option("--repo <path>", "Repository path")
  .option("--json", "Output as JSON")
  .action(async (opts) => {
    const { worktreeList } = await import("./commands/worktree/list.js");
    await worktreeList(opts);
  });

program
  .command("new <branch>")
  .description("Create worktree (alias for: worktree create)")
  .option("--repo <path>", "Repository path")
  .option("--base <branch>", "Base branch")
  .option("--reuse", "Reuse existing branch")
  .option("--json", "Output as JSON")
  .action(async (branch, opts) => {
    const { worktreeCreate } = await import("./commands/worktree/create.js");
    await worktreeCreate(branch, opts);
  });

program
  .command("open <target>")
  .description("Open worktree in IDE (alias for: worktree open)")
  .option("--repo <path>", "Repository path")
  .action(async (target, opts) => {
    const { worktreeOpen } = await import("./commands/worktree/open.js");
    await worktreeOpen(target, opts);
  });

// --- TUI launcher ---

async function launchTui(forceSetup = false): Promise<void> {
  let pendingScript: { scriptPath: string; cwd: string } | null = null;
  let pendingUpdate = false;

  const onRunScript = (scriptPath: string, cwd: string) => {
    pendingScript = { scriptPath, cwd };
  };

  const onUpdate = () => {
    pendingUpdate = true;
  };

  // eslint-disable-next-line no-constant-condition
  while (true) {
    const instance = render(
      <App onRunScript={onRunScript} watch={watchFlag} onUpdate={onUpdate} forceSetup={forceSetup} />,
      { patchConsole: true }
    );

    try {
      await instance.waitUntilExit();
    } catch (err) {
      log("debug", "cli", `TUI exit: ${err}`);
    }

    instance.unmount();
    instance.clear();

    if (pendingUpdate) {
      pendingUpdate = false;
      const pmInfo = detectPackageManager();
      console.log(`Updating Agent Monitor via ${pmInfo.command}...\n`);

      try {
        if (pmInfo.setup) {
          for (const step of pmInfo.setup) {
            execFileSync(step.command, step.args, { stdio: "inherit" });
          }
        }
        execFileSync(pmInfo.command, pmInfo.args, { stdio: "inherit" });
      } catch (err) {
        console.error(`\nUpdate failed: ${err}\n`);
        waitForEnter();
        process.stdout.write("\x1B[2J\x1B[3J\x1B[H");
        continue;
      }

      // Clear update cache so next launch does a fresh check
      const freshSettings = loadSettings();
      saveSettings({
        ...freshSettings,
        lastUpdateCheck: undefined,
        latestKnownVersion: undefined,
      });

      console.log("\nUpdate complete! Please restart am.\n");
      process.exit(0);
    }

    if (!pendingScript) {
      process.exit(0);
    }

    const { scriptPath, cwd } = pendingScript;
    pendingScript = null;

    runScript(scriptPath, cwd);
    waitForEnter();

    process.stdout.write("\x1B[2J\x1B[3J\x1B[H");
  }
}

// --- Parse & run ---

program.parseAsync(process.argv).catch((err) => {
  console.error(err.message ?? err);
  process.exit(1);
});
