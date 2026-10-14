//! The MCP stdio transport: newline-delimited JSON-RPC 2.0 on stdin/stdout.
//! Only protocol messages are written to stdout; logs go to `debug.log`.
//!
//! A `tools/call` runs this same `am` binary with the tool's argv as a child
//! process (stdin closed, the server's cwd), so every command keeps its CLI
//! behaviour, including exiting on errors, without taking the server down.

use std::io::{BufRead, Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use am_core::{debug, info, warn};
use serde_json::{Value, json};

use super::tools::{self, Tool};

/// Protocol revisions this server speaks; the newest is offered when the
/// client asks for one we don't know.
const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "agent-monitor (am) tracks git worktrees and the coding agents running in them. \
Use these tools to list worktrees with their agent status, git, PR/CI and Linear state, create or delete worktrees, \
and read or change am's settings. Tools default to the repository containing your working directory; pass `repo` \
to target another tracked repository. A worktree `target` is its branch name or path.";

/// What one child `am` run produced.
pub struct Output {
    /// Exit code; `None` when killed (timeout or signal).
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

pub fn serve() {
    info!("mcp", "MCP server started in {}", std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default());
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(req) => handle(&req, &run_am),
            Err(e) => Some(error(Value::Null, -32700, &format!("Parse error: {e}"))),
        };
        if let Some(r) = reply
            && (writeln!(out, "{r}").is_err() || out.flush().is_err())
        {
            break;
        }
    }
    info!("mcp", "MCP server stopped (stdin closed)");
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// The reply to one message (`None` for a notification). `run` executes a
/// tool's argv; tests pass a fake.
pub fn handle(req: &Value, run: &dyn Fn(&Tool, &[String]) -> Output) -> Option<Value> {
    let method = req.get("method").and_then(Value::as_str);
    let Some(id) = req.get("id").cloned() else {
        debug!("mcp", "Notification {}", method.unwrap_or("?"));
        return None;
    };
    let Some(method) = method else {
        return Some(error(id, -32600, "Invalid request: no method"));
    };
    debug!("mcp", "Request {method}");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
            let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
            let client = params.pointer("/clientInfo/name").and_then(Value::as_str).unwrap_or("?");
            info!("mcp", "Initialized by {client} (asked {asked}, using {version})");
            ok(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "agent-monitor", "title": "Agent Monitor", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({"tools": tools::TOOLS.iter().map(Tool::describe).collect::<Vec<_>>()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let Some(tool) = tools::find(name) else {
                return Some(error(id, -32602, &format!("Unknown tool: {name}")));
            };
            let empty = serde_json::Map::new();
            let args = match params.get("arguments") {
                None | Some(Value::Null) => &empty,
                Some(Value::Object(m)) => m,
                Some(_) => return Some(error(id, -32602, "arguments must be an object")),
            };
            ok(id, call(tool, args, run))
        }
        _ => error(id, -32601, &format!("Method not found: {method}")),
    })
}

fn text_result(text: &str, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

fn call(tool: &Tool, args: &serde_json::Map<String, Value>, run: &dyn Fn(&Tool, &[String]) -> Output) -> Value {
    let argv = match tool.argv(args) {
        Ok(a) => a,
        Err(e) => {
            debug!("mcp", "{} refused: {e}", tool.name);
            return text_result(&format!("Invalid arguments: {e}"), true);
        }
    };
    let started = Instant::now();
    let out = run(tool, &argv);
    let success = out.code == Some(0) && !out.timed_out;
    info!(
        "mcp",
        "{} → am {} exited {} in {} ms",
        tool.name,
        argv.join(" "),
        out.code.map(|c| c.to_string()).unwrap_or_else(|| "(killed)".into()),
        started.elapsed().as_millis()
    );

    let stdout = out.stdout.trim();
    let parsed = if success { serde_json::from_str::<Value>(stdout).ok() } else { None };
    // Compact JSON: the CLI pretty-prints, which costs the agent tokens.
    let mut text = parsed.as_ref().map(Value::to_string).unwrap_or_else(|| stdout.to_string());
    let stderr = out.stderr.trim();
    if !stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(stderr);
    }
    if out.timed_out {
        text.push_str(&format!("\nTimed out after {} s", tool.timeout.as_secs()));
    } else if text.is_empty() {
        text = match out.code {
            Some(0) => "Done.".into(),
            Some(c) => format!("Failed (exit code {c})."),
            None => "Failed (killed).".into(),
        };
    }
    let mut result = text_result(text.trim(), !success);
    if let Some(v) = parsed {
        result["structuredContent"] = if v.is_object() { v } else { json!({"result": v}) };
    }
    result
}

/// Run this `am` binary with `argv`, killing it after the tool's timeout.
fn run_am(tool: &Tool, argv: &[String]) -> Output {
    let failed = |msg: String| Output { code: None, stdout: String::new(), stderr: msg, timed_out: false };
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return failed(format!("can't find the am binary: {e}")),
    };
    let mut child = match Command::new(&exe).args(argv).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!("mcp", "Couldn't run {}: {e}", exe.display());
            return failed(format!("couldn't run {}: {e}", exe.display()));
        }
    };
    // Read both pipes on threads so a chatty child never blocks on a full pipe.
    let drain = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(mut r) = r {
                let _ = r.read_to_string(&mut s);
            }
            s
        })
    };
    let out = drain(child.stdout.take().map(|r| Box::new(r) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|r| Box::new(r) as Box<dyn Read + Send>));

    let deadline = Instant::now() + tool.timeout;
    let (code, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code(), false),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                warn!("mcp", "{} timed out after {} s; killing it", tool.name, tool.timeout.as_secs());
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            Err(e) => {
                let _ = child.kill();
                return failed(format!("waiting for am failed: {e}"));
            }
        }
    };
    // A process the child launched (an editor, a browser opener) can inherit
    // the pipes and keep them open, so don't wait for EOF for long.
    let grace = Instant::now() + if timed_out { Duration::ZERO } else { Duration::from_secs(2) };
    let join = |h: std::thread::JoinHandle<String>| {
        while !h.is_finished() && Instant::now() < grace {
            std::thread::sleep(Duration::from_millis(10));
        }
        if h.is_finished() { h.join().unwrap_or_default() } else { String::new() }
    };
    Output { code, stdout: join(out), stderr: join(err), timed_out }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(code: i32, stdout: &'static str, stderr: &'static str) -> impl Fn(&Tool, &[String]) -> Output {
        move |_, _| Output { code: Some(code), stdout: stdout.into(), stderr: stderr.into(), timed_out: false }
    }

    fn req(method: &str, params: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params})
    }

    #[test]
    fn initialize_negotiates_the_protocol_version() {
        let r = handle(&req("initialize", json!({"protocolVersion": "2025-03-26"})), &fake(0, "", "")).unwrap();
        assert_eq!(r["id"], 7);
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "agent-monitor");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        let r = handle(&req("initialize", json!({"protocolVersion": "1999-01-01"})), &fake(0, "", "")).unwrap();
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[test]
    fn notifications_get_no_reply_and_unknown_methods_an_error() {
        assert!(handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}), &fake(0, "", "")).is_none());
        assert_eq!(handle(&req("resources/list", json!({})), &fake(0, "", "")).unwrap()["error"]["code"], -32601);
        assert_eq!(handle(&req("ping", Value::Null), &fake(0, "", "")).unwrap()["result"], json!({}));
    }

    #[test]
    fn tools_list_describes_every_tool() {
        let r = handle(&req("tools/list", json!({})), &fake(0, "", "")).unwrap();
        let listed = r["result"]["tools"].as_array().unwrap();
        assert_eq!(listed.len(), tools::TOOLS.len());
        let delete = listed.iter().find(|t| t["name"] == "worktree_delete").unwrap();
        assert_eq!(delete["annotations"]["destructiveHint"], true);
        assert_eq!(delete["inputSchema"]["required"], json!(["target"]));
    }

    #[test]
    fn a_call_returns_compact_json_and_structured_content() {
        let r = handle(
            &req("tools/call", json!({"name": "worktree_list", "arguments": {}})),
            &fake(0, "[\n  {\"branch\": \"main\"}\n]\n", ""),
        )
        .unwrap();
        let res = &r["result"];
        assert_eq!(res["isError"], false);
        assert_eq!(res["content"][0]["text"], r#"[{"branch":"main"}]"#);
        assert_eq!(res["structuredContent"], json!({"result": [{"branch": "main"}]}));
    }

    #[test]
    fn failures_are_tool_errors_not_protocol_errors() {
        let r = handle(
            &req("tools/call", json!({"name": "worktree_info", "arguments": {"target": "nope"}})),
            &fake(1, "", "Worktree not found: nope\n"),
        )
        .unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert_eq!(r["result"]["content"][0]["text"], "Worktree not found: nope");
        assert!(r["result"].get("structuredContent").is_none());

        let r = handle(&req("tools/call", json!({"name": "worktree_info", "arguments": {}})), &fake(0, "", "")).unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().starts_with("Invalid arguments"));

        let r = handle(&req("tools/call", json!({"name": "rm_rf"})), &fake(0, "", "")).unwrap();
        assert_eq!(r["error"]["code"], -32602);
    }

    #[test]
    fn plain_text_output_passes_through() {
        let r = handle(&req("tools/call", json!({"name": "worktree_sync"})), &fake(0, "Synced 2 worktrees\n", "")).unwrap();
        assert_eq!(r["result"]["content"][0]["text"], "Synced 2 worktrees");
        let r = handle(&req("tools/call", json!({"name": "settings_reset"})), &fake(0, "", "")).unwrap();
        assert_eq!(r["result"]["content"][0]["text"], "Done.");
    }
}
