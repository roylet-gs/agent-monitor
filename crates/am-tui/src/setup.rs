//! First-run setup (shown when settings.json has no `setupCompleted`).
//! Port of the TS SetupWizard, trimmed to what a first run needs: editor,
//! agent hooks (Claude Code plus any other agent CLI found), repositories,
//! integrations, review requests (when GitHub is on), context window. Everything is also reachable
//! later via `s` (settings), `A` (add repo) and `am hooks install`.
//! Settings → Run setup again (or `am setup`) walks it again over the current
//! settings: it starts from their values, and Esc on the first step cancels.

use am_core::harness::{self, HarnessKind};
use serde_json::Value;

use crate::app::Key;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Editor,
    Hooks,
    Repos,
    Integrations,
    /// What counts as a review request (skipped with GitHub off).
    Reviews,
    Context,
}

/// The Reviews step's choices: `reviewRequests*`, and the review groups
/// they imply ([`ReviewSetup::groups`]): who's asked becomes an `Asked you`
/// and/or `Asked your team` group, and the two filters become every such
/// group's (and the default group's) `draft` and `reviewed` rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewSetup {
    pub enabled: bool,
    /// Index into `ReviewScope::IDS`.
    pub scope: usize,
    pub hide_drafts: bool,
    pub hide_reviewed: bool,
    /// The default review group the filters are written into (its other
    /// rules kept).
    pub default_group: am_core::review_groups::ReviewGroup,
    /// The review groups already saved; replaced only while they're none or
    /// exactly what this step makes ([`ReviewSetup::groups`]).
    pub existing_groups: Vec<am_core::review_groups::ReviewGroup>,
    /// Row under the cursor: see [`REVIEW_ROWS`].
    pub field: usize,
}

/// Rows of the Reviews step: the switch, the three scopes, two filters.
pub const REVIEW_ROWS: usize = 6;

impl ReviewSetup {
    fn from_settings(s: &am_core::settings::Settings) -> ReviewSetup {
        ReviewSetup {
            enabled: s.review_requests_enabled,
            scope: am_core::settings::ReviewScope::IDS.iter().position(|id| *id == s.review_scope().id()).unwrap_or(0),
            hide_drafts: s.review_default_group.draft == "ready",
            hide_reviewed: s.review_default_group.reviewed == "no",
            default_group: s.review_default_group.clone(),
            existing_groups: s.review_groups.clone(),
            field: 0,
        }
    }

    /// The default review group with these filters applied.
    pub fn group(&self) -> am_core::review_groups::ReviewGroup {
        let mut g = self.default_group.clone();
        apply_filters(&mut g, self.hide_drafts, self.hide_reviewed);
        g
    }

    /// The groups this step writes: one per kind of request the scope
    /// fetches (`Asked you` by name, `Asked your team`), each with the two
    /// filters. `None` keeps groups the user has made or changed.
    pub fn groups(&self) -> Option<Vec<am_core::review_groups::ReviewGroup>> {
        let mine = |g: &[am_core::review_groups::ReviewGroup]| {
            (0..3).any(|scope| [false, true].iter().any(|&d| [false, true].iter().any(|&r| made(scope, d, r) == g)))
        };
        (self.existing_groups.is_empty() || mine(&self.existing_groups))
            .then(|| made(self.scope, self.hide_drafts, self.hide_reviewed))
    }

    /// Space (or a second click) on the row under the cursor.
    pub fn toggle(&mut self) {
        match self.field {
            0 => self.enabled = !self.enabled,
            1..=3 => self.scope = self.field - 1,
            4 => self.hide_drafts = !self.hide_drafts,
            _ => self.hide_reviewed = !self.hide_reviewed,
        }
    }
}

/// Set the two setup filters on `g`, leaving any other choice of the rule.
fn apply_filters(g: &mut am_core::review_groups::ReviewGroup, hide_drafts: bool, hide_reviewed: bool) {
    g.draft = if hide_drafts { "ready" } else if g.draft == "ready" { "any" } else { g.draft.as_str() }.to_string();
    g.reviewed = if hide_reviewed { "no" } else if g.reviewed == "no" { "any" } else { g.reviewed.as_str() }.to_string();
}

/// The groups the Reviews step makes for `scope` (index into
/// `ReviewScope::IDS`: all, direct, teams) and the two filters.
fn made(scope: usize, hide_drafts: bool, hide_reviewed: bool) -> Vec<am_core::review_groups::ReviewGroup> {
    use am_core::review_groups::ReviewGroup;
    let group = |name: &str, asked: &str| {
        let mut g = ReviewGroup { name: name.into(), asked: asked.into(), ..ReviewGroup::default() };
        apply_filters(&mut g, hide_drafts, hide_reviewed);
        g
    };
    let you = group("Asked you", "you");
    let team = group("Asked your team", "team");
    match scope {
        1 => vec![you],
        2 => vec![team],
        _ => vec![you, team],
    }
}

