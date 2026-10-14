//! Registers am's MCP server (`am mcp`, a stdio server exposing the CLI as
//! tools) with the agent CLIs that read MCP servers from a JSON file
//! ([`Harness::mcp_config`](crate::harness::Harness::mcp_config)). The entry
//! is `"agent-monitor": {"command": "<abs path>/am", "args": ["mcp"]}` under the
//! file's server map; everyone else's servers are kept. A CLI that rewrites its
//! config while it runs (Claude Code's `~/.claude.json`) is driven through its
//! own `mcp add`/`mcp remove` commands when it is installed.
//!
//! Besides the harness plugins there are MCP-only apps ([`Target::Desktop`],
//! Claude Desktop): they read MCP servers but run no hooks, so they are not
//! harnesses and live here instead.

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

/// Something `am mcp install` can register with: a harness plugin, or an
/// app that only reads MCP servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Agent(HarnessKind),
    /// Claude Desktop (`claude_desktop_config.json`). It has no CLI to drive,
    /// starts servers in `/` and with a minimal `$PATH`, so its entry carries
    /// the installing shell's `PATH` for the `git`/`gh` that am runs.
    Desktop,
}

pub const DESKTOP_ID: &str = "claude-desktop";

/// Claude Desktop's config file (macOS; the Linux path community builds use).
pub fn desktop_config_path() -> PathBuf {
    let rel = if cfg!(target_os = "macos") {
        "Library/Application Support/Claude/claude_desktop_config.json"
    } else {
        ".config/Claude/claude_desktop_config.json"
    };
    crate::paths::home_dir().join(rel)
}

impl Target {
    /// Every target: the registered harnesses, then the MCP-only apps.
    pub fn all() -> Vec<Target> {
        crate::harness::all().iter().map(|k| Target::Agent(*k)).chain([Target::Desktop]).collect()
    }

    /// By `--agent` id (a harness id or `claude-desktop`).
    pub fn get(id: &str) -> Option<Target> {
        if id == DESKTOP_ID {
            return Some(Target::Desktop);
        }
        HarnessKind::get(id).map(Target::Agent)
    }

    pub fn id(self) -> &'static str {
        match self {
            Target::Agent(k) => k.id(),
            Target::Desktop => DESKTOP_ID,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Target::Agent(k) => k.provider().name(),
            Target::Desktop => "Claude Desktop",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Target::Agent(k) => k.provider().short_name(),
            Target::Desktop => "Claude Desktop",
        }
    }

    /// Installed on this machine (its config dir exists).
    pub fn is_present(self) -> bool {
        match self {
            Target::Agent(k) => k.provider().is_present(),
            Target::Desktop => desktop_config_path().parent().is_some_and(Path::is_dir),
        }
    }

    pub fn config(self) -> Option<McpConfig> {
        match self {
            Target::Agent(k) => k.provider().mcp_config(),
            Target::Desktop => {
                let mut entry = Map::new();
                if let Some(path) = std::env::var_os("PATH") {
                    entry.insert("env".into(), json!({"PATH": path.to_string_lossy()}));
                }
                Some(McpConfig { path: desktop_config_path(), key: "mcpServers".into(), entry })
            }
        }
    }

    fn add_command(self, entry: &Value) -> Option<Vec<String>> {
        match self {
            Target::Agent(k) => k.provider().mcp_add_command(SERVER_NAME, entry),
            Target::Desktop => None,
        }
    }

    fn remove_command(self) -> Option<Vec<String>> {
        match self {
            Target::Agent(k) => k.provider().mcp_remove_command(SERVER_NAME),
            Target::Desktop => None,
        }
    }
}

fn unsupported(target: Target) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!("am can't register MCP servers with {} yet", target.name()),
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

/// Register am's server with `target`. Returns where it went, for display
/// (`~/.gemini/settings.json`, or `claude mcp add-json`).
pub fn install_mcp(target: Target) -> std::io::Result<String> {
    let cfg = target.config().ok_or_else(|| unsupported(target))?;
    let program = am_program();
    let entry = server_entry(&cfg, &program);
    if let Some(add) = via_cli(target.add_command(&entry)) {
        // `add` refuses an existing name, so replace it.
        if is_installed_in(&cfg)
            && let Some(remove) = target.remove_command()
        {
            run(&remove)?;
        }
        run(&add)?;
        info!("mcp", "Registered MCP server with {} via `{}` using {}", target.name(), add[..3].join(" "), program.display());
        return Ok(add[..3].join(" "));
    }
    install_in(&cfg, entry)?;
    info!("mcp", "Registered MCP server with {} in {} using {}", target.name(), cfg.path.display(), program.display());
    Ok(tilde(&cfg.path))
}

/// Remove am's server from `target`'s config; a no-op when it isn't there.
pub fn uninstall_mcp(target: Target) -> std::io::Result<String> {
    let cfg = target.config().ok_or_else(|| unsupported(target))?;
    if !is_installed_in(&cfg) {
        return Ok(tilde(&cfg.path));
    }
    if let Some(remove) = via_cli(target.remove_command()) {
        run(&remove)?;
        info!("mcp", "Removed MCP server from {} via `{}`", target.name(), remove[..3].join(" "));
        return Ok(remove[..3].join(" "));
    }
    uninstall_in(&cfg)?;
    info!("mcp", "Removed MCP server from {} ({})", target.name(), cfg.path.display());
    Ok(tilde(&cfg.path))
}

pub fn is_mcp_installed(target: Target) -> bool {
    target.config().is_some_and(|c| is_installed_in(&c))
}

/// The program `target`'s registered entry runs, if registered.
pub fn installed_program(target: Target) -> Option<String> {
    let cfg = target.config()?;
    servers(&read_settings(&cfg.path).ok()?, &cfg.key).get(SERVER_NAME)?.get("command")?.as_str().map(String::from)
}

/// Display form of where `target` keeps MCP servers.
pub fn display_path(target: Target) -> Option<String> {
    target.config().map(|c| tilde(&c.path))
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

    #[test]
    fn claude_desktop_is_a_target_edited_in_place() {
        assert_eq!(Target::get(DESKTOP_ID), Some(Target::Desktop));
        assert_eq!(Target::get("claude"), Some(Target::Agent(HarnessKind::CLAUDE)));
        assert_eq!(Target::all().last(), Some(&Target::Desktop));
        let cfg = Target::Desktop.config().unwrap();
        assert!(cfg.path.ends_with("Claude/claude_desktop_config.json"), "{}", cfg.path.display());
        assert_eq!(cfg.key, "mcpServers");
        assert!(Target::Desktop.add_command(&json!({})).is_none());
    }
}
