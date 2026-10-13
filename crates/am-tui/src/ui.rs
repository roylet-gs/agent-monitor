//! Rendering. lazygit-style titled panes: `[1] Worktrees` and `[2] Subagents`,
//! a context line for the selection, and a status bar with a mode pill.
//! ratatui diffs each frame against the previous one cell by cell, so a
//! redraw only writes what changed — no full-screen clears, no flicker.

use am_core::harness::HarnessKind;
use am_core::model::{ChecksStatus, WorktreeView};
use am_core::status::Status;
use am_core::{context, timefmt};
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
    let areas = dashboard_areas(app, area);
    draw_keybar(f, app, theme, areas.keys);
    if app.show_log {
        draw_log(f, theme, areas.log, log_lines);
    }

    draw_worktrees(f, app, theme, areas.list, now);
    if let Some(jobs) = areas.jobs {
        draw_jobs(f, app, theme, jobs, now);
    }
    if app.show_help {
        draw_help(f, theme, area);
    }
    if let Some(editor) = &app.settings_editor {
        match &editor.builder {
            Some(b) => draw_theme_builder(f, theme, area, b),
            None => draw_settings(f, theme, area, editor, shown_update(app)),
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

/// Where the dashboard's parts go. `jobs` is `None` while the jobs pane is hidden.
struct DashAreas {
    list: Rect,
    jobs: Option<Rect>,
    log: Rect,
    keys: Rect,
}

fn dashboard_areas(app: &App, area: Rect) -> DashAreas {
    let log_h = if app.show_log { (area.height / 3).max(5) } else { 0 };
    let [body, log, keys] =
        Layout::vertical([Constraint::Min(5), Constraint::Length(log_h), Constraint::Length(1)]).areas(area);
    if app.jobs.visible() {
        let pane_w = (body.width / 3).clamp(34, 60).min(body.width.saturating_sub(40));
        let [list, jobs] = Layout::horizontal([Constraint::Min(40), Constraint::Length(pane_w)]).areas(body);
        DashAreas { list, jobs: Some(jobs), log, keys }
    } else {
        DashAreas { list: body, jobs: None, log, keys }
    }
}

/// Whether a click landed in the jobs pane.
pub fn in_jobs_pane(app: &App, area: Rect, x: u16, y: u16) -> bool {
    dashboard_areas(app, area).jobs.is_some_and(|r| contains(r, x, y))
}

/// Settings box, then inside it the update banner, field list and help line.
fn settings_areas(area: Rect, update: Option<&am_core::update_check::Update>) -> (Rect, [Rect; 3]) {
    let w = 84.min(area.width.saturating_sub(2));
    // Headline, the (possibly wrapped) upgrade hint, then a blank line.
    let banner_h = update.map_or(0, |u| {
        let hint_w = " Update: ".width() + u.hint.width();
        2 + hint_w.div_ceil((w as usize).saturating_sub(2).max(1)) as u16
    });
    let h = area.height.saturating_sub(2).min(crate::settings_ui::FIELDS.len() as u16 + 16 + banner_h);
    let rect = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
    let parts = Layout::vertical([Constraint::Length(banner_h), Constraint::Min(3), Constraint::Length(2)])
        .areas(Block::bordered().inner(rect));
    (rect, parts)
}

/// The settings list as lines: `Some(field index)` for a field, `None` for a
/// section header or blank line.
fn settings_lines() -> Vec<Option<usize>> {
    let mut lines = Vec::new();
    let mut section = "";
    for (i, field) in crate::settings_ui::FIELDS.iter().enumerate() {
        if field.section != section {
            section = field.section;
            if !lines.is_empty() {
                lines.push(None);
            }
            lines.push(None);
        }
        lines.push(Some(i));
    }
    lines
}

/// First visible settings line: keeps the cursor (plus one line) in view.
fn settings_start(lines: &[Option<usize>], cursor: usize, height: usize) -> usize {
    let sel_line = lines.iter().position(|l| *l == Some(cursor)).unwrap_or(0);
    (sel_line + 2).saturating_sub(height)
}

/// The settings field under a click, if any (field list only, not the
/// sort editor or theme builder).
pub fn settings_field_at(app: &App, area: Rect, x: u16, y: u16) -> Option<usize> {
    let ed = app.settings_editor.as_ref()?;
    let (_, [_, list, _]) = settings_areas(area, shown_update(app));
    if !contains(list, x, y) {
        return None;
    }
    let lines = settings_lines();
    let start = settings_start(&lines, ed.cursor, list.height as usize);
    lines.get(start + (y - list.y) as usize).copied().flatten()
}

fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

fn draw_settings(
    f: &mut Frame,
    theme: &Theme,
    area: Rect,
    ed: &crate::settings_ui::SettingsEditor,
    update: Option<&am_core::update_check::Update>,
) {
    use crate::settings_ui::{FIELDS, Kind, sort_label};
    let (rect, [banner, list, help]) = settings_areas(area, update);
    clear(f, theme, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.bold(theme.border_focus))
        .title(Line::from(vec![Span::styled("─", theme.bold(theme.border_focus)), Span::styled("Settings ", theme.bold(theme.text))]))
        .title_bottom(
            Line::styled(" ⏎/space change  ←→ adjust  e edit file  esc close ", theme.faint()).alignment(Alignment::Right),
        );
    f.render_widget(block, rect);
    if let Some(u) = update {
        let lines = vec![
            Line::from(vec![
                Span::styled(format!(" ⬆ am v{} is available", u.latest), theme.bold(theme.green)),
                Span::styled(format!(" (you have v{})", u.current), theme.faint()),
            ]),
            Line::from(vec![Span::styled(" Update: ", theme.faint()), Span::styled(u.hint.clone(), theme.fg(theme.text))]),
        ];
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), banner);
    }

    if let Some(pl) = &ed.plugins {
        draw_plugins(f, theme, list, help, pl);
        return;
    }

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

    let layout = settings_lines();
    let mut lines: Vec<Line> = Vec::new();
    let label_w = 30usize;
    for (n, entry) in layout.iter().enumerate() {
        let Some(i) = *entry else {
            // A header is followed by its section's first field; blank otherwise.
            match layout.get(n + 1).copied().flatten() {
                Some(next) => lines.push(Line::styled(format!(" {}", FIELDS[next].section), theme.bold(theme.accent))),
                None => lines.push(Line::raw("")),
            }
            continue;
        };
        let field = &FIELDS[i];
        let sel = i == ed.cursor;
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
    let start = settings_start(&layout, ed.cursor, height);
    f.render_widget(Paragraph::new(lines.into_iter().skip(start).take(height).collect::<Vec<_>>()), list);

    let field = ed.field();
    let help_line = match &ed.error {
        Some(e) => Line::styled(format!(" {e}"), theme.fg(theme.red)),
        None => Line::styled(format!(" {}", field.help), theme.faint()),
    };
    f.render_widget(Paragraph::new(vec![Line::raw(""), help_line]), help);
}

/// Settings → Agent plugins.
fn draw_plugins(f: &mut Frame, theme: &Theme, list: Rect, help: Rect, pl: &crate::plugins_ui::PluginEditor) {
    use crate::plugins_ui::Confirm;
    let mut lines = vec![
        Line::styled(" Agent plugins — space on/off · i install hooks · d default · esc back", theme.faint()),
        Line::styled(" Training — T train a new harness plugin · e retrain · p publish · x remove", theme.faint()),
        Line::raw(""),
    ];
    for (i, r) in pl.rows.iter().enumerate() {
        let sel = i == pl.cursor;
        let hooks = match (r.enabled, r.hooks, r.present) {
            (_, true, _) => Span::styled("hooks ✓      ", theme.fg(theme.green)),
            (true, false, true) => Span::styled("hooks ✗      ", theme.fg(theme.yellow)),
            (true, false, false) => Span::styled("not set up   ", theme.faint()),
            (false, false, _) => Span::styled("—            ", theme.faint()),
        };
        let default = if r.id == pl.default_agent { "★ default" } else { "" };
        let mut spans = vec![
            Span::styled(if sel { "▌" } else { " " }, theme.bold(theme.border_focus)),
            Span::styled(if r.enabled { " [x] " } else { " [ ] " }, theme.fg(if r.enabled { theme.green } else { theme.faint })),
            Span::styled(format!("{:<14}", truncate(&r.name, 14)), if r.enabled { theme.fg(theme.text) } else { theme.faint() }),
            Span::styled(format!("{:<12}", truncate(&r.source, 12)), theme.faint()),
            hooks,
            Span::styled(format!("{default:<10}"), theme.fg(theme.accent)),
        ];
        if r.draft {
            spans.push(Span::styled("✎ draft ", theme.fg(theme.yellow)));
        }
        if r.issues > 0 {
            spans.push(Span::styled(format!("⚠ {}", r.issues), theme.fg(theme.red)));
        }
        let line = Line::from(spans);
        lines.push(if sel { line.style(theme.selected()) } else { line });
    }
    let sel = pl.on_train_row();
    let train = Line::from(vec![
        Span::styled(if sel { "▌" } else { " " }, theme.bold(theme.border_focus)),
        Span::styled(" + Train a new harness plugin…", theme.fg(theme.accent)),
    ]);
    lines.push(if sel { train.style(theme.selected()) } else { train });
    if let Some(cmd) = &pl.train {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled(" Command that starts it: ", theme.fg(theme.text)),
            Span::styled(format!("{cmd}▏"), theme.bold(theme.accent)),
        ]));
        lines.push(Line::styled(
            "   e.g. cursor-agent · it opens in a new terminal, prompt copied",
            theme.faint(),
        ));
    }
    if !pl.errors.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(" Skipped plugin files:", theme.fg(theme.yellow)));
        for e in &pl.errors {
            lines.push(Line::styled(format!("   {}", truncate(e, (list.width as usize).saturating_sub(4))), theme.faint()));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        format!(" Add plugins as JSON manifests in {}, or train one (T)", pl.plugins_dir),
        theme.faint(),
    ));
    f.render_widget(Paragraph::new(lines), list);
    let help_line = match (&pl.confirm, &pl.message, pl.selected()) {
        (Some(Confirm::Publish(id)), _, _) => {
            Line::styled(format!(" Publish {id}? Stops capturing, drops the draft badge · y yes · any key no"), theme.fg(theme.yellow))
        }
        (Some(Confirm::Remove(id)), _, _) => {
            Line::styled(format!(" Remove {id} and its hooks? (rollback restores it) · y yes · any key no"), theme.fg(theme.yellow))
        }
        (None, Some((ok, m)), _) => Line::styled(format!(" {m}"), theme.fg(if *ok { theme.green } else { theme.red })),
        (None, None, Some(r)) if r.draft => Line::styled(
            format!(" {} · draft · {} capture(s) · {} issue(s) · am harness issues {}", r.id, r.captures, r.issues, r.id),
            theme.faint(),
        ),
        (None, None, Some(r)) => {
            let note = r.note.as_deref().map(|n| format!(" · {n}")).unwrap_or_default();
            Line::styled(format!(" {} · hooks in {}{note}", r.id, r.settings_path), theme.faint())
        }
        (None, None, None) if pl.train.is_some() => Line::styled(" ⏎ start training · esc cancel", theme.faint()),
        (None, None, None) => Line::styled(" ⏎ train a new harness plugin: the harness writes and tests its own plugin", theme.faint()),
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
        Modal::Busy { text } => {
            let line = Line::from(vec![Span::styled(format!("{} ", spinner(now)), theme.fg(theme.accent)), Span::raw(text.clone())]);
            ("Working".into(), theme.accent, "", vec![line])
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
    use crate::setup::{CONTEXT_CHOICES, CONTEXT_CUSTOM, IDES, Step};
    let steps = [
        (Step::Editor, "Editor"),
        (Step::Hooks, "Agent hooks"),
        (Step::Repos, "Repositories"),
        (Step::Integrations, "Integrations"),
        (Step::Context, "Context"),
    ];
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
            lines.push(Line::raw("am learns what each agent is doing from its CLI's hooks"));
            lines.push(Line::raw("(other hooks in those files are kept):"));
            lines.push(Line::raw(""));
            for (kind, on) in &s.agents {
                let p = kind.provider();
                lines.push(Line::from(vec![
                    Span::styled(if *on { "  ✓ " } else { "  ○ " }, theme.fg(if *on { theme.green } else { theme.faint })),
                    Span::styled(format!("{:<12}", p.name()), theme.fg(theme.text)),
                    Span::styled(am_core::hooks_installer::display_path(p), theme.faint()),
                ]));
            }
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
            hint = "tab field  space toggle  ⏎ next  esc back";
        }
        Step::Context => {
            lines.push(Line::raw("How many tokens of context should the bar measure against?"));
            lines.push(Line::styled("A session past it doubles the scale until it fits.", theme.faint()));
            lines.push(Line::raw(""));
            let radio = |sel: bool, label: String| {
                Line::from(vec![
                    Span::styled(if sel { "  ● " } else { "  ○ " }, theme.fg(if sel { theme.border_focus } else { theme.faint })),
                    Span::styled(label, if sel { theme.bold(theme.text) } else { theme.fg(theme.text) }),
                ])
            };
            for (i, (_, label)) in CONTEXT_CHOICES.iter().enumerate() {
                lines.push(radio(i == s.ctx_choice, label.to_string()));
            }
            let custom = s.ctx_choice == CONTEXT_CUSTOM;
            let mut row = radio(custom, "Custom: ".into());
            row.spans.push(Span::styled(
                if s.ctx_input.is_empty() && !custom { "e.g. 128k".to_string() } else { format!("{}{}", s.ctx_input, if custom { "▏" } else { "" }) },
                theme.faint(),
            ));
            lines.push(row);
            if let Some(e) = &s.ctx_error {
                lines.push(Line::styled(e.clone(), theme.fg(theme.red)));
            }
            hint = "↑↓ choose  ⏎ finish  esc back";
        }
    }
    // 82 columns fits the five breadcrumbs on one line.
    let rect = modal_rect(area, 82, lines.len() as u16 + 4);
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
        let is_command = raw.starts_with("cd ") || am_core::harness::all().iter().any(|k| raw.starts_with(k.provider().program()));
        let style = if is_command { theme.bold(theme.accent) } else { theme.fg(theme.text) };
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

