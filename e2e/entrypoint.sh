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
