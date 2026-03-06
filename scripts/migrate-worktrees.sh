#!/usr/bin/env bash
# One-off migration: move worktrees from .worktrees/ to .claude/worktrees/
# Run from anywhere. Scans all repos registered in agent-monitor's SQLite DB.
set -euo pipefail

DB="$HOME/.agent-monitor/agent-monitor.db"

if [ ! -f "$DB" ]; then
  echo "No agent-monitor database found at $DB"
  exit 1
fi

moved=0
skipped=0
errors=0

# Get all registered repo paths
repo_paths=$(sqlite3 "$DB" "SELECT path FROM repositories;")

while IFS= read -r repo; do
  [ -z "$repo" ] && continue
  old_dir="$repo/.worktrees"

  if [ ! -d "$old_dir" ]; then
    continue
  fi

  new_dir="$repo/.claude/worktrees"
  mkdir -p "$new_dir"

  for wt in "$old_dir"/*/; do
    [ ! -d "$wt" ] && continue
    dirname=$(basename "$wt")
    new_path="$new_dir/$dirname"

    if [ -d "$new_path" ]; then
      echo "SKIP: $new_path already exists"
      skipped=$((skipped + 1))
      continue
    fi

    echo "MOVE: $wt -> $new_path"
    if git -C "$repo" worktree move "$wt" "$new_path" 2>/dev/null; then
      moved=$((moved + 1))
    else
      echo "  ERROR: git worktree move failed, trying filesystem move..."
      if mv "$wt" "$new_path"; then
        moved=$((moved + 1))
      else
        echo "  ERROR: could not move $wt"
        errors=$((errors + 1))
      fi
    fi
  done

  # Remove old .worktrees dir if empty
  rmdir "$old_dir" 2>/dev/null || true
done <<< "$repo_paths"

echo ""
echo "Migration complete: $moved moved, $skipped skipped, $errors errors"
echo "Run 'am' to launch the TUI — syncWorktrees will update DB paths automatically."
