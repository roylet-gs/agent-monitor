#!/bin/bash
# Entrypoint for the `app` E2E container: seeds a repo, worktrees, agent state
# and a standalone session through the native CLI (`am`), then serves am-tui.

set -e

export AM_DATA_DIR="${AM_DATA_DIR:-/tmp/am-e2e}"
mkdir -p "$AM_DATA_DIR"

git config --global init.defaultBranch main
git config --global user.email "test@test.com"
git config --global user.name "Test"

# A local bare "origin" so the New Worktree form's live check (git ls-remote)
# has a real remote to answer from, and fresh worktrees branch off origin/main.
git init --quiet --bare /srv/origin.git

mkdir -p /work
cd /work
git init --quiet
echo "# e2e" > README.md
git add README.md
git commit -m "init" --quiet
git remote add origin /srv/origin.git
git push --quiet -u origin main

# A branch that exists only on origin ("✓ exists on origin" in the form).
git branch feature/on-origin
git push --quiet origin feature/on-origin
git branch -D --quiet feature/on-origin
git fetch --quiet origin

# A second worktree for the delete dialog (the main checkout can't be deleted).
git worktree add --quiet /work/.claude/worktrees/feature-to-delete -b feature/to-delete
# Three agents at once in one worktree (18-multi-session.spec.ts). Added after
# feature/to-delete, so it sorts first (newest) and to-delete → main stays adjacent.
git worktree add --quiet /work/.claude/worktrees/feature-pair -b feature/pair
echo ".claude/" > .git/info/exclude

# Register the repo (also syncs its worktrees into the DB).
am repo add /work

# A startup script, so creating a worktree asks to run it in the jobs pane.
# No terminal emulator here, so it runs in-process with its output piped
# into the pane.
REPO_ID=$(am repo list --json | sed -n 's/.*"id": *"\([^"]*\)".*/\1/p' | head -1)
mkdir -p "$AM_DATA_DIR/scripts"
cat > "$AM_DATA_DIR/scripts/$REPO_ID.sh" <<'SH'
#!/bin/sh
echo "e2e startup script running in $(basename "$PWD")"
printf '\033[32mcoloured line\033[0m\n'
echo "e2e startup script done"
SH

# Skip the setup wizard, show main, enable Linear against the mock.
# linearAutoNickname is off so rows keep their branch names (the mock links
# every branch to the same ticket, which would rename them all). Usage reports
# are answered (off), so the one-time prompt stays away (spec 34 brings it back).
cat > "$AM_DATA_DIR/settings.json" <<'JSON'
{
  "setupCompleted": true,
  "telemetry": "off",
  "hideMainBranch": false,
  "linearEnabled": true,
  "linearApiKey": "lin_api_mock",
  "linearAutoNickname": false,
  "runningProcessFilter": "sleep infinity"
}
JSON

# A fresh update-check cache naming a newer release, so the dashboard shows
# "⬆ v9.9.9 available" without network (a cache < 6 h old is trusted).
echo "{\"checkedAt\": $(date +%s), \"latest\": \"9.9.9\"}" > "$AM_DATA_DIR/update-check.json"

