//! `am harness …` (alias `plugin`): manage harness plugins and train new ones
//! (see `am_core::harness::{lifecycle, validate, capture, replay, training}`).
//! Every command here is also what a training agent runs, so errors say what
//! to do next and `--json` is available wherever output is structured.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use am_core::db;
use am_core::harness::lifecycle::{self, AddOptions};
use am_core::harness::manifest::{Manifest, PluginStatus, valid_id};
use am_core::harness::validate::{self, Context, Report};
use am_core::harness::{self, HarnessKind, Origin, capture, replay, training};
use am_core::hooks_installer as hi;
use am_core::paths;
use serde_json::{Value, json};

use crate::fail;
use crate::output::{self, col};

fn kind_or_fail(id: &str) -> HarnessKind {
    HarnessKind::get(id).unwrap_or_else(|| {
        let errs = harness::plugin_errors().iter().filter(|e| e.starts_with(&format!("{id}.json:"))).cloned().collect::<Vec<_>>();
        let why = if errs.is_empty() { String::new() } else { format!("\n{}", errs.join("\n")) };
        fail(&format!("Unknown harness: {id}{why}\nValid harnesses: {}", harness::ids().join(", ")))
    })
}

/// An id that may only exist as captures or issues (a plugin that doesn't load).
fn id_or_fail(id: &str) -> &str {
    if !valid_id(id) {
        fail(&format!("Invalid harness id: {id}"));
    }
    id
}

fn issue_counts() -> HashMap<String, usize> {
    db::open().ok().and_then(|c| db::plugin_issue_counts(&c).ok()).unwrap_or_default()
}

fn source(kind: HarnessKind) -> String {
    match kind.provider().origin() {
        Origin::BuiltIn => "built-in".into(),
        Origin::Plugin(p) => hi::tilde(&p),
    }
}

