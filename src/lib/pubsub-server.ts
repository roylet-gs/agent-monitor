import net from "net";
import { existsSync, unlinkSync } from "fs";
import { SOCKET_PATH } from "./paths.js";
import { log } from "./logger.js";
import type { PubSubMessage } from "./pubsub-types.js";

/**
 * Start a Unix domain socket server for receiving pub/sub messages.
 * Returns the server, or null if another TUI already owns the socket.
 */
export async function startPubSubServer(
  onMessage: (msg: PubSubMessage) => void
): Promise<net.Server | null> {
  // If socket file exists, check if it's stale
  if (existsSync(SOCKET_PATH)) {
    const isActive = await checkSocketActive();
    if (isActive) {
      log("warn", "pubsub-server", "Another TUI owns the socket — running polling-only");
      return null;
    }
    // Stale socket, clean up
    try {
      unlinkSync(SOCKET_PATH);
    } catch {
      // Ignore
    }
  }

  const server = net.createServer((conn) => {
    let buffer = "";

    conn.on("data", (chunk) => {
      buffer += chunk.toString();
      const lines = buffer.split("\n");
      // Keep the last partial line in the buffer
      buffer = lines.pop()!;

      for (const line of lines) {
        if (!line.trim()) continue;
        try {
          const msg = JSON.parse(line) as PubSubMessage;
          onMessage(msg);
        } catch {
          log("debug", "pubsub-server", `Failed to parse message: ${line.slice(0, 200)}`);
        }
      }
    });

    conn.on("error", () => {
      // Client disconnected, ignore
    });
  });

  server.on("error", (err) => {
    log("error", "pubsub-server", `Server error: ${err.message}`);
  });

  try {
    server.listen(SOCKET_PATH);
    log("info", "pubsub-server", `Listening on ${SOCKET_PATH}`);
  } catch (err) {
    log("error", "pubsub-server", `Failed to listen: ${err}`);
    return null;
  }

  // Clean up socket file on exit
  const cleanup = () => {
    try {
      server.close();
    } catch {
      // Ignore
    }
    try {
      if (existsSync(SOCKET_PATH)) {
        unlinkSync(SOCKET_PATH);
      }
    } catch {
      // Ignore
    }
  };

  process.on("exit", cleanup);
  process.on("SIGINT", () => {
    cleanup();
    process.exit(0);
  });
  process.on("SIGTERM", () => {
    cleanup();
    process.exit(0);
  });

  return server;
}

/**
 * Stop the pub/sub server and remove the socket file.
 */
export function stopPubSubServer(server: net.Server): void {
  try {
    server.close();
  } catch {
    // Ignore
  }
  try {
    if (existsSync(SOCKET_PATH)) {
      unlinkSync(SOCKET_PATH);
    }
  } catch {
    // Ignore
  }
}

/**
 * Check if the socket file is owned by an active process.
 * Returns true if another process is listening, false if stale.
 */
function checkSocketActive(): Promise<boolean> {
  return new Promise((resolve) => {
    const client = net.createConnection(SOCKET_PATH, () => {
      // Connected — another TUI is active
      client.end();
      resolve(true);
    });
    client.on("error", () => {
      // ECONNREFUSED or other error — stale socket
      resolve(false);
    });
    client.setTimeout(500, () => {
      client.destroy();
      resolve(false);
    });
  });
}
