#!/bin/bash
# DEPRECATED: For local manual debugging only.
# Evidence capture now uses Docker E2E infrastructure (pnpm test:e2e).
# See .claude/commands/capture-evidence.md for the current workflow.
#
# Create an isolated data dir and seed it with the current repo.
# Usage: source seed-evidence-data.sh
# Sets AM_DATA_DIR and AM_EVIDENCE_DIR env vars for the caller.

set -euo pipefail

export AM_EVIDENCE_DIR=$(mktemp -d /tmp/am-evidence-XXXXXX)
export AM_DATA_DIR="$AM_EVIDENCE_DIR/data"
mkdir -p "$AM_DATA_DIR"

echo "Created isolated data dir: $AM_DATA_DIR"

# Register the current repo so the TUI has data to display
npx tsx src/cli.tsx repo add . 2>&1 || {
  echo "WARNING: repo add failed, TUI may have no data" >&2
}

# Write settings so TUI skips setup wizard (loadSettings merges with defaults)
echo '{"setupCompleted":true}' > "$AM_DATA_DIR/settings.json"

echo "Seeded with current repo. AM_DATA_DIR=$AM_DATA_DIR"
