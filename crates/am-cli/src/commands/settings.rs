//! `am settings list|get|set|reset` (src/commands/settings/*.ts).
//!
//! `settings.json` is shared with the TS app: writes go through
//! `am_core::settings::save`, which keeps unknown keys and the file's key order.

use am_core::info;
use am_core::settings::{self, Settings};
use serde_json::{Map, Value};

use crate::fail;
use crate::output::{self, display_value};

fn valid_keys(view: &Map<String, Value>) -> String {
    view.keys().cloned().collect::<Vec<_>>().join(", ")
}

fn require_key(view: &Map<String, Value>, key: &str) {
    if !view.contains_key(key) {
        fail(&format!("Unknown setting: {key}\nValid keys: {}", valid_keys(view)));
    }
}

pub fn list(json: bool) {
    let view = settings::load().to_view();
    if json {
        output::json(&Value::Object(view));
        return;
    }
    let pairs: Vec<(String, String)> = view.iter().map(|(k, v)| (k.clone(), display_value(v))).collect();
    output::key_value(&pairs);
}

pub fn get(key: &str, json: bool) {
    let view = settings::load().to_view();
    require_key(&view, key);
    let value = &view[key];
    if json {
        output::json(&serde_json::json!({ "key": key, "value": value }));
    } else {
        println!("{}", display_value(value));
    }
}

/// Coerce a CLI string by the type of the key's default value, like the TS
/// command (`true`/`1`, `false`/`0`; `Number(value)`; otherwise a string).
pub fn coerce(default: Option<&Value>, value: &str) -> Result<Value, String> {
    match default {
        Some(Value::Bool(_)) => match value {
            "true" | "1" => Ok(Value::Bool(true)),
            "false" | "0" => Ok(Value::Bool(false)),
            _ => Err(format!("Invalid boolean value: {value}. Use true/false.")),
        },
        Some(Value::Number(_)) => {
            let t = value.trim();
            // JS Number(""): 0. Keep integers integral so the typed fields accept them.
            let n: f64 = if t.is_empty() { 0.0 } else { t.parse().map_err(|_| format!("Invalid number value: {value}"))? };
            if !n.is_finite() {
                return Err(format!("Invalid number value: {value}"));
            }
            if n.fract() == 0.0 && n.abs() < 9.0e15 {
                Ok(Value::from(n as i64))
            } else {
                serde_json::Number::from_f64(n).map(Value::Number).ok_or_else(|| format!("Invalid number value: {value}"))
            }
        }
        _ => Ok(Value::String(value.to_string())),
    }
}

