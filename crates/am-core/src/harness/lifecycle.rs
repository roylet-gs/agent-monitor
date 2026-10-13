//! Adding, publishing, removing and rolling back manifest plugins, shared by
//! `am harness …` and the TUI's Harnesses page.
//!
//! Every change goes through the same steps: validate, uninstall the hooks of
//! the version on disk (so events a draft captured are cleaned up too), keep
//! the previous file in `<app dir>/agents/.history/<id>/`, write the new one,
//! reinstall, and [`reload`](super::reload) this process's registry. The first
//! install into a harness's settings file leaves a `<file>.am-backup` copy.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::manifest::{Manifest, ManifestProvider, PluginStatus};
use super::validate::{self, Context, Report};
use super::{HarnessKind, capture, plugins_dir};
use crate::{db, hooks_installer, info};

fn history_dir(id: &str) -> PathBuf {
    plugins_dir().join(".history").join(id)
}

pub fn manifest_path(id: &str) -> PathBuf {
    plugins_dir().join(format!("{id}.json"))
}

/// The manifest plugin `id` as it is on disk now.
fn on_disk(id: &str) -> Option<ManifestProvider> {
    let source = manifest_path(id);
    let manifest = Manifest::parse(&std::fs::read_to_string(&source).ok()?).ok()?;
    Some(ManifestProvider { manifest, source })
}

fn stamp() -> String {
    time::OffsetDateTime::now_utc()
        .format(time::macros::format_description!("[year][month][day]-[hour][minute][second]"))
        .unwrap_or_else(|_| "0".into())
}

/// Move the current file into the history; returns where it went.
fn archive(id: &str) -> std::io::Result<Option<PathBuf>> {
    let src = manifest_path(id);
    if !src.exists() {
        return Ok(None);
    }
    let dir = history_dir(id);
    std::fs::create_dir_all(&dir)?;
    let mut dst = dir.join(format!("{}.json", stamp()));
    let mut n = 1;
    while dst.exists() {
        dst = dir.join(format!("{}-{n}.json", stamp()));
        n += 1;
    }
    std::fs::copy(&src, &dst)?;
    Ok(Some(dst))
}

/// A copy of the harness's settings file before am first writes into it.
fn backup_settings(p: &ManifestProvider) {
    use super::Harness;
    let path = p.settings_path();
    let backup = PathBuf::from(format!("{}.am-backup", path.display()));
    if path.exists() && !backup.exists() && std::fs::copy(&path, &backup).is_ok() {
        info!("hooks", "Backed up {} to {}", path.display(), backup.display());
    }
}

/// `text` with `status` set, keeping the file's key order.
fn with_status(text: &str, status: PluginStatus) -> Result<String, String> {
    let mut v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("manifest must be a JSON object")?;
    obj.insert("status".into(), status.as_str().into());
    let mut out = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    out.push('\n');
    Ok(out)
}

fn errors_text(r: &Report) -> String {
    r.errors.iter().map(|e| format!("  ✗ {e}")).collect::<Vec<_>>().join("\n")
}

#[derive(Debug)]
pub struct Added {
    pub id: String,
    pub path: PathBuf,
    pub status: PluginStatus,
    pub replaced: bool,
    /// The hook file written, `None` with `install: false`.
    pub hooks: Option<PathBuf>,
    pub report: Report,
}

pub struct AddOptions {
    /// Replace an existing plugin with the same id.
    pub replace: bool,
    /// Keep the file's own `status` instead of forcing `draft`.
    pub keep_status: bool,
    /// Install the hooks.
    pub install: bool,
}

