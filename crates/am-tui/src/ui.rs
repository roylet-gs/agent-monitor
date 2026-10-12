//! Rendering. lazygit-style titled panes: `[1] Worktrees` and `[2] Subagents`,
//! a context line for the selection, and a status bar with a mode pill.
//! ratatui diffs each frame against the previous one cell by cell, so a
//! redraw only writes what changed — no full-screen clears, no flicker.

use am_core::model::{ChecksStatus, WorktreeView};
use am_core::status::Status;
use am_core::timefmt;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Row};
use crate::theme::{Theme, status_glyph, status_label};


pub fn draw(f: &mut Frame, app: &App, theme: &Theme, now: i64, log_lines: &[String]) {
    // The theme builder previews its draft on the whole dashboard.
    let draft;
    let theme = match app.settings_editor.as_ref().and_then(|e| e.builder.as_ref()) {
        Some(b) => {
            draft = b.theme();
            &draft
        }
        None => theme,
    };
    let area = f.area();
    if theme.background.is_some() {
        f.render_widget(Block::new().style(theme.base()), area);
    }
    let log_h = if app.show_log { (area.height / 3).max(5) } else { 0 };
    let [body, log, keys] =
        Layout::vertical([Constraint::Min(5), Constraint::Length(log_h), Constraint::Length(1)]).areas(area);
    draw_keybar(f, app, theme, keys);
    if app.show_log {
        draw_log(f, theme, log, log_lines);
    }

    draw_worktrees(f, app, theme, body, now);
    if app.show_help {
        draw_help(f, theme, area);
    }
    if let Some(editor) = &app.settings_editor {
        match &editor.builder {
            Some(b) => draw_theme_builder(f, theme, area, b),
            None => draw_settings(f, theme, area, editor),
        }
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
    clear(f, theme, rect);
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

/// One colour as a swatch: a filled block, or a word for the special values.
fn swatch(theme: &Theme, c: am_core::themes::ThemeColor) -> Span<'static> {
    use am_core::themes::ThemeColor;
    match c {
        ThemeColor::Reverse => Span::styled("▒▒", theme.fg(theme.text).add_modifier(Modifier::REVERSED)),
        ThemeColor::Default => Span::styled("··", theme.faint()),
        other => Span::styled("██", theme.fg(crate::theme::to_color(other))),
    }
}

fn draw_theme_builder(f: &mut Frame, theme: &Theme, area: Rect, b: &crate::theme_builder::ThemeBuilder) {
    use crate::theme_builder::{COLS, Mode};
    use am_core::themes::Token;
    let w = 100.min(area.width.saturating_sub(2));
    let h = 24.min(area.height.saturating_sub(2));
    let rect = modal_rect(area, w, h);
    clear(f, theme, rect);
    let hint = match b.mode {
        Mode::Tokens => " ⏎ palette  ←→ nudge  # hex  x inherit  b base  n name  w save  esc close ",
        Mode::Picker { .. } => " arrows move  ⏎ pick  # hex  esc back ",
        Mode::Hex(_) | Mode::Name(_) => " ⏎ apply  esc cancel ",
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.bold(theme.border_focus))
        .title(Line::from(vec![Span::styled("─", theme.bold(theme.border_focus)), Span::styled("Theme builder ", theme.bold(theme.text))]))
        .title_bottom(Line::styled(hint, theme.faint()).alignment(Alignment::Right));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let [head, body, foot] = Layout::vertical([Constraint::Length(2), Constraint::Min(3), Constraint::Length(2)]).areas(inner);
    let [left, right] = Layout::horizontal([Constraint::Length(46), Constraint::Min(20)]).areas(body);

    let name = match &b.mode {
        Mode::Name(buf) => Span::styled(format!("{buf}▏"), theme.bold(theme.accent)),
        _ => Span::styled(b.name.clone(), theme.bold(theme.text)),
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Name ", theme.faint()),
            name,
            Span::styled("   Base ", theme.faint()),
            Span::styled(b.base.clone(), theme.fg(theme.text)),
            Span::styled(if b.dirty { "   • unsaved" } else { "" }, theme.fg(theme.yellow)),
        ])),
        head,
    );

    let mut lines: Vec<Line> = Vec::new();
    for (i, t) in Token::ALL.iter().enumerate() {
        let (c, own) = b.color(*t);
        let sel = i == b.cursor;
        let value = match (&b.mode, sel) {
            (Mode::Hex(buf), true) => Span::styled(format!("{buf}▏"), theme.bold(theme.accent)),
            _ => Span::styled(format!("{c:<12}"), if own { theme.fg(theme.text) } else { theme.faint() }),
        };
        let line = Line::from(vec![
            Span::styled(if sel { "▌" } else { " " }, theme.bold(theme.border_focus)),
            Span::styled(format!(" {:<14}", t.key()), theme.fg(theme.text)),
            swatch(theme, c),
            Span::raw(" "),
            value,
            Span::styled(if own { "" } else { " base" }, theme.faint()),
        ]);
        lines.push(if sel && matches!(b.mode, Mode::Tokens | Mode::Hex(_)) { line.style(theme.selected()) } else { line });
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(format!("  {}: {}", b.token().key(), b.token().describe()), theme.faint()));
    f.render_widget(Paragraph::new(lines), left);

    if let Mode::Picker { cursor } = b.mode {
        let pal = b.palette();
        let mut lines = vec![Line::styled(format!(" {} — pick a colour", b.token().key()), theme.bold(theme.accent))];
        for (row, chunk) in pal.chunks(COLS).enumerate() {
            if row == 2 || row == 7 {
                lines.push(Line::raw(""));
            }
            let mut spans = vec![Span::raw(" ")];
            for (col, c) in chunk.iter().enumerate() {
                let here = row * COLS + col == cursor;
                spans.push(Span::styled(if here { "[" } else { " " }, theme.bold(theme.text)));
                spans.push(swatch(theme, *c));
                spans.push(Span::styled(if here { "]" } else { " " }, theme.bold(theme.text)));
            }
            lines.push(Line::from(spans));
        }
        lines.push(Line::raw(""));
        if let Some(c) = pal.get(cursor) {
            lines.push(Line::from(vec![Span::styled(" ", theme.faint()), swatch(theme, *c), Span::styled(format!(" {c}"), theme.fg(theme.text))]));
        }
        lines.push(Line::styled(" rows: terminal colours · curated · this theme", theme.faint()));
        f.render_widget(Paragraph::new(lines), right);
    } else {
        f.render_widget(Paragraph::new(theme_preview(theme)).block(pane_block(theme, title("Preview", theme), false)), right);
    }

    let msg = match &b.error {
        Some(e) => Line::styled(format!(" {e}"), theme.fg(theme.red)),
        None => Line::styled(" Changes preview on the whole dashboard; w saves to themes/<name>.json and selects it", theme.faint()),
    };
    f.render_widget(Paragraph::new(vec![Line::raw(""), msg]).wrap(Wrap { trim: false }), foot);
}

