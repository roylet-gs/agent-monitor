//! Rendering. lazygit-style titled panes: `[1] Worktrees` and `[2] Subagents`,
//! a context line for the selection, and a status bar with a mode pill.
//! ratatui diffs each frame against the previous one cell by cell, so a
//! redraw only writes what changed — no full-screen clears, no flicker.

use am_core::model::{ChecksStatus, Subagent, WorktreeView};
use am_core::status::Status;
use am_core::timefmt;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Pane, Row, Scope};
use crate::theme::{Theme, status_glyph, status_label};

/// Side-by-side panes from this width; below it they stack.
pub const SIDE_BY_SIDE_MIN_COLS: u16 = 100;

pub fn draw(f: &mut Frame, app: &App, theme: &Theme, now: i64, log_lines: &[String]) {
    let area = f.area();
    let log_h = if app.show_log { (area.height / 3).max(5) } else { 0 };
    let [body, log, context, status] = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(log_h),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    if app.show_log {
        draw_log(f, theme, log, log_lines);
    }

    let (left, right) = if area.width >= SIDE_BY_SIDE_MIN_COLS {
        let [l, r] = Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(body);
        (l, r)
    } else {
        let [t, b] = Layout::vertical([Constraint::Percentage(62), Constraint::Percentage(38)]).areas(body);
        (t, b)
    };

    draw_worktrees(f, app, theme, left, now);
    draw_subagents(f, app, theme, right, now);
    draw_context(f, app, theme, context);
    draw_status_bar(f, app, theme, status);
    if app.show_help {
        draw_help(f, theme, area);
    }
    if let Some(editor) = &app.settings_editor {
        draw_settings(f, theme, area, editor);
    }
    if let Some(setup) = &app.setup {
        draw_setup(f, theme, area, setup);
    }
    if let Some(modal) = &app.modal {
        draw_modal(f, theme, area, modal, now);
    }
    if let Some(msg) = &app.popup {
        draw_popup(f, theme, area, msg);
    }
}

fn draw_settings(f: &mut Frame, theme: &Theme, area: Rect, ed: &crate::settings_ui::SettingsEditor) {
    use crate::settings_ui::{FIELDS, Kind, sort_label};
    let w = 84.min(area.width.saturating_sub(2));
    let h = area.height.saturating_sub(2).min(FIELDS.len() as u16 + 16);
    let rect = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.bold(theme.border_focus))
        .title(Line::from(vec![Span::styled("─", theme.bold(theme.border_focus)), Span::styled("Settings ", theme.bold(theme.text))]))
        .title_bottom(
            Line::styled(" ⏎/space change  ←→ adjust  e edit file  esc close ", theme.faint()).alignment(Alignment::Right),
        );
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let [list, help] = Layout::vertical([Constraint::Min(3), Constraint::Length(2)]).areas(inner);

    if let Some(sort) = &ed.sort {
        let mut lines = vec![
            Line::styled(" Sort order — space on/off · d direction · J/K move · ⏎ save · esc cancel", theme.faint()),
            Line::raw(""),
        ];
        for (i, c) in sort.items.iter().enumerate() {
            let sel = i == sort.cursor;
            let dir = match c.direction {
                am_core::settings::Direction::Asc => "↑ asc ",
                am_core::settings::Direction::Desc => "↓ desc",
            };
            let line = Line::from(vec![
                Span::styled(if sel { "▌" } else { " " }, theme.bold(theme.border_focus)),
                Span::styled(format!("{:>2}. ", i + 1), theme.faint()),
                Span::styled(if c.enabled { "[x] " } else { "[ ] " }, theme.fg(if c.enabled { theme.green } else { theme.faint })),
                Span::styled(format!("{:<16}", sort_label(c.key)), if c.enabled { theme.fg(theme.text) } else { theme.faint() }),
                Span::styled(dir, theme.faint()),
            ]);
            lines.push(if sel { line.style(theme.selected()) } else { line });
        }
        f.render_widget(Paragraph::new(lines), list);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    let mut sel_line = 0;
    let mut section = "";
    let label_w = 30usize;
    for (i, field) in FIELDS.iter().enumerate() {
        if field.section != section {
            section = field.section;
            if !lines.is_empty() {
                lines.push(Line::raw(""));
            }
            lines.push(Line::styled(format!(" {section}"), theme.bold(theme.accent)));
        }
        let sel = i == ed.cursor;
        if sel {
            sel_line = lines.len();
        }
        let value = match (&ed.editing, sel) {
            (Some(buf), true) => {
                let shown = if matches!(field.kind, Kind::Text { secret: true }) { "•".repeat(buf.chars().count()) } else { buf.clone() };
                format!("{shown}▏")
            }
            _ => ed.display(field),
        };
        let vstyle = match field.kind {
            Kind::Bool if value == "on" => theme.fg(theme.green),
            Kind::Bool => theme.faint(),
            _ => theme.fg(theme.text),
        };
        let avail = (list.width as usize).saturating_sub(label_w + 4);
        let line = Line::from(vec![
            Span::styled(if sel { "▌" } else { " " }, theme.bold(theme.border_focus)),
            Span::styled(format!("  {:<width$}", field.label, width = label_w), theme.fg(theme.text)),
            Span::styled(truncate(&value, avail), vstyle),
        ]);
        lines.push(if sel { line.style(theme.selected()) } else { line });
    }
    let height = list.height as usize;
    let start = (sel_line + 2).saturating_sub(height);
    f.render_widget(Paragraph::new(lines.into_iter().skip(start).take(height).collect::<Vec<_>>()), list);

    let field = ed.field();
    let help_line = match &ed.error {
        Some(e) => Line::styled(format!(" {e}"), theme.fg(theme.red)),
        None => Line::styled(format!(" {}", field.help), theme.faint()),
    };
    f.render_widget(Paragraph::new(vec![Line::raw(""), help_line]), help);
}

fn modal_rect(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

fn modal_block<'a>(theme: &Theme, title: String, color: ratatui::style::Color, hint: &'a str) -> Block<'a> {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.bold(color))
        .padding(ratatui::widgets::Padding::horizontal(1))
        .title(Line::from(vec![Span::styled("─", theme.bold(color)), Span::styled(format!("{title} "), theme.bold(theme.text))]));
    if hint.is_empty() {
        block
    } else {
        block.title_bottom(Line::styled(format!(" {hint} "), theme.faint()).alignment(Alignment::Right))
    }
}

