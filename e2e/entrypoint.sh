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

# Only stage lightweight files (skip node_modules)
echo "node_modules/" > .gitignore
git add .gitignore package.json
git commit -m "init" --quiet

# Seed the repo into the database
npx tsx src/cli.tsx repo add /work

# Seed standalone sessions (simulates Claude sessions in untracked directories)
npx tsx e2e/seed-standalone.ts

# Skip setup wizard and show main branch (it's the only worktree in the container)
# Enable Linear with a dummy key so the mock-api can serve fixture data
echo '{"setupCompleted":true,"hideMainBranch":false,"linearEnabled":true,"linearApiKey":"lin_api_mock"}' > "$AM_DATA_DIR/settings.json"

# Seed a Claude session id on the main worktree via a hook event so the detail
# panel's "Session" row renders. UserPromptSubmit maps to "executing" and does
# not touch the active_subagents counter; the status --set below overrides the
# status to "delegating" while COALESCE preserves this session_id.
echo '{"hook_event_name":"UserPromptSubmit","session_id":"3f2a91c8-7b4d-4e0a-9c1f-8d2e5a6b7c90"}' | npx tsx src/cli.tsx hook-event --worktree /work || true

# Seed the "delegating" agent status on the main worktree so the TUI shows the
# magenta pulsing dot / "Delegating" label (main turn stopped, subagents running).
# This is set at startup so getDisplayStatus keeps it fresh (not stale).
npx tsx src/cli.tsx status --worktree /work --set delegating || true

# Start ttyd serving the TUI
exec ttyd -p 7681 --writable npx tsx src/cli.tsx
