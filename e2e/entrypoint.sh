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

# Seed auto-approval rules for testing the Manage Rules view
npx tsx src/cli.tsx rule add Bash --input "git status*"
npx tsx src/cli.tsx rule add Bash --input "git diff*"
npx tsx src/cli.tsx rule add Bash --input "git log*"
npx tsx src/cli.tsx rule add WebSearch
npx tsx src/cli.tsx rule add Bash --input "pnpm test*"
npx tsx src/cli.tsx rule add Bash --input "pnpm build*"
npx tsx src/cli.tsx rule add WebFetch --input "domain:docs.anthropic.com"
npx tsx src/cli.tsx rule add Bash --input "ls*"

# Skip setup wizard and show main branch (it's the only worktree in the container)
# Enable Linear with a dummy key so the mock-api can serve fixture data
echo '{"setupCompleted":true,"hideMainBranch":false,"linearEnabled":true,"linearApiKey":"lin_api_mock"}' > "$AM_DATA_DIR/settings.json"

# Start ttyd serving the TUI
exec ttyd -p 7681 --writable npx tsx src/cli.tsx