# --- Agent activity on main, through the real hook path -------------------
SID="3f2a91c8-7b4d-4e0a-9c1f-8d2e5a6b7c90"
# Subagent description comes from Claude Code's <session>/subagents/agent-<id>.meta.json.
mkdir -p "/tmp/claude-sessions/$SID/subagents"
echo '{"description":"Survey auth module","agentType":"Explore"}' > "/tmp/claude-sessions/$SID/subagents/agent-a1b2c3d4.meta.json"
hook() { am hook-event --worktree "$1" || true; }
# Transcript with token usage for the context-window bar: 150k of the default
# 1M window (15% used, so it renders green). The response spans two content
# blocks, each line repeating its usage; today's token total counts it once
# (150,120 tokens).
NOW_ISO=$(date -u +%Y-%m-%dT%H:%M:%S.000Z)
mkdir -p "$HOME/.claude/projects/-work"
cat > "$HOME/.claude/projects/-work/$SID.jsonl" <<JSONL
{"type":"user","cwd":"/work","sessionId":"$SID","timestamp":"$NOW_ISO","message":{"role":"user","content":"Refactor auth"}}
{"type":"assistant","isSidechain":false,"cwd":"/work","sessionId":"$SID","timestamp":"$NOW_ISO","message":{"id":"msg_e2e_1","model":"claude-opus-5-5","usage":{"input_tokens":2,"cache_read_input_tokens":148000,"cache_creation_input_tokens":1998,"output_tokens":120}}}
{"type":"assistant","isSidechain":false,"cwd":"/work","sessionId":"$SID","timestamp":"$NOW_ISO","message":{"id":"msg_e2e_1","model":"claude-opus-5-5","usage":{"input_tokens":2,"cache_read_input_tokens":148000,"cache_creation_input_tokens":1998,"output_tokens":120}}}
JSONL
# Usage outside every tracked row: counted by `dailyTokensScope: all` (total
# 1.0M today) but not by `visible` (150k). Yesterday's line counts for neither.
mkdir -p "$HOME/.claude/projects/-home-user-elsewhere"
cat > "$HOME/.claude/projects/-home-user-elsewhere/sess-elsewhere.jsonl" <<JSONL
{"type":"assistant","cwd":"/home/user/elsewhere","timestamp":"$(date -u -d '-2 days' +%Y-%m-%dT%H:%M:%S.000Z)","message":{"id":"msg_e2e_old","model":"m","usage":{"input_tokens":5000000,"output_tokens":0}}}
{"type":"assistant","cwd":"/home/user/elsewhere","timestamp":"$NOW_ISO","message":{"id":"msg_e2e_2","model":"m","usage":{"input_tokens":49880,"cache_read_input_tokens":700000,"output_tokens":100000}}}
JSONL

echo "{\"hook_event_name\":\"UserPromptSubmit\",\"session_id\":\"$SID\",\"transcript_path\":\"/tmp/claude-sessions/$SID.jsonl\"}" | hook /work
echo "{\"hook_event_name\":\"SubagentStart\",\"session_id\":\"$SID\",\"agent_id\":\"a1b2c3d4\",\"agent_type\":\"Explore\",\"transcript_path\":\"/tmp/claude-sessions/$SID.jsonl\"}" | hook /work
echo "{\"hook_event_name\":\"PreToolUse\",\"session_id\":\"$SID\",\"agent_id\":\"a1b2c3d4\",\"agent_type\":\"Explore\",\"tool_name\":\"Grep\",\"tool_input\":{\"pattern\":\"verifyToken\"},\"transcript_path\":\"/tmp/claude-sessions/$SID.jsonl\"}" | hook /work
# Main turn stops while the subagent still runs → "delegating".
echo "{\"hook_event_name\":\"Stop\",\"session_id\":\"$SID\",\"transcript_path\":\"/tmp/claude-sessions/$SID.jsonl\"}" | hook /work
am status --worktree /work

# --- Three agents in one worktree (18-multi-session.spec.ts) ----------------
# Two Claude sessions and a Gemini one in feature/pair, each with its own
# status. Claude #3a7c: executing, one running subagent, a 300k transcript.
# Claude #9e41: turn ended with two subagents still running → delegating.
# Gemini: turn finished → done. (Gemini CLI's hooks carry no subagent events.)
PAIR=/work/.claude/worktrees/feature-pair
C1=3a7c9d10-1111-4a2b-9c3d-000000000001
C2=9e41b2c7-2222-4b3c-8d4e-000000000002
mkdir -p "$HOME/.claude/projects/-work--claude-worktrees-feature-pair"
cat > "$HOME/.claude/projects/-work--claude-worktrees-feature-pair/$C1.jsonl" <<JSONL
{"type":"user","cwd":"/work/.claude/worktrees/feature-pair","sessionId":"$C1","message":{"role":"user","content":"Write the migration"}}
{"type":"assistant","isSidechain":false,"cwd":"/work/.claude/worktrees/feature-pair","sessionId":"$C1","message":{"model":"claude-opus-5-5","usage":{"input_tokens":2,"cache_read_input_tokens":298000,"cache_creation_input_tokens":1998,"output_tokens":120}}}
JSONL
claude_pair() { echo "{\"hook_event_name\":\"$2\",\"session_id\":\"$1\"$3}" | hook "$PAIR"; }
claude_pair "$C1" SessionStart ',"source":"startup"'
claude_pair "$C1" UserPromptSubmit
claude_pair "$C1" SubagentStart ',"agent_id":"p1","agent_type":"Explore"'
claude_pair "$C1" PreToolUse ',"tool_name":"Bash","tool_input":{"command":"cargo test"}'
claude_pair "$C2" SessionStart ',"source":"startup"'
claude_pair "$C2" UserPromptSubmit
claude_pair "$C2" SubagentStart ',"agent_id":"q1","agent_type":"general-purpose"'
claude_pair "$C2" SubagentStart ',"agent_id":"q2","agent_type":"Explore"'
claude_pair "$C2" Stop
pair_gemini() { am hook-event --agent gemini --worktree "$PAIR" --event "$1" || true; }
echo '{"hook_event_name":"SessionStart","session_id":"pair-gem","source":"startup","transcript_path":""}' | pair_gemini SessionStart
echo '{"hook_event_name":"BeforeAgent","session_id":"pair-gem","prompt":"review the diff"}' | pair_gemini BeforeAgent
echo '{"hook_event_name":"AfterAgent","session_id":"pair-gem","prompt_response":"Looks good"}' | pair_gemini AfterAgent

