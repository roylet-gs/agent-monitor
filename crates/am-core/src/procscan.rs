//! Native process scanning: replaces the `lsof`/`ps` shell-outs in
//! `src/lib/process.ts`.
//!
//! [`capture`] takes one snapshot of the process table per poll cycle (pid,
//! parent pid, executable, cwd and — only where it can matter — argv). Every
//! per-worktree question is then answered from that snapshot by pure code that
//! is unit-tested with fixture processes:
//!
//! - [`ProcessSnapshot::has_terminal`] — an *interactive* shell sits in the
//!   worktree. `zsh -c …` wrappers (Claude's Bash tool, scripts) don't count.
//! - [`ProcessSnapshot::open_ide`] — VS Code or Cursor has the worktree open.
//! - [`ProcessSnapshot::running_processes`] — dev servers etc., attributed to
//!   their most-specific worktree root.
//!
//! # Backend
//!
//! macOS uses the libproc calls from `libc` directly (`proc_listallpids`,
//! `proc_pidinfo(PROC_PIDTBSDINFO | PROC_PIDVNODEPATHINFO)`, `proc_pidpath`,
//! `sysctl(KERN_PROCARGS2)`); Linux reads `/proc`. Both share the same capture
//! plan: parent + cwd for every process, then executable path only for
//! processes that sit in a directory other than `/` *and their ancestors*, and
//! argv only for the former. Daemons and app helpers almost all have cwd `/`, so
//! this skips most of the expensive argv reads. Measured on an M-series Mac with
//! ~900 processes: ~2.4 ms per snapshot, vs ~5 ms for `sysinfo` refreshing only
//! cwd + cmd + exe (fresh or reused `System`), vs hundreds of ms for the old
//! `lsof` + `ps` pair.
//!
//! Processes we may not inspect (other users', with EPERM on cwd) are simply
//! invisible to the per-worktree queries, which is fine: a worktree's tools run
//! as the user.
//!
//! # IDE detection heuristic
//!
//! VS Code / Cursor launched from the Dock carry no workspace on argv, so the
//! old argv parse could never work. What the editor *does* do is start each
//! window's extension host (and other `Code Helper (Plugin)` processes) with its
//! cwd set to the window's first workspace folder, and start integrated-terminal
//! shells as direct children of its pty host. So a worktree has an IDE open when
//! some process whose cwd is inside the worktree is either:
//!
//! 1. itself an editor executable (anything inside `Visual Studio Code.app`,
//!    `Cursor.app`, … or a Linux `code`/`cursor` binary or remote server), or
//! 2. an interactive shell whose *direct parent* is an editor process (an
//!    integrated terminal, at its current cwd).
//!
//! Only the direct parent counts for shells, so a Claude session started in a
//! VS Code terminal does not make every directory its tools `cd` into look like
//! an open editor. Limits: a window with no folder (cwd `/` or `$HOME`) is never
//! attributed; a multi-root workspace is only attributed to its first folder
//! (plus any folder an integrated terminal is in); Remote-SSH / Dev Container
//! windows are only seen when am runs on the remote side (the server binary
//! lives in `~/.vscode-server` / `~/.cursor-server`); and an integrated terminal
//! the user `cd`s into another worktree attributes that worktree too.
//!
//! # Nested worktrees
//!
//! `has_terminal` / `open_ide` take a single root, so they cannot use the
//! most-specific-root rule from `running_processes`. Instead a cwd only counts
//! for a root when no directory between the root and the cwd (excluding the
//! root, including the cwd) contains a `.git` entry — i.e. the cwd isn't inside
//! a nested worktree, clone or submodule. Without this, a main checkout that
//! keeps worktrees under `.claude/worktrees/` would light up whenever any of
//! them had a terminal open.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::model::{Ide, RunningProcess};


/// Ancestor walks stop after this many hops (guards against pid-reuse cycles).
const MAX_DEPTH: usize = 64;

/// Longest label `running_processes` produces, in characters.
const LABEL_MAX: usize = 40;

/// One process as captured from the OS (or built by hand in tests).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: i32,
    pub ppid: i32,
    /// Short name: executable file name, else the kernel's command name.
    pub name: String,
    /// Executable path. Only captured for processes in a non-`/` cwd and their
    /// ancestors.
    pub exe: Option<PathBuf>,
    /// `None` when unreadable (permissions, zombie, raced exit).
    pub cwd: Option<PathBuf>,
    /// Only captured for processes in a non-`/` cwd.
    pub argv: Vec<String>,
}

/// One scan of the process table. Cheap to query repeatedly.
#[derive(Debug, Clone, Default)]
pub struct ProcessSnapshot {
    procs: BTreeMap<i32, ProcInfo>,
    /// The scanning process; it and its parent are never reported.
    self_pid: i32,
}

/// Scan the process table once. Never fails: unreadable processes are skipped
/// and a total failure yields an empty snapshot.
pub fn capture() -> ProcessSnapshot {
    // Runs every 2 s in the TUI: deliberately not logged per scan.
    ProcessSnapshot::from_processes(os::scan(), std::process::id() as i32)
}

impl ProcessSnapshot {
    /// Build a snapshot from already-gathered processes (fixtures in tests).
    pub fn from_processes(procs: impl IntoIterator<Item = ProcInfo>, self_pid: i32) -> Self {
        ProcessSnapshot { procs: procs.into_iter().map(|p| (p.pid, p)).collect(), self_pid }
    }

    /// Every captured process, ordered by pid.
    pub fn processes(&self) -> impl Iterator<Item = &ProcInfo> {
        self.procs.values()
    }

    /// An interactive shell (zsh/bash/fish/nu/sh/dash) has its cwd inside
    /// `worktree_root` (and not inside a worktree nested beneath it).
    pub fn has_terminal(&self, worktree_root: &Path) -> bool {
        let root = canonical(worktree_root);
        self.procs
            .values()
            .any(|p| is_interactive_shell(p) && cwd_belongs_to(p.cwd.as_deref(), &root, has_git_entry))
    }

    /// Claude Code shell commands running in `worktree_root` (not in a
    /// worktree nested beneath it). Claude wraps every Bash tool command in
    /// `<shell> -c source ~/.claude/shell-snapshots/…`, so while its agents are
    /// between turns these are the background commands they're waiting on.
    pub fn claude_shell_commands(&self, worktree_root: &Path) -> usize {
        let root = canonical(worktree_root);
        self.procs
            .values()
            .filter(|p| is_claude_shell_command(p) && cwd_belongs_to(p.cwd.as_deref(), &root, has_git_entry))
            .count()
    }