/// Validate `src` and install it as plugin `<id>` (a draft unless
/// `keep_status`). Errors carry every validation problem.
pub fn add(src: &Path, opts: &AddOptions) -> Result<Added, String> {
    let text = std::fs::read_to_string(src).map_err(|e| format!("can't read {}: {e}", src.display()))?;
    let id = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.get("id")?.as_str().map(str::to_owned));
    let caps = id.as_deref().map(capture::list).unwrap_or_default();
    let others: Vec<HarnessKind> = super::all().to_vec();
    let report = validate::validate(&text, &Context { others, captures: &caps });
    if !report.ok() {
        return Err(format!("{} has errors:\n{}", src.display(), errors_text(&report)));
    }
    let manifest = Manifest::parse(&text)?;
    let id = manifest.id.clone();
    let target = manifest_path(&id);
    let existing = on_disk(&id);
    if target.exists() && !opts.replace && src.canonicalize().ok() != target.canonicalize().ok() {
        return Err(format!("a plugin \"{id}\" already exists ({}); use --replace", target.display()));
    }
    let status = if opts.keep_status { manifest.status } else { PluginStatus::Draft };
    let text = with_status(&text, status)?;
    let fresh = ManifestProvider { manifest: Manifest::parse(&text)?, source: target.clone() };

    if let Some(old) = &existing {
        hooks_installer::uninstall_for(old).map_err(|e| format!("couldn't remove the old hooks: {e}"))?;
        archive(&id).map_err(|e| format!("couldn't keep the old version: {e}"))?;
    }
    std::fs::create_dir_all(plugins_dir()).map_err(|e| e.to_string())?;
    std::fs::write(&target, &text).map_err(|e| format!("can't write {}: {e}", target.display()))?;
    let hooks = if opts.install {
        backup_settings(&fresh);
        hooks_installer::install_for(&fresh).map_err(|e| format!("added, but installing hooks failed: {e}"))?;
        use super::Harness;
        Some(fresh.settings_path())
    } else {
        None
    };
    super::reload();
    info!("harness", "Added plugin {id} ({}) from {}", status.as_str(), src.display());
    Ok(Added { id, path: target, status, replaced: existing.is_some(), hooks, report })
}

fn plugin(id: &str) -> Result<ManifestProvider, String> {
    if super::BUILTIN.iter().any(|k| k.id() == id) {
        return Err(format!("{id} is built in; only manifest plugins can be changed"));
    }
    on_disk(id).ok_or_else(|| format!("no plugin \"{id}\" in {} (or it doesn't load; see `am harness list`)", plugins_dir().display()))
}

/// Mark plugin `id` draft or published. Publishing needs a clean validation;
/// the hooks are reinstalled so a draft's capture-only events come and go.
pub fn set_status(id: &str, status: PluginStatus) -> Result<Report, String> {
    let old = plugin(id)?;
    let text = std::fs::read_to_string(&old.source).map_err(|e| e.to_string())?;
    let report = validate::validate(&text, &Context { others: super::all().to_vec(), captures: &capture::list(id) });
    if status == PluginStatus::Published && !report.ok() {
        return Err(format!("{id} can't be published yet:\n{}", errors_text(&report)));
    }
    if old.manifest.status == status {
        return Ok(report);
    }
    let was_installed = hooks_installer::is_installed_for(&old);
    let text = with_status(&text, status)?;
    let fresh = ManifestProvider { manifest: Manifest::parse(&text)?, source: old.source.clone() };
    if was_installed {
        hooks_installer::uninstall_for(&old).map_err(|e| e.to_string())?;
    }
    archive(id).map_err(|e| e.to_string())?;
    std::fs::write(&old.source, text).map_err(|e| e.to_string())?;
    if was_installed {
        hooks_installer::install_for(&fresh).map_err(|e| e.to_string())?;
    }
    super::reload();
    info!("harness", "Plugin {id} is now {}", status.as_str());
    Ok(report)
}

/// Uninstall plugin `id`'s hooks and move its manifest into the history.
/// Its issues go; its captures too unless `keep_captures`.
pub fn remove(id: &str, keep_captures: bool) -> Result<(), String> {
    let old = plugin(id)?;
    hooks_installer::uninstall_for(&old).map_err(|e| format!("couldn't remove the hooks: {e}"))?;
    archive(id).map_err(|e| e.to_string())?;
    std::fs::remove_file(&old.source).map_err(|e| e.to_string())?;
    if let Ok(conn) = db::open() {
        let _ = db::clear_plugin_issues(&conn, id);
    }
    if !keep_captures {
        capture::clear(id);
    }
    super::reload();
    info!("harness", "Removed plugin {id}");
    Ok(())
}

/// Saved versions of plugin `id`, newest first.
pub fn history(id: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(history_dir(id)) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    files.sort();
    files.reverse();
    files
}

