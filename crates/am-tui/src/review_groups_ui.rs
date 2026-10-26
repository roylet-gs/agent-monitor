//! Review filters → Groups…: the review page's groups (`reviewGroups`) and
//! the default group under them (`reviewDefaultGroup`). The list adds,
//! removes, reorders and opens groups; a group's form edits its name, rules
//! and badge toggle, with autocomplete for logins, teams and repos. Pure
//! state and keys; every change comes back as `GroupsOutcome::Save` for
//! `settings_ui` to persist.

use am_core::model::ReviewPerson;
use am_core::review_groups::{self as rg, ReviewGroup};

use crate::app::Key;

/// Autocomplete suggestions shown at once.
pub const MAX_SUGGESTIONS: usize = 6;

/// What the list fields can be completed from (the app keeps it current).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Candidates {
    /// People who can author PRs (GitHub) plus the review list's authors.
    pub people: Vec<ReviewPerson>,
    /// Teams asked in the review list (`org/team`).
    pub teams: Vec<ReviewPerson>,
    /// Tracked repos (directory names, with `owner/name` as the name).
    pub repos: Vec<ReviewPerson>,
}

/// PRs per group (the default group last) and how many are hidden, under
/// the saved groups (the app keeps it current).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupCounts {
    pub per_group: Vec<usize>,
    pub hidden: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    People,
    Teams,
    Repos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormKind {
    Text,
    /// Comma separated, completed from a [`Source`].
    List(Source),
    Choice(&'static [&'static str]),
    Days,
    Bool,
}

pub struct FormField {
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub kind: FormKind,
}