pub fn list(json: bool) {
    let issues = issue_counts();
    let s = am_core::settings::load();
    let rows: Vec<Value> = harness::all()
        .iter()
        .map(|k| {
            json!({
                "id": k.id(),
                "name": k.provider().name(),
                "source": source(*k),
                "status": k.provider().status().as_str(),
                "enabled": harness::is_enabled(*k, &s),
                "hooks": hi::is_hooks_installed(*k),
                "issues": issues.get(k.id()).copied().unwrap_or(0),
                "captures": if k.is_builtin() { 0 } else { capture::list(k.id()).len() },
            })
        })
        .collect();
    if json {
        output::json(&json!({ "harnesses": rows, "skipped": harness::plugin_errors() }));
        return;
    }
    let table: Vec<Vec<(&'static str, String)>> = rows
        .iter()
        .map(|r| {
            let s = |k: &str| r[k].as_str().unwrap_or_default().to_owned();
            let yes = |k: &str| if r[k].as_bool().unwrap_or(false) { "yes".to_owned() } else { "no".to_owned() };
            vec![
                ("id", s("id")),
                ("status", s("status")),
                ("enabled", yes("enabled")),
                ("hooks", yes("hooks")),
                ("issues", r["issues"].to_string()),
                ("source", s("source")),
            ]
        })
        .collect();
    output::table(
        &table,
        &[col("id", "Id"), col("status", "Status"), col("enabled", "On"), col("hooks", "Hooks"), col("issues", "Issues"), col("source", "Source")],
    );
    for e in harness::plugin_errors() {
        println!("skipped: {e}");
    }
}

pub fn show(id: &str, json: bool) {
    let kind = kind_or_fail(id);
    let p = kind.provider();
    let issues = db::open().ok().and_then(|c| db::plugin_issues(&c, id).ok()).unwrap_or_default();
    let captures = if kind.is_builtin() { 0 } else { capture::list(id).len() };
    let workspace = paths::training_dir().join(id);
    let manifest = p.manifest().map(|m| m.raw.clone()).unwrap_or(Value::Null);
    if json {
        output::json(&json!({
            "id": id,
            "name": p.name(),
            "program": p.program(),
            "source": source(kind),
            "status": p.status().as_str(),
            "hooks": { "settingsPath": hi::display_path(p), "installed": hi::is_hooks_installed(kind), "events": p.hook_events() },
            "captureDir": paths::plugin_capture_dir(id),
            "captures": captures,
            "trainingWorkspace": workspace.exists().then_some(workspace),
            "issues": issues,
            "manifest": manifest,
        }));
        return;
    }
    let mut pairs = vec![
        ("id".to_string(), id.to_string()),
        ("name".into(), p.name().into()),
        ("program".into(), p.program().into()),
        ("source".into(), source(kind)),
        ("status".into(), p.status().as_str().into()),
        ("hooks".into(), format!("{} ({})", hi::display_path(p), if hi::is_hooks_installed(kind) { "installed" } else { "not installed" })),
        ("events".into(), p.hook_events().join(", ")),
    ];
    if !kind.is_builtin() {
        pairs.push(("captures".into(), format!("{captures} in {}", hi::tilde(&paths::plugin_capture_dir(id)))));
        pairs.push(("issues".into(), issues.len().to_string()));
    }
    if workspace.exists() {
        pairs.push(("workspace".into(), hi::tilde(&workspace)));
    }
    if let Some(t) = p.manifest().and_then(|m| m.training.as_ref()) {
        if let Some(v) = &t.verified_with {
            pairs.push(("verified with".into(), v.clone()));
        }
        if let Some(n) = &t.notes {
            pairs.push(("training notes".into(), n.clone()));
        }
    }
    output::key_value(&pairs);
    if !manifest.is_null() {
        println!("\n{}", serde_json::to_string_pretty(&manifest).unwrap_or_default());
    }
}

pub fn schema(json: bool) {
    if json {
        print!("{}", validate::JSON_SCHEMA);
    } else {
        print!("{}", validate::REFERENCE);
    }
}

fn print_report(r: &Report, what: &str) {
    for e in &r.errors {
        println!("✗ {e}");
    }
    for w in &r.warnings {
        println!("! {w}");
    }
    if r.ok() {
        println!("✓ {what} is valid{}", if r.warnings.is_empty() { String::new() } else { format!(" ({} warning(s))", r.warnings.len()) });
    }
}

/// A file path, or the id of a plugin (its manifest on disk).
fn manifest_text(target: &str) -> (String, String, Option<String>) {
    let path = Path::new(target);
    if path.exists() {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| fail(&format!("Can't read {target}: {e}")));
        let id = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.get("id")?.as_str().map(str::to_owned));
        return (text, target.to_owned(), id);
    }
    let kind = kind_or_fail(target);
    let m = kind.provider().manifest().unwrap_or_else(|| fail(&format!("{target} is built in code; it has no manifest")));
    match kind.provider().origin() {
        Origin::Plugin(p) => (std::fs::read_to_string(&p).unwrap_or_else(|e| fail(&e.to_string())), hi::tilde(&p), Some(target.into())),
        Origin::BuiltIn => (m.raw.to_string(), format!("built-in {target}"), Some(target.into())),
    }
}

pub fn validate(target: &str, json: bool) {
    let (text, what, id) = manifest_text(target);
    let caps = id.as_deref().filter(|i| valid_id(i)).map(capture::list).unwrap_or_default();
    let r = validate::validate(&text, &Context { others: harness::all().to_vec(), captures: &caps });
    if json {
        output::json(&serde_json::to_value(&r).unwrap_or_default());
    } else {
        print_report(&r, &what);
    }
    if !r.ok() {
        std::process::exit(1);
    }
}

pub fn add(file: &str, replace: bool, keep_status: bool, no_install: bool) {
    let opts = AddOptions { replace, keep_status, install: !no_install };
    match lifecycle::add(Path::new(file), &opts) {
        Ok(a) => {
            for w in &a.report.warnings {
                println!("! {w}");
            }
            println!("{} {} ({}) → {}", if a.replaced { "Replaced" } else { "Added" }, a.id, a.status.as_str(), hi::tilde(&a.path));
            match &a.hooks {
                Some(h) => println!("Hooks installed into {}", hi::tilde(h)),
                None => println!("Hooks not installed (--no-install); run `am hooks install --agent {}`", a.id),
            }
            if a.status == PluginStatus::Draft {
                println!(
                    "Draft: every hook event is captured to {}.\nNext: trigger events in a new session, then `am harness captures {id}` and `am harness test {id}`.",
                    hi::tilde(&paths::plugin_capture_dir(&a.id)),
                    id = a.id
                );
            }
        }
        Err(e) => fail(&e),
    }
}

