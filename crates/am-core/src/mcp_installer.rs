//! Registers am's MCP server (`am mcp`, a stdio server exposing the CLI as
//! tools) with the agent CLIs that read MCP servers from a JSON file
//! ([`Harness::mcp_config`](crate::harness::Harness::mcp_config)). The entry
//! is `"agent-monitor": {"command": "<abs path>/am", "args": ["mcp"]}` under the
//! file's server map; everyone else's servers are kept. A CLI that rewrites its
//! config while it runs (Claude Code's `~/.claude.json`) is driven through its
//! own `mcp add`/`mcp remove` commands when it is installed.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Map, Value, json};

use crate::harness::{HarnessKind, McpConfig};
use crate::hooks_installer::{find_installed_binary, is_executable, read_settings, tilde, write_settings};
use crate::info;

/// The server's name in every agent's config.
pub const SERVER_NAME: &str = "agent-monitor";

/// The `am` an MCP entry runs: an installed one by absolute path, else bare
/// `am` for the agent CLI to find on its `$PATH`.
pub fn am_program() -> PathBuf {
    find_installed_binary("am").unwrap_or_else(|| PathBuf::from("am"))
}

pub fn server_entry(cfg: &McpConfig, program: &Path) -> Value {
    let mut e = cfg.entry.clone();
    e.insert("command".into(), program.display().to_string().into());
    e.insert("args".into(), json!(["mcp"]));
    Value::Object(e)
}

fn unsupported(kind: HarnessKind) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!("am can't register MCP servers with {} yet", kind.provider().name()),
    )
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| is_executable(&d.join(program))))
}

fn run(argv: &[String]) -> std::io::Result<()> {
    let out = Command::new(&argv[0]).args(&argv[1..]).stdin(Stdio::null()).output()?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let err = if err.trim().is_empty() { String::from_utf8_lossy(&out.stdout) } else { err };
    Err(std::io::Error::other(format!("`{}` failed: {}", argv[..3.min(argv.len())].join(" "), err.trim())))
}

/// The agent CLI's own command when it should be used: given and on `$PATH`.
fn via_cli(argv: Option<Vec<String>>) -> Option<Vec<String>> {
    argv.filter(|a| a.first().is_some_and(|p| on_path(p)))
}

/// Register am's server with `kind`. Returns where it went, for display
/// (`~/.gemini/settings.json`, or `claude mcp add-json`).
pub fn install_mcp(kind: HarnessKind) -> std::io::Result<String> {
    let p = kind.provider();
    let cfg = p.mcp_config().ok_or_else(|| unsupported(kind))?;
    let program = am_program();
    let entry = server_entry(&cfg, &program);
    if let Some(add) = via_cli(p.mcp_add_command(SERVER_NAME, &entry)) {
        // `add` refuses an existing name, so replace it.
        if is_installed_in(&cfg)
            && let Some(remove) = p.mcp_remove_command(SERVER_NAME)
        {
            run(&remove)?;
        }
        run(&add)?;
        info!("mcp", "Registered MCP server with {} via `{}` using {}", p.name(), add[..3].join(" "), program.display());
        return Ok(add[..3].join(" "));
    }
    install_in(&cfg, entry)?;
    info!("mcp", "Registered MCP server with {} in {} using {}", p.name(), cfg.path.display(), program.display());
    Ok(tilde(&cfg.path))
}

/// Remove am's server from `kind`'s config; a no-op when it isn't there.
pub fn uninstall_mcp(kind: HarnessKind) -> std::io::Result<String> {
    let p = kind.provider();
    let cfg = p.mcp_config().ok_or_else(|| unsupported(kind))?;
    if !is_installed_in(&cfg) {
        return Ok(tilde(&cfg.path));
    }
    if let Some(remove) = via_cli(p.mcp_remove_command(SERVER_NAME)) {
        run(&remove)?;
        info!("mcp", "Removed MCP server from {} via `{}`", p.name(), remove[..3].join(" "));
        return Ok(remove[..3].join(" "));
    }
    uninstall_in(&cfg)?;
    info!("mcp", "Removed MCP server from {} ({})", p.name(), cfg.path.display());
    Ok(tilde(&cfg.path))
}

