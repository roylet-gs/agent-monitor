use std::path::PathBuf;

/// `~/.agent-monitor`, or `$AM_DATA_DIR` when set (test isolation).
pub fn app_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("AM_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    home_dir().join(".agent-monitor")
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn db_path() -> PathBuf {
    app_dir().join("agent-monitor.db")
}

pub fn settings_path() -> PathBuf {
    app_dir().join("settings.json")
}

pub fn log_path() -> PathBuf {
    app_dir().join("debug.log")
}

pub fn socket_path() -> PathBuf {
    app_dir().join("am.sock")
}

pub fn capture_dir() -> PathBuf {
    app_dir().join("hook-captures")
}

/// Custom themes, one `<name>.json` each.
pub fn themes_dir() -> PathBuf {
    app_dir().join("themes")
}

/// Last update check (`update_check`), shared by every running TUI.
pub fn update_check_path() -> PathBuf {
    app_dir().join("update-check.json")
}

/// Hook payloads captured for one plugin (training mode): `hook-captures/<id>/`.
pub fn plugin_capture_dir(id: &str) -> PathBuf {
    capture_dir().join(id)
}

/// Training workspaces, one git-initialised dir per session (`training/<slug>/`).
pub fn training_dir() -> PathBuf {
    app_dir().join("training")
}