    /// Which editor, if any, has `worktree_root` open. See the module docs for
    /// the heuristic and its limits.
    pub fn open_ide(&self, worktree_root: &Path) -> Option<Ide> {
        let root = canonical(worktree_root);
        for p in self.procs.values() {
            if !cwd_belongs_to(p.cwd.as_deref(), &root, has_git_entry) {
                continue;
            }
            if let Some(ide) = self.ide_signal(p) {
                return Some(ide);
            }
        }
        None
    }

    /// Ports `getWorktreeProcesses` + `processesForWorktree`: every process
    /// whose cwd is inside one of `worktree_roots`, attributed to the
    /// most-specific (deepest) root containing it. Shells, Claude and the tools
    /// it runs directly (MCP servers), editors and their helpers, and am itself
    /// are excluded. A non-blank `filter` keeps only processes whose full
    /// command line contains it (case-insensitive).
    ///
    /// Keys are the roots exactly as passed in; roots without processes are
    /// absent. Each list is ordered by pid.
    pub fn running_processes(&self, worktree_roots: &[PathBuf], filter: &str) -> HashMap<PathBuf, Vec<RunningProcess>> {
        let roots: Vec<PathBuf> = worktree_roots.iter().map(|r| canonical(r)).collect();
        let needle = filter.trim().to_lowercase();
        let self_parent = self.procs.get(&self.self_pid).map(|p| p.ppid);
        let mut out: HashMap<PathBuf, Vec<RunningProcess>> = HashMap::new();

        for p in self.procs.values() {
            let Some(cwd) = p.cwd.as_deref() else { continue };
            let Some(idx) = most_specific_root(cwd, &roots) else { continue };
            if p.pid == self.self_pid || Some(p.pid) == self_parent {
                continue;
            }
            if self.exclusion(p).is_some() {
                continue;
            }
            if !needle.is_empty() && !command_line(p).to_lowercase().contains(&needle) {
                continue;
            }
            out.entry(worktree_roots[idx].clone())
                .or_default()
                .push(RunningProcess { pid: p.pid, command: friendly_command_label(&p.argv, &p.name) });
        }
        out
    }

    /// Agent CLI processes per worktree root, by harness id: how many sessions
    /// of each harness are really running there (attributed to the
    /// most-specific root, like `running_processes`). A CLI that relaunches
    /// itself (its parent is the same harness) counts once.
    pub fn agent_sessions(&self, worktree_roots: &[PathBuf]) -> HashMap<PathBuf, HashMap<&'static str, usize>> {
        let roots: Vec<PathBuf> = worktree_roots.iter().map(|r| canonical(r)).collect();
        let mut out: HashMap<PathBuf, HashMap<&'static str, usize>> = HashMap::new();
        for p in self.procs.values() {
            let Some(id) = agent_cli(p) else { continue };
            if self.procs.get(&p.ppid).and_then(agent_cli) == Some(id) {
                continue;
            }
            let Some(cwd) = p.cwd.as_deref() else { continue };
            let Some(idx) = most_specific_root(cwd, &roots) else { continue };
            *out.entry(worktree_roots[idx].clone()).or_default().entry(id).or_default() += 1;
        }
        out
    }

    /// Why `p` is not a user process worth showing, if it isn't.
    fn exclusion(&self, p: &ProcInfo) -> Option<Cow<'static, str>> {
        if is_shell(p) {
            return Some(Cow::Borrowed("shell"));
        }
        if let Some(agent) = agent_cli(p) {
            return Some(Cow::Owned(agent.to_owned()));
        }
        if is_editor(p) {
            return Some(Cow::Borrowed("editor"));
        }
        if is_agent_monitor(p) {
            return Some(Cow::Borrowed("agent-monitor"));
        }
        // Tooling spawned directly (no shell in between) by an agent CLI, an editor
        // or am: MCP servers, language servers, extension helpers, am's own
        // `git status` polls. A dev server started from Claude's Bash tool or a
        // terminal always has a shell between it and its owner, so it is kept.
        for anc in self.ancestors(p) {
            if is_shell(anc) {
                return None;
            }
            if let Some(agent) = agent_cli(anc) {
                return Some(Cow::Owned(format!("spawned by {agent}")));
            }
            if ide_of(anc).is_some() {
                return Some(Cow::Borrowed("spawned by an editor"));
            }
            if is_agent_monitor(anc) {
                return Some(Cow::Borrowed("spawned by agent-monitor"));
            }
        }
        None
    }

    /// The editor `p` is evidence of, if any (see module docs).
    fn ide_signal(&self, p: &ProcInfo) -> Option<Ide> {
        if let Some(ide) = ide_of(p) {
            return Some(ide);
        }
        if is_interactive_shell(p) {
            return self.procs.get(&p.ppid).and_then(ide_of);
        }
        None
    }

    /// Parents of `p`, nearest first, stopping at init/launchd.
    fn ancestors<'a>(&'a self, p: &ProcInfo) -> impl Iterator<Item = &'a ProcInfo> + 'a {
        let mut next = p.ppid;
        std::iter::from_fn(move || {
            if next <= 1 {
                return None;
            }
            let anc = self.procs.get(&next)?;
            next = if anc.ppid == anc.pid { 0 } else { anc.ppid };
            Some(anc)
        })
        .take(MAX_DEPTH)
    }
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// A cwd that could be inside a worktree. `/` (where nearly every daemon and
/// app helper lives) can't be.
fn in_interesting_cwd(cwd: Option<&Path>) -> bool {
    cwd.is_some_and(|c| c.parent().is_some())
}

fn has_git_entry(dir: &Path) -> bool {
    dir.join(".git").symlink_metadata().is_ok()
}

/// `cwd` is `root` or below it, and not inside a repository nested beneath
/// `root` (a directory strictly below `root`, up to and including `cwd`, that
/// has a `.git` entry).
fn cwd_belongs_to(cwd: Option<&Path>, root: &Path, is_repo_root: impl Fn(&Path) -> bool) -> bool {
    let Some(cwd) = cwd else { return false };
    if !cwd.starts_with(root) {
        return false;
    }
    !cwd.ancestors().take_while(|d| *d != root).any(is_repo_root)
}