/// Put back the newest saved version of plugin `id` (the current one is
/// saved first, so a rollback can itself be undone).
pub fn rollback(id: &str) -> Result<PathBuf, String> {
    let current = on_disk(id);
    let prev = history(id).into_iter().next().ok_or_else(|| format!("no earlier version of {id}"))?;
    let text = std::fs::read_to_string(&prev).map_err(|e| e.to_string())?;
    let restored = ManifestProvider { manifest: Manifest::parse(&text)?, source: manifest_path(id) };
    let was_installed = current.as_ref().is_some_and(|c| hooks_installer::is_installed_for(c));
    if let Some(cur) = &current {
        hooks_installer::uninstall_for(cur).map_err(|e| e.to_string())?;
    }
    let tmp = prev.with_extension("restoring");
    std::fs::rename(&prev, &tmp).map_err(|e| e.to_string())?;
    archive(id).map_err(|e| e.to_string())?;
    std::fs::write(manifest_path(id), &text).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&tmp);
    if was_installed || current.is_none() {
        hooks_installer::install_for(&restored).map_err(|e| e.to_string())?;
    }
    super::reload();
    info!("harness", "Rolled plugin {id} back to {}", prev.display());
    Ok(prev)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;

    fn manifest(settings: &Path, extra: &str) -> String {
        format!(
            r#"{{ "id": "toycli", "name": "Toy CLI", "program": "sh", "configDir": "/",
                 "hooks": {{ "settingsPath": "{}", "projectDirVar": "TOY_DIR" }},
                 "events": {{ "Prompt": "UserPromptSubmit", "Done": "Stop" }},
                 "training": {{ "captureEvents": ["Thinking"] }} {extra} }}"#,
            settings.display()
        )
    }

    fn hooked_events(settings: &Path) -> Vec<String> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(settings).unwrap_or("{}".into())).unwrap();
        v.get("hooks").and_then(Value::as_object).map(|h| h.keys().cloned().collect()).unwrap_or_default()
    }

    #[test]
    fn add_publish_rollback_and_remove() {
        let _g = crate::test_env::lock();
        let tmp = tempfile::tempdir().unwrap();
        let _env = crate::test_env::data_dir(tmp.path());
        let settings = tmp.path().join("toy-settings.json");
        std::fs::write(&settings, r#"{"theme":"dark"}"#).unwrap();
        let src = tmp.path().join("manifest.json");
        std::fs::write(&src, manifest(&settings, "")).unwrap();

        let added = add(&src, &AddOptions { replace: false, keep_status: false, install: true }).unwrap();
        assert_eq!(added.status, PluginStatus::Draft);
        assert!(PathBuf::from(format!("{}.am-backup", settings.display())).exists());
        assert_eq!(hooked_events(&settings), ["Prompt", "Done", "Thinking"]);
        let k = HarnessKind::get("toycli").expect("registered after reload");
        assert!(k.is_draft());
        assert!(add(&src, &AddOptions { replace: false, keep_status: false, install: true }).unwrap_err().contains("--replace"));

        set_status("toycli", PluginStatus::Published).unwrap();
        assert_eq!(hooked_events(&settings), ["Prompt", "Done"], "capture-only events go on publish");
        assert!(!HarnessKind::get("toycli").unwrap().is_draft());
        assert_eq!(history("toycli").len(), 1);

        rollback("toycli").unwrap();
        assert!(HarnessKind::get("toycli").unwrap().is_draft());
        assert_eq!(hooked_events(&settings), ["Prompt", "Done", "Thinking"]);

        remove("toycli", false).unwrap();
        assert!(HarnessKind::get("toycli").is_none());
        assert!(hooked_events(&settings).is_empty());
        let kept: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(kept["theme"], "dark", "other settings survive");
        assert!(set_status("gemini", PluginStatus::Draft).unwrap_err().contains("built in"));
    }

    #[test]
    fn publishing_is_refused_with_errors() {
        let _g = crate::test_env::lock();
        let tmp = tempfile::tempdir().unwrap();
        let _env = crate::test_env::data_dir(tmp.path());
        let src = tmp.path().join("bad.json");
        std::fs::write(&src, manifest(&tmp.path().join("s.json"), r#", "surprise": 1"#)).unwrap();
        let err = add(&src, &AddOptions { replace: false, keep_status: false, install: false }).unwrap_err();
        assert!(err.contains("/surprise: unknown key"), "{err}");
        let p = ManifestProvider { manifest: Manifest::parse(&manifest(&tmp.path().join("s.json"), "")).unwrap(), source: src };
        assert_eq!(p.hook_events(), ["Prompt", "Done"], "not a draft: no capture events");
    }
}
