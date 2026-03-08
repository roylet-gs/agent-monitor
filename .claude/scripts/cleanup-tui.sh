#!/bin/bash
# Cleanup ttyd process started by capture-tui.sh

PID_FILE="/tmp/ttyd-evidence.pid"

if [ -f "$PID_FILE" ]; then
  PID=$(cat "$PID_FILE")
  if kill "$PID" 2>/dev/null; then
    echo "Stopped ttyd (PID $PID)"
  else
    echo "ttyd (PID $PID) was not running"
  fi
  rm -f "$PID_FILE"
else
  echo "No ttyd PID file found"
fi