impl Default for ReviewSetup {
    fn default() -> Self {
        ReviewSetup::from_settings(&am_core::settings::Settings::default())
    }
}

#[derive(Debug, Clone)]
pub struct Setup {
    pub step: Step,
    /// Index into `IDES`; the last one (`custom`) takes a typed command.
    pub ide: usize,
    pub ide_command: String,
    pub ide_error: Option<String>,
    /// True once every agent in `agents` has hooks.
    pub hooks_installed: bool,
    /// Agent CLIs the hooks step installs for, and whether each has hooks.
    pub agents: Vec<(HarnessKind, bool)>,
    pub hook_binary: Option<String>,
    pub repo_input: String,
    pub repos_added: Vec<String>,
    pub repo_error: Option<String>,
    pub gh: bool,
    pub linear_key: String,
    pub integration_field: usize,
    pub review: ReviewSetup,
    /// Index into `CONTEXT_CHOICES`; the last one is a typed custom value.
    pub ctx_choice: usize,
    pub ctx_input: String,
    pub ctx_error: Option<String>,
    /// A re-run (setup was done before): Esc on the first step leaves
    /// without changing anything.
    pub cancellable: bool,
}

pub const IDES: [(&str, &str); 6] = am_core::launch::IdeChoice::ALL;
pub const IDE_CUSTOM: usize = IDES.len() - 1;

/// Context-window presets offered by the wizard; a third row takes a typed value.
pub const CONTEXT_CHOICES: [(u64, &str); 2] = [(1_000_000, "1M (default)"), (200_000, "200k")];
pub const CONTEXT_CUSTOM: usize = CONTEXT_CHOICES.len();