/// A few representative dashboard rows in the given theme.
fn theme_preview(theme: &Theme) -> Vec<Line<'static>> {
    let row = |status: Status, name: &str, extra: Vec<Span<'static>>| {
        let mut spans = vec![
            Span::raw(" "),
            Span::styled(status_glyph(Some(status)), theme.fg(theme.status(Some(status)))),
            Span::styled(format!(" {name:<16}"), theme.fg(theme.text)),
        ];
        spans.extend(extra);
        Line::from(spans)
    };
    let mut selected = row(Status::Waiting, "fix/crash", vec![Span::styled("ENG-7  ", theme.fg(theme.blue)), Span::styled("#41✗ changes", theme.fg(theme.red))]);
    selected.spans.insert(0, Span::styled("▌", theme.bold(theme.border_focus)));
    selected.spans.remove(1);
    vec![
        Line::from(vec![Span::styled(" work ", theme.bold(theme.text)), Span::styled("───", theme.fg(theme.border))]),
        Line::from(vec![Span::styled(" ═ ", theme.fg(theme.magenta)), Span::styled("Dashboard", theme.bold(theme.magenta)), Span::styled(" ═", theme.fg(theme.magenta))]),
        row(Status::Executing, "feature/login", vec![Span::styled("ENG-123 ", theme.fg(theme.blue)), Span::styled("#42✓ ready", theme.fg(theme.green))]),
        selected.style(theme.selected()),
        row(Status::Delegating, "main", vec![Span::styled("◆ 1 subagent", theme.fg(theme.accent))]),
        row(Status::Planning, "spike/api", vec![Span::styled("#43 draft", theme.faint())]),
        row(Status::Done, "chore/deps", vec![Span::styled("#40⇡", theme.fg(theme.magenta)), Span::styled("  ↑2 ±3", theme.fg(theme.yellow))]),
        row(Status::Idle, "old/branch", vec![Span::styled("3d ago", theme.faint())]),
        Line::raw(""),
        Line::styled(" [WARN] gh offline — cached", theme.fg(theme.yellow)),
        Line::styled(" [ERROR] fetch failed", theme.fg(theme.red)),
    ]
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
        Modal::Busy { text, deleting } => {
            let line = Line::from(vec![Span::styled(format!("{} ", spinner(now)), theme.fg(theme.accent)), Span::raw(text.clone())]);
            let hint = if deleting.is_some() { "any key: continue in background" } else { "" };
            ("Working".into(), theme.accent, hint, vec![line])
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
    // Long names wrap rather than run off the edge; size the box to fit.
    let w = 72.min(area.width.saturating_sub(2));
    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    let h = para.line_count(w.saturating_sub(4)) as u16 + 2;
    let rect = modal_rect(area, w, h);
    clear(f, theme, rect);
    f.render_widget(para.block(modal_block(theme, title, color, hint)), rect);
}