# --- An untracked git repo for the add-repository folder browser ----------
# (22-repositories.spec.ts adds it through Settings → Repositories… and
# removes it again.)
mkdir -p /home/user/other-repo
git -C /home/user/other-repo init --quiet
echo "# other" > /home/user/other-repo/README.md
git -C /home/user/other-repo add README.md
git -C /home/user/other-repo commit -m "init" --quiet

# --- Standalone session ("Other sessions"): hook-event on an untracked dir --
mkdir -p /home/user/my-project
echo '{"hook_event_name":"UserPromptSubmit","session_id":"sess-standalone-1"}' | hook /home/user/my-project
# A session whose directory was deleted mid-run (no SessionEnd): left
# `executing`, it must not show (08-standalone.spec.ts).
mkdir -p /home/user/gone-project
echo '{"hook_event_name":"UserPromptSubmit","session_id":"sess-gone-1"}' | hook /home/user/gone-project
rm -rf /home/user/gone-project

# --- Gemini CLI session: native Gemini events through `--agent gemini` -------
# The last event uses the `--harness` alias so both spellings stay covered.
mkdir -p /home/user/gemini-project
gemini_hook() { am hook-event "${2:---agent}" gemini --worktree /home/user/gemini-project --event "$1" || true; }
echo '{"hook_event_name":"SessionStart","session_id":"gem-1","source":"startup","transcript_path":""}' | gemini_hook SessionStart
echo '{"hook_event_name":"BeforeAgent","session_id":"gem-1","prompt":"add tests"}' | gemini_hook BeforeAgent
echo '{"hook_event_name":"Notification","session_id":"gem-1","notification_type":"ToolPermission","message":"Allow run_shell_command?"}' | gemini_hook Notification --harness
# Decoy: a Claude-shaped transcript under the Gemini session's id. Context
# usage must come from the session's own harness (Gemini has no reader), so
# the Gemini row must never show this 150k (16-gemini.spec.ts).
mkdir -p "$HOME/.claude/projects/-home-user-gemini-project"
cat > "$HOME/.claude/projects/-home-user-gemini-project/gem-1.jsonl" <<'JSONL'
{"type":"user","cwd":"/home/user/gemini-project","sessionId":"gem-1","message":{"role":"user","content":"add tests"}}
{"type":"assistant","isSidechain":false,"cwd":"/home/user/gemini-project","sessionId":"gem-1","message":{"model":"claude-opus-5-5","usage":{"input_tokens":2,"cache_read_input_tokens":148000,"cache_creation_input_tokens":1998,"output_tokens":120}}}
JSONL

# --- Codex CLI session: Claude-shaped events through `--agent codex` --------
# Codex has no project-dir variable, so its hooks pass `--worktree ""` and
# am-hook finds the directory from the payload's cwd. Ends on a permission
# request → waiting (16-gemini.spec.ts).
mkdir -p /home/user/codex-project
codex_hook() { am hook-event --agent codex --worktree "" --event "$1" || true; }
echo '{"hook_event_name":"SessionStart","session_id":"cdx-1","cwd":"/home/user/codex-project","source":"startup","model":"gpt-5"}' | codex_hook SessionStart
echo '{"hook_event_name":"UserPromptSubmit","session_id":"cdx-1","cwd":"/home/user/codex-project","prompt":"bump deps","model":"gpt-5"}' | codex_hook UserPromptSubmit
echo '{"hook_event_name":"PermissionRequest","session_id":"cdx-1","cwd":"/home/user/codex-project","tool_name":"Bash","tool_input":{"command":"cargo update"},"model":"gpt-5"}' | codex_hook PermissionRequest

