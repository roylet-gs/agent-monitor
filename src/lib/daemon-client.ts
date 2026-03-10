/**
 * Persistent TUI subscriber client for communicating with the daemon.
 *
 * Maintains a persistent connection, sends subscribe/force-refresh/config-reload
 * messages, and receives push updates (refresh-result, agent-update).
 * Auto-reconnects on disconnect with exponential backoff.
 */
import net from "net";
import { existsSync } from "fs";
import { spawn } from "child_process";
import { fileURLToPath } from "url";
import { dirname, join } from "path";
import { SOCKET_PATH } from "./paths.js";
import { isDaemonRunning } from "./daemon.js";
import { log } from "./logger.js";
import type { DaemonToTuiMessage } from "./daemon-types.js";
import type { ForceRefreshMessage, ConfigReloadMessage } from "./daemon-types.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

export interface DaemonClientOptions {
  onData: (msg: DaemonToTuiMessage) => void;
  onConnected?: () => void;
  onDisconnected?: () => void;
}

export class DaemonClient {
  private conn: net.Socket | null = null;
  private buffer = "";
  private options: DaemonClientOptions;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectDelay = 500;
  private maxReconnectDelay = 10_000;
  private destroyed = false;
  private _connected = false;
  private pendingResolvers = new Map<string, () => void>();

  constructor(options: DaemonClientOptions) {
    this.options = options;
  }

  get connected(): boolean {
    return this._connected;
  }

  /**
   * Ensure the daemon is running and connect to it.
   * Returns true if connection succeeds, false if daemon can't be reached.
   */
  async connect(): Promise<boolean> {
    if (this.destroyed) return false;

    // Start daemon if not running
    if (!isDaemonRunning()) {
      await this.startDaemon();
      // Wait a moment for daemon to start listening
      await new Promise((r) => setTimeout(r, 500));
    }

    return this.doConnect();
  }

  private doConnect(): Promise<boolean> {
    return new Promise((resolve) => {
      if (this.destroyed) {
        resolve(false);
        return;
      }

      if (!existsSync(SOCKET_PATH)) {
        resolve(false);
        return;
      }

      const conn = net.createConnection(SOCKET_PATH, () => {
        this.conn = conn;
        this._connected = true;
        this.reconnectDelay = 500;
        log("info", "daemon-client", "Connected to daemon");

        // Send subscribe message
        conn.write(JSON.stringify({ type: "subscribe" }) + "\n");

        this.options.onConnected?.();
        resolve(true);
      });

      conn.on("data", (chunk) => {
        this.buffer += chunk.toString();
        const lines = this.buffer.split("\n");
        this.buffer = lines.pop()!;

        for (const line of lines) {
          if (!line.trim()) continue;
          try {
            const msg = JSON.parse(line) as DaemonToTuiMessage;
            // Resolve pending force-refresh promises
            if (msg.type === "refresh-result" && msg.id) {
              const resolver = this.pendingResolvers.get(msg.id);
              if (resolver) {
                this.pendingResolvers.delete(msg.id);
                resolver();
              }
            }
            this.options.onData(msg);
          } catch {
            log("debug", "daemon-client", `Failed to parse: ${line.slice(0, 200)}`);
          }
        }
      });

      conn.on("error", (err) => {
        log("debug", "daemon-client", `Connection error: ${err.message}`);
        resolve(false);
      });

      conn.on("close", () => {
        this._connected = false;
        this.conn = null;
        this.buffer = "";

        // Reject all pending resolvers
        for (const resolver of this.pendingResolvers.values()) {
          resolver();
        }
        this.pendingResolvers.clear();

        this.options.onDisconnected?.();

        if (!this.destroyed) {
          this.scheduleReconnect();
        }
      });

      conn.setTimeout(5000, () => {
        conn.destroy();
        resolve(false);
      });
    });
  }

  private scheduleReconnect(): void {
    if (this.destroyed || this.reconnectTimer) return;

    log("debug", "daemon-client", `Reconnecting in ${this.reconnectDelay}ms`);
    this.reconnectTimer = setTimeout(async () => {
      this.reconnectTimer = null;
      if (this.destroyed) return;

      // Try to start daemon if not running
      if (!isDaemonRunning()) {
        await this.startDaemon();
        await new Promise((r) => setTimeout(r, 500));
      }

      this.doConnect().then((ok) => {
        if (!ok && !this.destroyed) {
          this.reconnectDelay = Math.min(this.reconnectDelay * 2, this.maxReconnectDelay);
          this.scheduleReconnect();
        }
      });
    }, this.reconnectDelay);
  }

  /**
   * Send a force-refresh request to the daemon.
   * Returns a promise that resolves when the daemon responds with the matching refresh-result.
   */
  forceRefresh(includeIntegrations: boolean): Promise<void> {
    const id = `fr-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const msg: ForceRefreshMessage = {
      type: "force-refresh",
      id,
      includeIntegrations,
    };

    return new Promise<void>((resolve) => {
      if (!this.conn || !this._connected) {
        resolve();
        return;
      }

      // Set up resolver with timeout
      const timer = setTimeout(() => {
        this.pendingResolvers.delete(id);
        resolve();
      }, 15_000);

      this.pendingResolvers.set(id, () => {
        clearTimeout(timer);
        resolve();
      });

      try {
        this.conn.write(JSON.stringify(msg) + "\n");
      } catch {
        this.pendingResolvers.delete(id);
        clearTimeout(timer);
        resolve();
      }
    });
  }

  /**
   * Tell the daemon to reload config from SQLite/settings.
   */
  configReload(): void {
    if (!this.conn || !this._connected) return;
    const msg: ConfigReloadMessage = { type: "config-reload" };
    try {
      this.conn.write(JSON.stringify(msg) + "\n");
    } catch {
      // ignore
    }
  }

  /**
   * Disconnect and stop reconnecting.
   */
  destroy(): void {
    this.destroyed = true;
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.conn) {
      try { this.conn.end(); } catch { /* ignore */ }
      this.conn = null;
    }
    this._connected = false;

    // Resolve all pending
    for (const resolver of this.pendingResolvers.values()) {
      resolver();
    }
    this.pendingResolvers.clear();
  }

  /**
   * Start the daemon as a detached background process.
   */
  private async startDaemon(): Promise<void> {
    log("info", "daemon-client", "Starting daemon process");
    try {
      // Find the daemon script
      const daemonScript = join(__dirname, "..", "lib", "daemon.js");
      const daemonTsScript = join(__dirname, "..", "lib", "daemon.ts");

      let cmd: string;
      let args: string[];

      if (existsSync(daemonScript)) {
        // Built version — use node directly
        cmd = process.execPath;
        args = [daemonScript];
      } else if (existsSync(daemonTsScript)) {
        // Dev mode — use tsx
        cmd = "npx";
        args = ["tsx", daemonTsScript];
      } else {
        log("error", "daemon-client", "Cannot find daemon script");
        return;
      }

      const child = spawn(cmd, args, {
        detached: true,
        stdio: "ignore",
        env: { ...process.env, AM_DAEMON_MODE: "1" },
      });
      child.unref();
      log("info", "daemon-client", `Daemon spawned (pid=${child.pid})`);
    } catch (err) {
      log("error", "daemon-client", `Failed to start daemon: ${err}`);
    }
  }
}