/// The update to advertise, unless `checkForUpdates` was turned off.
fn shown_update(app: &App) -> Option<&am_core::update_check::Update> {
    app.update_available.as_ref().filter(|_| app.settings.check_for_updates)
}

/// Top-right "update available" label: shortened on narrow panes, dropped
/// when it would run into the `─Worktrees` title.
fn update_label(app: &App, width: u16) -> Option<String> {
    let latest = &shown_update(app)?.latest;
    // Two corners, `─Worktrees `, and one border cell between the titles.
    let room = (width as usize).saturating_sub(2 + "─Worktrees ".width() + 1);
    [format!(" ⬆ v{latest} available "), format!(" ⬆ v{latest} ")].into_iter().find(|l| l.width() <= room)
}

/// Bottom-right ` today 4.1M ` (` today (visible) 4.1M ` for the visible
/// scope), shortened or dropped when it doesn't fit beside ` N of M `.
fn tokens_label(app: &App, width: u16) -> Option<String> {
    let tokens = app.snapshot.today_tokens.filter(|_| app.settings.show_daily_tokens)?;
    let n = context::short_tokens(tokens);
    let pos = if app.flat.is_empty() { 0 } else { format!(" {} of {} ", app.selected + 1, app.flat.len()).width() };
    // Two corners, one border cell on the left and one between the titles.
    let room = (width as usize).saturating_sub(2 + 1 + pos + 1);
    let long = app.settings.daily_tokens_visible_only().then(|| format!(" today (visible) {n} "));
    long.into_iter().chain([format!(" today {n} ")]).find(|l| l.width() <= room)
}

