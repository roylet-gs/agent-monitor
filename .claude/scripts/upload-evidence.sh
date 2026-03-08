#!/bin/bash
# Upload screenshots from .github/evidence/ to the evidence-images orphan branch.
# Usage: upload-evidence.sh [evidence-dir]
# Outputs the raw GitHub URLs for each uploaded image.

set -euo pipefail

EVIDENCE_DIR="${1:-.github/evidence}"
BRANCH=$(git branch --show-current)
REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')

if [ ! -d "$EVIDENCE_DIR" ] || [ -z "$(ls "$EVIDENCE_DIR"/*.png 2>/dev/null)" ]; then
  echo "ERROR: No PNG files found in $EVIDENCE_DIR" >&2
  exit 1
fi

TMPDIR=$(mktemp -d)
trap 'git worktree remove --force "$TMPDIR" 2>/dev/null || true; rm -rf "$TMPDIR"' EXIT

# Check if orphan branch exists on remote
if git ls-remote --heads origin evidence-images | grep -q evidence-images; then
  git fetch origin evidence-images
  git worktree add --force "$TMPDIR" evidence-images
else
  git worktree add --force --orphan -b evidence-images "$TMPDIR"
  git -C "$TMPDIR" rm -rf . 2>/dev/null || true
  git -C "$TMPDIR" commit --allow-empty -m "init evidence-images branch"
fi

# Copy screenshots into branch-named folder
mkdir -p "$TMPDIR/$BRANCH"
cp "$EVIDENCE_DIR"/*.png "$TMPDIR/$BRANCH/"

# Commit and push
git -C "$TMPDIR" add .
if git -C "$TMPDIR" diff --cached --quiet; then
  echo "No new images to upload" >&2
else
  git -C "$TMPDIR" commit -m "evidence: $BRANCH"
  if ! git -C "$TMPDIR" push origin evidence-images; then
    echo "ERROR: Failed to push evidence-images branch" >&2
    exit 1
  fi
fi

# Output URLs
for img in "$EVIDENCE_DIR"/*.png; do
  NAME=$(basename "$img")
  echo "https://raw.githubusercontent.com/$REPO/evidence-images/$BRANCH/$NAME"
done
