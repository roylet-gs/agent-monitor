//! Read-only view of `settings.json` — only the keys the native binaries need.
//! The file is shared with the TS app, so unknown keys are ignored, never dropped.

use crate::log::Level;
use crate::paths;

#[derive(Debug, Clone)]
pub struct Settings {
    pub log_level: Level,
    pub max_log_size_mb: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self { log_level: Level::Info, max_log_size_mb: 2 }
    }
}

pub fn load() -> Settings {
    let mut s = Settings::default();
    let Ok(raw) = std::fs::read_to_string(paths::settings_path()) else { return s };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else { return s };
    if let Some(level) = v.get("logLevel").and_then(|l| l.as_str()).and_then(Level::parse) {
        s.log_level = level;
    }
    if let Some(mb) = v.get("maxLogSizeMb").and_then(|m| m.as_u64()) {
        s.max_log_size_mb = mb.max(1);
    }
    s
}
