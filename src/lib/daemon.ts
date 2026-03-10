/**
 * Background daemon service for agent-monitor.
 *
 * Runs as a detached background process. Owns the Unix domain socket,
 * receives hook-event messages, runs all expensive polling (git, lsof, ps, APIs),
 * and broadcasts enriched data to subscribed TUI clients.
 *
 * Can be run directly: `tsx src/lib/daemon.ts`
 */
import net from "net";
import { existsSync, unlinkSync, writeFileSync, readFileSync, mkdirSync } from "fs";
import { SOCKET_PATH, DAEMON_PID_PATH, APP_DIR } from "./paths.js";
import { initLogger, log } from "./logger.js";
import { loadSettings } from "./settings.js";
import { getDb, getRepositories, getWorktrees, getAgentStatuses, getStandaloneSessions, pruneStaleStandaloneSessions, updateWorktreeCustomName, clearLinearNicknames } from "./db.js";
import { getGitStatus, getLastCommit } from "./git.js";
import { fetchAllPrInfo } from "./github.js";
import { fetchLinearInfo, linearAttachmentToPrInfo } from "./linear.js";
import { getTerminalPathsAsync, getIdePathsAsync } from "./process.js";
import { isEffectivelyOpenStandalone } from "./agent-utils.js";
import { realpathSync } from "fs";
import type { PubSubMessage } from "./pubsub-types.js";
import type {
  DaemonInboundMessage,
  DaemonData,
  RefreshResultMessage,
  AgentUpdatePassthroughMessage,
  DaemonToTuiMessage,
} from "./daemon-types.js";
import type { WorktreeGroup, WorktreeWithStatus, PrInfo, LinearInfo, Repository, Settings, StandaloneSession } from "./types.js";
import { getVersion } from "./version.js";

// --- State ---

let settings: Settings;
let repositories: Repository[];

// Caches (moved from useWorktrees refs)
const prCache = new Map<string, PrInfo | null>();
const prNumberCache = new Map<string, number>();
const linearCache = new Map<string, LinearInfo | null>();

// TUI subscriber connections
const tuiClients = new Set<net.Socket>();

// Timers
let mainPollTimer: ReturnType<typeof setInterval> | null = null;
let ghPollTimer: ReturnType<typeof setInterval> | null = null;
let linearPollTimer: ReturnType<typeof setInterval> | null = null;
let gracePeriodTimer: ReturnType<typeof setTimeout> | null = null;

const GRACE_PERIOD_MS = 30_000;

let server: net.Server | null = null;
let lastBroadcastFingerprint = "";

// --- Socket server ---

function startDaemonServer(): net.Server {
  // Clean up stale socket
  if (existsSync(SOCKET_PATH)) {
    try {
      unlinkSync(SOCKET_PATH);
    } catch {
      // ignore
    }
  }

  const srv = net.createServer((conn) => {
    let buffer = "";
    let isSubscriber = false;

    conn.on("data", (chunk) => {
      buffer += chunk.toString();
      const lines = buffer.split("\n");
      buffer = lines.pop()!;

      for (const line of lines) {
        if (!line.trim()) continue;
        try {
          const msg = JSON.parse(line) as DaemonInboundMessage;
          handleInboundMessage(msg, conn);
          if (msg.type === "subscribe") {
            isSubscriber = true;
          }
        } catch {
          log("debug", "daemon", `Failed to parse message: ${line.slice(0, 200)}`);
        }
      }
    });

    conn.on("error", () => {
      if (isSubscriber) {
        tuiClients.delete(conn);
        onTuiDisconnected();
      }
    });

    conn.on("close", () => {
      if (isSubscriber) {
        tuiClients.delete(conn);
        onTuiDisconnected();
      }
    });
  });

  srv.on("error", (err) => {
    log("error", "daemon", `Server error: ${err.message}`);
  });

  srv.listen(SOCKET_PATH);
  log("info", "daemon", `Daemon listening on ${SOCKET_PATH}`);
  return srv;
}

function handleInboundMessage(msg: DaemonInboundMessage, conn: net.Socket): void {
  switch (msg.type) {
    case "subscribe":
      tuiClients.add(conn);
      cancelGracePeriod();
      log("info", "daemon", `TUI subscribed (${tuiClients.size} clients)`);
      // Send current data immediately
      doRefresh(null, false);
      break;

    case "force-refresh":
      log("debug", "daemon", `Force refresh requested (id=${msg.id}, integrations=${msg.includeIntegrations})`);
      doRefresh(msg.id, msg.includeIntegrations);
      break;

    case "config-reload":
      log("info", "daemon", "Config reload requested");
      reloadConfig();
      doRefresh(null, false);
      break;

    // Passthrough from hook-event
    case "agent-status-update":
    case "standalone-status-update":
    case "git-activity":
      handleHookMessage(msg);
      break;
  }
}

