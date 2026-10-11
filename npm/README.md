# Release packaging

Everything here is assembled by `.github/workflows/publish.yml` on each merge
to `main`; nothing in this directory is published as-is.

- `agent-monitor/` — the `@roylet-gs/agent-monitor` package: a tiny Node
  launcher (`bin/am.js`) that execs the native `am` from the matching
  platform package. Its version and `optionalDependencies` versions are set
  by CI.
- `platform/package.json` — template for the per-platform packages
  (`@roylet-gs/agent-monitor-{darwin,linux}-{arm64,x64}`), each holding
  `bin/am`, `bin/am-tui` and `bin/am-hook`. `os`/`cpu` make npm install only
  the one that matches.

Claude Code hooks point at `am-hook` by absolute path (`am hooks install`),
so the Node launcher is never on the per-event hot path.

Homebrew: `agent-monitor.rb.tmpl` is rendered into `Formula/agent-monitor.rb`
at the repo root by the same workflow (version + tarball checksums), so the
repo itself is the tap.
