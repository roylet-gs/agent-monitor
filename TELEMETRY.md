# Usage reports

`am` can send anonymous usage and error reports. They tell us which versions are in use, which features matter, how fast `am` is on real machines, and where in its code errors and crashes happen, so problems can be fixed before anyone has to report them.

**They are off unless you turn them on.** Setup asks once; if you set `am` up before reports existed, it asks once the next time you open it. Nothing is recorded until you answer.

## Turning them on or off

- `am telemetry on` / `am telemetry off`
- Settings → Privacy → Usage reports
- `am settings set telemetry on|off`

Turning them off deletes anything waiting to be sent. These always turn them off, whatever the setting says:

- `DO_NOT_TRACK=1`
- `AM_TELEMETRY=0`
- `CI` set (CI runners)

`am telemetry status` shows whether they're on, and why.

## What is sent

Every report carries:

| Field | Example |
|---|---|
| `am` version | `2.2.0` |
| Which binary | `am`, `am-tui`, `am-hook` |
| OS and CPU | `darwin`, `arm64` |
| How `am` was installed | `homebrew`, `npm`, `cargo` |
| A random install id | `1b9d…` (a UUID made on this machine; `am telemetry reset-id` replaces it) |

And one of these events:

| Event | What it holds |
|---|---|
| `am.command` | A CLI command's name as `am` defines it (`worktree.create`), and whether you or an agent (through MCP) ran it. Never its arguments. |
| `am.features` | How many times a dashboard command ran (`command: newWorktree, count: 3`), one event per command, every 15 minutes. |
| `am.heartbeat` | Once a day: dashboard or review-only mode, which editor setting (`vscode`, …, or `custom`), how many repositories and worktrees, how many sessions per built-in agent (a custom plugin counts as `custom`), whether GitHub, Linear, review requests, compact view and cmux are on, how many review groups and changed key bindings, whether the theme is built in, and the terminal app's name (`iTerm.app`, `ghostty`). |
| `am.perf` | Every hour, per stage (`collector.git`, `ui.frame`, …): sample count and p50/p95/max time. |
| `am.error` | A warning or error was logged: its level, `am`'s module (`github`), the file and line in `am`'s own source (`crates/am-core/src/integrations/github.rs:120`), and an error kind where the code gives one (`rate_limited`). **Not the message.** |
| `am.crash` | `am` panicked: the file and line in its own source. Not the message. |

## What is never sent

Your code, prompts or agent output; log messages; file or directory paths; repository, branch, worktree, PR, ticket, team or user names; hostnames; your IP address (the relay drops it); settings values beyond the ones listed above.

This is enforced in code, not only by convention. Every event is built from a fixed list of fields (`crates/am-core/src/telemetry/schema.rs`), and text values must be short identifier-like tokens: no spaces, slashes, `@` or `~`. Source paths are the one exception, and must start with `crates/`. Anything else is dropped before it is written. It's checked again before sending, and once more by the relay.

## Seeing it for yourself

`am telemetry show` lists every event waiting to be sent. `am telemetry show --json` prints the exact request body.

Events wait in `~/.agent-monitor/telemetry/spool.jsonl`, a plain text file you can read. They are capped at 1 MB and deleted unsent after 7 days. The dashboard sends them every 15 minutes, or you can send them now with `am telemetry flush`. The hook and the CLI never use the network for this.

## Where it goes

Reports go over HTTPS to a small relay run by the `am` maintainers. The relay checks them against the same schema, drops the sender's IP address, and forwards them to a private Grafana Cloud stack that only the maintainers can see. Data there is kept for Grafana Cloud's standard retention.

## Your own collector

Separately from usage reports, `am` can send its full OpenTelemetry stream (logs with their messages, traces and metrics) to a collector you run. Set `OTEL_EXPORTER_OTLP_ENDPOINT`; see the README's Troubleshooting section. That data goes only where you point it.