function handleHookMessage(msg: PubSubMessage): void {
  // Broadcast to all TUI clients so they can track unseen status
  const passthrough: AgentUpdatePassthroughMessage = {
    type: "agent-update",
    original: msg,
  };
  broadcast(passthrough);

  // Trigger a refresh
  if (msg.type === "agent-status-update" || msg.type === "standalone-status-update") {
    // Light refresh — no integration calls
    doRefresh(null, false);
  } else if (msg.type === "git-activity") {
    // Delayed full refresh to give GitHub time to process
    log("info", "daemon", `Git activity: ${msg.activity} on ${msg.branch}`);
    setTimeout(() => doRefresh(null, true), 3000);
  }
}

function broadcast(msg: DaemonToTuiMessage): void {
  const payload = JSON.stringify(msg) + "\n";
  for (const client of tuiClients) {
    try {
      client.write(payload);
    } catch {
      // Client disconnected, will be cleaned up on close event
    }
  }
}

function onTuiDisconnected(): void {
  log("info", "daemon", `TUI disconnected (${tuiClients.size} clients remaining)`);
  if (tuiClients.size === 0) {
    startGracePeriod();
  }
}

function startGracePeriod(): void {
  cancelGracePeriod();
  log("info", "daemon", `No TUI clients — starting ${GRACE_PERIOD_MS / 1000}s grace period`);
  gracePeriodTimer = setTimeout(() => {
    if (tuiClients.size === 0) {
      log("info", "daemon", "Grace period expired with no TUI clients — shutting down");
      shutdown();
    }
  }, GRACE_PERIOD_MS);
}

function cancelGracePeriod(): void {
  if (gracePeriodTimer) {
    clearTimeout(gracePeriodTimer);
    gracePeriodTimer = null;
  }
}

// --- Config ---

function reloadConfig(): void {
  settings = loadSettings();
  repositories = getRepositories();
  restartPolling();
}

// --- Polling ---

function restartPolling(): void {
  if (mainPollTimer) clearInterval(mainPollTimer);
  if (ghPollTimer) clearInterval(ghPollTimer);
  if (linearPollTimer) clearInterval(linearPollTimer);

  // Main poll
  mainPollTimer = setInterval(() => doRefresh(null, false), settings.pollingIntervalMs);

  // GitHub PR poll
  if (settings.ghPrStatus && repositories.length > 0) {
    const doGhPoll = async () => {
      await refreshPrInfo();
      doRefresh(null, false);
    };
    doGhPoll();
    ghPollTimer = setInterval(doGhPoll, settings.ghPollingIntervalMs);
  }

  // Linear poll
  if (settings.linearEnabled && repositories.length > 0) {
    const doLinearPoll = async () => {
      await refreshLinearInfoAll();
      autoSetLinearNicknames();
      doRefresh(null, false);
    };
    doLinearPoll();
    linearPollTimer = setInterval(doLinearPoll, settings.linearPollingIntervalMs);
  }
}

// --- Integration fetching ---

async function refreshPrInfo(): Promise<void> {
  const repoGroups: Array<{ repoPath: string; repoId: string; branches: string[] }> = [];
  for (const repo of repositories) {
    const dbWorktrees = getWorktrees(repo.id);
    const branches = dbWorktrees
      .map((wt) => wt.branch)
      .filter((b) => !linearCache.get(b)?.prAttachment);
    repoGroups.push({ repoPath: repo.path, repoId: repo.id, branches });
  }

  await Promise.all(
    repoGroups.map(async ({ repoPath, repoId, branches }) => {
      if (branches.length === 0) return;
      const repoPrNumbers = new Map<string, number>();
      for (const branch of branches) {
        const num = prNumberCache.get(`${repoId}:${branch}`);
        if (num != null) repoPrNumbers.set(branch, num);
      }
      try {
        const repoPrCache = new Map<string, PrInfo | null>();
        for (const branch of branches) {
          const cacheKey = `${repoId}:${branch}`;
          if (prCache.has(cacheKey)) {
            repoPrCache.set(branch, prCache.get(cacheKey)!);
          }
        }
        const prMap = await fetchAllPrInfo(repoPath, branches, repoPrNumbers, repoPrCache);
        for (const [branch, info] of prMap) {
          const cacheKey = `${repoId}:${branch}`;
          if (info !== null || !prCache.has(cacheKey)) {
            prCache.set(cacheKey, info);
          }
          if (info?.number != null) {
            prNumberCache.set(cacheKey, info.number);
          }
        }
      } catch (err) {
        log("warn", "daemon", `Batch PR fetch failed for repo ${repoId}: ${err}`);
      }
    })
  );
}