pub struct CapturesArgs<'a> {
    pub event: Option<&'a str>,
    pub last: Option<usize>,
    pub full: bool,
    pub json: bool,
    pub clear: bool,
}

pub fn captures(id: &str, a: &CapturesArgs) {
    let id = id_or_fail(id);
    if a.clear {
        println!("Removed {} capture(s) of {id}", capture::clear(id));
        return;
    }
    let mut caps: Vec<_> = capture::list(id).into_iter().filter(|c| a.event.is_none_or(|e| c.native_event == e)).collect();
    if let Some(n) = a.last {
        caps = caps.split_off(caps.len().saturating_sub(n));
    }
    if a.full || a.json && a.last.is_some() {
        let bodies: Vec<Value> = caps.iter().map(|c| c.body.clone()).collect();
        if a.json {
            output::json(&Value::Array(bodies));
        } else {
            for c in &caps {
                println!("── {} ({})", c.native_event, c.path.file_name().unwrap_or_default().to_string_lossy());
                println!("{}", serde_json::to_string_pretty(&c.body).unwrap_or_default());
            }
        }
        return;
    }
    // Summary: per native event, count, last seen and the payload keys seen.
    let mut order: Vec<String> = Vec::new();
    let mut by: HashMap<String, (usize, String, std::collections::BTreeSet<String>)> = HashMap::new();
    for c in &caps {
        let e = by.entry(c.native_event.clone()).or_insert_with(|| {
            order.push(c.native_event.clone());
            (0, String::new(), Default::default())
        });
        e.0 += 1;
        e.1 = c.body.get("receivedAt").and_then(Value::as_str).unwrap_or_default().to_owned();
        if let Some(obj) = c.payload().as_object() {
            e.2.extend(obj.keys().cloned());
        }
    }
    if a.json {
        let rows: Vec<Value> =
            order.iter().map(|ev| json!({ "event": ev, "count": by[ev].0, "lastSeen": by[ev].1, "keys": by[ev].2 })).collect();
        output::json(&json!({ "id": id, "dir": paths::plugin_capture_dir(id), "events": rows }));
        return;
    }
    if caps.is_empty() {
        println!("No captures for {id} yet ({}).", hi::tilde(&paths::plugin_capture_dir(id)));
        println!("Captures are recorded while a plugin is a draft: run its harness in a new session to fire its hooks.");
        return;
    }
    let table: Vec<Vec<(&'static str, String)>> = order
        .iter()
        .map(|ev| {
            let (n, last, keys) = &by[ev];
            vec![("event", ev.clone()), ("count", n.to_string()), ("last", last.clone()), ("keys", keys.iter().cloned().collect::<Vec<_>>().join(" "))]
        })
        .collect();
    output::table(&table, &[col("event", "Native event"), col("count", "Count"), col("last", "Last seen"), col("keys", "Payload keys")]);
    println!("\n{} capture(s) in {}; `--full --last N` prints them.", caps.len(), hi::tilde(&paths::plugin_capture_dir(id)));
}

pub fn test(id: &str, manifest: Option<&str>, files: &[PathBuf], json: bool) {
    let id = id_or_fail(id);
    let m: Manifest = match manifest {
        Some(f) => {
            let text = std::fs::read_to_string(f).unwrap_or_else(|e| fail(&format!("Can't read {f}: {e}")));
            Manifest::parse(&text).unwrap_or_else(|e| fail(&format!("{f}: {e}\nRun `am harness validate {f}` for every problem.")))
        }
        None => kind_or_fail(id).provider().manifest().cloned().unwrap_or_else(|| fail(&format!("{id} has no manifest"))),
    };
    let caps: Vec<_> = if files.is_empty() {
        capture::list(id)
    } else {
        files.iter().map(|f| capture::read(f).unwrap_or_else(|| fail(&format!("Can't read capture {}", f.display())))).collect()
    };
    if caps.is_empty() {
        fail(&format!("No captures for {id}. Fire some hook events first (`am harness captures {id}`)."));
    }
    let r = replay::replay(&m, &caps);
    if json {
        output::json(&serde_json::to_value(&r).unwrap_or_default());
    } else {
        for s in &r.steps {
            let mapped = match &s.canonical {
                Some(c) => format!("{} → {c}", s.native),
                None => format!("{} → (not mapped)", s.native),
            };
            let tool = s.tool.as_deref().map(|t| format!(" tool={t}")).unwrap_or_default();
            let flow = match (s.from, s.to) {
                (_, None) if s.canonical.is_some() => "  (no change)".to_owned(),
                (from, Some(to)) => format!("  {} → {to}", from.unwrap_or("·")),
                _ => String::new(),
            };
            let session = s.session_id.as_deref().map(|x| format!(" [{x}]")).unwrap_or_default();
            println!("{mapped}{tool}{session}{flow}");
            for i in &s.issues {
                println!("    ! {i}");
            }
        }
        let c = &r.coverage;
        println!("\nCoverage: {} capture(s), {} event type(s), {} session(s)", r.steps.len(), c.seen.len(), c.sessions);
        let line = |label: &str, items: &[String]| {
            if !items.is_empty() {
                println!("  {label}: {}", items.join(", "));
            }
        };
        line("not mapped", &c.unmapped_events);
        line("mapped, never captured", &c.never_seen);
        line("tools not mapped", &c.unmapped_tools);
        for (f, n) in &c.missing_fields {
            println!("  missing {f}: {n} event(s)");
        }
    }
    if r.has_failures() {
        std::process::exit(1);
    }
}

pub fn issues(id: &str, json: bool, clear: bool) {
    let id = id_or_fail(id);
    let conn = db::open().unwrap_or_else(|e| fail(&format!("Can't open the database: {e}")));
    if clear {
        let n = db::clear_plugin_issues(&conn, id).unwrap_or(0);
        println!("Cleared {n} issue(s) of {id}");
        return;
    }
    let rows = db::plugin_issues(&conn, id).unwrap_or_default();
    if json {
        output::json(&serde_json::to_value(&rows).unwrap_or_default());
        return;
    }
    if rows.is_empty() {
        println!("No issues recorded for {id}.");
        return;
    }
    let table: Vec<Vec<(&'static str, String)>> = rows
        .iter()
        .map(|i| {
            vec![
                ("kind", i.kind.clone()),
                ("key", i.key.clone()),
                ("count", i.count.to_string()),
                ("last", i.last_seen.clone()),
                ("sample", i.sample.as_deref().map(|s| am_core::status::truncate(s, 80)).unwrap_or_default()),
            ]
        })
        .collect();
    output::table(&table, &[col("kind", "Kind"), col("key", "Key"), col("count", "Count"), col("last", "Last seen"), col("sample", "Sample")]);
}

pub fn set_status(id: &str, status: PluginStatus) {
    match lifecycle::set_status(id, status) {
        Ok(r) => {
            for w in &r.warnings {
                println!("! {w}");
            }
            println!("{id} is {}", status.as_str());
        }
        Err(e) => fail(&e),
    }
}

pub fn remove(id: &str, keep_captures: bool) {
    match lifecycle::remove(id, keep_captures) {
        Ok(()) => println!("Removed {id} (hooks uninstalled; `am harness rollback {id}` restores it)"),
        Err(e) => fail(&e),
    }
}

pub fn rollback(id: &str) {
    match lifecycle::rollback(id) {
        Ok(p) => println!("Restored {id} from {}", hi::tilde(&p)),
        Err(e) => fail(&e),
    }
}

pub fn train(command: Option<&str>, edit: Option<&str>, print: bool, no_open: bool) {
    let mode = match (command, edit) {
        (_, Some(id)) => training::Mode::Edit { id: id.to_owned() },
        (Some(c), None) => training::Mode::Create { command: c.to_owned() },
        (None, None) => fail("Pass --command <how you start the harness> to train a new one, or --edit <id>"),
    };
    let p = training::prepare(&mode).unwrap_or_else(|e| fail(&e));
    if print {
        print!("{}", p.prompt);
        return;
    }
    println!("Workspace: {}\nPrompt: {}", hi::tilde(&p.workspace), hi::tilde(&p.prompt_path));
    if no_open {
        println!("Run `cd {} && {}`, then: {}", am_core::launch::sh_quote(&p.workspace.display().to_string()), p.command, training::BOOTSTRAP);
        return;
    }
    match training::start(&p) {
        Ok(msg) => println!("{msg}"),
        Err(e) => fail(&e),
    }
}
