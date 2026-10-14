//! `am mcp [serve]|install|uninstall|status`: run the MCP server, or register
//! it with the agent CLIs (`am_core::mcp_installer`).

use am_core::harness::{self, HarnessKind};
use am_core::mcp_installer as mi;
use am_core::settings;

use crate::{fail, output};

/// `--agent` values, or `default` when none were given. An explicit agent
/// that can't take MCP servers is an error.
fn selected(ids: &[String], default: Vec<HarnessKind>) -> Vec<HarnessKind> {
    if ids.is_empty() {
        return default.into_iter().filter(|k| k.provider().mcp_config().is_some()).collect();
    }
    ids.iter()
        .map(|id| {
            let kind = HarnessKind::get(id)
                .unwrap_or_else(|| fail(&format!("Unknown agent: {id}. Valid agents: {}", harness::ids().join(", "))));
            if kind.provider().mcp_config().is_none() {
                fail(&format!("am can't register its MCP server with {} yet", kind.provider().name()));
            }
            kind
        })
        .collect()
}

/// Default: every enabled agent CLI found on this machine that reads MCP servers.
pub fn install(ids: &[String]) {
    let s = settings::load();
    let mut failed = false;
    for kind in selected(ids, harness::detected(&s)) {
        match mi::install_mcp(kind) {
            Ok(via) => println!("MCP server registered with {} ({via})", kind.provider().name()),
            Err(e) => {
                eprintln!("Failed to register the MCP server with {}: {e}", kind.provider().name());
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
    for kind in selected(ids, harness::all().to_vec()) {
        if !explicit && !mi::is_mcp_installed(kind) {
            continue;
        }
        match mi::uninstall_mcp(kind) {
            Ok(via) => println!("MCP server removed from {} ({via})", kind.provider().name()),
            Err(e) => {
                eprintln!("Failed to remove the MCP server from {}: {e}", kind.provider().name());
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}

pub fn status(json: bool) {
    let rows: Vec<(HarnessKind, Option<bool>)> = harness::all()
        .iter()
        .map(|k| (*k, k.provider().mcp_config().map(|_| mi::is_mcp_installed(*k))))
        .collect();
    if json {
        let agents: serde_json::Map<String, serde_json::Value> = rows
            .iter()
            .map(|(k, on)| {
                let v = match on {
                    Some(on) => serde_json::json!({
                        "installed": on,
                        "config": mi::display_path(*k),
                        "command": mi::installed_program(*k),
                    }),
                    None => serde_json::json!({"installed": false, "supported": false}),
                };
                (k.id().to_string(), v)
            })
            .collect();
        output::json(&serde_json::json!({ "server": mi::SERVER_NAME, "agents": agents }));
        return;
    }
    for (kind, on) in rows {
        let p = kind.provider();
        // Like `hooks status`: Claude always, others once present or registered.
        if kind != HarnessKind::CLAUDE && on != Some(true) && !p.is_present() {
            continue;
        }
        let state = match on {
            Some(true) => format!("registered ({})", mi::display_path(kind).unwrap_or_default()),
            Some(false) => "not registered".into(),
            None => "not supported".into(),
        };
        println!("{} MCP server: {state}", p.short_name());
    }
}