pub const FORM: &[FormField] = &[
    FormField { key: "name", label: "Name", help: "Section header and badge label (empty: Group N)", kind: FormKind::Text },
    FormField { key: "enabled", label: "On", help: "Off: the group takes nothing and its PRs fall to the next (the default off hides the rest)", kind: FormKind::Bool },
    FormField { key: "sort", label: "Order", help: "direct: asks you by name first; repo: by repository; updated: newest first; oldest: waiting longest", kind: FormKind::Choice(&am_core::settings::ReviewSort::IDS) },
    FormField { key: "authors", label: "Authors", help: "PR authors, comma separated (⇥ completes from GitHub)", kind: FormKind::List(Source::People) },
    FormField { key: "asked", label: "Asked", help: "you: asks you by name; team: asks one of your teams", kind: FormKind::Choice(&rg::ASKED) },
    FormField { key: "teams", label: "Teams", help: "Asks one of these teams (⇥ completes from the list)", kind: FormKind::List(Source::Teams) },
    FormField { key: "repos", label: "Only repos", help: "In one of these tracked repos (⇥ completes)", kind: FormKind::List(Source::Repos) },
    FormField { key: "excludeRepos", label: "Not in repos", help: "Not in any of these tracked repos (⇥ completes)", kind: FormKind::List(Source::Repos) },
    FormField { key: "draft", label: "Draft", help: "draft: only drafts; ready: no drafts", kind: FormKind::Choice(&rg::DRAFT) },
    FormField { key: "checks", label: "CI checks", help: "The PR's checks", kind: FormKind::Choice(&rg::CHECKS) },
    FormField { key: "review", label: "Review state", help: "approved, changes requested, or still needs a review", kind: FormKind::Choice(&rg::REVIEW) },
    FormField { key: "reviewed", label: "Reviewed by you", help: "yes: you've already reviewed it; no: you haven't yet", kind: FormKind::Choice(&rg::REVIEWED) },
    FormField { key: "olderThanDays", label: "Older than (days)", help: "Opened more than this many days ago; 0 = any age", kind: FormKind::Days },
    FormField { key: "ticket", label: "Linear ticket", help: "yes: has a ticket; no: has none", kind: FormKind::Choice(&rg::TICKET) },
    FormField { key: "text", label: "Title/branch has", help: "Text in the PR title or branch (any case)", kind: FormKind::Text },
    FormField { key: "query", label: "GitHub search", help: "Search terms GitHub must match, e.g. label:urgent -author:app/dependabot", kind: FormKind::Text },
    FormField { key: "badge", label: "Show on dashboard", help: "Count this group in the dashboard's review badge", kind: FormKind::Bool },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupsOutcome {
    Stay,
    Close,
    /// The groups changed: persist `groups` and `default`.
    Save,
}

/// One group's form.
#[derive(Debug, Clone)]
pub struct GroupForm {
    /// Index into `groups`, or `None` for the default group.
    pub which: Option<usize>,
    pub cursor: usize,
    pub editing: Option<String>,
    /// Highlighted entry of `completions`.
    pub suggest: usize,
}

#[derive(Debug, Clone)]
pub struct GroupsEditor {
    pub groups: Vec<ReviewGroup>,
    pub default: ReviewGroup,
    /// `0..groups.len()` a group, `groups.len()` the default group.
    pub cursor: usize,
    pub form: Option<GroupForm>,
    /// Group index waiting for `y` to be removed.
    pub confirm_remove: Option<usize>,
}

/// The entry being typed in a list buffer: what follows the last separator,
/// without `@`.
fn typed(buf: &str) -> &str {
    let start = buf.rfind([',', ' ']).map_or(0, |i| i + 1);
    buf[start..].trim_start_matches('@')
}

/// `cands` matching the entry being typed in `buf` (all of them before
/// anything is typed), leaving out the ones already listed: names starting
/// with it first, then a display-name word starting with it, then anything
/// containing it. At most [`MAX_SUGGESTIONS`].
pub fn complete<'a>(cands: &'a [ReviewPerson], buf: &str) -> Vec<&'a ReviewPerson> {
    let t = typed(buf).to_lowercase();
    let listed = am_core::settings::split_logins(&buf[..buf.len() - typed(buf).len()]);
    let rank = |p: &ReviewPerson| {
        let (login, name) = (p.login.to_lowercase(), p.name.to_lowercase());
        let short = login.rsplit('/').next().unwrap_or("").to_string();
        if login.starts_with(&t) || short.starts_with(&t) {
            Some(0)
        } else if name.split_whitespace().any(|w| w.starts_with(&t)) {
            Some(1)
        } else if login.contains(&t) || name.contains(&t) {
            Some(2)
        } else {
            None
        }
    };
    let mut found: Vec<(u8, &ReviewPerson)> = cands
        .iter()
        .filter(|p| !listed.contains(&p.login.to_lowercase()))
        .filter_map(|p| rank(p).map(|r| (r, p)))
        .collect();
    found.sort_by_key(|(r, p)| (*r, p.login.to_lowercase()));
    found.into_iter().take(MAX_SUGGESTIONS).map(|(_, p)| p).collect()
}

/// Put `login` in place of the entry being typed in `buf`.
fn accept(buf: &mut String, login: &str) {
    let keep = buf.len() - typed(buf).len();
    buf.truncate(keep);
    if !buf.is_empty() && !buf.ends_with([',', ' ']) {
        buf.push(' ');
    }
    buf.push_str(login);
    buf.push_str(", ");
}

fn list_text(v: &[String], at: bool) -> String {
    let v: Vec<String> = am_core::settings::split_logins(&v.join(","));
    if v.is_empty() {
        return "any".into();
    }
    v.iter().map(|s| if at { format!("@{s}") } else { s.clone() }).collect::<Vec<_>>().join(", ")
}

impl GroupForm {
    pub fn field(&self) -> &'static FormField {
        &FORM[self.cursor]
    }

    /// Suggestions for the list field being edited.
    pub fn completions<'a>(&self, cands: &'a Candidates) -> Vec<&'a ReviewPerson> {
        let (Some(buf), FormKind::List(src)) = (&self.editing, self.field().kind) else { return Vec::new() };
        let pool = match src {
            Source::People => &cands.people,
            Source::Teams => &cands.teams,
            Source::Repos => &cands.repos,
        };
        complete(pool, buf)
    }

    /// Take suggestion `i` (a click, or ⇥ on the highlighted one).
    pub fn accept(&mut self, i: usize, cands: &Candidates) {
        let Some(login) = self.completions(cands).get(i).map(|p| p.login.clone()) else { return };
        if let Some(buf) = &mut self.editing {
            accept(buf, &login);
        }
        self.suggest = 0;
    }
}

