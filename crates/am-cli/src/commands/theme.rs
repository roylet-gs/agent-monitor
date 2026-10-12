//! `am theme list|show|use|export|import|remove`. Themes are built in or
//! stored one per file in `<app dir>/themes/<name>.json` (see
//! `am_core::themes`); the TUI's theme builder writes the same files.

use std::io::IsTerminal;

use am_core::info;
use am_core::themes::{self, ThemeColor, ThemeSpec, Token};
use serde_json::{Value, json};

use crate::fail;
use crate::output::{self, col};

fn active() -> String {
    am_core::settings::load().theme
}

fn load_or_fail(name: &str) -> ThemeSpec {
    if !themes::exists(name) {
        fail(&format!("Unknown theme: {name}\nRun `am theme list` to see the available themes."));
    }
    themes::load(name).unwrap_or_else(|e| fail(&e))
}

pub fn list(json: bool) {
    let current = active();
    let current = themes::builtin(&current).map(|t| t.name).unwrap_or(current);
    let rows: Vec<(String, &str, bool)> = themes::all_names()
        .into_iter()
        .map(|n| {
            let kind = if themes::is_builtin(&n) { "builtin" } else { "custom" };
            let on = n == current;
            (n, kind, on)
        })
        .collect();
    if json {
        output::json(&Value::Array(rows.iter().map(|(n, k, a)| json!({ "name": n, "kind": k, "active": a })).collect()));
        return;
    }
    let table: Vec<Vec<(&'static str, String)>> = rows
        .into_iter()
        .map(|(n, k, a)| vec![("active", if a { "*".into() } else { String::new() }), ("name", n), ("kind", k.to_string())])
        .collect();
    output::table(&table, &[col("active", " "), col("name", "Name"), col("kind", "Kind")]);
}

fn colors_json(spec: &ThemeSpec) -> Value {
    Value::Object(Token::ALL.iter().map(|t| (t.key().to_string(), Value::from(spec.get(*t).to_string()))).collect())
}

pub fn show(name: &str, json: bool) {
    let spec = load_or_fail(name);
    if json {
        output::json(&json!({ "name": spec.name, "base": spec.base, "colors": colors_json(&spec) }));
        return;
    }
    let tty = std::io::stdout().is_terminal();
    let mut pairs = vec![("name".to_string(), spec.name.clone())];
    if let Some(b) = &spec.base {
        pairs.push(("base".into(), b.clone()));
    }
    for t in Token::ALL {
        let c = spec.get(t);
        let swatch = match (tty, c) {
            (true, ThemeColor::Rgb(r, g, b)) => format!("\x1b[38;2;{r};{g};{b}m██\x1b[0m "),
            (true, ThemeColor::Ansi(i)) => format!("\x1b[38;5;{i}m██\x1b[0m "),
            (true, _) => "   ".into(),
            (false, _) => String::new(),
        };
        pairs.push((t.key().to_string(), format!("{swatch}{c:<10}  {}", t.describe())));
    }
    output::key_value(&pairs);
}

pub fn use_theme(name: &str) {
    load_or_fail(name);
    crate::commands::settings::set("theme", name);
}

/// Every token resolved, with no base, so the file stands alone as a template.
pub fn export(name: &str, out: Option<&str>) {
    let spec = load_or_fail(name);
    let full = ThemeSpec { name: spec.name.clone(), base: None, colors: spec.colors };
    let text = full.to_json();
    match out {
        Some(path) => {
            std::fs::write(path, &text).unwrap_or_else(|e| fail(&format!("Couldn't write {path}: {e}")));
            println!("Exported {name} to {path}");
        }
        None => print!("{text}"),
    }
}

/// Strict check for import: unlike loading (which skips bad values), report
/// every problem so a typo doesn't silently fall back to the base colour.
fn check_file(text: &str) -> Result<(Option<String>, Option<String>), Vec<String>> {
    let v: Value = serde_json::from_str(text).map_err(|e| vec![format!("invalid JSON: {e}")])?;
    let obj = v.as_object().ok_or_else(|| vec!["a theme file must be a JSON object".to_string()])?;
    let mut errors = Vec::new();
    let base = obj.get("base").and_then(Value::as_str).map(str::to_string);
    if let Some(b) = &base
        && !themes::exists(b)
    {
        errors.push(format!("base: unknown theme '{b}'"));
    }
    match obj.get("colors") {
        Some(Value::Object(map)) => {
            for (k, v) in map {
                let Some(t) = Token::from_key(k) else {
                    errors.push(format!("{k}: unknown token (valid: {})", Token::ALL.map(Token::key).join(", ")));
                    continue;
                };
                match v.as_str().map(ThemeColor::parse) {
                    Some(Ok(c)) if themes::allowed(t, c) => {}
                    Some(Ok(c)) => errors.push(format!("{k}: '{c}' only works for selection")),
                    Some(Err(e)) => errors.push(format!("{k}: {e}")),
                    None => errors.push(format!("{k}: expected a string")),
                }
            }
        }
        Some(_) => errors.push("colors: expected an object".into()),
        None => errors.push("colors: missing".into()),
    }
    let name = obj.get("name").and_then(Value::as_str).map(str::to_string);
    if errors.is_empty() { Ok((name, base)) } else { Err(errors) }
}

pub fn import(file: &str, name: Option<&str>, force: bool) {
    let text = std::fs::read_to_string(file).unwrap_or_else(|e| fail(&format!("Couldn't read {file}: {e}")));
    let (file_name, _) = check_file(&text).unwrap_or_else(|errs| fail(&format!("Invalid theme file {file}:\n  {}", errs.join("\n  "))));
    let stem = std::path::Path::new(file).file_stem().and_then(|s| s.to_str()).map(str::to_string);
    let name = name.map(str::to_string).or(file_name).or(stem).unwrap_or_else(|| fail("Give the theme a name with --name"));
    themes::validate_name(&name).unwrap_or_else(|e| fail(&e));
    if themes::exists(&name) && !force {
        fail(&format!("A theme named {name} already exists (use --force to replace it)"));
    }
    let mut spec = ThemeSpec::from_json(&name, &text).unwrap_or_else(|e| fail(&e));
    spec.name = name.clone();
    let path = themes::save_custom(&spec).unwrap_or_else(|e| fail(&e));
    info!("theme", "Imported theme {name} from {file}");
    println!("Imported {name} → {}", path.display());
    println!("Use it with: am theme use {name}");
}

pub fn remove(name: &str) {
    themes::remove_custom(name).unwrap_or_else(|e| fail(&e));
    info!("theme", "Removed theme {name}");
    println!("Removed theme {name}");
    if active() == name {
        crate::commands::settings::set("theme", "ansi");
    }
}