fn key_line<'a>(theme: &Theme, key: &str, text: impl Into<String>, danger: bool) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{key:>9}  "), theme.bold(if danger { theme.red } else { theme.yellow })),
        Span::styled(text.into(), theme.fg(theme.text)),
    ])
}

fn draw_modal(f: &mut Frame, theme: &Theme, area: Rect, modal: &crate::modals::Modal, now: i64) {
    use crate::modals::{DeleteStep, FormField, Modal};
    let (title, color, hint, lines): (String, _, &str, Vec<Line>) = match modal {
        Modal::Busy(text) => {
            const SPIN: [&str; 4] = ["◐", "◓", "◑", "◒"];
            let s = SPIN[(now.rem_euclid(4)) as usize];
            ("Working".into(), theme.accent, "", vec![Line::from(vec![Span::styled(format!("{s} "), theme.fg(theme.accent)), Span::raw(text.clone())])])
        }
        Modal::New(form) => {
            let field = |label: &str, value: String, which: FormField, note: Option<Line<'static>>| -> Vec<Line<'static>> {
                let active = form.field == which;
                let mut v = vec![Line::from(vec![
                    Span::styled(if active { "▌" } else { " " }, theme.bold(theme.border_focus)),
                    Span::styled(format!("{label:<10}"), if active { theme.bold(theme.text) } else { theme.faint() }),
                    Span::styled(
                        if active && which != FormField::Repo { format!("{value}▏") } else { value },
                        theme.fg(theme.text),
                    ),
                ])];
                if let Some(n) = note {
                    v.push(n);
                }
                v
            };
            let mut lines = Vec::new();
            if form.repos.len() > 1 {
                lines.extend(field("Repo", format!("‹ {} ›", form.repo().name), FormField::Repo, None));
            }
            let note = if form.remote_exists() {
                Some(Line::styled("           ✓ exists on origin — Enter will offer to pull it", theme.fg(theme.green)))
            } else if form.local_only() {
                Some(Line::styled("           ! exists locally only — Enter will offer to reuse it", theme.fg(theme.yellow)))
            } else if form.pending_check.is_some() {
                Some(Line::styled("           checking…", theme.faint()))
            } else {
                None
            };
            lines.extend(field("Branch", form.branch.clone(), FormField::Branch, note));
            lines.extend(field("Name", if form.name.is_empty() && form.field != FormField::Name { "(optional)".into() } else { form.name.clone() }, FormField::Name, None));
            if form.remote_exists() {
                lines.push(Line::styled(format!(" Base      tracks origin/{}", form.branch), theme.faint()));
            } else {
                lines.extend(field("Base", form.base.clone(), FormField::Base, None));
            }
            if let Some(e) = &form.error {
                lines.push(Line::raw(""));
                lines.push(Line::styled(format!(" {e}"), theme.fg(theme.red)));
            }
            ("New worktree".into(), theme.border_focus, "tab next  ⏎ create  esc cancel", lines)
        }
        Modal::BranchExists { req, local, remote, .. } => {
            let b = &req.branch;
            let header = match (local, remote) {
                (true, true) => format!("Branch \"{b}\" exists locally and on origin"),
                (_, true) => format!("Branch \"{b}\" exists on origin"),
                _ => format!("Branch \"{b}\" already exists locally"),
            };
            let mut lines = vec![Line::styled(header, theme.bold(theme.yellow)), Line::raw("")];
            if *remote {
                lines.push(key_line(theme, "⏎ / p", if *local { "Pull remote (reset local to origin)".to_string() } else { format!("Pull remote (track origin/{b})") }, false));
                lines.push(key_line(
                    theme,
                    "c",
                    if *local { "Create disconnected (deletes local, no tracking)" } else { "Create disconnected (new branch from base, no tracking)" },
                    *local,
                ));
            } else {
                lines.push(key_line(theme, "⏎ / r", "Reuse existing local branch", false));
                lines.push(key_line(theme, "d", "Delete local branch and re-create from base", true));
            }
            lines.push(key_line(theme, "esc / n", "Go back", false));
            ("Branch exists".into(), theme.yellow, "", lines)
        }
        Modal::Delete(d) => {
            let w = &d.view.worktree;
            let name = w.display_name();
            let title = if d.stale { format!("Remove stale entry \"{name}\"") } else if d.branch_only { format!("Delete branch \"{name}\"") } else { format!("Delete worktree \"{name}\"") };
            let mut lines = Vec::new();
            let g = d.view.git.unwrap_or_default();
            if g.dirty > 0 {
                lines.push(Line::styled(format!("⚠ {} uncommitted change{}", g.dirty, if g.dirty == 1 { "" } else { "s" }), theme.fg(theme.yellow)));
            }
            if g.ahead > 0 {
                lines.push(Line::styled(format!("⚠ {} commit{} not pushed", g.ahead, if g.ahead == 1 { "" } else { "s" }), theme.fg(theme.yellow)));
            }
            if !lines.is_empty() {
                lines.push(Line::raw(""));
            }
            match d.step {
                DeleteStep::Confirm => {
                    lines.push(Line::raw(if d.stale {
                        format!("Remove stale entry and delete local branch {}?", w.branch)
                    } else if d.branch_only {
                        format!("Delete branch {} and switch back to the default branch?", w.branch)
                    } else {
                        "Remove this worktree from disk?".to_string()
                    }));
                    lines.push(Line::raw(""));
                    lines.push(key_line(theme, "⏎ / y", "Yes", true));
                    lines.push(key_line(theme, "n / esc", "Cancel", false));
                }
                DeleteStep::LocalBranch => {
                    lines.push(Line::raw(format!("Also delete local branch {}?", w.branch)));
                    lines.push(Line::raw(""));
                    lines.push(key_line(theme, "⏎ / y", "Yes", true));
                    lines.push(key_line(theme, "n", "No, keep it", false));
                    lines.push(key_line(theme, "esc", "Cancel", false));
                }
            }
            (title, if g.dirty > 0 || g.ahead > 0 { theme.yellow } else { theme.red }, "", lines)
        }
        Modal::AddRepo { path, error } => {
            let mut lines = vec![
                Line::raw("Path to a git repository:"),
                Line::from(vec![Span::styled("› ", theme.bold(theme.border_focus)), Span::raw(format!("{path}▏"))]),
            ];
            if let Some(e) = error {
                lines.push(Line::raw(""));
                lines.push(Line::styled(e.clone(), theme.fg(theme.red)));
            }
            ("Add repository".into(), theme.border_focus, "tab complete  ⏎ add  esc cancel", lines)
        }
        Modal::Suggest { title, body, on_body, error } => {
            let row = |label: &str, value: &str, active: bool, placeholder: &str| {
                Line::from(vec![
                    Span::styled(if active { "▌" } else { " " }, theme.bold(theme.border_focus)),
                    Span::styled(format!("{label:<13}"), if active { theme.bold(theme.text) } else { theme.faint() }),
                    if value.is_empty() && !active {
                        Span::styled(placeholder.to_string(), theme.faint())
                    } else {
                        Span::raw(format!("{value}{}", if active { "▏" } else { "" }))
                    },
                ])
            };
            let mut lines = vec![
                Line::styled("Opens an issue on roylet-gs/agent-monitor via gh.", theme.faint()),
                Line::raw(""),
                row("Title", title, !*on_body, "(required)"),
                row("Description", body, *on_body, "(optional)"),
            ];
            if let Some(e) = error {
                lines.push(Line::raw(""));
                lines.push(Line::styled(e.clone(), theme.fg(theme.red)));
            }
            ("Suggest a feature".into(), theme.border_focus, "tab next field  ⏎ submit  esc cancel", lines)
        }
        Modal::RunScript { repo_id, .. } => {
            let path = am_core::worktrees::script_path(repo_id);
            (
                "Startup script".into(),
                theme.green,
                "",
                vec![
                    Line::raw("A startup script exists for this repository:"),
                    Line::styled(path.display().to_string(), theme.faint()),
                    Line::raw(""),
                    key_line(theme, "⏎ / y", "Run script", false),
                    key_line(theme, "esc / n", "Skip", false),
                ],
            )
        }
    };
    let rect = modal_rect(area, 72, lines.len() as u16 + 2);
    f.render_widget(Clear, rect);
    f.render_widget(Paragraph::new(lines).block(modal_block(theme, title, color, hint)), rect);
}

