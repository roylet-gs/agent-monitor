#!/bin/bash
set -e

export AM_DATA_DIR="${AM_DATA_DIR:-/tmp/am-e2e}"
mkdir -p "$AM_DATA_DIR"

# Initialize git repo so the app can detect it
cd /work
git config --global init.defaultBranch main
git init --quiet
git config user.email "test@test.com"
git config user.name "Test"
git add -A
git commit -m "init" --quiet 2>/dev/null || true

# Seed the repo into the database
npx tsx src/cli.tsx repo add /work

# Skip setup wizard
echo '{"setupCompleted":true}' > "$AM_DATA_DIR/settings.json"

# Start ttyd serving the TUI
exec ttyd -p 7681 --writable npx tsx src/cli.tsx