/// A form field's value as shown.
pub fn display(g: &ReviewGroup, f: &FormField) -> String {
    match f.key {
        "name" if g.name.trim().is_empty() => "—".into(),
        "name" => g.name.clone(),
        "authors" => list_text(&g.authors, true),
        "teams" => list_text(&g.teams, true),
        "repos" => list_text(&g.repos, false),
        "excludeRepos" if g.exclude_repos.is_empty() => "none".into(),
        "excludeRepos" => list_text(&g.exclude_repos, false),
        "sort" => g.sort.clone(),
        "reviewed" => g.reviewed.clone(),
        "query" if g.query.trim().is_empty() => "—".into(),
        "query" => g.query.clone(),
        "asked" => g.asked.clone(),
        "draft" => g.draft.clone(),
        "checks" => g.checks.clone(),
        "review" => g.review.clone(),
        "olderThanDays" if g.older_than_days == 0 => "any".into(),
        "olderThanDays" => format!("{} days", g.older_than_days),
        "ticket" => g.ticket.clone(),
        "text" if g.text.trim().is_empty() => "—".into(),
        "text" => g.text.clone(),
        "badge" => if g.badge { "on" } else { "off" }.into(),
        "enabled" => if g.enabled { "on" } else { "off" }.into(),
        _ => String::new(),
    }
}

/// The text a field starts from when editing begins.
fn edit_text(g: &ReviewGroup, key: &str) -> String {
    let list = |v: &[String]| am_core::settings::split_logins(&v.join(",")).iter().map(|s| format!("{s}, ")).collect();
    match key {
        "name" => g.name.clone(),
        "authors" => list(&g.authors),
        "teams" => list(&g.teams),
        "repos" => list(&g.repos),
        "excludeRepos" => list(&g.exclude_repos),
        "query" => g.query.clone(),
        "olderThanDays" => g.older_than_days.to_string(),
        "text" => g.text.clone(),
        _ => String::new(),
    }
}

/// Flip a switch field of `g`.
fn toggle(g: &mut ReviewGroup, key: &str) {
    match key {
        "enabled" => g.enabled = !g.enabled,
        _ => g.badge = !g.badge,
    }
}

/// Store an edited text field's `text` in `g`.
fn set_text(g: &mut ReviewGroup, key: &str, text: &str) {
    let list = || am_core::settings::split_logins(text);
    match key {
        "name" => g.name = text.trim().to_string(),
        "authors" => g.authors = list(),
        "teams" => g.teams = list(),
        "repos" => g.repos = list(),
        "excludeRepos" => g.exclude_repos = list(),
        "query" => g.query = am_core::review_groups::query_key(text),
        "olderThanDays" => g.older_than_days = text.trim().parse().unwrap_or(0),
        "text" => g.text = text.trim().to_string(),
        _ => {}
    }
}

/// Entries of a list field that match none of `cands` (a typo, or a
/// partial name), so the form can say so.
pub fn unknown(g: &ReviewGroup, key: &str, cands: &Candidates) -> Vec<String> {
    let (entries, pool) = match key {
        "authors" => (&g.authors, &cands.people),
        "teams" => (&g.teams, &cands.teams),
        "repos" => (&g.repos, &cands.repos),
        "excludeRepos" => (&g.exclude_repos, &cands.repos),
        _ => return Vec::new(),
    };
    // Nothing to compare against yet (not loaded): don't cry wolf.
    if pool.is_empty() {
        return Vec::new();
    }
    let known = |e: &str| {
        pool.iter().any(|p| {
            let login = p.login.to_lowercase();
            login == e || login.rsplit('/').next() == Some(e) || p.name.to_lowercase() == e
        })
    };
    am_core::settings::split_logins(&entries.join(",")).into_iter().filter(|e| !known(e)).collect()
}