#[allow(clippy::needless_late_init)] // each arm builds lines then picks its hint
fn draw_setup(f: &mut Frame, theme: &Theme, area: Rect, s: &crate::setup::Setup) {
    use crate::setup::{IDES, Step};
    let steps = [(Step::Editor, "Editor"), (Step::Hooks, "Claude hooks"), (Step::Repos, "Repositories"), (Step::Integrations, "Integrations")];
    let mut crumbs = vec![Span::raw(" ")];
    let cur = steps.iter().position(|(st, _)| *st == s.step).unwrap_or(0);
    for (i, (_, name)) in steps.iter().enumerate() {
        if i > 0 {
            crumbs.push(Span::styled("  ›  ", theme.faint()));
        }
        let style = if i == cur { theme.bold(theme.border_focus) } else if i < cur { theme.fg(theme.green) } else { theme.faint() };
        crumbs.push(Span::styled(format!("{} {name}", if i < cur { "✓" } else { "" }).trim().to_string(), style));
    }
    let mut lines = vec![Line::from(crumbs), Line::raw("")];
    let hint;
    match s.step {
        Step::Editor => {
            lines.push(Line::raw("Where should Enter open a worktree?"));
            lines.push(Line::raw(""));
            for (i, (_, label)) in IDES.iter().enumerate() {
                let sel = i == s.ide;
                lines.push(Line::from(vec![
                    Span::styled(if sel { "  ● " } else { "  ○ " }, theme.fg(if sel { theme.border_focus } else { theme.faint })),
                    Span::styled(*label, if sel { theme.bold(theme.text) } else { theme.fg(theme.text) }),
                ]));
            }
            hint = "↑↓ choose  ⏎ next";
        }
        Step::Hooks => {
            lines.push(Line::raw("am learns what each agent is doing from Claude Code hooks"));
            lines.push(Line::raw("(written to ~/.claude/settings.json; other hooks are kept)."));
            lines.push(Line::raw(""));
            if s.hooks_installed {
                lines.push(Line::styled("✓ Hooks are installed", theme.fg(theme.green)));
            } else {
                lines.push(Line::styled("Hooks are not installed yet", theme.fg(theme.yellow)));
            }
            lines.push(Line::styled(
                match &s.hook_binary {
                    Some(b) => format!("Native hook binary: {b} (~2 ms per event)"),
                    None => "Native hook binary not found — falling back to `am hook-event`".into(),
                },
                theme.faint(),
            ));
            if let Some(e) = &s.repo_error {
                lines.push(Line::styled(e.clone(), theme.fg(theme.red)));
            }
            hint = if s.hooks_installed { "⏎ next  esc back" } else { "⏎/y install  n skip  esc back" };
        }
        Step::Repos => {
            lines.push(Line::raw("Add the git repositories whose worktrees you want to watch."));
            lines.push(Line::raw(""));
            for r in &s.repos_added {
                lines.push(Line::styled(format!("  ✓ {r}"), theme.fg(theme.green)));
            }
            lines.push(Line::from(vec![Span::styled("› ", theme.bold(theme.border_focus)), Span::raw(format!("{}▏", s.repo_input))]));
            if let Some(e) = &s.repo_error {
                lines.push(Line::styled(e.clone(), theme.fg(theme.red)));
            }
            hint = "tab complete  ⏎ add (empty: next)  esc back";
        }
        Step::Integrations => {
            lines.push(Line::raw("Optional: show PRs/CI and Linear tickets next to each branch."));
            lines.push(Line::raw(""));
            let f0 = s.integration_field == 0;
            lines.push(Line::from(vec![
                Span::styled(if f0 { "▌" } else { " " }, theme.bold(theme.border_focus)),
                Span::raw(format!(" [{}] GitHub PR & CI status (via gh)", if s.gh { "x" } else { " " })),
            ]));
            let masked = "•".repeat(s.linear_key.chars().count());
            lines.push(Line::from(vec![
                Span::styled(if f0 { " " } else { "▌" }, theme.bold(theme.border_focus)),
                Span::raw(" Linear API key: "),
                Span::styled(if masked.is_empty() && f0 { "(optional)".to_string() } else { format!("{masked}{}", if f0 { "" } else { "▏" }) }, theme.faint()),
            ]));
            hint = "tab field  space toggle  ⏎ finish  esc back";
        }
    }
    let rect = modal_rect(area, 76, lines.len() as u16 + 4);
    f.render_widget(Clear, rect);
    let block = modal_block(theme, "Welcome to agent-monitor".into(), theme.border_focus, hint).padding(ratatui::widgets::Padding::new(1, 1, 1, 0));
    f.render_widget(Paragraph::new(lines).block(block), rect);
}