async function refreshLinearInfoAll(): Promise<void> {
  const allBranches: string[] = [];
  for (const repo of repositories) {
    const dbWorktrees = getWorktrees(repo.id);
    for (const wt of dbWorktrees) {
      allBranches.push(wt.branch);
    }
  }

  if (allBranches.length === 0) return;

  const entries = await Promise.all(
    allBranches.map(async (branch) => {
      try {
        const info = await fetchLinearInfo(branch, settings.linearApiKey);
        return [branch, info] as const;
      } catch {
        return [branch, linearCache.get(branch) ?? null] as const;
      }
    })
  );
  for (const [branch, info] of entries) {
    linearCache.set(branch, info);
  }
}

function autoSetLinearNicknames(): void {
  if (!settings.linearAutoNickname || !settings.linearEnabled) return;
  for (const repo of repositories) {
    const dbWorktrees = getWorktrees(repo.id);
    for (const wt of dbWorktrees) {
      if (wt.custom_name) continue;
      const linearInfo = linearCache.get(wt.branch);
      if (!linearInfo) continue;
      log("info", "daemon", `Auto-setting nickname for ${wt.branch} from Linear: "${linearInfo.title}"`);
      updateWorktreeCustomName(wt.id, linearInfo.title, "linear");
    }
  }
}

// --- Refresh & broadcast ---

let refreshInProgress = false;

async function doRefresh(requestId: string | null, includeIntegrations: boolean): Promise<void> {
  // Prevent concurrent refreshes from stacking up
  if (refreshInProgress) return;
  refreshInProgress = true;

  try {
    // Re-read repos from DB each time (cheap, synchronous)
    repositories = getRepositories();

    if (includeIntegrations) {
      const prPromise = settings.ghPrStatus ? refreshPrInfo() : Promise.resolve();
      const linearPromise = settings.linearEnabled ? refreshLinearInfoAll() : Promise.resolve();
      await Promise.all([prPromise, linearPromise]);
      autoSetLinearNicknames();
    }

    const data = await buildData();

    // Fingerprint check — skip broadcast if nothing changed and this isn't a targeted request
    const fingerprint = JSON.stringify(data);
    if (requestId === null && fingerprint === lastBroadcastFingerprint) {
      return;
    }
    lastBroadcastFingerprint = fingerprint;

    const msg: RefreshResultMessage = {
      type: "refresh-result",
      id: requestId,
      data,
    };
    broadcast(msg);
  } catch (err) {
    log("error", "daemon", `Refresh failed: ${err}`);
  } finally {
    refreshInProgress = false;
  }
}

async function buildData(): Promise<DaemonData> {
  const hideMain = settings.hideMainBranch;
  const newGroups: WorktreeGroup[] = [];
  const allFlat: WorktreeWithStatus[] = [];

  // Async terminal/IDE detection
  const [terminalPaths, idePaths] = await Promise.all([
    getTerminalPathsAsync(),
    getIdePathsAsync(),
  ]);

  for (const repo of repositories) {
    const dbWorktrees = getWorktrees(repo.id);
    const statuses = getAgentStatuses(repo.id);

    const enriched: WorktreeWithStatus[] = await Promise.all(
      dbWorktrees.map(async (wt) => {
        let git_status = null;
        let last_commit = null;
        try {
          [git_status, last_commit] = await Promise.all([
            getGitStatus(wt.path),
            getLastCommit(wt.path),
          ]);
        } catch (err) {
          log("warn", "daemon", `Failed to get git info for ${wt.path}: ${err}`);
        }

        let has_terminal = false;
        let open_ide: "cursor" | "vscode" | null = null;
        try {
          const realPath = realpathSync(wt.path);
          has_terminal = terminalPaths.has(realPath);
          open_ide = idePaths.get(realPath) ?? null;
        } catch {
          // path doesn't exist or can't be resolved
        }

        return {
          ...wt,
          agent_status: statuses.get(wt.id) ?? null,
          git_status,
          last_commit,
          has_terminal,
          open_ide,
          pr_info: (() => {
            const linearInfo = linearCache.get(wt.branch);
            if (linearInfo?.prAttachment) return linearAttachmentToPrInfo(linearInfo.prAttachment);
            return prCache.get(`${repo.id}:${wt.branch}`) ?? null;
          })(),
          linear_info: linearCache.get(wt.branch) ?? null,
        };
      })
    );

    enriched.sort((a, b) => {
      if (a.is_main !== b.is_main) return a.is_main - b.is_main;
      return b.created_at.localeCompare(a.created_at);
    });

    const filtered = hideMain
      ? enriched.filter((wt) => !(wt.is_main === 1 && (wt.branch === "main" || wt.branch === "master")))
      : enriched;

    if (filtered.length > 0 || repositories.length === 1) {
      newGroups.push({ repo, worktrees: filtered });
    }
    allFlat.push(...filtered);
  }

  // Standalone sessions
  const allSessions = getStandaloneSessions();
  const visibleSessions = allSessions.filter(
    (s) => s.is_open === 1 || isEffectivelyOpenStandalone(s)
  );

  return {
    groups: newGroups,
    flatWorktrees: allFlat,
    standaloneSessions: visibleSessions,
  };
}