pub fn set(key: &str, value: &str) {
    let current = settings::load();
    let view = current.to_view();
    require_key(&view, key);

    // The sort list is an ordered array of objects that can't be expressed on
    // the command line.
    if key == "worktreeSort" {
        fail(
            "worktreeSort is a list and can't be set from the CLI.\n\
             Edit it in the TUI (s → Sort order) or\n\
             run `am` and use the \"Open settings.json\" action to edit it directly.",
        );
    }

    if key == "keyBindings" {
        fail(
            "keyBindings is a map and can't be set from the CLI.\n\
             Edit it in the TUI (s → Key bindings…) or in settings.json,\n\
             e.g. \"keyBindings\": { \"newWorktree\": \"N\" }.",
        );
    }

    if key == "theme" && !am_core::themes::exists(value) {
        fail(&format!(
            "Unknown theme: {value}\nAvailable: {}",
            am_core::themes::all_names().join(", ")
        ));
    }
    if key == "defaultAgent" && am_core::harness::HarnessKind::get(value).is_none() {
        fail(&format!("Unknown agent: {value}\nAvailable: {}", am_core::harness::ids().join(", ")));
    }

    if key == "ide" && !am_core::launch::IdeChoice::IDS.contains(&value) {
        fail(&format!("Invalid value for ide: {value}. Use one of: {}.", am_core::launch::IdeChoice::IDS.join(", ")));
    }

    if key == "dailyTokensScope" && !["all", "visible"].contains(&value) {
        fail(&format!("Invalid value for dailyTokensScope: {value}. Use all or visible."));
    }

    if key == "reviewSort" && am_core::settings::ReviewSort::parse(value).is_none() {
        fail(&format!(
            "Invalid value for reviewSort: {value}. Use one of: {}.",
            am_core::settings::ReviewSort::IDS.join(", ")
        ));
    }

    if key == "reviewRequestsScope" && am_core::settings::ReviewScope::parse(value).is_none() {
        fail(&format!(
            "Invalid value for reviewRequestsScope: {value}. Use one of: {}.",
            am_core::settings::ReviewScope::IDS.join(", ")
        ));
    }

    if key == "appMode" && am_core::settings::AppMode::parse(value).is_none() {
        fail(&format!("Invalid value for appMode: {value}. Use one of: {}.", am_core::settings::AppMode::IDS.join(", ")));
    }

    use am_core::settings::{WORKTREE_GIT_FILTERS, WORKTREE_PR_FILTERS, WORKTREE_STATUS_FILTERS};
    if key == "worktreeFilterStatus"
        && value != "any"
        && value.split(',').map(str::trim).any(|id| id != "active" && !WORKTREE_STATUS_FILTERS.contains(&id))
    {
        fail(&format!(
            "Invalid value for worktreeFilterStatus: {value}. Use any, or a comma-separated list of: {}.",
            WORKTREE_STATUS_FILTERS.join(", ")
        ));
    }
    for (name, ids) in [
        ("worktreeFilterPr", &WORKTREE_PR_FILTERS[..]),
        ("worktreeFilterGit", &WORKTREE_GIT_FILTERS[..]),
    ] {
        if key == name && !ids.contains(&value) {
            fail(&format!("Invalid value for {name}: {value}. Use one of: {}.", ids.join(", ")));
        }
    }

    if key == "reviewGroups" || key == "reviewDefaultGroup" {
        fail(&format!(
            "{key} holds review groups and can't be set from the CLI.\n\
             Edit them on the review page (v, then f → Groups…) or in settings.json."
        ));
    }

    if key == "dailyTokensMode" && am_core::settings::DailyTokensMode::parse(value).is_none() {
        fail(&format!(
            "Invalid value for dailyTokensMode: {value}. Use one of: {}.",
            am_core::settings::DailyTokensMode::IDS.join(", ")
        ));
    }

    let defaults = Settings::default().to_view();
    let coerced = if key == "contextWindowTokens" {
        // Token counts also take `200k` / `1m`, like the TUI editor.
        match am_core::context::parse_tokens(value) {
            Some(n) if n >= am_core::context::MIN_WINDOW => Value::from(n),
            _ => fail(&format!("Invalid token count: {value}. Use a number ≥ 1000, e.g. 128000, 200k or 1m.")),
        }
    } else {
        coerce(defaults.get(key), value).unwrap_or_else(|e| fail(&e))
    };

    // Apply to the file's own JSON, then re-parse so the typed fields agree.
    let mut raw = current.raw.clone();
    raw.insert(key.to_string(), coerced.clone());
    let mut next = Settings::parse(&Value::Object(raw.clone()).to_string());
    next.raw = raw;
    if let Err(e) = settings::save(&next) {
        fail(&format!("Failed to save settings: {e}"));
    }
    info!("settings", "Set {key} = {coerced}");
    println!("{key} = {}", display_value(&coerced));
}

pub fn reset() {
    // Known keys go back to their defaults; keys the settings model doesn't
    // know (e.g. the TS app's `setupCompleted`) are kept so a reset doesn't
    // re-trigger the setup wizard.
    let next = settings::reset_to_defaults(settings::load(), true);
    if let Err(e) = settings::save(&next) {
        fail(&format!("Failed to save settings: {e}"));
    }
    info!("settings", "Settings reset to defaults");
    println!("Settings reset to defaults.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn coerces_like_ts() {
        let b = json!(true);
        let n = json!(1);
        assert_eq!(coerce(Some(&b), "1").unwrap(), json!(true));
        assert_eq!(coerce(Some(&b), "false").unwrap(), json!(false));
        assert!(coerce(Some(&b), "maybe").unwrap_err().contains("boolean"));
        assert_eq!(coerce(Some(&n), "5000").unwrap(), json!(5000));
        assert_eq!(coerce(Some(&n), "1.5").unwrap(), json!(1.5));
        assert!(coerce(Some(&n), "abc").unwrap_err().contains("number"));
        assert_eq!(coerce(None, "x").unwrap(), json!("x"));
    }
}
