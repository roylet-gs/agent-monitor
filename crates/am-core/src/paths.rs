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