# --- A shell command an agent left running in feature/to-delete ------------
# Claude Code wraps each Bash tool command in `<shell> -c source
# ~/.claude/shell-snapshots/…`; procscan counts those per worktree, and since
# this one matches the seeded runningProcessFilter ("sleep infinity") the row
# shows `$ 1 command` with its name in teal (25-shell-commands.spec.ts).
# `& wait` forks a subshell that keeps the same argv, as a real `cmd &` in a
# Bash tool command would; it still counts as one command.
mkdir -p "$HOME/.claude/shell-snapshots"
echo ': e2e snapshot' > "$HOME/.claude/shell-snapshots/snapshot-bash-e2e.sh"
(cd /work/.claude/worktrees/feature-to-delete && \
  setsid bash -c "source $HOME/.claude/shell-snapshots/snapshot-bash-e2e.sh && sleep infinity & wait" \
  </dev/null >/dev/null 2>&1 &)

# --- A dev server an editor started directly in feature/pair ----------------
# VS Code's npm scripts panel and tasks run `pnpm dev` with no shell in
# between. A copy of coreutils `timeout` under ~/.vscode-server stands in for
# the editor (procscan recognises it by its exe path; cwd `/`, so it is no
# worktree's), and it forks + execs `env -C <worktree> e2e-dev-server` (a
# copied `sleep`), so the server's parent is the editor itself. Without a
# filter it is hidden as editor tooling; with runningProcessFilter
# "e2e-dev-server" feature/pair shows ▶ (26-editor-spawned-process.spec.ts).
# The seeded filter ("sleep infinity") doesn't match it, so no other spec sees it.
mkdir -p /home/user/.vscode-server/bin/e2e
cp "$(command -v timeout)" /home/user/.vscode-server/bin/e2e/node
cp "$(command -v sleep)" /usr/local/bin/e2e-dev-server
(cd / && setsid /home/user/.vscode-server/bin/e2e/node 1d \
  env -C /work/.claude/worktrees/feature-pair /usr/local/bin/e2e-dev-server 86400 \
  </dev/null >/dev/null 2>&1 &)

# 7682: a fresh, empty data dir on every connection → first-run setup wizard.
cat > /usr/local/bin/am-tui-fresh <<'SH'
#!/bin/bash
rm -rf /tmp/am-fresh /tmp/home-fresh
mkdir -p /tmp/am-fresh /tmp/home-fresh
export AM_DATA_DIR=/tmp/am-fresh HOME=/tmp/home-fresh
exec am-tui
SH
chmod +x /usr/local/bin/am-tui-fresh
# disableLeaveAlert: no "leave site?" prompt when a spec navigates away.
ttyd -p 7682 --writable -t disableLeaveAlert=true am-tui-fresh &

# 7683: the seeded dashboard, after a hook from an agent already running in
# the untracked git repo /home/user/late-repo (24-repo-added-mid-session.spec.ts).
# Fired per connection, so the spec can add the repo, stop tracking it and
# still find a standalone session on a retry.
mkdir -p /home/user/late-repo
git -C /home/user/late-repo init --quiet
echo "# late" > /home/user/late-repo/README.md
git -C /home/user/late-repo add README.md
git -C /home/user/late-repo commit -m "init" --quiet
cat > /usr/local/bin/am-tui-late-session <<'SH'
#!/bin/bash
echo '{"hook_event_name":"UserPromptSubmit","session_id":"sess-late-1"}' \
  | am hook-event --worktree /home/user/late-repo || true
exec am-tui
SH
chmod +x /usr/local/bin/am-tui-late-session
ttyd -p 7683 --writable -t disableLeaveAlert=true am-tui-late-session &

# 7681: the seeded dashboard. ttyd spawns a new am-tui per connection.
exec ttyd -p 7681 --writable -t disableLeaveAlert=true am-tui
