import { useEffect, useRef } from "react";
import type net from "net";
import { startPubSubServer, stopPubSubServer } from "../lib/pubsub-server.js";
import { log } from "../lib/logger.js";
import type { PubSubMessage } from "../lib/pubsub-types.js";

/**
 * React hook that manages the pub/sub server lifecycle.
 * Starts the server on mount, stops on unmount.
 * Uses a callback ref to always invoke the latest handler.
 */
export function usePubSub(onMessage: (msg: PubSubMessage) => void): void {
  const callbackRef = useRef(onMessage);
  useEffect(() => {
    callbackRef.current = onMessage;
  }, [onMessage]);

  const serverRef = useRef<net.Server | null>(null);

  useEffect(() => {
    let cancelled = false;

    startPubSubServer((msg) => {
      callbackRef.current(msg);
    }).then((server) => {
      if (cancelled) {
        if (server) stopPubSubServer(server);
        return;
      }
      serverRef.current = server;
      if (!server) {
        log("info", "usePubSub", "Pub/sub server not started (another TUI may be running)");
      }
    }).catch((err) => {
      log("error", "usePubSub", `Failed to start pub/sub server: ${err}`);
    });

    return () => {
      cancelled = true;
      if (serverRef.current) {
        stopPubSubServer(serverRef.current);
        serverRef.current = null;
      }
    };
  }, []);
}
