# npm packaging for the native binaries (Phase 4 — not active yet)

Plan (esbuild/biome pattern), applied at cut-over:

- `@roylet-gs/agent-monitor` keeps its name and `am` bin. Its `bin/am.js` is a
  tiny launcher that resolves the platform package below and `exec`s the
  native `am` (falls back to a clear error if the optional dep is missing).
- One package per platform, published from the `release-native` artifacts,
  each with `os`/`cpu` fields so npm installs only the matching one:
  `@roylet-gs/agent-monitor-darwin-arm64`, `-darwin-x64`, `-linux-x64`,
  `-linux-arm64`. Each contains `bin/am`, `bin/am-tui`, `bin/am-hook`.
- These are listed in the main package's `optionalDependencies`.
- Claude Code hooks point at the absolute path of `am-hook` (written by
  `am hooks install`), so the Node launcher is never on the per-event path.

Homebrew (`roylet-gs/homebrew-tap`, needs creating) gets a formula that
downloads the same release tarballs; see `homebrew/agent-monitor.rb`.