fn draw_log(f: &mut Frame, theme: &Theme, area: Rect, lines: &[String]) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.fg(theme.border))
        .title(Line::from(vec![Span::styled("─", theme.fg(theme.border)), Span::styled("[w] Log ", theme.fg(theme.text))]));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let take = inner.height as usize;
    let rendered: Vec<Line> = lines[lines.len().saturating_sub(take)..]
        .iter()
        .map(|l| {
            let style = if l.contains("[ERROR]") {
                theme.fg(theme.red)
            } else if l.contains("[WARN]") {
                theme.fg(theme.yellow)
            } else if l.contains("[INFO]") {
                theme.fg(theme.text)
            } else {
                theme.faint()
            };
            // Drop the date part of the timestamp to save width.
            let short = l.get(12..).filter(|_| l.starts_with('[')).unwrap_or(l);
            Line::styled(truncate(short, inner.width as usize), style)
        })
        .collect();
    f.render_widget(Paragraph::new(rendered), inner);
}

fn draw_popup(f: &mut Frame, theme: &Theme, area: Rect, msg: &str) {
    let w = 72.min(area.width.saturating_sub(4));
    let inner_w = w.saturating_sub(4) as usize;
    let mut lines: Vec<Line> = Vec::new();
    for raw in msg.lines() {
        let style = if raw.starts_with("claude") || raw.starts_with("cd ") { theme.bold(theme.accent) } else { theme.fg(theme.text) };
        lines.push(Line::styled(truncate(raw, inner_w), style));
    }
    let h = (lines.len() as u16 + 2).min(area.height);
    let rect = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
    let error = msg.starts_with('✗');
    let border = if error { theme.bold(theme.red) } else { theme.bold(theme.green) };
    f.render_widget(Clear, rect);
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(border).padding(ratatui::widgets::Padding::horizontal(1));
    f.render_widget(Paragraph::new(lines).block(block), rect);
}

fn pane_block<'a>(theme: &Theme, title: Line<'a>, focused: bool) -> Block<'a> {
    let border = if focused { theme.bold(theme.border_focus) } else { theme.fg(theme.border) };
    Block::bordered().border_type(BorderType::Rounded).border_style(border).title(title)
}

