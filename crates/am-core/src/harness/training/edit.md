# Improve the am harness plugin `{{id}}`

You are running inside the harness that plugin `{{id}}` describes (started with `{{command}}`). The
plugin is a **draft** in agent-monitor (am). It has been in use, and am has collected what went
wrong. Fix the plugin. Work in `{{workspace}}` with the `{{am}}` CLI only. Same rules as before:
don't edit am's files (`{{app_dir}}`) or another harness's config by hand, never run
`{{am}} harness publish` (the user publishes), keep probe runs tiny.

## 1. Gather the evidence

    {{am}} harness show {{id}}
    {{am}} harness issues {{id}}
    {{am}} harness captures {{id}}
    {{am}} harness test {{id}}
    {{am}} logs --module plugin:{{id}} -n 200
    {{am}} harness schema

Current manifest: `{{manifest_path}}`. The previous agent's notes are in its `training.notes`.
Captures: `{{capture_dir}}`. Issue kinds: {{issue_kinds}}. Each has a sample. Start with the highest
counts.

## 2. Diagnose
For each issue, find the cause in the captures (`--full`) and in what you know about your harness.
If an issue needs fresh evidence (e.g. an event that has never fired), trigger it the same way as
during training: a headless run of your harness from `{{workspace}}`, or exact steps for the user.
Check whether your harness version changed (`{{command}} --version` vs `training.verifiedWith`).

## 3. Fix and verify

    cp {{manifest_path}} ./manifest.json     # edit this copy
    {{am}} harness test {{id}} --manifest ./manifest.json
    {{am}} harness validate ./manifest.json
    {{am}} harness add ./manifest.json --replace

After replacing, clear issues you have fixed (`{{am}} harness issues {{id}} --clear`) so new ones
stand out, and trigger a short session to confirm nothing new appears. `{{am}} harness rollback {{id}}`
restores the previous version if you make things worse.

## 4. Report
Update `training.notes` / `training.verifiedWith`. Tell the user what you fixed, what is still open,
and that they can publish from Settings → Harnesses when they are happy.