struct Columns {
    ticket: usize,
    pr: usize,
    /// Review state as a word (`ready`, `approved`…) rather than a glyph.
    pr_words: bool,
    git: usize,
    age: usize,
    /// Context bar cells (0 hides the column); the token count follows it.
    ctx_bar: usize,
    ctx_window: u64,
}

fn draw_worktrees(f: &mut Frame, app: &App, theme: &Theme, area: Rect, now: i64) {
    let focused = !app.jobs.focused;
    let mut block = pane_block(theme, title("Worktrees", theme), focused);
    if !app.flat.is_empty() {
        block = block.title_bottom(
            Line::styled(format!(" {} of {} ", app.selected + 1, app.flat.len()), theme.faint()).alignment(Alignment::Right),
        );
    }
    if let Some(label) = tokens_label(app, area.width) {
        block = block.title_bottom(Line::styled(label, theme.faint()).alignment(Alignment::Right));
    }
    if let Some(label) = update_label(app, area.width) {
        block = block.title(Line::styled(label, theme.bold(theme.green)).alignment(Alignment::Right));
    }
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.flat.is_empty() && !app.rows.iter().any(|r| matches!(r, Row::Pending(_))) {
        let msg = if app.loaded { "No worktrees yet — press A to add a repository" } else { "Loading…" };
        f.render_widget(Paragraph::new(Line::styled(msg, theme.faint())).alignment(Alignment::Center), centered_line(inner));
        return;
    }

    let s = &app.settings;
    let pr_words = inner.width >= 100;
    let cols = Columns {
        ticket: if s.show_linear_ticket { app.flat.iter().map(|v| ticket_text(v).width()).max().unwrap_or(0) } else { 0 },
        pr: if s.show_pr_status { app.flat.iter().map(|v| pr_text(v, pr_words).width()).max().unwrap_or(0) } else { 0 },
        pr_words,
        git: if s.show_git_ahead_behind { app.flat.iter().map(|v| git_text(v).width()).max().unwrap_or(0) } else { 0 },
        age: if s.show_last_commit { 3 } else { 0 },
        ctx_bar: if s.show_context_window && app.flat.iter().any(|v| v.context.is_some()) {
            if pr_words { 8 } else { 5 }
        } else {
            0
        },
        ctx_window: s.context_window_tokens,
    };

    let height = inner.height as usize;
    let start = list_start(app, height);
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
            Row::Pending(id) => pending_line(app, theme, *id, inner.width, now),
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

/// First visible row of the worktree list. Keeps the selected row in view;
/// headers scroll with their rows.
fn list_start(app: &App, height: usize) -> usize {
    let sel_row = app.rows.iter().position(|r| *r == Row::Worktree(app.selected)).unwrap_or(0);
    let mut start = sel_row.saturating_sub(height.saturating_sub(1));
    // Show the header above the selection when there's room.
    if start > 0 && sel_row - start < height.saturating_sub(1) {
        start = start.saturating_sub(1);
    }
    start
}

/// The worktree (`flat` index) under a click; headers and blank space are `None`.
pub fn worktree_at(app: &App, area: Rect, x: u16, y: u16) -> Option<usize> {
    let inner = Block::bordered().inner(dashboard_areas(app, area).list);
    if app.flat.is_empty() || !contains(inner, x, y) {
        return None;
    }
    let start = list_start(app, inner.height as usize);
    match app.rows.get(start + (y - inner.y) as usize) {
        Some(Row::Worktree(i)) => Some(*i),
        _ => None,
    }
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
    let mut ctx_range = 0..0;
    if cols.ctx_bar > 0 {
        right.push(Span::raw(" "));
        let start = right.len();
        right.extend(ctx_spans(v, theme, cols.ctx_bar, cols.ctx_window, deleting));
        ctx_range = start..right.len();
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
    // Which agent CLI, for anything but the default (Claude Code).
    let agent_badge = match v.agent.as_ref().map(|a| a.agent) {
        Some(kind) if !deleting && kind != HarnessKind::CLAUDE => {
            if width >= 100 { format!(" ✦ {}", kind.id()) } else { " ✦".to_string() }
        }
        _ => String::new(),
    };
    badge.push_str(&agent_badge);
    // A plugin still being trained (Settings → Agent plugins publishes it).
    let draft_badge = match v.agent.as_ref().map(|a| a.agent) {
        Some(kind) if !deleting && kind.is_draft() => if width >= 100 { " ✎ draft" } else { "✎" },
        _ => "",
    };
    badge.push_str(draft_badge);
    if !deleting && subagents > 0 {
        if width >= 100 {
            badge.push_str(&format!(" ◆ {subagents} subagent{}", if subagents == 1 { "" } else { "s" }));
        } else {
            badge.push_str(&format!(" ◆{subagents}"));
        }
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
    if let Some(r) = rest.strip_prefix(agent_badge.as_str()).filter(|_| !agent_badge.is_empty()) {
        left.push(Span::styled(agent_badge.clone(), theme.fg(theme.blue)));
        rest = r;
    }
    if let Some(r) = rest.strip_prefix(draft_badge).filter(|_| !draft_badge.is_empty()) {
        left.push(Span::styled(draft_badge, theme.fg(theme.yellow)));
        rest = r;
    }
    if let Some(r) = rest.strip_prefix(" deleting…") {
        left.push(Span::styled(" deleting…", theme.fg(theme.red)));
        rest = r;
    }
    left.push(Span::styled(rest.to_string(), theme.fg(theme.magenta)));
    left.push(Span::raw(" ".repeat(fill)));

    let ctx_range = ctx_range.start + left.len()..ctx_range.end + left.len();
    let mut spans = left;
    spans.extend(right);
    let line = Line::from(spans);
    if selected && focused {
        if theme.name == "ansi" {
            // Reverse video swaps fg/bg, so drop span colours to stay legible.
            // The context bar keeps its colours outside the reverse: reversed,
            // its `█` cells would take the background colour and vanish.
            let plain: Vec<Span> = line
                .spans
                .into_iter()
                .enumerate()
                .map(|(i, s)| {
                    if ctx_range.contains(&i) {
                        Span::styled(s.content, s.style.remove_modifier(Modifier::REVERSED))
                    } else {
                        Span::raw(s.content)
                    }
                })
                .collect();
            return Line::from(plain).style(theme.selected().add_modifier(Modifier::BOLD));
        }
        return line.style(theme.selected());
    }
    line
}

/// Context-window bar plus tokens used (`████▌░░░ 380k`). The fill and the
/// count go green → yellow → red as the session nears its window.
fn ctx_spans<'a>(v: &WorktreeView, theme: &Theme, bar_w: usize, configured: u64, deleting: bool) -> Vec<Span<'a>> {
    let Some(u) = &v.context else { return vec![Span::raw(" ".repeat(bar_w + 5))] };
    let window = context::window_for(u.tokens, configured);
    let style = if deleting {
        theme.faint()
    } else {
        match context::level(u.tokens, window) {
            context::Level::Ok => theme.fg(theme.green),
            context::Level::Warn => theme.fg(theme.yellow),
            context::Level::Critical => theme.bold(theme.red),
        }
    };
    let (filled, empty) = bar_cells(context::fraction(u.tokens, window), bar_w);
    vec![
        Span::styled(filled, style),
        Span::styled(empty, theme.faint()),
        Span::styled(format!(" {:>4}", context::short_tokens(u.tokens)), style),
    ]
}

/// `frac` of `width` cells in eighth blocks: (filled part, `░` remainder).
fn bar_cells(frac: f64, width: usize) -> (String, String) {
    const EIGHTHS: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];
    let eighths = (frac.clamp(0.0, 1.0) * (width * 8) as f64).round() as usize;
    let mut filled = "█".repeat(eighths / 8);
    let mut cells = eighths / 8;
    if !eighths.is_multiple_of(8) {
        filled.push(EIGHTHS[eighths % 8]);
        cells += 1;
    }
    (filled, "░".repeat(width - cells))
}

/// Placeholder row for a worktree being created: spinner, name, branch and
/// the job's current stage.
fn pending_line<'a>(app: &App, theme: &Theme, id: u64, width: u16, now: i64) -> Line<'a> {
    let Some(job) = app.jobs.get(id) else { return Line::raw("") };
    let Some((_, req, _)) = job.pending_create() else { return Line::raw("") };
    let name = req.custom_name.clone().unwrap_or_else(|| req.branch.clone());
    let branch = if req.custom_name.is_some() { format!("  {}", req.branch) } else { String::new() };
    let what = if job.question.is_some() {
        " waiting — ⇥ to answer".to_string()
    } else {
        format!(" creating…{}", job.current_step().map(|s| format!(" {s}")).unwrap_or_default())
    };
    let room = (width as usize).saturating_sub(4);
    let name = truncate(&name, room);
    let branch = truncate(&branch, room.saturating_sub(name.width()));
    let what = truncate(&what, room.saturating_sub(name.width() + branch.width()));
    let (glyph, color) = if job.question.is_some() { ("?", theme.yellow) } else { (spinner(now), theme.green) };
    Line::from(vec![
        Span::raw(" "),
        Span::styled(glyph, theme.fg(color)),
        Span::raw(" "),
        Span::styled(name, theme.faint()),
        Span::styled(branch, theme.faint()),
        Span::styled(what, theme.fg(color)),
    ])
}

// --------------------------------------------------------------------- jobs

/// Right-hand jobs pane: each create/delete job with its stages, any
/// question it is waiting on (answered with Tab-focus + key), and output.
fn draw_jobs(f: &mut Frame, app: &App, theme: &Theme, area: Rect, now: i64) {
    use crate::jobs::{JobKind, JobState, Question};
    let jobs = &app.jobs;
    let mut block = pane_block(theme, title("Jobs", theme), jobs.focused);
    if jobs.waiting() && !jobs.focused {
        block = block.border_style(theme.bold(theme.yellow)).title_bottom(Line::styled(" ⇥ answer ", theme.bold(theme.yellow)).alignment(Alignment::Right));
    } else if jobs.focused {
        block = block.title_bottom(Line::styled(" ⇥ back  x dismiss ", theme.faint()).alignment(Alignment::Right));
    }
    let inner = block.inner(area);
    f.render_widget(block, area);
    let width = inner.width as usize;

    let mut lines: Vec<Line> = Vec::new();
    // `lead` (e.g. "   ✓ ") then `text`, wrapped with continuation lines
    // indented under the text.
    let mut push = |lead: Vec<Span<'static>>, text: &str, style: ratatui::style::Style| {
        let indent: usize = lead.iter().map(|s| s.content.width()).sum();
        let mut rows = wrap_words(text, width.saturating_sub(indent).max(8)).into_iter();
        let mut first = lead;
        first.push(Span::styled(rows.next().unwrap_or_default(), style));
        lines.push(Line::from(first));
        for r in rows {
            lines.push(Line::from(vec![Span::raw(" ".repeat(indent)), Span::styled(r, style)]));
        }
    };
    for (i, job) in jobs.list.iter().enumerate() {
        if i > 0 {
            push(Vec::new(), "", theme.faint());
        }
        let (glyph, color) = match &job.state {
            JobState::Running => (spinner(now), theme.accent),
            JobState::Waiting => ("?", theme.yellow),
            JobState::Done => ("✓", theme.green),
            JobState::Failed(_) => ("✗", theme.red),
        };
        let cursor = jobs.focused && i == jobs.cursor;
        push(
            vec![Span::styled(if cursor { "▌" } else { " " }, theme.bold(theme.border_focus)), Span::styled(format!("{glyph} "), theme.bold(color))],
            &job.label,
            theme.bold(theme.text),
        );
        for step in &job.steps {
            let (g, style) = match (&job.state, step.done) {
                (_, true) => ("✓".to_string(), theme.faint()),
                (JobState::Running, false) => (spinner(now).to_string(), theme.fg(theme.text)),
                (JobState::Failed(_), false) => ("✗".to_string(), theme.fg(theme.red)),
                _ => ("·".to_string(), theme.faint()),
            };
            push(vec![Span::raw("   "), Span::styled(format!("{g} "), style)], &step.text, style);
        }
        if let JobState::Failed(e) = &job.state {
            push(vec![Span::raw("   ")], e, theme.fg(theme.red));
        }
        let choice = |key: &str, danger: bool| -> Vec<Span<'static>> {
            vec![Span::raw("   "), Span::styled(format!("{key:<6}"), theme.bold(if danger { theme.red } else { theme.yellow }))]
        };
        match (&job.question, &job.kind) {
            (Some(Question::BranchExists { local, remote }), JobKind::Create { req, .. }) => {
                let b = &req.branch;
                let header = match (local, remote) {
                    (true, true) => format!("Branch \"{b}\" exists locally and on origin"),
                    (_, true) => format!("Branch \"{b}\" exists on origin"),
                    _ => format!("Branch \"{b}\" already exists locally"),
                };
                push(vec![Span::raw("   ")], &header, theme.bold(theme.yellow));
                if *remote {
                    let pull = if *local { "Pull remote (reset local to origin)".to_string() } else { format!("Pull remote (track origin/{b})") };
                    push(choice("⏎ / p", false), &pull, theme.fg(theme.text));
                    let disc = if *local { "Create disconnected (deletes local, no tracking)" } else { "Create disconnected (new branch from base, no tracking)" };
                    push(choice("c", *local), disc, theme.fg(theme.text));
                } else {
                    push(choice("⏎ / r", false), "Reuse existing local branch", theme.fg(theme.text));
                    push(choice("d", true), "Delete local branch and re-create from base", theme.fg(theme.text));
                }
                push(choice("n", false), "Cancel", theme.fg(theme.text));
            }
            (Some(Question::RunScript { repo_id, .. }), _) => {
                push(vec![Span::raw("   ")], "Run the repo's startup script?", theme.bold(theme.yellow));
                push(vec![Span::raw("   ")], &am_core::worktrees::script_path(repo_id).display().to_string(), theme.faint());
                let where_ = if app.settings.startup_script_in_window() { "Run in a new terminal window" } else { "Run here (output below)" };
                push(choice("⏎ / y", false), where_, theme.fg(theme.text));
                push(choice("n", false), "Skip", theme.fg(theme.text));
            }
            _ => {}
        }
        for l in job.log.iter().skip(job.log.len().saturating_sub(200)) {
            push(vec![Span::raw("   ")], l, theme.faint());
        }
    }
    // Keep the bottom (newest output) in view.
    let scroll = (lines.len() as u16).saturating_sub(inner.height);
    f.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

/// Greedy word wrap to `width` display columns; words longer than a line
/// are split.
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut out = vec![String::new()];
    for word in text.split(' ') {
        let cur = out.last_mut().unwrap();
        let need = if cur.is_empty() { word.width() } else { cur.width() + 1 + word.width() };
        if need <= width {
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
            continue;
        }
        if !cur.is_empty() {
            out.push(String::new());
        }
        for ch in word.chars() {
            let cur = out.last_mut().unwrap();
            if cur.width() + unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0) > width {
                out.push(String::new());
            }
            out.last_mut().unwrap().push(ch);
        }
    }
    out
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