fn spinner(now: i64) -> &'static str {
    const SPIN: [&str; 4] = ["◐", "◓", "◑", "◒"];
    SPIN[now.rem_euclid(4) as usize]
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
    clear(f, theme, rect);
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
    let mut lines: Vec<Line> = Vec::new();
    for raw in msg.lines() {
        let style = if raw.starts_with("claude") || raw.starts_with("cd ") { theme.bold(theme.accent) } else { theme.fg(theme.text) };
        lines.push(Line::styled(raw.to_string(), style));
    }
    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    let h = (para.line_count(w.saturating_sub(4)) as u16 + 2).min(area.height);
    let rect = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
    let error = msg.starts_with('✗');
    let border = if error { theme.bold(theme.red) } else { theme.bold(theme.green) };
    clear(f, theme, rect);
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(border).padding(ratatui::widgets::Padding::horizontal(1));
    f.render_widget(para.block(block), rect);
}

fn pane_block<'a>(theme: &Theme, title: Line<'a>, focused: bool) -> Block<'a> {
    let border = if focused { theme.bold(theme.border_focus) } else { theme.fg(theme.border) };
    Block::bordered().border_type(BorderType::Rounded).border_style(border).title(title)
}

fn title<'a>(name: &'a str, theme: &Theme) -> Line<'a> {
    Line::from(vec![
        Span::styled("─", theme.bold(theme.border_focus)),
        Span::styled(name, theme.bold(theme.border_focus)),
        Span::raw(" "),
    ])
}

// ---------------------------------------------------------------- worktrees

/// Below this inner width the ticket, git and age columns are dropped.
const COMPACT_WIDTH: u16 = 60;

struct Columns {
    ticket: usize,
    pr: usize,
    /// Review state as a word (`ready`, `approved`…) rather than a glyph.
    pr_words: bool,
    git: usize,
    age: usize,
}

fn draw_worktrees(f: &mut Frame, app: &App, theme: &Theme, area: Rect, now: i64) {
    let focused = true;
    let mut block = pane_block(theme, title("Worktrees", theme), focused);
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
    let pr_words = inner.width >= 100;
    // Sidebar / narrow: name, status and the PR glyph only.
    let compact = app.sidebar || s.compact_view || inner.width < COMPACT_WIDTH;
    let cols = Columns {
        ticket: if s.show_linear_ticket && !compact { app.flat.iter().map(|v| ticket_text(v).width()).max().unwrap_or(0) } else { 0 },
        pr: if s.show_pr_status { app.flat.iter().map(|v| pr_text(v, pr_words).width()).max().unwrap_or(0) } else { 0 },
        pr_words,
        git: if s.show_git_ahead_behind && !compact { app.flat.iter().map(|v| git_text(v).width()).max().unwrap_or(0) } else { 0 },
        age: if s.show_last_commit && !compact { 3 } else { 0 },
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
            Row::Worktree(i) => {
                worktree_line(app, theme, &app.flat[*i], &app.tree[*i], *i == app.selected, focused, &cols, inner.width, now)
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

#[allow(clippy::too_many_arguments)]
fn worktree_line<'a>(
    app: &App,
    theme: &Theme,
    v: &'a WorktreeView,
    tree: &str,
    selected: bool,
    focused: bool,
    cols: &Columns,
    width: u16,
    now: i64,
) -> Line<'a> {
    let status = v.status();
    let subagents = app.running_subagents_for(&v.worktree.id);
    let deleting = app.is_deleting(&v.worktree.id);

    // Right-hand columns, fixed width so they align down the list.
    let mut right: Vec<Span> = Vec::new();
    if cols.ticket > 0 {
        right.push(Span::raw(" "));
        right.push(Span::styled(pad(&ticket_text(v), cols.ticket), theme.fg(theme.blue)));
    }
    if cols.pr > 0 {
        right.push(Span::raw(" "));
        right.extend(pr_spans(v, theme, cols.pr, cols.pr_words));
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
        Span::styled(tree.to_string(), theme.fg(theme.border)),
        if deleting {
            Span::styled(spinner(now), theme.fg(theme.red))
        } else {
            Span::styled(status_glyph(status), theme.fg(theme.status(status)))
        },
        Span::raw(" "),
    ];
    let mut badge = String::new();
    // A row being deleted shows only that, in place of its other badges.
    if deleting {
        badge.push_str(" deleting…");
    } else if app.unseen.contains(&v.worktree.id) {
        badge.push_str(" •");
    }
    if !deleting && !v.running_processes.is_empty() {
        badge.push_str(" ▶");
    }
    if !deleting && subagents > 0 {
        if width >= 100 {
            badge.push_str(&format!(" ◆ {subagents} subagent{}", if subagents == 1 { "" } else { "s" }));
        } else {
            badge.push_str(&format!(" ◆{subagents}"));
        }
    }
    // A `claude --bg` session runs here (Enter / c attach to it).
    if let Some(bg) = v.background.as_ref().filter(|_| !deleting) {
        badge.push_str(if bg.failed { " bg✗" } else { " bg" });
    }
    let name_w = (width as usize).saturating_sub(right_w + 3 + badge.width() + tree.width());
    // Nickname (e.g. from the Linear ticket) first, then the branch, dimmed.
    // When space is short they share it so the branch never disappears.
    let w = &v.worktree;
    let show_branch = w.display_name() != w.branch && !w.is_standalone();
    let (name, branch) = if show_branch {
        let full = w.display_name().width() + 2 + w.branch.width();
        if full <= name_w {
            (w.display_name().to_string(), w.branch.clone())
        } else {
            let branch_w = w.branch.width().min(name_w.saturating_sub(2) / 2).max(name_w.saturating_sub(2 + w.display_name().width()));
            let nick_w = name_w.saturating_sub(2 + branch_w);
            (truncate(w.display_name(), nick_w), truncate(&w.branch, branch_w))
        }
    } else {
        (truncate(w.display_name(), name_w), String::new())
    };
    let name_style = if deleting {
        theme.faint()
    } else if status == Some(Status::Waiting) {
        theme.bold(theme.yellow)
    } else if selected {
        theme.bold(theme.text)
    } else {
        theme.fg(theme.text)
    };
    // Badges sit right after the name; padding goes after them.
    let used = name.width() + if branch.is_empty() { 0 } else { 2 + branch.width() };
    let fill = name_w.saturating_sub(used);
    left.push(Span::styled(name, name_style));
    if !branch.is_empty() {
        left.push(Span::styled(format!("  {branch}"), theme.faint()));
    }
    let mut rest = badge.as_str();
    if let Some(r) = rest.strip_prefix(" •") {
        left.push(Span::styled(" •", theme.bold(theme.accent)));
        rest = r;
    }
    if let Some(r) = rest.strip_prefix(" ▶") {
        left.push(Span::styled(" ▶", theme.fg(theme.green)));
        rest = r;
    }
    if let Some(r) = rest.strip_prefix(" deleting…") {
        left.push(Span::styled(" deleting…", theme.fg(theme.red)));
        rest = r;
    }
    left.push(Span::styled(rest.to_string(), theme.fg(theme.magenta)));
    left.push(Span::raw(" ".repeat(fill)));

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

fn pr_text(v: &WorktreeView, words: bool) -> String {
    let Some(pr) = &v.pr else { return String::new() };
    let merged = if pr.state == "MERGED" { "⇡" } else { "" };
    let review = review_state(pr).map(|r| format!(" {}", if words { r.word() } else { r.glyph() })).unwrap_or_default();
    format!("#{}{}{merged}{review}", pr.number, checks_glyph(pr.checks_status))
}

/// Where an open PR stands on review, combined with CI and conflicts so
/// "approved and good to go" stands apart from "approved but failing".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Review {
    /// Approved, checks passing (or none), no merge conflicts.
    Ready,
    /// Approved, but checks are failing or still running.
    Approved,
    /// Approved and green, but conflicts with its base.
    Conflicts,
    ChangesRequested,
    /// Waiting for a review.
    Pending,
    Draft,
}

impl Review {
    fn word(self) -> &'static str {
        match self {
            Review::Ready => "ready",
            Review::Approved => "approved",
            Review::Conflicts => "conflicts",
            Review::ChangesRequested => "changes",
            Review::Pending => "review",
            Review::Draft => "draft",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Review::Ready | Review::Approved => "✔",
            Review::Conflicts => "⚠",
            Review::ChangesRequested => "!",
            Review::Pending => "…",
            Review::Draft => "~",
        }
    }

    fn color(self) -> ColorPick {
        match self {
            Review::Ready => |t| t.green,
            Review::Approved | Review::Conflicts => |t| t.yellow,
            Review::ChangesRequested => |t| t.red,
            Review::Pending | Review::Draft => |t| t.faint,
        }
    }
}