fn choice_mut<'a>(g: &'a mut ReviewGroup, key: &str) -> Option<&'a mut String> {
    Some(match key {
        "asked" => &mut g.asked,
        "draft" => &mut g.draft,
        "checks" => &mut g.checks,
        "review" => &mut g.review,
        "ticket" => &mut g.ticket,
        "sort" => &mut g.sort,
        "reviewed" => &mut g.reviewed,
        _ => return None,
    })
}

impl GroupsEditor {
    pub fn new(groups: Vec<ReviewGroup>, default: ReviewGroup) -> GroupsEditor {
        GroupsEditor { groups, default, cursor: 0, form: None, confirm_remove: None }
    }

    /// The group at `which` (`None` = the default group).
    pub fn group(&self, which: Option<usize>) -> &ReviewGroup {
        which.and_then(|i| self.groups.get(i)).unwrap_or(&self.default)
    }

    fn group_mut(&mut self, which: Option<usize>) -> &mut ReviewGroup {
        match which {
            Some(i) if i < self.groups.len() => &mut self.groups[i],
            _ => &mut self.default,
        }
    }

    /// The groups as they'd be with the field being typed saved (GitHub
    /// search terms aside, which only GitHub can check): the page previews
    /// them from the cached list while the form is open.
    pub fn preview(&self) -> (Vec<ReviewGroup>, ReviewGroup) {
        let (mut groups, mut default) = (self.groups.clone(), self.default.clone());
        if let Some(form) = &self.form
            && let Some(buf) = &form.editing
            && form.field().key != "query"
        {
            let g = match form.which {
                Some(i) if i < groups.len() => &mut groups[i],
                _ => &mut default,
            };
            set_text(g, form.field().key, buf);
        }
        (groups, default)
    }

    /// The list row's label: the name, `Group N`, or for the default group
    /// its name or `Everything else`.
    pub fn label(&self, i: usize) -> String {
        match self.groups.get(i) {
            Some(g) if !g.name.trim().is_empty() => g.name.trim().to_string(),
            Some(_) => format!("Group {}", i + 1),
            None if !self.default.name.trim().is_empty() => self.default.name.trim().to_string(),
            None => "Everything else".into(),
        }
    }

    #[cfg(test)]
    pub fn open_for_test(&mut self, i: usize) {
        self.cursor = i;
        self.open(i);
    }

    fn open(&mut self, i: usize) {
        let which = (i < self.groups.len()).then_some(i);
        self.form = Some(GroupForm { which, cursor: 0, editing: None, suggest: 0 });
    }

    pub fn on_key(&mut self, key: Key, cands: &Candidates) -> GroupsOutcome {
        if self.form.is_some() {
            return self.form_key(key, cands);
        }
        if let Some(i) = self.confirm_remove.take() {
            if key == Key::Char('y') && i < self.groups.len() {
                self.groups.remove(i);
                self.cursor = self.cursor.min(self.groups.len());
                return GroupsOutcome::Save;
            }
            return GroupsOutcome::Stay;
        }
        let n = self.groups.len();
        let on_group = self.cursor < n;
        match key {
            Key::Esc | Key::Char('q') => return GroupsOutcome::Close,
            Key::Down | Key::Char('j') => self.cursor = (self.cursor + 1).min(n),
            Key::Up | Key::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            Key::Char('a') => {
                // A new group goes after the selected one (above the default).
                let at = if on_group { self.cursor + 1 } else { n };
                self.groups.insert(at, ReviewGroup::default());
                self.cursor = at;
                self.open(at);
                return GroupsOutcome::Save;
            }
            Key::Char('x') if on_group => self.confirm_remove = Some(self.cursor),
            Key::Char('J') if self.cursor + 1 < n => {
                self.groups.swap(self.cursor, self.cursor + 1);
                self.cursor += 1;
                return GroupsOutcome::Save;
            }
            Key::Char('K') if on_group && self.cursor > 0 => {
                self.groups.swap(self.cursor, self.cursor - 1);
                self.cursor -= 1;
                return GroupsOutcome::Save;
            }
            Key::Char(' ') => {
                let which = on_group.then_some(self.cursor);
                let g = self.group_mut(which);
                g.enabled = !g.enabled;
                return GroupsOutcome::Save;
            }
            Key::Char('b') => {
                let which = on_group.then_some(self.cursor);
                let g = self.group_mut(which);
                g.badge = !g.badge;
                return GroupsOutcome::Save;
            }
            Key::Enter => self.open(self.cursor),
            _ => {}
        }
        GroupsOutcome::Stay
    }

