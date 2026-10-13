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
# every branch to the same ticket, which would rename them all).
cat > "$AM_DATA_DIR/settings.json" <<'JSON'
{
  "setupCompleted": true,
  "hideMainBranch": false,
  "linearEnabled": true,
  "linearApiKey": "lin_api_mock",
  "linearAutoNickname": false
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
# 1M window (15% used, so it renders green).
mkdir -p "$HOME/.claude/projects/-work"
cat > "$HOME/.claude/projects/-work/$SID.jsonl" <<JSONL
{"type":"user","cwd":"/work","sessionId":"$SID","message":{"role":"user","content":"Refactor auth"}}
{"type":"assistant","isSidechain":false,"cwd":"/work","sessionId":"$SID","message":{"model":"claude-opus-5-5","usage":{"input_tokens":2,"cache_read_input_tokens":148000,"cache_creation_input_tokens":1998,"output_tokens":120}}}
JSONL

echo "{\"hook_event_name\":\"UserPromptSubmit\",\"session_id\":\"$SID\",\"transcript_path\":\"/tmp/claude-sessions/$SID.jsonl\"}" | hook /work
echo "{\"hook_event_name\":\"SubagentStart\",\"session_id\":\"$SID\",\"agent_id\":\"a1b2c3d4\",\"agent_type\":\"Explore\",\"transcript_path\":\"/tmp/claude-sessions/$SID.jsonl\"}" | hook /work
echo "{\"hook_event_name\":\"PreToolUse\",\"session_id\":\"$SID\",\"agent_id\":\"a1b2c3d4\",\"agent_type\":\"Explore\",\"tool_name\":\"Grep\",\"tool_input\":{\"pattern\":\"verifyToken\"},\"transcript_path\":\"/tmp/claude-sessions/$SID.jsonl\"}" | hook /work
# Main turn stops while the subagent still runs → "delegating".
echo "{\"hook_event_name\":\"Stop\",\"session_id\":\"$SID\",\"transcript_path\":\"/tmp/claude-sessions/$SID.jsonl\"}" | hook /work
am status --worktree /work

# --- Standalone session ("Other sessions"): hook-event on an untracked dir --
mkdir -p /home/user/my-project
echo '{"hook_event_name":"UserPromptSubmit","session_id":"sess-standalone-1"}' | hook /home/user/my-project

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

# 7681: the seeded dashboard. ttyd spawns a new am-tui per connection.
exec ttyd -p 7681 --writable -t disableLeaveAlert=true am-tui