fn review_state(pr: &am_core::model::PrInfo) -> Option<Review> {
    if pr.state != "OPEN" {
        return None;
    }
    if pr.is_draft {
        return Some(Review::Draft);
    }
    Some(match pr.review_decision.as_str() {
        "CHANGES_REQUESTED" => Review::ChangesRequested,
        "APPROVED" => match pr.checks_status {
            ChecksStatus::Failing | ChecksStatus::Pending => Review::Approved,
            _ if pr.mergeable.as_deref() == Some("CONFLICTING") => Review::Conflicts,
            _ => Review::Ready,
        },
        "REVIEW_REQUIRED" => Review::Pending,
        // Empty: the repo doesn't require reviews, or the PR came from a
        // Linear attachment — unknown, so say nothing.
        _ => return None,
    })
}

fn checks_glyph(c: ChecksStatus) -> &'static str {
    match c {
        ChecksStatus::Passing => "✓",
        ChecksStatus::Failing => "✗",
        ChecksStatus::Pending => "◌",
        ChecksStatus::None => "",
    }
}

/// Failing checks are red even on a draft: a red PR is the thing to notice.
fn pr_color(pr: &am_core::model::PrInfo) -> ColorPick {
    match (pr.state.as_str(), pr.checks_status) {
        ("MERGED", _) => |t| t.magenta,
        ("CLOSED", _) => |t| t.faint,
        (_, ChecksStatus::Failing) => |t| t.red,
        _ if pr.is_draft => |t| t.faint,
        (_, ChecksStatus::Pending) => |t| t.yellow,
        _ if pr.review_decision == "CHANGES_REQUESTED" => |t| t.yellow,
        _ => |t| t.green,
    }
}

