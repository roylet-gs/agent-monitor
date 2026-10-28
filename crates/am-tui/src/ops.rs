//! Background worktree jobs (create / delete / startup script / live branch
//! check). Each runs on its own thread with its own DB connection and
//! reports back via Msg; create, delete and the script carry their jobs-pane
//! id (jobs.rs).

use std::path::Path;
use std::sync::mpsc::Sender;

use am_core::model::{Repository, WorktreeView};
use am_core::settings::Settings;
use am_core::worktrees::{self, CreateRequest, DeleteOptions, ScriptKind};
use am_core::{db, info, launch};

use crate::launcher::Msg;

pub fn check_branch(repo_path: String, branch: String, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let r = worktrees::check_branch(Path::new(&repo_path), &branch);
        am_core::debug!("tui", "Live branch check {branch}: local={} remote={:?}", r.local, r.remote);
        let _ = tx.send(Msg::BranchChecked(branch, r));
    });
}

pub fn create(settings: Settings, repo: Repository, req: CreateRequest, job: u64, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let mut progress = |text: &str| {
            let _ = tx.send(Msg::JobStep(job, text.to_string()));
        };
        let result = db::open()
            .map_err(|e| worktrees::CreateError::Db(e.to_string()))
            .and_then(|conn| worktrees::create_worktree_progress(&conn, &settings, &repo, &req, &mut progress));
        if let Ok(w) = &result {
            info!("tui", "Created worktree {} at {}", w.branch, w.path);
        }
        let _ = tx.send(Msg::Created(job, result));
    });
}

pub fn delete(view: WorktreeView, delete_branch: bool, job: u64, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let what = if delete_branch { "Removing worktree and branch…" } else { "Removing worktree…" };
        let _ = tx.send(Msg::JobStep(job, what.into()));
        // The user confirmed after seeing the dirty/unpushed warnings, so
        // remove like the TS dashboard does (force; `git branch -D`).
        let opts = DeleteOptions { delete_branch, delete_remote: false, force: true };
        let result = db::open().map_err(|e| e.to_string()).and_then(|conn| worktrees::delete_worktree(&conn, &view.worktree, &opts));
        let label = view.worktree.display_name().to_string();
        if let Err(e) = &result {
            am_core::warn!("tui", "Delete of {} failed: {e}", view.worktree.branch);
        }
        let _ = tx.send(Msg::Deleted(job, view.worktree.id.clone(), label, result.map(|r| r.warnings)));
    });
}

/// Run a repo's startup script for a new worktree, or its review script for
/// a pulled PR (with `env` describing the PR). With `in_window` (always for
/// a review script; `startupScriptMode: window` for the startup script) it
/// opens in a new terminal window, the env set on its command line; otherwise
/// (and when no window can be opened) it runs here with its output streamed
/// to the job.
pub fn run_script(kind: ScriptKind, repo_id: String, cwd: String, in_window: bool, env: Vec<(String, String)>, job: u64, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let script = kind.path(&repo_id);
        if in_window {
            let vars: String = env.iter().map(|(k, v)| format!("{k}={} ", launch::sh_quote(v))).collect();
            let cmd = format!("{vars}/bin/sh {}", launch::sh_quote(&script.to_string_lossy()));
            let _ = tx.send(Msg::JobStep(job, format!("Opening a terminal for the {}…", kind.label())));
            match launch::open_terminal(Path::new(&cwd), Some(&cmd)) {
                Ok(place) => {
                    let _ = tx.send(Msg::JobStep(job, format!("Running in {place}")));
                    if kind == ScriptKind::Review {
                        let _ = tx.send(Msg::Popup(format!("✓ The review script{} is running in {place}", pr_label(&env))));
                    }
                    let _ = tx.send(Msg::ScriptDone(job, Ok(())));
                    return;
                }
                Err(e) => {
                    am_core::debug!("tui", "No terminal window for the {} ({e}); running it in the jobs pane", kind.label());
                    if kind == ScriptKind::Review {
                        let _ = tx.send(Msg::Popup(format!(
                            "Couldn't open a terminal window, so the review script{} runs in the jobs pane (⇥)\n\n{e}",
                            pr_label(&env)
                        )));
                    }
                }
            }
        }
        let _ = tx.send(Msg::JobStep(job, format!("Running {}", script.display())));
        let result = run_script_piped(&script, &cwd, &env, job, &tx);
        match &result {
            Ok(()) => info!("tui", "{kind:?} script for {repo_id} finished in {cwd}"),
            Err(e) => am_core::warn!("tui", "{kind:?} script for {repo_id} in {cwd}: {e}"),
        }
        let _ = tx.send(Msg::ScriptDone(job, result));
    });
}