/// A key-bar entry: a key hint or a `│` separator.
enum KeyItem {
    Key { key: &'static str, label: &'static str, available: bool },
    Sep,
}

impl KeyItem {
    fn width(&self) -> usize {
        match self {
            KeyItem::Key { key, label, .. } => key.width() + label.width() + 3,
            KeyItem::Sep => 2,
        }
    }
}

/// The hints that fit in `width` (lower-priority ones drop first), after a
/// one-cell left margin. Shared by drawing and click hit-testing.
fn keybar_items(app: &App, width: usize) -> Vec<KeyItem> {
    // While jobs are showing, Tab moves between the list and the pane.
    let mut keys: Vec<(&'static str, &'static str, u8)> = Vec::new();
    if app.jobs.focused {
        keys.extend([("⇥", "back to list", 0), ("j/k", "select job", 1), ("x", "dismiss", 1), ("|", "", 0), ("q", "quit", 0)]);
    } else {
        if app.jobs.visible() {
            keys.extend([("⇥", if app.jobs.waiting() { "answer" } else { "jobs" }, 0), ("|", "", 0)]);
        }
        keys.extend(KEYS.iter().copied());
    }
    let pick = |max_prio: u8| -> Vec<KeyItem> {
        let mut items = Vec::new();
        let mut last_sep = true;
        for &(k, label, prio) in &keys {
            if prio > max_prio {
                continue;
            }
            if k == "|" {
                if !last_sep {
                    items.push(KeyItem::Sep);
                    last_sep = true;
                }
                continue;
            }
            items.push(KeyItem::Key { key: k, label, available: app.key_available(k) });
            last_sep = false;
        }
        items
    };
    for prio in (0..=3).rev() {
        let candidate = pick(prio);
        // +1 for the left margin.
        if candidate.iter().map(KeyItem::width).sum::<usize>() < width {
            return candidate;
        }
    }
    pick(0)
}

