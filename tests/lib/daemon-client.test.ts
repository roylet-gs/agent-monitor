import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import net from "net";
import { mkdtempSync, unlinkSync, writeFileSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";

const { testSocketPath, tempDir } = vi.hoisted(() => {
  const { mkdtempSync } = require("fs");
  const { tmpdir } = require("os");
  const { join } = require("path");
  const tempDir = mkdtempSync(join(tmpdir(), "am-dclient-test-"));
  return { testSocketPath: join(tempDir, "am.sock"), tempDir };
});

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/paths.js", () => ({
  SOCKET_PATH: testSocketPath,
  APP_DIR: tempDir,
  DB_PATH: join(tempDir, "agent-monitor.db"),
  SETTINGS_PATH: join(tempDir, "settings.json"),
  LOG_PATH: join(tempDir, "debug.log"),
  RULES_PATH: join(tempDir, "rules.json"),
  AM_MANAGED_PERMISSIONS_PATH: join(tempDir, "am-managed-permissions.json"),
  DAEMON_PID_PATH: join(tempDir, "am.daemon.pid"),
}));

import { DaemonClient } from "../../src/lib/daemon-client.js";
import type { DaemonToTuiMessage } from "../../src/lib/daemon-types.js";

const testPidPath = join(tempDir, "am.daemon.pid");

describe("DaemonClient", () => {
  let mockServer: net.Server | null = null;

  function startMockServer(): Promise<net.Server> {
    return new Promise((resolve) => {
      const srv = net.createServer();
      srv.listen(testSocketPath, () => resolve(srv));
    });
  }

  beforeEach(() => {
    // Create a fake PID file so the client will attempt to connect
    writeFileSync(testPidPath, String(process.pid));
  });

  afterEach(() => {
    if (mockServer) {
      mockServer.close();
      mockServer = null;
      try { unlinkSync(testSocketPath); } catch { /* ignore */ }
    }
    try { unlinkSync(testPidPath); } catch { /* ignore */ }
  });

  it("connects to a running daemon and sends subscribe", async () => {
    mockServer = await startMockServer();

    const received: string[] = [];
    mockServer.on("connection", (conn) => {
      conn.on("data", (chunk) => {
        received.push(chunk.toString().trim());
      });
    });

    const messages: DaemonToTuiMessage[] = [];
    const client = new DaemonClient({
      onData: (msg) => messages.push(msg),
    });

    const ok = await client.connect();
    expect(ok).toBe(true);
    expect(client.connected).toBe(true);

    // Wait for subscribe message to arrive
    await new Promise((r) => setTimeout(r, 50));
    expect(received.length).toBeGreaterThanOrEqual(1);
    expect(JSON.parse(received[0])).toEqual({ type: "subscribe" });

    client.destroy();
    expect(client.connected).toBe(false);
  });

  it("receives messages from daemon", async () => {
    mockServer = await startMockServer();

    let serverConn: net.Socket | null = null;
    mockServer.on("connection", (conn) => {
      serverConn = conn;
    });

    const messages: DaemonToTuiMessage[] = [];
    const client = new DaemonClient({
      onData: (msg) => messages.push(msg),
    });

    await client.connect();
    await new Promise((r) => setTimeout(r, 50));

    // Send a refresh-result from server
    const msg: DaemonToTuiMessage = {
      type: "refresh-result",
      id: null,
      data: { groups: [], flatWorktrees: [], standaloneSessions: [] },
    };
    serverConn!.write(JSON.stringify(msg) + "\n");
    await new Promise((r) => setTimeout(r, 50));

    expect(messages.length).toBe(1);
    expect(messages[0].type).toBe("refresh-result");

    client.destroy();
  });

  it("forceRefresh sends message and resolves on response", async () => {
    mockServer = await startMockServer();

    mockServer.on("connection", (conn) => {
      let buffer = "";
      conn.on("data", (chunk) => {
        buffer += chunk.toString();
        const lines = buffer.split("\n");
        buffer = lines.pop()!;
        for (const line of lines) {
          if (!line.trim()) continue;
          const parsed = JSON.parse(line);
          if (parsed.type === "force-refresh") {
            const response: DaemonToTuiMessage = {
              type: "refresh-result",
              id: parsed.id,
              data: { groups: [], flatWorktrees: [], standaloneSessions: [] },
            };
            conn.write(JSON.stringify(response) + "\n");
          }
        }
      });
    });

    const client = new DaemonClient({ onData: () => {} });
    await client.connect();
    await new Promise((r) => setTimeout(r, 50));

    await client.forceRefresh(true);

    client.destroy();
  });

  it("returns false when no daemon PID file exists", async () => {
    try { unlinkSync(testPidPath); } catch { /* ignore */ }
    const client = new DaemonClient({ onData: () => {} });
    const ok = await client.connect();
    expect(ok).toBe(false);
    client.destroy();
  });

  it("configReload sends message without error", async () => {
    mockServer = await startMockServer();

    const received: string[] = [];
    mockServer.on("connection", (conn) => {
      conn.on("data", (chunk) => {
        for (const line of chunk.toString().split("\n")) {
          if (line.trim()) received.push(line.trim());
        }
      });
    });

    const client = new DaemonClient({ onData: () => {} });
    await client.connect();
    await new Promise((r) => setTimeout(r, 50));

    client.configReload();
    await new Promise((r) => setTimeout(r, 50));

    const configMsg = received.find((r) => JSON.parse(r).type === "config-reload");
    expect(configMsg).toBeDefined();

    client.destroy();
  });
});