/// Index of the deepest root that contains `cwd` (component-wise, so `/wt/ab`
/// is not inside `/wt/a`).
fn most_specific_root(cwd: &Path, roots: &[PathBuf]) -> Option<usize> {
    roots
        .iter()
        .enumerate()
        .filter(|(_, r)| cwd.starts_with(r))
        .max_by_key(|(_, r)| r.components().count())
        .map(|(i, _)| i)
}

fn basename(s: &str) -> &str {
    let s = s.trim_end_matches('/');
    s.rsplit('/').next().unwrap_or(s)
}

/// Basename of argv[0] without a login-shell `-`, else the process name.
fn argv0_name(p: &ProcInfo) -> &str {
    p.argv
        .first()
        .map(|a| basename(a).trim_start_matches('-'))
        .filter(|a| !a.is_empty())
        .unwrap_or(&p.name)
}

const SHELLS: &[&str] = &["zsh", "bash", "fish", "nu", "sh", "dash"];
/// Excluded from running processes alongside the shells, as in the TS.
const SHELL_LIKE: &[&str] = &["login", "tmux"];

fn shell_name(p: &ProcInfo) -> Option<&str> {
    let name = p.name.trim_start_matches('-');
    [name, argv0_name(p)].into_iter().find(|n| SHELLS.contains(n))
}

/// A shell Claude Code started for its Bash tool.
fn is_claude_shell_command(p: &ProcInfo) -> bool {
    is_shell(p) && p.argv.iter().skip(1).any(|a| a.contains("/.claude/shell-snapshots/"))
}

fn is_shell(p: &ProcInfo) -> bool {
    shell_name(p).is_some() || SHELL_LIKE.contains(&p.name.trim_start_matches('-')) || SHELL_LIKE.contains(&argv0_name(p))
}

/// A shell a human types into: a login shell (`-zsh`), or one with neither a
/// command (`-c`, `-lc`, `--command`) nor a script argument. Empty argv
/// (unreadable) counts as interactive.
fn is_interactive_shell(p: &ProcInfo) -> bool {
    if shell_name(p).is_none() {
        return false;
    }
    let Some((argv0, rest)) = p.argv.split_first() else { return true };
    if argv0.starts_with('-') {
        return true;
    }
    for arg in rest {
        match arg.as_str() {
            "--" => break,
            "--command" | "--commands" => return false,
            a if a.starts_with("--") => {}
            // Option clusters: `-il` is interactive, `-c` / `-lc` run a command.
            a if a.starts_with('-') => {
                if a[1..].contains('c') {
                    return false;
                }
            }
            a if a.starts_with('+') => {} // zsh `+o option`
            _ => return false, // a script path
        }
    }
    true
}

/// Which editor an executable path belongs to. macOS app bundles (including
/// all their helper processes), Linux binaries, and remote servers.
fn ide_of_path(path: &str) -> Option<Ide> {
    if path.contains("/Cursor.app/") || path.contains("/.cursor-server/") {
        return Some(Ide::Cursor);
    }
    const VSCODE_BUNDLES: &[&str] = &[
        "/Visual Studio Code.app/",
        "/Visual Studio Code - Insiders.app/",
        "/VSCodium.app/",
        "/Code - OSS.app/",
        "/.vscode-server/",
        "/.vscode-server-insiders/",
    ];
    if VSCODE_BUNDLES.iter().any(|b| path.contains(b)) {
        return Some(Ide::Vscode);
    }
    // Linux installs: /usr/share/code/code, /opt/cursor/cursor, AppImages…
    match basename(path) {
        "cursor" | "Cursor" => Some(Ide::Cursor),
        "code" | "code-insiders" | "code-oss" | "codium" | "vscodium" if path.starts_with('/') => Some(Ide::Vscode),
        _ => None,
    }
}

fn ide_of(p: &ProcInfo) -> Option<Ide> {
    match &p.exe {
        Some(exe) => ide_of_path(&exe.to_string_lossy()),
        None => p.argv.first().and_then(|a| ide_of_path(a)),
    }
}

/// The interpreter prefix `friendly_command_label` strips.
fn is_interpreter(token: &str) -> bool {
    matches!(basename(token), "node" | "nodejs" | "deno" | "bun" | "tsx" | "ts-node")
}

/// argv with a leading node-family interpreter dropped when it is running a
/// script (`node …/vite` → `…/vite`), but not when the next token is a flag
/// (`python3 -m http.server` stays whole).
fn program_argv(argv: &[String]) -> &[String] {
    match argv {
        [first, second, ..] if is_interpreter(first) && !second.starts_with('-') => &argv[1..],
        _ => argv,
    }
}

/// The registered agent CLI (`crate::harness`) `p` is, if any, by id.
fn agent_cli(p: &ProcInfo) -> Option<&'static str> {
    crate::harness::all().iter().find(|k| k.provider().matches_process(&p.name, &p.argv)).map(|k| k.id())
}

fn is_editor(p: &ProcInfo) -> bool {
    let program = program_argv(&p.argv).first().map(String::as_str).unwrap_or("");
    ide_of(p).is_some() || matches!(basename(program), "code" | "cursor")
}

/// am itself: the native binaries (`am`, `am-hook`, `am-tui`, …), and the TS
/// app (`…/agent-monitor/…`, `cli.tsx`, `daemon.js`) while both coexist.
fn is_agent_monitor(p: &ProcInfo) -> bool {
    if p.name == "am" || p.name.starts_with("am-") {
        return true;
    }
    let program = program_argv(&p.argv).first().map(String::as_str).unwrap_or("");
    if program.contains("agent-monitor") {
        return true;
    }
    let base = basename(program);
    let (stem, ext) = base.rsplit_once('.').unwrap_or((base, ""));
    matches!(stem, "cli" | "daemon") && matches!(ext, "ts" | "tsx" | "js" | "jsx")
}

/// argv joined with spaces, or the process name when argv is unknown.
fn command_line(p: &ProcInfo) -> String {
    if p.argv.is_empty() { p.name.clone() } else { p.argv.join(" ") }
}

/// Short, readable label for a command line:
///   `node /repo/node_modules/.bin/vite`          → `vite`
///   `/opt/homebrew/bin/node /repo/.../pnpm dev`  → `pnpm dev`
///   `npm run dev`                                → `npm run dev`
///
/// Works on real argv (not a whitespace split), so executables under paths
/// with spaces still basename correctly. Labels over 40 characters are cut to
/// 39 plus `…`.
pub fn friendly_command_label(argv: &[String], fallback: &str) -> String {
    let argv: Vec<String> = argv.iter().map(|a| a.trim()).filter(|a| !a.is_empty()).map(String::from).collect();
    let rest = program_argv(&argv);
    let Some((first, tail)) = rest.split_first() else { return fallback.to_string() };
    let mut label = basename(first).to_string();
    for t in tail {
        label.push(' ');
        label.push_str(t);
    }
    if label.chars().count() > LABEL_MAX {
        label = label.chars().take(LABEL_MAX - 1).collect();
        label.push('…');
    }
    label
}