fn title<'a>(n: u8, name: &'a str, extra: Option<String>, theme: &Theme, focused: bool) -> Line<'a> {
    let main = if focused { theme.bold(theme.border_focus) } else { theme.fg(theme.text) };
    let mut spans = vec![
        Span::styled("─", if focused { theme.bold(theme.border_focus) } else { theme.fg(theme.border) }),
        Span::styled(format!("[{n}] "), theme.faint()),
        Span::styled(name, main),
    ];
    if let Some(e) = extra {
        spans.push(Span::styled(format!(" · {e}"), theme.faint()));
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

// ---------------------------------------------------------------- worktrees

struct Columns {
    ticket: usize,
    pr: usize,
    git: usize,
    age: usize,
}

fn draw_worktrees(f: &mut Frame, app: &App, theme: &Theme, area: Rect, now: i64) {
    let focused = app.focus == Pane::Worktrees;
    let mut block = pane_block(theme, title(1, "Worktrees", None, theme, focused), focused);
    if !app.flat.is_empty() {
        block = block.title_bottom(
            Line::styled(format!(" {} of {} ", app.selected + 1, app.flat.len()), theme.faint()).alignment(Alignment::Right),
        );
    }
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.flat.is_empty() {
        let msg = if app.loaded { "No worktrees yet — press A to add a repository" } else { "Loading…" };
        f.render_widget(Paragraph::new(Line::styled(msg, theme.faint())).alignment(Alignment::Center), centered_line(inner));
        return;
    }

    let s = &app.settings;
    let cols = Columns {
        ticket: if s.show_linear_ticket { app.flat.iter().map(|v| ticket_text(v).width()).max().unwrap_or(0) } else { 0 },
        pr: if s.show_pr_status { app.flat.iter().map(|v| pr_text(v).0.width()).max().unwrap_or(0) } else { 0 },
        git: if s.show_git_ahead_behind { app.flat.iter().map(|v| git_text(v).width()).max().unwrap_or(0) } else { 0 },
        age: if s.show_last_commit { 3 } else { 0 },
    };

    // Keep the selected row in view; headers scroll with their rows.
    let height = inner.height as usize;
    let sel_row = app.rows.iter().position(|r| *r == Row::Worktree(app.selected)).unwrap_or(0);
    let mut start = sel_row.saturating_sub(height.saturating_sub(1));
    // Show the header above the selection when there's room.
    if start > 0 && sel_row - start < height.saturating_sub(1) {
        start = start.saturating_sub(1);
    }
    let lines: Vec<Line> = app
        .rows
        .iter()
        .skip(start)
        .take(height)
        .map(|row| match row {
            Row::Spacer => Line::raw(""),
            // Repo header: name followed by a rule to the pane edge.
            Row::Repo(name) => {
                let rule = (inner.width as usize).saturating_sub(name.width() + 1);
                Line::from(vec![
                    Span::styled(format!("{name} "), theme.bold(theme.accent)),
                    Span::styled("─".repeat(rule), theme.fg(theme.border)),
                ])
            }
            Row::Project(name) => Line::from(vec![
                Span::styled("═ ", theme.fg(theme.magenta)),
                Span::styled(name.clone(), theme.bold(theme.magenta)),
                Span::styled(" ═", theme.fg(theme.magenta)),
            ]),
            Row::Ticket(t) => Line::styled(format!("  — {} —", truncate(t, inner.width as usize - 6)), theme.faint()),
            Row::Worktree(i) => worktree_line(app, theme, &app.flat[*i], *i == app.selected, focused, &cols, inner.width, now),
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

#[allow(clippy::too_many_arguments)]
fn worktree_line<'a>(
    app: &App,
    theme: &Theme,
    v: &'a WorktreeView,
    selected: bool,
    focused: bool,
    cols: &Columns,
    width: u16,
    now: i64,
) -> Line<'a> {
    let status = v.status();
    let subagents = app.running_subagents_for(&v.worktree.id);

    // Right-hand columns, fixed width so they align down the list.
    let mut right: Vec<Span> = Vec::new();
    if cols.ticket > 0 {
        right.push(Span::raw(" "));
        right.push(Span::styled(pad(&ticket_text(v), cols.ticket), theme.fg(theme.blue)));
    }
    if cols.pr > 0 {
        let (text, style) = pr_text(v);
        right.push(Span::raw(" "));
        right.push(Span::styled(pad(&text, cols.pr), style.map(|c| theme.fg(c(theme))).unwrap_or_default()));
    }
    if cols.git > 0 {
        right.push(Span::raw(" "));
        right.extend(git_spans(v, theme, cols.git));
    }
    if cols.age > 0 {
        let age = v.last_commit.as_ref().map(|c| timefmt::short_age(now - c.timestamp)).unwrap_or_default();
        right.push(Span::raw(" "));
        right.push(Span::styled(format!("{age:>3}"), theme.faint()));
    }
    right.push(Span::raw(" "));
    let right_w: usize = right.iter().map(|s| s.content.width()).sum();

    let bar = if selected { if focused { "▌" } else { "▏" } } else { " " };
    let mut left = vec![
        Span::styled(bar, theme.bold(theme.border_focus)),
        Span::styled(status_glyph(status), theme.fg(theme.status(status))),
        Span::raw(" "),
    ];
    let mut badge = String::new();
    if app.unseen.contains(&v.worktree.id) {
        badge.push_str(" •");
    }
    if !v.running_processes.is_empty() {
        badge.push_str(" ▶");
    }
    if subagents > 0 {
        badge.push_str(&format!(" ◆{subagents}"));
    }
    let name_w = (width as usize).saturating_sub(right_w + 3 + badge.width());
    let name = truncate(v.worktree.display_name(), name_w);
    let name_style = if status == Some(Status::Waiting) {
        theme.bold(theme.yellow)
    } else if selected {
        theme.bold(theme.text)
    } else {
        theme.fg(theme.text)
    };
    left.push(Span::styled(pad(&name, name_w), name_style));
    let mut rest = badge.as_str();
    if let Some(r) = rest.strip_prefix(" •") {
        left.push(Span::styled(" •", theme.bold(theme.accent)));
        rest = r;
    }
    if let Some(r) = rest.strip_prefix(" ▶") {
        left.push(Span::styled(" ▶", theme.fg(theme.green)));
        rest = r;
    }
    left.push(Span::styled(rest.to_string(), theme.fg(theme.magenta)));

    let mut spans = left;
    spans.extend(right);
    let line = Line::from(spans);
    if selected && focused {
        if theme.name == "ansi" {
            // Reverse video swaps fg/bg, so drop span colours to stay legible.
            let plain: Vec<Span> = line.spans.into_iter().map(|s| Span::raw(s.content)).collect();
            return Line::from(plain).style(theme.selected().add_modifier(Modifier::BOLD));
        }
        return line.style(theme.selected());
    }
    line
}

fn ticket_text(v: &WorktreeView) -> String {
    v.linear.as_ref().map(|l| l.identifier.clone()).unwrap_or_default()
}

type ColorPick = fn(&Theme) -> ratatui::style::Color;

fn pr_text(v: &WorktreeView) -> (String, Option<ColorPick>) {
    let Some(pr) = &v.pr else { return (String::new(), None) };
    let checks = match pr.checks_status {
        ChecksStatus::Passing => "✓",
        ChecksStatus::Failing => "✗",
        ChecksStatus::Pending => "◌",
        ChecksStatus::None => "",
    };
    let color: ColorPick = match (pr.state.as_str(), pr.is_draft, pr.checks_status) {
        ("MERGED", ..) => |t| t.magenta,
        ("CLOSED", ..) => |t| t.faint,
        (_, true, _) => |t| t.faint,
        (_, _, ChecksStatus::Failing) => |t| t.red,
        (_, _, ChecksStatus::Pending) => |t| t.yellow,
        _ if pr.review_decision == "CHANGES_REQUESTED" => |t| t.yellow,
        _ => |t| t.green,
    };
    let merged = if pr.state == "MERGED" { "⇡" } else { "" };
    (format!("#{}{checks}{merged}", pr.number), Some(color))
}

fn git_text(v: &WorktreeView) -> String {
    git_parts(v).into_iter().map(|(s, _)| s).collect::<Vec<_>>().join(" ")
}

fn git_parts(v: &WorktreeView) -> Vec<(String, u8)> {
    let Some(g) = v.git else { return Vec::new() };
    let mut parts = Vec::new();
    if g.ahead > 0 {
        parts.push((format!("↑{}", g.ahead), 0));
    }
    if g.behind > 0 {
        parts.push((format!("↓{}", g.behind), 1));
    }
    if g.dirty > 0 {
        parts.push((format!("±{}", g.dirty), 2));
    }
    parts
}

fn git_spans<'a>(v: &WorktreeView, theme: &Theme, width: usize) -> Vec<Span<'a>> {
    let parts = git_parts(v);
    let used: usize = parts.iter().map(|(s, _)| s.width()).sum::<usize>() + parts.len().saturating_sub(1);
    let mut spans = vec![Span::raw(" ".repeat(width.saturating_sub(used)))];
    for (i, (s, kind)) in parts.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        let c = match kind {
            0 => theme.green,
            1 => theme.yellow,
            _ => theme.faint,
        };
        spans.push(Span::styled(s, theme.fg(c)));
    }
    spans
}

// ---------------------------------------------------------------- subagents

fn draw_subagents(f: &mut Frame, app: &App, theme: &Theme, area: Rect, now: i64) {
    let focused = app.focus == Pane::Subagents;
    let items = app.visible_subagents();
    let running = items.iter().filter(|(_, s)| s.running).count();
    let done = items.len() - running;
    let scope = match app.scope {
        Scope::All => "all worktrees".to_string(),
        Scope::Selected => app.selected_view().map(|v| v.worktree.display_name().to_string()).unwrap_or_default(),
    };
    let block = pane_block(theme, title(2, "Subagents", Some(scope).filter(|s| !s.is_empty()), theme, focused), focused).title_bottom(
        Line::styled(
            format!(" {running} running · {done} done · a: {} ", if app.scope == Scope::All { "this worktree" } else { "all" }),
            theme.faint(),
        )
        .alignment(Alignment::Right),
    );
    let inner = block.inner(area);
    f.render_widget(block, area);

    if items.is_empty() {
        f.render_widget(
            Paragraph::new(Line::styled("No subagents running", theme.faint())).alignment(Alignment::Center),
            centered_line(inner),
        );
        return;
    }

    let names: std::collections::HashMap<&str, &str> =
        app.flat.iter().map(|v| (v.worktree.id.as_str(), v.worktree.display_name())).collect();
    let width = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut sel_line = 0;
    let mut last_wt: Option<&str> = None;
    for (i, (wt, s)) in items.iter().enumerate() {
        if app.scope == Scope::All && last_wt != wt.as_deref() {
            last_wt = wt.as_deref();
            let name = wt.as_deref().and_then(|w| names.get(w).copied()).unwrap_or("(hidden worktree)");
            lines.push(Line::styled(name.to_string(), theme.bold(theme.accent)));
        }
        let selected = focused && i == app.sub_selected;
        if selected {
            sel_line = lines.len();
        }
        let [a, b] = subagent_lines(s, theme, width, now, selected);
        lines.push(a);
        lines.push(b);
    }
    let height = inner.height as usize;
    let start = (sel_line + 2).saturating_sub(height);
    f.render_widget(Paragraph::new(lines.into_iter().skip(start).take(height).collect::<Vec<_>>()), inner);
}

fn subagent_lines<'a>(s: &'a Subagent, theme: &Theme, width: usize, now: i64, selected: bool) -> [Line<'a>; 2] {
    let started = timefmt::parse_sqlite(&s.started_at).unwrap_or(now);
    let (glyph, gstyle, when) = if s.running {
        ("◆", theme.fg(theme.magenta), timefmt::elapsed(now - started))
    } else {
        let ended = s.ended_at.as_deref().and_then(timefmt::parse_sqlite).unwrap_or(now);
        ("✓", theme.fg(theme.teal), format!("done {} ago", timefmt::short_age(now - ended)))
    };
    let kind = s.agent_type.clone().unwrap_or_default();
    let right = format!("{kind}  {when}");
    let name = s
        .description
        .clone()
        .or_else(|| s.agent_type.clone())
        .unwrap_or_else(|| format!("agent {}", &s.agent_id[..s.agent_id.len().min(8)]));
    let name_w = width.saturating_sub(right.width() + 4);
    let name_style = if s.running { theme.bold(theme.text) } else { theme.fg(theme.text) };
    let bar = if selected { Span::styled("▌", theme.bold(theme.border_focus)) } else { Span::raw(" ") };
    let first = Line::from(vec![
        bar,
        Span::styled(glyph, gstyle),
        Span::raw(" "),
        Span::styled(pad(&truncate(&name, name_w), name_w), name_style),
        Span::raw(" "),
        Span::styled(right, theme.faint()),
    ]);

    let tools = format!("{} tools", s.tool_count);
    let mut second = vec![Span::styled("   └ ", theme.faint())];
    let avail = width.saturating_sub(5 + tools.width() + 1);
    match (&s.last_tool, &s.last_tool_input) {
        (Some(tool), input) => {
            let input = input.as_deref().unwrap_or("");
            let tool_w = tool.width().min(avail);
            second.push(Span::styled(truncate(tool, tool_w), theme.fg(theme.accent)));
            let rest = avail.saturating_sub(tool_w + 2);
            second.push(Span::styled(pad(&format!("  {}", truncate(input, rest)), avail - tool_w), theme.faint()));
        }
        (None, _) => second.push(Span::styled(pad(if s.running { "starting…" } else { "" }, avail), theme.faint())),
    }
    second.push(Span::styled(format!(" {tools}"), theme.faint()));
    let second = Line::from(second);
    if selected {
        return [first.style(theme.selected()), second.style(theme.selected())];
    }
    [first, second]
}

