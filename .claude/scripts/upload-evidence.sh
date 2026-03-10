#!/bin/bash
# Upload screenshots to a GitHub release as assets.
# Usage: upload-evidence.sh [evidence-dir]
# Outputs the GitHub release asset URLs for each uploaded image.

set -euo pipefail

EVIDENCE_DIR="${1:-.github/evidence}"
BRANCH=$(git branch --show-current)
REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')
RELEASE_TAG="evidence-store"

if [ ! -d "$EVIDENCE_DIR" ] || [ -z "$(ls "$EVIDENCE_DIR"/*.png 2>/dev/null)" ]; then
  echo "ERROR: No PNG files found in $EVIDENCE_DIR" >&2
  exit 1
fi

# Ensure the release exists
if ! gh release view "$RELEASE_TAG" --repo "$REPO" >/dev/null 2>&1; then
  echo "Creating release '$RELEASE_TAG'..." >&2
  gh release create "$RELEASE_TAG" --repo "$REPO" --title "Evidence Store" --notes "Automated screenshot storage for PRs and documentation." --latest=false
fi

# Sanitize branch name: replace / with -
SAFE_BRANCH="${BRANCH//\//-}"

# Copy PNGs to temp dir with branch-prefixed names
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

for img in "$EVIDENCE_DIR"/*.png; do
  NAME=$(basename "$img")
  cp "$img" "$TMPDIR/${SAFE_BRANCH}--${NAME}"
done

# Upload all assets (--clobber overwrites existing assets with same name)
gh release upload "$RELEASE_TAG" "$TMPDIR"/*.png --repo "$REPO" --clobber

# Output URLs
for img in "$EVIDENCE_DIR"/*.png; do
  NAME=$(basename "$img")
  ASSET_NAME="${SAFE_BRANCH}--${NAME}"
  echo "https://github.com/$REPO/releases/download/$RELEASE_TAG/$ASSET_NAME"
done