pub fn is_mcp_installed(kind: HarnessKind) -> bool {
    kind.provider().mcp_config().is_some_and(|c| is_installed_in(&c))
}

/// The program `kind`'s registered entry runs, if registered.
pub fn installed_program(kind: HarnessKind) -> Option<String> {
    let cfg = kind.provider().mcp_config()?;
    servers(&read_settings(&cfg.path).ok()?, &cfg.key).get(SERVER_NAME)?.get("command")?.as_str().map(String::from)
}

/// Display form of where `kind` keeps MCP servers.
pub fn display_path(kind: HarnessKind) -> Option<String> {
    kind.provider().mcp_config().map(|c| tilde(&c.path))
}

fn servers(settings: &Map<String, Value>, key: &str) -> Map<String, Value> {
    match settings.get(key) {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    }
}

fn is_installed_in(cfg: &McpConfig) -> bool {
    read_settings(&cfg.path).is_ok_and(|s| servers(&s, &cfg.key).contains_key(SERVER_NAME))
}

fn install_in(cfg: &McpConfig, entry: Value) -> std::io::Result<()> {
    let mut settings = read_settings(&cfg.path)?;
    let mut map = servers(&settings, &cfg.key);
    map.insert(SERVER_NAME.into(), entry);
    settings.insert(cfg.key.clone(), Value::Object(map));
    write_settings(&cfg.path, &settings)
}

fn uninstall_in(cfg: &McpConfig) -> std::io::Result<()> {
    let mut settings = read_settings(&cfg.path)?;
    let mut map = servers(&settings, &cfg.key);
    map.shift_remove(SERVER_NAME);
    if map.is_empty() {
        settings.shift_remove(&cfg.key);
    } else {
        settings.insert(cfg.key.clone(), Value::Object(map));
    }
    write_settings(&cfg.path, &settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::BUILTIN;

    fn cfg(path: PathBuf) -> McpConfig {
        McpConfig { path, key: "mcpServers".into(), entry: json!({"type": "stdio"}).as_object().cloned().unwrap() }
    }

    #[test]
    fn install_keeps_other_servers_and_uninstall_restores_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let c = cfg(dir.path().join("settings.json"));
        let before = json!({"theme": "dark", "mcpServers": {"other": {"command": "x"}}});
        write_settings(&c.path, before.as_object().unwrap()).unwrap();

        install_in(&c, server_entry(&c, Path::new("/opt/bin/am"))).unwrap();
        assert!(is_installed_in(&c));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&c.path).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["mcpServers"][SERVER_NAME], json!({"type": "stdio", "command": "/opt/bin/am", "args": ["mcp"]}));
        assert_eq!(v["theme"], "dark");

        uninstall_in(&c).unwrap();
        assert!(!is_installed_in(&c));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&c.path).unwrap()).unwrap();
        assert_eq!(v, before);
    }

    #[test]
    fn a_fresh_file_is_created_and_emptied_maps_go() {
        let dir = tempfile::tempdir().unwrap();
        let c = cfg(dir.path().join("sub/mcp.json"));
        install_in(&c, server_entry(&c, Path::new("am"))).unwrap();
        assert!(is_installed_in(&c));
        uninstall_in(&c).unwrap();
        assert_eq!(std::fs::read_to_string(&c.path).unwrap(), "{}\n");
    }

    #[test]
    fn a_file_with_comments_is_never_touched() {
        let dir = tempfile::tempdir().unwrap();
        let c = cfg(dir.path().join("settings.json"));
        std::fs::write(&c.path, "{ // mine\n}").unwrap();
        assert!(install_in(&c, json!({})).is_err());
        assert_eq!(std::fs::read_to_string(&c.path).unwrap(), "{ // mine\n}");
    }

    #[test]
    fn builtins_that_read_mcp_servers_say_where() {
        let with: Vec<&str> = BUILTIN.iter().filter(|k| k.provider().mcp_config().is_some()).map(|k| k.id()).collect();
        assert_eq!(with, ["claude", "gemini", "qwen", "cursor"]);
        let add = HarnessKind::CLAUDE.provider().mcp_add_command(SERVER_NAME, &json!({"command": "am"})).unwrap();
        assert_eq!(add, ["claude", "mcp", "add-json", "--scope", "user", SERVER_NAME, r#"{"command":"am"}"#]);
    }
}