// ------------------------------------------------------------ context + bar

fn draw_context(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let Some(v) = app.selected_view() else { return };
    let sep = || Span::styled(" · ", theme.faint());
    let mut spans = vec![Span::raw(" "), Span::styled(v.worktree.branch.clone(), theme.bold(theme.text))];
    if let Some(l) = &v.linear {
        spans.push(sep());
        spans.push(Span::styled(l.identifier.clone(), theme.fg(theme.blue)));
        spans.push(Span::styled(format!(" {}", l.title), theme.fg(theme.text)));
        spans.push(Span::styled(format!(" ({})", l.state.name), theme.faint()));
    }
    if let Some(pr) = &v.pr {
        spans.push(sep());
        let state = if pr.is_draft { "draft".to_string() } else { pr.state.to_lowercase() };
        spans.push(Span::styled(format!("PR #{} {state}", pr.number), theme.fg(theme.text)));
    }
    if let Some(sid) = v.agent.as_ref().and_then(|a| a.session_id.as_ref()) {
        spans.push(sep());
        spans.push(Span::styled(format!("session {sid}"), theme.faint()));
    }
    let mut open_in = Vec::new();
    match v.open_ide {
        Some(am_core::model::Ide::Vscode) => open_in.push("VS Code"),
        Some(am_core::model::Ide::Cursor) => open_in.push("Cursor"),
        None => {}
    }
    if v.has_terminal {
        open_in.push("terminal");
    }
    if !open_in.is_empty() {
        spans.push(sep());
        spans.push(Span::styled(format!("open in {}", open_in.join(" + ")), theme.fg(theme.accent)));
    }
    if !v.running_processes.is_empty() {
        spans.push(sep());
        let procs: Vec<&str> = v.running_processes.iter().map(|p| p.command.as_str()).collect();
        spans.push(Span::styled(format!("▶ {}", procs.join(", ")), theme.fg(theme.green)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_status_bar(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let mode = match app.focus {
        Pane::Worktrees => " WORKTREES ",
        Pane::Subagents => " SUBAGENTS ",
    };
    let pill = Style::new().fg(ratatui::style::Color::Black).bg(theme.border_focus).add_modifier(Modifier::BOLD);
    let mut left = vec![Span::styled(mode, pill), Span::raw(" ")];
    for (s, n) in app.status_counts() {
        left.push(Span::styled(format!("{} {n}  ", status_glyph(Some(s))), theme.fg(theme.status(Some(s)))));
    }
    let h = &app.snapshot.health;
    let offline = match (h.github_failing, h.linear_failing) {
        (true, true) => Some("GitHub + Linear"),
        (true, false) => Some("GitHub"),
        (false, true) => Some("Linear"),
        _ => None,
    };
    if let Some(src) = offline {
        left.push(Span::styled(format!("⚠ {src} offline — cached  "), theme.fg(theme.yellow)));
    }
    if let Some(n) = &app.notice {
        left.push(Span::styled(n.clone(), theme.faint()));
    }
    let hints = if area.width >= 110 { "⏎ open  t term  c claude  a scope  r refresh  ? help  q quit " } else { "⏎ open  ? help  q quit " };
    let [l, r] = Layout::horizontal([Constraint::Min(0), Constraint::Length(hints.width() as u16)]).areas(area);
    f.render_widget(Paragraph::new(Line::from(left)), l);
    f.render_widget(Paragraph::new(Line::styled(hints, theme.faint())), r);
}

fn draw_help(f: &mut Frame, theme: &Theme, area: Rect) {
    let keys: &[(&str, &str)] = &[
        ("j / k  ↑ / ↓", "move"),
        ("gg / G", "top / bottom"),
        ("ctrl-d / ctrl-u", "page down / up"),
        ("enter", "open (IDE; resumes last Claude session)"),
        ("t", "terminal at worktree"),
        ("c", "terminal running claude --resume"),
        ("o / l / p", "open PR / Linear ticket / Linear project"),
        ("w", "toggle log pane"),
        ("n / d", "new worktree / delete worktree"),
        ("A", "add repository"),
        ("f", "suggest a feature (files a GitHub issue)"),
        ("s", "settings"),
        ("tab  1  2", "switch pane"),
        ("a", "subagents: this worktree ↔ all"),
        ("r", "refresh now (bypasses backoff)"),
        ("?", "toggle help"),
        ("q  esc", "quit"),
    ];
    let legend: Vec<Span> = [Status::Executing, Status::Planning, Status::Waiting, Status::Delegating, Status::Done, Status::Idle]
        .into_iter()
        .flat_map(|s| {
            [
                Span::styled(status_glyph(Some(s)), theme.fg(theme.status(Some(s)))),
                Span::styled(format!(" {}  ", status_label(Some(s))), theme.faint()),
            ]
        })
        .collect();
    let mut lines: Vec<Line> = keys
        .iter()
        .map(|(k, d)| Line::from(vec![Span::styled(format!("{k:>16}  "), theme.bold(theme.accent)), Span::raw(*d)]))
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(legend));
    let w = 80.min(area.width.saturating_sub(4));
    let h = (lines.len() as u16 + 2).min(area.height);
    let rect = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.bold(theme.border_focus))
        .title(Line::styled(" Keys ", theme.bold(theme.text)));
    f.render_widget(Paragraph::new(lines).block(block), rect);
}

// ------------------------------------------------------------------ helpers

fn centered_line(r: Rect) -> Rect {
    Rect { y: r.y + r.height / 2, height: 1.min(r.height), ..r }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

fn pad(s: &str, width: usize) -> String {
    let w = s.width();
    if w >= width { s.to_string() } else { format!("{s}{}", " ".repeat(width - w)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Key, Snapshot};
    use am_core::model::*;
    use am_core::settings::Settings;
    use ratatui::{Terminal, backend::TestBackend};

    const NOW: i64 = 1_760_176_800; // 2025-10-11 10:00:00 UTC

    fn view(id: &str, repo: &str, branch: &str, status: Option<Status>) -> WorktreeView {
        WorktreeView {
            worktree: Worktree {
                id: id.into(),
                repo_id: repo.into(),
                path: format!("/r/{id}"),
                branch: branch.into(),
                name: branch.into(),
                custom_name: None,
                is_main: false,
                created_at: "2025-10-01 00:00:00".into(),
            },
            agent: status.map(|s| AgentState {
                status: s,
                session_id: Some("0c5e7f2a-1111-2222-3333-a91f00000000".into()),
                last_response: None,
                is_open: true,
                active_subagents: 0,
                updated_at: "2025-10-11 09:59:00".into(),
            }),
            git: Some(GitStatus { ahead: 2, behind: 0, dirty: 3 }),
            last_commit: Some(CommitInfo { hash: "eaa3440".into(), message: "bump".into(), timestamp: NOW - 240 }),
            pr: None,
            linear: None,
            has_terminal: false,
            open_ide: None,
            running_processes: vec![],
        }
    }

    fn sub(wt: &str, id: &str, desc: &str, running: bool, tool: Option<(&str, &str)>) -> (Option<String>, Subagent) {
        (
            Some(wt.into()),
            Subagent {
                session_id: "s".into(),
                agent_id: id.into(),
                agent_type: Some("general-purpose".into()),
                description: Some(desc.into()),
                running,
                tool_count: 23,
                last_tool: tool.map(|t| t.0.into()),
                last_tool_input: tool.map(|t| t.1.into()),
                started_at: "2025-10-11 09:57:46".into(),
                ended_at: (!running).then(|| "2025-10-11 09:59:30".into()),
                updated_at: "2025-10-11 09:59:59".into(),
                transcript_path: None,
            },
        )
    }

    fn fixture() -> App {
        let mut refactor = view("w1", "r1", "feature/refactor", Some(Status::Delegating));
        refactor.pr = Some(PrInfo {
            number: 127,
            title: "Refactor".into(),
            url: "u".into(),
            state: "OPEN".into(),
            is_draft: false,
            review_decision: String::new(),
            has_feedback: false,
            checks_status: ChecksStatus::Passing,
            active_check_url: None,
            active_check_name: None,
            checks_waiting: false,
        });
        refactor.linear = Some(LinearInfo {
            identifier: "ENG-412".into(),
            title: "Refactor grouping core".into(),
            url: "u".into(),
            state: LinearState { name: "In Progress".into(), color: String::new(), kind: "started".into() },
            priority_label: String::new(),
            assignee: None,
            project: None,
            pr_attachment: None,
        });
        let mut waiting = view("w2", "r1", "fix/offline-cache", Some(Status::Waiting));
        waiting.git = Some(GitStatus::default());
        let main = view("w3", "r2", "main", None);
        let snapshot = Snapshot {
            repos: vec![],
            groups: vec![
                WorktreeGroup {
                    repo: Repository { id: "r1".into(), path: "/r1".into(), name: "agent-monitor".into() },
                    worktrees: vec![refactor, waiting],
                },
                WorktreeGroup {
                    repo: Repository { id: "r2".into(), path: "/r2".into(), name: "gridsight".into() },
                    worktrees: vec![main],
                },
            ],
            subagents: vec![
                sub("w1", "a1", "Analyze am debug logs", true, Some(("Bash", "grep -c WARN debug.log"))),
                sub("w1", "a2", "Research TUI styles", true, Some(("WebFetch", "github.com/dlvhdr/gh-dash"))),
                sub("w1", "a3", "Research languages", false, Some(("SubagentHandback", ""))),
            ],
            health: Default::default(),
        };
        let settings = Settings { worktree_sort: vec![], ..Settings::default() };
        let mut app = App::new(settings);
        app.apply(snapshot);
        app
    }

    fn render(app: &App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app, &Theme::by_name("ansi"), NOW, &[])).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn dashboard_wide() {
        insta::assert_snapshot!(render(&fixture(), 120, 16));
    }

    #[test]
    fn dashboard_narrow_stacks_panes() {
        insta::assert_snapshot!(render(&fixture(), 80, 24));
    }

    #[test]
    fn global_subagent_feed_and_focus() {
        let mut app = fixture();
        app.on_key(Key::Char('a'));
        app.on_key(Key::Tab);
        app.on_key(Key::Char('j'));
        insta::assert_snapshot!(render(&app, 120, 16));
    }

    #[test]
    fn help_overlay() {
        let mut app = fixture();
        app.on_key(Key::Char('?'));
        insta::assert_snapshot!(render(&app, 100, 20));
    }

    #[test]
    fn offline_hint_and_empty_state() {
        let mut app = App::new(Settings::default());
        app.apply(Snapshot { health: crate::app::Health { github_failing: true, linear_failing: false }, ..Default::default() });
        insta::assert_snapshot!(render(&app, 100, 8));
    }

    #[test]
    fn selection_follows_worktree_across_refreshes() {
        let mut app = fixture();
        app.on_key(Key::Char('j'));
        assert_eq!(app.selected_view().unwrap().worktree.id, "w2");
        let mut snap = app.snapshot.clone();
        snap.groups[0].worktrees.reverse();
        app.apply(snap);
        assert_eq!(app.selected_view().unwrap().worktree.id, "w2");
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn status_changes_mark_unseen_and_queue_sounds() {
        let mut app = fixture();
        app.settings.audio_notifications = true;
        assert!(app.unseen.is_empty() && app.sounds.is_empty(), "first load is silent");
        let mut snap = app.snapshot.clone();
        // w2 (not selected) finishes; w1 (selected) starts waiting.
        snap.groups[0].worktrees[1].agent.as_mut().unwrap().status = Status::Done;
        snap.groups[0].worktrees[0].agent.as_mut().unwrap().status = Status::Waiting;
        app.apply(snap);
        assert!(app.unseen.contains("w2"));
        assert!(!app.unseen.contains("w1"), "the selected worktree is seen immediately");
        assert_eq!(app.sounds, vec!["Glass".to_string(), "Funk".to_string()]);
        app.on_key(Key::Char('j'));
        assert!(!app.unseen.contains("w2"), "selecting marks it seen");
    }

    #[test]
    fn link_keys() {
        let mut app = fixture();
        assert_eq!(app.on_key(Key::Char('o')), crate::app::Action::OpenUrl("u".into()));
        app.on_key(Key::Char('j'));
        assert_eq!(app.on_key(Key::Char('o')), crate::app::Action::None);
        assert_eq!(app.notice.as_deref(), Some("No PR for this branch"));
    }

    #[test]
    fn new_worktree_modal_renders() {
        let mut app = fixture();
        app.snapshot.repos = vec![
            Repository { id: "r1".into(), path: "/r1".into(), name: "agent-monitor".into() },
            Repository { id: "r2".into(), path: "/r2".into(), name: "gridsight".into() },
        ];
        app.on_key(Key::Char('n'));
        for c in "x".chars() {
            app.on_key(Key::Char(c));
        }
        if let Some(m) = &mut app.modal {
            m.on_check_result("feature/x".into(), am_core::worktrees::BranchCheck { local: false, remote: Some(true) });
        }
        insta::assert_snapshot!(render(&app, 100, 16));
    }

    #[test]
    fn delete_modal_warns_about_dirty_and_unpushed() {
        let mut app = fixture();
        app.on_key(Key::Char('d'));
        insta::assert_snapshot!(render(&app, 100, 16));
    }

    #[test]
    fn truncate_respects_display_width() {
        assert_eq!(truncate("feature/refactor", 8), "feature…");
        assert_eq!(truncate("short", 8), "short");
    }
}
