import net from "net";
import { existsSync } from "fs";
import { SOCKET_PATH } from "./paths.js";
import type { PubSubMessage } from "./pubsub-types.js";

/**
 * Fire-and-forget publish a message to the pub/sub server.
 * Returns true if the message was sent, false otherwise.
 * All errors are silently swallowed — the DB write is the source of truth.
 */
export function publishMessage(msg: PubSubMessage): Promise<boolean> {
  return new Promise((resolve) => {
    if (!existsSync(SOCKET_PATH)) {
      resolve(false);
      return;
    }

    const client = net.createConnection(SOCKET_PATH, () => {
      try {
        client.end(JSON.stringify(msg) + "\n");
        resolve(true);
      } catch {
        resolve(false);
      }
    });

    client.on("error", () => {
      resolve(false);
    });

    client.setTimeout(1000, () => {
      client.destroy();
      resolve(false);
    });
  });
}
