//! `claude -p --output-format stream-json --verbose` output, one JSON event
//! per line, turned into lines a person can read in the jobs pane (a review
//! script's output): the session starting, the assistant's text, each tool
//! call as `→ Tool summary`, and the result. Tool results and other events
//! are dropped. Lines that aren't such events are left alone (`None`).
//! Paths under `cwd` (the script's directory) are shown relative to it.

use serde_json::Value;

use crate::status::summarize_tool_input;

pub fn readable(line: &str, cwd: Option<&str>) -> Option<Vec<String>> {
    let v: Value = serde_json::from_str(line).ok()?;
    let kind = v.get("type")?.as_str()?;
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    Some(match kind {
        "system" if v.get("subtype").and_then(Value::as_str) == Some("init") => {
            let id: String = s(&v, "session_id").unwrap_or_default().chars().take(8).collect();
            let model = s(&v, "model").map(|m| format!(" ({m})")).unwrap_or_default();
            vec![format!("● Claude session {id} started{model}")]
        }
        "assistant" => {
            let content = v.pointer("/message/content").and_then(Value::as_array).cloned().unwrap_or_default();
            let mut out = Vec::new();
            for c in &content {
                match c.get("type").and_then(Value::as_str) {
                    Some("text") => out.extend(
                        s(c, "text").unwrap_or_default().lines().map(str::trim_end).filter(|l| !l.is_empty()).map(str::to_string),
                    ),
                    Some("tool_use") => {
                        let name = s(c, "name").unwrap_or_default();
                        let what = summarize_tool_input(&name, c.get("input"), cwd).map(|w| format!(" {w}")).unwrap_or_default();
                        out.push(format!("→ {name}{what}"));
                    }
                    _ => {}
                }
            }
            out
        }
        "result" => {
            let secs = v.get("duration_ms").and_then(Value::as_u64).unwrap_or(0) / 1000;
            let took = if secs >= 60 { format!("{}m {}s", secs / 60, secs % 60) } else { format!("{secs}s") };
            let turns = v.get("num_turns").and_then(Value::as_u64).map(|n| format!(" · {n} turns")).unwrap_or_default();
            if v.get("is_error").and_then(Value::as_bool) == Some(true) {
                let why = s(&v, "result").or(s(&v, "subtype")).unwrap_or_default();
                vec![format!("✗ failed after {took}{turns}: {why}")]
            } else {
                vec![format!("✓ done in {took}{turns}")]
            }
        }
        // Tool results, hook and rate-limit events, …: not worth a line.
        "user" | "system" | "stream_event" | "rate_limit_event" => Vec::new(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_json_reads_as_text_tool_calls_and_a_summary() {
        let init = r#"{"type":"system","subtype":"init","session_id":"3a7c91de-0000","model":"claude-opus-5-5","cwd":"/w"}"#;
        assert_eq!(readable(init, None), Some(vec!["● Claude session 3a7c91de started (claude-opus-5-5)".to_string()]));
        let turn = r#"{"type":"assistant","message":{"content":[
            {"type":"thinking","thinking":"","signature":"x"},
            {"type":"text","text":"Looking at the diff.\n\nFirst the parser."},
            {"type":"tool_use","name":"Read","input":{"file_path":"/w/src/parse.rs"}},
            {"type":"tool_use","name":"Bash","input":{"command":"git diff origin/main"}}]}}"#
            .replace('\n', "");
        assert_eq!(
            readable(&turn, Some("/w")).unwrap(),
            vec!["Looking at the diff.", "First the parser.", "→ Read src/parse.rs", "→ Bash git diff origin/main"]
        );
        let tool_result = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"..."}]}}"#;
        assert_eq!(readable(tool_result, None), Some(vec![]));
        for noise in [
            r#"{"type":"system","subtype":"hook_started","hook_name":"SessionStart:startup"}"#,
            r#"{"type":"system","subtype":"thinking_tokens","estimated_tokens":50}"#,
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed"}}"#,
        ] {
            assert_eq!(readable(noise, None), Some(vec![]), "{noise}");
        }
        let done = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":133500,"num_turns":14,"result":"LGTM"}"#;
        assert_eq!(readable(done, None), Some(vec!["✓ done in 2m 13s · 14 turns".to_string()]));
        let failed = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"duration_ms":4000}"#;
        assert_eq!(readable(failed, None), Some(vec!["✗ failed after 4s: error_max_turns".to_string()]));
    }

    #[test]
    fn other_lines_are_left_alone() {
        assert_eq!(readable("Review script finished for #61.", None), None);
        assert_eq!(readable(r#"{"level":"info","msg":"not claude"}"#, None), None);
        assert_eq!(readable(r#"{"type":"something-new"}"#, None), None);
    }
}
