#!/bin/bash
# Cleanup ttyd process started by capture-tui.sh and evidence data dir

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

# Clean up isolated evidence data dir if set (safety: only /tmp/am-evidence-*)
if [ -n "${AM_EVIDENCE_DIR:-}" ]; then
  case "$AM_EVIDENCE_DIR" in
    /tmp/am-evidence-*)
      rm -rf "$AM_EVIDENCE_DIR"
      echo "Cleaned up evidence dir: $AM_EVIDENCE_DIR"
      ;;
    *)
      echo "WARNING: AM_EVIDENCE_DIR ($AM_EVIDENCE_DIR) not in /tmp/am-evidence-*, skipping cleanup" >&2
      ;;
  esac
fi