/// ` for #61`, from the review script's environment.
fn pr_label(env: &[(String, String)]) -> String {
    env.iter().find(|(k, _)| k == "AM_PR_NUMBER").map(|(_, n)| format!(" for #{n}")).unwrap_or_default()
}

fn run_script_piped(script: &Path, cwd: &str, env: &[(String, String)], job: u64, tx: &Sender<Msg>) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Read};
    use std::process::{Command, Stdio};
    let mut child = Command::new("/bin/sh")
        .arg(script)
        .current_dir(cwd)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't run script: {e}"))?;
    let pump = |r: Box<dyn Read + Send>| {
        let tx = tx.clone();
        let cwd = cwd.to_string();
        std::thread::spawn(move || {
            for line in BufReader::new(r).lines().map_while(Result::ok) {
                let line = crate::jobs::strip_ansi(&line);
                // An agent's JSON event stream (`claude -p --output-format
                // stream-json`) reads as text and tool calls.
                match am_core::harness::readable_output(&line, Some(&cwd)) {
                    Some(lines) => lines.into_iter().for_each(|l| {
                        let _ = tx.send(Msg::JobLog(job, l));
                    }),
                    None => {
                        let _ = tx.send(Msg::JobLog(job, line));
                    }
                }
            }
        })
    };
    let readers = [
        child.stdout.take().map(|o| pump(Box::new(o))),
        child.stderr.take().map(|e| pump(Box::new(e))),
    ];
    let status = child.wait().map_err(|e| format!("Script didn't finish: {e}"))?;
    for r in readers.into_iter().flatten() {
        let _ = r.join();
    }
    if status.success() { Ok(()) } else { Err(format!("Script exited with {status}")) }
}

pub fn add_repo(path: String, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let expanded = match path.strip_prefix('~') {
            Some(rest) => format!("{}{rest}", am_core::paths::home_dir().display()),
            None => path.clone(),
        };
        let result = db::open()
            .map_err(|e| e.to_string())
            .and_then(|conn| worktrees::add_repository(&conn, Path::new(expanded.trim_end_matches('/'))).map_err(|e| e.to_string()));
        let _ = tx.send(Msg::RepoAdded(path, result.map(|r| r.name)));
    });
}

/// `gh issue create` on the app's repo with an environment footer
/// (port of `createFeatureRequest`).
pub fn suggest(title: String, body: String, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let full = format!(
            "{body}\n\n---\n_Submitted from agent-monitor_\n- am version: {} (native)\n- platform: {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS
        );
        let out = std::process::Command::new("gh")
            .args(["issue", "create", "--repo", am_core::update_check::OWN_REPO, "--title", &title, "--body", &full])
            .stdin(std::process::Stdio::null())
            .output();
        let result = match out {
            Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("gh failed").to_string()),
            Err(e) => Err(format!("Couldn't run gh: {e}")),
        };
        if let Ok(url) = &result {
            info!("tui", "Feature request created: {url}");
        }
        let _ = tx.send(match result {
            Ok(url) => Msg::Popup(format!("✓ Feature request filed — thanks!\n\n{url}")),
            Err(e) => Msg::Popup(format!("✗ {e}")),
        });
    });
}
