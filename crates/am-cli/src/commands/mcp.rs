//! `am mcp [serve]|install|uninstall|status`: run the MCP server, or register
//! it with the agent CLIs and MCP-only apps (`am_core::mcp_installer`).

use am_core::harness::{self, HarnessKind};
use am_core::mcp_installer::{self as mi, Target};
use am_core::settings;

use crate::{fail, output};

fn valid_ids() -> String {
    harness::ids().into_iter().chain([mi::DESKTOP_ID]).collect::<Vec<_>>().join(", ")
}

/// `--agent` values, or `default` when none were given. An explicit agent
/// that can't take MCP servers is an error.
fn selected(ids: &[String], default: Vec<Target>) -> Vec<Target> {
    if ids.is_empty() {
        return default.into_iter().filter(|t| t.config().is_some()).collect();
    }
    ids.iter()
        .map(|id| {
            let t = Target::get(id).unwrap_or_else(|| fail(&format!("Unknown agent: {id}. Valid agents: {}", valid_ids())));
            if t.config().is_none() {
                fail(&format!("am can't register its MCP server with {} yet", t.name()));
            }
            t
        })
        .collect()
}

/// Default: every enabled agent CLI found on this machine that reads MCP
/// servers, plus Claude Desktop when it is installed.
pub fn install(ids: &[String]) {
    let s = settings::load();
    let detected = harness::detected(&s).into_iter().map(Target::Agent);
    let default = detected.chain(Target::Desktop.is_present().then_some(Target::Desktop)).collect();
    let mut failed = false;
    for t in selected(ids, default) {
        match mi::install_mcp(t) {
            Ok(via) => {
                println!("MCP server registered with {} ({via})", t.name());
                if t == Target::Desktop {
                    println!("  Quit Claude Desktop fully (⌘Q) and reopen it to load it.");
                }
            }
            Err(e) => {
                eprintln!("Failed to register the MCP server with {}: {e}", t.name());
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}

/// Default: every agent that has it registered.
pub fn uninstall(ids: &[String]) {
    let explicit = !ids.is_empty();
    let mut failed = false;
    for t in selected(ids, Target::all()) {
        if !explicit && !mi::is_mcp_installed(t) {
            continue;
        }
        match mi::uninstall_mcp(t) {
            Ok(via) => println!("MCP server removed from {} ({via})", t.name()),
            Err(e) => {
                eprintln!("Failed to remove the MCP server from {}: {e}", t.name());
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}

pub fn status(json: bool) {
    let rows: Vec<(Target, Option<bool>)> =
        Target::all().into_iter().map(|t| (t, t.config().map(|_| mi::is_mcp_installed(t)))).collect();
    if json {
        let agents: serde_json::Map<String, serde_json::Value> = rows
            .iter()
            .map(|(t, on)| {
                let v = match on {
                    Some(on) => serde_json::json!({
                        "installed": on,
                        "config": mi::display_path(*t),
                        "command": mi::installed_program(*t),
                    }),
                    None => serde_json::json!({"installed": false, "supported": false}),
                };
                (t.id().to_string(), v)
            })
            .collect();
        output::json(&serde_json::json!({ "server": mi::SERVER_NAME, "agents": agents }));
        return;
    }
    for (t, on) in rows {
        // Like `hooks status`: Claude always, others once present or registered.
        if t != Target::Agent(HarnessKind::CLAUDE) && on != Some(true) && !t.is_present() {
            continue;
        }
        let state = match on {
            Some(true) => format!("registered ({})", mi::display_path(t).unwrap_or_default()),
            Some(false) => "not registered".into(),
            None => "not supported".into(),
        };
        println!("{} MCP server: {state}", t.short_name());
    }
}
