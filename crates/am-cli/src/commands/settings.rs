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

    if key == "theme" && !am_core::themes::exists(value) {
        fail(&format!(
            "Unknown theme: {value}\nAvailable: {}",
            am_core::themes::all_names().join(", ")
        ));
    }

    let defaults = Settings::default().to_view();
    let coerced = coerce(defaults.get(key), value).unwrap_or_else(|e| fail(&e));

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
    let current = settings::load();
    let known = Settings::default().to_view();
    let next = Settings {
        raw: current.raw.into_iter().filter(|(k, _)| !known.contains_key(k)).collect(),
        ..Settings::default()
    };
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
