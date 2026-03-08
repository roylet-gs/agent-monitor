#!/bin/bash
# Usage: capture-tui.sh <command> [screenshot-dir] [port]
# Starts ttyd serving the given command, waits for it to be ready,
# and outputs the URL. PID file at /tmp/ttyd-evidence.pid for cleanup.

set -euo pipefail

COMMAND="$1"
SCREENSHOT_DIR="${2:-.github/evidence}"
PORT="${3:-7681}"

if ! command -v ttyd &>/dev/null; then
  echo "ERROR: ttyd not installed. Run: brew install ttyd" >&2
  exit 1
fi

mkdir -p "$SCREENSHOT_DIR"

# Kill any existing ttyd on this port
kill "$(cat /tmp/ttyd-evidence.pid 2>/dev/null)" 2>/dev/null || true
rm -f /tmp/ttyd-evidence.pid

# Start ttyd in background
ttyd -p "$PORT" --writable $COMMAND &
TTYD_PID=$!
echo "$TTYD_PID" > /tmp/ttyd-evidence.pid

# Wait for ttyd to be ready (up to 6 seconds)
for i in {1..30}; do
  if curl -s "http://localhost:$PORT" > /dev/null 2>&1; then
    echo "http://localhost:$PORT"
    exit 0
  fi
  sleep 0.2
done

echo "ERROR: ttyd failed to start within 6 seconds" >&2
kill "$TTYD_PID" 2>/dev/null || true
rm -f /tmp/ttyd-evidence.pid
exit 1