pub enum SetupEffect {
    None,
    InstallHooks,
    AddRepo(String),
    /// Esc on the first step of a re-run: close, nothing saved.
    Cancel,
    /// Finished: these keys go into settings.json (plus setupCompleted).
    Finish(Vec<(&'static str, Value)>),
}

impl Setup {
    pub fn new(existing_repos: Vec<String>, settings: &am_core::settings::Settings) -> Setup {
        let ide = settings.ide.as_str();
        let agents: Vec<(HarnessKind, bool)> =
            harness::detected(settings).into_iter().map(|k| (k, am_core::hooks_installer::is_hooks_installed(k))).collect();
        let hooks_installed = agents.iter().all(|(_, on)| *on);
        let context_window = settings.context_window_tokens;
        let preset = CONTEXT_CHOICES.iter().position(|(n, _)| *n == context_window);
        // A re-run starts from what's saved, so finishing it keeps the Linear
        // key and the PR setting; a first run guesses from what's installed.
        let rerun = !needed(settings);
        Setup {
            step: Step::Editor,
            ide: IDES.iter().position(|(k, _)| *k == ide).unwrap_or(0),
            ide_command: settings.ide_command.clone(),
            ide_error: None,
            agents,
            hooks_installed,
            hook_binary: am_core::hooks_installer::find_native_hook_binary().map(|p| p.display().to_string()),
            repo_input: "~/".into(),
            repos_added: existing_repos,
            repo_error: None,
            gh: if rerun { settings.gh_pr_status } else { which("gh") },
            linear_key: if rerun { settings.linear_api_key.clone() } else { String::new() },
            integration_field: 0,
            review: ReviewSetup::from_settings(settings),
            ctx_choice: preset.unwrap_or(CONTEXT_CUSTOM),
            ctx_input: if preset.is_some() { String::new() } else { am_core::context::short_tokens(context_window) },
            ctx_error: None,
            cancellable: rerun,
        }
    }

    pub fn on_key(&mut self, key: Key) -> SetupEffect {
        match self.step {
            Step::Editor => {
                self.ide_error = None;
                let custom = self.ide == IDE_CUSTOM;
                let up = |i: usize| (i + IDES.len() - 1) % IDES.len();
                let down = |i: usize| (i + 1) % IDES.len();
                match key {
                    Key::Up => self.ide = up(self.ide),
                    Key::Down | Key::Tab => self.ide = down(self.ide),
                    // On the custom row letters are the command, so only arrows move.
                    Key::Left | Key::Char('k') | Key::Char('h') if !custom => self.ide = up(self.ide),
                    Key::Right | Key::Char('j') | Key::Char('l') if !custom => self.ide = down(self.ide),
                    Key::Backspace if custom => {
                        self.ide_command.pop();
                    }
                    Key::Char(c) if custom => self.ide_command.push(c),
                    Key::Enter if custom && self.ide_command.trim().is_empty() => {
                        self.ide_error = Some("Type the command that opens a folder, e.g. idea or subl -n".into());
                    }
                    Key::Enter => self.step = Step::Hooks,
                    Key::Esc if self.cancellable => return SetupEffect::Cancel,
                    _ => {}
                }
            }
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
                Key::Enter => self.step = if self.gh { Step::Reviews } else { Step::Context },
                _ => {}
            },
            Step::Reviews => {
                let r = &mut self.review;
                match key {
                    Key::Esc => self.step = Step::Integrations,
                    Key::Up | Key::BackTab | Key::Char('k') => r.field = (r.field + REVIEW_ROWS - 1) % REVIEW_ROWS,
                    Key::Down | Key::Tab | Key::Char('j') => r.field = (r.field + 1) % REVIEW_ROWS,
                    Key::Char(' ') => r.toggle(),
                    Key::Enter => self.step = Step::Context,
                    _ => {}
                }
            }
            Step::Context => {
                self.ctx_error = None;
                let custom = self.ctx_choice == CONTEXT_CUSTOM;
                match key {
                    Key::Esc => self.step = if self.gh { Step::Reviews } else { Step::Integrations },
                    Key::Up => self.ctx_choice = (self.ctx_choice + CONTEXT_CUSTOM) % (CONTEXT_CUSTOM + 1),
                    Key::Down | Key::Tab => self.ctx_choice = (self.ctx_choice + 1) % (CONTEXT_CUSTOM + 1),
                    // On the custom row letters are input (`128k`, `1m`), so only arrows move.
                    Key::Char('k') if !custom => self.ctx_choice = (self.ctx_choice + CONTEXT_CUSTOM) % (CONTEXT_CUSTOM + 1),
                    Key::Char('j') if !custom => self.ctx_choice = (self.ctx_choice + 1) % (CONTEXT_CUSTOM + 1),
                    Key::Backspace if custom => {
                        self.ctx_input.pop();
                    }
                    Key::Char(c) if custom => self.ctx_input.push(c),
                    Key::Enter => {
                        let tokens = match CONTEXT_CHOICES.get(self.ctx_choice) {
                            Some((n, _)) => *n,
                            None => match am_core::context::parse_tokens(&self.ctx_input) {
                                Some(n) if n >= am_core::context::MIN_WINDOW => n,
                                _ => {
                                    self.ctx_error = Some("Enter a token count ≥ 1k, e.g. 128k, 200k, 1m".into());
                                    return SetupEffect::None;
                                }
                            },
                        };
                        let key = self.linear_key.trim().to_string();
                        let mut keys = vec![("ide", Value::from(IDES[self.ide].0))];
                        if self.ide == IDE_CUSTOM {
                            keys.push(("ideCommand", Value::from(self.ide_command.trim())));
                        }
                        keys.extend([
                            ("ghPrStatus", Value::Bool(self.gh)),
                            ("reviewRequestsEnabled", Value::Bool(self.review.enabled)),
                            ("reviewRequestsScope", Value::from(am_core::settings::ReviewScope::IDS[self.review.scope.min(2)])),
                            ("reviewDefaultGroup", serde_json::to_value(self.review.group()).unwrap_or_default()),
                            ("linearEnabled", Value::Bool(!key.is_empty())),
                            ("linearApiKey", Value::from(key)),
                            ("contextWindowTokens", Value::from(tokens)),
                            ("setupCompleted", Value::Bool(true)),
                        ]);
                        // The review groups follow the step (skipped with GitHub off).
                        if self.gh
                            && let Some(groups) = self.review.groups()
                        {
                            keys.push(("reviewGroups", serde_json::to_value(groups).unwrap_or_default()));
                        }
                        return SetupEffect::Finish(keys);
                    }
                    _ => {}
                }
            }
        }
        SetupEffect::None
    }

    pub fn on_hooks_installed(&mut self, result: Result<Option<String>, String>) {
        match result {
            Ok(bin) => {
                self.hooks_installed = true;
                for a in &mut self.agents {
                    a.1 = true;
                }
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
            ide_command: String::new(),
            ide_error: None,
            hooks_installed: false,
            agents: vec![(HarnessKind::CLAUDE, false), (HarnessKind::GEMINI, true)],
            hook_binary: None,
            repo_input: "~/".into(),
            repos_added: vec![],
            repo_error: None,
            gh: true,
            linear_key: String::new(),
            integration_field: 0,
            review: ReviewSetup::default(),
            ctx_choice: 0,
            ctx_input: String::new(),
            ctx_error: None,
            cancellable: false,
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
        assert!(s.agents.iter().all(|(_, on)| *on));
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
        assert!(matches!(s.on_key(Key::Enter), SetupEffect::None));
        assert_eq!(s.step, Step::Reviews);
        s.on_key(Key::Down);
        s.on_key(Key::Down);
        s.on_key(Key::Char(' ')); // only me directly
        s.on_key(Key::Down);
        s.on_key(Key::Down);
        s.on_key(Key::Char(' ')); // hide drafts
        s.on_key(Key::Enter);
        assert_eq!(s.step, Step::Context);
        let SetupEffect::Finish(v) = s.on_key(Key::Enter) else { panic!() };
        assert!(v.contains(&("reviewRequestsEnabled", Value::Bool(true))));
        assert!(v.contains(&("reviewRequestsScope", Value::from("direct"))));
        // Only me: one "Asked you" group, with the filters too.
        let groups = v.iter().find(|(k, _)| *k == "reviewGroups").map(|(_, g)| g.clone()).unwrap();
        assert_eq!(groups.as_array().unwrap().len(), 1);
        assert_eq!((groups[0]["name"].as_str(), groups[0]["asked"].as_str(), groups[0]["draft"].as_str()), (Some("Asked you"), Some("you"), Some("ready")));
        let group = v.iter().find(|(k, _)| *k == "reviewDefaultGroup").map(|(_, g)| g.clone()).unwrap();
        // A fresh default group has every filter off; the step turned drafts off.
        assert_eq!((group["draft"].as_str(), group["reviewed"].as_str()), (Some("ready"), Some("any")));
        assert!(!v.iter().any(|(k, _)| *k == "reviewHideDrafts"), "legacy keys aren't written");
        assert!(v.contains(&("ide", Value::from("cursor"))));
        assert!(!v.iter().any(|(k, _)| *k == "ideCommand"));
        assert!(v.contains(&("linearEnabled", Value::Bool(true))));
        assert!(v.contains(&("contextWindowTokens", Value::from(1_000_000u64))));
        assert!(v.contains(&("setupCompleted", Value::Bool(true))));
    }

    #[test]
    fn editor_step_takes_a_custom_command() {
        let mut s = setup();
        s.on_key(Key::Up); // wraps to the custom row
        assert_eq!(s.ide, IDE_CUSTOM);
        s.on_key(Key::Enter);
        assert!(s.ide_error.is_some(), "an empty command is refused");
        assert_eq!(s.step, Step::Editor);
        for c in "subl -n".chars() {
            s.on_key(Key::Char(c)); // `l` is input here, not "next"
        }
        assert_eq!((s.ide, s.ide_command.as_str()), (IDE_CUSTOM, "subl -n"));
        s.on_key(Key::Enter);
        assert_eq!(s.step, Step::Hooks);
        s.step = Step::Context;
        let SetupEffect::Finish(v) = s.on_key(Key::Enter) else { panic!() };
        assert!(v.contains(&("ide", Value::from("custom"))));
        assert!(v.contains(&("ideCommand", Value::from("subl -n"))));
    }

    #[test]
    fn rerun_starts_from_saved_settings_and_can_be_cancelled() {
        let saved = am_core::settings::Settings::parse(r#"{"setupCompleted":true,"ghPrStatus":false,"linearApiKey":"lin_x"}"#);
        let mut s = Setup::new(vec!["work".into()], &saved);
        assert!(s.cancellable);
        assert!(!s.gh);
        assert_eq!(s.linear_key, "lin_x");
        assert_eq!(s.repos_added, vec!["work".to_string()]);
        s.step = Step::Context;
        let SetupEffect::Finish(v) = s.on_key(Key::Enter) else { panic!() };
        assert!(v.contains(&("linearEnabled", Value::Bool(true))));
        assert!(v.contains(&("linearApiKey", Value::from("lin_x"))));
        assert!(v.contains(&("ghPrStatus", Value::Bool(false))));
        s.step = Step::Editor;
        assert!(matches!(s.on_key(Key::Esc), SetupEffect::Cancel));

        let first = Setup::new(vec![], &am_core::settings::Settings::parse(r#"{"linearApiKey":"lin_x"}"#));
        assert!(!first.cancellable);
        assert!(first.linear_key.is_empty());
        let mut first = first;
        assert!(matches!(first.on_key(Key::Esc), SetupEffect::None));
    }

    #[test]
    fn new_preselects_vscode() {
        let s = Setup::new(vec![], &am_core::settings::Settings::default());
        assert_eq!(IDES[s.ide].0, "vscode");
    }

    #[test]
    fn context_step_takes_a_preset_or_a_typed_value() {
        let mut s = setup();
        s.step = Step::Context;
        s.on_key(Key::Char('j'));
        let SetupEffect::Finish(v) = s.on_key(Key::Enter) else { panic!() };
        assert!(v.contains(&("contextWindowTokens", Value::from(200_000u64))));

        let mut s = setup();
        s.step = Step::Context;
        s.on_key(Key::Up); // wraps to the custom row
        assert_eq!(s.ctx_choice, CONTEXT_CUSTOM);
        for c in "abc".chars() {
            s.on_key(Key::Char(c));
        }
        assert!(matches!(s.on_key(Key::Enter), SetupEffect::None));
        assert!(s.ctx_error.is_some());
        for _ in 0..3 {
            s.on_key(Key::Backspace);
        }
        for c in "500k".chars() {
            s.on_key(Key::Char(c)); // `k` is input here, not "up"
        }
        assert_eq!(s.ctx_choice, CONTEXT_CUSTOM);
        let SetupEffect::Finish(v) = s.on_key(Key::Enter) else { panic!() };
        assert!(v.contains(&("contextWindowTokens", Value::from(500_000u64))));
        s.step = Step::Context;
        s.on_key(Key::Esc);
        assert_eq!(s.step, Step::Reviews);
        s.on_key(Key::Esc);
        assert_eq!(s.step, Step::Integrations);
    }

    #[test]
    fn the_reviews_step_makes_groups_but_keeps_the_users_own() {
        let names = |g: Option<Vec<am_core::review_groups::ReviewGroup>>| {
            g.map(|g| g.iter().map(|x| (x.name.clone(), x.asked.clone())).collect::<Vec<_>>())
        };
        let mut r = ReviewSetup::default();
        assert_eq!(
            names(r.groups()),
            Some(vec![("Asked you".into(), "you".into()), ("Asked your team".into(), "team".into())]),
            "me and my teams"
        );
        r.scope = 2;
        assert_eq!(names(r.groups()), Some(vec![("Asked your team".into(), "team".into())]));
        // Groups this step made are replaced when it's run again…
        let mut again = ReviewSetup { existing_groups: made(0, true, false), ..ReviewSetup::default() };
        again.scope = 1;
        assert_eq!(names(again.groups()), Some(vec![("Asked you".into(), "you".into())]));
        // …but not ones the user made or changed.
        let mut own = made(0, false, false);
        own[0].authors = vec!["bob".into()];
        let kept = ReviewSetup { existing_groups: own, ..ReviewSetup::default() };
        assert_eq!(kept.groups(), None);
    }

    #[test]
    fn reviews_step_is_skipped_without_github_and_starts_from_saved_values() {
        let mut s = setup();
        s.gh = false;
        s.step = Step::Integrations;
        s.on_key(Key::Enter);
        assert_eq!(s.step, Step::Context);
        s.on_key(Key::Esc);
        assert_eq!(s.step, Step::Integrations);

        let saved = am_core::settings::Settings::parse(
            r#"{"setupCompleted":true,"reviewRequestsScope":"teams","reviewHideDrafts":false,"reviewRequestsEnabled":false}"#,
        );
        let s = Setup::new(vec![], &saved);
        assert_eq!((s.review.enabled, s.review.scope, s.review.hide_drafts, s.review.hide_reviewed), (false, 2, false, true));
        // A saved default group is what the step starts from, and keeps.
        let saved = am_core::settings::Settings::parse(r#"{"reviewDefaultGroup":{"draft":"ready","authors":["bob"]}}"#);
        let s = Setup::new(vec![], &saved);
        assert!(s.review.hide_drafts && !s.review.hide_reviewed);
        assert_eq!(s.review.group().authors, ["bob"]);
    }

    #[test]
    fn new_preselects_the_current_window() {
        let with = |n: u64| Setup::new(vec![], &am_core::settings::Settings { context_window_tokens: n, ..Default::default() });
        assert_eq!(with(1_000_000).ctx_choice, 0);
        assert_eq!(with(200_000).ctx_choice, 1);
        let s = with(128_000);
        assert_eq!((s.ctx_choice, s.ctx_input.as_str()), (CONTEXT_CUSTOM, "128k"));
    }

    #[test]
    fn needed_only_without_setup_completed() {
        assert!(needed(&am_core::settings::Settings::parse("{}")));
        assert!(!needed(&am_core::settings::Settings::parse(r#"{"setupCompleted":true}"#)));
    }
}
