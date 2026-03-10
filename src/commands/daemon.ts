import { getDaemonPid, isDaemonRunning, stopDaemon } from "../lib/daemon.js";
import { DaemonClient } from "../lib/daemon-client.js";

export function daemonStart(): void {
  if (isDaemonRunning()) {
    const pid = getDaemonPid();
    console.log(`Daemon is already running (pid=${pid})`);
    return;
  }

  // Use DaemonClient to start the daemon
  const client = new DaemonClient({
    onData: () => {},
  });

  client.connect().then((ok) => {
    client.destroy();
    if (ok) {
      const pid = getDaemonPid();
      console.log(`Daemon started (pid=${pid})`);
    } else {
      console.error("Failed to start daemon");
      process.exit(1);
    }
  });
}

export function daemonStop(): void {
  if (!isDaemonRunning()) {
    console.log("Daemon is not running");
    return;
  }

  const pid = getDaemonPid();
  const ok = stopDaemon();
  if (ok) {
    console.log(`Daemon stopped (pid=${pid})`);
  } else {
    console.error("Failed to stop daemon");
    process.exit(1);
  }
}

export function daemonStatus(): void {
  const pid = getDaemonPid();
  if (pid !== null) {
    console.log(`Daemon is running (pid=${pid})`);
  } else {
    console.log("Daemon is not running");
  }
}
