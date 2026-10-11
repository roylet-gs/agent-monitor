//! First-run setup (shown when settings.json has no `setupCompleted`).
//! Port of the TS SetupWizard, trimmed to what a first run needs: editor,
//! Claude hooks, repositories, integrations. Everything is also reachable
//! later via `s` (settings), `A` (add repo) and `am hooks install`.

use serde_json::Value;

use crate::app::Key;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Editor,
    Hooks,
    Repos,
    Integrations,
}

#[derive(Debug, Clone)]
pub struct Setup {
    pub step: Step,
    pub ide: usize,
    pub hooks_installed: bool,
    pub hook_binary: Option<String>,
    pub repo_input: String,
    pub repos_added: Vec<String>,
    pub repo_error: Option<String>,
    pub gh: bool,
    pub linear_key: String,
    pub integration_field: usize,
}

pub const IDES: [(&str, &str); 3] = [("cursor", "Cursor"), ("vscode", "VS Code"), ("terminal", "Terminal")];

pub enum SetupEffect {
    None,
    InstallHooks,
    AddRepo(String),
    /// Finished: these keys go into settings.json (plus setupCompleted).
    Finish(Vec<(&'static str, Value)>),
}

impl Setup {
    pub fn new(existing_repos: Vec<String>, ide: &str) -> Setup {
        Setup {
            step: Step::Editor,
            ide: IDES.iter().position(|(k, _)| *k == ide).unwrap_or(0),
            hooks_installed: am_core::hooks_installer::is_global_hooks_installed(),
            hook_binary: am_core::hooks_installer::find_native_hook_binary().map(|p| p.display().to_string()),
            repo_input: "~/".into(),
            repos_added: existing_repos,
            repo_error: None,
            gh: which("gh"),
            linear_key: String::new(),
            integration_field: 0,
        }
    }

    pub fn on_key(&mut self, key: Key) -> SetupEffect {
        match self.step {
            Step::Editor => match key {
                Key::Left | Key::Up | Key::Char('k') | Key::Char('h') => self.ide = (self.ide + IDES.len() - 1) % IDES.len(),
                Key::Right | Key::Down | Key::Char('j') | Key::Char('l') | Key::Tab => self.ide = (self.ide + 1) % IDES.len(),
                Key::Enter => self.step = Step::Hooks,
                _ => {}
            },
            Step::Hooks => match key {
                Key::Enter | Key::Char('y') if !self.hooks_installed => return SetupEffect::InstallHooks,
                Key::Enter | Key::Char('n') => self.step = Step::Repos,
                Key::Esc => self.step = Step::Editor,
                _ => {}
            },
            Step::Repos => {
                self.repo_error = None;
                match key {
                    Key::Tab => self.repo_input = crate::modals::complete_dir(&self.repo_input),
                    Key::Backspace => {
                        self.repo_input.pop();
                    }
                    Key::Char(c) => self.repo_input.push(c),
                    Key::Esc => self.step = Step::Hooks,
                    Key::Enter => {
                        let p = self.repo_input.trim().to_string();
                        if p.is_empty() || p == "~/" || p == "~" {
                            self.step = Step::Integrations;
                        } else {
                            return SetupEffect::AddRepo(p);
                        }
                    }
                    _ => {}
                }
            }
            Step::Integrations => match key {
                Key::Esc => self.step = Step::Repos,
                Key::Tab | Key::Down | Key::Up => self.integration_field ^= 1,
                Key::Char(' ') if self.integration_field == 0 => self.gh = !self.gh,
                Key::Backspace if self.integration_field == 1 => {
                    self.linear_key.pop();
                }
                Key::Char(c) if self.integration_field == 1 => self.linear_key.push(c),
                Key::Enter => {
                    let key = self.linear_key.trim().to_string();
                    return SetupEffect::Finish(vec![
                        ("ide", Value::from(IDES[self.ide].0)),
                        ("ghPrStatus", Value::Bool(self.gh)),
                        ("linearEnabled", Value::Bool(!key.is_empty())),
                        ("linearApiKey", Value::from(key)),
                        ("setupCompleted", Value::Bool(true)),
                    ]);
                }
                _ => {}
            },
        }
        SetupEffect::None
    }

    pub fn on_hooks_installed(&mut self, result: Result<Option<String>, String>) {
        match result {
            Ok(bin) => {
                self.hooks_installed = true;
                self.hook_binary = bin.or(self.hook_binary.take());
                self.step = Step::Repos;
            }
            Err(e) => self.repo_error = Some(format!("Couldn't install hooks: {e}")),
        }
    }

    pub fn on_repo_added(&mut self, path: String, result: Result<String, String>) {
        match result {
            Ok(name) => {
                self.repos_added.push(name);
                self.repo_input = "~/".into();
            }
            Err(e) => {
                self.repo_input = path;
                self.repo_error = Some(e);
            }
        }
    }
}

fn which(cmd: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
}

/// True when this is a first run (the TS app writes `setupCompleted: true`).
pub fn needed(settings: &am_core::settings::Settings) -> bool {
    !settings.raw.get("setupCompleted").and_then(Value::as_bool).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Setup {
        Setup {
            step: Step::Editor,
            ide: 0,
            hooks_installed: false,
            hook_binary: None,
            repo_input: "~/".into(),
            repos_added: vec![],
            repo_error: None,
            gh: true,
            linear_key: String::new(),
            integration_field: 0,
        }
    }

    #[test]
    fn walks_through_all_steps() {
        let mut s = setup();
        s.on_key(Key::Right);
        s.on_key(Key::Enter);
        assert_eq!(s.step, Step::Hooks);
        assert!(matches!(s.on_key(Key::Enter), SetupEffect::InstallHooks));
        s.on_hooks_installed(Ok(Some("/x/am-hook".into())));
        assert_eq!(s.step, Step::Repos);
        for _ in 0..2 {
            s.on_key(Key::Backspace);
        }
        for c in "/r".chars() {
            s.on_key(Key::Char(c));
        }
        assert!(matches!(s.on_key(Key::Enter), SetupEffect::AddRepo(p) if p == "/r"));
        s.on_repo_added("/r".into(), Ok("r".into()));
        assert_eq!(s.repos_added, vec!["r".to_string()]);
        s.on_key(Key::Enter); // empty (~/) → next
        assert_eq!(s.step, Step::Integrations);
        s.on_key(Key::Tab);
        for c in "lin_key".chars() {
            s.on_key(Key::Char(c));
        }
        let SetupEffect::Finish(v) = s.on_key(Key::Enter) else { panic!() };
        assert!(v.contains(&("ide", Value::from("vscode"))));
        assert!(v.contains(&("linearEnabled", Value::Bool(true))));
        assert!(v.contains(&("setupCompleted", Value::Bool(true))));
    }

    #[test]
    fn needed_only_without_setup_completed() {
        assert!(needed(&am_core::settings::Settings::parse("{}")));
        assert!(!needed(&am_core::settings::Settings::parse(r#"{"setupCompleted":true}"#)));
    }
}
