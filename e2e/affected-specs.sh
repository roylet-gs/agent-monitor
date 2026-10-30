#!/bin/bash
# Pick the E2E specs (tests/e2e/docker/NN-*.spec.ts) that a branch's changes
# can affect. This is the one place the file → spec mapping lives; e2e/run.sh
# uses it so `pnpm test:e2e` only runs what the diff touches.
#
#   e2e/affected-specs.sh              changes since the merge base with origin/main
#   e2e/affected-specs.sh --explain    also print "file → specs" lines on stderr
#   ... | e2e/affected-specs.sh --files   map the file paths read from stdin
#
# Output on stdout: `ALL`, the matching spec file names (one per line), or
# nothing when no spec is affected. The changed files are the committed diff
# against the merge base plus staged, unstaged and untracked files. Set
# AM_E2E_BASE to diff against another ref.
#
# When in doubt a file maps to ALL: anything not listed below runs every spec,
# so a new source file is never silently skipped. When you add a spec, add
# its number to the rows for the code it covers.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
explain=false
from_stdin=false
for arg in "$@"; do
  case $arg in
    --explain) explain=true ;;
    --files) from_stdin=true ;;
    *) echo "usage: $0 [--explain] [--files]" >&2; exit 2 ;;
  esac
done

# The specs a path affects: spec number prefixes, `ALL`, or `-` (none).
specs_for() {
  case $1 in
    # --- not covered by E2E (docs, CI, packaging, Rust-only tests) ---------
    *.md | docs/* | .github/* | npm/* | Formula/* | .claude/* | prompt.md | telemetry/* | \
    .gitignore | .mcp.json | .npmrc | \
    crates/*/tests/* | crates/*/examples/* | crates/am-tui/src/snapshots/* | \
    crates/am-tui/src/perf_tests.rs)
      echo - ;;

    # --- the harness itself: everything -----------------------------------
    e2e/* | tests/e2e/docker/helpers/* | .dockerignore | package.json | \
    pnpm-lock.yaml | Cargo.toml | Cargo.lock | crates/*/Cargo.toml)
      echo ALL ;;
    tests/e2e/docker/[0-9][0-9]-*.spec.ts)
      local base=${1##*/}; echo "${base%%-*}" ;;

    # --- am-tui -----------------------------------------------------------
    # ui/app/main draw and drive every screen (and every screenshot).
    crates/am-tui/src/ui.rs | crates/am-tui/src/app.rs | crates/am-tui/src/main.rs) echo ALL ;;
    crates/am-tui/src/setup.rs) echo 11 22 28 32 ;;
    crates/am-tui/src/settings_ui.rs) echo 03 05 12 13 14 15 16 17 20 21 22 23 24 25 26 28 30 32 33 34 35 ;;
    crates/am-tui/src/usage_reports.rs) echo 34 ;; # heartbeat, feature counts, the sending thread
    crates/am-tui/src/keymap.rs | crates/am-tui/src/keys_ui.rs) echo 01 02 15 20 27 28 32 33 35 ;; # dashboard keys, ? overlay, key-bar clicks, Settings → Key bindings
    crates/am-tui/src/reviews_ui.rs | crates/am-tui/src/review_groups_ui.rs) echo 28 31 32 ;;    # the review-requests page and its groups
    crates/am-tui/src/plugins_ui.rs) echo 16 ;;   # Settings → Harness plugins
    crates/am-tui/src/repos_ui.rs | crates/am-tui/src/folder_browser.rs) echo 23 24 28 ;; # Settings → Repositories, add-repo browser
    crates/am-tui/src/nav.rs) echo 01 03 23 28 29 ;; # list movement: dashboard, Settings, folder browser, review page, scrolling
    crates/am-tui/src/theme.rs | crates/am-tui/src/theme_builder.rs) echo 12 ;;
    crates/am-tui/src/modals.rs) echo 09 10 13 22 23 27 34 ;;
    crates/am-tui/src/jobs.rs) echo 09 10 13 22 27 28 31 ;;
    crates/am-tui/src/ops.rs) echo 09 10 11 23 24 28 ;;
    crates/am-tui/src/data.rs | crates/am-tui/src/gitwork.rs) echo 01 06 07 08 13 14 17 18 24 25 26 28 32 33 ;;
    crates/am-tui/src/cmux_sync.rs) echo - ;; # only runs inside cmux; unit tests cover it
    crates/am-tui/src/launcher.rs) echo 13 21 23 28 ;; # opens IDEs/terminals; 21 covers the settings.json and debug.log opens, 28 the review window, 13 the update job

    # --- am-core ----------------------------------------------------------
    crates/am-core/src/lib.rs | crates/am-core/src/db.rs | crates/am-core/src/settings.rs | \
    crates/am-core/src/model.rs | crates/am-core/src/paths.rs) echo ALL ;;
    crates/am-core/src/grouping.rs) echo 01 05 08 18 24 27 28 33 ;;
    crates/am-core/src/gitinfo.rs) echo 01 ;;
    crates/am-core/src/timefmt.rs) echo 01 17 ;;
    crates/am-core/src/log.rs | crates/am-core/src/perf.rs) echo 01 ;; # `w` log pane; perf writes `[perf]` log lines
    # Usage reports and the OpenTelemetry export: every binary starts them, so
    # 01 for "it still runs"; 34 the prompt and setting (sending: cli.rs).
    crates/am-core/src/telemetry/*) echo 01 34 ;;
    crates/am-core/src/status.rs) echo 07 08 16 18 24 ;;
    # Harness plugins: hook normalisation seeds every agent row; the installer
    # runs in the wizard; the plugin page and Gemini row are spec 16.
    crates/am-core/src/harness/claude/transcript.rs) echo 14 ;;  # Claude's context reader
    crates/am-core/src/harness/*|crates/am-core/src/agents/*) echo 07 08 11 16 17 ;; # agents/ = pre-rename paths
    crates/am-core/src/worktrees.rs) echo 09 10 11 23 24 27 28 ;; # 23/24 add repos
    crates/am-core/src/hooks_installer.rs) echo 11 16 ;;
    crates/am-core/src/mcp_installer.rs) echo - ;; # am mcp install; cli.rs covers it
    crates/am-core/src/context.rs) echo 14 ;;      # context-window bar
    crates/am-core/src/usage.rs) echo 17 ;;        # today's token total
    crates/am-core/src/update_check.rs) echo 13 ;; # update-available notice, clicking it to update
    crates/am-core/src/themes.rs | crates/am-core/src/themes/*) echo 12 ;;
    crates/am-core/src/integrations/github.rs) echo 04 28 ;;
    crates/am-core/src/integrations/reviews.rs | crates/am-core/src/review_groups.rs) echo 28 ;;
    crates/am-core/src/integrations/linear.rs) echo 04 05 ;;
    crates/am-core/src/integrations/cache.rs | crates/am-core/src/integrations/breaker.rs) echo 06 28 ;;
    crates/am-core/src/integrations/mod.rs) echo 04 05 06 28 ;;
    crates/am-core/src/launch.rs) echo 21 ;; # open_in_editor's failure path (no IDE in the container)
    crates/am-core/src/procscan.rs) echo 25 26 ;; # the seeded agent shell command, the editor-spawned dev server
    crates/am-core/src/stacks.rs) echo - ;; # unit / snapshot tests only
    crates/am-core/src/cmux.rs) echo - ;; # no cmux in the container: inert there; unit tests cover it

    # --- am-hook: seeds the delegating main, feature/pair and the standalone sessions
    # (24: a standalone session promoted when its repo is added)
    crates/am-hook/src/*) echo 07 08 16 18 24 ;;

    # --- am-cli: the entrypoint runs `repo add/list`, `hook-event`, `status` ---
    crates/am-cli/src/main.rs | crates/am-cli/src/commands/mod.rs | \
    crates/am-cli/src/commands/repo.rs | crates/am-cli/src/commands/status.rs | \
    crates/am-cli/src/output.rs | crates/am-cli/src/resolve.rs | \
    crates/am-cli/src/store.rs) echo 01 07 08 ;;
    crates/am-cli/src/*) echo - ;; # crates/am-cli/tests/cli.rs covers the rest

    *) echo ALL ;;
  esac
}

if $from_stdin; then
  files=$(cat)
else
  cd "$root"
  base=${AM_E2E_BASE:-}
  if [ -z "$base" ]; then
    for ref in origin/main main; do
      if git rev-parse -q --verify "$ref" >/dev/null; then base=$ref; break; fi
    done
  fi
  [ -n "$base" ] || { echo "affected-specs: no origin/main or main to diff against" >&2; echo ALL; exit 0; }
  mb=$(git merge-base "$base" HEAD)
  # The diff against the merge base covers commits plus staged and unstaged
  # edits; --no-renames lists both sides of a rename.
  files=$( { git diff --name-only --no-renames "$mb"; git ls-files --others --exclude-standard; } | sort -u)
fi

all=false
wanted=" "
while IFS= read -r f; do
  [ -n "$f" ] || continue
  specs=$(specs_for "$f")
  $explain && printf '%s → %s\n' "$f" "$([ "$specs" = - ] && echo "no E2E" || echo "$specs")" >&2
  case $specs in
    ALL) all=true ;;
    -) ;;
    *) for s in $specs; do case $wanted in *" $s "*) ;; *) wanted="$wanted$s " ;; esac; done ;;
  esac
done <<< "$files"

if $all; then echo ALL; exit 0; fi
for s in $wanted; do
  # A spec deleted on this branch has nothing left to run.
  for path in "$root"/tests/e2e/docker/"$s"-*.spec.ts; do
    [ -e "$path" ] && basename "$path"
  done
done | sort
