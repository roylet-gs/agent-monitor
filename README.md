<h1 align="center">Agent Monitor</h1>

<p align="center">
  <strong>A terminal dashboard for managing git worktrees and monitoring coding agents (Claude Code, Codex, Gemini CLI, Cursor, Copilot CLI, Qwen Code, Factory Droid, Campfire)</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/built%20with-Rust-orange?style=flat-square" alt="Built with Rust">
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey?style=flat-square" alt="Platform">
</p>

<p align="center">
  Agent Monitor (<code>am</code>) gives you one terminal dashboard for your git worktrees across repositories. It shows what each coding agent (Claude Code, Codex, Gemini CLI, Cursor, Copilot CLI, Qwen Code, Factory Droid or Campfire) is doing in real time, along with GitHub PR/CI and Linear ticket status, so you don't have to leave the terminal to check.
</p>

---

## Table of Contents

- [Quick Start](#quick-start)
- [Requirements](#requirements)
- [Installation](#installation)
- [First Run](#first-run)
- [The Dashboard](#the-dashboard)
  - [Keyboard Shortcuts](#keyboard-shortcuts)
  - [Mouse](#mouse)
  - [Status Legend](#status-legend)
  - [Row Badges and Columns](#row-badges-and-columns)
  - [Stacked Branches](#stacked-branches)
- [Worktree Management](#worktree-management)
- [GitHub and Linear](#github-and-linear)
  - [Review Requests](#review-requests)
  - [Review-only mode](#review-only-mode)
- [Startup and Review Scripts](#startup-and-review-scripts)
- [Settings](#settings)
- [Themes](#themes)
- [CLI Reference](#cli-reference)
- [Use am from your agents (MCP)](#use-am-from-your-agents-mcp)
- [How It Works](#how-it-works)
  - [Supported harnesses](#supported-harnesses)
- [Upgrading from 1.x](#upgrading-from-1x)
- [Troubleshooting](#troubleshooting)
- [Development](#development)

---

## Quick Start

```sh
# npm (GitHub Packages; see Installation for the one-time .npmrc setup)
npm install -g @roylet-gs/agent-monitor

# or Homebrew
brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
brew install agent-monitor

# then
am
```

On first launch a setup wizard asks whether you want the full dashboard or only the PRs waiting on your review ([review-only mode](#review-only-mode)), then picks your editor, installs the agent hooks (Claude Code, plus every other supported harness whose config dir exists, e.g. `~/.gemini`, `~/.codex`, `~/.cursor`), adds your repositories, configures the GitHub/Linear integrations and sets the context window for the context bar.

---

## Requirements

- **macOS or Linux** (arm64 or x64)
- **git**
- At least one supported coding agent with hook support, for agent monitoring: **Claude Code**, **Codex CLI**, **Gemini CLI**, **Cursor** (editor agent or `cursor-agent`), **GitHub Copilot CLI**, **Qwen Code**, **Factory Droid** or **Campfire**
- **gh CLI** (optional), for PR and CI status ([install](https://cli.github.com)); run `gh auth login` once
- **Linear API key** (optional), for ticket tracking
- **Node.js >= 18** only if you install through npm (the package is a small launcher for the native binary)

---

## Installation

Every install method gives you three binaries: `am` (the CLI, and the dashboard launcher), `am-tui` (the dashboard) and `am-hook` (the hook handler the agent CLIs run).

### npm (GitHub Packages)

The package is published to GitHub Packages, which requires authentication. Get the shared PAT from [1Password](https://start.1password.com/open/i?a=CFXLFRMHFNBWZLNUZISXEKVBPQ&v=kwxg7dvupd3om3qtfe2x3v7rqi&i=m6yhuhvalu4cwod6cwernzyfia&h=my.1password.com) (Gridsight members only), then configure npm once:

```sh
echo "//npm.pkg.github.com/:_authToken=YOUR_GITHUB_PAT" >> ~/.npmrc
echo "@roylet-gs:registry=https://npm.pkg.github.com" >> ~/.npmrc
```

Then install globally:

```sh
npm install -g @roylet-gs/agent-monitor
```

npm installs only the native package for your platform (`@roylet-gs/agent-monitor-darwin-arm64`, `-darwin-x64`, `-linux-arm64` or `-linux-x64`). Don't pass `--no-optional`, or the binaries won't be installed.

pnpm works the same way, using the same `.npmrc`:

```sh
pnpm add -g @roylet-gs/agent-monitor
```

### Homebrew

The repository is its own tap:

```sh
brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor
brew install agent-monitor
```

### From source

Requires a stable Rust toolchain ([rustup](https://rustup.rs)).

```sh
git clone https://github.com/roylet-gs/agent-monitor.git
cd agent-monitor
cargo install --path crates/am-cli    # am
cargo install --path crates/am-tui    # am-tui
cargo install --path crates/am-hook   # am-hook
```

The binaries land in `~/.cargo/bin`, which must be on your `PATH`.

Prebuilt tarballs for each platform (with `SHA256SUMS`) are also attached to every [GitHub Release](https://github.com/roylet-gs/agent-monitor/releases). Keep the three binaries in the same directory.

---

## First Run

Run `am`. If `settings.json` has no `setupCompleted` flag, the setup wizard walks through:

0. **Mode**: *Agents & worktrees* (the full dashboard) or *Review requests only* ([review-only mode](#review-only-mode), which skips the hooks and context-window steps, needs at least one repository, and turns GitHub and review requests on)
1. **Editor**: Cursor, VS Code or Terminal (what `Enter` opens worktrees in)
2. **Agent hooks**: installs am's hooks into `~/.claude/settings.json` and into each other harness in use (see [Supported harnesses](#supported-harnesses)), pointing at the native `am-hook`
3. **Repositories**: type a path (Tab completes) to start tracking a repo and its worktrees
4. **Integrations**: GitHub PR status (needs `gh`) and an optional Linear API key
5. **Review requests** (with GitHub on): show the PRs waiting on your review, whose requests count (you and your teams, only you, or only your teams), and whether to hide drafts and PRs you've already reviewed. Your choice sets up the review page's groups: an **Asked you** group and/or an **Asked your team** group, each hiding drafts and reviewed PRs if you said so (running setup again replaces only groups it made itself)
6. **Context window**: the token count the context bar is measured against: 1M (the default), 200k, or a custom value such as `128k`

Everything can be changed later (the mode too: **Settings → Mode**): `s` for settings (**Repositories…** manages the tracked repos), `A` to add a repository, `am hooks install` for the hooks. To run the wizard again, use **Settings → Run setup again…** or `am setup`. It starts from your current settings and repositories, nothing is saved until you finish it, and `Esc` on the first step closes it. **Settings → Reset settings…** (confirm with `y`) puts every setting back to its default and then opens the wizard. Repositories, worktrees, history, custom themes, startup scripts and agent hooks are kept. Run `am doctor` to check the setup.

---

## The Dashboard

One pane lists your worktrees, grouped by repository (with a header per repo when you track more than one), plus an **Other sessions** group for agent sessions running in directories am doesn't track. Worktrees you pull down to review (someone else's branch, the **tmp space**) sit together in one `Reviewing` group after your repos, tagged with their repo when you track several, and don't count against the worktree limit. Sessions from other harnesses carry a blue badge with their id (`✦ gemini`, `✦ codex`, `✦ cursor`…); Claude Code sessions have none. The selected row is marked with `▌`, and the pane's bottom border shows `N of M`. The bottom line shows the key hints; a short notice or an offline warning appears on its right.

Agent status changes show up within about 200 ms of the hook firing.

### Keyboard Shortcuts

| Key | Action |
| --- | ------ |
| `j` / `k`, `↑` / `↓` | Move the selection |
| `Home` / `G` (`End`) | Jump to top / bottom |
| `Ctrl-d` / `Ctrl-u` (`PgDn` / `PgUp`) | Page down / up |
| `Enter` | Open the worktree in your editor (and resume its last agent session, see below) |
| `g` / `o` | Open the PR in the browser (a merged PR opens its last active check) |
| `l` | Open the Linear ticket |
| `p` | Open the Linear project |
| `n` | New worktree (adds a repository first if none is tracked) |
| `d` | Delete the worktree; on an "Other sessions" row, stop tracking it |
| `v` | Review requests: the PRs waiting on your review, in place of the list (see [Review Requests](#review-requests)) |
| `V` | Review requests beside the list (in terminals with the kitty keyboard protocol — kitty, Ghostty, WezTerm, recent iTerm2 — holding Shift shows this on the key bar, and holding Ctrl shows `Ctrl+V`'s); `Tab` moves between them, and `V` again (or `Esc` on the review side) closes it |
| `Ctrl+V` | Review requests in a new terminal window (`am-tui --reviews`; leaving the page there closes it) |
| `Tab` | Focus the Jobs pane (answer its question, `x` dismisses a job); `Tab` again returns to the list. With the review page beside the list, `Tab` goes list → review page → Jobs pane. The Jobs pane stays in view on every page (Settings, help, dialogs) while it has jobs; on the review page only while a job needs an answer |
| `A` | Add a repository (folder browser: `⏎` adds a git repo or opens a folder, `←` goes up, type to filter or type a `/…`/`~/…` path) |
| `w` | Toggle the log pane (tail of `debug.log`) |
| `s` | Settings |
| `f` | Suggest a feature (files a GitHub issue via `gh`) |
| `r` | Refresh now, including GitHub/Linear (bypasses offline backoff) |
| `h` | Help overlay with every key and the status legend |
| `q` / `Esc` / `Ctrl-c` | Quit |

These are the defaults. To change them, open **Settings → Key bindings…**. It lists the actions by section (Worktrees, Review requests, General, Moving), each with its key, its default when you changed it, and what the key does on the review page, which uses the same keys (`⏎` pulls a PR or opens the pulled worktree, `f` opens the review filters, `r` clears the review cache and fetches again, `q` goes back; `—` means the key does nothing there). Pick an action, press `Enter`, then press the new key. If another action already has that key, the two swap. `x` puts one key back to its default and `R` resets them all. The key bar and the `h` overlay always show the keys you chose. Some keys can't be rebound, so the dashboard always stays usable: the arrows, `PgUp`/`PgDn`, `Home`/`End`, `Tab`, `Esc` and `Ctrl-c`. `o` keeps opening the PR unless you give it to another action. Only your changes are saved, under `keyBindings` in `settings.json`, e.g. `"keyBindings": { "newWorktree": "N", "feedback": "none" }`. A key can be one character, or one of `space`, `enter`, `backspace`, `left`, `right`, `ctrl-d`, `ctrl-u`, `ctrl-s`, `ctrl-v`; `none` unbinds the action.

Every other list moves the same way: Settings and its pages (repositories, harness plugins, key bindings, sort order, review groups, theme builder), the review page and the folder browser take `↑`/`↓`, `PgUp`/`PgDn`, `Home`/`End`, and (except where letters are typed, like the folder browser) `j`/`k`, `Ctrl-d`/`Ctrl-u` and `G`. Like the worktree list, they scroll only when the selection leaves the view, so moving within the visible rows never shifts the page.

### Mouse

The mouse is on by default (`mouseEnabled`, or `s` → General → Mouse):

- **Scroll** to move the selection; **click** a row to select it; **double-click** to open it (same as `Enter`).
- **Hover** over anything clickable to highlight it (a softer tint than the selection).
- The mouse works everywhere: in Settings and its pages (repositories, harness plugins, key bindings, sort order, theme builder), dialogs, the folder browser, the setup wizard and the jobs pane. Click a row to select it, and click it again to act on it, as `Enter` or `space` would. Key hints such as `⏎ save` or `esc cancel` are clickable buttons.
- **Click a key-bar hint** (`n new`, `s settings`, `? help`…) to run that action.
- **Click the Jobs pane** to focus it (scrolling then moves through the jobs); click a worktree row to go back to the list.
- In **Settings**, scroll to move, click a field to select it, and click it again to change it.
- Any click closes a popup or the help overlay.

While am captures the mouse, your terminal's click-and-drag text selection needs a modifier (usually `Shift`, or `Option` in iTerm2/Terminal.app). Turn `mouseEnabled` off to get plain selection back. The change applies immediately.

**Resume last session.** With `resumeLastSession` on (the default), `Enter` also picks up the worktree's most recent agent session (of whichever harness ran last; started at the worktree root or any subdirectory) unless one is already open:

- **Terminal** mode: the new terminal runs `claude --resume <id>` directly.
- **Cursor / VS Code** mode: `claude --resume <id>` is copied to the clipboard, a popup tells you so, and the editor opens. Paste it into the editor's terminal. am doesn't type into the editor's terminal for you, because that needs macOS Accessibility permission and fails silently without it.

Supported terminals: Terminal.app, iTerm2, Ghostty and Warp on macOS (the one you run am in), and `x-terminal-emulator` on Linux.

### Status Legend

| Glyph | Status | Meaning |
| ----- | ------ | ------- |
| `●` | executing | Claude is running tools or generating |
| `◇` | planning | Claude is in plan mode |
| `◐` | waiting | Claude needs you: a permission prompt, a question, or plan approval. The name is shown bold yellow |
| `◑` (green) | delegating | The main turn ended but background subagents are still running |
| `✓` | done | Claude finished a task and needs nothing from you |
| `○` | idle | The session started or ended without doing work |
| `·` | no agent | No Claude session has reported for this worktree |

See [STATUS_MAPPING.md](./STATUS_MAPPING.md) for exactly which hook events produce which status.

### Row Badges and Columns

After the name (a nickname, if set, followed by the branch in dim text):

- `•`: the status changed since you last selected this row
- `▶`: processes such as dev servers are running in the worktree (with `showRunningProcesses` on)
- `$ 2 commands` (bold teal, with the name turned teal too): an agent's shell command whose command line contains the `runningProcessFilter` (Settings → Process filter) is still running in the worktree; with no filter set, nothing is highlighted
- `◆ 2 subagents` (`◆2` on narrow terminals): background subagents currently running
- `⧉ 2 sessions` (`⧉2`): two or more agents are working in this worktree at once (see below)

Right-aligned columns, each of which can be turned off in settings:

- **Linear ticket**: e.g. `ENG-123`
- **PR**: `#42` coloured by state (green open, dim draft, yellow pending or changes requested, red failing, magenta merged, dim closed), followed by the checks glyph `✓` passing, `✗` failing or `◌` pending, and `⇡` when merged. Open PRs then show their review state: `approved` (bold green when checks pass and there are no merge conflicts, yellow while checks fail or still run), `conflicts` (approved and green but conflicting with the base), `changes` (changes requested), `review` (awaiting a required review) or `draft`; a PR closed without merging shows red `closed`. Terminals under 100 columns get a glyph instead (`✔` `⚠` `!` `…` `~` `⊘`), and `?` shows the legend. Nothing is shown when the repo doesn't require reviews or the PR only comes from a Linear attachment
- **Git**: `↑2` commits ahead, `↓1` behind, `±3` uncommitted changes
- **Age** of the last commit (`4m`, `3h`, `2d`, `3w`)
- **Context window**: `████▌░░░ 121k`, a bar of how much of the agent's context window the session has used, then the tokens in use. The bar and count are green, then yellow from 70% used and red from 90%. Usage comes from the session's Claude Code transcript, so it updates after every turn. It is measured against `contextWindowTokens` (1M by default). A session that has gone past that is measured against the setting doubled until it fits (200k → 400k → 800k …), whatever model it runs. The bar clears after a compaction until the next turn

At the bottom right of the list, `today 182M` is the number of tokens your agents have used today (local day). It covers input, cache writes, cache reads and output, subagents included, because each of these is billed. Cache reads usually make up 90% or more of the raw count, since every turn re-reads the whole conversation from the cache, though they cost only about a tenth of the input price. `dailyTokensMode` picks how the total is shown:

| Mode | Shows |
|---|---|
| `tokens` (default) | Every billed token, unweighted: `today 52M` |
| `weighted` | Input-equivalent tokens, each class weighted by its price ratio (cache read 0.1×, 5-minute cache write 1.25×, 1-hour cache write 2×, output 5×): `today 9.9M weighted` |
| `cost` | Estimated US dollars at Claude API list prices for each response's model: `today ~$28.90` (a trailing `+` means some usage had a model without a known price). Subscription plans aren't billed per token, so treat it as the API-equivalent cost |
| `breakdown` | Raw tokens per class: `today in 1k · write 2.2M · read 50M · out 216k` |

It is read incrementally from the session transcripts, so a refresh costs a few milliseconds. With `dailyTokensScope` set to `visible` it counts only sessions under a row on screen (`today (visible) 4.1M`), so filters and hidden rows change it. Turn it off with `showDailyTokens`.

When several adjacent worktrees share a Linear ticket, a `— ENG-123 Ticket title —` line groups them. When "project" is the first enabled sort criterion, `═ Project name ═` headers group rows by Linear project. Headers are never selectable.

### Several Agents in One Worktree

Each agent session keeps its own status, so two agents in the same worktree never overwrite each other. That holds for two Claude Code sessions as well as different harnesses. With two or more live sessions, the row shows `⧉ N sessions` and lists each one under it:

```
● feature/api ⧉ 3 sessions                          ENG-412 #127✓ ↑2  4m
 ├ ● claude #3a7c  executing · 2m ◆ 1 subagent                  ██▌░░░░░ 300k
 ├ ◑ claude #9e41  delegating · 1m ◆ 2 subagents                █░░░░░░░ 120k
 └ ✓ gemini  done · 30s
```

Each session row has its own status glyph, harness (with a short session id when two sessions share a harness), status and age, subagent badge and context bar. The worktree row's glyph is its most urgent session: waiting, then executing, planning, delegating, done, idle. Session rows aren't selectable, and clicking one selects its worktree. A session killed without ending properly drops off once am's process scan sees fewer agent CLIs running in the worktree than it has open sessions.

### Stacked Branches

If a worktree's branch is built on top of another worktree's branch, it's listed under its parent with tree lines:

```
● feature/api
├─ ● feature/api-auth
│  └─ ○ feature/api-auth-tests
└─ ✓ feature/api-docs
```

The parent comes from the open PR's base branch when it targets another worktree's branch (works with Graphite, `gh` stacks or manual stacks, and survives squash-rebases). Without such a PR, am uses git: the closest other worktree branch whose tip is among this branch's commits that aren't on `origin/<main>` yet.

---

## Worktree Management

Press `n` for the New Worktree form: repository (when you track several), branch name (prefilled with `defaultBranchPrefix`), an optional nickname, and the base branch. While you type the branch name, am checks in the background whether it already exists:

- `✓ exists on origin — Enter will pull it`: someone else's branch (say, a PR you're reviewing). Enter checks it out tracking `origin/<branch>` without asking, and the base field is skipped
- `✓ on origin and locally — Enter will ask first`: pulling resets your local copy to origin, so you're asked first
- `! exists locally only — Enter will offer to reuse it`

Creating and deleting run in the background: a **Jobs** pane opens on the right showing each step (fetch, `git worktree add`, sync…), the new worktree appears in the list straight away as a `creating…` placeholder, and the dashboard stays usable. Any follow-up question is asked in that pane; press `Tab` to focus it and answer (`Tab` again goes back to the list, `x` dismisses a finished or failed job). The pane closes by itself once nothing is running or waiting. If the repo has a startup script, the pane offers to run it; its output streams into the pane (set `startupScriptMode` to `window` to open it in a new terminal window instead).

Otherwise, if the branch exists, the Jobs pane offers the choices: pull the remote (track `origin/<branch>`, or reset the local branch to it), create disconnected (a new branch from the base with no tracking), reuse the existing local branch, or delete the local branch and re-create it from the base. Worktrees are created at `<repo>/.claude/worktrees/<branch>` (with `/` replaced by `-`), branching from `origin/<base>` when it exists.

Press `d` to delete. The dialog warns about uncommitted changes and unpushed commits, then asks whether to delete the local branch too. The `main`/`master` checkout can't be deleted. A main checkout on a non-default branch offers to delete just that branch, and a row whose directory has disappeared is cleaned up as a stale entry.

Worktrees created outside am (`git worktree add`, other tools) are picked up automatically within about 10 seconds.

**Worktree limit.** Turn on `worktreeLimitEnabled` to cap dedicated worktrees per repository at `maxWorktrees` (default 5; the main checkout doesn't count). Creation is blocked in both the dashboard and the CLI once the cap is reached.

---

## GitHub and Linear

**GitHub** (`ghPrStatus`, needs an authenticated `gh`): am fetches the PR for every worktree branch with batched `gh api graphql` calls (8 branches per call, a few calls in parallel, so a repo with hundreds of checks per PR stays under GitHub's 10 s request limit), every `ghPollingIntervalMs` (default 3 minutes), right away when a new branch appears, and when you press `r`. You get the PR number, draft/merged/closed state, review decision, mergeability (conflicts) and CI checks.

**Linear** (`linearEnabled` + `linearApiKey`): am looks up the Linear issue linked to each branch name (Linear's git-branch search, so branches named the way Linear suggests, e.g. `eng-123-fix-login`, match). You get the identifier, title, state, assignee and project. With `linearAutoNickname` on, worktrees without a nickname are named after their ticket's title. If GitHub has no PR for the branch, the PR attached to the Linear issue is shown instead (without checks).

**Offline.** Results are cached in the database, and only a successful fetch replaces them. If GitHub or Linear can't be reached, the dashboard keeps showing the last good data, along with `⚠ GitHub offline — cached` (or `Linear`, or both) on the key bar. am backs off from 30 seconds up to 15 minutes while offline. If GitHub rate-limits you, the hint reads `⚠ GitHub rate-limited — cached` and am backs off the same way; the review page says so too, with when the limit resets, instead of waiting on an empty list. Press `r` once you're back online to retry immediately; the warning clears on the next successful fetch.

### Review Requests

With GitHub on, am also looks for the open PRs in your tracked repos that are waiting on your review (one GitHub search every `reviewPollingIntervalMs`, default 2 minutes, plus on `r` and whenever you open the page). The count sits in the top-right corner of the dashboard's border as ` 3 to review ` (with groups, it counts each group shown on the dashboard by name, e.g. ` Platform 4 · 26 to review `, where `to review` is the unnamed default group; `reviewBadgeTotal` adds every open PR asking you or your teams on GitHub regardless of filters, ` · 60 on GitHub `, counted in the same search request); click it or press `v` to open the **Review requests** page, whose border carries the same badge (`V` opens it beside the worktree list, `Ctrl+V` in a new terminal window).
Pulling a PR into **Reviewing** (a checkout of someone else's branch, tracking origin, with no questions and no startup script) happens from this page; `am worktree create --tmp` does the same from the CLI. It lists each PR, with stacked PRs (one whose base is another listed PR's branch) indented under the PR they target as a tree, with its author, who was asked (`you` or `@team`), its Linear ticket (looked up by the PR's branch), CI and review state, and how long ago it changed. A pulled PR whose worktree has an agent in it shows the agent's status dot before its title, as on the dashboard:

| Key | Action |
| --- | ------ |
| `Enter` | On a pulled PR, open its worktree in your IDE (just like `Enter` on the dashboard); otherwise pull it into Reviewing, like `t` (a PR from a fork, which can't be pulled, opens in the browser). Clicking the selected row (or double-clicking one) does the same |
| `g` / `o` | Open the PR in the browser |
| `l` | Open its Linear ticket |
| `t` | Pull its branch into **Reviewing**. You stay on the page, so you can go down the list pulling several; the row shows `pulling…` and then `✓ pulled` (or `✗ pull failed`, and `t` tries again). On a PR that's already pulled, `t` selects that worktree on the dashboard (beside the list, the list takes the keys); a PR from a fork can't be pulled |
| `T` / `Ctrl+⏎` | **Pull + run script**: pull it like `t`, then run the repo's [review script](#startup-and-review-scripts) in the new worktree (it opens in a new terminal window, and a popup says so). If the repo has no review script yet, am asks whether to add one: `y` creates it from a template and opens it in your editor while the PR is pulled, and it runs once you save it (or press `⏎` on the question in the Jobs pane to run it as is). Only on a PR that isn't pulled yet. `Ctrl+⏎` needs a terminal that reports it (the kitty keyboard protocol: kitty, Ghostty, WezTerm, iTerm2 with the option on…); elsewhere use `T` |
| `d` | Remove the pulled worktree (the usual delete dialog), staying on the page |
| `f` | Filters: who's asked, the groups (each with its own order and filters), the badge's GitHub total, **Reset filters…** (who's asked, the default group's filters and the GitHub total back to defaults; groups kept) and **Reset groups…** (removes every group), both asking `y` first, in a small box over the page (changes save at once). The page's bottom border sums up what's in force; clicking it opens the box too |
| `r` | Clear the review caches (the list, group matches, GitHub total, autocomplete people) and search again now |
| `Esc` / `q` / `v` | Back to the dashboard (beside the list: `Esc`/`q`/`V` close the page and `v` gives it the whole screen) |
| `Tab` | Beside the list: hand the keys to the list (or the Jobs pane). On the full page: to the Jobs pane and back, while it shows. Pulls and removals show on their rows, so it only opens there when a job needs an answer (a branch that already exists locally, a review script waiting to be saved) |

Once you approve a PR you pulled, GitHub stops asking for your review, but the worktree is still there. While it is, the PR stays on the page, in an **Approved, still pulled** section at the bottom (merged or closed too, until you remove the worktree with `d`). A group of yours whose rules match it (say, *reviewed by you: yes*) still takes it; the default group never does. These PRs don't count toward the `to review` badge. am finds them by looking up the PRs of the branches in **Reviewing** that the search no longer lists (one extra GraphQL request, only when there are any).

### Review-only mode

If you only want the review page, pick **Review requests only** on the setup wizard's first step, or set **Settings → Mode** to `reviews` (`am settings set appMode reviews`). `am` then opens straight to the review page, full screen, and nothing else:

- `Esc`/`q` quits; there's no worktree list to go back to, so `V` and `Ctrl+V` do nothing and `t` on an already-pulled PR just says so (`⏎` opens it, `d` removes it). Pulls and removals show on their rows; the Jobs pane opens beside the page only when one needs an answer.
- Settings lists only what the page uses: mode, editor, repositories, key bindings, mouse, appearance, update check, the review-request settings, Linear (for each PR's ticket), logging and setup. Everything else keeps its value for when you switch back. Key bindings lists only the page's keys, and `h` shows the page's own help.
- am does no dashboard work in the background: no agent hooks are needed (`am doctor` doesn't warn about them), and there's no git status, process scan, context or token usage, or PR/ticket fetching for worktrees. It still lists worktrees, so it knows which PRs you've pulled.
- The `⬆ vX.Y.Z available` notice sits on the page's top border.

Switching **Settings → Mode** to `reviews` turns GitHub PR status and review requests on (the page needs them, so Settings hides their switches) and opens the page at once. Switching back to `full` keeps you on the page, and `Esc` then shows the dashboard. To open the page alone once without changing the mode, run `am reviews` (the same as `Ctrl+V`, in this terminal).

What counts is set under **Settings → Review requests** (or in the setup wizard): `reviewRequestsScope` (`all`: requested from you or a team you're on, `direct`: you by name, `teams`: only your teams) decides what's fetched, across every tracked repo. **Groups** (`f` → **Groups…**) split the page: each group takes the PRs its rules match and passes the rest down to the next, top to bottom; what no group takes falls to the **default group** at the bottom, which can have rules too: what it doesn't match is hidden (the bottom border then says `N hidden`). With no groups, everything is in the default group and the page looks as it always has. Each group (the default one too) has its own order, `direct` (PRs asking you by name first), `repo`, `updated` (newest first) or `oldest` (waiting longest), and its own filters: authors, who's asked (you by name, or one of your teams), specific teams, only / not in repos, draft or ready, CI checks, review state, whether you've already reviewed it, age (older than N days), whether it has a Linear ticket, text in the title or branch, and GitHub search terms (`label:urgent -author:app/dependabot`, checked by GitHub in the same request). A new default group has every filter off; settings from before groups (`reviewHideDrafts`, `reviewHideReviewed`, `reviewIncludeRepos`, `reviewExcludeRepos`, `reviewSearchQualifiers`, `reviewSort`) become the default group's filters and order. Rows always show the PR's `@author` in full; a long title is cut instead. All of a group's rules must match; a list (authors, teams, repos) matches any of its entries. On the groups page `a` adds a group under the selected one, `x` removes it, `J`/`K` move it, `⏎` edits it, `space` switches it off or on (a group that's off takes nothing, so its PRs fall to the next group; the default group off hides everything no group took) and `b` toggles whether it's counted on the dashboard. In a group's form, the authors, teams and repos lists autocomplete: people who can author PRs in your tracked repos (each repo's assignable users on GitHub, looked up at most once an hour and cached, plus the authors already on the page), the teams asked in the list, and your tracked repos; `⇥` (or `⏎` partway through a name) takes the highlighted suggestion, `↑`/`↓` pick, and a click takes one; an entry that matches no known person, team or repo is flagged with ⚠ (a group that never matches anything gets no section on the page). While the groups page is open it sits at the bottom of the page, and the page above previews the groups as you edit them, keystroke by keystroke, from the PRs already loaded: no GitHub calls until a group's GitHub search terms change. GitHub has no push channel a local app can subscribe to, so this is polling: one GraphQL request per refresh.

---

## Startup and Review Scripts

Each repository can have a shell script that runs after a new worktree is created, for example to install dependencies or copy `.env` files. After creating a worktree from the dashboard, am asks whether to run it; the script runs in your terminal, in the new worktree's directory, and am returns when you press Enter.

A repository can also have a **review script**, run by **pull + run script** (`T`) on the Review requests page once the PR's branch is checked out into Reviewing (plain pulls never run it). It opens in a new terminal window in the PR's worktree, so an agent it starts shows its full UI and you can step in (where no terminal window can be opened, it runs in the Jobs pane instead), with the PR in its environment: `AM_PR_NUMBER`, `AM_PR_URL`, `AM_PR_TITLE`, `AM_PR_AUTHOR`, `AM_PR_BRANCH`, `AM_PR_BASE`, `AM_REPO` (`owner/name`) and `AM_WORKTREE_PATH`. For example, to have Claude review the PR:

```sh
claude "Review PR #$AM_PR_NUMBER ($AM_PR_URL) against origin/$AM_PR_BASE and list bugs and risky changes." \
  --allowedTools "Read,Grep,Glob,Bash(git diff:*),Bash(git log:*),Bash(gh pr view:*)"
```

The session's hooks show its status on the worktree's row, and `Enter` on the row later resumes the conversation. When it runs in the Jobs pane instead, a headless `claude -p … --output-format stream-json --verbose` is shown one readable line per step (`→ Read src/parse.rs`, Claude's text, `✓ done in 2m 13s · 14 turns`).

```sh
am script edit     # create or open the current repo's startup script in your editor
am script show     # print it
am script remove   # delete it
am script edit --review   # the same for the review script (also show/remove)
```

Both can also be edited from **Settings → Repositories…**: `s` on a repo asks which script, with what each is for and where it runs; `⏎` edits it (creating it from a template), `x` removes it. Scripts are stored at `~/.agent-monitor/scripts/<repo-id>.sh` and `<repo-id>.review.sh`.

---

## Settings

Press `s` to open the settings editor:

| Key | Action |
| --- | ------ |
| `j` / `k` | Move |
| `Enter` / `Space` | Toggle, cycle a choice, or edit a text or number field |
| `←` / `→` (`h` / `l`) | Step a choice backwards or forwards |
| `e` | Open `settings.json` in your IDE (the `ide` setting, incl. a custom command; the dashboard reloads it when saved), or in `$VISUAL` / `$EDITOR` (default `vi`) when the IDE setting is `terminal`, for anything not listed |
| `Esc` / `q` / `s` | Close |

Changes are saved immediately. The **Sort order** field opens a sub-editor: `j`/`k` move, `Space` turns a criterion on or off, `d` flips its direction, `J`/`K` move it down/up, `Enter` saves, `Esc` cancels. Criteria apply in order, and the first one that tells two rows apart decides their order.

Settings are stored in `~/.agent-monitor/settings.json` and can also be read or changed with `am settings` (except `worktreeSort`, which is a list, and `keyBindings`, which is a map).

<details>
<summary><strong>All settings</strong></summary>

| Key | Default | Description |
| --- | ------- | ----------- |
| `ide` | `"vscode"` | What `Enter` (and `am open`) opens worktrees in: `vscode`, `cursor`, `zed`, `windsurf`, `terminal`, or `custom` |
| `ideCommand` | `""` | The command `ide: "custom"` runs, through `sh`. `{path}` is replaced by the quoted worktree path, or the path is appended when there's no `{path}`. E.g. `idea`, `subl -n`, `open -a Nova {path}` |
| `resumeLastSession` | `true` | `Enter` also resumes the worktree's latest agent session |
| `defaultAgent` | `"claude"` | Harness plugin `Enter` starts in a worktree with no session yet (set it in Settings → Harness plugins) |
| `disabledAgents` | `[]` | Harness plugins switched off: no hooks, no session discovery (Settings → Harness plugins) |
| `keyBindings` | `{}` | Dashboard keys you changed: action id (`open`, `openPr`, `openLinear`, `openProject`, `newWorktree`, `deleteWorktree`, `addRepo`, `reviewRequests`, `reviewSplit`, `reviewWindow`, `pullToTmpSpace`, `toggleLog`, `settings`, `feedback`, `refresh`, `help`, `quit`, `moveDown`, `moveUp`, `pageDown`, `pageUp`, `top`, `bottom`) → key. Edit it in Settings → Key bindings…; see [Keyboard Shortcuts](#keyboard-shortcuts) |
| `mouseEnabled` | `true` | Mouse support (click, double-click, scroll, hover highlight); off restores the terminal's own text selection |
| `startupScriptMode` | `"pane"` | Where the repo's startup script runs after creating a worktree: `pane` streams its output into the Jobs pane, `window` opens a new terminal window |
| `defaultBranchPrefix` | `"feature/"` | Prefix for new branch names |
| `defaultBaseBranch` | `"main"` | Default base branch for new worktrees |
| `pollingIntervalMs` | `30000` | Full git refresh interval (min 5000); a worktree whose agent acts is refreshed immediately |
| `theme` | `"ansi"` | A built-in theme or one of yours from `~/.agent-monitor/themes/`; see [Themes](#themes) |
| `worktreeSort` | main last › ticket › created (newest first) | Ordered sort criteria: `isMain`, `repo`, `linearTicket`, `linearProject`, `agentStatus`, `lastActivity`, `createdAt`, `branchName`, `prStatus`, `gitDirty`, each with a direction and on/off. Edit in the TUI |
| `hideMainBranch` | `true` | Hide the `main`/`master` checkout |
| `hideMergedClosedPrs` | `false` | Hide worktrees whose PR is merged or closed |
| `hideIdleDoneAgents` | `false` | Hide worktrees whose agent is idle, done or absent |
| `hideWithoutLinearTicket` | `false` | Hide worktrees with no Linear ticket |
| `showPrStatus` | `true` | Show the PR column |
| `showLinearTicket` | `true` | Show the ticket column |
| `showGitAheadBehind` | `true` | Show the ahead/behind/dirty column |
| `showLastCommit` | `true` | Show the last-commit age |
| `showContextWindow` | `true` | Show the context-window bar at the far right of each row |
| `contextWindowTokens` | `1000000` | Context window the bar is scaled to. A session past it is shown against this value doubled until it fits. In the settings editor `⏎` takes `128k`/`1m` and `←→` step through common sizes; `am settings set contextWindowTokens 200k` works too |
| `showDailyTokens` | `true` | Show today's token usage at the bottom right of the list |
| `compactView` | `false` | Compact view for small panes: panes keep only their top line, no blank rows, glyphs instead of words, no git/age columns, and the jobs pane and `V` review split open below the list |
| `dailyTokensScope` | `"all"` | What that total counts: `all` (every session today) or `visible` (only sessions under a row on screen) |
| `showRunningProcesses` | `false` | Detect processes (dev servers etc.) running inside each worktree (`▶`) |
| `runningProcessFilter` | `""` | Only count processes whose command line contains this text (case-insensitive) |
| `audioNotifications` | `false` | Play a macOS system sound when an agent starts waiting or finishes |
| `audioWaitingSound` | `"Glass"` | Sound for waiting |
| `audioDoneSound` | `"Funk"` | Sound for done |
| `appMode` | `"full"` | `full` (the dashboard) or `reviews` ([review-only mode](#review-only-mode): `am` opens to the review page alone) |
| `checkForUpdates` | `true` | Check GitHub Releases (at most every 6 h) and show `⬆ vX.Y.Z available` at the top right of the dashboard, plus a banner at the top of Settings with the upgrade command for how you installed am (`brew update && brew upgrade agent-monitor`, `npm install -g …@latest`, `pnpm add -g …@latest`, or the source steps) |
| `ghPrStatus` | `true` | Fetch GitHub PR and CI status |
| `ghPollingIntervalMs` | `180000` | GitHub refresh interval (min 30000) |
| `ghRefreshOnManual` | `true` | Include GitHub when you press `r` |
| `reviewRequestsEnabled` | `true` | Look for PRs waiting on your review (needs `ghPrStatus`) |
| `reviewRequestsScope` | `"all"` | `all` (you or your teams), `direct` (you by name) or `teams` (only your teams) |
| `reviewPollingIntervalMs` | `120000` | How often to search for review requests (min 30 s) |
| `reviewGroups` | `[]` | The review page's groups, top to bottom (edit them with `f` → Groups… on the page) |
| `reviewDefaultGroup` | catch-all | The group under them, catching the rest; what its rules don't match is hidden. Without one, the 2.1 keys `reviewHideDrafts`, `reviewHideReviewed`, `reviewIncludeRepos`, `reviewExcludeRepos`, `reviewSearchQualifiers` and `reviewSort` (still read, no longer shown) describe it |
| `reviewBadgeTotal` | `false` | The review badge also counts every open PR asking you or your teams on GitHub, unfiltered |
| `linearEnabled` | `false` | Fetch Linear tickets |
| `linearApiKey` | `""` | Linear personal API key |
| `linearPollingIntervalMs` | `180000` | Linear refresh interval (min 30000) |
| `linearRefreshOnManual` | `true` | Include Linear when you press `r` |
| `linearUseDesktopApp` | `false` | Open Linear links in the desktop app (`linear://`) |
| `linearAutoNickname` | `true` | Name worktrees after their Linear ticket title (never overwrites a name you set) |
| `worktreeLimitEnabled` | `false` | Block creating more than `maxWorktrees` worktrees per repo |
| `maxWorktrees` | `5` | Per-repo limit (excludes the main checkout) |
| `logLevel` | `"info"` | `debug`, `info`, `warn`, `error` |
| `maxLogSizeMb` | `2` | `debug.log` is rotated to `debug.log.1` past this size |

Keys from 1.x that the native app no longer uses (`autoSyncOnStartup`, `agentPermissionMode`, `agentClaudeArgs`) are kept in the file but ignored.

</details>

---

## Themes

Pick a theme in Settings (`s` → **Theme**, `←`/`→` to cycle) or with `am theme use <name>`. `ansi`, the default, uses your terminal's own 16 colours. The other themes set their own colours. Dark themes keep your terminal's background, while light and retro themes paint their own.

| Family | Themes |
| ------ | ------ |
| Terminal | `ansi` |
| Catppuccin | `catppuccin-mocha`, `catppuccin-macchiato`, `catppuccin-frappe`, `catppuccin-latte` (light) |
| Dark | `tokyonight`, `tokyonight-storm`, `gruvbox`, `dracula`, `nord`, `one-dark`, `solarized-dark`, `rose-pine`, `kanagawa`, `everforest`, `github-dark`, `monokai`, `nightfox` |
| Light | `solarized-light`, `github-light`, `gruvbox-light` |
| Retro | `dos` (MS-DOS EDIT blue), `turbo-pascal` (Borland IDE), `ibm-3270` (mainframe terminal), `green-phosphor` and `amber-phosphor` (IBM 5151 monochrome) |

### Theme builder

**Settings → Theme builder…** starts from the current theme. A built-in theme is copied as `<name>-custom`, and a custom theme is edited in place. The whole dashboard redraws in the draft as you edit, and a preview pane shows every status, PR badge and header.

| Key | Action |
| --- | ------ |
| `j` / `k` | Move between colour slots (text, faint, accent, border, borderFocus, selection, background, green, yellow, red, blue, magenta, teal) |
| `Enter` | Open the palette: the 16 terminal colours, a curated grid of 40 colours, and the colours already in your theme. Arrow keys move, `Enter` picks |
| `←` / `→` | Step the slot through the palette without opening it |
| `#` | Type a hex colour (`#rgb` or `#rrggbb`) |
| `x` / `X` | Clear this slot / all slots, so they come from the base again |
| `b` / `B` | Change the base theme (your own colours are kept) |
| `n` | Rename |
| `w` / `Ctrl-S` | Save to `~/.agent-monitor/themes/<name>.json` and switch to it |
| `Esc` | Close (press twice if there are unsaved changes) |

A theme file stores only the colours you changed. Anything missing comes from `base`, then from `ansi`. Colours can be `#rrggbb`, a terminal colour name (`red`, `lightblue`, `darkgray`…), or `default`. `selection` can also be `reverse`.

```json
{
  "name": "midnight",
  "base": "tokyonight",
  "colors": { "accent": "#f5c2e7", "background": "#11111b", "selection": "#313244" }
}
```

To share a theme, run `am theme export <name> -o file.json` on your machine (this writes every colour, so built-ins make good templates), then `am theme import file.json` on the other.

---

## CLI Reference

Every command that takes a `<target>` accepts a branch name or a worktree path. `--repo <path>` picks the repository; by default it's detected from the current directory (including from inside one of its worktrees). Most read commands accept `--json`.

```sh
am                                  # launch the dashboard
am --version
```

<details>
<summary><strong>Worktrees</strong> (<code>am worktree</code>, alias <code>am wt</code>)</summary>

| Command | Description | Options |
| ------- | ----------- | ------- |
| `am worktree list` / `am ls` | List worktrees with agent status, PR and Linear info | `--repo`, `--json` |
| `am worktree create <branch>` / `am new <branch>` | Create a worktree | `--repo`, `--name <name>`, `--base <branch>`, `--reuse` (use the existing local branch), `--track` (pull `origin/<branch>`), `--no-track` (new branch from base, no upstream), `--tmp` (check out an existing branch in the Reviewing space: one only on origin is tracked, one only local reused, without `--track`/`--reuse`; refused for a branch that doesn't exist), `--json` |
| `am worktree delete <target>` | Delete a worktree | `--repo`, `--force` (even with uncommitted changes), `--delete-branch`, `--delete-remote` |
| `am worktree open <target>` / `am open <target>` | Open in your configured editor | `--repo` |
| `am worktree info <target>` | Detailed info for one worktree | `--repo`, `--json` |
| `am worktree sync` | Sync the database with `git worktree list` | `--repo` |

</details>

<details>
<summary><strong>Repositories, sessions, PRs, tickets</strong></summary>

| Command | Description | Options |
| ------- | ----------- | ------- |
| `am repo list` | List tracked repositories | `--json` |
| `am repo add <path>` | Track a repository (and its existing worktrees) | `--json` |
| `am repo remove <name-or-path>` | Stop tracking a repository | |
| `am agent attach <target>` (`am a attach`) | Resume the worktree's latest agent session in this terminal (`claude --resume` / `gemini --resume`) | `--repo` |
| `am pr [target]` / `am pr show [target]` | PR info for a branch (default: current branch) | `--repo`, `--json` |
| `am pr open [target]` | Open the PR in the browser | `--repo` |
| `am linear [target]` / `am linear show [target]` | Linear ticket for a branch | `--repo`, `--json` |
| `am linear open [target]` | Open the ticket | `--repo` |
| `am script edit` / `show` / `remove` | Manage the repo's startup script (`--review`: its review script) | `--repo`, `--review` (`show` also `--json`) |
| `am status --worktree <path>` | Show the stored agent status | `--set <status>` (`idle`, `executing`, `planning`, `waiting`, `delegating`, `done`) |

</details>

<details>
<summary><strong>Setup and maintenance</strong></summary>

| Command | Description | Options |
| ------- | ----------- | ------- |
| `am hooks install` (`am install-hooks`) | Install hooks for Claude Code (`~/.claude/settings.json`) and every other agent CLI in use (Gemini: `~/.gemini/settings.json`) | `-a, --agent <id>` (repeatable) |
| `am hooks uninstall` (`am uninstall-hooks`) | Remove am's hooks (other hooks are kept) | `-a, --agent <id>` |
| `am hooks status` | Check which agents have hooks installed | `--json` |
| `am mcp` | Run the MCP server on stdin/stdout (what agent CLIs launch; see below) | |
| `am mcp install` | Register the MCP server with Claude Code, every other agent CLI in use that reads MCP config (Gemini, Qwen, Cursor), and Claude Desktop when installed | `-a, --agent <id>` (repeatable) |
| `am mcp uninstall` | Remove the registration (other servers are kept) | `-a, --agent <id>` |
| `am mcp status` | Check which agents have the MCP server registered | `--json` |
| `am settings list` | Show all settings | `--json` |
| `am settings get <key>` | Get one setting | `--json` |
| `am settings set <key> <value>` | Set one setting (booleans and numbers are parsed) | |
| `am settings reset` | Reset all settings to defaults (keeps `setupCompleted`) | |
| `am theme list` | Built-in and custom themes; `*` marks the active one | `--json` |
| `am theme show <name>` | A theme's colours (with swatches in a terminal) | `--json` |
| `am theme use <name>` | Switch theme (same as `am settings set theme <name>`) | |
| `am theme export <name>` | Print a theme as a standalone JSON file | `-o, --output <file>` |
| `am theme import <file>` | Validate a theme file and copy it into `~/.agent-monitor/themes/` | `--name <name>`, `--force` |
| `am theme remove <name>` | Delete a custom theme (switches back to `ansi` if it was active) | |
| `am doctor` | Check the app directory, database, settings, hooks, `am-hook`, `gh`, `ttyd`, theme, Linear | `--json` |
| `am perf` | How long each part of am has been taking (UI frames, the collector, git, GitHub/Linear, hooks), from the log, against per-stage budgets | `--since <window>` (`30m`, `1h`, `2d`, `all`; default `24h`), `--json` |
| `am logs` | Show recent log lines | `-n, --lines <n>` (default 50), `-f, --follow`, `--level <level>` (that level and above), `--module <name>`, `--clear`, `--open` (in your editor, or the folder once rotated) |
| `am setup` | Open the dashboard with the setup wizard (it also runs automatically on first run; also Settings → Run setup again…) | |
| `am reviews` | Open only the review-requests page, this once (`appMode` is unchanged; `Esc` quits). Alias `am review` | |
| `am hook-event` | Receive a hook event on stdin (used by the agent CLIs; same handler as `am-hook`) | `-w, --worktree <path>`, `-e, --event <name>`, `-a, --agent <id>` |

</details>

---

## Use am from your agents (MCP)

`am mcp` is a stdio [MCP](https://modelcontextprotocol.io) server that gives coding agents am's commands as tools, so an agent can list worktrees with their PR, CI and Linear state, create a worktree for a subtask, or check another agent's status. Register it once:

```bash
am mcp install                         # Claude Code, plus Gemini CLI / Qwen Code / Cursor / Claude Desktop when in use
am mcp install --agent claude          # just one
am mcp install --agent claude-desktop  # Claude Desktop (quit with ⌘Q and reopen to load it)
# or by hand:
claude mcp add --scope user agent-monitor -- am mcp
```

Each tool runs the matching `am` command (with `--json` where it has one) in the agent's working directory, so tools default to the repository the agent is in; pass `repo` to target another one. Claude Desktop starts the server outside any repo: with one tracked repo the tools use it, otherwise pass `repo`.

| Tools | Command |
| ----- | ------- |
| `worktree_list`, `worktree_info` | `am worktree list` / `info` |
| `worktree_create`, `worktree_delete`, `worktree_sync`, `worktree_open` | `am worktree create` / `delete` / `sync` / `open` |
| `repo_list`, `repo_add`, `repo_remove` | `am repo …` |
| `pr_show`, `pr_open`, `linear_show`, `linear_open` | `am pr …`, `am linear …` |
| `agent_status` | `am status` (get, or `set`) |
| `settings_list`, `settings_get`, `settings_set`, `settings_reset` | `am settings …` |
| `theme_list`, `theme_show`, `theme_use` | `am theme …` |
| `hooks_status`, `hooks_install`, `hooks_uninstall` | `am hooks …` |
| `script_show`, `script_remove` | `am script …` |
| `doctor`, `logs`, `perf` | `am doctor`, `am logs`, `am perf` |

Each tool is marked read-only, destructive or open-world (it talks to GitHub/Linear or opens an app), so your agent CLI can ask before running the risky ones. Commands that need the terminal (the dashboard, `agent attach`, `script edit`, `logs --follow`) have no tool. Calls are logged under the `mcp` module (`am logs --module mcp`).

---

## How It Works

```
Agent CLI hook ──▶ am-hook ─▶ SQLite (WAL) ◀── am-tui collector (PRAGMA data_version, every 200 ms)
                                    ▲
           gh / Linear ─▶ integrations thread (cached in the DB)
```

1. **The agent CLI runs `am-hook`** on every hook event (Claude Code: 11 events covering tool use, prompts, stops, notifications, permission requests, session and subagent lifecycle; other harnesses run it with `--agent <id>`, e.g. Gemini CLI's 7 events with `--agent gemini`). It takes about 3 ms, translates the agent's native event into am's canonical one, maps it to a status and writes it to SQLite. It never blocks or fails the agent. See `docs/agent-providers.md`.
2. **Subagents are tracked individually** by their agent id, so a background subagent's tool calls don't overwrite the parent's status, and the worktree shows **delegating** until the last one finishes.
3. **The dashboard** watches SQLite's change counter, so a hook write shows up within about 200 ms. Git status runs on its own thread, for worktrees whose agent just acted, plus a sweep every `pollingIntervalMs` that only re-checks worktrees whose git files changed (and all of them every fourth sweep, or on `r`). Terminals, editor windows and dev servers are detected with a native process scan every 2 seconds.
4. **GitHub and Linear** are fetched on a background thread, batched per repository, and cached in the database so the data survives restarts and outages.

Everything is local: the database, settings and logs live in `~/.agent-monitor/` (or `$AM_DATA_DIR`).

| File | Purpose |
| ---- | ------- |
| `agent-monitor.db` | SQLite database: repositories, worktrees, agent status, subagents, cached GitHub/Linear data |
| `settings.json` | Your settings |
| `debug.log` (`debug.log.1`) | Log, rotated at `maxLogSizeMb` |
| `scripts/<repo-id>.sh`, `scripts/<repo-id>.review.sh` | Per-repo startup and review scripts |

See [CLAUDE.md](./CLAUDE.md) for the full architecture.

### Supported harnesses

A harness is the CLI or editor that runs the agent and fires the hooks. `am hooks install` (or the setup wizard) installs into every one whose config dir exists; `am hooks install --agent <id>` picks one.

| Harness | id | Hooks written to | Waiting shown for | Resume |
| ------- | -- | ---------------- | ----------------- | ------ |
| Claude Code | `claude` | `~/.claude/settings.json` | permission prompts, questions | `claude --resume <id>` |
| Gemini CLI | `gemini` | `~/.gemini/settings.json` | tool permission, questions | `gemini --resume <id>` |
| Codex CLI | `codex` | `~/.codex/hooks.json` (trust it in `/hooks`) | permission requests | `codex resume <id>` |
| Qwen Code | `qwen` | `~/.qwen/settings.json` | permission prompts | `qwen --resume <id>` |
| Factory Droid | `droid` | `~/.factory/hooks.json` (or `settings.json` when that's the one Droid reads) | permission prompts | `droid --resume <id>` |
| GitHub Copilot CLI | `copilot` | `~/.copilot/hooks/agent-monitor.json` | questions | `copilot --resume <id>` |
| Cursor (editor agent, `cursor-agent`) | `cursor` | `~/.cursor/hooks.json` | — (no permission hook) | `cursor-agent --resume <id>` |
| Campfire | `campfire` | `~/.campfire/agent/extensions/agent-monitor-campfire.ts` (an extension am writes and owns) | a collaborator waiting to join or asking for a capability | `campfire --session <id>` |

Any other CLI whose hooks run a command with a JSON payload on stdin can be added without code as a JSON manifest in `~/.agent-monitor/agents/` (`docs/agent-providers.md`). Not supported yet: OpenCode and Amp (plugins only, no command hooks), Aider (no hooks), Windsurf and Crush (no session or turn-end events), JetBrains Junie (its `Stop` hook carries no session or directory) and Augment Auggie (hook commands must be bare script paths).

---

## Upgrading from 1.x

Version 2 is a native rewrite (Rust) of the TypeScript app. Install it the same way (`npm install -g @roylet-gs/agent-monitor`) or switch to Homebrew, then **run this once**:

```sh
am hooks install
```

Update with the same package manager you installed 1.x with (`npm install -g @roylet-gs/agent-monitor` or `pnpm add -g @roylet-gs/agent-monitor`) and check `am --version` prints `am 2.x`.

This points your Claude Code hooks at the native `am-hook` binary by absolute path. Old 1.x hook entries (`am hook-event`) keep working, but on npm installs they go through the Node launcher on every event; reinstalling removes that overhead. `am doctor` reports whether the native hook is found.

- **Your data carries over unchanged.** The database, settings, startup scripts and cached PR/Linear data are read in place; nothing needs migrating or re-adding.
- **Layout:** the detail panel is gone. PR, CI, ticket and git info now sit in the worktree row, and the `?` overlay lists every key.
- **Preview builds:** if you installed the pre-release binaries with `cargo install` (`am-rs`, `am-tui`, `am-hook` in `~/.cargo/bin`), remove them so they don't shadow the release on your `PATH`: `cargo uninstall am-cli am-tui am-hook`, then run `am hooks install` again.
- **Removed:** managed (headless) Claude sessions and the chat view (`am agent send|list|log|stop`), and `am daemon`. Use `am agent attach` to resume a session in a terminal instead. The Unix socket (`am.sock`) is no longer used.

---

## Troubleshooting

Run `am doctor` first. It checks the data directory, database, settings, the hooks of each agent CLI in use, the native `am-hook` binary, `gh`, and your Linear key.

`am logs` shows the debug log, with filters:

```sh
am logs --level warn            # warnings and errors only
am logs -f                      # follow
am logs --module hook-event     # one module (e.g. hook-event, integrations, tui, sync)
am logs -n 500 --level debug    # more history (set logLevel to debug first)
am logs --open                  # the whole file in your editor
```

You can also press `w` in the dashboard to see the log live, or use **Settings → View logs…** to open it in your editor (the `Open in` setting; once the log has rotated into `debug.log.1`, it opens the log folder in Finder instead).

**The dashboard feels slow.** `am perf` shows how long each part has been taking against its budget, and `am doctor` warns when one was over budget in the last hour:

```
Stage               Samples      p50      p95       Max  Budget (p95)  Status
collector.cycle          91  17.38ms  77.11ms  735.16ms      500.00ms  was slow
collector.procscan       74  15.28ms  22.75ms   39.55ms       50.00ms  ok
ui.frame                155   0.23ms   0.54ms    1.67ms       16.00ms  ok
```

am logs a summary every 5 minutes (and when it quits), plus a `[perf]` warning when a stage's p95 goes over budget. In a large repo, `git status` is the usual cost; `git config core.fsmonitor true && git config core.untrackedCache true` in that repo makes it much cheaper.

<details>
<summary><strong>Common issues</strong></summary>

**Agent status isn't updating**

- `am hooks status` should report the hooks as installed; if not, run `am hooks install`.
- After upgrading from 1.x, run `am hooks install` once so the hooks use `am-hook`.
- **Repositories:** `s` → **Repositories…** lists the tracked repositories with their worktree count and whether they have a startup script. `a` adds one through the folder browser, `x` stops tracking one (after a `y`; its worktrees and files stay on disk), and `s` asks which of its scripts to open: the startup script or the review script (run by `T` on the review page, shown as a `review` tag), each with a line on what it's for and where it runs; `⏎` opens it in your editor (created from a template if missing) and `x` removes it.
- **Harness plugins:** `s` → **Harness plugins…** lists every harness plugin (the built-ins below, and any JSON manifests in `~/.agent-monitor/agents/`) with its hook status. Space turns one on or off, `i` reinstalls its hooks, and `d` makes it the default for `Enter` in a worktree with no session yet. To add an agent CLI without code, see `docs/agent-providers.md`.
- **Codex CLI:** Codex runs a user hook only after you trust it in `/hooks`, and again after `am hooks install` rewrites it.
- **Cursor:** Cursor also runs Claude Code's hooks by default; am ignores those for Cursor sessions and tracks them through Cursor's own `~/.cursor/hooks.json`. Cursor has no permission hook, so its sessions never show `waiting`, and the `cursor-agent` CLI may not fire every hook the editor does.
- **Campfire:** Campfire has no command hooks, so `am hooks install` writes a small extension, `agent-monitor-campfire.ts`, into Campfire's `extensions/` directory (`$CAMPFIRE_CODING_AGENT_DIR`, else `~/.campfire/agent`) that runs `am-hook` on session and turn events and when a collaborator waits to join or asks for a capability. Only the session's host is shown (`CAMPFIRE_SESSION_ROLE=host`); a joiner's view reports nothing. Campfire loads extensions at startup, so restart running sessions after installing.
- **Copilot CLI / Factory Droid:** both read hooks at startup, so restart running sessions after installing. Copilot's permission events aren't available in the hook format am uses, so only questions show as `waiting`.
- **Gemini CLI:** hooks need a Gemini CLI version with hook support (`npm i -g @google/gemini-cli` to upgrade), and Gemini runs hooks only in folders it trusts. Trust the repository's parent directory so new worktrees are covered. Gemini allows comments in `~/.gemini/settings.json`; if it has any, `am hooks install` refuses to rewrite the file (so the comments aren't lost). Remove them, or add the entries shown in `docs/agent-providers.md` by hand.
- Check `am logs --module hook-event` while the agent is working. No lines means the hooks aren't firing; for more detail, `am settings set logLevel debug`.
- A session in a directory am doesn't track shows up under **Other sessions**. Add the repository with `A` or `am repo add <path>`.

**`am-tui not found` or `no native build for …`**

- npm: reinstall without `--no-optional`. Only macOS and Linux on arm64/x64 are supported.
- From source: install all three crates, and make sure `~/.cargo/bin` is on your `PATH`.

**No sounds**

- Sounds are macOS only and off by default: `am settings set audioNotifications true` (or `s` → Notifications → Sounds).
- Picking a sound in the settings editor plays it, so you can check your volume.
- A sound plays when a worktree's status changes to waiting or done while `am` is open. `am logs --module audio` shows each sound played, or why it couldn't play.

**PR status isn't showing**

- `gh auth status` must succeed inside the repository.
- `am settings get ghPrStatus` should be `true`; `am pr <branch>` shows what am sees.

**Linear tickets aren't linking**

- `am settings get linearEnabled` should be `true`, with `linearApiKey` set.
- The branch has to be linked to the issue in Linear, which happens when it's named the way Linear suggests (containing the identifier, e.g. `eng-123-…`). `am linear <branch>` shows what am sees.

**`⚠ … offline — cached` on the key bar**

- The last GitHub or Linear fetch failed, so you're seeing the last data that was fetched successfully. It's real, just possibly out of date.
- Usual causes: no network, an expired `gh` login or Linear key, or rate limiting. Check `am doctor` and `am logs --module integrations`.
- Press `r` once you're back online; the hint clears on the next successful fetch.

**Database issues**

- `am doctor` checks that the database opens. Schema upgrades run automatically.
- As a last resort, delete `~/.agent-monitor/agent-monitor.db` and re-add your repositories.

</details>

---

## Development

```sh
cargo build --workspace
cargo run -p am-tui                                     # the dashboard
cargo run -p am-cli -- ls                               # the CLI
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm test:e2e                                           # Docker E2E: only the specs this branch affects
pnpm test:e2e:all                                       # Docker E2E: every spec
```

Set `AM_DATA_DIR=/tmp/am-dev` to run against an isolated data directory. See [CLAUDE.md](./CLAUDE.md) for the architecture, test setup and release process.

### Running a local build alongside the installed `am`

The installed `am` (from npm, Homebrew or `cargo install`) is left alone. Build the checkout and run its binaries from `target/` directly:

```sh
cargo build --workspace                  # target/debug/{am,am-tui,am-hook}
./target/debug/am                        # dashboard: runs the am-tui next to it, not the installed one
./target/debug/am --watch                # same, with the log pane open
./target/debug/am ls                     # any CLI command
cargo run -p am-tui                      # or rebuild + launch the dashboard in one step
```

A handy alias while iterating: `alias amd="cargo build --workspace -q && ./target/debug/am"`.

**Which data it uses.** By default the dev build reads and writes your real `~/.agent-monitor` (database, settings, themes). It sees your actual worktrees, and the installed `am` keeps working because schema changes are additive. To experiment without touching that data, use a throwaway directory:

```sh
export AM_DATA_DIR=/tmp/am-dev           # separate DB, settings.json, debug.log, themes/
./target/debug/am repo add .             # track this repo in the scratch data dir
./target/debug/am                        # the setup wizard runs on first launch
```

**Hooks.** Claude Code's hooks point at the *installed* `am-hook` by absolute path, so agent status still comes from the installed binary while you run a dev dashboard. That's fine unless you're changing the hook code (`crates/am-hook`, `am_core::status`). To test hook changes, point the hooks at your build and switch them back afterwards:

```sh
./target/debug/am hooks install          # hooks now run target/debug/am-hook
#   … use Claude Code, watch the dashboard …
am hooks install                         # back to the installed am-hook
```

You can also replay one event without touching `~/.claude`: `echo '{"hook_event_name":"Stop","session_id":"s1"}' | ./target/debug/am-hook hook-event --worktree "$PWD" --event Stop`.

**Debug logging.** Run `./target/debug/am settings set logLevel debug` (or use Settings → Log level), then `./target/debug/am logs -f --module tui`, or press `w` in the dashboard. If you set `AM_DATA_DIR`, the log is in that directory.
