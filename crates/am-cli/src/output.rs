//! CLI output formatting: table, key-value and JSON. Port of `src/lib/output.ts`
//! (same layout, so scripts parsing `am` output keep working).

use serde_json::Value;

pub struct Column {
    pub key: &'static str,
    pub header: &'static str,
    pub align_right: bool,
}

pub const fn col(key: &'static str, header: &'static str) -> Column {
    Column { key, header, align_right: false }
}

pub const fn col_right(key: &'static str, header: &'static str) -> Column {
    Column { key, header, align_right: true }
}

/// `JSON.stringify(data, null, 2)`.
pub fn json(data: &Value) {
    println!("{}", serde_json::to_string_pretty(data).unwrap_or_else(|_| "null".into()));
}

/// Display width the way the TS `.length` counted it (UTF-16 code units are
/// close enough to chars for the text we print).
fn width(s: &str) -> usize {
    s.chars().count()
}

pub fn render_table(rows: &[Vec<(&'static str, String)>], columns: &[Column]) -> String {
    if rows.is_empty() {
        return "No results.\n".into();
    }
    let get = |row: &Vec<(&'static str, String)>, key: &str| -> String {
        row.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone()).unwrap_or_default()
    };
    let widths: Vec<usize> = columns
        .iter()
        .map(|c| rows.iter().map(|r| width(&get(r, c.key))).max().unwrap_or(0).max(width(c.header)))
        .collect();
    let line = |cells: Vec<String>| -> String {
        let parts: Vec<String> = cells
            .iter()
            .zip(columns)
            .zip(&widths)
            .map(|((text, c), w)| pad_cell(text, *w, c.align_right))
            .collect();
        parts.join("  ")
    };
    let mut out = String::new();
    out.push_str(&line(columns.iter().map(|c| c.header.to_string()).collect()));
    out.push('\n');
    out.push_str(&widths.iter().map(|w| "─".repeat(*w)).collect::<Vec<_>>().join("  "));
    out.push('\n');
    for row in rows {
        out.push_str(&line(columns.iter().map(|c| get(row, c.key)).collect()));
        out.push('\n');
    }
    out
}

pub fn table(rows: &[Vec<(&'static str, String)>], columns: &[Column]) {
    print!("{}", render_table(rows, columns));
}

pub fn render_key_value(pairs: &[(String, String)]) -> String {
    let max = pairs.iter().map(|(k, _)| width(k)).max().unwrap_or(0);
    pairs.iter().map(|(k, v)| format!("{}  {v}\n", pad_end(k, max))).collect()
}

pub fn key_value(pairs: &[(String, String)]) {
    print!("{}", render_key_value(pairs));
}

fn pad_end(text: &str, w: usize) -> String {
    format!("{text}{}", " ".repeat(w.saturating_sub(width(text))))
}

fn pad_cell(text: &str, w: usize, right: bool) -> String {
    if width(text) > w {
        let cut: String = text.chars().take(w.saturating_sub(1)).collect();
        return format!("{cut}…");
    }
    let pad = " ".repeat(w - width(text));
    if right { format!("{pad}{text}") } else { format!("{text}{pad}") }
}

/// `String(value)` for a settings value: strings bare, everything else as JSON
/// (the TS app printed arrays of objects as `[object Object]`).
pub fn display_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_ts_layout() {
        let rows = vec![vec![("name", "repo1".to_string()), ("n", "3".to_string())]];
        let out = render_table(&rows, &[col("name", "Name"), col_right("n", "Worktrees")]);
        assert_eq!(out, "Name   Worktrees\n─────  ─────────\nrepo1          3\n");
    }

    #[test]
    fn empty_table_says_no_results() {
        assert_eq!(render_table(&[], &[col("a", "A")]), "No results.\n");
    }

    #[test]
    fn key_value_pads_keys() {
        let out = render_key_value(&[("PR:".into(), "#1".into()), ("Status:".into(), "Open".into())]);
        assert_eq!(out, "PR:      #1\nStatus:  Open\n");
    }
}