// ---------------------------------------------------------------------------
// OS backends
// ---------------------------------------------------------------------------

/// The shared capture plan over an OS-specific reader.
trait Reader {
    fn pids(&mut self) -> Vec<i32>;
    /// `(ppid, kernel command name)`.
    fn basic(&mut self, pid: i32) -> Option<(i32, String)>;
    fn cwd(&mut self, pid: i32) -> Option<PathBuf>;
    fn exe(&mut self, pid: i32) -> Option<PathBuf>;
    fn argv(&mut self, pid: i32) -> Vec<String>;
}

fn scan_with(r: &mut impl Reader) -> Vec<ProcInfo> {
    let mut procs: BTreeMap<i32, ProcInfo> = BTreeMap::new();
    for pid in r.pids() {
        if pid <= 0 {
            continue;
        }
        let Some((ppid, name)) = r.basic(pid) else { continue };
        let cwd = r.cwd(pid);
        procs.insert(pid, ProcInfo { pid, ppid, name, exe: None, cwd, argv: Vec::new() });
    }

    // Processes that might belong to a worktree get argv; they and every
    // ancestor (to classify shells/editors/claude up the chain) get exe.
    let interesting: Vec<i32> = procs.values().filter(|p| in_interesting_cwd(p.cwd.as_deref())).map(|p| p.pid).collect();
    let mut need_exe: Vec<i32> = Vec::new();
    for &pid in &interesting {
        let mut cur = pid;
        for _ in 0..MAX_DEPTH {
            let Some(p) = procs.get(&cur) else { break };
            need_exe.push(cur);
            if p.ppid <= 1 || p.ppid == cur {
                break;
            }
            cur = p.ppid;
        }
    }
    need_exe.sort_unstable();
    need_exe.dedup();
    for pid in need_exe {
        let exe = r.exe(pid);
        if let Some(p) = procs.get_mut(&pid) {
            if let Some(n) = exe.as_deref().and_then(Path::file_name) {
                p.name = n.to_string_lossy().into_owned();
            }
            p.exe = exe;
        }
    }
    for pid in interesting {
        let argv = r.argv(pid);
        if let Some(p) = procs.get_mut(&pid) {
            p.argv = argv;
        }
    }
    procs.into_values().collect()
}

#[cfg(target_os = "macos")]
mod os {
    use super::{ProcInfo, Reader, scan_with};
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;

    struct Darwin {
        argbuf: Vec<u8>,
    }

    pub(super) fn scan() -> Vec<ProcInfo> {
        scan_with(&mut Darwin { argbuf: vec![0; arg_max()] })
    }

    fn arg_max() -> usize {
        let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
        let mut val: libc::c_int = 0;
        let mut len = std::mem::size_of::<libc::c_int>();
        // SAFETY: mib/val/len are valid for the duration of the call and len
        // matches val's size.
        let rc = unsafe { libc::sysctl(mib.as_mut_ptr(), 2, (&raw mut val).cast(), &mut len, std::ptr::null_mut(), 0) };
        if rc == 0 && val > 0 { val as usize } else { 1024 * 1024 }
    }

    fn pidinfo<T>(pid: i32, flavor: libc::c_int) -> Option<T> {
        let size = std::mem::size_of::<T>() as libc::c_int;
        // SAFETY: T is a plain C struct from libc (all-zero is valid); the
        // kernel writes at most `size` bytes into it.
        unsafe {
            let mut v: T = std::mem::zeroed();
            let n = libc::proc_pidinfo(pid, flavor, 0, (&raw mut v).cast(), size);
            (n == size).then_some(v)
        }
    }

    fn c_chars(buf: &[libc::c_char]) -> String {
        // SAFETY: reinterpreting c_char (i8/u8) as u8 of the same length.
        let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), buf.len()) };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    }

    impl Reader for Darwin {
        fn pids(&mut self) -> Vec<i32> {
            // SAFETY: a null buffer asks for the count; the second call writes
            // at most `buf.len()` pids.
            unsafe {
                let n = libc::proc_listallpids(std::ptr::null_mut(), 0);
                if n <= 0 {
                    return Vec::new();
                }
                let mut buf = vec![0i32; n as usize + 64];
                let bytes = (buf.len() * std::mem::size_of::<i32>()) as libc::c_int;
                let n = libc::proc_listallpids(buf.as_mut_ptr().cast(), bytes);
                buf.truncate(n.max(0) as usize);
                buf
            }
        }

        fn basic(&mut self, pid: i32) -> Option<(i32, String)> {
            let bi: libc::proc_bsdinfo = pidinfo(pid, libc::PROC_PIDTBSDINFO)?;
            let name = c_chars(&bi.pbi_name);
            let name = if name.is_empty() { c_chars(&bi.pbi_comm) } else { name };
            Some((bi.pbi_ppid as i32, name))
        }

        fn cwd(&mut self, pid: i32) -> Option<PathBuf> {
            let vi: libc::proc_vnodepathinfo = pidinfo(pid, libc::PROC_PIDVNODEPATHINFO)?;
            // SAFETY: vip_path is a NUL-terminated MAXPATHLEN buffer.
            let path = unsafe { CStr::from_ptr(vi.pvi_cdir.vip_path.as_ptr().cast()) };
            let bytes = path.to_bytes();
            (!bytes.is_empty()).then(|| PathBuf::from(OsStr::from_bytes(bytes)))
        }

        fn exe(&mut self, pid: i32) -> Option<PathBuf> {
            let mut buf = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
            // SAFETY: the kernel writes at most buf.len() bytes.
            let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
            (n > 0).then(|| PathBuf::from(OsStr::from_bytes(&buf[..n as usize])))
        }

        fn argv(&mut self, pid: i32) -> Vec<String> {
            let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
            let mut len = self.argbuf.len();
            // SAFETY: argbuf is valid for `len` bytes; the kernel updates len.
            let rc = unsafe {
                libc::sysctl(mib.as_mut_ptr(), 3, self.argbuf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0)
            };
            if rc != 0 || len < 4 {
                return Vec::new();
            }
            parse_procargs2(&self.argbuf[..len])
        }
    }

    /// `KERN_PROCARGS2`: argc (i32), exec path, NUL padding, argv[0..argc]
    /// NUL-separated, then the environment (ignored).
    pub(super) fn parse_procargs2(buf: &[u8]) -> Vec<String> {
        let Some(argc) = buf.get(..4).map(|b| i32::from_ne_bytes([b[0], b[1], b[2], b[3]])) else {
            return Vec::new();
        };
        let mut rest = &buf[4..];
        let exec_end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        rest = &rest[exec_end..];
        let args_start = rest.iter().position(|&b| b != 0).unwrap_or(rest.len());
        rest = &rest[args_start..];
        rest.split(|&b| b == 0)
            .take(argc.max(0) as usize)
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect()
    }
}