    fn form_key(&mut self, key: Key, cands: &Candidates) -> GroupsOutcome {
        let Some(form) = &mut self.form else { return GroupsOutcome::Stay };
        let field = form.field();
        if let Some(buf) = &mut form.editing {
            if matches!(field.kind, FormKind::List(_)) {
                match key {
                    Key::Tab => {
                        let i = form.suggest;
                        form.accept(i, cands);
                        return GroupsOutcome::Stay;
                    }
                    Key::Down => {
                        form.suggest = (form.suggest + 1).min(form.completions(cands).len().saturating_sub(1));
                        return GroupsOutcome::Stay;
                    }
                    Key::Up => {
                        form.suggest = form.suggest.saturating_sub(1);
                        return GroupsOutcome::Stay;
                    }
                    Key::Char(_) | Key::Backspace => form.suggest = 0,
                    _ => {}
                }
            }
            match key {
                Key::Esc => form.editing = None,
                Key::Backspace => {
                    buf.pop();
                }
                Key::Char(c) if field.kind != FormKind::Days || c.is_ascii_digit() => buf.push(c),
                Key::Enter => {
                    // Partway through a name with a suggestion showing: take it.
                    if matches!(field.kind, FormKind::List(_)) && !typed(buf).trim().is_empty() {
                        let i = form.suggest;
                        form.accept(i, cands);
                    }
                    let text = form.editing.take().unwrap_or_default();
                    let which = form.which;
                    set_text(self.group_mut(which), field.key, &text);
                    return GroupsOutcome::Save;
                }
                _ => {}
            }
            return GroupsOutcome::Stay;
        }
        let which = form.which;
        match key {
            Key::Esc | Key::Char('q') => self.form = None,
            Key::Down | Key::Char('j') => form.cursor = (form.cursor + 1).min(FORM.len() - 1),
            Key::Up | Key::Char('k') => form.cursor = form.cursor.saturating_sub(1),
            Key::Enter | Key::Char(' ') => match field.kind {
                FormKind::Bool => {
                    toggle(self.group_mut(which), field.key);
                    return GroupsOutcome::Save;
                }
                FormKind::Choice(_) => return self.step(1),
                _ => {
                    let text = edit_text(self.group(which), field.key);
                    if let Some(form) = &mut self.form {
                        form.editing = Some(text);
                        form.suggest = 0;
                    }
                }
            },
            Key::Right | Key::Char('l') => return self.step(1),
            Key::Left | Key::Char('h') => return self.step(-1),
            _ => {}
        }
        GroupsOutcome::Stay
    }

