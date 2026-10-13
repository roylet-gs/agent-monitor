# Teach agent-monitor (am) to monitor this harness

You are running inside a coding-agent harness started with `{{command}}`. am is a dashboard that
tracks harness sessions through hook events. It doesn't support yours yet. Your job: write and verify
an am harness plugin manifest for the harness you are running in. Work only in `{{workspace}}` and
with the `{{am}}` CLI. Don't change any other project.

## Rules
- Never edit am's files by hand (`{{app_dir}}`); use `{{am}} harness …`. Never edit another harness's config.
- Never run `{{am}} harness publish`. The user publishes.
- Keep probe runs tiny (one short prompt). Ask the user before anything that costs much or needs approval.
- If something is unknown, say so in `training.notes`. Don't guess silently.

## 1. Learn the contract
Run `{{am}} harness schema` (the reference) and `{{am}} harness show <id> --json` for the built-ins
{{example_ids}} as worked examples. Supported integrations: {{formats}}.
Canonical events: {{canonical_events}}.
Canonical tools: {{canonical_tools}}.

## 2. Learn your harness
Use what you know about your own harness, then confirm it locally: `{{command}} --help`, its config
dir, its installed package files and docs, the web if you have it. Find:
- the hook mechanism (config file + format, or extension API), event names, and the stdin payload
  keys (session id, cwd, tool name/input, final response, notification/approval type, model,
  subagent ids),
- how to resume a session by id and continue the latest, and how to start it with an initial prompt
  (that goes in `training.promptArgs`, e.g. `["-i", "{prompt}"]`),
- where sessions/transcripts are stored and how to read id/cwd/modified time,
- what the process looks like in `ps` (name and argv).

If the hook mechanism matches none of the supported integrations, stop: write what you found to
`{{workspace}}/FINDINGS.md`, tell the user, and suggest they file it with `f` in am.

## 3. Draft and listen to yourself
Write `{{workspace}}/manifest.json` with `"status": "draft"`. Map the events you are sure of in
`events`. List **every other event name your harness documents** in `training.captureEvents`, so am
records them too. Then:

    {{am}} harness validate ./manifest.json
    {{am}} harness add ./manifest.json

am now captures every hook event your harness fires into `{{capture_dir}}`. Most harnesses read
hook config only at session start, so this session probably won't fire them. Trigger them in a
**new** session:
- Preferred: run your harness headless from `{{workspace}}`, e.g. a non-interactive mode with a
  prompt like "read README.md, run `ls`, then append a line to NOTES.txt", plus a turn that needs
  approval if your harness can do that headless.
- Otherwise ask the user to run `{{command}}` in a second terminal in `{{workspace}}`. Give them
  exact steps: send a prompt, let one shell tool run, approve one permission prompt, answer one
  question, exit.

Then read what arrived: `{{am}} harness captures <id>` and `{{am}} harness captures <id> --full --last 5`.
`env.dirs` in each capture lists the directory-valued variables the hook saw; the one holding the
session's project dir is your `hooks.projectDirVar`.

## 4. Fix until clean
Edit `manifest.json`, then replay the captures through it without reinstalling:

    {{am}} harness test <id> --manifest ./manifest.json

Aim for: every captured event either mapped or deliberately left in `captureEvents`; a full turn that
goes executing → done (or waiting on approval); tools mapped (`Bash`, `Read`, `Edit`, `Write`,
`AskUserQuestion`…); `session_id` and `cwd` found; `resume` correct. Check resume yourself from
`{{workspace}}`. Re-add with `{{am}} harness add ./manifest.json --replace` and repeat step 3 if you
changed hooks. Also check `{{am}} harness issues <id>` and `{{am}} logs --module plugin:<id> -n 50`.

## 5. Report
Record what you verified and what you couldn't in `training.verifiedWith` / `training.notes`, run
`{{am}} harness validate <id>` one last time, and give the user a short summary: what works, what
doesn't, and that the plugin is a **draft** they can publish from am's Settings → Harnesses.