/// Right side of the key bar: a transient notice, else the offline warning.
fn keybar_status(app: &App) -> Option<(String, bool)> {
    let h = &app.snapshot.health;
    let offline = match (h.github_failing, h.linear_failing) {
        (true, true) => Some("GitHub + Linear"),
        (true, false) => Some("GitHub"),
        (false, true) => Some("Linear"),
        _ => None,
    };
    match (&app.notice, offline) {
        (Some(n), _) => Some((format!("{n} "), false)),
        (None, Some(src)) => Some((format!("⚠ {src} offline — cached "), true)),
        _ => None,
    }
}

fn keybar_areas(app: &App, area: Rect) -> [Rect; 2] {
    let status_w = keybar_status(app).map_or(0, |(s, _)| s.width());
    Layout::horizontal([Constraint::Min(0), Constraint::Length(status_w as u16)]).areas(area)
}

/// The key-bar hint under a click (`"⏎"`, `"n"`, `"⇥"`…); dimmed hints
/// count too, the key itself refuses them like a key press would.
pub fn keybar_key_at(app: &App, area: Rect, x: u16, y: u16) -> Option<&'static str> {
    let keys = dashboard_areas(app, area).keys;
    let [keys, _] = keybar_areas(app, keys);
    if !contains(keys, x, y) {
        return None;
    }
    let mut col = keys.x as usize + 1;
    for item in keybar_items(app, keys.width as usize) {
        let w = item.width();
        if let KeyItem::Key { key, .. } = item
            && (col..col + w).contains(&(x as usize))
        {
            return Some(key);
        }
        col += w;
    }
    None
}

fn draw_keybar(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let [keys_area, status_area] = keybar_areas(app, area);
    if let Some((text, warn)) = keybar_status(app) {
        let color = if warn { theme.yellow } else { theme.accent };
        f.render_widget(Paragraph::new(Line::from(Span::styled(text, theme.fg(color)))), status_area);
    }
    let mut spans = vec![Span::raw(" ")];
    for item in keybar_items(app, keys_area.width as usize) {
        match item {
            KeyItem::Sep => spans.push(Span::styled("│ ", theme.fg(theme.border))),
            // Actions the selected worktree can't take stay in place, dimmed.
            KeyItem::Key { key, label, available: true } => {
                spans.push(Span::styled(key, theme.bold(theme.accent)));
                spans.push(Span::styled(format!(" {label}  "), theme.faint()));
            }
            KeyItem::Key { key, label, available: false } => {
                let off = theme.faint().add_modifier(Modifier::DIM);
                spans.push(Span::styled(key, off));
                spans.push(Span::styled(format!(" {label}  "), off));
            }
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), keys_area);
}