#[cfg(target_os = "linux")]
mod os {
    use super::{ProcInfo, Reader, scan_with};
    use std::path::PathBuf;

    struct Procfs;

    pub(super) fn scan() -> Vec<ProcInfo> {
        scan_with(&mut Procfs)
    }

    impl Reader for Procfs {
        fn pids(&mut self) -> Vec<i32> {
            let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
            dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok()).collect()
        }

        fn basic(&mut self, pid: i32) -> Option<(i32, String)> {
            parse_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
        }

        fn cwd(&mut self, pid: i32) -> Option<PathBuf> {
            std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
        }

        fn exe(&mut self, pid: i32) -> Option<PathBuf> {
            std::fs::read_link(format!("/proc/{pid}/exe")).ok()
        }

        fn argv(&mut self, pid: i32) -> Vec<String> {
            std::fs::read(format!("/proc/{pid}/cmdline")).map(|b| parse_cmdline(&b)).unwrap_or_default()
        }
    }

    /// `/proc/<pid>/stat`: `pid (comm) state ppid …`; comm may itself contain
    /// spaces and parentheses, so split on the last `)`.
    pub(super) fn parse_stat(stat: &str) -> Option<(i32, String)> {
        let open = stat.find('(')?;
        let close = stat.rfind(')')?;
        let comm = stat.get(open + 1..close)?.to_string();
        let mut fields = stat.get(close + 1..)?.split_whitespace();
        let _state = fields.next()?;
        let ppid = fields.next()?.parse().ok()?;
        Some((ppid, comm))
    }

    pub(super) fn parse_cmdline(buf: &[u8]) -> Vec<String> {
        let buf = buf.strip_suffix(&[0]).unwrap_or(buf);
        if buf.is_empty() {
            return Vec::new();
        }
        buf.split(|&b| b == 0).map(|a| String::from_utf8_lossy(a).into_owned()).collect()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod os {
    use super::ProcInfo;

    pub(super) fn scan() -> Vec<ProcInfo> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    fn proc(pid: i32, ppid: i32, exe: &str, cwd: &str, args: &[&str]) -> ProcInfo {
        ProcInfo {
            pid,
            ppid,
            name: basename(exe).to_string(),
            exe: Some(PathBuf::from(exe)),
            cwd: Some(PathBuf::from(cwd)),
            argv: args.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn pids(list: Option<&Vec<RunningProcess>>) -> Vec<i32> {
        list.map(|l| l.iter().map(|p| p.pid).collect()).unwrap_or_default()
    }

    // --- friendly_command_label (ported) -----------------------------------

    #[test]
    fn label_strips_node_interpreter_and_basenames_tool() {
        assert_eq!(friendly_command_label(&argv("node /repo/node_modules/.bin/vite"), "node"), "vite");
    }

    #[test]
    fn label_keeps_subcommand_after_tool() {
        assert_eq!(friendly_command_label(&argv("/opt/homebrew/bin/node /repo/.bin/pnpm dev"), "node"), "pnpm dev");
    }

    #[test]
    fn label_leaves_non_interpreter_command_intact() {
        assert_eq!(friendly_command_label(&argv("npm run dev"), "npm"), "npm run dev");
    }

    #[test]
    fn label_keeps_interpreter_when_followed_by_flag() {
        assert_eq!(friendly_command_label(&argv("python3 -m http.server"), "python3"), "python3 -m http.server");
        assert_eq!(friendly_command_label(&argv("node --watch server.js"), "node"), "node --watch server.js");
    }

    #[test]
    fn label_falls_back_to_name_when_argv_empty() {
        assert_eq!(friendly_command_label(&[], "node"), "node");
        assert_eq!(friendly_command_label(&["  ".into()], "node"), "node");
    }

    #[test]
    fn label_truncates_long_commands() {
        let label = friendly_command_label(&argv(&format!("someverylongtoolname {}", "x".repeat(80))), "x");
        assert_eq!(label.chars().count(), 40);
        assert!(label.ends_with('…'));
    }

    #[test]
    fn label_handles_executable_paths_with_spaces() {
        let args = vec!["/Applications/My Tools.app/Contents/MacOS/my server".to_string(), "--port".into(), "3000".into()];
        assert_eq!(friendly_command_label(&args, "x"), "my server --port 3000");
    }

    // --- running_processes (ports parseWorktreeProcesses + processesForWorktree)

    /// The TS `parseWorktreeProcesses` fixture, as a process table.
    fn ts_fixture() -> ProcessSnapshot {
        ProcessSnapshot::from_processes(
            [
                proc(1000, 1, "/usr/local/bin/node", "/wt/a", &["node", "/wt/a/node_modules/.bin/vite"]),
                proc(1001, 1, "/bin/zsh", "/wt/a", &["-zsh"]),
                proc(1002, 1, "/usr/local/bin/node", "/wt/b", &["node", "/Users/dev/.claude/local/claude", "--resume", "abc"]),
                proc(1003, 1, "/usr/local/bin/node", "/other", &["node", "/other/server.js"]),
                proc(1004, 1, "/usr/bin/python3", "/wt/a", &["python3", "-m", "http.server"]),
            ],
            99_999,
        )
    }

    fn roots(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn keeps_user_processes_and_derives_labels() {
        let map = ts_fixture().running_processes(&roots(&["/wt/a", "/wt/b"]), "");
        let a = &map[Path::new("/wt/a")];
        let mut labels: Vec<_> = a.iter().map(|p| p.command.as_str()).collect();
        labels.sort();
        assert_eq!(labels, ["python3 -m http.server", "vite"]);
        assert_eq!(a.iter().find(|p| p.command == "vite").unwrap().pid, 1000);
    }

    #[test]
    fn excludes_shells_and_claude() {
        let map = ts_fixture().running_processes(&roots(&["/wt/a", "/wt/b"]), "");
        assert!(!pids(map.get(Path::new("/wt/a"))).contains(&1001));
        assert!(!map.contains_key(Path::new("/wt/b")));
    }

    #[test]
    fn excludes_gemini_and_its_tooling() {
        let snap = ProcessSnapshot::from_processes(
            [
                proc(2000, 1, "/usr/local/bin/node", "/wt/g", &["node", "/Users/dev/.nvm/bin/gemini"]),
                proc(2001, 2000, "/usr/local/bin/node", "/wt/g", &["node", "--max-old-space-size=8192", "/n/lib/node_modules/@google/gemini-cli/bundle/gemini.js"]),
                proc(2002, 2001, "/usr/local/bin/node", "/wt/g", &["node", "/wt/g/mcp-server.js"]),
                proc(2003, 1, "/usr/local/bin/node", "/wt/g", &["node", "/wt/g/node_modules/.bin/vite"]),
            ],
            99_999,
        );
        let map = snap.running_processes(&roots(&["/wt/g"]), "");
        assert_eq!(pids(map.get(Path::new("/wt/g"))), [2003]);
    }

    #[test]
    fn processes_outside_every_root_are_ignored() {
        let map = ts_fixture().running_processes(&roots(&["/wt/a"]), "");
        assert_eq!(map.len(), 1);
        assert!(!map.values().flatten().any(|p| p.pid == 1003));
    }

    #[test]
    fn counts_claude_shell_commands_per_worktree() {
        let wrapper = "source /Users/me/.claude/shell-snapshots/snapshot-zsh-1-abc.sh 2>/dev/null || true && { pnpm test:e2e; }";
        let snap = ProcessSnapshot::from_processes(
            [
                proc(1, 0, "/bin/zsh", "/wt/a", &["/bin/zsh", "-c", wrapper]),
                proc(2, 1, "/usr/bin/node", "/wt/a", &["node", "pnpm", "test:e2e"]),
                proc(3, 0, "/bin/zsh", "/wt/a/sub", &["/bin/zsh", "-c", wrapper]),
                // An interactive shell and a plain `zsh -c` aren't Claude's.
                proc(4, 0, "/bin/zsh", "/wt/a", &["-zsh"]),
                proc(5, 0, "/bin/zsh", "/wt/a", &["/bin/zsh", "-c", "make watch"]),
                // A non-shell mentioning the path doesn't count either.
                proc(6, 0, "/usr/bin/cat", "/wt/a", &["cat", "/Users/me/.claude/shell-snapshots/x.sh"]),
                proc(7, 0, "/bin/bash", "/wt/b", &["bash", "-c", wrapper]),
            ],
            999,
        );
        assert_eq!(snap.claude_shell_commands(Path::new("/wt/a")), 2);
        assert_eq!(snap.claude_shell_commands(Path::new("/wt/b")), 1);
        assert_eq!(snap.claude_shell_commands(Path::new("/wt/c")), 0);
    }

    #[test]
    fn excludes_self_and_parent() {
        let mut procs: Vec<ProcInfo> = ts_fixture().processes().cloned().collect();
        procs.push(proc(500, 1000, "/usr/local/bin/am", "/wt/a", &["something-else"]));
        // Scanner pid 500 has parent 1000 (vite): both are hidden.
        let snap = ProcessSnapshot::from_processes(procs, 500);
        assert_eq!(pids(snap.running_processes(&roots(&["/wt/a"]), "").get(Path::new("/wt/a"))), [1004]);
    }

    #[test]
    fn filter_is_case_insensitive_on_full_command() {
        let snap = ts_fixture();
        let r = roots(&["/wt/a"]);
        assert_eq!(pids(snap.running_processes(&r, "VITE").get(Path::new("/wt/a"))), [1000]);
        // "http.server" is in pid 1004's argv, which is also its label; ".bin" is only in vite's argv.
        assert_eq!(pids(snap.running_processes(&r, "http.server").get(Path::new("/wt/a"))), [1004]);
        assert_eq!(pids(snap.running_processes(&r, "node_modules/.bin").get(Path::new("/wt/a"))), [1000]);
        assert!(snap.running_processes(&r, "nomatch").is_empty());
        assert_eq!(pids(snap.running_processes(&r, "   ").get(Path::new("/wt/a"))), [1000, 1004]);
    }

    #[test]
    fn counts_agent_sessions_per_root_and_harness() {
        let snap = ProcessSnapshot::from_processes(
            [
                proc(1, 0, "/usr/local/bin/claude", "/wt/main", &["claude"]),
                proc(2, 0, "/usr/local/bin/claude", "/wt/main/crates", &["claude", "--resume", "x"]),
                proc(3, 0, "/usr/local/bin/claude", "/wt/main/.claude/worktrees/feat", &["claude"]),
                // Gemini relaunches itself: parent + child are one session.
                proc(4, 0, "/usr/local/bin/node", "/wt/main", &["node", "/opt/gemini/bin/gemini"]),
                proc(5, 4, "/usr/local/bin/node", "/wt/main", &["node", "/opt/gemini/bin/gemini"]),
                proc(6, 0, "/usr/bin/vite", "/wt/main", &["vite"]),
            ],
            99_999,
        );
        let map = snap.agent_sessions(&roots(&["/wt/main", "/wt/main/.claude/worktrees/feat"]));
        let main = &map[Path::new("/wt/main")];
        assert_eq!((main.get("claude"), main.get("gemini")), (Some(&2), Some(&1)));
        assert_eq!(map[Path::new("/wt/main/.claude/worktrees/feat")].get("claude"), Some(&1));
    }

    #[test]
    fn attributes_to_most_specific_root() {
        let snap = ProcessSnapshot::from_processes(
            [
                proc(1, 0, "/usr/bin/vite", "/wt/main", &["vite"]),
                proc(2, 0, "/usr/bin/pnpm", "/wt/main/apps/api", &["pnpm", "dev"]),
                proc(3, 0, "/usr/bin/next", "/wt/main/.claude/worktrees/feat", &["next"]),
                proc(4, 0, "/usr/bin/webpack", "/wt/main/.claude/worktrees/feat/apps/web", &["webpack"]),
                proc(5, 0, "/usr/bin/server", "/unrelated", &["server"]),
                proc(6, 0, "/usr/bin/server", "/wt/mainline", &["server"]),
            ],
            99_999,
        );
        let map = snap.running_processes(&roots(&["/wt/main", "/wt/main/.claude/worktrees/feat"]), "");
        assert_eq!(pids(map.get(Path::new("/wt/main"))), [1, 2]);
        assert_eq!(pids(map.get(Path::new("/wt/main/.claude/worktrees/feat"))), [3, 4]);
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn excludes_editors_am_and_tooling_spawned_without_a_shell() {
        let vscode = "/Applications/Visual Studio Code.app/Contents/MacOS/Code";
        let plugin = "/Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin)";
        let snap = ProcessSnapshot::from_processes(
            [
                proc(10, 1, vscode, "/", &[vscode]),
                proc(11, 10, plugin, "/wt", &[plugin, "--type=extensionHost"]),
                proc(12, 11, "/home/u/.vscode/extensions/rust-analyzer", "/wt", &["rust-analyzer"]),
                proc(20, 1, "/usr/local/bin/claude", "/wt", &["claude"]),
                proc(21, 20, "/usr/local/bin/npm", "/wt", &["npm", "exec", "@playwright/mcp"]),
                proc(22, 21, "/usr/local/bin/node", "/wt", &["node", "/x/.bin/playwright-mcp"]),
                // claude → zsh -c → pnpm dev → node vite: a real dev server.
                proc(23, 20, "/bin/zsh", "/wt", &["/bin/zsh", "-c", "pnpm dev"]),
                proc(24, 23, "/usr/local/bin/pnpm", "/wt", &["pnpm", "dev"]),
                proc(25, 24, "/usr/local/bin/node", "/wt", &["node", "/wt/node_modules/.bin/vite"]),
                proc(30, 1, "/usr/local/bin/node", "/wt", &["node", "/opt/agent-monitor/dist/cli.js"]),
                proc(31, 1, "/usr/local/bin/am", "/wt", &["am"]),
                proc(32, 1, "/usr/local/bin/node", "/wt", &["node", "/Users/x/.claude/worktrees/y/node_modules/.bin/vite"]),
                // am's own git polls (TS app and native TUI).
                proc(33, 30, "/usr/bin/git", "/wt", &["git", "status", "--porcelain"]),
                proc(34, 1, "/opt/am/am-tui", "/", &["am-tui"]),
                proc(35, 34, "/usr/bin/git", "/wt", &["git", "fetch"]),
            ],
            99_999,
        );
        assert_eq!(pids(snap.running_processes(&roots(&["/wt"]), "").get(Path::new("/wt"))), [24, 25, 32]);
    }

    // --- has_terminal --------------------------------------------------------

    #[test]
    fn interactive_shell_rules() {
        let sh = |args: &[&str]| proc(1, 0, "/bin/zsh", "/wt", args);
        assert!(is_interactive_shell(&sh(&["-zsh"])));
        assert!(is_interactive_shell(&sh(&["/bin/zsh", "-il"])));
        assert!(is_interactive_shell(&sh(&["zsh"])));
        assert!(is_interactive_shell(&sh(&[])));
        assert!(!is_interactive_shell(&sh(&["/bin/zsh", "-c", "source x; pnpm dev"])));
        assert!(!is_interactive_shell(&sh(&["bash", "-lc", "make"])));
        assert!(!is_interactive_shell(&sh(&["bash", "./script.sh"])));
        assert!(!is_interactive_shell(&sh(&["fish", "--command", "ls"])));
        assert!(!is_interactive_shell(&proc(1, 0, "/usr/bin/node", "/wt", &["node"])));
        // Kernel name differs from argv0 (e.g. nu, fish via a wrapper).
        assert!(is_interactive_shell(&proc(1, 0, "/opt/homebrew/bin/fish", "/wt", &["fish"])));
    }

    #[test]
    fn has_terminal_matches_root_and_subdirs_only() {
        let snap = ProcessSnapshot::from_processes(
            [
                proc(1, 0, "/bin/zsh", "/wt/a/src", &["-zsh"]),
                proc(2, 0, "/bin/zsh", "/wt/b", &["/bin/zsh", "-c", "sleep 100"]),
                proc(3, 0, "/bin/bash", "/wt/abc", &["bash"]),
            ],
            99_999,
        );
        assert!(snap.has_terminal(Path::new("/wt/a")));
        assert!(!snap.has_terminal(Path::new("/wt/b")), "zsh -c is not a terminal");
        assert!(snap.has_terminal(Path::new("/wt/abc")));
        assert!(!snap.has_terminal(Path::new("/wt/ab")), "component-wise prefix");
    }

    #[test]
    fn single_root_queries_skip_nested_repositories() {
        let tmp = tempfile::tempdir().unwrap();
        let main = std::fs::canonicalize(tmp.path()).unwrap();
        let nested = main.join(".claude/worktrees/feat");
        std::fs::create_dir_all(nested.join("src")).unwrap();
        std::fs::write(nested.join(".git"), "gitdir: elsewhere").unwrap();
        std::fs::create_dir_all(main.join("apps/api")).unwrap();

        let shell_in = |pid, dir: &Path| proc(pid, 0, "/bin/zsh", &dir.to_string_lossy(), &["-zsh"]);
        let only_nested = ProcessSnapshot::from_processes([shell_in(1, &nested.join("src"))], 99_999);
        assert!(only_nested.has_terminal(&nested));
        assert!(!only_nested.has_terminal(&main));

        let in_main_subdir = ProcessSnapshot::from_processes([shell_in(1, &main.join("apps/api"))], 99_999);
        assert!(in_main_subdir.has_terminal(&main));
        assert!(!in_main_subdir.has_terminal(&nested));
    }

    // --- open_ide ------------------------------------------------------------

    #[test]
    fn ide_of_path_recognises_editors() {
        let plugin = "/Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin)";
        assert_eq!(ide_of_path(plugin), Some(Ide::Vscode));
        assert_eq!(ide_of_path("/Applications/Cursor.app/Contents/Frameworks/Cursor Helper (Plugin).app/x"), Some(Ide::Cursor));
        assert_eq!(ide_of_path("/usr/share/code/code"), Some(Ide::Vscode));
        assert_eq!(ide_of_path("/opt/cursor/cursor"), Some(Ide::Cursor));
        assert_eq!(ide_of_path("/home/u/.vscode-server/bin/abc/node"), Some(Ide::Vscode));
        assert_eq!(ide_of_path("/home/u/.cursor-server/bin/abc/node"), Some(Ide::Cursor));
        assert_eq!(ide_of_path("/usr/bin/node"), None);
        assert_eq!(ide_of_path("/System/Library/CursorUIViewService.xpc/Contents/MacOS/CursorUIViewService"), None);
    }

    /// Mirrors what this Mac actually runs: VS Code main process at `/`, an
    /// extension host per window with cwd = workspace folder, a pty host whose
    /// children are the integrated terminal shells, and Claude in a terminal
    /// whose Bash tool runs `zsh -c` in another worktree.
    fn ide_fixture() -> ProcessSnapshot {
        let app = "/Applications/Visual Studio Code.app/Contents";
        let main = format!("{app}/MacOS/Code");
        let helper = format!("{app}/Frameworks/Code Helper.app/Contents/MacOS/Code Helper");
        let plugin = format!("{app}/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin)");
        let cursor_plugin = "/Applications/Cursor.app/Contents/Frameworks/Cursor Helper (Plugin).app/Contents/MacOS/Cursor Helper (Plugin)";
        ProcessSnapshot::from_processes(
            [
                proc(808, 1, &main, "/", &[&main]),
                proc(2511, 808, &helper, "/", &[&helper, "--type=utility"]),
                proc(10413, 808, &plugin, "/wt/vscode-window", &[&plugin]),
                proc(10565, 2511, "/bin/zsh", "/wt/vscode-window", &["/bin/zsh", "-il"]),
                proc(10600, 2511, "/bin/zsh", "/wt/terminal-only", &["/bin/zsh", "-il"]),
                proc(11058, 10565, "/usr/local/bin/claude", "/wt/vscode-window", &["claude"]),
                proc(77450, 11058, "/bin/zsh", "/wt/agent-elsewhere", &["/bin/zsh", "-c", "cargo test"]),
                proc(900, 1, "/usr/bin/login", "/", &["login", "-pf", "u"]),
                proc(901, 900, "/bin/zsh", "/wt/ghostty", &["-zsh"]),
                proc(500, 1, cursor_plugin, "/wt/cursor-window", &[cursor_plugin]),
            ],
            99_999,
        )
    }

    #[test]
    fn open_ide_from_extension_host_cwd() {
        let snap = ide_fixture();
        assert_eq!(snap.open_ide(Path::new("/wt/vscode-window")), Some(Ide::Vscode));
        assert_eq!(snap.open_ide(Path::new("/wt/cursor-window")), Some(Ide::Cursor));
    }

    #[test]
    fn open_ide_from_integrated_terminal() {
        assert_eq!(ide_fixture().open_ide(Path::new("/wt/terminal-only")), Some(Ide::Vscode));
    }

    #[test]
    fn open_ide_ignores_indirect_descendants_and_plain_terminals() {
        let snap = ide_fixture();
        assert_eq!(snap.open_ide(Path::new("/wt/agent-elsewhere")), None);
        assert_eq!(snap.open_ide(Path::new("/wt/ghostty")), None);
        assert!(snap.has_terminal(Path::new("/wt/ghostty")));
        assert!(!snap.has_terminal(Path::new("/wt/agent-elsewhere")));
    }

    // --- OS parsers ----------------------------------------------------------

    #[cfg(target_os = "macos")]
    #[test]
    fn parses_procargs2() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/usr/local/bin/node\0\0\0\0node\0/repo/my app/vite\0PATH=/bin\0");
        assert_eq!(os::parse_procargs2(&buf), ["node", "/repo/my app/vite"]);
        assert!(os::parse_procargs2(&[1, 0]).is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_proc_stat_and_cmdline() {
        assert_eq!(os::parse_stat("42 (my (weird) cmd) S 7 42 42 0"), Some((7, "my (weird) cmd".into())));
        assert_eq!(os::parse_cmdline(b"node\0/repo/my app/vite\0"), ["node", "/repo/my app/vite"]);
        assert!(os::parse_cmdline(b"").is_empty());
    }

    #[test]
    fn capture_sees_this_test_process() {
        let snap = capture();
        let me = std::process::id() as i32;
        let p = snap.processes().find(|p| p.pid == me).expect("own pid in snapshot");
        assert_eq!(p.cwd.as_deref().map(canonical), std::env::current_dir().ok().map(|d| canonical(&d)));
        assert!(!p.argv.is_empty());
        assert!(p.exe.is_some());
    }

    /// Manual check against the live machine:
    /// `cargo test -p am-core --release procscan::tests::live_snapshot_summary -- --ignored --nocapture`
    /// Optionally set `AM_PROCSCAN_ROOTS` to a `:`-separated list of worktree
    /// roots; otherwise the cwds of interactive shells and editor processes are used.
    #[test]
    #[ignore]
    fn live_snapshot_summary() {
        for _ in 0..3 {
            capture();
        }
        let n = 20;
        let t = std::time::Instant::now();
        for _ in 0..n {
            capture();
        }
        let per = t.elapsed() / n;
        let snap = capture();
        let all = snap.processes().count();
        let with_cwd = snap.processes().filter(|p| p.cwd.is_some()).count();
        let interesting = snap.processes().filter(|p| in_interesting_cwd(p.cwd.as_deref())).count();
        println!("capture: {per:?}/scan; {all} processes, {with_cwd} readable cwd, {interesting} outside /");

        let roots: Vec<PathBuf> = match std::env::var("AM_PROCSCAN_ROOTS") {
            Ok(v) => v.split(':').filter(|s| !s.is_empty()).map(PathBuf::from).collect(),
            Err(_) => {
                let mut r: Vec<PathBuf> = snap
                    .processes()
                    .filter(|p| is_interactive_shell(p) || ide_of(p).is_some())
                    .filter_map(|p| p.cwd.clone())
                    .filter(|c| in_interesting_cwd(Some(c)))
                    .collect();
                r.sort();
                r.dedup();
                r
            }
        };
        let procs = snap.running_processes(&roots, "");
        for root in &roots {
            let running: Vec<String> =
                procs.get(root).map(|l| l.iter().map(|p| format!("{} {}", p.pid, p.command)).collect()).unwrap_or_default();
            println!(
                "{}\n    terminal={} ide={:?} running=[{}]",
                root.display(),
                snap.has_terminal(root),
                snap.open_ide(root),
                running.join(", ")
            );
        }
    }
}