// --- Lifecycle ---

function writePidFile(): void {
  if (!existsSync(APP_DIR)) {
    mkdirSync(APP_DIR, { recursive: true });
  }
  writeFileSync(DAEMON_PID_PATH, String(process.pid));
}

function removePidFile(): void {
  try {
    if (existsSync(DAEMON_PID_PATH)) {
      unlinkSync(DAEMON_PID_PATH);
    }
  } catch {
    // ignore
  }
}

function shutdown(): void {
  log("info", "daemon", "Shutting down daemon");

  if (mainPollTimer) clearInterval(mainPollTimer);
  if (ghPollTimer) clearInterval(ghPollTimer);
  if (linearPollTimer) clearInterval(linearPollTimer);
  cancelGracePeriod();

  // Close all client connections
  for (const client of tuiClients) {
    try { client.end(); } catch { /* ignore */ }
  }
  tuiClients.clear();

  if (server) {
    try { server.close(); } catch { /* ignore */ }
  }

  try {
    if (existsSync(SOCKET_PATH)) unlinkSync(SOCKET_PATH);
  } catch { /* ignore */ }

  removePidFile();
  process.exit(0);
}

// --- PID file utilities (exported for daemon-client and CLI) ---

export function getDaemonPid(): number | null {
  try {
    if (!existsSync(DAEMON_PID_PATH)) return null;
    const pid = parseInt(readFileSync(DAEMON_PID_PATH, "utf-8").trim(), 10);
    if (isNaN(pid)) return null;
    // Check if process is running
    try {
      process.kill(pid, 0);
      return pid;
    } catch {
      // Process not running — stale PID file
      removePidFile();
      return null;
    }
  } catch {
    return null;
  }
}

export function isDaemonRunning(): boolean {
  return getDaemonPid() !== null;
}

export function stopDaemon(): boolean {
  const pid = getDaemonPid();
  if (pid === null) return false;
  try {
    process.kill(pid, "SIGTERM");
    removePidFile();
    return true;
  } catch {
    removePidFile();
    return false;
  }
}

// --- Main entry point (when run as a script) ---

function main(): void {
  const version = getVersion();
  settings = loadSettings();
  initLogger(settings.logLevel, version, settings.maxLogSizeMb);

  log("info", "daemon", `Starting daemon v${version} (pid=${process.pid})`);

  // Initialize DB
  getDb();
  repositories = getRepositories();

  // Prune stale standalone sessions
  pruneStaleStandaloneSessions();

  // Clear Linear nicknames if feature is disabled
  if (!settings.linearEnabled || !settings.linearAutoNickname) {
    clearLinearNicknames();
  }

  // Write PID file
  writePidFile();

  // Start socket server
  server = startDaemonServer();

  // Start polling
  restartPolling();

  // Handle signals
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
  process.on("exit", () => {
    removePidFile();
    try {
      if (existsSync(SOCKET_PATH)) unlinkSync(SOCKET_PATH);
    } catch { /* ignore */ }
  });

  // Start grace period immediately — daemon shuts down if no TUI connects within 30s
  startGracePeriod();
}

// Run if executed directly
const isDirectRun = process.argv[1]?.endsWith("daemon.ts") ||
  process.argv[1]?.endsWith("daemon.js") ||
  process.env.AM_DAEMON_MODE === "1";

if (isDirectRun) {
  main();
}

export { main as startDaemonProcess };