fn draw_help(f: &mut Frame, theme: &Theme, area: Rect) {
    let keys: &[(&str, &str)] = &[
        ("j / k  ↑ / ↓", "move"),
        ("home / G", "top / bottom"),
        ("ctrl-d / ctrl-u", "page down / up"),
        ("enter", "open (IDE; resumes last agent session)"),
        ("g / l / p", "open PR / Linear ticket / Linear project"),
        ("w", "toggle log pane"),
        ("n / d / tab", "new / delete worktree / focus jobs pane"),
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
    lines.push(Line::from(vec![
        Span::styled("✦ gemini", theme.fg(theme.blue)),
        Span::styled(" session from another agent plugin, by id (no badge: Claude Code)", theme.faint()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("today 4.1M", theme.faint()),
        Span::styled(" tokens used today incl. cache (s → token usage: all / visible rows)", theme.faint()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("✎ draft", theme.fg(theme.yellow)),
        Span::styled(" plugin still being trained (Settings → Agent plugins publishes it)", theme.faint()),
    ]));
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
    use am_core::context::ContextUsage;
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
                agent: Default::default(),
            }),
            git: Some(GitStatus { ahead: 2, behind: 0, dirty: 3 }),
            last_commit: Some(CommitInfo { hash: "eaa3440".into(), message: "bump".into(), timestamp: NOW - 240 }),
            pr: None,
            linear: None,
            has_terminal: false,
            open_ide: None,
            running_processes: vec![],
            context: None,
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
        refactor.context = Some(ContextUsage { tokens: 121_000, model: Some("claude-opus-5-5".into()) });
        let mut waiting = view("w2", "r1", "fix/offline-cache", Some(Status::Waiting));
        waiting.git = Some(GitStatus::default());
        waiting.context = Some(ContextUsage { tokens: 185_000, model: Some("claude-opus-5-5".into()) });
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
            today_tokens: None,
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

    fn update(latest: &str) -> am_core::update_check::Update {
        am_core::update_check::Update {
            latest: latest.into(),
            current: "2.1.1".into(),
            hint: am_core::update_check::InstallMethod::Homebrew.upgrade_hint(),
        }
    }

    #[test]
    fn settings_show_update_banner_with_upgrade_command() {
        let mut app = fixture();
        app.update_available = Some(update("2.2.0"));
        app.settings_editor = Some(crate::settings_ui::SettingsEditor::new(&app.settings));
        insta::assert_snapshot!(render(&app, 100, 24));
        app.settings.check_for_updates = false;
        assert!(!render(&app, 100, 24).contains("is available"));
    }

    #[test]
    fn dashboard_update_available() {
        let mut app = fixture();
        app.update_available = Some(update("2.2.0"));
        insta::assert_snapshot!(render(&app, 120, 16));
    }

    #[test]
    fn update_label_shrinks_then_hides_and_follows_the_setting() {
        let mut app = fixture();
        app.update_available = Some(update("2.2.0"));
        assert_eq!(update_label(&app, 120).as_deref(), Some(" ⬆ v2.2.0 available "));
        assert_eq!(update_label(&app, 30).as_deref(), Some(" ⬆ v2.2.0 "));
        assert_eq!(update_label(&app, 20), None);
        app.settings.check_for_updates = false;
        assert_eq!(update_label(&app, 120), None);
    }

    #[test]
    fn tokens_label_names_the_scope_then_shrinks_and_hides() {
        let mut app = fixture();
        assert_eq!(tokens_label(&app, 120), None, "no total yet");
        let mut snap = app.snapshot.clone();
        snap.today_tokens = Some(4_120_000);
        app.apply(snap);
        assert_eq!(tokens_label(&app, 120).as_deref(), Some(" today 4.1M "));
        app.settings.daily_tokens_scope = "visible".into();
        assert_eq!(tokens_label(&app, 120).as_deref(), Some(" today (visible) 4.1M "));
        // ` 1 of 3 ` (8) + borders (4) + ` today 4.1M ` (12) = 24.
        assert_eq!(tokens_label(&app, 30).as_deref(), Some(" today 4.1M "));
        assert_eq!(tokens_label(&app, 23), None);
        app.settings.show_daily_tokens = false;
        assert_eq!(tokens_label(&app, 120), None);
    }

    #[test]
    fn dashboard_shows_todays_tokens_bottom_right() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        snap.today_tokens = Some(182_400_000);
        app.apply(snap);
        insta::assert_snapshot!(render(&app, 120, 16));
    }

    #[test]
    fn dashboard_narrow_stacks_panes() {
        insta::assert_snapshot!(render(&fixture(), 80, 24));
    }

    #[test]
    fn gemini_rows_carry_an_agent_badge() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        snap.groups[0].worktrees[1].agent.as_mut().unwrap().agent = HarnessKind::GEMINI;
        app.apply(snap);
        let row = |w: u16| render(&app, w, 12).lines().find(|l| l.contains("fix/offline-cache")).unwrap().to_string();
        assert!(row(120).contains("fix/offline-cache ✦ gemini"), "{}", row(120));
        let narrow = row(90);
        assert!(narrow.contains(" ✦") && !narrow.contains("✦ gemini"), "{narrow}");
        // Claude (the default) rows stay unbadged.
        assert!(!render(&app, 120, 12).lines().any(|l| l.contains("feature/refactor") && l.contains('✦')));
    }

    #[test]
    fn draft_plugin_rows_are_badged() {
        use am_core::harness::manifest::{Manifest, ManifestProvider};
        let manifest = Manifest::parse(
            r#"{"id":"toycli","name":"Toy CLI","program":"toycli","status":"draft",
                "hooks":{"settingsPath":"/tmp/toy.json","projectDirVar":"TOY_DIR"},"events":{"Done":"Stop"}}"#,
        )
        .unwrap();
        let toy = HarnessKind::leak(Box::new(ManifestProvider { manifest, source: "/tmp/toycli.json".into() }));
        assert!(toy.is_draft());
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        snap.groups[0].worktrees[1].agent.as_mut().unwrap().agent = toy;
        app.apply(snap);
        let row = |w: u16| render(&app, w, 12).lines().find(|l| l.contains("fix/offline-cache")).unwrap().to_string();
        assert!(row(120).contains("fix/offline-cache ✦ toycli ✎ draft"), "{}", row(120));
        let narrow = row(90);
        assert!(narrow.contains(" ✦✎") && !narrow.contains("draft"), "{narrow}");
        assert!(!render(&app, 120, 12).lines().any(|l| l.contains("feature/refactor") && l.contains('✎')));
    }

    #[test]
    fn settings_changes_keep_the_plugin_page_open() {
        // Regression: toggling a plugin persists settings, and rebuilding the
        // editor on that change used to close the page under the user.
        let mut app = fixture();
        let mut ed = crate::settings_ui::SettingsEditor::new(&app.settings);
        ed.plugins = Some(crate::plugins_ui::PluginEditor::new(vec![], "claude", vec![], String::new()));
        app.settings_editor = Some(ed);
        app.set_settings(Settings { disabled_agents: vec!["gemini".into()], ..app.settings.clone() });
        assert!(app.settings_editor.as_ref().is_some_and(|e| e.plugins.is_some()));
    }

    #[test]
    fn clicks_on_the_hidden_field_list_leave_the_plugin_page_alone() {
        let mut ed = crate::settings_ui::SettingsEditor::new(&Settings::default());
        let plugins_row = crate::settings_ui::FIELDS.iter().position(|f| f.key == "agentPlugins").unwrap();
        ed.cursor = plugins_row;
        ed.plugins = Some(crate::plugins_ui::PluginEditor::new(vec![], "claude", vec![], String::new()));
        // Same row as the cursor (would be ⏎) and another row (would move the cursor).
        for i in [plugins_row, 0] {
            assert!(matches!(ed.click(i), crate::settings_ui::Outcome::None));
            assert!(ed.plugins.is_some());
            assert_eq!(ed.cursor, plugins_row);
        }
    }

    #[test]
    fn agent_plugins_page() {
        use crate::plugins_ui::{PluginEditor, PluginRow};
        let mut app = fixture();
        let mut ed = crate::settings_ui::SettingsEditor::new(&app.settings);
        let row = |id: &str, name: &str, source: &str, enabled, hooks, present| PluginRow {
            id: id.into(),
            name: name.into(),
            source: source.into(),
            enabled,
            hooks,
            present,
            settings_path: format!("~/.{id}/settings.json"),
            note: None,
            builtin: source == "built-in",
            draft: false,
            issues: 0,
            captures: 0,
        };
        let mut pl = PluginEditor::new(
            vec![
                row("claude", "Claude Code", "built-in", true, true, true),
                row("gemini", "Gemini CLI", "built-in", true, false, true),
                row("mycli", "My CLI", "mycli.json", false, false, false),
            ],
            "claude",
            vec!["broken.json: expected value at line 1 column 3".into()],
            "~/.agent-monitor/agents".into(),
        );
        pl.cursor = 1;
        ed.plugins = Some(pl);
        app.settings_editor = Some(ed);
        insta::assert_snapshot!(render(&app, 100, 24));
    }

    #[test]
    fn agent_plugins_page_with_a_draft_in_training() {
        use crate::plugins_ui::{Confirm, PluginEditor, PluginRow};
        let mut app = fixture();
        let mut ed = crate::settings_ui::SettingsEditor::new(&app.settings);
        let row = |id: &str, name: &str, source: &str, draft: bool, issues: usize| PluginRow {
            id: id.into(),
            name: name.into(),
            source: source.into(),
            enabled: true,
            hooks: true,
            present: true,
            settings_path: format!("~/.{id}/settings.json"),
            note: None,
            builtin: source == "built-in",
            draft,
            issues,
            captures: 12,
        };
        let mut pl = PluginEditor::new(
            vec![row("claude", "Claude Code", "built-in", false, 0), row("toycli", "Toy CLI", "toycli.json", true, 3)],
            "claude",
            vec![],
            "~/.agent-monitor/agents".into(),
        );
        pl.cursor = 1;
        ed.plugins = Some(pl);
        app.settings_editor = Some(ed);
        insta::assert_snapshot!("plugins_draft_selected", render(&app, 100, 20));

        let pl = app.settings_editor.as_mut().unwrap().plugins.as_mut().unwrap();
        pl.confirm = Some(Confirm::Publish("toycli".into()));
        insta::assert_snapshot!("plugins_confirm_publish", render(&app, 100, 20));

        let pl = app.settings_editor.as_mut().unwrap().plugins.as_mut().unwrap();
        pl.confirm = None;
        pl.cursor = 2;
        pl.train = Some("cursor-agent".into());
        insta::assert_snapshot!("plugins_train_new", render(&app, 100, 20));
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

    /// Cell (x, y) of the first occurrence of `needle` on screen.
    fn find(app: &App, w: u16, h: u16, needle: &str) -> (u16, u16) {
        let screen = render(app, w, h);
        screen
            .lines()
            .enumerate()
            .find_map(|(y, line)| line.find(needle).map(|b| (line[..b].chars().count() as u16, y as u16)))
            .unwrap_or_else(|| panic!("{needle:?} not on screen:\n{screen}"))
    }

    fn click(app: &mut App, w: u16, h: u16, (x, y): (u16, u16), double: bool) -> crate::app::Action {
        app.on_mouse(crate::app::Mouse::Click { x, y, double }, Rect::new(0, 0, w, h))
    }

    #[test]
    fn clicking_a_row_selects_it_and_a_double_click_opens_it() {
        let mut app = fixture();
        let at = find(&app, 120, 16, "main");
        assert_eq!(click(&mut app, 120, 16, at, false), crate::app::Action::None);
        assert_eq!(app.selected_view().unwrap().worktree.branch, "main");
        // Headers and blank space select nothing.
        let header = find(&app, 120, 16, "agent-monitor");
        click(&mut app, 120, 16, header, false);
        click(&mut app, 120, 16, (5, 10), false);
        assert_eq!(app.selected, 2);
        let at = find(&app, 120, 16, "fix/offline-cache");
        assert!(matches!(click(&mut app, 120, 16, at, true), crate::app::Action::Open(v) if v.worktree.branch == "fix/offline-cache"));
    }

    #[test]
    fn double_click_on_a_deleting_row_does_not_open_it() {
        let mut app = fixture();
        app.deleting.insert("w2".into(), false);
        let at = find(&app, 120, 16, "fix/offline-cache");
        assert_eq!(click(&mut app, 120, 16, at, true), crate::app::Action::None);
        assert_eq!(app.notice.as_deref(), Some("Deleting in the background…"));
    }

    #[test]
    fn clicks_follow_the_scrolled_list() {
        let mut app = fixture();
        app.on_key(Key::End);
        // 7 rows: a 3-line list, so the top of the list has scrolled away.
        let screen = render(&app, 120, 7);
        assert!(!screen.contains("feature/refactor"), "{screen}");
        let at = find(&app, 120, 7, "main");
        click(&mut app, 120, 7, at, false);
        assert_eq!(app.selected, 2);
        let header = find(&app, 120, 7, "gridsight");
        click(&mut app, 120, 7, header, false);
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn wheel_moves_the_selection_and_clamps() {
        let mut app = fixture();
        let area = Rect::new(0, 0, 120, 16);
        app.on_mouse(crate::app::Mouse::Scroll(1), area);
        app.on_mouse(crate::app::Mouse::Scroll(1), area);
        app.on_mouse(crate::app::Mouse::Scroll(1), area);
        assert_eq!(app.selected, 2);
        app.on_mouse(crate::app::Mouse::Scroll(-1), area);
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn key_bar_hints_are_clickable() {
        let mut app = fixture();
        let help = find(&app, 120, 16, "? help");
        assert_eq!(keybar_key_at(&app, Rect::new(0, 0, 120, 16), help.0 + 3, help.1), Some("?"));
        click(&mut app, 120, 16, help, false);
        assert!(app.show_help);
        // Any click closes help.
        click(&mut app, 120, 16, (0, 0), false);
        assert!(!app.show_help);
        let sep = find(&app, 120, 16, "│ g PR");
        assert_eq!(keybar_key_at(&app, Rect::new(0, 0, 120, 16), sep.0, sep.1), None);
        let open = find(&app, 120, 16, "⏎ open");
        assert!(matches!(click(&mut app, 120, 16, open, false), crate::app::Action::Open(_)));
        let settings = find(&app, 120, 16, "s settings");
        click(&mut app, 120, 16, settings, false);
        assert!(app.settings_editor.is_some());
    }

    #[test]
    fn clicks_close_popups_and_modals_ignore_the_mouse() {
        let mut app = fixture();
        app.popup = Some("hello".into());
        app.on_mouse(crate::app::Mouse::Scroll(1), Rect::new(0, 0, 120, 16));
        assert!(app.popup.is_some());
        click(&mut app, 120, 16, (0, 0), false);
        assert!(app.popup.is_none());
        app.on_key(Key::Char('n'));
        assert!(app.modal.is_some());
        let at = find(&app, 120, 30, "main");
        assert_eq!(click(&mut app, 120, 30, at, true), crate::app::Action::None);
        assert!(app.modal.is_some());
    }

    #[test]
    fn jobs_pane_takes_focus_on_click_and_gives_it_back_on_a_row_click() {
        let mut app = fixture();
        app.jobs.start_delete("w2".into(), "fix/offline-cache");
        assert!(app.jobs.visible());
        let jobs = find(&app, 120, 16, "Jobs");
        click(&mut app, 120, 16, (jobs.0 + 2, jobs.1 + 2), false);
        assert!(app.jobs.focused);
        // The key bar switched to the pane's keys; ⇥ goes back to the list.
        let tab = find(&app, 120, 16, "⇥ back to list");
        click(&mut app, 120, 16, tab, false);
        assert!(!app.jobs.focused);
        click(&mut app, 120, 16, (jobs.0 + 2, jobs.1 + 2), false);
        assert!(app.jobs.focused);
        let at = find(&app, 120, 16, "main");
        click(&mut app, 120, 16, at, false);
        assert!(!app.jobs.focused);
        assert_eq!(app.selected_view().unwrap().worktree.branch, "main");
    }

    #[test]
    fn clicking_a_settings_field_selects_it() {
        let mut app = fixture();
        app.on_key(Key::Char('s'));
        let at = find(&app, 120, 50, "Mouse");
        click(&mut app, 120, 50, at, false);
        let ed = app.settings_editor.as_ref().unwrap();
        assert_eq!(ed.field().key, "mouseEnabled");
        // A section header isn't a field.
        let header = find(&app, 120, 50, " Appearance");
        click(&mut app, 120, 50, header, false);
        assert_eq!(app.settings_editor.as_ref().unwrap().field().key, "mouseEnabled");
        app.on_mouse(crate::app::Mouse::Scroll(1), Rect::new(0, 0, 120, 50));
        assert_eq!(app.settings_editor.as_ref().unwrap().field().key, "startupScriptMode");
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
    fn terminal_and_agent_keys_are_gone() {
        let mut app = fixture();
        assert_eq!(app.on_key(Key::Char('t')), crate::app::Action::None);
        assert_eq!(app.on_key(Key::Char('c')), crate::app::Action::None);
        let screen = render(&app, 120, 16);
        assert!(!screen.contains("t terminal") && !screen.contains("c agent"), "{screen}");
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
    fn setup_context_step_fits_every_breadcrumb() {
        let mut app = fixture();
        app.setup = Some(crate::setup::Setup {
            step: crate::setup::Step::Context,
            ide: 0,
            hooks_installed: true,
            agents: vec![(am_core::harness::HarnessKind::CLAUDE, true)],
            hook_binary: None,
            repo_input: "~/".into(),
            repos_added: vec![],
            repo_error: None,
            gh: true,
            linear_key: String::new(),
            integration_field: 0,
            ctx_choice: crate::setup::CONTEXT_CUSTOM,
            ctx_input: "128k".into(),
            ctx_error: None,
        });
        let out = render(&app, 100, 20);
        assert!(out.contains("✓ Integrations  ›  Context"), "breadcrumbs cut off:\n{out}");
        insta::assert_snapshot!(out);
    }

    #[test]
    fn context_bar_turns_yellow_then_red_as_the_window_fills() {
        let theme = Theme::ansi();
        let mut v = fixture().flat[0].clone();
        let at = |v: &mut WorktreeView, tokens| {
            v.context = Some(ContextUsage { tokens, model: None });
            let spans = ctx_spans(v, &theme, 8, 200_000, false);
            let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
            (text, spans[0].style.fg, spans[2].style.fg)
        };
        assert_eq!(at(&mut v, 100_000), ("████░░░░ 100k".into(), Some(theme.green), Some(theme.green)));
        assert_eq!(at(&mut v, 150_000), ("██████░░ 150k".into(), Some(theme.yellow), Some(theme.yellow)));
        assert_eq!(at(&mut v, 190_000), ("███████▋ 190k".into(), Some(theme.red), Some(theme.red)));
        // Past the configured window the scale doubles until it fits (400k) instead of pinning.
        assert_eq!(at(&mut v, 250_000), ("█████░░░ 250k".into(), Some(theme.green), Some(theme.green)));
        assert_eq!(at(&mut v, 700_000), ("███████░ 700k".into(), Some(theme.yellow), Some(theme.yellow)));
        v.context = None;
        let blank: String = ctx_spans(&v, &theme, 8, 200_000, false).iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(blank, " ".repeat(13), "rows without usage keep the column aligned");
    }

    #[test]
    fn context_bar_stays_visible_on_the_reversed_selection() {
        let app = fixture();
        let theme = Theme::ansi();
        let line = worktree_line(&app, &theme, &app.flat[0], "", true, true, &cols_for(&app), 120, NOW);
        assert!(line.style.add_modifier.contains(Modifier::REVERSED));
        let bar = line.spans.iter().find(|s| s.content.contains('█')).expect("bar span");
        assert_eq!(bar.style.fg, Some(theme.green));
        assert!(bar.style.sub_modifier.contains(Modifier::REVERSED), "bar opts out of the reverse");
        let name = line.spans.iter().find(|s| s.content.contains("feature/refactor")).unwrap();
        assert_eq!(name.style.fg, None, "the rest of the row still drops its colours");
    }

    fn cols_for(app: &App) -> Columns {
        Columns { ticket: 7, pr: 12, pr_words: true, git: 5, age: 3, ctx_bar: 8, ctx_window: app.settings.context_window_tokens }
    }

    #[test]
    fn context_column_follows_its_setting() {
        let mut app = fixture();
        assert!(render(&app, 120, 16).contains("121k"));
        app.settings.show_context_window = false;
        let out = render(&app, 120, 16);
        assert!(!out.contains("121k") && !out.contains('░'));
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
    fn delete_runs_in_the_jobs_pane() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        let nick = "Confirm the results read and the limit basis against a real customer feeder";
        snap.groups[0].worktrees[0].worktree.custom_name = Some(nick.into());
        app.apply(snap);
        app.on_key(Key::Char('d'));
        // Fixture paths don't exist, so this is the one-step stale-entry delete.
        let action = app.on_key(Key::Enter);
        let crate::app::Action::Delete(job, ..) = action else { panic!("{action:?}") };
        assert!(app.modal.is_none(), "no blocking box");
        app.jobs.step(job, "Removing worktree…".into());
        let screen = render(&app, 120, 16);
        assert!(screen.contains("Jobs") && screen.contains("feeder"), "label wraps in the pane:\n{screen}");
        assert!(screen.contains("deleting…"), "{screen}");
        assert!(screen.contains("⇥ jobs"), "{screen}");
        insta::assert_snapshot!("delete_in_jobs_pane", screen);

        // The row stays greyed and takes no actions; other rows still work.
        assert_eq!(app.on_key(Key::Enter), crate::app::Action::None);
        assert_eq!(app.on_key(Key::Char('d')), crate::app::Action::None);
        assert!(app.modal.is_none(), "can't delete it twice");
        app.on_key(Key::Char('j'));
        assert!(matches!(app.on_key(Key::Enter), crate::app::Action::Open(_)));

        // Done: the pane lingers, then closes.
        let t = std::time::Instant::now();
        app.jobs.finish(job, Ok(()), t);
        assert!(app.jobs.visible());
        app.jobs.tick(t + crate::jobs::DONE_LINGER);
        assert!(!app.jobs.visible());
    }

    fn start_create(app: &mut App, branch: &str) -> u64 {
        let repo = app.snapshot.groups[0].repo.clone();
        let req = am_core::worktrees::CreateRequest { branch: branch.into(), ..Default::default() };
        let id = app.jobs.start_create(repo, req);
        app.rebuild_rows();
        id
    }

    #[test]
    fn create_shows_a_placeholder_row_and_progress() {
        let mut app = fixture();
        let job = start_create(&mut app, "feature/new-thing");
        app.jobs.step(job, "Fetching origin/main…".into());
        app.jobs.step(job, "Adding worktree…".into());
        app.rebuild_rows();
        assert!(app.rows.contains(&Row::Pending(job)));
        let before = app.flat.len();
        app.on_key(Key::Char('G'));
        assert_eq!(app.selected, before - 1, "the placeholder takes no selection index");
        let screen = render(&app, 120, 16);
        assert!(screen.contains("feature/new-thing creating… Adding worktree…"), "{screen}");
        insta::assert_snapshot!("create_in_progress", screen);

        // Once the worktree exists the placeholder stays until the snapshot lists it.
        app.jobs.set_worktree(job, "w-new".into());
        app.jobs.finish(job, Ok(()), std::time::Instant::now());
        app.rebuild_rows();
        assert!(app.rows.contains(&Row::Pending(job)));
        let mut snap = app.snapshot.clone();
        let mut v = snap.groups[0].worktrees[0].clone();
        v.worktree.id = "w-new".into();
        v.worktree.branch = "feature/new-thing".into();
        snap.groups[0].worktrees.push(v);
        app.apply(snap);
        assert!(!app.rows.iter().any(|r| matches!(r, Row::Pending(_))));
    }

    #[test]
    fn branch_exists_question_is_answered_in_the_pane() {
        let mut app = fixture();
        let job = start_create(&mut app, "feature/on-origin");
        app.jobs.ask(job, crate::jobs::Question::BranchExists { local: false, remote: true });
        let screen = render(&app, 120, 18);
        assert!(screen.contains("Pull remote") && screen.contains("⇥ answer"), "{screen}");
        insta::assert_snapshot!("create_branch_exists_question", screen);
        // Dashboard keys still go to the list until Tab focuses the pane.
        app.on_key(Key::Char('j'));
        assert_eq!(app.selected, 1);
        app.on_key(Key::Tab);
        assert!(app.jobs.focused);
        let action = app.on_key(Key::Char('p'));
        assert!(
            matches!(&action, crate::app::Action::Create(j, _, req) if *j == job && req.mode == am_core::worktrees::CreateMode::TrackRemote),
            "{action:?}"
        );
        assert!(!app.jobs.focused);
    }

    #[test]
    fn failed_job_stays_until_dismissed() {
        let mut app = fixture();
        let job = start_create(&mut app, "feature/broken");
        app.jobs.step(job, "Adding worktree…".into());
        app.jobs.finish(job, Err("git: fatal: invalid reference: nope".into()), std::time::Instant::now());
        app.rebuild_rows();
        assert!(!app.rows.iter().any(|r| matches!(r, Row::Pending(_))), "no placeholder for a failed create");
        let screen = render(&app, 120, 16);
        assert!(screen.contains("invalid reference"), "{screen}");
        insta::assert_snapshot!("create_failed", screen);
        app.on_key(Key::Tab);
        app.on_key(Key::Char('x'));
        assert!(!app.jobs.visible());
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
        assert!(!app.key_available("⏎") && !app.key_available("g"));

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
                Row::Pending(_) => "pending",
            })
            .collect();
        assert_eq!(kinds, ["repo", "project", "wt", "spacer", "project", "wt", "spacer", "repo", "project", "wt"]);
    }

    #[test]
    fn wrap_words_wraps_and_splits_long_words() {
        assert_eq!(wrap_words("one two three", 7), vec!["one two", "three"]);
        assert_eq!(wrap_words("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap_words("", 4), vec![""]);
    }

    #[test]
    fn truncate_respects_display_width() {
        assert_eq!(truncate("feature/refactor", 8), "feature…");
        assert_eq!(truncate("short", 8), "short");
    }
}