    /// ←→ on a choice cycles it; on the age it steps a day.
    fn step(&mut self, dir: i64) -> GroupsOutcome {
        let Some(form) = &self.form else { return GroupsOutcome::Stay };
        let (which, field) = (form.which, form.field());
        let g = self.group_mut(which);
        match field.kind {
            FormKind::Choice(opts) => {
                let Some(v) = choice_mut(g, field.key) else { return GroupsOutcome::Stay };
                let i = opts.iter().position(|o| o == v).unwrap_or(0) as i64;
                *v = opts[(i + dir).rem_euclid(opts.len() as i64) as usize].to_string();
            }
            FormKind::Days => g.older_than_days = g.older_than_days.saturating_add_signed(dir),
            FormKind::Bool => toggle(g, field.key),
            _ => return GroupsOutcome::Stay,
        }
        GroupsOutcome::Save
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(login: &str, name: &str) -> ReviewPerson {
        ReviewPerson { login: login.into(), name: name.into() }
    }

    fn cands() -> Candidates {
        Candidates {
            people: vec![person("alice", "Alice Liddell"), person("bob", ""), person("carol", "Bob Carol")],
            teams: vec![person("acme/platform", ""), person("acme/docs", "")],
            repos: vec![person("web", "acme/web")],
        }
    }

    fn typing(ed: &mut GroupsEditor, text: &str) {
        for c in text.chars() {
            ed.on_key(Key::Char(c), &cands());
        }
    }

    #[test]
    fn add_reorder_toggle_and_remove_groups() {
        let c = cands();
        let mut ed = GroupsEditor::new(vec![], ReviewGroup::default());
        assert_eq!(ed.label(0), "Everything else");
        // `a` adds a group above the default and opens its form.
        assert_eq!(ed.on_key(Key::Char('a'), &c), GroupsOutcome::Save);
        assert_eq!((ed.groups.len(), ed.cursor, ed.form.as_ref().unwrap().which), (1, 0, Some(0)));
        ed.on_key(Key::Enter, &c); // Name
        typing(&mut ed, "Mine");
        assert_eq!(ed.on_key(Key::Enter, &c), GroupsOutcome::Save);
        ed.on_key(Key::Down, &c); // On
        assert_eq!(ed.on_key(Key::Enter, &c), GroupsOutcome::Save);
        assert!(!ed.groups[0].enabled);
        ed.on_key(Key::Char(' '), &c);
        assert!(ed.groups[0].enabled);
        ed.on_key(Key::Down, &c); // Order
        assert_eq!(ed.on_key(Key::Right, &c), GroupsOutcome::Save);
        assert_eq!(ed.groups[0].sort, "repo");
        ed.on_key(Key::Down, &c);
        ed.on_key(Key::Down, &c); // Asked
        assert_eq!(ed.on_key(Key::Right, &c), GroupsOutcome::Save);
        assert_eq!(ed.groups[0].asked, "you");
        assert_eq!(ed.on_key(Key::Esc, &c), GroupsOutcome::Stay);
        assert!(ed.form.is_none());

        ed.on_key(Key::Char('a'), &c);
        ed.on_key(Key::Esc, &c);
        assert_eq!(ed.label(1), "Group 2");
        assert_eq!(ed.on_key(Key::Char('K'), &c), GroupsOutcome::Save);
        assert_eq!((ed.label(0).as_str(), ed.label(1).as_str(), ed.cursor), ("Group 1", "Mine", 0));
        // The default can't move or go, but its badge and switch toggle.
        ed.cursor = 2;
        assert_eq!(ed.on_key(Key::Char('K'), &c), GroupsOutcome::Stay);
        assert_eq!(ed.on_key(Key::Char('x'), &c), GroupsOutcome::Stay);
        assert!(ed.confirm_remove.is_none());
        assert_eq!(ed.on_key(Key::Char('b'), &c), GroupsOutcome::Save);
        assert!(!ed.default.badge);
        assert_eq!(ed.on_key(Key::Char(' '), &c), GroupsOutcome::Save);
        assert!(!ed.default.enabled);
        ed.on_key(Key::Char(' '), &c);
        // x asks; anything but y keeps it.
        ed.cursor = 0;
        ed.on_key(Key::Char('x'), &c);
        assert_eq!(ed.on_key(Key::Char('n'), &c), GroupsOutcome::Stay);
        assert_eq!(ed.groups.len(), 2);
        ed.on_key(Key::Char('x'), &c);
        assert_eq!(ed.on_key(Key::Char('y'), &c), GroupsOutcome::Save);
        assert_eq!(ed.groups.len(), 1);
        assert_eq!(ed.groups[0].name, "Mine");
        assert_eq!(ed.on_key(Key::Esc, &c), GroupsOutcome::Close);
    }

    #[test]
    fn list_fields_complete_from_their_source() {
        let c = cands();
        let mut ed = GroupsEditor::new(vec![ReviewGroup::default()], ReviewGroup::default());
        ed.on_key(Key::Enter, &c);
        ed.form.as_mut().unwrap().cursor = FORM.iter().position(|f| f.key == "authors").unwrap();
        ed.on_key(Key::Enter, &c);
        let form = |ed: &GroupsEditor| ed.form.clone().unwrap();
        assert_eq!(form(&ed).completions(&c).len(), 3, "everyone before anything is typed");
        typing(&mut ed, "b");
        let logins = |ed: &GroupsEditor| form(ed).completions(&c).iter().map(|p| p.login.clone()).collect::<Vec<_>>();
        assert_eq!(logins(&ed), ["bob", "carol"], "login prefix, then a name's word");
        ed.on_key(Key::Down, &c);
        ed.on_key(Key::Tab, &c);
        assert_eq!(form(&ed).editing.as_deref(), Some("carol, "));
        typing(&mut ed, "@l");
        assert_eq!(logins(&ed), ["alice"], "listed ones are left out; substring match");
        ed.on_key(Key::Tab, &c);
        assert_eq!(ed.on_key(Key::Enter, &c), GroupsOutcome::Save);
        assert_eq!(ed.groups[0].authors, ["carol", "alice"]);
        assert_eq!(display(&ed.groups[0], &FORM[3]), "@carol, @alice");
        // Editing again continues after the listed ones.
        ed.on_key(Key::Enter, &c);
        assert_eq!(form(&ed).editing.as_deref(), Some("carol, alice, "));
        ed.on_key(Key::Esc, &c);

        // Teams complete by their short name too; repos from tracked ones.
        ed.on_key(Key::Down, &c);
        ed.on_key(Key::Down, &c); // Teams
        ed.on_key(Key::Enter, &c);
        typing(&mut ed, "pla");
        ed.on_key(Key::Tab, &c);
        ed.on_key(Key::Enter, &c);
        assert_eq!(ed.groups[0].teams, ["acme/platform"]);
        ed.on_key(Key::Down, &c); // Repos
        ed.on_key(Key::Enter, &c);
        ed.on_key(Key::Tab, &c);
        ed.on_key(Key::Enter, &c);
        assert_eq!(ed.groups[0].repos, ["web"]);
    }

    #[test]
    fn enter_takes_the_suggestion_and_unknown_entries_are_flagged() {
        let c = cands();
        let mut ed = GroupsEditor::new(vec![ReviewGroup::default()], ReviewGroup::default());
        ed.on_key(Key::Enter, &c);
        ed.form.as_mut().unwrap().cursor = FORM.iter().position(|f| f.key == "teams").unwrap();
        ed.on_key(Key::Enter, &c);
        typing(&mut ed, "pla");
        // The page previews the half-typed entry before it's saved.
        assert_eq!(ed.preview().0[0].teams, ["pla"]);
        assert!(ed.groups[0].teams.is_empty());
        assert_eq!(ed.on_key(Key::Enter, &c), GroupsOutcome::Save);
        assert_eq!(ed.groups[0].teams, ["acme/platform"], "⏎ took the suggestion");
        assert!(unknown(&ed.groups[0], "teams", &c).is_empty());
        // No suggestion: saved as typed, and flagged.
        ed.on_key(Key::Enter, &c);
        typing(&mut ed, "zzz");
        ed.on_key(Key::Enter, &c);
        assert_eq!(ed.groups[0].teams, ["acme/platform", "zzz"]);
        assert_eq!(unknown(&ed.groups[0], "teams", &c), ["zzz"]);
        // Short team names count as known.
        ed.groups[0].teams = vec!["platform".into()];
        assert!(unknown(&ed.groups[0], "teams", &c).is_empty());
        assert!(unknown(&ed.groups[0], "teams", &Candidates::default()).is_empty(), "nothing loaded: no warning");
    }

    #[test]
    fn days_take_digits_and_step() {
        let c = cands();
        let mut ed = GroupsEditor::new(vec![], ReviewGroup::default());
        ed.on_key(Key::Enter, &c); // the default group's form
        assert_eq!(ed.form.as_ref().unwrap().which, None);
        ed.form.as_mut().unwrap().cursor = FORM.iter().position(|f| f.key == "olderThanDays").unwrap();
        ed.on_key(Key::Enter, &c);
        ed.on_key(Key::Backspace, &c);
        typing(&mut ed, "1x4");
        ed.on_key(Key::Enter, &c);
        assert_eq!(ed.default.older_than_days, 14);
        ed.on_key(Key::Left, &c);
        assert_eq!(display(&ed.default, ed.form.as_ref().unwrap().field()), "13 days");
    }
}