/// PR cell: number in the state colour, checks glyph always in its own
/// colour (green ✓ / red ✗ / yellow ◌) so CI is readable on drafts too,
/// then the review state (`ready` / `approved` / `changes`…).
fn pr_spans<'a>(v: &WorktreeView, theme: &Theme, width: usize, words: bool) -> Vec<Span<'a>> {
    let Some(pr) = &v.pr else { return vec![Span::raw(" ".repeat(width))] };
    let text = pr_text(v, words);
    let num = format!("#{}", pr.number);
    let glyph = checks_glyph(pr.checks_status);
    let merged = if pr.state == "MERGED" { "⇡" } else { "" };
    let gcolor = match pr.checks_status {
        ChecksStatus::Passing => theme.green,
        ChecksStatus::Failing => theme.red,
        ChecksStatus::Pending => theme.yellow,
        ChecksStatus::None => theme.faint,
    };
    let mut spans = vec![
        Span::styled(num, theme.fg(pr_color(pr)(theme))),
        Span::styled(glyph, theme.fg(gcolor)),
        Span::styled(merged, theme.fg(theme.magenta)),
    ];
    if let Some(r) = review_state(pr) {
        let label = if words { r.word() } else { r.glyph() };
        let style = if r == Review::Ready { theme.bold(r.color()(theme)) } else { theme.fg(r.color()(theme)) };
        spans.push(Span::styled(format!(" {label}"), style));
    }
    spans.push(Span::raw(" ".repeat(width.saturating_sub(text.width()))));
    spans
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

// ------------------------------------------------------------ context + bar



/// Every dashboard key, grouped. Lower `priority` survives narrower widths;
/// what doesn't fit is still in the `?` overlay.
const KEYS: &[(&str, &str, u8)] = &[
    ("⏎", "open", 0),
    ("t", "terminal", 2),
    ("c", "claude", 1),
    ("|", "", 0),
    ("g", "PR", 0),
    ("l", "linear", 0),
    ("p", "project", 2),
    ("|", "", 0),
    ("n", "new", 1),
    ("d", "delete", 1),
    ("A", "add repo", 3),
    ("|", "", 0),
    ("w", "log", 3),
    ("s", "settings", 1),
    ("f", "feedback", 3),
    ("r", "refresh", 1),
    ("?", "help", 0),
    ("q", "quit", 0),
];

fn draw_keybar(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    // Right side: a transient notice, else the offline warning.
    let h = &app.snapshot.health;
    let offline = match (h.github_failing, h.linear_failing) {
        (true, true) => Some("GitHub + Linear"),
        (true, false) => Some("GitHub"),
        (false, true) => Some("Linear"),
        _ => None,
    };
    let status: Option<Span> = match (&app.notice, offline) {
        (Some(n), _) => Some(Span::styled(format!("{n} "), theme.fg(theme.accent))),
        (None, Some(src)) => Some(Span::styled(format!("⚠ {src} offline — cached "), theme.fg(theme.yellow))),
        _ => None,
    };
    let status_w = status.as_ref().map_or(0, |s| s.content.width());
    let [keys_area, status_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(status_w as u16)]).areas(area);
    if let Some(s) = status {
        f.render_widget(Paragraph::new(Line::from(s)), status_area);
    }
    let area = keys_area;
    let width = area.width as usize;
    // `short` (sidebar widths): keys only, except `? help` to find the rest.
    let render = |max_prio: u8, short: bool| -> Vec<Span<'static>> {
        let mut spans = vec![Span::raw(" ")];
        let mut last_sep = true;
        for (k, label, prio) in KEYS {
            if *prio > max_prio {
                continue;
            }
            if *k == "|" {
                if !last_sep {
                    spans.push(Span::styled("│ ", theme.fg(theme.border)));
                    last_sep = true;
                }
                continue;
            }
            let label = if short && *k != "?" { " ".to_string() } else { format!(" {label}  ") };
            // Actions the selected worktree can't take stay in place, dimmed.
            if app.key_available(k) {
                spans.push(Span::styled(k.to_string(), theme.bold(theme.accent)));
                spans.push(Span::styled(label, theme.faint()));
            } else {
                let off = theme.faint().add_modifier(Modifier::DIM);
                spans.push(Span::styled(k.to_string(), off));
                spans.push(Span::styled(label, off));
            }
            last_sep = false;
        }
        spans
    };
    let mut spans = render(0, true);
    for prio in (0..=3).rev() {
        let candidate = render(prio, false);
        if candidate.iter().map(|s| s.content.width()).sum::<usize>() <= width {
            spans = candidate;
            break;
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_help(f: &mut Frame, theme: &Theme, area: Rect) {
    let keys: &[(&str, &str)] = &[
        ("j / k  ↑ / ↓", "move"),
        ("home / G", "top / bottom"),
        ("ctrl-d / ctrl-u", "page down / up"),
        ("enter", "open (IDE; resumes last Claude session)"),
        ("t", "terminal at worktree"),
        ("c", "terminal running claude --resume"),
        ("g / l / p", "open PR / Linear ticket / Linear project"),
        ("w", "toggle log pane"),
        ("n / d", "new worktree / delete worktree"),
        ("A", "add repository"),
        ("f", "suggest a feature (files a GitHub issue)"),
        ("s", "settings"),
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
    // PR review states: ready = approved + CI green, approved = CI ✗/◌.
    let mut pr_legend = vec![Span::styled("PR ", theme.faint())];
    for (r, note) in [
        (Review::Ready, "=approved+CI ✓"),
        (Review::Approved, "=CI ✗/◌"),
        (Review::Conflicts, ""),
        (Review::ChangesRequested, ""),
        (Review::Pending, "=awaiting"),
    ] {
        pr_legend.push(Span::styled(r.word(), theme.fg(r.color()(theme))));
        pr_legend.push(Span::styled(format!("{note}  "), theme.faint()));
    }
    lines.push(Line::from(pr_legend));
    let w = 80.min(area.width.saturating_sub(4));
    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    let h = (para.line_count(w.saturating_sub(2)) as u16 + 2).min(area.height);
    let rect = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
    clear(f, theme, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.bold(theme.border_focus))
        .title(Line::styled(" Keys ", theme.bold(theme.text)));
    f.render_widget(para.block(block), rect);
}

// ------------------------------------------------------------------ helpers

/// Blank an overlay's area in the theme's own background.
fn clear(f: &mut Frame, theme: &Theme, rect: Rect) {
    f.render_widget(Clear, rect);
    f.render_widget(Block::new().style(theme.base()), rect);
}

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
            background: None,
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
            review_decision: "APPROVED".into(),
            has_feedback: false,
            checks_status: ChecksStatus::Passing,
            active_check_url: None,
            active_check_name: None,
            checks_waiting: false,
            base_ref_name: None,
            mergeable: None,
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
            stack: Default::default(),
        };
        let settings = Settings { worktree_sort: vec![], ..Settings::default() };
        let mut app = App::new(settings);
        app.apply(snapshot);
        app
    }

    fn render(app: &App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app, &Theme::ansi(), NOW, &[])).unwrap();
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
    fn sidebar_drops_columns() {
        let mut app = fixture();
        app.sidebar = true;
        insta::assert_snapshot!(render(&app, 40, 16));
    }

    #[test]
    fn narrow_width_is_compact_without_the_flag() {
        insta::assert_snapshot!(render(&fixture(), 55, 16));
    }

    #[test]
    fn background_session_badge() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        snap.groups[0].worktrees[1].background =
            Some(am_core::model::BackgroundAgent { id: "ab12".into(), name: None, failed: false, waiting_for: None });
        app.apply(snap);
        let out = render(&app, 80, 8);
        assert!(out.lines().any(|l| l.contains("fix/offline-cache bg")), "{out}");
    }

    #[test]
    fn help_overlay() {
        let mut app = fixture();
        app.on_key(Key::Char('?'));
        insta::assert_snapshot!(render(&app, 100, 20));
    }

    fn with_builder(current: &str) -> App {
        use am_core::themes;
        let mut app = fixture();
        let mut ed = crate::settings_ui::SettingsEditor::new(&app.settings);
        let catalog = themes::builtin_names().into_iter().filter_map(themes::builtin).collect();
        ed.builder = Some(crate::theme_builder::ThemeBuilder::open(current, None, catalog));
        app.settings_editor = Some(ed);
        app
    }

    #[test]
    fn theme_builder_overlay() {
        let mut app = with_builder("catppuccin-mocha");
        app.on_key(Key::Char('j'));
        app.on_key(Key::Char('j'));
        insta::assert_snapshot!(render(&app, 110, 28));
    }

    #[test]
    fn theme_builder_palette_picker() {
        let mut app = with_builder("ansi");
        app.on_key(Key::Enter);
        app.on_key(Key::Down);
        app.on_key(Key::Down);
        insta::assert_snapshot!(render(&app, 110, 28));
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
        assert_eq!(app.on_key(Key::Char('g')), crate::app::Action::OpenUrl("u".into()));
        assert_eq!(app.on_key(Key::Char('l')), crate::app::Action::OpenUrl("u".into()));
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
    fn failing_pr_is_red_even_as_a_draft() {
        let theme = Theme::ansi();
        let mut v = fixture().flat[0].clone();
        let pr = v.pr.as_mut().unwrap();
        pr.is_draft = true;
        pr.checks_status = ChecksStatus::Failing;
        let spans = pr_spans(&v, &theme, 16, true);
        assert_eq!(spans[0].content, "#127");
        assert_eq!(spans[0].style.fg, Some(theme.red));
        assert_eq!(spans[1].content, "✗");
        assert_eq!(spans[1].style.fg, Some(theme.red));
        v.pr.as_mut().unwrap().checks_status = ChecksStatus::Passing;
        let spans = pr_spans(&v, &theme, 16, true);
        assert_eq!(spans[0].style.fg, Some(theme.faint), "passing draft stays dim");
        assert_eq!(spans[1].style.fg, Some(theme.green));
    }

    #[test]
    fn pr_review_state_tells_ready_from_approved_but_failing() {
        let theme = Theme::ansi();
        let mut v = fixture().flat[0].clone();
        let label = |v: &WorktreeView, words: bool| {
            let spans = pr_spans(v, &theme, 16, words);
            spans.iter().find(|s| s.content.starts_with(' ') && !s.content.trim().is_empty()).map(|s| (s.content.trim().to_string(), s.style.fg))
        };
        let set = |v: &mut WorktreeView, review: &str, checks: ChecksStatus, mergeable: Option<&str>| {
            let pr = v.pr.as_mut().unwrap();
            pr.review_decision = review.into();
            pr.checks_status = checks;
            pr.mergeable = mergeable.map(str::to_string);
        };
        set(&mut v, "APPROVED", ChecksStatus::Passing, Some("MERGEABLE"));
        assert_eq!(label(&v, true), Some(("ready".into(), Some(theme.green))));
        assert_eq!(pr_text(&v, true), "#127✓ ready");
        set(&mut v, "APPROVED", ChecksStatus::None, None);
        assert_eq!(label(&v, true).unwrap().0, "ready", "no CI configured still counts as ready");
        set(&mut v, "APPROVED", ChecksStatus::Failing, Some("MERGEABLE"));
        assert_eq!(label(&v, true), Some(("approved".into(), Some(theme.yellow))));
        assert_eq!(pr_text(&v, true), "#127✗ approved");
        set(&mut v, "APPROVED", ChecksStatus::Pending, None);
        assert_eq!(label(&v, true).unwrap().0, "approved");
        set(&mut v, "APPROVED", ChecksStatus::Passing, Some("CONFLICTING"));
        assert_eq!(label(&v, true).unwrap().0, "conflicts");
        set(&mut v, "CHANGES_REQUESTED", ChecksStatus::Passing, None);
        assert_eq!(label(&v, true), Some(("changes".into(), Some(theme.red))));
        set(&mut v, "REVIEW_REQUIRED", ChecksStatus::Passing, None);
        assert_eq!(label(&v, true).unwrap().0, "review");
        set(&mut v, "", ChecksStatus::Passing, None);
        assert_eq!(pr_text(&v, true), "#127✓", "unknown review state shows nothing");
        // Narrow terminals get a glyph instead of the word.
        set(&mut v, "APPROVED", ChecksStatus::Passing, None);
        assert_eq!(pr_text(&v, false), "#127✓ ✔");
        // Merged / closed PRs have no review state.
        v.pr.as_mut().unwrap().state = "MERGED".into();
        assert_eq!(pr_text(&v, true), "#127✓⇡");
    }

    #[test]
    fn stacked_branches_nest_under_their_parent() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        // r1: refactor, offline-cache. Add two stacked on refactor and one on those.
        let mut a = view("w4", "r1", "feature/refactor-part-2", Some(Status::Executing));
        a.pr = None;
        let b = view("w5", "r1", "feature/refactor-part-3", None);
        let c = view("w6", "r1", "feature/refactor-alt", None);
        snap.groups[0].worktrees.splice(0..0, [a, b, c]);
        snap.stack.insert("w4".into(), "w1".into());
        snap.stack.insert("w5".into(), "w4".into());
        snap.stack.insert("w6".into(), "w1".into());
        app.apply(snap);
        let order: Vec<(&str, &str)> = app.flat.iter().zip(&app.tree).map(|(v, t)| (v.worktree.id.as_str(), t.as_str())).collect();
        assert_eq!(
            order,
            [("w1", ""), ("w4", "├─ "), ("w5", "│  └─ "), ("w6", "└─ "), ("w2", ""), ("w3", "")],
            "children follow the parent, depth-first, with tree lines"
        );
        insta::assert_snapshot!(render(&app, 100, 14));
    }

    #[test]
    fn nicknamed_rows_still_show_the_branch() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        snap.groups[0].worktrees[0].worktree.custom_name = Some("Refactor grouping core".into());
        app.apply(snap);
        let wide = render(&app, 120, 8);
        assert!(wide.contains("Refactor grouping core  feature/refactor"), "{wide}");
        let narrow = render(&app, 80, 8);
        assert!(narrow.contains("feature/"), "branch survives at 80 cols:\n{narrow}");
    }

    #[test]
    fn long_delete_label_wraps_and_can_go_to_the_background() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        let nick = "Confirm the results read and the limit basis against a real customer feeder";
        snap.groups[0].worktrees[0].worktree.custom_name = Some(nick.into());
        app.apply(snap);
        app.on_key(Key::Char('d'));
        // Fixture paths don't exist, so this is the one-step stale-entry delete.
        let action = app.on_key(Key::Enter);
        assert!(matches!(action, crate::app::Action::Delete(..)));
        let busy = render(&app, 100, 16);
        assert!(busy.contains("customer feeder…"), "label wraps instead of being cut off:\n{busy}");
        insta::assert_snapshot!("delete_busy_modal", busy);

        // Any key hides the box; the row stays greyed and takes no actions.
        assert_eq!(app.on_key(Key::Char('x')), crate::app::Action::None);
        assert!(app.modal.is_none());
        let row = render(&app, 100, 16);
        assert!(row.contains("deleting…"), "{row}");
        assert_eq!(app.on_key(Key::Enter), crate::app::Action::None);
        assert_eq!(app.on_key(Key::Char('d')), crate::app::Action::None);
        assert!(app.modal.is_none(), "can't delete it twice");
        // Other rows still work.
        app.on_key(Key::Char('j'));
        assert!(matches!(app.on_key(Key::Enter), crate::app::Action::Open(_)));
        insta::assert_snapshot!("delete_in_background", row);
    }

    #[test]
    fn key_bar_dims_actions_the_selected_worktree_lacks() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        snap.groups[1].worktrees[0].worktree.is_main = true;
        app.apply(snap);
        // w1: PR + Linear ticket (no project).
        assert!(app.key_available("g") && app.key_available("l") && app.key_available("d"));
        assert!(!app.key_available("p"), "ticket without a project");
        // w2: no PR, no ticket.
        app.on_key(Key::Char('j'));
        assert!(!app.key_available("g") && !app.key_available("l"));
        assert!(app.key_available("⏎") && app.key_available("d"));
        // w3: the main checkout on main can't be deleted.
        app.on_key(Key::Char('j'));
        assert!(!app.key_available("d"));
        assert!(app.key_available("n") && app.key_available("?"), "global keys are always on");
        // A row being deleted takes no row actions at all.
        app.deleting.insert("w3".into(), false);
        assert!(!app.key_available("⏎") && !app.key_available("t"));

        // Rendered: available keys are bold accent, unavailable ones dimmed.
        let theme = Theme::ansi();
        app.deleting.clear();
        let mut term = Terminal::new(TestBackend::new(100, 16)).unwrap();
        term.draw(|f| draw(f, &app, &theme, NOW, &[])).unwrap();
        let buf = term.backend().buffer().clone();
        let bar: String = (0..100).map(|x| buf[(x, 15)].symbol().to_string()).collect();
        let style_of = |needle: &str| {
            let col = bar.chars().collect::<Vec<_>>();
            let i = (0..col.len()).find(|&i| col[i..].iter().collect::<String>().starts_with(needle)).unwrap();
            buf[(i as u16, 15)].style()
        };
        assert!(style_of("d delete").add_modifier.contains(Modifier::DIM), "{bar}");
        assert!(style_of("g PR").add_modifier.contains(Modifier::DIM));
        assert!(!style_of("n new").add_modifier.contains(Modifier::DIM));
        assert!(style_of("n new").add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn delegating_is_a_green_half_circle() {
        let theme = Theme::ansi();
        assert_eq!(status_glyph(Some(Status::Delegating)), "◑");
        assert_eq!(theme.status(Some(Status::Delegating)), theme.green);
        assert_ne!(status_glyph(Some(Status::Delegating)), status_glyph(Some(Status::Waiting)));
        assert_ne!(theme.status(Some(Status::Delegating)), theme.status(Some(Status::Waiting)));
    }

    #[test]
    fn finished_delete_clears_on_the_next_snapshot_and_failure_right_away() {
        let mut app = fixture();
        app.deleting.insert("w1".into(), false);
        app.deleting.insert("w2".into(), false);
        app.finish_delete("w2", false);
        assert!(!app.is_deleting("w2"), "a failed delete makes the row usable again");
        app.finish_delete("w1", true);
        assert!(app.is_deleting("w1"), "stays greyed until the refresh removes it");
        let snap = app.snapshot.clone();
        app.apply(snap);
        assert!(!app.is_deleting("w1"));
    }

    #[test]
    fn project_sections_are_separated_by_a_blank_line() {
        use am_core::settings::{Direction, SortCriterion, SortKey};
        let mut app = fixture();
        app.settings.worktree_sort = vec![SortCriterion { key: SortKey::LinearProject, direction: Direction::Asc, enabled: true }];
        let mut snap = app.snapshot.clone();
        let proj = |name: &str| LinearProject { id: name.into(), name: name.into(), color: None, url: None };
        let mut l = snap.groups[0].worktrees[0].linear.clone().unwrap();
        l.project = Some(proj("Alpha"));
        snap.groups[0].worktrees[0].linear = Some(l.clone());
        l.identifier = "ENG-9".into();
        l.project = Some(proj("Beta"));
        snap.groups[0].worktrees[1].linear = Some(l);
        app.apply(snap);
        let kinds: Vec<&str> = app
            .rows
            .iter()
            .map(|r| match r {
                Row::Spacer => "spacer",
                Row::Repo(_) => "repo",
                Row::Project(_) => "project",
                Row::Ticket(_) => "ticket",
                Row::Worktree(_) => "wt",
            })
            .collect();
        assert_eq!(kinds, ["repo", "project", "wt", "spacer", "project", "wt", "spacer", "repo", "project", "wt"]);
    }

    #[test]
    fn truncate_respects_display_width() {
        assert_eq!(truncate("feature/refactor", 8), "feature…");
        assert_eq!(truncate("short", 8), "short");
    }
}
