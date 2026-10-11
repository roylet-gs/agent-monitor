#!/usr/bin/env node
// Launcher for the native `am` binary. The binaries ship in a per-platform
// package (installed as an optional dependency, so npm only fetches the one
// matching this machine); this script just finds it and hands over.
// Claude Code hooks call `am-hook` by absolute path, so this Node shim is
// never on the per-event path.
"use strict";

const { spawnSync } = require("node:child_process");
const path = require("node:path");

const pkg = `@roylet-gs/agent-monitor-${process.platform}-${process.arch}`;

function binDir() {
  if (process.env.AM_BINARY_DIR) return process.env.AM_BINARY_DIR;
  try {
    return path.join(path.dirname(require.resolve(`${pkg}/package.json`)), "bin");
  } catch {
    return null;
  }
}

const dir = binDir();
if (!dir) {
  console.error(
    `agent-monitor: no native build for ${process.platform}-${process.arch} (${pkg} is not installed).\n` +
      "Supported: macOS (arm64, x64) and Linux (arm64, x64). If you are on one of these,\n" +
      "reinstall without --no-optional: npm i -g @roylet-gs/agent-monitor"
  );
  process.exit(1);
}

const result = spawnSync(path.join(dir, "am"), process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`agent-monitor: failed to run ${path.join(dir, "am")}: ${result.error.message}`);
  process.exit(1);
}
if (result.signal) process.kill(process.pid, result.signal);
process.exit(result.status ?? 1);
