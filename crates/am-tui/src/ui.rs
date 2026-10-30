//! Rendering. lazygit-style titled panes: `[1] Worktrees` and `[2] Subagents`,
//! a context line for the selection, and a status bar with a mode pill.
//! ratatui diffs each frame against the previous one cell by cell, so a
//! redraw only writes what changed — no full-screen clears, no flicker.

use am_core::harness::HarnessKind;
use am_core::context::ContextUsage;
use am_core::model::{ChecksStatus, SessionView, WorktreeView};
use am_core::settings::DailyTokensMode;
use am_core::status::Status;
use am_core::{context, timefmt};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use std::cell::Cell;
use unicode_width::UnicodeWidthStr;

use crate::actions::{ActionBar, BarHit, Entry};
use crate::app::{FOLD_CLOSED, FOLD_OPEN, App, Key, Row};
use crate::keymap::Command;
use crate::theme::{Theme, status_glyph, status_label};

thread_local! {
    /// `compactView` for the frame being drawn (set by `draw`).
    static COMPACT: Cell<bool> = const { Cell::new(false) };
}

/// The compact view: panes keep only their top line, overlays fill the
/// screen, side panes open below and rows use glyphs.
fn compact() -> bool {
    COMPACT.with(Cell::get)
}

/// Room for words rather than glyphs (100 columns; never when compact).
fn wide(width: u16) -> bool {
    !compact() && width >= 100
}

/// A pane's footer (` N of M `, hints): its bottom border, or its top line
/// when compact.
fn foot<'a>(block: Block<'a>, line: Line<'a>) -> Block<'a> {
    if compact() { block.title(line) } else { block.title_bottom(line) }
}

/// The row a pane's footer is drawn on.
fn foot_y(rect: Rect) -> u16 {
    if compact() { rect.y } else { rect.y + rect.height.saturating_sub(1) }
}

/// Border cells at a pane's left or right edge.
fn edge() -> u16 {
    u16::from(!compact())
}

pub fn draw(f: &mut Frame, app: &App, theme: &Theme, now: i64, log_lines: &[String]) {
    COMPACT.with(|c| c.set(app.settings.compact_view));
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
    // Each layer records what a click on it does; a layer drawn on top
    // starts over, so only the top one takes the mouse.
    let mut hits: Vec<Hit> = Vec::new();
    let areas = dashboard_areas(app, area);
    draw_keybar(f, app, theme, areas.keys, &mut hits);
    if app.show_log {
        draw_log(f, theme, areas.log, log_lines);
    }

    if let Some(reviews) = areas.reviews {
        // `V`: the review page beside the list.
        draw_worktrees(f, app, theme, areas.list, now, &mut hits);
        draw_reviews(f, app, theme, reviews, now, &mut hits);
    } else if app.reviews.is_some() {
        // The full review page takes the list's place.
        draw_reviews(f, app, theme, areas.list, now, &mut hits);
    } else {
        draw_worktrees(f, app, theme, areas.list, now, &mut hits);
    }
    if let Some(jobs) = areas.jobs {
        draw_jobs(f, app, theme, jobs, now, &mut hits);
    }
    // Overlays keep clear of the jobs pane, so it stays in view on every page.
    let area = areas.jobs.map_or(area, |jobs| beside_jobs(area, jobs));
    if app.show_help {
        hits.clear();
        if app.reviews_only() {
            draw_review_help(f, theme, area, &app.keymap);
        } else {
            draw_help(f, theme, area, &app.keymap);
        }
    }
    if let Some(editor) = &app.settings_editor {
        hits.clear();
        match &editor.builder {
            Some(b) => draw_theme_builder(f, theme, area, b, &mut hits),
            None => draw_settings(f, theme, area, editor, shown_update(app), &mut hits),
        }
    }
    if let Some(setup) = &app.setup {
        hits.clear();
        draw_setup(f, theme, area, setup, &mut hits);
    }
    if let Some(modal) = &app.modal {
        hits.clear();
        draw_modal(f, theme, area, modal, now, &mut hits);
    }
    if let Some(msg) = &app.popup {
        hits.clear();
        draw_popup(f, theme, area, msg);
    }
    if app.settings.mouse_enabled
        && let Some(hit) = app.hover.and_then(|(x, y)| hit_at(&hits, x, y))
        && !hit.selected
    {
        hover(f, theme, hit.rect);
    }
    *app.hits.borrow_mut() = hits;
}

/// What a click on part of the screen does. Every layer records these while
/// it draws (`App::hits`), so clicks and hover always match what's shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Exactly what pressing this key does (in whatever layer is on top).
    Key(Key),
    /// A dashboard command from the key bar.
    Command(Command),
    /// A worktree (or one of its session rows) in the dashboard list.
    Worktree(usize),
    /// A stack root's fold arrow or `+N stacked hidden` pill: fold/unfold.
    Fold(usize),
    /// The `⬆ vX.Y.Z available` notice: offer to update.
    Update,
    /// Anywhere in the jobs pane: focus it.
    JobsPane,
    /// A key answering job `.0`'s question, as if pressed with it selected.
    JobKey(usize, Key),
    /// Row `.1` of a list in the top layer: a click moves the list's cursor
    /// there; a click on the row already under the cursor acts on it.
    Item(List, usize),
}

/// The lists `Target::Item` can point into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum List {
    Settings,
    Sort,
    /// A `Kind::Multi` field's checklist.
    Multi,
    Plugins,
    Repos,
    /// The script chooser for one repository.
    RepoScripts,
    Keys,
    ThemeTokens,
    Palette,
    Browser,
    SetupMode,
    SetupPrivacy,
    SetupIde,
    SetupIntegrations,
    SetupReviews,
    SetupContext,
    NewForm,
    /// Autocomplete under a review group's list field.
    Authors,
    /// The review groups page: its list, or a group's form.
    Groups,
    Jobs,
    Reviews,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub rect: Rect,
    pub target: Target,
    /// Already shows the selection highlight, so hover leaves it alone.
    pub selected: bool,
}

/// The hit under a cell: the last one recorded wins (drawn on top).
pub fn hit_at(hits: &[Hit], x: u16, y: u16) -> Option<Hit> {
    hits.iter().rev().find(|h| contains(h.rect, x, y)).copied()
}

/// One full-width row of a list area, if `row` (0-based, after scrolling)
/// is inside it.
fn push_row(hits: &mut Vec<Hit>, area: Rect, row: usize, target: Target, selected: bool) {
    if row < area.height as usize {
        hits.push(Hit { rect: Rect::new(area.x, area.y + row as u16, area.width, 1), target, selected });
    }
}

/// The key a hint like `⏎ save`, `esc cancel` or `x reset` stands for.
fn hint_key(token: &str) -> Option<Key> {
    match token {
        t if t.starts_with('⏎') => Some(Key::Enter),
        "esc" | "any" => Some(Key::Esc),
        "space" => Some(Key::Char(' ')),
        "tab" | "⇥" => Some(Key::Tab),
        "→" => Some(Key::Right),
        "←" => Some(Key::Left),
        t => {
            let mut chars = t.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_graphic() => Some(Key::Char(c)),
                _ => None,
            }
        }
    }
}

/// Make each `key label` hint in `text` (drawn from column `x` on row `y`)
/// clickable. Hints are separated by two spaces, ` · ` or ` — `; ones whose
/// key isn't a single press (`←→ adjust`, `type to filter`) are skipped.
fn hint_hits(hits: &mut Vec<Hit>, text: &str, x: u16, y: u16) {
    let mut segments = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < text.len() {
        let rest = &text[i..];
        let sep = [" · ", " — ", "  "].into_iter().find(|p| rest.starts_with(p)).map_or(0, str::len);
        if sep > 0 {
            segments.push((start, &text[start..i]));
            i += sep;
            start = i;
        } else {
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    segments.push((start, &text[start..]));
    for (at, seg) in segments {
        let lead = seg.len() - seg.trim_start().len();
        let seg = seg.trim();
        let Some(key) = seg.split(' ').next().and_then(hint_key) else { continue };
        let col = x + text[..at + lead].width() as u16;
        hits.push(Hit { rect: Rect::new(col, y, seg.width() as u16, 1), target: Target::Key(key), selected: false });
    }
}

/// Hints in a bordered block's bottom-right title.
fn bottom_hint_hits(hits: &mut Vec<Hit>, text: &str, rect: Rect) {
    let x = (rect.x + rect.width).saturating_sub(1 + text.width() as u16);
    hint_hits(hits, text, x, rect.y + rect.height.saturating_sub(1));
}

/// Hints in a pane's right-aligned footer (`foot`).
fn foot_hint_hits(hits: &mut Vec<Hit>, text: &str, rect: Rect) {
    let x = (rect.x + rect.width).saturating_sub(edge() + text.width() as u16);
    hint_hits(hits, text, x, foot_y(rect));
}

/// Tint the cells under the mouse pointer. Text drawn in the tint's own
/// colour (faint text on `ansi`) switches to the text colour.
fn hover(f: &mut Frame, theme: &Theme, rect: Rect) {
    let bg = theme.hover_bg();
    let rect = rect.intersection(f.area());
    let buf = f.buffer_mut();
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            let cell = &mut buf[(x, y)];
            if cell.fg == bg {
                cell.set_fg(theme.text);
            }
            cell.set_bg(bg);
        }
    }
}

/// Where the dashboard's parts go. `jobs` is `None` while the jobs pane is hidden.
struct DashAreas {
    list: Rect,
    jobs: Option<Rect>,
    /// The review page beside the list (`V`).
    reviews: Option<Rect>,
    log: Rect,
    keys: Rect,
}

/// Narrower than this, the split review page goes under the list.
const SPLIT_SIDE_BY_SIDE: u16 = 100;

fn dashboard_areas(app: &App, area: Rect) -> DashAreas {
    let log_h = match (app.show_log, compact()) {
        (false, _) => 0,
        (true, false) => (area.height / 3).max(5),
        (true, true) => (area.height / 4).max(3),
    };
    let [body, log, keys] =
        Layout::vertical([Constraint::Min(5), Constraint::Length(log_h), Constraint::Length(1)]).areas(area);
    if app.reviews.as_ref().is_some_and(|p| p.split) {
        // List | review page, or stacked when narrow or compact; the jobs
        // pane goes under the review page.
        let [list, side] = if body.width >= SPLIT_SIDE_BY_SIDE && !compact() {
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body)
        } else {
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body)
        };
        if app.jobs.visible() {
            let [reviews, jobs] = Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(side);
            return DashAreas { list, jobs: Some(jobs), reviews: Some(reviews), log, keys };
        }
        return DashAreas { list, jobs: None, reviews: Some(side), log, keys };
    }
    if app.jobs_pane_shown() && compact() {
        // Under the list rather than beside it.
        let jobs_h = (body.height * 2 / 5).clamp(5, 14);
        let [list, jobs] = Layout::vertical([Constraint::Min(4), Constraint::Length(jobs_h)]).areas(body);
        return DashAreas { list, jobs: Some(jobs), reviews: None, log, keys };
    }
    if app.jobs_pane_shown() {
        let pane_w = (body.width / 3).clamp(34, 60).min(body.width.saturating_sub(40));
        let [list, jobs] = Layout::horizontal([Constraint::Min(40), Constraint::Length(pane_w)]).areas(body);
        DashAreas { list, jobs: Some(jobs), reviews: None, log, keys }
    } else {
        DashAreas { list: body, jobs: None, reviews: None, log, keys }
    }
}

/// The bigger part of the screen the jobs pane leaves: left of it, or above it.
fn beside_jobs(area: Rect, jobs: Rect) -> Rect {
    let left = Rect { width: jobs.x - area.x, ..area };
    let above = Rect { height: jobs.y - area.y, ..area };
    if left.area() >= above.area() { left } else { above }
}

/// Settings box, then inside it the update banner, field list and help line.
/// `rows` fields plus `extra` lines (headers, blanks, help) tall.
fn settings_areas(area: Rect, update: Option<&am_core::update_check::Update>, rows: u16, extra: u16) -> (Rect, [Rect; 3]) {
    let w = settings_width(area);
    // Headline, the (possibly wrapped) upgrade hint, then a blank line.
    let banner_h = update.map_or(0, |u| {
        let hint_w = " Update: ".width() + u.hint.width();
        2 + hint_w.div_ceil((w as usize).saturating_sub(2).max(1)) as u16
    });
    let h = area.height.saturating_sub(2 * edge()).min(rows + extra + banner_h);
    let rect = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
    let parts = Layout::vertical([Constraint::Length(banner_h), Constraint::Min(3), Constraint::Length(2)])
        .areas(Block::bordered().inner(rect));
    (rect, parts)
}

/// The settings box's width: 84 columns, or the whole screen when compact.
fn settings_width(area: Rect) -> u16 {
    if compact() { area.width } else { 84.min(area.width.saturating_sub(2)) }
}

/// The settings list as lines: `Some(field index)` for a field, `None` for a
/// section header or blank line. The filter boxes have no headers.
fn settings_lines(ed: &crate::settings_ui::SettingsEditor) -> Vec<Option<usize>> {
    if ed.is_filter_box() {
        return ed.fields().into_iter().map(Some).collect();
    }
    let mut lines = Vec::new();
    let mut section = "";
    for i in ed.fields() {
        let field = &crate::settings_ui::FIELDS[i];
        if field.section != section {
            section = field.section;
            if !lines.is_empty() && !compact() {
                lines.push(None);
            }
            lines.push(None);
        }
        lines.push(Some(i));
    }
    lines
}

fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

const SETTINGS_HINT: &str = " ⏎/space change  ←→ adjust  e edit file  esc close ";
const FILTERS_HINT: &str = " ⏎/space change  ←→ adjust  esc close ";
const MULTI_HINT: &str = " space pick  x any  esc back ";
/// While typing a review group's list (authors, teams, repos).
const AUTHORS_HINT: &str = " ⇥ complete  ↑↓ pick  ⏎ save  esc cancel ";

fn draw_settings(
    f: &mut Frame,
    theme: &Theme,
    area: Rect,
    ed: &crate::settings_ui::SettingsEditor,
    update: Option<&am_core::update_check::Update>,
    hits: &mut Vec<Hit>,
) {
    use crate::settings_ui::{FIELDS, Kind, sort_label};
    // The filter boxes (`f`) are the same editor, cut down.
    let filters = ed.is_filter_box();
    let (name, hint, update) = if filters { (ed.filter_title(), FILTERS_HINT, None) } else { ("Settings ", SETTINGS_HINT, update) };
    let form = ed.groups.as_ref().and_then(|g| g.form.as_ref());
    let typing_list =
        form.is_some_and(|f| f.editing.is_some() && matches!(f.field().kind, crate::review_groups_ui::FormKind::List(_)));
    let hint = if typing_list {
        AUTHORS_HINT
    } else if ed.multi.is_some() {
        MULTI_HINT
    } else {
        hint
    };
    let (rect, [banner, list, help]) = if filters {
        // The groups page needs room for a group's whole form, wrapped.
        let rows = match ed.groups.as_ref() {
            Some(g) => match &g.form {
                Some(form) => {
                    let inner_w = (settings_width(area) as usize).saturating_sub(2);
                    group_form_lines(g.group(form.which), form, inner_w).len() + 2
                }
                None => g.groups.len() + 4,
            },
            None => match &ed.multi {
                Some(m) => m.options.len() + 2,
                None => ed.fields().len(),
            },
        };
        // At the bottom of the page, so the groups above show the edits.
        let (rect, parts) = settings_areas(area, None, rows as u16, 4);
        let y = (area.y + area.height).saturating_sub(rect.height + 2).max(area.y);
        let shift = |r: Rect| Rect { y: r.y + (y - rect.y), ..r };
        (shift(rect), parts.map(shift))
    } else {
        settings_areas(area, update, FIELDS.len() as u16, 16)
    };
    clear(f, theme, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.bold(theme.border_focus))
        .title(Line::from(vec![Span::styled("─", theme.bold(theme.border_focus)), Span::styled(name, theme.bold(theme.text))]))
        .title_bottom(Line::styled(hint, theme.faint()).alignment(Alignment::Right));
    f.render_widget(block, rect);
    // The sub-pages take the same keys (⏎, esc…), so the hints work there too.
    bottom_hint_hits(hits, hint, rect);
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

    if let Some(rp) = &ed.repos {
        hits.clear();
        draw_repos(f, theme, list, help, rp, hits);
        return;
    }
    if let Some(g) = &ed.groups {
        hits.clear();
        match &g.form {
            Some(form) => draw_group_form(f, theme, list, help, ed, g, form, hits),
            None => draw_groups(f, theme, list, help, ed, g, hits),
        }
        return;
    }
    if let Some(pl) = &ed.plugins {
        hits.clear();
        draw_plugins(f, theme, list, help, pl, hits);
        return;
    }
    if let Some(keys) = &ed.keys {
        // While it waits for a key, a click mustn't count as one.
        hits.clear();
        if !keys.capturing {
            draw_key_bindings(f, theme, list, help, keys, hits);
        } else {
            draw_key_bindings(f, theme, list, help, keys, &mut Vec::new());
        }
        return;
    }

    if let Some(m) = &ed.multi {
        hits.clear();
        let header = if m.options == am_core::settings::WORKTREE_STATUS_FILTERS {
            format!(" {}: show worktrees in any of the ticked; none ticked shows all", m.label)
        } else {
            format!(" {}: post the ticked on the worktree's cmux workspace", m.label)
        };
        let mut lines = vec![Line::styled(header, theme.faint()), Line::raw("")];
        for (i, (id, on)) in m.options.iter().zip(&m.picked).enumerate() {
            let sel = i == m.cursor;
            let line = Line::from(vec![
                Span::styled(if sel { "▌" } else { " " }, theme.bold(theme.border_focus)),
                Span::styled(if *on { "[x] " } else { "[ ] " }, theme.fg(if *on { theme.green } else { theme.faint })),
                Span::styled(crate::settings_ui::multi_label(m.options, id).to_string(), if *on { theme.fg(theme.text) } else { theme.faint() }),
            ]);
            push_row(hits, list, lines.len(), Target::Item(List::Multi, i), sel);
            lines.push(if sel { line.style(theme.selected()) } else { line });
        }
        f.render_widget(Paragraph::new(lines), list);
        return;
    }
    if let Some(sort) = &ed.sort {
        hits.clear();
        let header = " Sort order — space on/off · d direction · J/K move · ⏎ save · esc cancel";
        hint_hits(hits, header, list.x, list.y);
        let mut lines = vec![Line::styled(header, theme.faint()), Line::raw("")];
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
            push_row(hits, list, lines.len(), Target::Item(List::Sort, i), sel);
            lines.push(if sel { line.style(theme.selected()) } else { line });
        }
        f.render_widget(Paragraph::new(lines), list);
        return;
    }

    let layout = settings_lines(ed);
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
    let sel = layout.iter().position(|l| *l == Some(ed.cursor)).unwrap_or(0);
    let start = scroll_start(&ed.scroll, layout.len(), height, (sel, sel), |i| layout[i].is_some());
    for (row, entry) in layout.iter().skip(start).take(height).enumerate() {
        if let Some(i) = *entry {
            push_row(hits, list, row, Target::Item(List::Settings, i), i == ed.cursor);
        }
    }
    f.render_widget(Paragraph::new(lines.into_iter().skip(start).take(height).collect::<Vec<_>>()), list);

    let field = ed.field();
    let help_line = match &ed.error {
        Some(e) => Line::styled(format!(" {e}"), theme.fg(theme.red)),
        None if ed.confirm.is_some() => {
            let what = match ed.confirm {
                Some("resetReviewGroups") => "Remove every review group?",
                Some("resetWorktreeFilters") => "Reset the worktree filters to their defaults?",
                _ => "Reset the review filters to their defaults?",
            };
            let text = format!(" {what} y confirm, any other key cancels");
            let at = help.x + format!(" {what}").width() as u16;
            hint_hits(hits, " y confirm", at, help.y + 1);
            Line::styled(text, theme.bold(theme.yellow))
        }
        None => Line::styled(format!(" {}", field.help), theme.faint()),
    };
    f.render_widget(Paragraph::new(vec![Line::raw(""), help_line]), help);
}

/// Review filters → Groups…: the groups in cascade order, then the
/// default group, each with its rules, badge toggle and current count.
fn draw_groups(
    f: &mut Frame,
    theme: &Theme,
    list: Rect,
    help: Rect,
    ed: &crate::settings_ui::SettingsEditor,
    g: &crate::review_groups_ui::GroupsEditor,
    hits: &mut Vec<Hit>,
) {
    let header = " Review groups — a add · x remove · J/K move · ⏎ edit · space on/off · b badge · esc back";
    hint_hits(hits, header, list.x, list.y);
    let mut lines = vec![Line::styled(header, theme.faint()), Line::raw("")];
    let counts = &ed.group_counts;
    let name_w = (0..=g.groups.len()).map(|i| g.label(i).width() + 2).max().unwrap_or(0).clamp(14, 24);
    let rules_w = (list.width as usize).saturating_sub(1 + 5 + name_w + 10 + 6).max(8);
    for i in 0..=g.groups.len() {
        let sel = i == g.cursor;
        let group = g.group((i < g.groups.len()).then_some(i));
        let mark = if i < g.groups.len() { format!("  {:<3}", i + 1) } else { "  ⤓  ".to_string() };
        let rules = match group.summary() {
            r if !r.is_empty() => r,
            _ if i < g.groups.len() => "everything that reaches it".into(),
            _ => "everything left".into(),
        };
        let warn = group_warning(group, &ed.candidates).is_some();
        let rules = match (group.enabled, warn) {
            (false, _) => format!("off · {rules}"),
            (true, true) => format!("⚠ {rules}"),
            (true, false) => rules,
        };
        let on = group.enabled;
        let count = counts.per_group.get(i).map_or(String::new(), |n| format!("{n:>4}"));
        let line = Line::from(vec![
            Span::styled(if sel { "▌" } else { " " }, theme.bold(theme.border_focus)),
            Span::styled(mark, theme.faint()),
            Span::styled(
                format!("{:<name_w$}", truncate(&g.label(i), name_w - 1)),
                if on { theme.bold(theme.text) } else { theme.faint() },
            ),
            Span::styled(
                format!("{:<rules_w$}", truncate(&rules, rules_w - 1)),
                if warn { theme.fg(theme.yellow) } else { theme.faint() },
            ),
            match (on, group.badge) {
                (true, true) => Span::styled("▣ badge   ", theme.fg(theme.green)),
                (false, true) => Span::styled("▣ badge   ", theme.faint()),
                _ => Span::styled("□         ", theme.faint()),
            },
            Span::styled(count, if on { theme.fg(theme.text) } else { theme.faint() }),
        ]);
        push_row(hits, list, lines.len(), Target::Item(List::Groups, i), sel);
        lines.push(if sel { line.style(theme.selected()) } else { line });
    }
    if counts.hidden > 0 {
        lines.push(Line::styled(format!("        {} hidden: the default group's rules don't match them", counts.hidden), theme.faint()));
    }
    f.render_widget(Paragraph::new(lines), list);
    let selected = g.group((g.cursor < g.groups.len()).then_some(g.cursor));
    let help_line = match g.confirm_remove {
        Some(i) => Line::styled(format!(" Remove {}? y confirm, any other key cancels", g.label(i)), theme.bold(theme.yellow)),
        None if group_warning(selected, &ed.candidates).is_some() => Line::styled(
            format!(" ⚠ {} — ⏎ to fix", group_warning(selected, &ed.candidates).unwrap_or_default()),
            theme.fg(theme.yellow),
        ),
        None if !selected.enabled && g.cursor < g.groups.len() => {
            Line::styled(" Off: takes nothing, its PRs fall to the group below (space turns it on)", theme.faint())
        }
        None if !selected.enabled => Line::styled(" Off: everything no group took is hidden (space turns it on)", theme.faint()),
        None if g.cursor < g.groups.len() => {
            Line::styled(" Takes the PRs it matches; the rest fall to the group below", theme.faint())
        }
        None => Line::styled(" Catches what no group took; what its rules don't match is hidden", theme.faint()),
    };
    f.render_widget(Paragraph::new(vec![Line::raw(""), help_line]), help);
}

/// One review group's form: name, rules and badge toggle, with
/// autocomplete under a list being typed.
#[allow(clippy::too_many_arguments)]
fn draw_group_form(
    f: &mut Frame,
    theme: &Theme,
    list: Rect,
    help: Rect,
    ed: &crate::settings_ui::SettingsEditor,
    g: &crate::review_groups_ui::GroupsEditor,
    form: &crate::review_groups_ui::GroupForm,
    hits: &mut Vec<Hit>,
) {
    use crate::review_groups_ui::{FORM, FormKind};
    let label = match form.which {
        Some(i) => g.label(i),
        None => format!("{} (default)", g.label(g.groups.len())),
    };
    let header = format!(" {label} — ⏎ change · ←→ adjust · esc back");
    hint_hits(hits, &header, list.x, list.y);
    f.render_widget(Paragraph::new(Line::styled(header, theme.faint())), Rect { height: 1, ..list });
    let body = Rect { y: list.y + 2, height: list.height.saturating_sub(2), ..list };
    let group = g.group(form.which);
    let rows = group_form_lines(group, form, list.width as usize);
    // Keep the selected field (all its lines) in view.
    let height = body.height as usize;
    let last = rows.iter().rposition(|r| r.0 == form.cursor).unwrap_or(0);
    let first = rows.iter().position(|r| r.0 == form.cursor).unwrap_or(0);
    let start = scroll_start(&form.scroll, rows.len(), height, (first, last), |_| true);
    let mut lines = Vec::new();
    for (n, (i, head, value)) in rows.iter().enumerate().skip(start).take(height) {
        let field = &FORM[*i];
        let sel = *i == form.cursor;
        let vstyle = match field.kind {
            FormKind::Bool if value == "on" => theme.fg(theme.green),
            _ if value == "any" || value == "—" || value == "off" || value == "none" => theme.faint(),
            _ => theme.fg(theme.text),
        };
        let line = Line::from(vec![
            Span::styled(if sel && *head { "▌" } else { " " }, theme.bold(theme.border_focus)),
            Span::styled(format!("  {:<w$}", if *head { field.label } else { "" }, w = FORM_LABEL_W), theme.fg(theme.text)),
            Span::styled(value.clone(), vstyle),
        ]);
        push_row(hits, body, n - start, Target::Item(List::Groups, *i), sel);
        lines.push(if sel { line.style(theme.selected()) } else { line });
    }
    f.render_widget(Paragraph::new(lines), body);
    let unknown = crate::review_groups_ui::unknown(group, form.field().key, &ed.candidates);
    let help_line = if form.editing.is_some() && matches!(form.field().kind, FormKind::List(_)) {
        suggestions_line(form, &ed.candidates, theme, help, hits)
    } else if !unknown.is_empty() {
        Line::styled(format!(" ⚠ {} — ⏎ then ⇥ picks a real one", unknown_text(form.field().key, &unknown)), theme.fg(theme.yellow))
    } else {
        Line::styled(format!(" {}", form.field().help), theme.faint())
    };
    f.render_widget(Paragraph::new(vec![Line::raw(""), help_line]), help);
}

/// `team @con matches nothing in the review list` for a group's list
/// entries that match no candidate.
fn unknown_text(key: &str, unknown: &[String]) -> String {
    let (what, at) = match key {
        "teams" => ("team", true),
        "authors" => ("author", true),
        _ => ("repo", false),
    };
    let names = unknown.iter().map(|u| if at { format!("@{u}") } else { u.clone() }).collect::<Vec<_>>().join(", ");
    let verb = if unknown.len() == 1 { "matches" } else { "match" };
    format!("{what} {names} {verb} nothing in the review list")
}

/// The first list entries of `g` that match nothing, as a warning.
fn group_warning(g: &am_core::review_groups::ReviewGroup, cands: &crate::review_groups_ui::Candidates) -> Option<String> {
    ["authors", "teams", "repos", "excludeRepos"].into_iter().find_map(|key| {
        let unknown = crate::review_groups_ui::unknown(g, key, cands);
        (!unknown.is_empty()).then(|| unknown_text(key, &unknown))
    })
}

/// Label column of a review group's form.
const FORM_LABEL_W: usize = 22;

/// A review group's form as screen lines `(field, first line of it, text)`:
/// each value wrapped (at spaces, so lists break between entries) to what
/// fits beside the labels in `width`; the field being edited shows its
/// buffer and cursor.
fn group_form_lines(
    group: &am_core::review_groups::ReviewGroup,
    form: &crate::review_groups_ui::GroupForm,
    width: usize,
) -> Vec<(usize, bool, String)> {
    use crate::review_groups_ui::{FORM, display};
    let avail = width.saturating_sub(FORM_LABEL_W + 4).max(8);
    let mut out = Vec::new();
    for (i, field) in FORM.iter().enumerate() {
        let value = match (&form.editing, i == form.cursor) {
            (Some(buf), true) => format!("{buf}▏"),
            _ => display(group, field),
        };
        for (k, part) in wrap_words(&value, avail).into_iter().enumerate() {
            out.push((i, k == 0, part));
        }
    }
    out
}

/// The autocomplete line under a group's list being typed:
/// ` ⇥ alice (Alice Liddell) · bob`, the highlighted one selected; each is
/// clickable.
fn suggestions_line<'a>(
    form: &crate::review_groups_ui::GroupForm,
    cands: &crate::review_groups_ui::Candidates,
    theme: &Theme,
    help: Rect,
    hits: &mut Vec<Hit>,
) -> Line<'a> {
    let found = form.completions(cands);
    if found.is_empty() {
        let msg = match form.field().kind {
            crate::review_groups_ui::FormKind::List(crate::review_groups_ui::Source::People) if cands.people.is_empty() => {
                " no one to suggest yet (looked up on GitHub) · ⏎ saves as typed"
            }
            _ => " no match · ⏎ saves as typed",
        };
        return Line::styled(msg, theme.faint());
    }
    let mut spans = vec![Span::styled(" ⇥ ", theme.faint())];
    let mut x = help.x + 3;
    let max_x = help.x + help.width;
    for (i, p) in found.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", theme.faint()));
            x += 3;
        }
        let text = if p.name.is_empty() { p.login.clone() } else { format!("{} ({})", p.login, p.name) };
        let w = text.width() as u16;
        if x + w > max_x {
            break;
        }
        let selected = i == form.suggest;
        hits.push(Hit { rect: Rect::new(x, help.y + 1, w, 1), target: Target::Item(List::Authors, i), selected });
        spans.push(Span::styled(text, if selected { theme.selected() } else { theme.fg(theme.text) }));
        x += w;
    }
    Line::from(spans)
}

/// Settings → Harness plugins.
fn draw_plugins(f: &mut Frame, theme: &Theme, list: Rect, help: Rect, pl: &crate::plugins_ui::PluginEditor, hits: &mut Vec<Hit>) {
    let header = " Harness plugins — space on/off · i install hooks · d default · esc back";
    hint_hits(hits, header, list.x, list.y);
    let mut lines = vec![Line::styled(header, theme.faint()), Line::raw("")];
    // Wide enough for the longest name ("GitHub Copilot CLI") plus a gap.
    let name_w = pl.rows.iter().map(|r| r.name.chars().count() + 1).max().unwrap_or(0).clamp(14, 24);
    for (i, r) in pl.rows.iter().enumerate() {
        let sel = i == pl.cursor;
        let hooks = match (r.enabled, r.hooks, r.present) {
            (_, true, _) => Span::styled("hooks ✓      ", theme.fg(theme.green)),
            (true, false, true) => Span::styled("hooks ✗      ", theme.fg(theme.yellow)),
            (true, false, false) => Span::styled("not set up   ", theme.faint()),
            (false, false, _) => Span::styled("—            ", theme.faint()),
        };
        let default = if r.id == pl.default_agent { "★ default" } else { "" };
        let line = Line::from(vec![
            Span::styled(if sel { "▌" } else { " " }, theme.bold(theme.border_focus)),
            Span::styled(if r.enabled { " [x] " } else { " [ ] " }, theme.fg(if r.enabled { theme.green } else { theme.faint })),
            Span::styled(format!("{:<name_w$}", truncate(&r.name, name_w - 1)), if r.enabled { theme.fg(theme.text) } else { theme.faint() }),
            Span::styled(format!("{:<12}", truncate(&r.source, 12)), theme.faint()),
            hooks,
            Span::styled(format!("{default:<10}"), theme.fg(theme.accent)),
        ]);
        push_row(hits, list, lines.len(), Target::Item(List::Plugins, i), sel);
        lines.push(if sel { line.style(theme.selected()) } else { line });
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
        format!(" Add plugins as JSON manifests in {}", pl.plugins_dir),
        theme.faint(),
    ));
    f.render_widget(Paragraph::new(lines), list);
    let help_line = match (&pl.message, pl.selected()) {
        (Some((ok, m)), _) => Line::styled(format!(" {m}"), theme.fg(if *ok { theme.green } else { theme.red })),
        (None, Some(r)) => {
            let note = r.note.as_deref().map(|n| format!(" · {n}")).unwrap_or_default();
            Line::styled(format!(" {} · hooks in {}{note}", r.id, r.settings_path), theme.faint())
        }
        (None, None) => Line::raw(""),
    };
    f.render_widget(Paragraph::new(vec![Line::raw(""), help_line]), help);
}

/// Settings → Repositories.
fn draw_repos(f: &mut Frame, theme: &Theme, list: Rect, help: Rect, ed: &crate::repos_ui::RepoEditor, hits: &mut Vec<Hit>) {
    if let (Some(at), Some(r)) = (ed.scripts, ed.selected()) {
        return draw_repo_scripts(f, theme, list, help, r, at, ed.message.as_ref(), hits);
    }
    let header = " Repositories — a add · x remove · s scripts · esc back";
    hint_hits(hits, header, list.x, list.y);
    let mut lines = vec![Line::styled(header, theme.faint()), Line::raw("")];
    if ed.rows.is_empty() {
        lines.push(Line::styled("   No repositories yet — press a to add one", theme.faint()));
    }
    let name_w = ed.rows.iter().map(|r| r.name.width() + 2).max().unwrap_or(0).clamp(12, 24);
    // name, path, "12 worktrees", "script review"
    let path_w = (list.width as usize).saturating_sub(1 + 2 + name_w + 14 + 14).max(8);
    let height = (list.height as usize).saturating_sub(lines.len());
    let start = scroll_start(&ed.scroll, ed.rows.len(), height, (ed.cursor, ed.cursor), |_| true);
    for (i, r) in ed.rows.iter().enumerate().skip(start).take(height) {
        let sel = i == ed.cursor;
        let count = match r.worktrees {
            1 => "1 worktree".to_string(),
            n => format!("{n} worktrees"),
        };
        let line = Line::from(vec![
            Span::styled(if sel { "▌" } else { " " }, theme.bold(theme.border_focus)),
            Span::styled(format!("  {:<name_w$}", truncate(&r.name, name_w - 1)), theme.bold(theme.text)),
            if r.exists {
                Span::styled(format!("{:<path_w$}", truncate(&r.path, path_w - 1)), theme.faint())
            } else {
                Span::styled(format!("{:<path_w$}", truncate(&format!("{} (missing)", r.path), path_w - 1)), theme.fg(theme.red))
            },
            Span::styled(format!("{count:<14}"), theme.faint()),
            Span::styled(if r.script { "script " } else { "       " }, theme.fg(theme.green)),
            if r.review_script { Span::styled("review", theme.fg(theme.teal)) } else { Span::raw("") },
        ]);
        push_row(hits, list, lines.len(), Target::Item(List::Repos, i), sel);
        lines.push(if sel { line.style(theme.selected()) } else { line });
    }
    f.render_widget(Paragraph::new(lines), list);
    let confirm = ed.confirm_remove.as_ref().and_then(|id| ed.rows.iter().find(|r| &r.id == id));
    let help_line = match (confirm, &ed.message, ed.selected()) {
        (Some(r), _, _) => {
            let text = format!(" Stop tracking {}? Files stay on disk · y confirm, any other key cancels", r.name);
            // `y confirm` and `any other key cancels` sit after the last ` · `.
            if let Some(at) = text.rfind(" · ") {
                let x = help.x + text[..at].width() as u16;
                hint_hits(hits, &format!(" · {}", text[at + " · ".len()..].replace(", ", "  ")), x, help.y + 1);
            }
            Line::styled(text, theme.bold(theme.yellow))
        }
        (None, Some((ok, m)), _) => Line::styled(format!(" {m}"), theme.fg(if *ok { theme.green } else { theme.red })),
        (None, None, Some(_)) => Line::styled(" s or ⏎ picks one of its scripts (startup or review) to add, edit or remove", theme.faint()),
        (None, None, None) => Line::raw(""),
    };
    f.render_widget(Paragraph::new(vec![Line::raw(""), help_line]), help);
}

/// What each script is for and where it runs, under its row in the chooser.
fn script_about(kind: am_core::worktrees::ScriptKind) -> &'static str {
    match kind {
        am_core::worktrees::ScriptKind::Startup => {
            "Runs after you create a worktree from the dashboard (am asks first), in the new worktree; output in the jobs pane (a new terminal window with startupScriptMode: window). For installing dependencies, copying .env files…"
        }
        am_core::worktrees::ScriptKind::Review => {
            "Runs after pull + run script (T or ctrl-⏎ on the review page) pulls a PR into Reviewing, in that PR's worktree, headless in the jobs pane, with the PR in AM_PR_* variables. For an automatic review, e.g. claude -p \"review this PR\"."
        }
    }
}

/// Settings → Repositories… → `s` on a repo: pick the script to edit or remove.
#[allow(clippy::too_many_arguments)]
fn draw_repo_scripts(
    f: &mut Frame,
    theme: &Theme,
    list: Rect,
    help: Rect,
    r: &crate::repos_ui::RepoRow,
    at: usize,
    message: Option<&(bool, String)>,
    hits: &mut Vec<Hit>,
) {
    let header = format!(" Scripts for {} — ⏎ add/edit · x remove · esc back", r.name);
    hint_hits(hits, &header, list.x, list.y);
    let mut lines = vec![Line::styled(header, theme.faint()), Line::raw("")];
    let text_w = (list.width as usize).saturating_sub(6).max(20);
    for (i, kind) in crate::repos_ui::SCRIPTS.into_iter().enumerate() {
        let sel = i == at;
        let label = kind.label();
        let name = format!("{}{}", label[..1].to_uppercase(), &label[1..]);
        let (state, colour) = if r.has(kind) { ("✓ added", theme.green) } else { ("— not added yet", theme.text) };
        let line = Line::from(vec![
            Span::styled(if sel { "▌" } else { " " }, theme.bold(theme.border_focus)),
            Span::styled(format!("  {name:<16}"), theme.bold(theme.text)),
            Span::styled(state, if r.has(kind) { theme.fg(colour) } else { theme.faint() }),
        ]);
        push_row(hits, list, lines.len(), Target::Item(List::RepoScripts, i), sel);
        lines.push(if sel { line.style(theme.selected()) } else { line });
        for l in wrap_words(script_about(kind), text_w) {
            push_row(hits, list, lines.len(), Target::Item(List::RepoScripts, i), false);
            lines.push(Line::styled(format!("     {l}"), theme.faint()));
        }
        // Short, under the app dir (`~/.agent-monitor` unless AM_DATA_DIR).
        let file = kind.path(&r.id).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        lines.push(Line::styled(format!("     scripts/{file}"), theme.faint()));
        lines.push(Line::raw(""));
    }
    f.render_widget(Paragraph::new(lines), list);
    let help_line = match message {
        Some((ok, m)) => Line::styled(format!(" {m}"), theme.fg(if *ok { theme.green } else { theme.red })),
        None if r.has(crate::repos_ui::SCRIPTS[at]) => Line::styled(" ⏎ opens it in your editor · x removes it", theme.faint()),
        None => Line::styled(" ⏎ creates it from a template and opens it in your editor", theme.faint()),
    };
    f.render_widget(Paragraph::new(vec![Line::raw(""), help_line]), help);
}

/// The add-repository folder browser (a modal with a scrolling list).
fn draw_browser(f: &mut Frame, theme: &Theme, area: Rect, b: &crate::folder_browser::FolderBrowser, now: i64, hits: &mut Vec<Hit>) {
    let w = 72.min(area.width.saturating_sub(2));
    let h = 24.min(area.height.saturating_sub(2));
    let rect = modal_rect(area, w, h);
    let hint = if b.path_mode() { "tab complete  ⏎ add/open  esc cancel" } else { "⏎ add/open  → open  ← up  type to filter  esc cancel" };
    let dir = am_core::hooks_installer::tilde(&b.dir);
    clear(f, theme, rect);
    let block = modal_block(theme, "Add repository".into(), theme.border_focus, hint);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    bottom_hint_hits(hits, &format!(" {hint} "), rect);
    let [head, list, foot] = Layout::vertical([Constraint::Length(3), Constraint::Min(1), Constraint::Length(2)]).areas(inner);
    let iw = inner.width as usize;
    let filter = if b.filter.is_empty() {
        Line::from(vec![Span::styled("› ", theme.bold(theme.border_focus)), Span::styled("▏type to filter, or a path (/… ~/…)", theme.faint())])
    } else {
        Line::from(vec![Span::styled("› ", theme.bold(theme.border_focus)), Span::raw(format!("{}▏", b.filter))])
    };
    f.render_widget(
        Paragraph::new(vec![Line::styled(truncate(&dir, iw), theme.bold(theme.text)), filter, Line::raw("")]),
        head,
    );
    let rows = b.visible();
    let height = list.height as usize;
    let start = scroll_start(&b.scroll, rows.len(), height, (b.cursor, b.cursor), |_| true);
    let mut lines: Vec<Line> = Vec::new();
    if rows.is_empty() && !b.path_mode() {
        lines.push(Line::styled(if b.filter.is_empty() { "  (no folders here)" } else { "  (no match)" }, theme.faint()));
    }
    for (i, e) in rows.iter().enumerate().skip(start).take(height) {
        let sel = i == b.cursor;
        let (tag, style, tag_style) = if e.tracked {
            ("✓ tracked", theme.faint(), theme.faint())
        } else if e.is_repo {
            ("git", theme.bold(theme.accent), theme.fg(theme.green))
        } else {
            ("", theme.fg(theme.text), theme.faint())
        };
        let name = if e.name == ".." { "../".to_string() } else { format!("{}/", e.name) };
        let line = Line::from(vec![
            Span::styled(if sel { "▸ " } else { "  " }, theme.bold(theme.border_focus)),
            Span::styled(format!("{:<w$}", truncate(&name, iw.saturating_sub(14)), w = iw.saturating_sub(14)), style),
            Span::styled(tag, tag_style),
        ]);
        push_row(hits, list, lines.len(), Target::Item(List::Browser, i), sel);
        lines.push(if sel { line.style(theme.selected()) } else { line });
    }
    f.render_widget(Paragraph::new(lines), list);
    let status = match (&b.adding, &b.error) {
        (Some(p), _) => Line::from(vec![
            Span::styled(format!("{} ", spinner(now)), theme.fg(theme.accent)),
            Span::raw(format!("Adding {}…", am_core::hooks_installer::tilde(std::path::Path::new(p)))),
        ]),
        (None, Some(e)) => Line::styled(truncate(e, iw), theme.fg(theme.red)),
        (None, None) => Line::raw(""),
    };
    f.render_widget(Paragraph::new(vec![Line::raw(""), status]), foot);
}

/// Settings → Key bindings: the commands by section, each with its key,
/// its default when changed and what it does on the review page.
fn draw_key_bindings(f: &mut Frame, theme: &Theme, list: Rect, help: Rect, ed: &crate::keys_ui::KeyEditor, hits: &mut Vec<Hit>) {
    use crate::keymap::{self, Command};
    // `▌` + label, key, `(default ctrl-v)`, then the review-page column.
    const REVIEW_X: usize = 1 + 2 + 24 + 8 + 17;
    // Review-only mode lists only the page's keys, so the column says what
    // each does there.
    let review_col = list.width as usize >= REVIEW_X + 20;
    let column_title = if ed.reviews_mode { "What it does" } else { "On the review page" };
    let header = " Key bindings — ⏎ change · x reset · R reset all · esc back";
    hint_hits(hits, header, list.x, list.y);
    let column = if review_col { format!("{:REVIEW_X$}{column_title}", "") } else { String::new() };
    f.render_widget(
        Paragraph::new(vec![Line::styled(header, theme.faint()), Line::styled(column, theme.faint())]),
        Rect { height: 2.min(list.height), ..list },
    );
    let footer = if ed.reviews_mode {
        " Always on: ↑↓ pgup pgdn home end · ⇥ jobs · esc back · ctrl-c quit"
    } else {
        " Always on: ↑↓ pgup pgdn home end · ⇥ list/reviews/jobs · esc back · ctrl-c quit"
    };
    // (command index, line); section headers take no index.
    let mut rows: Vec<(Option<usize>, Line)> = Vec::new();
    let mut section = "";
    for i in ed.listed() {
        let cmd = Command::ALL[i];
        if cmd.section() != section {
            section = cmd.section();
            rows.push((None, Line::styled(format!(" {section}"), theme.bold(theme.accent))));
        }
        let sel = i == ed.cursor;
        let key = ed.keymap.key(cmd);
        let mut spans = vec![
            Span::styled(if sel { "▌" } else { " " }, theme.bold(theme.border_focus)),
            Span::styled(format!("  {:<24}", cmd.label()), theme.fg(theme.text)),
        ];
        if sel && ed.capturing {
            spans.push(Span::styled("press a key…", theme.bold(theme.accent)));
        } else {
            let shown = ed.keymap.shown(cmd);
            spans.push(Span::styled(format!("{shown:<8}"), if key.is_some() { theme.bold(theme.accent) } else { theme.faint() }));
            let default = if key != cmd.default_key() {
                format!("(default {})", cmd.default_key().map(keymap::display).unwrap_or_else(|| "none".into()))
            } else {
                String::new()
            };
            spans.push(Span::styled(format!("{default:<17}"), theme.faint()));
            if review_col {
                let what = if ed.reviews_mode { cmd.in_reviews_mode() } else { cmd.on_review_page() };
                let (text, style) = match what {
                    Some(t) => (t, theme.fg(theme.text)),
                    None => ("—", theme.faint()),
                };
                spans.push(Span::styled(truncate(text, (list.width as usize).saturating_sub(REVIEW_X)), style));
            }
        }
        let line = Line::from(spans);
        rows.push((Some(i), if sel { line.style(theme.selected()) } else { line }));
    }
    // The footer sits under the list, or at the bottom once the list scrolls.
    let body = Rect { y: list.y + 2, height: list.height.saturating_sub(5).min(rows.len() as u16), ..list };
    let foot = Rect { y: body.y + body.height, height: list.height.saturating_sub(2).min(3), ..list };
    f.render_widget(Paragraph::new(vec![Line::raw(""), Line::styled(truncate(footer, list.width as usize), theme.faint())]), foot);
    // Keep the selection in view, with its section header when it opens one.
    let height = body.height as usize;
    let at = rows.iter().position(|(i, _)| *i == Some(ed.cursor)).unwrap_or(0);
    let start = scroll_start(&ed.scroll, rows.len(), height, (at, at), |i| rows[i].0.is_some());
    let mut lines = Vec::new();
    for (n, (i, line)) in rows.into_iter().enumerate().skip(start).take(height) {
        if let Some(i) = i {
            push_row(hits, body, n - start, Target::Item(List::Keys, i), i == ed.cursor);
        }
        lines.push(line);
    }
    f.render_widget(Paragraph::new(lines), body);
    let cmd = ed.selected();
    let first_line = match &ed.message {
        Some((ok, m)) => Line::styled(format!(" {m}"), theme.fg(if *ok { theme.green } else { theme.red })),
        None => Line::styled(format!(" {}", cmd.help()), theme.faint()),
    };
    let review_line = match cmd.on_review_page() {
        Some("same") => " Does the same on the review page".to_string(),
        Some(t) => format!(" On the review page: {t}"),
        None => " Does nothing on the review page".to_string(),
    };
    f.render_widget(Paragraph::new(vec![first_line, Line::styled(review_line, theme.faint())]), help);
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

fn draw_theme_builder(f: &mut Frame, theme: &Theme, area: Rect, b: &crate::theme_builder::ThemeBuilder, hits: &mut Vec<Hit>) {
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
    bottom_hint_hits(hits, hint, rect);
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
        let highlighted = sel && matches!(b.mode, Mode::Tokens | Mode::Hex(_));
        // Typing a hex value or a name: clicks would only interrupt it.
        if matches!(b.mode, Mode::Tokens | Mode::Picker { .. }) {
            push_row(hits, Rect { width: 44.min(left.width), ..left }, i, Target::Item(List::ThemeTokens, i), highlighted);
        }
        lines.push(if highlighted { line.style(theme.selected()) } else { line });
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
            let y = right.y + lines.len() as u16;
            for (col, c) in chunk.iter().enumerate() {
                let here = row * COLS + col == cursor;
                // Each swatch is `[██]`: four cells.
                let x = right.x + 1 + 4 * col as u16;
                if y < right.y + right.height && x + 4 <= right.x + right.width {
                    let target = Target::Item(List::Palette, row * COLS + col);
                    hits.push(Hit { rect: Rect::new(x, y, 4, 1), target, selected: false });
                }
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
        row(Status::Executing, "feature/login", vec![Span::styled("ENG-123 ", theme.fg(theme.blue)), Span::styled("#42✓ approved", theme.fg(theme.green))]),
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
    // Compact: up to the screen's edges.
    let w = w.min(area.width.saturating_sub(2 * edge()));
    let h = h.min(area.height.saturating_sub(2 * edge()));
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

fn draw_modal(f: &mut Frame, theme: &Theme, area: Rect, modal: &crate::modals::Modal, now: i64, hits: &mut Vec<Hit>) {
    use crate::modals::{DeleteStep, FormField, Modal};
    if let Modal::AddRepo(b) = modal {
        draw_browser(f, theme, area, b, now, hits);
        return;
    }
    // Line index → what a click on it does (lines may wrap, so rows are
    // worked out once the box width is known).
    let mut marks: Vec<(usize, Target, bool)> = Vec::new();
    let (title, color, hint, lines): (String, _, &str, Vec<Line>) = match modal {
        Modal::New(form) => {
            let field_index = |which: FormField| match which {
                FormField::Repo => 0,
                FormField::Branch => 1,
                FormField::Name => 2,
                FormField::Base => 3,
            };
            let mut lines = Vec::new();
            let mut field = |lines: &mut Vec<Line<'static>>, label: &str, value: String, which: FormField, note: Option<Line<'static>>| {
                marks.push((lines.len(), Target::Item(List::NewForm, field_index(which)), false));
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
                lines.extend(v);
            };
            if form.repos.len() > 1 {
                field(&mut lines, "Repo", format!("‹ {} ›", form.repo().name), FormField::Repo, None);
            }
            // Which of these Enter takes without asking: NewForm::request.
            let note = if form.origin_only() {
                Some(Line::styled("           ✓ exists on origin — Enter will pull it", theme.fg(theme.green)))
            } else if form.remote_exists() {
                Some(Line::styled("           ✓ on origin and locally — Enter will ask first", theme.fg(theme.green)))
            } else if form.local_only() {
                Some(Line::styled("           ! exists locally only — Enter will offer to reuse it", theme.fg(theme.yellow)))
            } else if form.pending_check.is_some() {
                Some(Line::styled("           checking…", theme.faint()))
            } else {
                None
            };
            field(&mut lines, "Branch", form.branch.clone(), FormField::Branch, note);
            field(&mut lines, "Name", if form.name.is_empty() && form.field != FormField::Name { "(optional)".into() } else { form.name.clone() }, FormField::Name, None);
            if form.remote_exists() {
                lines.push(Line::styled(format!(" Base      tracks origin/{}", form.branch), theme.faint()));
            } else {
                field(&mut lines, "Base", form.base.clone(), FormField::Base, None);
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
                    marks.push((lines.len(), Target::Key(Key::Enter), false));
                    lines.push(key_line(theme, "⏎ / y", "Yes", true));
                    marks.push((lines.len(), Target::Key(Key::Esc), false));
                    lines.push(key_line(theme, "n / esc", "Cancel", false));
                }
                DeleteStep::LocalBranch => {
                    lines.push(Line::raw(format!("Also delete local branch {}?", w.branch)));
                    lines.push(Line::raw(""));
                    marks.push((lines.len(), Target::Key(Key::Enter), false));
                    lines.push(key_line(theme, "⏎ / y", "Yes", true));
                    marks.push((lines.len(), Target::Key(Key::Char('n')), false));
                    lines.push(key_line(theme, "n", "No, keep it", false));
                    marks.push((lines.len(), Target::Key(Key::Esc), false));
                    lines.push(key_line(theme, "esc", "Cancel", false));
                }
            }
            (title, if g.dirty > 0 || g.ahead > 0 { theme.yellow } else { theme.red }, "", lines)
        }
        Modal::ConfirmReset => {
            let lines = vec![
                Line::raw("Put every setting back to its default, then run setup again?"),
                Line::styled("Repositories, worktrees, history, themes, startup scripts and agent hooks are kept.", theme.faint()),
                Line::raw(""),
                key_line(theme, "y", "Reset", true),
                key_line(theme, "any key", "Cancel", false),
            ];
            marks.push((3, Target::Key(Key::Char('y')), false));
            marks.push((4, Target::Key(Key::Esc), false));
            ("Reset settings".into(), theme.red, "", lines)
        }
        Modal::Telemetry => {
            let lines = vec![
                Line::raw("Help improve am with anonymous usage reports?"),
                Line::styled("They hold am's version and OS, how it was installed, which features you use, how long things take, and where in am's code errors happen.", theme.faint()),
                Line::styled("Never your code, log text, paths, or repository, branch, PR or user names. am telemetry show lists what would be sent.", theme.faint()),
                Line::raw(""),
                key_line(theme, "⏎ / y", "Send anonymous reports", true),
                key_line(theme, "n / esc", "Don't send anything", false),
            ];
            marks.push((4, Target::Key(Key::Char('y')), false));
            marks.push((5, Target::Key(Key::Char('n')), false));
            ("Usage reports".into(), theme.border_focus, "", lines)
        }
        Modal::AddReviewScript { repo, .. } => {
            let lines = vec![
                Line::raw(format!("{repo} has no review script yet. Add one?")),
                Line::styled("It opens in your editor from a template while the PR is pulled, and runs in the new worktree once you save it.", theme.faint()),
                Line::raw(""),
                key_line(theme, "⏎ / y", "Add the script, pull and run it", true),
                key_line(theme, "any key", "Cancel", false),
            ];
            marks.push((3, Target::Key(Key::Char('y')), false));
            marks.push((4, Target::Key(Key::Esc), false));
            ("Add a review script".into(), theme.yellow, "", lines)
        }
        Modal::ConfirmUpdate(u) => {
            let command = u.method.upgrade_steps().map(|s| am_core::update_check::shell_text(&s)).unwrap_or_default();
            let lines = vec![
                Line::raw(format!("Update am v{} → v{}?", u.current, u.latest)),
                Line::styled(format!("Runs: {command}"), theme.faint()),
                Line::styled("Its output shows in the jobs pane; am offers to restart once it's done.", theme.faint()),
                Line::raw(""),
                key_line(theme, "⏎ / y", "Update now", true),
                key_line(theme, "any key", "Cancel", false),
            ];
            marks.push((4, Target::Key(Key::Char('y')), false));
            marks.push((5, Target::Key(Key::Esc), false));
            ("Update am".into(), theme.green, "", lines)
        }
        Modal::Restart { latest } => {
            let lines = vec![
                Line::raw(format!("am v{latest} is installed. Restart now to use it?")),
                Line::styled("Agent hooks pick it up on their own; only this dashboard runs the old version.", theme.faint()),
                Line::raw(""),
                key_line(theme, "⏎ / y", "Restart am", true),
                key_line(theme, "any key", "Later", false),
            ];
            marks.push((3, Target::Key(Key::Char('y')), false));
            marks.push((4, Target::Key(Key::Esc), false));
            ("Updated".into(), theme.green, "", lines)
        }
        Modal::AddRepo(_) => unreachable!("drawn by draw_browser"),
    };
    // Long names wrap rather than run off the edge; size the box to fit.
    let w = 72.min(area.width.saturating_sub(2));
    let para_lines = lines.clone();
    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    let h = para.line_count(w.saturating_sub(4)) as u16 + 2;
    let rect = modal_rect(area, w, h);
    clear(f, theme, rect);
    if !hint.is_empty() {
        bottom_hint_hits(hits, &format!(" {hint} "), rect);
    }
    // Inside the border and one column of padding each side.
    let inner = Rect { x: rect.x + 2, y: rect.y + 1, width: rect.width.saturating_sub(4), height: rect.height.saturating_sub(2) };
    for (line, target, selected) in marks {
        let row = Paragraph::new(para_lines[..line].to_vec()).wrap(Wrap { trim: false }).line_count(inner.width);
        let tall = Paragraph::new(vec![para_lines[line].clone()]).wrap(Wrap { trim: false }).line_count(inner.width);
        for r in row..row + tall {
            push_row(hits, inner, r, target, selected);
        }
    }
    f.render_widget(para.block(modal_block(theme, title, color, hint)), rect);
}

fn spinner(now: i64) -> &'static str {
    const SPIN: [&str; 4] = ["◐", "◓", "◑", "◒"];
    SPIN[now.rem_euclid(4) as usize]
}

#[allow(clippy::needless_late_init)] // each arm builds lines then picks its hint
fn draw_setup(f: &mut Frame, theme: &Theme, area: Rect, s: &crate::setup::Setup, hits: &mut Vec<Hit>) {
    use crate::setup::{CONTEXT_CHOICES, CONTEXT_CUSTOM, IDE_CUSTOM, IDES, Step};
    let name = |step: Step| match step {
        Step::Mode => "Mode",
        Step::Editor => "Editor",
        Step::Hooks => "Hooks",
        Step::Repos => "Repos",
        // Review-only mode: that step is only GitHub (and Linear).
        Step::Integrations if s.reviews_mode => "GitHub",
        Step::Integrations => "Integrations",
        Step::Reviews => "Reviews",
        Step::Context => "Context",
        Step::Privacy => "Privacy",
    };
    let steps = s.steps();
    let mut crumbs = vec![Span::raw(" ")];
    let cur = steps.iter().position(|st| *st == s.step).unwrap_or(0);
    for (i, name) in steps.iter().map(|st| name(*st)).enumerate() {
        if i > 0 {
            crumbs.push(Span::styled(" › ", theme.faint()));
        }
        let style = if i == cur { theme.bold(theme.border_focus) } else if i < cur { theme.fg(theme.green) } else { theme.faint() };
        crumbs.push(Span::styled(format!("{} {name}", if i < cur { "✓" } else { "" }).trim().to_string(), style));
    }
    let mut lines = vec![Line::from(crumbs), Line::raw("")];
    // Line index → what a click on it does.
    let mut marks: Vec<(usize, Target, bool)> = Vec::new();
    let hint;
    match s.step {
        Step::Mode => {
            lines.push(Line::raw("What do you want am for?"));
            lines.push(Line::raw(""));
            let choices = [
                ("Agents & worktrees", "the full dashboard: agent status, worktrees, PRs, tickets and reviews"),
                ("Review requests only", "just the PRs waiting on your review, to pull and open"),
            ];
            for (i, (label, what)) in choices.into_iter().enumerate() {
                let sel = (i == 1) == s.reviews_mode;
                marks.push((lines.len(), Target::Item(List::SetupMode, i), false));
                lines.push(Line::from(vec![
                    Span::styled(if sel { "  ● " } else { "  ○ " }, theme.fg(if sel { theme.border_focus } else { theme.faint })),
                    Span::styled(label, if sel { theme.bold(theme.text) } else { theme.fg(theme.text) }),
                ]));
                lines.push(Line::styled(format!("    {what}"), theme.faint()));
            }
            lines.push(Line::raw(""));
            lines.push(Line::styled("Settings → Mode switches later", theme.faint()));
            hint = if s.cancellable { "↑↓ choose  ⏎ next  esc cancel" } else { "↑↓ choose  ⏎ next" };
        }
        Step::Editor => {
            lines.push(Line::raw(if s.reviews_mode {
                "Where should Enter open a pulled PR?"
            } else {
                "Where should Enter open a worktree?"
            }));
            lines.push(Line::raw(""));
            for (i, (_, label)) in IDES.iter().enumerate() {
                let sel = i == s.ide;
                marks.push((lines.len(), Target::Item(List::SetupIde, i), false));
                let mut row = Line::from(vec![
                    Span::styled(if sel { "  ● " } else { "  ○ " }, theme.fg(if sel { theme.border_focus } else { theme.faint })),
                    Span::styled(*label, if sel { theme.bold(theme.text) } else { theme.fg(theme.text) }),
                ]);
                if i == IDE_CUSTOM {
                    let text = if s.ide_command.is_empty() && !sel {
                        "e.g. idea, subl -n".to_string()
                    } else {
                        format!("{}{}", s.ide_command, if sel { "▏" } else { "" })
                    };
                    row.spans.push(Span::styled(format!(": {text}"), theme.faint()));
                }
                lines.push(row);
            }
            if s.ide == IDE_CUSTOM {
                lines.push(Line::styled("    Runs with the worktree path appended, or in place of {path}", theme.faint()));
            }
            if let Some(e) = &s.ide_error {
                lines.push(Line::styled(e.clone(), theme.fg(theme.red)));
            }
            hint = "↑↓ choose  ⏎ next  esc back";
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
            lines.push(Line::raw(if s.reviews_mode {
                "Add the git repositories whose pull requests you review."
            } else {
                "Add the git repositories whose worktrees you want to watch."
            }));
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
            let f0 = s.integration_field == 0;
            if s.reviews_mode {
                lines.push(Line::raw("Review requests come from GitHub, through the gh CLI."));
                lines.push(Line::raw("Optional: a Linear key shows each PR's ticket."));
                lines.push(Line::raw(""));
            } else {
                lines.push(Line::raw("Optional: show PRs/CI and Linear tickets next to each branch."));
                lines.push(Line::raw(""));
            }
            marks.push((lines.len(), Target::Item(List::SetupIntegrations, 0), false));
            marks.push((lines.len() + 1, Target::Item(List::SetupIntegrations, 1), false));
            let cursor = Span::styled(if f0 { "▌" } else { " " }, theme.bold(theme.border_focus));
            lines.push(match (s.reviews_mode, s.gh_found) {
                (false, _) => Line::from(vec![cursor, Span::raw(format!(" [{}] GitHub PR & CI status (via gh)", if s.gh { "x" } else { " " }))]),
                (true, true) => Line::from(vec![cursor, Span::styled(" ✓ gh found (it must be logged in: gh auth status)", theme.fg(theme.green))]),
                (true, false) => Line::from(vec![
                    cursor,
                    Span::styled(" ✗ gh not found — install it, then run gh auth login", theme.fg(theme.red)),
                ]),
            });
            let masked = "•".repeat(s.linear_key.chars().count());
            lines.push(Line::from(vec![
                Span::styled(if f0 { " " } else { "▌" }, theme.bold(theme.border_focus)),
                Span::raw(" Linear API key: "),
                Span::styled(if masked.is_empty() && f0 { "(optional)".to_string() } else { format!("{masked}{}", if f0 { "" } else { "▏" }) }, theme.faint()),
            ]));
            hint = if s.reviews_mode { "tab field  ⏎ next  esc back" } else { "tab field  space toggle  ⏎ next  esc back" };
        }
        Step::Reviews => {
            let r = &s.review;
            if s.reviews_mode {
                lines.push(Line::raw("Which pull requests are waiting on your review?"));
            } else {
                lines.push(Line::raw("Which pull requests are waiting on your review? (v opens them;"));
                lines.push(Line::raw("the dashboard border shows how many)"));
            }
            lines.push(Line::raw(""));
            let cursor = |i: usize| Span::styled(if r.field == i { "▌" } else { " " }, theme.bold(theme.border_focus));
            let check = |on: bool| if on { "[x]" } else { "[ ]" };
            // Review-only mode has no on/off row: they're what it's for.
            if !s.reviews_mode {
                marks.push((lines.len(), Target::Item(List::SetupReviews, 0), false));
                lines.push(Line::from(vec![cursor(0), Span::raw(format!(" {} Show PRs waiting for my review", check(r.enabled)))]));
            }
            let scopes = ["Me and my teams", "Only me, by name", "Only my teams"];
            for (k, label) in scopes.iter().enumerate() {
                let sel = r.scope == k;
                marks.push((lines.len(), Target::Item(List::SetupReviews, k + 1), false));
                lines.push(Line::from(vec![
                    cursor(k + 1),
                    Span::styled(if sel { "     ● " } else { "     ○ " }, theme.fg(if sel { theme.border_focus } else { theme.faint })),
                    Span::styled(*label, if sel { theme.bold(theme.text) } else { theme.fg(theme.text) }),
                ]));
            }
            marks.push((lines.len(), Target::Item(List::SetupReviews, 4), false));
            lines.push(Line::from(vec![cursor(4), Span::raw(format!(" {} Hide drafts", check(r.hide_drafts)))]));
            marks.push((lines.len(), Target::Item(List::SetupReviews, 5), false));
            lines.push(Line::from(vec![cursor(5), Span::raw(format!(" {} Hide PRs I've already reviewed", check(r.hide_reviewed)))]));
            lines.push(Line::raw(""));
            let groups = match r.groups() {
                Some(g) => format!("Groups: {}", g.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", ")),
                None if r.existing_groups.len() == 1 => "Keeps your review group".to_string(),
                None => format!("Keeps your {} review groups", r.existing_groups.len()),
            };
            lines.push(Line::styled(groups, theme.fg(theme.text)));
            lines.push(Line::styled(
                if s.reviews_mode { "More groups and filters: f → Groups…" } else { "More groups and filters: v, then f → Groups…" },
                theme.faint(),
            ));
            hint = "↑↓ move  space toggle  ⏎ next  esc back";
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
                marks.push((lines.len(), Target::Item(List::SetupContext, i), false));
                lines.push(radio(i == s.ctx_choice, label.to_string()));
            }
            marks.push((lines.len(), Target::Item(List::SetupContext, CONTEXT_CUSTOM), false));
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
            hint = "↑↓ choose  ⏎ next  esc back";
        }
        Step::Privacy => {
            lines.push(Line::raw("Help improve am with anonymous usage reports?"));
            lines.push(Line::raw(""));
            let choices: [(&str, &[&str]); 2] = [
                (
                    "Send anonymous reports",
                    &["am's version and OS, how it was installed, which features you use,", "how long things take, and where in am's code errors happen"],
                ),
                ("Don't send anything", &["nothing is recorded"]),
            ];
            for (i, (label, what)) in choices.into_iter().enumerate() {
                let sel = (i == 0) == s.telemetry;
                marks.push((lines.len(), Target::Item(List::SetupPrivacy, i), false));
                lines.push(Line::from(vec![
                    Span::styled(if sel { "  ● " } else { "  ○ " }, theme.fg(if sel { theme.border_focus } else { theme.faint })),
                    Span::styled(label, if sel { theme.bold(theme.text) } else { theme.fg(theme.text) }),
                ]));
                for line in what {
                    lines.push(Line::styled(format!("    {line}"), theme.faint()));
                }
            }
            lines.push(Line::raw(""));
            lines.push(Line::styled("Never your code, log text, paths, or repository, branch, PR or user names.", theme.faint()));
            lines.push(Line::styled("am telemetry show lists what would be sent; Settings → Usage reports changes it later.", theme.faint()));
            hint = "↑↓ choose  ⏎ finish  esc back";
        }
    }
    // 96 columns fits the eight breadcrumbs on one line.
    let rect = modal_rect(area, 96, lines.len() as u16 + 4);
    clear(f, theme, rect);
    bottom_hint_hits(hits, &format!(" {hint} "), rect);
    // Inside the border and the padding (1 left/right, 1 top).
    let inner = Rect { x: rect.x + 2, y: rect.y + 2, width: rect.width.saturating_sub(4), height: rect.height.saturating_sub(3) };
    for (line, target, selected) in marks {
        push_row(hits, inner, line, target, selected);
    }
    let block = modal_block(theme, "Welcome to agent-monitor".into(), theme.border_focus, hint).padding(ratatui::widgets::Padding::new(1, 1, 1, 0));
    f.render_widget(Paragraph::new(lines).block(block), rect);
}

fn draw_log(f: &mut Frame, theme: &Theme, area: Rect, lines: &[String]) {
    let block = Block::bordered()
        .borders(if compact() { Borders::TOP } else { Borders::ALL })
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
    let w = if compact() { area.width } else { 72.min(area.width.saturating_sub(4)) };
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
    // Compact: only the top line, which carries the footer too (`foot`).
    let borders = if compact() { Borders::TOP } else { Borders::ALL };
    Block::bordered().borders(borders).border_type(BorderType::Rounded).border_style(border).title(title)
}

fn title<'a>(name: &'a str, theme: &Theme) -> Line<'a> {
    Line::from(vec![
        Span::styled("─", theme.bold(theme.border_focus)),
        Span::styled(name, theme.bold(theme.border_focus)),
        Span::raw(" "),
    ])
}

/// The version in page titles; fixed under test so snapshots survive releases.
const VERSION: &str = if cfg!(test) { "0.0.0" } else { env!("CARGO_PKG_VERSION") };

/// A page title's text, `am v2.1.67 · Worktrees`, shortened to `am ·
/// Worktrees`, then `Worktrees`, when it (with its leading `─` and trailing
/// space) doesn't fit in `room` columns.
fn page_title_text(page: &str, room: usize) -> String {
    [format!("am v{VERSION} · {page}"), format!("am · {page}")]
        .into_iter()
        .find(|t| t.width() + 2 <= room)
        .unwrap_or_else(|| page.to_string())
}

/// A page title from [`page_title_text`]: the tool and version in the
/// border colour, the page name bold.
fn page_title<'a>(text: String, page: &str, theme: &Theme) -> Line<'a> {
    let tool = text.strip_suffix(page).unwrap_or("").to_string();
    Line::from(vec![
        Span::styled("─", theme.bold(theme.border_focus)),
        Span::styled(tool, theme.fg(theme.border_focus)),
        Span::styled(page.to_string(), theme.bold(theme.border_focus)),
        Span::raw(" "),
    ])
}

// ---------------------------------------------------------------- worktrees

/// The `─Worktrees` pane's title text: as long as the corner's update
/// notice and review badge leave room for (two corners and one border cell
/// between).
fn worktrees_title(app: &App, width: u16) -> String {
    let corner = update_label(app, width).map_or(0, |l| l.width()) + review_badge(app, width).map_or(0, |b| b.width());
    page_title_text("Worktrees", (width as usize).saturating_sub(2 + corner + 1))
}

/// The update to advertise, unless `checkForUpdates` was turned off.
fn shown_update(app: &App) -> Option<&am_core::update_check::Update> {
    app.update_available.as_ref().filter(|_| app.settings.check_for_updates)
}

/// Top-right "update available" label: shortened on narrow panes, dropped
/// when it would run into the `─Worktrees` title.
fn update_label(app: &App, width: u16) -> Option<String> {
    let latest = &shown_update(app)?.latest;
    let room = |badge: usize| (width as usize).saturating_sub(2 + "─Worktrees ".width() + badge + 1);
    // Two corners, the shortest title (`─Worktrees `, which gives way to
    // it), the review badge beside it, and one border cell between the titles.
    let badge = review_badge(app, width).map_or(0, |b| b.width());
    update_forms(app, latest).into_iter().find(|l| l.width() <= room(badge))
}

/// The update notice, longest first: ` ⬆ v2.2.0 available `, or
/// ` ⬆ updating to v2.2.0… ` while the upgrade job runs.
fn update_forms(app: &App, latest: &str) -> [String; 2] {
    if app.jobs.updating().is_some() {
        [format!(" ⬆ updating to v{latest}… "), format!(" ⬆ v{latest}… ")]
    } else {
        [format!(" ⬆ v{latest} available "), format!(" ⬆ v{latest} ")]
    }
}

/// Top-right ` 3 to review ` in the `─Worktrees` border's corner (`⚑3` on
/// narrow panes), or per review group shown on the dashboard
/// (` Platform 4 · 26 to review · 60 on GitHub `, ` ⚑4·26·60 `); zero
/// counts are left out, and there's no badge while all are 0 or the feature
/// is off.
fn review_badge(app: &App, width: u16) -> Option<String> {
    let (wide, narrow) = review_badge_forms(app)?;
    // The wide form needs room beside the pane title.
    Some(if width >= 60 && wide.width() + 14 <= width as usize { wide } else { narrow })
}

/// The review badge's wide and narrow (`⚑`) forms.
fn review_badge_forms(app: &App) -> Option<(String, String)> {
    use crate::app::BadgePart;
    let parts: Vec<BadgePart> = app
        .review_badge_parts()?
        .into_iter()
        .filter(|p| !matches!(p, BadgePart::Group(_, 0) | BadgePart::Rest(0) | BadgePart::Total(Some(0))))
        .collect();
    if !parts.iter().any(|p| !matches!(p, BadgePart::Total(None))) {
        return None;
    }
    let wide = parts.iter().map(|p| match p {
        BadgePart::Group(name, n) => format!("{name} {n}"),
        BadgePart::Rest(n) => format!("{n} to review"),
        BadgePart::Total(n) => format!("{} on GitHub", n.map_or("?".into(), |n| n.to_string())),
    });
    let wide = format!(" {} ", wide.collect::<Vec<_>>().join(" · "));
    let narrow = parts.iter().map(|p| match p {
        BadgePart::Group(_, n) | BadgePart::Rest(n) => n.to_string(),
        BadgePart::Total(n) => n.map_or("?".into(), |n| n.to_string()),
    });
    let narrow = format!(" ⚑{} ", narrow.collect::<Vec<_>>().join("·"));
    Some((wide, narrow))
}

/// Bottom-right ` today 4.1M ` (` today (visible) 4.1M ` for the visible
/// scope), shown per `dailyTokensMode`; shortened (scope first, then detail)
/// or dropped when it doesn't fit beside ` N of M `.
fn tokens_label(app: &App, width: u16) -> Option<String> {
    let tally = app.snapshot.today_tokens.filter(|_| app.settings.show_daily_tokens)?;
    let t = &tally.tokens;
    let n = context::short_tokens;
    // Longest first.
    let bodies: Vec<String> = match app.settings.daily_tokens_mode() {
        DailyTokensMode::Tokens => vec![n(t.total())],
        DailyTokensMode::Weighted => vec![format!("{} weighted", n(t.weighted())), n(t.weighted())],
        DailyTokensMode::Cost => vec![usd_label(tally.cost_usd, tally.unpriced > 0)],
        DailyTokensMode::Breakdown => vec![
            format!("in {} · write {} · read {} · out {}", n(t.input), n(t.cache_write()), n(t.cache_read), n(t.output)),
            n(t.total()),
        ],
    };
    let scopes: &[&str] = if app.settings.daily_tokens_visible_only() { &["today (visible)", "today"] } else { &["today"] };
    let pos = if app.flat.is_empty() { 0 } else { format!(" {} of {} ", app.selected + 1, app.flat.len()).width() };
    // Two corners, one border cell on the left and one between the titles.
    let room = (width as usize).saturating_sub(2 + 1 + pos + 1);
    bodies.iter().flat_map(|b| scopes.iter().map(move |s| format!(" {s} {b} "))).find(|l| l.width() <= room)
}

/// `~$12.34` (whole dollars from $100); a trailing `+` when some usage had
/// no known price.
fn usd_label(usd: f64, unpriced: bool) -> String {
    let amount = if usd < 100.0 { format!("{usd:.2}") } else { format!("{usd:.0}") };
    format!("~${amount}{}", if unpriced { "+" } else { "" })
}

struct Columns {
    ticket: usize,
    pr: usize,
    /// Review state as a word (`approved`, `changes`…) rather than a glyph.
    pr_words: bool,
    git: usize,
    age: usize,
    /// Context bar cells (0 hides the column); the token count follows it.
    ctx_bar: usize,
    ctx_window: u64,
}

fn draw_worktrees(f: &mut Frame, app: &App, theme: &Theme, area: Rect, now: i64, hits: &mut Vec<Hit>) {
    let focused = !app.jobs.focused && !app.review_focused();
    let heading = worktrees_title(app, area.width);
    // `─` + title + space.
    let title_w = heading.width() + 2;
    let mut block = pane_block(theme, page_title(heading, "Worktrees", theme), focused);
    // Footer before the corner: compact puts both on the top line, and the
    // last right-aligned title is the rightmost.
    let mut used = 0;
    if !app.flat.is_empty() {
        let pos = format!(" {} of {} ", app.selected + 1, app.flat.len());
        used += pos.width() + 1;
        block = foot(block, Line::styled(pos, theme.faint()).alignment(Alignment::Right));
    }
    let corner_w = update_label(app, area.width).map_or(0, |l| l.width() + 1) + review_badge(app, area.width).map_or(0, |b| b.width());
    if let Some(label) = tokens_label(app, area.width)
        .filter(|l| !compact() || title_w + used + l.width() + 1 + corner_w <= area.width as usize)
    {
        used += label.width() + 1;
        block = foot(block, Line::styled(label, theme.faint()).alignment(Alignment::Right));
    }
    // Bottom left: the filters in force; a click opens the filter box.
    if let Some(summary) = worktree_filter_summary(&app.settings, app.snapshot.hidden_worktrees) {
        let summary = format!(" {summary} ");
        // Compact: on the top line after the title, so it shares the row.
        let lead = if compact() { title_w + 1 } else { 0 };
        let taken = if compact() { used + corner_w } else { 0 };
        let room = (area.width as usize).saturating_sub(16 + lead + taken);
        if room > 10 {
            let summary = truncate(&summary, room);
            if let Some(k) = app.keymap.key(Command::Filter) {
                let rect = Rect::new(area.x + edge() + lead as u16, foot_y(area), summary.width() as u16, 1);
                hits.push(Hit { rect, target: Target::Key(k), selected: false });
            }
            let style = match () {
                _ if !app.settings.worktree_filters_enabled => theme.fg(theme.yellow),
                _ if picks_filters(&app.settings) => theme.fg(theme.accent),
                _ => theme.faint(),
            };
            block = foot(block, Line::styled(summary, style).alignment(Alignment::Left));
        }
    }
    // Top right: the update notice, then the review badge in the corner.
    let mut corner: Vec<Span> = Vec::new();
    if let Some(label) = update_label(app, area.width) {
        // Clickable: offers to update. It sits just left of the badge.
        let badge = review_badge(app, area.width).map_or(0, |b| b.width() as u16);
        let w = label.width() as u16;
        let rect = Rect::new((area.x + area.width).saturating_sub(edge() + badge + w), area.y, w, 1).intersection(area);
        hits.push(Hit { rect, target: Target::Update, selected: false });
        corner.push(Span::styled(label, theme.bold(theme.green)));
    }
    if let Some(badge) = review_badge(app, area.width) {
        // Clickable: opens the review page.
        let w = badge.width() as u16;
        let rect = Rect::new((area.x + area.width).saturating_sub(edge() + w), area.y, w, 1).intersection(area);
        hits.push(Hit { rect, target: Target::Command(Command::ReviewRequests), selected: false });
        corner.push(Span::styled(badge, theme.bold(theme.yellow).add_modifier(Modifier::REVERSED)));
    }
    if !corner.is_empty() {
        block = block.title(Line::from(corner).alignment(Alignment::Right));
    }
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.flat.is_empty() && !app.rows.iter().any(|r| matches!(r, Row::Pending(_))) {
        let msg = match (app.loaded, app.snapshot.hidden_worktrees) {
            (false, _) => "Loading…".to_string(),
            (true, 0) => "No worktrees yet — press A to add a repository".to_string(),
            // Only the base filters at work: F wouldn't bring anything back.
            (true, n) if !picks_filters(&app.settings) => {
                format!("{n} hidden by the base filters (Settings → Base filters)")
            }
            (true, n) => {
                let key = |c| app.keymap.key(c).map_or("?".into(), crate::keymap::display);
                format!("{n} filtered out — {} changes the filters, {} turns them off", key(Command::Filter), key(Command::ToggleFilters))
            }
        };
        f.render_widget(Paragraph::new(Line::styled(msg, theme.faint())).alignment(Alignment::Center), centered_line(inner));
        return;
    }

    let s = &app.settings;
    let pr_words = wide(inner.width);
    let cols = Columns {
        ticket: if s.show_linear_ticket { app.flat.iter().map(|v| ticket_text(v).width()).max().unwrap_or(0) } else { 0 },
        pr: if s.show_pr_status { app.flat.iter().map(|v| pr_text(v.pr.as_ref(), pr_words).width()).max().unwrap_or(0) } else { 0 },
        pr_words,
        // Compact: no git or last-commit columns.
        git: if s.show_git_ahead_behind && !compact() { app.flat.iter().map(|v| git_text(v).width()).max().unwrap_or(0) } else { 0 },
        age: if s.show_last_commit && !compact() { 3 } else { 0 },
        ctx_bar: if s.show_context_window
            && app.flat.iter().any(|v| v.context.is_some() || v.sessions.iter().any(|s| s.context.is_some()))
        {
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
            // The stack below is built on this branch, which has no worktree.
            Row::Base(b) => Line::styled(
                format!(" ┌ {} · no worktree", truncate(b, (inner.width as usize).saturating_sub(17))),
                theme.faint(),
            ),
            Row::Worktree(i) => {
                worktree_line(app, theme, &app.flat[*i], &app.tree[*i], *i == app.selected, focused, &cols, inner.width, now)
            }
            Row::Pending(id) => pending_line(app, theme, *id, inner.width, now),
            Row::Session(i, k) => session_line(app, theme, &app.flat[*i], &app.tree[*i], *k, &cols, inner.width, now),
        })
        .collect();
    // Worktree and session rows select their worktree; the focused
    // selection keeps its own highlight. A stack root's fold arrow and
    // `+N stacked hidden` pill fold/unfold it (pushed after, so they win).
    for (n, row) in app.rows.iter().skip(start).take(height).enumerate() {
        if let Row::Worktree(i) | Row::Session(i, _) = row {
            let selected = *row == Row::Worktree(app.selected) && focused;
            push_row(hits, inner, n, Target::Worktree(*i), selected);
        }
        let Row::Worktree(i) = row else { continue };
        if n >= inner.height as usize {
            continue;
        }
        let y = inner.y + n as u16;
        if app.tree[*i].starts_with([FOLD_OPEN, FOLD_CLOSED]) {
            hits.push(Hit { rect: Rect::new(inner.x + 1, y, 1, 1), target: Target::Fold(*i), selected: false });
        }
        if let Some(&hidden) = app.folded.get(&app.flat[*i].worktree.id) {
            let mut x = 0u16;
            for span in &lines[n].spans {
                let w = span.content.width() as u16;
                if span.content == fold_pill(hidden, wide(inner.width)) {
                    hits.push(Hit { rect: Rect::new(inner.x + x, y, w, 1), target: Target::Fold(*i), selected: false });
                    break;
                }
                x += w;
            }
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}

// ---------------------------------------------------------------- reviews

/// The review-requests page: one row per PR waiting on your review, grouped
/// by repo when there are several.
fn draw_reviews(f: &mut Frame, app: &App, theme: &Theme, area: Rect, now: i64, hits: &mut Vec<Hit>) {
    use crate::reviews_ui::{ReviewRow, order, rows};
    let list = app.snapshot.reviews.as_deref().unwrap_or(&[]);
    let grouping = app.grouping();
    let ordered = order(list, &app.snapshot.repos, grouping);
    let cursor = app.reviews.as_ref().map_or(0, |p| p.cursor);
    // Review-only, the page is all there is, so it names the tool too
    // (leaving the corner room for a badge).
    let heading = match app.reviews_only() {
        true => page_title_text("Review requests", (area.width as usize).saturating_sub(2 + 1 + 24)),
        false => "Review requests".to_string(),
    };
    let title_w = heading.width() + 2;
    let mut block = pane_block(theme, page_title(heading, "Review requests", theme), app.review_focused());
    if !ordered.is_empty() {
        block = foot(block, Line::styled(format!(" {} of {} ", cursor + 1, ordered.len()), theme.faint()).alignment(Alignment::Right));
    }
    // Bottom left: what the search is filtered by; a click opens the filters.
    let hidden = crate::reviews_ui::counts(list, &app.snapshot.repos, grouping).1;
    let summary = format!(" {} ", review_filter_summary(&app.settings, hidden));
    // Compact: on the top line after the title, so it shares the row.
    let lead = if compact() { title_w + 1 } else { 0 };
    let room = (area.width as usize).saturating_sub(16 + lead);
    if room > 10 {
        let summary = truncate(&summary, room);
        if let Some(k) = app.keymap.key(Command::Filter) {
            let rect = Rect::new(area.x + edge() + lead as u16, foot_y(area), summary.width() as u16, 1);
            hits.push(Hit { rect, target: Target::Key(k), selected: false });
        }
        block = foot(block, Line::styled(summary, theme.faint()).alignment(Alignment::Left));
    }
    let error = app.snapshot.review_error.as_ref();
    // Top right: a failed refresh beside a list we still have (from before,
    // or the cache), then the dashboard's review badge in the corner.
    let mut corner: Vec<Span> = Vec::new();
    if let Some(e) = error.filter(|_| !ordered.is_empty()) {
        let what = if e.rate_limited { "rate-limited" } else { "couldn't refresh" };
        let label = match reset_text(e, now) {
            Some(r) => format!(" ⚠ GitHub {what} ({r}) — showing cached "),
            None => format!(" ⚠ GitHub {what} — showing cached "),
        };
        corner.push(Span::styled(label, theme.bold(theme.yellow)));
    }
    // Beside the list (`V`), the ─Worktrees border already shows it.
    let split = app.reviews.as_ref().is_some_and(|p| p.split);
    if let Some((wide, narrow)) = review_badge_forms(app).filter(|_| !split) {
        // Two corners, `─Review requests `, the warning and one border cell.
        let used = corner.iter().map(|s| s.content.width()).sum::<usize>();
        let room = (area.width as usize).saturating_sub(2 + title_w + used + 1);
        if let Some(badge) = [wide, narrow].into_iter().find(|b| b.width() <= room) {
            corner.push(Span::styled(badge, theme.bold(theme.yellow).add_modifier(Modifier::REVERSED)));
        }
    }
    // Review-only: no ─Worktrees border to carry the update notice, so it
    // goes first here when it fits.
    if let Some(latest) = shown_update(app).map(|u| &u.latest).filter(|_| app.reviews_only()) {
        let used = corner.iter().map(|s| s.content.width()).sum::<usize>();
        let room = (area.width as usize).saturating_sub(2 + title_w + used + 1);
        if let Some(label) = update_forms(app, latest).into_iter().find(|l| l.width() <= room) {
            let w = label.width() as u16;
            let rect = Rect::new((area.x + area.width).saturating_sub(edge() + used as u16 + w), area.y, w, 1).intersection(area);
            hits.push(Hit { rect, target: Target::Update, selected: false });
            corner.insert(0, Span::styled(label, theme.bold(theme.green)));
        }
    }
    if !corner.is_empty() {
        block = block.title(Line::from(corner).alignment(Alignment::Right));
    }
    let inner = block.inner(area);
    f.render_widget(block, area);
    // Nothing to show and the last search failed: say why, not "Loading…"
    // (nor "Nothing waiting", which a stale empty list would claim).
    if let Some(e) = error.filter(|_| ordered.is_empty()) {
        let headline = if e.rate_limited {
            match reset_text(e, now) {
                Some(r) => format!("⚠ GitHub has rate-limited you — {r}"),
                None => "⚠ GitHub has rate-limited you".to_string(),
            }
        } else {
            "⚠ Couldn't load review requests".to_string()
        };
        let width = inner.width.saturating_sub(4) as usize;
        let mut lines = vec![Line::styled(headline, theme.bold(theme.yellow)), Line::raw("")];
        lines.extend(wrap_words(&e.message, width).into_iter().map(|l| Line::styled(l, theme.faint())));
        lines.push(Line::raw(""));
        lines.push(Line::styled("r tries again now · am retries on its own as GitHub allows", theme.faint()));
        let h = (lines.len() as u16).min(inner.height);
        let rect = Rect { y: inner.y + inner.height.saturating_sub(h) / 2, height: h, ..inner };
        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), rect);
        return;
    }
    if ordered.is_empty() {
        let msg = match &app.snapshot.reviews {
            // Review-only mode hides those switches; setup turns them back on.
            None if !app.settings.review_requests_on() && app.settings.reviews_mode() => {
                "Review requests are off — Settings (s) → Run setup again… turns them on"
            }
            None if !app.settings.review_requests_on() => "Review requests are off — turn them on in Settings (s)",
            None => "Loading…",
            Some(_) => "Nothing waiting on your review",
        };
        f.render_widget(Paragraph::new(Line::styled(msg, theme.faint())).alignment(Alignment::Center), centered_line(inner));
        return;
    }

    let words = wide(inner.width);
    let ticket_w = ordered.iter().map(|&i| list[i].linear.as_ref().map_or(0, |l| l.identifier.width())).max().unwrap_or(0);
    let pr_w = ordered.iter().map(|&i| pr_text(Some(&list[i].pr), words).width()).max().unwrap_or(0);
    let who_w = ordered.iter().map(|&i| reviewers_text(&list[i]).width()).max().unwrap_or(0).min(if words { 16 } else { 10 });

    let rows = rows(list, &ordered, &app.snapshot.repos, grouping);
    let tree = crate::reviews_ui::prefixes(list, &ordered, &app.snapshot.repos, grouping);
    // Outside the repo order, name each row's repo when there are several.
    let tag_repos = !grouping.by_repo()
        && ordered.windows(2).any(|w| list[w[0]].repo_id != list[w[1]].repo_id);
    let height = inner.height as usize;
    let sel_row = rows.iter().position(|r| *r == ReviewRow::Pr(cursor)).unwrap_or(0);
    let is_pr = |i: usize| matches!(rows[i], ReviewRow::Pr(_));
    let unscrolled = Cell::new(0);
    let offset = app.reviews.as_ref().map_or(&unscrolled, |p| &p.scroll);
    let start = scroll_start(offset, rows.len(), height, (sel_row, sel_row), is_pr);
    let mut lines = Vec::new();
    for (n, row) in rows.iter().skip(start).take(height).enumerate() {
        lines.push(match row {
            ReviewRow::Header(name) => {
                let rule = (inner.width as usize).saturating_sub(name.width() + 1);
                Line::from(vec![
                    Span::styled(format!("{name} "), theme.bold(theme.accent)),
                    Span::styled("─".repeat(rule), theme.fg(theme.border)),
                ])
            }
            ReviewRow::Pr(pos) => {
                let selected = *pos == cursor;
                push_row(hits, inner, n, Target::Item(List::Reviews, *pos), selected);
                let r = &list[ordered[*pos]];
                let repo = tag_repos
                    .then(|| app.snapshot.repos.iter().find(|x| x.id == r.repo_id).map_or(r.repo.as_str(), |x| x.name.as_str()));
                // The pull, then (once pulled) a review script it runs.
                let pull = app.review_pull(r).or_else(|| app.review_script_job(r));
                // A pulled PR's worktree with an agent in it: its status dot, as on the dashboard.
                let agent = r.worktree_id.as_deref().and_then(|id| {
                    app.snapshot.groups.iter().flat_map(|g| &g.worktrees).find(|v| v.worktree.id == id).and_then(|v| v.status())
                });
                review_line(theme, r, &tree[*pos], repo, (pull, agent), |id| app.is_deleting(id), selected, (who_w, ticket_w, pr_w), words, inner.width, now)
            }
        });
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// `resets in 12m` (or `resets now`) for a rate limit with a known reset time.
fn reset_text(e: &am_core::model::ReviewFetchError, now: i64) -> Option<String> {
    let at = e.resets_at?;
    Some(if at > now { format!("resets in {}", timefmt::short_age(at - now)) } else { "resets now".into() })
}

/// Who the review is pending from, kept short (the column matters less
/// than the title and author): `you` when you're asked by name, else the
/// first reviewer (`@team` for a team), then `+N` for the rest.
fn reviewers_text(r: &am_core::model::ReviewRequest) -> String {
    let short = |w: &String| match w.split_once('/') {
        Some((_, team)) => format!("@{team}"),
        None => w.clone(),
    };
    let first = r.requested.iter().find(|w| *w == "you").or(r.requested.first());
    match (first, r.requested.len()) {
        (None, _) => String::new(),
        (Some(w), 1) => short(w),
        (Some(w), n) => format!("{} +{}", short(w), n - 1),
    }
}

/// Whether an `f` filter is in force (`F` on and something picked).
fn picks_filters(s: &am_core::settings::Settings) -> bool {
    s.worktree_filters_enabled
        && (!s.worktree_filter_text.trim().is_empty()
            || !am_core::settings::worktree_status_picks(&s.worktree_filter_status).is_empty()
            || [&s.worktree_filter_pr, &s.worktree_filter_git].iter().any(|f| f.as_str() != "any"))
}

/// The dashboard's filters in a few words, e.g. `filters on: "auth" ·
/// waiting · CI failing · no main · 3 hidden`: the `f` filters (`filters
/// off` while `F` has them off), then the base `hide*` ones, which always
/// apply; `None` while only the defaults apply.
fn worktree_filter_summary(s: &am_core::settings::Settings, hidden: usize) -> Option<String> {
    let mut parts = Vec::new();
    if s.worktree_filters_enabled {
        let text = s.worktree_filter_text.trim();
        if !text.is_empty() {
            parts.push(format!("\"{text}\""));
        }
        let picks = am_core::settings::worktree_status_picks(&s.worktree_filter_status);
        if !picks.is_empty() {
            parts.push(picks.into_iter().map(|id| crate::settings_ui::multi_label(&am_core::settings::WORKTREE_STATUS_FILTERS, id)).collect::<Vec<_>>().join("/"));
        }
        match s.worktree_filter_pr.as_str() {
            "open" => parts.push("open PR".into()),
            "none" => parts.push("no PR".into()),
            "failing" => parts.push("CI failing".into()),
            _ => {}
        }
        match s.worktree_filter_git.as_str() {
            "changes" => parts.push("changes".into()),
            "clean" => parts.push("clean".into()),
            _ => {}
        }
        // Say so, ahead of what they pick.
        if let Some(first) = parts.first_mut() {
            *first = format!("filters on: {first}");
        }
    } else {
        parts.push("filters off".into());
    }
    for (on, label) in [
        (s.hide_merged_closed_prs, "no merged/closed"),
        (s.hide_idle_done_agents, "no idle/done"),
        (s.hide_without_linear_ticket, "ticketed only"),
    ] {
        if on {
            parts.push(label.to_string());
        }
    }
    // Hiding the main checkout is the default, so it's only named beside
    // another filter; showing it is worth saying.
    if !s.hide_main_branch {
        parts.push("main shown".into());
    } else if !parts.is_empty() {
        parts.push("no main".into());
    }
    if parts.is_empty() {
        return None;
    }
    if hidden > 0 {
        parts.push(format!("{hidden} hidden"));
    }
    Some(parts.join(" · "))
}

/// What the page shows in a few words, e.g. `you + your teams · 2 groups ·
/// 3 hidden`: who's asked, then the groups and what the default group hides.
fn review_filter_summary(s: &am_core::settings::Settings, hidden: usize) -> String {
    use am_core::settings::ReviewScope;
    let mut parts = vec![match s.review_scope() {
        ReviewScope::All => "you + your teams".to_string(),
        ReviewScope::Direct => "you only".to_string(),
        ReviewScope::Teams => "your teams only".to_string(),
    }];
    match s.review_groups.len() {
        0 => {}
        1 => parts.push("1 group".into()),
        n => parts.push(format!("{n} groups")),
    }
    let rules = s.review_default_group.summary();
    if !rules.is_empty() {
        parts.push(rules);
    }
    if hidden > 0 {
        parts.push(format!("{hidden} hidden"));
    }
    parts.join(" · ")
}

#[allow(clippy::too_many_arguments)]
fn review_line<'a>(
    theme: &Theme,
    r: &'a am_core::model::ReviewRequest,
    prefix: &str,
    repo: Option<&str>,
    (pull, agent): (Option<&crate::jobs::Job>, Option<am_core::status::Status>),
    deleting: impl Fn(&str) -> bool,
    selected: bool,
    (who_w, ticket_w, pr_w): (usize, usize, usize),
    words: bool,
    width: u16,
    now: i64,
) -> Line<'a> {
    let mut right: Vec<Span> = Vec::new();
    if who_w > 0 {
        right.push(Span::raw(" "));
        let style = if crate::reviews_ui::asks_you(r) { theme.bold(theme.magenta) } else { theme.fg(theme.magenta) };
        right.push(Span::styled(pad(&truncate(&reviewers_text(r), who_w), who_w), style));
    }
    if ticket_w > 0 {
        right.push(Span::raw(" "));
        let t = r.linear.as_ref().map(|l| l.identifier.clone()).unwrap_or_default();
        right.push(Span::styled(pad(&t, ticket_w), theme.fg(theme.blue)));
    }
    right.push(Span::raw(" "));
    right.extend(pr_spans(Some(&r.pr), theme, pr_w, words));
    let age = timefmt::parse_iso(&r.updated_at).map(|t| timefmt::short_age(now - t)).unwrap_or_default();
    right.push(Span::styled(format!(" {age:>3} "), theme.faint()));
    let right_w: usize = right.iter().map(|s| s.content.width()).sum();

    let mut tags: Vec<(String, ratatui::style::Style)> = Vec::new();
    if let Some(name) = repo {
        tags.push((format!(" ({name})"), theme.faint()));
    }
    match &r.worktree_id {
        Some(id) if deleting(id) => tags.push((format!(" {} removing…", spinner(now)), theme.fg(theme.red))),
        Some(_) => tags.push((" ✓ pulled".into(), theme.fg(theme.teal))),
        None => {}
    }
    if let Some(job) = pull.filter(|_| r.worktree_id.is_some()) {
        // Pulled: the job is running the review script.
        tags.push(match &job.state {
            crate::jobs::JobState::Failed(_) => (" ✗ script failed".into(), theme.fg(theme.red)),
            _ if job.question.is_some() => (" ? save the review script to run it (⇥ to run now)".into(), theme.fg(theme.yellow)),
            _ => {
                let last = job.log.last().map(|l| format!(" {}", truncate(l.trim(), 40))).unwrap_or_default();
                (format!(" {} running script…{last}", spinner(now)), theme.fg(theme.green))
            }
        });
    } else if let Some(job) = pull {
        tags.push(match &job.state {
            crate::jobs::JobState::Failed(_) => (" ✗ pull failed — t retries".into(), theme.fg(theme.red)),
            _ if job.question.is_some() => (" ? waiting — ⇥ to answer".into(), theme.fg(theme.yellow)),
            _ => {
                let step = job.current_step().map(|s| format!(" {s}")).unwrap_or_default();
                (format!(" {} pulling…{step}", spinner(now)), theme.fg(theme.green))
            }
        });
    }
    if r.is_cross_repository {
        tags.push((" fork".into(), theme.faint()));
    }
    let author = if r.author.is_empty() { String::new() } else { format!("  @{}", r.author) };
    let tags_w: usize = tags.iter().map(|(t, _)| t.width()).sum();
    let dot = agent.map(|s| (format!("{} ", status_glyph(Some(s))), theme.fg(theme.status(Some(s)))));
    let dot_w = dot.as_ref().map_or(0, |(d, _)| d.width());
    let room = (width as usize).saturating_sub(1 + prefix.width() + dot_w + right_w + tags_w);
    // The author always fits: the title gives way first, down to a few
    // cells (only a very narrow row cuts the author too).
    let author_w = author.width().min(room.saturating_sub(r.pr.title.width().min(10)));
    let title = truncate(&r.pr.title, r.pr.title.width().min(room.saturating_sub(author_w)));
    let author = truncate(&author, author_w);
    let fill = room.saturating_sub(title.width() + author.width());

    let mut spans = vec![
        Span::styled(if selected { "▌" } else { " " }, theme.bold(theme.border_focus)),
        Span::styled(prefix.to_string(), theme.faint()),
    ];
    spans.extend(dot.map(|(d, st)| Span::styled(d, st)));
    spans.extend([
        Span::styled(title, if selected { theme.bold(theme.text) } else { theme.fg(theme.text) }),
        Span::styled(author, theme.faint()),
    ]);
    spans.extend(tags.into_iter().map(|(t, st)| Span::styled(t, st)));
    spans.push(Span::raw(" ".repeat(fill)));
    spans.extend(right);
    let line = Line::from(spans);
    if !selected {
        return line;
    }
    if theme.name == "ansi" {
        // Reverse video: drop span colours so the row stays legible.
        let plain: Vec<Span> = line.spans.into_iter().map(|s| Span::raw(s.content)).collect();
        return Line::from(plain).style(theme.selected().add_modifier(Modifier::BOLD));
    }
    line.style(theme.selected())
}

/// First visible row of the worktree list (offset kept in `App::list_scroll`);
/// the selection's session rows count as part of it.
fn list_start(app: &App, height: usize) -> usize {
    let sel_row = app.rows.iter().position(|r| *r == Row::Worktree(app.selected)).unwrap_or(0);
    let sessions = app.rows[sel_row.min(app.rows.len())..].iter().skip(1).take_while(|r| matches!(r, Row::Session(..))).count();
    let is_row = |i: usize| matches!(app.rows[i], Row::Worktree(_) | Row::Session(..));
    scroll_start(&app.list_scroll, app.rows.len(), height, (sel_row, sel_row + sessions), is_row)
}

/// First visible line of a scrolling list of `len` lines, `height` of them on
/// screen. Every list scrolls this way: only as far as needed to keep the
/// selection (its first and last line) in view, so moving inside the page
/// never scrolls it; the offset is remembered in `offset` between frames.
/// Scrolling up brings the lines that aren't items (`is_item` false: headers,
/// blank lines) just above the selection into view too.
fn scroll_start(offset: &Cell<usize>, len: usize, height: usize, sel: (usize, usize), is_item: impl Fn(usize) -> bool) -> usize {
    let (first, last) = sel;
    // Never scroll the selection's first line off the top.
    let last = last.min(first + height.saturating_sub(1));
    let mut start = offset.get();
    if last >= start + height {
        start = last + 1 - height.max(1);
    } else if first < start {
        start = first;
        while start > 0 && last + 1 - start < height && !is_item(start - 1) {
            start -= 1;
        }
    }
    // Never leave blank lines below the list (it shrank, or the area grew).
    start = start.min(len.saturating_sub(height));
    offset.set(start);
    start
}

/// The pill on a folded stack's root: ` +3 stacked hidden ` (` +3 ` when narrow).
fn fold_pill(hidden: usize, wide: bool) -> String {
    if wide { format!(" +{hidden} stacked hidden ") } else { format!(" +{hidden} ") }
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
    // With its sessions listed underneath, the row shows how many there are;
    // harness, subagents and context move to the session rows.
    let listed = v.sessions.len() > 1;
    let subagents = if listed { 0 } else { app.running_subagents_for(&v.worktree.id) };
    let deleting = app.is_deleting(&v.worktree.id);

    // Right-hand columns, fixed width so they align down the list.
    let mut right: Vec<Span> = Vec::new();
    if cols.ticket > 0 {
        right.push(Span::raw(" "));
        right.push(Span::styled(pad(&ticket_text(v), cols.ticket), theme.fg(theme.blue)));
    }
    if cols.pr > 0 {
        right.push(Span::raw(" "));
        right.extend(pr_spans(v.pr.as_ref(), theme, cols.pr, cols.pr_words));
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
        let usage = if listed { None } else { v.context.as_ref() };
        right.extend(ctx_spans(usage, theme, cols.ctx_bar, cols.ctx_window, deleting));
        ctx_range = start..right.len();
    }
    right.push(Span::raw(" "));
    let right_w: usize = right.iter().map(|s| s.content.width()).sum();

    let bar = if selected { if focused { "▌" } else { "▏" } } else { " " };
    // A stack root's fold arrow leads its tree prefix: accent while open,
    // yellow while folded.
    let (arrow, tree_rest) = match tree.chars().next() {
        Some(c @ (FOLD_OPEN | FOLD_CLOSED)) => (Some(c), &tree[c.len_utf8()..]),
        _ => (None, tree),
    };
    let mut left = vec![Span::styled(bar, theme.bold(theme.border_focus))];
    if let Some(c) = arrow {
        let color = if c == FOLD_CLOSED { theme.yellow } else { theme.accent };
        left.push(Span::styled(c.to_string(), theme.bold(color)));
    }
    left.extend([
        Span::styled(tree_rest.to_string(), theme.fg(theme.border)),
        if deleting {
            Span::styled(spinner(now), theme.fg(theme.red))
        } else {
            Span::styled(status_glyph(status), theme.fg(theme.status(status)))
        },
        Span::raw(" "),
    ]);
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
    // An agent's shell command (tests, a build…) is still running here.
    let commands = if deleting { 0 } else { v.shell_commands };
    let command_badge = match commands {
        0 => String::new(),
        n if wide(width) => format!(" $ {n} command{}", if n == 1 { "" } else { "s" }),
        1 => " $".to_string(),
        n => format!(" ${n}"),
    };
    badge.push_str(&command_badge);
    // Which agent CLI, for anything but the default (Claude Code).
    let agent_badge = match v.agent.as_ref().map(|a| a.agent) {
        _ if listed && !deleting => {
            let n = v.sessions.len();
            if wide(width) { format!(" ⧉ {n} sessions") } else { format!(" ⧉{n}") }
        }
        Some(kind) if !deleting && kind != HarnessKind::CLAUDE => {
            if wide(width) { format!(" ✦ {}", kind.id()) } else { " ✦".to_string() }
        }
        _ => String::new(),
    };
    badge.push_str(&agent_badge);
    // Busy, but only waiting on something outside (`gh run watch`, `sleep`…).
    let watch_badge = match v.watching() {
        Some(_) if !deleting && !listed => if wide(width) { " ⧗ watching".to_string() } else { " ⧗".to_string() },
        _ => String::new(),
    };
    badge.push_str(&watch_badge);
    if !deleting && subagents > 0 {
        if wide(width) {
            badge.push_str(&format!(" ◆ {subagents} subagent{}", if subagents == 1 { "" } else { "s" }));
        } else {
            badge.push_str(&format!(" ◆{subagents}"));
        }
    }
    // A folded stack (←): a loud pill saying how much it hides, so a
    // hidden stack is never missed.
    let fold_badge = app.folded.get(&v.worktree.id).map(|n| format!(" {}", fold_pill(*n, wide(width)))).unwrap_or_default();
    // The tmp space mixes repos: name the row's own when several are tracked.
    let repo_tag = match app.snapshot.repos.iter().find(|r| r.id == v.worktree.repo_id) {
        Some(r) if v.worktree.tmp_space && app.snapshot.repos.len() > 1 => format!(" ({})", r.name),
        _ => String::new(),
    };
    let name_w = (width as usize).saturating_sub(right_w + 3 + badge.width() + fold_badge.width() + repo_tag.width() + tree.width());
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
    } else if commands > 0 {
        theme.bold(theme.teal)
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
    if !repo_tag.is_empty() {
        left.push(Span::styled(repo_tag, theme.faint()));
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
    if let Some(r) = rest.strip_prefix(command_badge.as_str()).filter(|_| !command_badge.is_empty()) {
        left.push(Span::styled(command_badge.clone(), theme.bold(theme.teal)));
        rest = r;
    }
    if let Some(r) = rest.strip_prefix(agent_badge.as_str()).filter(|_| !agent_badge.is_empty()) {
        left.push(Span::styled(agent_badge.clone(), theme.fg(if listed { theme.magenta } else { theme.blue })));
        rest = r;
    }
    if let Some(r) = rest.strip_prefix(watch_badge.as_str()).filter(|_| !watch_badge.is_empty()) {
        left.push(Span::styled(watch_badge.clone(), theme.fg(theme.blue)));
        rest = r;
    }
    if let Some(r) = rest.strip_prefix(" deleting…") {
        left.push(Span::styled(" deleting…", theme.fg(theme.red)));
        rest = r;
    }
    left.push(Span::styled(rest.to_string(), theme.fg(theme.magenta)));
    if let Some(pill) = fold_badge.strip_prefix(' ') {
        left.push(Span::raw(" "));
        left.push(Span::styled(pill.to_string(), theme.bold(theme.yellow).add_modifier(Modifier::REVERSED)));
    }
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

/// One of several live sessions in a worktree, under its row:
/// `├ ● claude  executing · 2m  ◆ 1 subagent` (plus a short `#3f2a` session
/// id when two sessions share a harness), with its own context bar in the
/// worktree's context column. The other columns are left blank.
#[allow(clippy::too_many_arguments)]
fn session_line<'a>(
    app: &App,
    theme: &Theme,
    v: &WorktreeView,
    tree: &str,
    k: usize,
    cols: &Columns,
    width: u16,
    now: i64,
) -> Line<'a> {
    let s: &SessionView = &v.sessions[k];
    let deleting = app.is_deleting(&v.worktree.id);
    let last = k + 1 == v.sessions.len();
    let subagents = app.running_subagents_in(&v.worktree.id, &s.session_id);

    // Same right-hand layout as `worktree_line`, blank but for the context bar.
    let mut blank = 0;
    for w in [cols.ticket, cols.pr, cols.git, cols.age] {
        if w > 0 {
            blank += 1 + w;
        }
    }
    let mut right: Vec<Span> = vec![Span::raw(" ".repeat(blank))];
    if cols.ctx_bar > 0 {
        right.push(Span::raw(" "));
        right.extend(ctx_spans(s.context.as_ref(), theme, cols.ctx_bar, cols.ctx_window, deleting));
    }
    right.push(Span::raw(" "));
    let right_w: usize = right.iter().map(|s| s.content.width()).sum();

    // The connector sits under the worktree's status glyph.
    let indent = " ".repeat(1 + tree.width());
    let connector = if last { "└ " } else { "├ " };
    let status = Some(s.status);
    let glyph_style = if deleting { theme.faint() } else { theme.fg(theme.status(status)) };
    let harness = s.agent.id().to_string();
    let harness_style = if deleting {
        theme.faint()
    } else if s.status == Status::Waiting {
        theme.bold(theme.yellow)
    } else if s.agent == HarnessKind::CLAUDE {
        theme.fg(theme.text)
    } else {
        theme.fg(theme.blue)
    };
    let age = timefmt::parse_sqlite(&s.updated_at).map(|t| timefmt::short_age(now - t)).unwrap_or_default();
    // Executing, but only watching something: say what.
    let label = match s.watching() {
        Some(what) => format!("watching {what}"),
        None => status_label(status).to_string(),
    };
    let detail = if age.is_empty() { format!("  {label}") } else { format!("  {label} · {age}") };
    let badge = match subagents {
        0 => String::new(),
        n if wide(width) => format!(" ◆ {n} subagent{}", if n == 1 { "" } else { "s" }),
        n => format!(" ◆{n}"),
    };

    // Two sessions of one harness: a short session id tells them apart.
    let twins = v.sessions.iter().filter(|o| o.agent == s.agent).count() > 1;
    let tag = if twins { format!(" #{}", s.session_id.chars().take(4).collect::<String>()) } else { String::new() };

    let fixed = indent.width() + connector.width() + 2 + right_w;
    let avail = (width as usize).saturating_sub(fixed);
    let harness = truncate(&harness, avail);
    let tag = truncate(&tag, avail.saturating_sub(harness.width()));
    let detail = truncate(&detail, avail.saturating_sub(harness.width() + tag.width() + badge.width()));
    let used = harness.width() + tag.width() + detail.width() + badge.width();
    let mut spans = vec![
        Span::raw(indent),
        Span::styled(connector, theme.fg(theme.border)),
        Span::styled(status_glyph(status), glyph_style),
        Span::raw(" "),
        Span::styled(harness, harness_style),
        Span::styled(tag, theme.faint()),
        Span::styled(detail, theme.faint()),
    ];
    if used <= avail {
        spans.push(Span::styled(badge, theme.fg(theme.magenta)));
    }
    spans.push(Span::raw(" ".repeat(avail.saturating_sub(used))));
    spans.extend(right);
    Line::from(spans)
}

/// Context-window bar plus tokens used (`████▌░░░ 380k`). The fill and the
/// count go green → yellow → red as the session nears its window.
fn ctx_spans<'a>(usage: Option<&ContextUsage>, theme: &Theme, bar_w: usize, configured: u64, deleting: bool) -> Vec<Span<'a>> {
    // Compact: the bar without the token count.
    let count_w = if compact() { 0 } else { 5 };
    let Some(u) = usage else { return vec![Span::raw(" ".repeat(bar_w + count_w))] };
    let window = u.window(configured);
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
    let mut spans = vec![Span::styled(filled, style), Span::styled(empty, theme.faint())];
    if !compact() {
        spans.push(Span::styled(format!(" {:>4}", context::short_tokens(u.tokens)), style));
    }
    spans
}

/// `frac` of `width` cells: (filled part, `░` remainder). A partly filled
/// cell is a shade (`░▒▓`) rather than an eighth block like `▋`: those are
/// left-aligned, so the rest of the cell would show as a gap before the track.
fn bar_cells(frac: f64, width: usize) -> (String, String) {
    const PARTIAL: [char; 8] = [' ', '░', '░', '▒', '▒', '▒', '▓', '▓'];
    let eighths = (frac.clamp(0.0, 1.0) * (width * 8) as f64).round() as usize;
    let mut filled = "█".repeat(eighths / 8);
    let mut cells = eighths / 8;
    if !eighths.is_multiple_of(8) {
        filled.push(PARTIAL[eighths % 8]);
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
fn draw_jobs(f: &mut Frame, app: &App, theme: &Theme, area: Rect, now: i64, hits: &mut Vec<Hit>) {
    use crate::jobs::{JobKind, JobState, Question};
    let jobs = &app.jobs;
    let mut block = pane_block(theme, title("Jobs", theme), jobs.focused);
    // Anywhere in the pane focuses it; rows, answers and hints below win.
    hits.push(Hit { rect: area, target: Target::JobsPane, selected: false });
    let hint = if jobs.waiting() && !jobs.focused {
        block = foot(block.border_style(theme.bold(theme.yellow)), Line::styled(" ⇥ answer ", theme.bold(theme.yellow)).alignment(Alignment::Right));
        " ⇥ answer "
    } else if jobs.focused {
        block = foot(block, Line::styled(" ⇥ back  x dismiss ", theme.faint()).alignment(Alignment::Right));
        " ⇥ back  x dismiss "
    } else {
        ""
    };
    let inner = block.inner(area);
    f.render_widget(block, area);
    let width = inner.width as usize;

    let mut lines: Vec<Line> = Vec::new();
    // Lines a click acts on: the job's label row(s) and its answers.
    let mut marks: Vec<(std::ops::Range<usize>, Target, bool)> = Vec::new();
    // `lead` (e.g. "   ✓ ") then `text`, wrapped with continuation lines
    // indented under the text. Returns the lines it added.
    let mut push = |lead: Vec<Span<'static>>, text: &str, style: ratatui::style::Style| {
        let first_line = lines.len();
        let indent: usize = lead.iter().map(|s| s.content.width()).sum();
        let mut rows = wrap_words(text, width.saturating_sub(indent).max(8)).into_iter();
        let mut first = lead;
        first.push(Span::styled(rows.next().unwrap_or_default(), style));
        lines.push(Line::from(first));
        for r in rows {
            lines.push(Line::from(vec![Span::raw(" ".repeat(indent)), Span::styled(r, style)]));
        }
        first_line..lines.len()
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
        let label = push(
            vec![Span::styled(if cursor { "▌" } else { " " }, theme.bold(theme.border_focus)), Span::styled(format!("{glyph} "), theme.bold(color))],
            &job.label,
            theme.bold(theme.text),
        );
        marks.push((label, Target::Item(List::Jobs, i), false));
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
                    let r = push(choice("⏎ / p", false), &pull, theme.fg(theme.text));
                    marks.push((r, Target::JobKey(i, Key::Enter), false));
                    // The tmp space only checks out existing branches: no new-branch choices.
                    if !req.tmp_space {
                        let disc = if *local { "Create disconnected (deletes local, no tracking)" } else { "Create disconnected (new branch from base, no tracking)" };
                        let r = push(choice("c", *local), disc, theme.fg(theme.text));
                        marks.push((r, Target::JobKey(i, Key::Char('c')), false));
                    }
                } else {
                    let r = push(choice("⏎ / r", false), "Reuse existing local branch", theme.fg(theme.text));
                    marks.push((r, Target::JobKey(i, Key::Enter), false));
                    if !req.tmp_space {
                        let r = push(choice("d", true), "Delete local branch and re-create from base", theme.fg(theme.text));
                        marks.push((r, Target::JobKey(i, Key::Char('d')), false));
                    }
                }
                let r = push(choice("n", false), "Cancel", theme.fg(theme.text));
                    marks.push((r, Target::JobKey(i, Key::Char('n')), false));
            }
            (Some(Question::SaveReviewScript { repo_id, .. }), _) => {
                push(vec![Span::raw("   ")], "Runs the review script once you save it:", theme.bold(theme.yellow));
                push(vec![Span::raw("   ")], &am_core::worktrees::ScriptKind::Review.path(repo_id).display().to_string(), theme.faint());
                let r = push(choice("⏎ / y", false), "Run it now", theme.fg(theme.text));
                    marks.push((r, Target::JobKey(i, Key::Enter), false));
                let r = push(choice("n", false), "Skip it", theme.fg(theme.text));
                    marks.push((r, Target::JobKey(i, Key::Char('n')), false));
            }
            (Some(Question::RunScript { repo_id, .. }), _) => {
                push(vec![Span::raw("   ")], "Run the repo's startup script?", theme.bold(theme.yellow));
                push(vec![Span::raw("   ")], &am_core::worktrees::script_path(repo_id).display().to_string(), theme.faint());
                let where_ = if app.settings.startup_script_in_window() { "Run in a new terminal window" } else { "Run here (output below)" };
                let r = push(choice("⏎ / y", false), where_, theme.fg(theme.text));
                    marks.push((r, Target::JobKey(i, Key::Enter), false));
                let r = push(choice("n", false), "Skip", theme.fg(theme.text));
                    marks.push((r, Target::JobKey(i, Key::Char('n')), false));
            }
            _ => {}
        }
        for l in job.log.iter().skip(job.log.len().saturating_sub(200)) {
            push(vec![Span::raw("   ")], l, theme.faint());
        }
    }
    // Keep the bottom (newest output) in view.
    let scroll = (lines.len() as u16).saturating_sub(inner.height);
    for (range, target, selected) in marks {
        for line in range {
            if let Some(row) = line.checked_sub(scroll as usize) {
                push_row(hits, inner, row, target, selected);
            }
        }
    }
    if !hint.is_empty() {
        foot_hint_hits(hits, hint, area);
    }
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

fn pr_text(pr: Option<&am_core::model::PrInfo>, words: bool) -> String {
    let Some(pr) = pr else { return String::new() };
    let merged = if pr.state == "MERGED" { "⇡" } else { "" };
    let review = review_state(pr).map(|r| format!(" {}", if words { r.word() } else { r.glyph() })).unwrap_or_default();
    format!("#{}{}{merged}{review}", pr.number, checks_glyph(pr.checks_status))
}

/// Where an open PR stands on review, combined with CI and conflicts so
/// "approved and good to go" stands apart from "approved but failing";
/// or `Closed` for a PR closed without merging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Review {
    /// Approved, checks passing (or none), no merge conflicts: bold green `approved`.
    Ready,
    /// Approved, but checks are failing or still running: yellow `approved`.
    Approved,
    /// Approved and green, but conflicts with its base.
    Conflicts,
    ChangesRequested,
    /// Waiting for a review.
    Pending,
    /// A draft not yet approved (an approved draft reads `approved`).
    Draft,
    /// Closed without merging (a merged PR shows `⇡` instead).
    Closed,
}

impl Review {
    fn word(self) -> &'static str {
        match self {
            Review::Ready => "approved",
            Review::Approved => "approved",
            Review::Conflicts => "conflicts",
            Review::ChangesRequested => "changes",
            Review::Pending => "review",
            Review::Draft => "draft",
            Review::Closed => "closed",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Review::Ready | Review::Approved => "✔",
            Review::Conflicts => "⚠",
            Review::ChangesRequested => "!",
            Review::Pending => "…",
            Review::Draft => "~",
            Review::Closed => "⊘",
        }
    }

    fn color(self) -> ColorPick {
        match self {
            Review::Ready => |t| t.green,
            Review::Approved | Review::Conflicts => |t| t.yellow,
            Review::ChangesRequested | Review::Closed => |t| t.red,
            Review::Pending | Review::Draft => |t| t.faint,
        }
    }
}

pub(crate) fn review_state(pr: &am_core::model::PrInfo) -> Option<Review> {
    if pr.state == "CLOSED" {
        return Some(Review::Closed);
    }
    if pr.state != "OPEN" {
        return None;
    }
    // An approval shows even on a draft (and whatever CI says); otherwise a
    // draft just reads `draft`.
    if pr.is_draft && pr.review_decision != "APPROVED" {
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
/// then the review state (`approved` / `changes` / `review`…).
fn pr_spans<'a>(pr: Option<&am_core::model::PrInfo>, theme: &Theme, width: usize, words: bool) -> Vec<Span<'a>> {
    let Some(pr) = pr else { return vec![Span::raw(" ".repeat(width))] };
    let text = pr_text(Some(pr), words);
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



/// The dashboard's key-bar commands, grouped (`None` = a `│` separator).
/// Lower priority survives narrower widths; what doesn't fit is still in the
/// help overlay. Each lands in its key's layer (`actions::Layer`), so `F`,
/// `V`, `A` and `G` only show while Shift is held, `ctrl-v` with Ctrl; where
/// held keys aren't reported they join the plain bar, last to fit.
const KEYS: &[(Option<Command>, u8)] = &[
    (Some(Command::Open), 0),
    (None, 0),
    (Some(Command::OpenPr), 0),
    (Some(Command::OpenLinear), 0),
    (Some(Command::OpenProject), 3),
    (None, 0),
    (Some(Command::NewWorktree), 1),
    (Some(Command::Delete), 1),
    (Some(Command::Filter), 2),
    (Some(Command::ToggleFilters), 3),
    (Some(Command::AddRepo), 3),
    (None, 0),
    (Some(Command::ReviewRequests), 1),
    (Some(Command::ReviewSplit), 3),
    (Some(Command::ReviewWindow), 3),
    (Some(Command::Settings), 1),
    (Some(Command::ToggleLog), 3),
    (Some(Command::Refresh), 1),
    (Some(Command::Bottom), 3),
];

/// A key-bar entry: a key hint or a `│` separator.
enum KeyItem {
    Key { key: String, label: std::borrow::Cow<'static, str>, hit: Option<BarHit>, available: bool },
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

/// The dashboard's actions.
fn dashboard_actions(app: &App) -> ActionBar {
    let mut bar = ActionBar::new();
    if app.reviews.is_some() {
        bar.key(Key::Tab, "reviews", 0);
        bar.sep();
    } else if app.jobs.visible() {
        bar.key(Key::Tab, if app.jobs.waiting() { "answer" } else { "jobs" }, 0);
        bar.sep();
    }
    for &(cmd, prio) in KEYS {
        match cmd {
            None => bar.sep(),
            Some(Command::ToggleFilters) => {
                let label = if app.settings.worktree_filters_enabled { "filters off" } else { "filters on" };
                bar.command(&app.keymap, Command::ToggleFilters, label, prio);
            }
            Some(Command::ReviewSplit) => bar.command(&app.keymap, Command::ReviewSplit, "reviews beside list", prio),
            Some(Command::ReviewWindow) => bar.command(&app.keymap, Command::ReviewWindow, "reviews in new window", prio),
            Some(Command::Bottom) => bar.command(&app.keymap, Command::Bottom, "bottom", prio),
            Some(c) => {
                if let Some(label) = c.bar_label() {
                    bar.command(&app.keymap, c, label, prio);
                }
            }
        }
    }
    bar.tail_command(&app.keymap, Command::Help, "help", 0);
    bar.tail_command(&app.keymap, Command::Quit, "quit", 0);
    bar
}

/// The review page's actions: the page's own meaning for each key, pressed
/// as keys (`App::review_key`).
fn review_actions(app: &App) -> ActionBar {
    let mut bar = ActionBar::pressing_keys();
    let km = &app.keymap;
    let split = app.reviews.as_ref().is_some_and(|p| p.split);
    // ⏎ opens a pulled PR's worktree, pulls one that isn't, and opens a fork's PR.
    let enter = match app.selected_review() {
        _ if app.review_worktree().is_some() => "open",
        Some(r) if !r.is_cross_repository => "pull",
        _ => "open PR",
    };
    bar.key(Key::Enter, enter, 0);
    if enter != "open PR" {
        bar.command(km, Command::OpenPr, "PR", 1);
    }
    bar.command(km, Command::OpenLinear, "ticket", 0);
    bar.command(km, Command::Delete, "remove", 2);
    bar.command(km, Command::Filter, "filter", 2);
    // What a held key adds: pull + run the review script, the page beside
    // the list or in a new window.
    bar.command(km, Command::PullAndRunScript, "pull + run script", 3);
    bar.key(Key::CtrlEnter, "pull + run script", 0);
    if !app.reviews_only() {
        bar.command(km, Command::ReviewSplit, if split { "close" } else { "beside list" }, 3);
        bar.command(km, Command::ReviewWindow, "new window", 3);
    }
    bar.sep();
    bar.command(km, Command::Refresh, "refresh", 1);
    if split {
        bar.key(Key::Tab, if app.jobs.visible() { "jobs" } else { "list" }, 0);
    } else if app.jobs.waiting() {
        // The pane only shows here while a job waits for an answer.
        bar.key(Key::Tab, "answer", 0);
    }
    bar.tail_command(km, Command::Help, "help", 2);
    let esc = match () {
        _ if split => "close",
        _ if app.reviews_only() => "quit",
        _ => "back",
    };
    bar.tail_key(Key::Esc, esc, 0);
    bar
}

/// The jobs pane's actions, while it has the keys.
fn jobs_actions(app: &App) -> ActionBar {
    // Tab moves between the list (or the full review page) and the pane.
    let back = if app.reviews.as_ref().is_some_and(|p| !p.split) { "back to reviews" } else { "back to list" };
    let mut bar = ActionBar::new();
    bar.key(Key::Tab, back, 0);
    bar.note("j/k", "select job", 1);
    bar.key(Key::Char('x'), "dismiss", 1);
    bar.tail_key(Key::Char('q'), "quit", 0);
    bar
}

/// The hints that fit in `width` (lower-priority ones drop first), after a
/// one-cell left margin, for whichever of the list, the review page and the
/// jobs pane has the keys, and whatever modifier is held. Shared by drawing
/// and click hit-testing.
fn keybar_items(app: &App, width: usize) -> Vec<KeyItem> {
    let bar = if app.review_focused() {
        review_actions(app)
    } else if app.jobs.focused {
        jobs_actions(app)
    } else {
        dashboard_actions(app)
    };
    let entries = bar.visible(app.held, app.modifiers_reported);
    let pick = |max_prio: u8| -> Vec<KeyItem> {
        let mut items = Vec::new();
        let mut last_sep = true;
        for entry in &entries {
            let Entry::Hint(h) = entry else {
                if !last_sep {
                    items.push(KeyItem::Sep);
                    last_sep = true;
                }
                continue;
            };
            if h.prio > max_prio {
                continue;
            }
            let available = match h.hit {
                Some(BarHit::Command(c)) => app.key_available(c),
                Some(BarHit::Key(k)) if app.review_focused() => app.review_key_available(k),
                _ => true,
            };
            items.push(KeyItem::Key { key: h.key.clone(), label: h.label.clone(), hit: h.hit, available });
            last_sep = false;
        }
        if last_sep {
            items.pop_if(|i| matches!(i, KeyItem::Sep));
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
    let offline = match (h.github_failing, h.linear_failing, h.github_rate_limited) {
        (true, true, true) => Some("GitHub rate-limited, Linear offline"),
        (true, false, true) => Some("GitHub rate-limited"),
        (true, true, false) => Some("GitHub + Linear offline"),
        (true, false, false) => Some("GitHub offline"),
        (false, true, _) => Some("Linear offline"),
        _ => None,
    };
    match (&app.notice, offline) {
        (Some(n), _) => Some((format!("{n} "), false)),
        (None, Some(what)) => Some((format!("⚠ {what} — cached "), true)),
        _ => None,
    }
}

fn keybar_areas(app: &App, area: Rect) -> [Rect; 2] {
    let status_w = keybar_status(app).map_or(0, |(s, _)| s.width());
    Layout::horizontal([Constraint::Min(0), Constraint::Length(status_w as u16)]).areas(area)
}

fn draw_keybar(f: &mut Frame, app: &App, theme: &Theme, area: Rect, hits: &mut Vec<Hit>) {
    let [keys_area, status_area] = keybar_areas(app, area);
    if let Some((text, warn)) = keybar_status(app) {
        let color = if warn { theme.yellow } else { theme.accent };
        f.render_widget(Paragraph::new(Line::from(Span::styled(text, theme.fg(color)))), status_area);
    }
    let mut spans = vec![Span::raw(" ")];
    // Every hint is clickable; dimmed ones too, the command itself refuses
    // them like a key press would.
    let mut col = keys_area.x + 1;
    for item in keybar_items(app, keys_area.width as usize) {
        let w = item.width() as u16;
        if let KeyItem::Key { hit: Some(hit), .. } = &item {
            let target = match *hit {
                BarHit::Key(k) => Target::Key(k),
                BarHit::Command(c) => Target::Command(c),
            };
            hits.push(Hit { rect: Rect::new(col, keys_area.y, w.saturating_sub(2), 1), target, selected: false });
        }
        col += w;
        match item {
            KeyItem::Sep => spans.push(Span::styled("│ ", theme.fg(theme.border))),
            // Actions the selected worktree can't take stay in place, dimmed.
            KeyItem::Key { key, label, available: true, .. } => {
                spans.push(Span::styled(key, theme.bold(theme.accent)));
                spans.push(Span::styled(format!(" {label}  "), theme.faint()));
            }
            KeyItem::Key { key, label, available: false, .. } => {
                let off = theme.faint().add_modifier(Modifier::DIM);
                spans.push(Span::styled(key, off));
                spans.push(Span::styled(format!(" {label}  "), off));
            }
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), keys_area);
}

/// A command's key on the help overlay: its binding, else its first fixed
/// key when the line adds none of its own (`fixed` empty), else `—`.
fn help_key(keymap: &crate::keymap::Keymap, c: Command, fixed: &str) -> String {
    match (keymap.key(c), c.fixed_keys().first(), fixed.is_empty()) {
        (Some(k), _, _) => crate::keymap::display(k),
        (None, Some(k), true) => crate::keymap::display(*k),
        _ => "—".into(),
    }
}

/// Draw a help overlay: `lines` in a centred `Keys` box.
fn draw_help_box(f: &mut Frame, theme: &Theme, area: Rect, lines: Vec<Line>) {
    let w = if compact() { area.width } else { 80.min(area.width.saturating_sub(4)) };
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

/// The PR review-state legend line (`approved` is green with CI ✓, yellow
/// with CI ✗/◌).
fn pr_legend(theme: &Theme) -> Line<'static> {
    let mut spans = vec![Span::styled("PR ", theme.faint())];
    for (r, note) in [
        (Review::Ready, "=CI ✓"),
        (Review::Approved, "=CI ✗/◌"),
        (Review::Conflicts, ""),
        (Review::ChangesRequested, ""),
        (Review::Pending, "=awaiting"),
        (Review::Closed, "=not merged"),
    ] {
        spans.push(Span::styled(r.word(), theme.fg(r.color()(theme))));
        spans.push(Span::styled(format!("{note}  "), theme.faint()));
    }
    Line::from(spans)
}

/// Help in review-only mode: the review page's keys (each with what it
/// does there, [`Command::in_reviews_mode`]) and its row legend.
fn draw_review_help(f: &mut Frame, theme: &Theme, area: Rect, keymap: &crate::keymap::Keymap) {
    use Command as C;
    let groups: &[(&[Command], &str)] = &[
        (&[C::MoveDown, C::MoveUp], "  ↑ / ↓"),
        (&[C::Top, C::Bottom], ""),
        (&[C::PageDown, C::PageUp], ""),
        (&[C::Open], ""),
        (&[C::OpenPr], ""),
        (&[C::OpenLinear], ""),
        (&[C::PullToTmpSpace], ""),
        (&[C::PullAndRunScript], " / ctrl-⏎"),
        (&[C::Delete], " / del"),
        (&[C::Filter], ""),
        (&[C::Refresh], ""),
        (&[C::Settings], ""),
        (&[C::Help], ""),
        (&[C::Quit], "  esc"),
    ];
    let desc = |cmds: &[Command]| match cmds {
        [C::MoveDown, ..] => "move".to_string(),
        [C::Top, ..] => "top / bottom".into(),
        [C::PageDown, ..] => "page down / up".into(),
        [C::Settings] => "settings (Mode: full brings the agent dashboard back)".into(),
        [C::Help] => "toggle help".into(),
        [C::PullAndRunScript] => "pull + run the review script (in a new terminal window)".into(),
        [c, ..] => c.in_reviews_mode().unwrap_or("").into(),
        [] => String::new(),
    };
    let mut lines: Vec<Line> = groups
        .iter()
        .map(|(cmds, fixed)| {
            let keys = format!("{}{fixed}", cmds.iter().map(|c| help_key(keymap, *c, fixed)).collect::<Vec<_>>().join(" / "));
            Line::from(vec![Span::styled(format!("{keys:>16}  "), theme.bold(theme.accent)), Span::raw(desc(cmds))])
        })
        .collect();
    lines.push(Line::from(vec![Span::styled(format!("{:>16}  ", "tab"), theme.bold(theme.accent)), Span::raw("focus the jobs pane (pulls and removals)")]));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("✓ pulled", theme.fg(theme.green)),
        Span::styled(" checked out in a worktree (⏎ opens it, d removes it)  ", theme.faint()),
        Span::styled("fork", theme.faint()),
        Span::styled(" from a fork: ⏎ opens the PR", theme.faint()),
    ]));
    lines.push(pr_legend(theme));
    draw_help_box(f, theme, area, lines);
}

fn draw_help(f: &mut Frame, theme: &Theme, area: Rect, keymap: &crate::keymap::Keymap) {
    use Command as C;
    // Each command's key (unbound: its fixed key, else `—`), then the
    // group's always-on keys.
    let groups: &[(&[Command], &str, &str)] = &[
        (&[C::MoveDown, C::MoveUp], "  ↑ / ↓", "move"),
        (&[C::Top, C::Bottom], "", "top / bottom"),
        (&[C::PageDown, C::PageUp], "", "page down / up"),
        (&[C::Collapse, C::Expand], "", "fold / unfold the selected stack (▾ / ▸, clickable)"),
        (&[C::Open], "", "open (IDE; resumes last agent session)"),
        (&[C::OpenPr, C::OpenLinear, C::OpenProject], "", "open PR / Linear ticket / Linear project"),
        (&[C::ToggleLog], "", "toggle log pane (Settings → View logs… opens the file)"),
        (&[C::NewWorktree], "", "new worktree"),
        (&[C::Delete], " / del", "delete worktree (asks first)"),
        (&[], "tab", "focus the jobs pane"),
        (&[C::ReviewRequests, C::ReviewSplit, C::ReviewWindow], "", "PRs to review: page / beside the list / new window"),
        (&[C::PullToTmpSpace, C::PullAndRunScript], " / ctrl-⏎", "review page: pull into Reviewing / + run script"),
        (&[C::AddRepo], "", "add repository"),
        (&[C::Filter, C::ToggleFilters], "", "filter worktrees / filters on/off"),
        (&[C::Settings], "", "settings (Key bindings… changes these keys)"),
        (&[C::Refresh], "", "refresh now (bypasses backoff)"),
        (&[C::Help], "", "toggle help"),
        (&[C::Quit], "  esc", "quit"),
    ];
    let keys: Vec<(String, &str)> = groups
        .iter()
        .map(|(cmds, fixed, desc)| {
            (format!("{}{fixed}", cmds.iter().map(|c| help_key(keymap, *c, fixed)).collect::<Vec<_>>().join(" / ")), *desc)
        })
        .collect();
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
    lines.push(Line::from(vec![
        Span::styled(format!("{:>16}  ", "hold ⇧ / ctrl"), theme.bold(theme.accent)),
        Span::raw("the key bar swaps to the keys they add (where supported)"),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(legend));
    lines.push(pr_legend(theme));
    lines.push(Line::from(vec![
        Span::styled("✦ gemini", theme.fg(theme.blue)),
        Span::styled(" session from another harness plugin, by id (no badge: Claude Code)", theme.faint()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("today 4.1M", theme.faint()),
        Span::styled(" tokens used today incl. cache (s → token usage: all / visible rows)", theme.faint()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("⧉ 2 sessions", theme.fg(theme.magenta)),
        Span::styled(" agents in one worktree, each listed below it with its own status", theme.faint()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("$ 1 command", theme.bold(theme.teal)),
        Span::styled(" an agent's shell command matching the process filter is running; the name turns teal", theme.faint()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("⧗ watching", theme.fg(theme.blue)),
        Span::styled(" busy, but only waiting on something outside (gh run watch, a Monitor, sleep)", theme.faint()),
    ]));
    draw_help_box(f, theme, area, lines);
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
    mod perf_tests {
        include!("perf_tests.rs");
    }

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
                tmp_space: false,
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
            shell_commands: 0,
            context: None,
            sessions: Vec::new(),
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
            parent: None,
        });
        refactor.context = Some(ContextUsage { tokens: 121_000, model: Some("claude-opus-5-5".into()), window: None });
        let mut waiting = view("w2", "r1", "fix/offline-cache", Some(Status::Waiting));
        waiting.git = Some(GitStatus::default());
        waiting.context = Some(ContextUsage { tokens: 185_000, model: Some("claude-opus-5-5".into()), window: None });
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
            stack_base: Default::default(),
            today_tokens: None,
            reviews: None,
            review_error: None,
            review_people: vec![],
            review_total: None,
            review_query_hits: Default::default(),
            worktree_paths: Vec::new(),
            hidden_worktrees: 0,
        };
        let settings = Settings { worktree_sort: vec![], ..Settings::default() };
        let mut app = App::new(settings);
        app.apply(snapshot);
        app
    }

    /// `screen` with the Settings row "Harness plugins…" value (`8 of 8 on ·
    /// default claude`, which counts the built-ins) replaced by a fixed text
    /// of the same width, so adding a harness doesn't change a snapshot.
    fn fixed_plugin_count(screen: &str) -> String {
        const LABEL: &str = "Harness plugins…";
        let fix = |line: &str| -> String {
            let Some(at) = line.find(LABEL) else { return line.to_owned() };
            let after = &line[at + LABEL.len()..];
            let Some(start) = after.find(|c: char| c != ' ') else { return line.to_owned() };
            let value = &after[start..];
            let len = value.find('│').unwrap_or(value.len());
            // The cell up to the border, less the one-space margin before it.
            let width = value[..len].chars().count().saturating_sub(1);
            let fixed = "N of N on · default claude";
            let mut text: String = fixed.chars().chain(std::iter::repeat(' ')).take(width).collect();
            if width < fixed.chars().count() {
                text = text.chars().take(width.saturating_sub(1)).chain(['…']).collect();
            }
            format!("{}{text} {}", &line[..at + LABEL.len() + start], &value[len..])
        };
        screen.lines().map(fix).collect::<Vec<_>>().join("\n")
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
    fn worktree_filters_box_and_border_summary() {
        let mut app = fixture();
        app.settings.worktree_filter_text = "auth".into();
        app.settings.worktree_filter_status = "waiting".into();
        app.snapshot.hidden_worktrees = 3;
        let screen = render(&app, 120, 16);
        assert!(screen.contains(r#" filters on: "auth" · waiting · no main · 3 hidden "#), "{screen}");
        // A click on the summary opens the box.
        let at = find(&app, 120, 16, "3 hidden");
        click(&mut app, 120, 16, at, false);
        assert_eq!(app.settings_editor.as_ref().map(|e| e.only), Some(Some(crate::settings_ui::WORKTREE_FILTERS)));
        insta::assert_snapshot!(render(&app, 120, 24));
        app.settings_editor = None;
        // Off: the summary says so; the base filters still show.
        app.settings.worktree_filters_enabled = false;
        assert!(render(&app, 120, 16).contains(" filters off · no main · 3 hidden "));
        // Only the defaults: nothing on the border.
        app.settings = am_core::settings::Settings { worktree_sort: vec![], ..Default::default() };
        assert!(!render(&app, 120, 16).contains("hidden"));
    }

    #[test]
    fn agent_status_checklist() {
        // Ticking saves settings.json: into the tests' temp data dir.
        let (_guard, _) = crate::settings_ui::tests::data_dir();
        let mut app = fixture();
        app.settings.worktree_filter_status = "waiting,done".into();
        app.snapshot.hidden_worktrees = 2;
        assert!(render(&app, 120, 16).contains(" filters on: waiting/done · no main · 2 hidden "));
        let mut ed = crate::settings_ui::SettingsEditor::only(&app.settings, crate::settings_ui::WORKTREE_FILTERS);
        ed.cursor = crate::settings_ui::FIELDS.iter().position(|f| f.key == "worktreeFilterStatus").unwrap();
        assert_eq!(ed.display(&crate::settings_ui::FIELDS[ed.cursor]), "waiting, done");
        app.settings_editor = Some(ed);
        app.on_key(Key::Enter);
        let screen = render(&app, 120, 24);
        assert!(screen.contains("[x] waiting") && screen.contains("[ ] no agent"), "{screen}");
        // A click on a row moves there; a second ticks it.
        let at = find(&app, 120, 24, "no agent");
        click(&mut app, 120, 24, at, false);
        click(&mut app, 120, 24, at, false);
        assert!(app.settings_editor.as_ref().unwrap().multi.as_ref().unwrap().picked[6]);
        insta::assert_snapshot!(render(&app, 120, 24));
    }

    fn update(latest: &str) -> am_core::update_check::Update {
        am_core::update_check::Update {
            latest: latest.into(),
            current: "2.1.1".into(),
            hint: am_core::update_check::InstallMethod::Homebrew.upgrade_hint(),
            method: am_core::update_check::InstallMethod::Homebrew,
        }
    }

    #[test]
    fn reset_settings_asks_first() {
        let mut app = fixture();
        app.settings_editor = Some(crate::settings_ui::SettingsEditor::new(&app.settings));
        app.settings_editor.as_mut().unwrap().cursor =
            crate::settings_ui::FIELDS.iter().position(|f| f.key == "resetSettings").unwrap();
        assert_eq!(app.on_key(Key::Enter), crate::app::Action::None);
        assert!(matches!(app.modal, Some(crate::modals::Modal::ConfirmReset)));
        insta::assert_snapshot!(render(&app, 100, 24));
        // ⏎ again (or any key but y) cancels and leaves the editor open.
        assert_eq!(app.on_key(Key::Enter), crate::app::Action::None);
        assert!(app.modal.is_none() && app.settings_editor.is_some());
        app.on_key(Key::Enter);
        assert_eq!(app.on_key(Key::Char('y')), crate::app::Action::ResetSettings);
        assert!(app.modal.is_none());
    }

    #[test]
    fn run_setup_again_opens_and_esc_cancels() {
        let mut app = fixture();
        app.settings_editor = Some(crate::settings_ui::SettingsEditor::new(&app.settings));
        app.settings_editor.as_mut().unwrap().cursor =
            crate::settings_ui::FIELDS.iter().position(|f| f.key == "rerunSetup").unwrap();
        assert_eq!(app.on_key(Key::Enter), crate::app::Action::RunSetup);
        // main.rs opens the wizard; a re-run is cancellable from its first step.
        app.settings_editor = None;
        let mut s = crate::setup::Setup::new(vec!["work".into()], &Settings::parse(r#"{"setupCompleted":true}"#));
        s.hook_binary = None;
        app.setup = Some(s);
        assert!(render(&app, 100, 30).contains("esc cancel"));
        assert_eq!(app.on_key(Key::Esc), crate::app::Action::None);
        assert!(app.setup.is_none());
        assert_eq!(app.notice.as_deref(), Some("Setup closed — settings unchanged"));
    }

    #[test]
    fn settings_show_update_banner_with_upgrade_command() {
        let mut app = fixture();
        app.update_available = Some(update("2.2.0"));
        app.settings_editor = Some(crate::settings_ui::SettingsEditor::new(&app.settings));
        insta::assert_snapshot!(fixed_plugin_count(&render(&app, 100, 24)));
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
    fn clicking_the_update_notice_asks_then_runs_the_upgrade_as_a_job() {
        use am_core::update_check::InstallMethod;
        let mut app = fixture();
        app.update_available = Some(update("2.2.0"));
        let at = find(&app, 120, 16, "⬆ v2.2.0 available");
        assert_eq!(target_at(&app, 120, 16, at), Some(Target::Update));
        assert_eq!(click(&mut app, 120, 16, at, false), crate::app::Action::None);
        assert!(matches!(&app.modal, Some(crate::modals::Modal::ConfirmUpdate(u)) if u.latest == "2.2.0"));
        let screen = render(&app, 120, 16);
        assert!(screen.contains("Update am v2.1.1 → v2.2.0?"), "{screen}");
        assert!(screen.contains("Runs: brew update && brew upgrade agent-monitor"), "{screen}");
        insta::assert_snapshot!(screen);
        let crate::app::Action::Update(job, InstallMethod::Homebrew) = app.on_key(Key::Char('y')) else { panic!("expected the upgrade") };
        assert!(app.modal.is_none());
        assert_eq!(app.jobs.updating(), Some("2.2.0"));
        assert!(render(&app, 120, 16).contains("⬆ updating to v2.2.0…"));
        // A second click while it runs doesn't start another.
        let at = find(&app, 120, 16, "⬆ updating to v2.2.0…");
        assert_eq!(click(&mut app, 120, 16, at, false), crate::app::Action::None);
        assert!(app.modal.is_none());
        assert_eq!(app.notice.as_deref(), Some("Already updating to v2.2.0"));
        assert_eq!(app.jobs.get(job).map(|j| j.label.as_str()), Some("Update am to v2.2.0"));
    }

    #[test]
    fn the_update_notice_says_how_to_update_where_am_cannot() {
        use am_core::update_check::{InstallMethod, Update};
        let mut app = fixture();
        app.update_available = Some(Update { method: InstallMethod::Cargo, hint: InstallMethod::Cargo.upgrade_hint(), ..update("2.2.0") });
        let at = find(&app, 120, 16, "⬆ v2.2.0 available");
        click(&mut app, 120, 16, at, false);
        assert!(app.modal.is_none());
        let popup = app.popup.clone().unwrap_or_default();
        assert!(popup.contains("am v2.2.0 is available") && popup.contains("cargo install --path"), "{popup}");
    }

    #[test]
    fn restart_after_an_update_waits_for_other_jobs() {
        let mut app = fixture();
        app.modal = Some(crate::modals::Modal::Restart { latest: "2.2.0".into() });
        insta::assert_snapshot!(render(&app, 120, 16));
        assert_eq!(app.on_key(Key::Enter), crate::app::Action::Restart);

        app.modal = Some(crate::modals::Modal::Restart { latest: "2.2.0".into() });
        assert_eq!(app.on_key(Key::Char('n')), crate::app::Action::None);
        assert_eq!(app.notice.as_deref(), Some("Restart am to use v2.2.0"));

        app.jobs.start_delete("w2".into(), "feature/x");
        app.modal = Some(crate::modals::Modal::Restart { latest: "2.2.0".into() });
        assert_eq!(app.on_key(Key::Char('y')), crate::app::Action::None, "a restart would drop the delete");
        assert_eq!(app.notice.as_deref(), Some("Restart am to use v2.2.0 once the job is done"));
    }

    #[test]
    fn the_review_only_page_update_notice_is_clickable() {
        let mut app = by_repo();
        app.settings.app_mode = "reviews".into();
        app.update_available = Some(update("2.2.0"));
        app.enter_reviews_mode();
        let at = find(&app, 120, 16, "⬆ v2.2.0 available");
        assert_eq!(target_at(&app, 120, 16, at), Some(Target::Update));
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
        snap.today_tokens = Some(output_tally(4_120_000));
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

    fn output_tally(output: u64) -> am_core::usage::Tally {
        am_core::usage::Tally {
            tokens: am_core::usage::TokenCounts { output, ..Default::default() },
            ..Default::default()
        }
    }

    #[test]
    fn tokens_label_follows_the_display_mode() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        let tokens = am_core::usage::TokenCounts {
            input: 2_000,
            cache_write_5m: 500_000,
            cache_write_1h: 1_000_000,
            cache_read: 18_600_000,
            output: 60_000,
        };
        snap.today_tokens = Some(am_core::usage::Tally { tokens, cost_usd: 31.456, unpriced: 0 });
        app.apply(snap);
        let label = |app: &App, w| tokens_label(app, w);
        assert_eq!(label(&app, 120).as_deref(), Some(" today 20M "));
        app.settings.daily_tokens_mode = "weighted".into();
        // 2k + 625k + 2M + 1.86M + 300k
        assert_eq!(label(&app, 120).as_deref(), Some(" today 4.8M weighted "));
        assert_eq!(label(&app, 30).as_deref(), Some(" today 4.8M "));
        app.settings.daily_tokens_mode = "cost".into();
        assert_eq!(label(&app, 120).as_deref(), Some(" today ~$31.46 "));
        app.snapshot.today_tokens.as_mut().unwrap().unpriced = 3;
        assert_eq!(label(&app, 120).as_deref(), Some(" today ~$31.46+ "));
        app.settings.daily_tokens_mode = "breakdown".into();
        app.settings.daily_tokens_scope = "visible".into();
        assert_eq!(label(&app, 120).as_deref(), Some(" today (visible) in 2k · write 1.5M · read 19M · out 60k "));
        assert_eq!(label(&app, 60).as_deref(), Some(" today in 2k · write 1.5M · read 19M · out 60k "));
        assert_eq!(label(&app, 40).as_deref(), Some(" today (visible) 20M "));
    }

    #[test]
    fn usd_label_rounds_large_amounts() {
        assert_eq!(usd_label(0.004, false), "~$0.00");
        assert_eq!(usd_label(99.994, false), "~$99.99");
        assert_eq!(usd_label(1234.5, true), "~$1234+");
    }

    #[test]
    fn dashboard_shows_todays_tokens_bottom_right() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        snap.today_tokens = Some(output_tally(182_400_000));
        app.apply(snap);
        insta::assert_snapshot!(render(&app, 120, 16));
    }

    #[test]
    fn dashboard_narrow_stacks_panes() {
        insta::assert_snapshot!(render(&fixture(), 80, 24));
    }

    fn compact(mut app: App) -> App {
        app.settings.compact_view = true;
        app.rebuild_rows();
        app
    }

    #[test]
    fn compact_dashboard_keeps_only_the_top_line() {
        let app = compact(fixture());
        let screen = render(&app, 60, 12);
        insta::assert_snapshot!("compact_dashboard", screen);
        let first = screen.lines().next().unwrap();
        assert!(first.starts_with("─am v0.0.0 · Worktrees") && first.contains(" of "), "{first}");
        // No side or bottom border (the key bar's `│` separators aside).
        let body = |s: &str| s.lines().rev().skip(1).map(str::to_string).collect::<Vec<_>>().join("\n");
        assert!(!body(&screen).contains('│') && !screen.contains('╰'), "{screen}");
        assert!(!app.rows.iter().any(|r| matches!(r, Row::Spacer | Row::Ticket(_))));
        // Same worktrees, same order.
        assert_eq!(app.flat.iter().map(|v| v.worktree.id.clone()).collect::<Vec<_>>(),
            fixture().flat.iter().map(|v| v.worktree.id.clone()).collect::<Vec<_>>());
        // Wide, it still uses glyphs and drops the token count.
        let wide = render(&app, 120, 12);
        assert!(wide.contains("◆2") && !wide.contains("subagents"), "{wide}");
    }

    #[test]
    fn compact_clicks_land_on_the_drawn_rows() {
        let mut app = compact(fixture());
        let at = find(&app, 60, 12, "fix/offline-cache");
        click(&mut app, 60, 12, at, false);
        assert_eq!(app.flat[app.selected].worktree.branch, "fix/offline-cache");
    }

    #[test]
    fn compact_puts_the_jobs_pane_and_split_reviews_below() {
        let mut app = compact(fixture());
        app.jobs.start_delete("w2".into(), "fix/offline-cache");
        insta::assert_snapshot!("compact_jobs_below", render(&app, 60, 16));
        let mut app = compact(by_repo());
        app.on_key(Key::Char('V'));
        // Stacked even where there's room side by side.
        let screen = render(&app, 120, 16);
        insta::assert_snapshot!("compact_review_split", screen);
        assert!(!screen.lines().next().unwrap().contains("Review requests"), "{screen}");
    }

    #[test]
    fn compact_review_page_and_overlays_fill_the_screen() {
        let mut app = compact(by_repo());
        app.on_key(Key::Char('v'));
        insta::assert_snapshot!("compact_review_page", render(&app, 60, 12));
        let mut app = compact(fixture());
        app.show_log = true;
        insta::assert_snapshot!("compact_log_pane", render(&app, 60, 16));
        let mut app = compact(fixture());
        app.settings_editor = Some(crate::settings_ui::SettingsEditor::new(&app.settings));
        let screen = render(&app, 60, 20);
        insta::assert_snapshot!("compact_settings", fixed_plugin_count(&screen));
        assert!(screen.lines().next().unwrap().starts_with("╭─Settings") && screen.lines().next().unwrap().width() == 60, "{screen}");
        let mut app = compact(fixture());
        let repo = am_core::model::Repository { id: "r".into(), path: "/r".into(), name: "r".into() };
        app.modal = Some(crate::modals::Modal::new_worktree(vec![repo], None, "", "main"));
        insta::assert_snapshot!("compact_new_worktree_modal", render(&app, 60, 20));
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
    fn repositories_page() {
        use crate::repos_ui::{RepoEditor, RepoRow};
        let mut app = fixture();
        let mut ed = crate::settings_ui::SettingsEditor::new(&app.settings);
        let row = |id: &str, path: &str, exists, worktrees, script| RepoRow {
            id: id.into(),
            name: id.into(),
            path: path.into(),
            exists,
            worktrees,
            script,
            review_script: script,
        };
        let mut rp = RepoEditor::new(vec![
            row("agent-monitor", "~/code/agent-monitor", true, 4, true),
            row("website", "~/code/website", true, 1, false),
            row("old-thing", "~/gone/old-thing", false, 2, false),
        ]);
        rp.cursor = 1;
        rp.confirm_remove = Some("website".into());
        ed.repos = Some(rp);
        app.settings_editor = Some(ed);
        insta::assert_snapshot!(render(&app, 100, 20));
    }

    #[test]
    fn add_repo_folder_browser() {
        use crate::folder_browser::{Entry, FolderBrowser};
        let mut app = fixture();
        let dir = std::path::PathBuf::from("/src");
        let e = |name: &str, is_repo, tracked| Entry { name: name.into(), path: dir.join(name), is_repo, tracked };
        let mut b = FolderBrowser::fixture(
            dir.clone(),
            vec![e("agent-monitor", true, true), e("website", true, false), e("notes", false, false), e("scratch", false, false)],
        );
        b.cursor = 2;
        app.modal = Some(crate::modals::Modal::AddRepo(b));
        insta::assert_snapshot!(render(&app, 100, 26));
    }

    #[test]
    fn help_overlay() {
        let mut app = fixture();
        app.on_key(Key::Char('h'));
        insta::assert_snapshot!(render(&app, 100, 20));
    }

    fn rebound(bindings: &str) -> App {
        let mut app = fixture();
        let mut settings = am_core::settings::Settings::parse(&format!(r#"{{"keyBindings":{bindings}}}"#));
        settings.worktree_sort = vec![];
        app.set_settings(settings);
        app
    }

    #[test]
    fn rebound_keys_drive_the_dashboard() {
        let mut app = rebound(r#"{"newWorktree":"N","moveDown":"J","open":"O"}"#);
        assert_eq!(app.on_key(Key::Char('n')), crate::app::Action::None);
        assert!(app.modal.is_none(), "n is free now");
        app.on_key(Key::Char('N'));
        assert!(app.modal.is_some());
        app.modal = None;
        app.on_key(Key::Char('J'));
        assert_eq!(app.selected, 1);
        app.on_key(Key::Down);
        assert_eq!(app.selected, 2, "arrows always move");
        assert_eq!(app.on_key(Key::Enter), crate::app::Action::None, "Enter is unbound");
        assert!(matches!(app.on_key(Key::Char('O')), crate::app::Action::Open(_)));
        assert_eq!(app.on_key(Key::CtrlC), crate::app::Action::Quit);
        // A double-click still opens, whatever Enter does.
        let row = find(&app, 120, 16, "fix/offline-cache");
        assert!(matches!(click(&mut app, 120, 16, row, true), crate::app::Action::Open(_)));
    }

    #[test]
    fn key_bar_and_help_show_rebound_keys() {
        let mut app = rebound(r#"{"newWorktree":"N","filter":"none"}"#);
        // `N` needs Shift, so it shows with Shift held.
        app.modifiers_reported = true;
        app.held.shift = true;
        let screen = render(&app, 120, 16);
        let bar = screen.lines().last().unwrap();
        assert!(bar.contains("N new") && !bar.contains("f filter"), "{bar}");
        // Clicking the hint runs the command.
        let hint = find(&app, 120, 16, "N new");
        click(&mut app, 120, 16, hint, false);
        assert!(app.modal.is_some());
        app.modal = None;
        app.on_key(Key::Char('h'));
        let help = render(&app, 100, 24);
        assert!(help.contains("N  new worktree") && help.contains("d / del  delete worktree"), "{help}");
        assert!(help.contains("— / F  filter worktrees"), "{help}");
    }

    #[test]
    fn key_bindings_page() {
        let mut app = rebound(r#"{"newWorktree":"N"}"#);
        let mut ed = crate::settings_ui::SettingsEditor::new(&app.settings);
        let mut keys = crate::keys_ui::KeyEditor::new(app.keymap.clone());
        keys.cursor = 4;
        ed.keys = Some(keys);
        app.settings_editor = Some(ed);
        insta::assert_snapshot!("key_bindings_page", render(&app, 100, 34));
        let keys = app.settings_editor.as_mut().unwrap().keys.as_mut().unwrap();
        keys.cursor = 12;
        keys.on_key(Key::Enter);
        insta::assert_snapshot!("key_bindings_capturing", render(&app, 100, 34));
        // Too narrow for the review-page column.
        let keys = app.settings_editor.as_mut().unwrap().keys.as_mut().unwrap();
        keys.on_key(Key::Esc);
        keys.cursor = 7;
        insta::assert_snapshot!("key_bindings_narrow", render(&app, 66, 34));
        // The list scrolls to keep the selection in view.
        let keys = app.settings_editor.as_mut().unwrap().keys.as_mut().unwrap();
        keys.on_key(Key::End);
        let screen = render(&app, 100, 34);
        assert!(screen.contains("Jump to bottom") && !screen.contains("Open worktree"), "{screen}");
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
        app.apply(Snapshot { health: crate::app::Health { github_failing: true, ..Default::default() }, ..Default::default() });
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

    /// Click a cell of a `w`×`h` screen (drawn first: clicks hit-test the last frame).
    fn click(app: &mut App, w: u16, h: u16, (x, y): (u16, u16), double: bool) -> crate::app::Action {
        render(app, w, h);
        app.on_mouse(crate::app::Mouse::Click { x, y, double })
    }

    /// What's under (x, y) on a freshly drawn `w`×`h` screen.
    fn target_at(app: &App, w: u16, h: u16, (x, y): (u16, u16)) -> Option<Target> {
        render(app, w, h);
        hit_at(&app.hits.borrow(), x, y).map(|h| h.target)
    }

    #[test]
    fn only_review_requests_that_arrive_later_are_announced() {
        use crate::reviews_ui::tests::review;
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        snap.reviews = Some(vec![review("r1", 1, "2026-10-26T10:00:00Z")]);
        app.apply(snap.clone());
        assert!(app.review_alerts.is_empty(), "the first list is only remembered");
        let mut new = review("r1", 2, "2026-10-27T10:00:00Z");
        new.pr.title = "Batch the PR queries".into();
        snap.reviews.as_mut().unwrap().push(new);
        app.apply(snap.clone());
        assert_eq!(app.review_alerts, [("Review requested: acme/r1 #2".to_string(), "Batch the PR queries — @alice".to_string())]);
        app.review_alerts.clear();
        app.apply(snap);
        assert!(app.review_alerts.is_empty(), "announced once");
    }

    #[test]
    fn a_review_request_brought_into_view_by_a_filter_is_not_announced() {
        use crate::reviews_ui::tests::review;
        let mut app = fixture();
        let mut settings = app.settings.clone();
        settings.review_default_group.draft = "ready".into();
        app.settings = settings.clone();
        let mut draft = review("r1", 2, "2026-10-27T10:00:00Z");
        draft.pr.is_draft = true;
        let mut snap = app.snapshot.clone();
        snap.reviews = Some(vec![review("r1", 1, "2026-10-26T10:00:00Z"), draft]);
        app.apply(snap.clone());
        // Drafts shown now: #2 was already waiting, so it isn't new.
        settings.review_default_group.draft = "any".into();
        app.settings = settings;
        app.apply(snap.clone());
        assert!(app.review_alerts.is_empty(), "{:?}", app.review_alerts);
        // A review window leaves the announcing to the dashboard.
        app.reviews_window = true;
        snap.reviews.as_mut().unwrap().push(review("r1", 3, "2026-10-28T10:00:00Z"));
        app.apply(snap);
        assert!(app.review_alerts.is_empty());
    }

    /// `fixture()` with three PRs waiting on review: one already pulled
    /// (head `feature/refactor`, worktree w1), one from a fork, one in the
    /// second repo with a Linear ticket and a team request.
    fn with_reviews() -> App {
        use crate::reviews_ui::tests::review;
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        snap.repos = vec![
            Repository { id: "r1".into(), path: "/r1".into(), name: "agent-monitor".into() },
            Repository { id: "r2".into(), path: "/r2".into(), name: "gridsight".into() },
        ];
        let mut pulled = review("r1", 51, "2026-10-26T10:00:00Z");
        pulled.pr.title = "Batch the PR queries".into();
        pulled.head_ref_name = "feature/refactor".into();
        pulled.worktree_id = Some("w1".into());
        let mut fork = review("r1", 48, "2026-10-25T09:00:00Z");
        fork.pr.title = "Typo in README".into();
        fork.is_cross_repository = true;
        fork.author = "outsider".into();
        fork.requested = vec!["acme/docs".into()];
        let mut ticket = review("r2", 902, "2026-10-26T11:00:00Z");
        ticket.pr.title = "Feeder load forecast v2".into();
        ticket.pr.checks_status = ChecksStatus::Failing;
        ticket.requested = vec!["you".into(), "gridsight/platform".into()];
        ticket.linear = Some(LinearInfo {
            identifier: "ENG-77".into(),
            title: "Forecast".into(),
            url: "https://linear.app/t/issue/ENG-77".into(),
            state: LinearState { name: "In Review".into(), color: String::new(), kind: "started".into() },
            priority_label: String::new(),
            assignee: None,
            project: None,
            pr_attachment: None,
            parent: None,
        });
        snap.reviews = Some(vec![fork, ticket, pulled]);
        app.apply(snap);
        app
    }

    /// [`with_reviews`] with the page in repo order.
    fn by_repo() -> App {
        let mut app = with_reviews();
        app.settings.review_default_group.sort = "repo".into();
        app
    }

    #[test]
    fn review_badge_on_the_worktrees_border_opens_the_page() {
        let mut app = with_reviews();
        let screen = render(&app, 120, 10);
        let top = screen.lines().next().unwrap();
        assert!(top.starts_with("╭─am v0.0.0 · Worktrees ─") && top.ends_with(" 3 to review ╮"), "{screen}");
        // Beside an update notice: the notice, then the badge in the corner.
        let mut both = with_reviews();
        both.update_available = Some(update("2.2.0"));
        let top = render(&both, 120, 6).lines().next().unwrap().to_string();
        assert!(top.ends_with(" ⬆ v2.2.0 available  3 to review ╮"), "{top}");
        let at = find(&both, 120, 6, "3 to review");
        assert_eq!(target_at(&both, 120, 6, at), Some(Target::Command(Command::ReviewRequests)));
        insta::assert_snapshot!(screen);
        let at = find(&app, 120, 10, "3 to review");
        assert_eq!(target_at(&app, 120, 10, at), Some(Target::Command(Command::ReviewRequests)));
        assert_eq!(click(&mut app, 120, 10, at, false), crate::app::Action::RefreshReviews);
        assert!(app.reviews.is_some());
        // Narrow: a short badge; none at all when nothing waits.
        let app = with_reviews();
        assert!(render(&app, 50, 6).contains(" ⚑3 "));
        let mut empty = with_reviews();
        let mut snap = empty.snapshot.clone();
        snap.reviews = Some(vec![]);
        empty.apply(snap);
        assert!(!render(&empty, 120, 6).contains("to review"));
    }

    fn groups(json: &str) -> Vec<am_core::review_groups::ReviewGroup> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn review_badge_lists_the_groups_shown_on_the_dashboard() {
        let top = |app: &App, w: u16| render(app, w, 6).lines().next().unwrap().to_string();
        let mut app = with_reviews();
        // The fixture: 3 PRs, 2 asking you by name, 1 by @outsider.
        app.settings.review_groups = groups(r#"[{"name": "Outside", "authors": ["outsider"]}, {"name": "Quiet", "badge": false}]"#);
        // Quiet (no badge) takes the other two; the default, left with none,
        // drops out of the badge.
        assert!(top(&app, 120).ends_with(" Outside 1 ╮"), "{}", top(&app, 120));
        app.settings.review_groups[1].badge = true;
        app.settings.review_default_group.name = "Rest".into();
        assert!(top(&app, 120).ends_with(" Outside 1 · Quiet 2 ╮"), "{}", top(&app, 120));
        // The GitHub total reads ? until a search has counted it.
        app.settings.review_groups.truncate(1);
        app.settings.review_default_group.name.clear();
        app.settings.review_badge_total = true;
        assert!(top(&app, 120).ends_with(" Outside 1 · 2 to review · ? on GitHub ╮"), "{}", top(&app, 120));
        let mut snap = app.snapshot.clone();
        snap.review_total = Some(40);
        app.apply(snap);
        assert!(top(&app, 120).ends_with(" Outside 1 · 2 to review · 40 on GitHub ╮"), "{}", top(&app, 120));
        assert!(top(&app, 50).contains(" ⚑1·2·40 "), "{}", top(&app, 50));
        let at = find(&app, 120, 6, "Outside 1");
        assert_eq!(target_at(&app, 120, 6, at), Some(Target::Command(Command::ReviewRequests)));
        // Nothing to count: no badge.
        app.settings.review_badge_total = false;
        app.settings.review_groups[0].badge = false;
        app.settings.review_default_group.authors = vec!["nobody".into()];
        assert!(!top(&app, 120).contains("review"), "{}", top(&app, 120));
    }

    #[test]
    fn review_page_splits_into_groups_and_hides_the_rest() {
        let mut app = with_reviews();
        app.settings.review_groups = groups(r#"[{"name": "Outside", "authors": ["@Outsider"]}, {"name": "CI broken", "checks": "failing"}]"#);
        app.on_key(Key::Char('v'));
        assert_eq!(app.review_list().iter().map(|r| r.pr.number).collect::<Vec<_>>(), [48, 902, 51]);
        let screen = render(&app, 120, 12);
        assert!(screen.contains("Outside ─") && screen.contains("CI broken ─") && screen.contains("Asked you ─"), "{screen}");
        assert!(screen.contains("· 2 groups ─"), "{screen}");
        insta::assert_snapshot!("review_page_groups", screen);
        // A default that only takes PRs asking you by name hides the rest.
        app.settings.review_groups.truncate(1);
        app.settings.review_groups[0].authors = vec!["alice".into()];
        app.settings.review_default_group.asked = "you".into();
        assert_eq!(app.review_list().iter().map(|r| r.pr.number).collect::<Vec<_>>(), [902, 51]);
        assert!(render(&app, 120, 12).contains("· 1 group · asks you · 1 hidden"));
    }

    #[test]
    fn a_long_group_value_wraps_under_its_field() {
        let mut app = with_reviews();
        let many: Vec<String> = (1..=12).map(|i| format!("someone-{i}")).collect();
        app.settings.review_groups = groups(&format!(r#"[{{"name": "Lots", "authors": {}}}]"#, serde_json::json!(many)));
        app.on_key(Key::Char('v'));
        app.on_key(Key::Char('f'));
        app.on_key(Key::Down);
        app.on_key(Key::Enter); // Groups…
        app.on_key(Key::Enter); // the group's form
        let screen = render(&app, 120, 34);
        assert!(screen.contains("@someone-12"), "every author is shown:\n{screen}");
        assert!(!screen.contains('…'), "nothing truncated:\n{screen}");
        let authors = screen.lines().position(|l| l.contains("Authors")).unwrap();
        assert!(screen.lines().nth(authors + 1).unwrap().contains("@someone-"), "continues on the next line:\n{screen}");
        insta::assert_snapshot!("review_group_form_wrapped", screen);
    }

    #[test]
    fn a_group_switched_off_passes_its_prs_on() {
        let mut app = with_reviews();
        app.settings.review_groups = groups(r#"[{"name": "Outside", "authors": ["outsider"], "enabled": false}]"#);
        app.on_key(Key::Char('v'));
        let screen = render(&app, 120, 12);
        assert!(!screen.contains("Outside ─") && screen.contains("Typo in README"), "{screen}");
        assert!(!render(&app, 120, 6).lines().next().unwrap().contains("Outside"), "nor on the badge");
        // On the groups page it reads off; space switches it back on.
        app.on_key(Key::Char('f'));
        app.on_key(Key::Down);
        app.on_key(Key::Enter);
        let screen = render(&app, 120, 30);
        assert!(screen.contains("off · by @outsider"), "{screen}");
        let g = app.settings_editor.as_mut().unwrap().groups.as_mut().unwrap();
        g.groups[0].enabled = true;
        g.cursor = 1;
        g.default.enabled = false;
        app.on_key(Key::Char('j')); // refreshes the preview
        // Only Outside left: the rest (default off) is hidden.
        assert_eq!(app.review_list().iter().map(|r| r.pr.number).collect::<Vec<_>>(), [48]);
        assert!(render(&app, 120, 30).contains("Off: everything no group took is hidden"));
    }

    #[test]
    fn the_page_previews_a_group_while_it_is_typed() {
        let mut app = with_reviews();
        let mut snap = app.snapshot.clone();
        snap.review_people = vec![am_core::model::ReviewPerson { login: "outsider".into(), name: String::new() }];
        app.apply(snap);
        app.settings.review_groups = groups(r#"[{"name": "Outside"}]"#);
        app.on_key(Key::Char('v'));
        app.on_key(Key::Char('f'));
        app.on_key(Key::Down);
        app.on_key(Key::Enter); // Groups…
        app.on_key(Key::Enter); // Outside's form
        app.settings_editor.as_mut().unwrap().groups.as_mut().unwrap().form.as_mut().unwrap().cursor = 3; // Authors
        app.on_key(Key::Enter);
        // Before anything is typed the group takes everything; then only what's typed.
        assert_eq!(app.review_list().len(), 3);
        for c in "outsider".chars() {
            app.on_key(Key::Char(c));
        }
        assert_eq!(app.review_list().iter().map(|r| r.pr.number).collect::<Vec<_>>(), [48, 902, 51], "previewed, not saved");
        assert!(app.settings.review_groups[0].authors.is_empty());
        let screen = render(&app, 120, 40);
        assert!(screen.lines().nth(1).unwrap().contains("Outside ─"), "the box sits low, the groups show above:\n{screen}");
        let counts = &app.settings_editor.as_ref().unwrap().group_counts;
        assert_eq!(counts.per_group, [1, 2], "the groups page counts the preview too");
        // A name that matches nobody is flagged once it's left.
        for _ in 0..8 {
            app.on_key(Key::Backspace);
        }
        for c in "con".chars() {
            app.on_key(Key::Char(c));
        }
        app.on_key(Key::Esc);
        app.settings_editor.as_mut().unwrap().groups.as_mut().unwrap().groups[0].authors = vec!["con".into()];
        let screen = render(&app, 120, 40);
        assert!(screen.contains("⚠ author @con matches nothing in the review list"), "{screen}");
        // Leaving the page drops the preview.
        app.on_key(Key::Esc);
        app.on_key(Key::Esc);
        assert!(app.group_preview.is_none());
    }

    #[test]
    fn review_groups_page_adds_a_group_with_autocomplete() {
        use am_core::model::ReviewPerson;
        let mut app = with_reviews();
        let mut snap = app.snapshot.clone();
        snap.review_people = vec![
            ReviewPerson { login: "alice".into(), name: "Alice Liddell".into() },
            ReviewPerson { login: "olga".into(), name: String::new() },
            ReviewPerson { login: "outsider".into(), name: String::new() },
        ];
        app.apply(snap);
        app.settings.review_groups = groups(r#"[{"name": "Outside", "authors": ["outsider"]}]"#);
        app.on_key(Key::Char('v'));
        app.on_key(Key::Char('f'));
        let field = |app: &App| app.settings_editor.as_ref().unwrap().field().key;
        while field(&app) != "reviewGroups" {
            app.on_key(Key::Down);
        }
        assert_eq!(app.on_key(Key::Enter), crate::app::Action::LoadReviewPeople);
        let screen = render(&app, 120, 30);
        assert!(screen.contains("Review groups — a add") && screen.contains("by @outsider") && screen.contains("everything left"), "{screen}");
        insta::assert_snapshot!("review_groups_page", screen);
        // ⏎ opens the group's form; Authors suggests people, a click takes one.
        let ed = app.settings_editor.as_mut().unwrap();
        let g = ed.groups.as_mut().unwrap();
        g.open_for_test(0);
        g.form.as_mut().unwrap().cursor = 3; // Authors
        g.form.as_mut().unwrap().editing = Some("outsider, o".into());
        let screen = render(&app, 120, 30);
        assert!(screen.contains("⇥ olga"), "{screen}");
        assert!(screen.contains("⇥ complete"), "{screen}");
        insta::assert_snapshot!("review_group_form_suggestions", screen);
        let at = find(&app, 120, 30, "olga");
        assert_eq!(click(&mut app, 120, 30, at, false), crate::app::Action::None);
        let form = app.settings_editor.as_ref().unwrap().groups.as_ref().unwrap().form.clone().unwrap();
        assert_eq!(form.editing.as_deref(), Some("outsider, olga, "));
    }

    #[test]
    fn reviewers_read_as_the_first_one_and_a_count() {
        use crate::reviews_ui::tests::review_asking;
        let mut r = review_asking("a", 1, "x", "acme/grid-vis-squad");
        assert_eq!(reviewers_text(&r), "@grid-vis-squad");
        r.requested = vec!["acme/platform".into(), "you".into(), "bob".into()];
        assert_eq!(reviewers_text(&r), "you +2", "you first, whatever the order");
        r.requested = vec!["acme/data".into(), "bob".into()];
        assert_eq!(reviewers_text(&r), "@data +1");
        r.requested.clear();
        assert_eq!(reviewers_text(&r), "");
    }

    #[test]
    fn review_rows_keep_the_author_whole_and_cut_the_title() {
        let mut app = with_reviews();
        let mut snap = app.snapshot.clone();
        let r = &mut snap.reviews.as_mut().unwrap()[1];
        r.pr.title = "Feeder load forecast v2 with a very long title that keeps going and going on and on".into();
        r.author = "aiden-sobey-gridsight".into();
        app.apply(snap);
        app.on_key(Key::Char('v'));
        let screen = render(&app, 100, 10);
        let row = screen.lines().find(|l| l.contains("Feeder load")).unwrap();
        assert!(row.contains("@aiden-sobey-gridsight"), "{row}");
        assert!(row.contains("…  @aiden-sobey-gridsight"), "the title gives way: {row}");
    }

    #[test]
    fn review_page_groups_prs_by_repo() {
        let mut app = by_repo();
        assert_eq!(app.on_key(Key::Char('v')), crate::app::Action::RefreshReviews);
        insta::assert_snapshot!("review_page_wide", render(&app, 120, 10));
        insta::assert_snapshot!("review_page_narrow", render(&app, 80, 10));
    }

    #[test]
    fn review_page_keys_open_pull_and_go_back() {
        use crate::app::Action;
        let mut app = by_repo();
        app.on_key(Key::Char('v'));
        // Page order: agent-monitor newest first (#51 pulled, #48 fork), then gridsight #902.
        assert_eq!(app.selected_review().unwrap().pr.number, 51);
        // #51 is pulled: ⏎ opens its worktree like on the dashboard, g the PR.
        let Action::Open(v) = app.on_key(Key::Enter) else { panic!("expected an open") };
        assert_eq!(v.worktree.id, "w1");
        assert_eq!(app.on_key(Key::Char('g')), Action::OpenUrl("https://github.com/acme/r1/pull/51".into()));
        let bar = render(&app, 120, 10).lines().last().unwrap().to_string();
        assert!(bar.contains("⏎ open  g PR"), "{bar}");
        assert_eq!(app.on_key(Key::Char('l')), Action::None);
        assert_eq!(app.notice.as_deref(), Some("No Linear ticket for this PR"));
        app.on_key(Key::Char('j'));
        assert_eq!(app.on_key(Key::Char('t')), Action::None, "a fork can't be pulled");
        assert!(app.notice.as_deref().unwrap().contains("fork"));
        assert!(!app.review_key_available(Key::Char('t')));
        // A fork's ⏎ opens the PR instead.
        assert_eq!(app.on_key(Key::Enter), Action::OpenUrl("https://github.com/acme/r1/pull/48".into()));
        assert!(render(&app, 120, 10).lines().last().unwrap().contains("⏎ open PR"));
        app.on_key(Key::Down);
        assert_eq!(app.on_key(Key::Char('l')), Action::OpenUrl("https://linear.app/t/issue/ENG-77".into()));
        // Not pulled: ⏎ pulls it, like t.
        let bar = render(&app, 120, 10).lines().last().unwrap().to_string();
        assert!(bar.contains("⏎ pull  g PR"), "{bar}");
        assert_eq!(app.on_key(Key::Char('r')), Action::ReloadReviews, "r clears the review caches");
        let Action::Create(_, repo, req) = app.on_key(Key::Enter) else { panic!("expected a create") };
        assert_eq!(repo.id, "r2");
        assert_eq!(req.branch, "feature/902");
        assert!(req.tmp_space);
        assert_eq!(req.mode, am_core::worktrees::CreateMode::TrackRemote);
        assert!(app.reviews.is_some(), "stays on the page to pull more");
        assert!(app.rows.iter().any(|r| matches!(r, crate::app::Row::Pending(_))));
        assert!(render(&app, 120, 10).contains("pulling…"));
        assert!(!app.review_key_available(Key::Char('t')));
        assert_eq!(app.on_key(Key::Char('t')), Action::None, "already pulling");
        assert!(app.notice.as_deref().unwrap().contains("already being pulled"));
        app.notice = None;
        assert_eq!(app.on_key(Key::Enter), Action::None, "⏎ doesn't pull twice");
        assert!(app.notice.as_deref().unwrap().contains("already being pulled"));
        // A failed pull says so on the row, and t tries again.
        let job = app.jobs.list.last().unwrap().id;
        app.jobs.finish(job, Err("no such ref".into()), std::time::Instant::now());
        assert!(render(&app, 120, 10).contains("✗ pull failed"));
        assert!(matches!(app.on_key(Key::Char('t')), Action::Create(..)));
        assert_eq!(app.jobs.list.len(), 1, "the failed job made way for the retry");

        // Already pulled: jump to the worktree instead.
        let mut app = by_repo();
        app.on_key(Key::End);
        app.on_key(Key::Char('v'));
        assert_eq!(app.on_key(Key::Char('t')), Action::None);
        assert!(app.reviews.is_none());
        assert_eq!(app.selected_view().unwrap().worktree.id, "w1");

        // d removes a pulled worktree without leaving the page.
        let mut app = by_repo();
        app.on_key(Key::Char('v'));
        assert!(app.review_key_available(Key::Char('d')));
        assert_eq!(app.on_key(Key::Char('d')), Action::None);
        assert!(matches!(app.modal, Some(crate::modals::Modal::Delete(_))));
        let mut act = app.on_key(Key::Char('y'));
        if app.modal.is_some() {
            act = app.on_key(Key::Char('n'));
        }
        let Action::Delete(_, view, _) = act else { panic!("expected a delete, got {act:?}") };
        assert_eq!(view.worktree.id, "w1");
        assert!(app.reviews.is_some(), "still on the page");
        assert!(render(&app, 120, 10).contains("removing…"));
        assert!(!app.review_key_available(Key::Char('d')));
        app.on_key(Key::Char('j'));
        assert!(!app.review_key_available(Key::Char('d')), "not pulled");
        assert_eq!(app.on_key(Key::Char('d')), Action::None);
        assert!(app.modal.is_none());

        // Esc and q go back; v toggles.
        let mut app = by_repo();
        for k in [Key::Esc, Key::Char('q'), Key::Char('v')] {
            app.on_key(Key::Char('v'));
            assert!(app.reviews.is_some());
            assert_eq!(app.on_key(k), Action::None);
            assert!(app.reviews.is_none(), "{k:?}");
        }
    }

    #[test]
    fn pull_and_run_script_pulls_then_shows_the_review_script() {
        use crate::app::Action;
        use am_core::worktrees::ScriptKind;
        let (_guard, _) = crate::settings_ui::tests::data_dir();
        // A repo id of its own, so other tests rendering r2 never see its script.
        const ID: &str = "r2-review-script";
        let _ = ScriptKind::Review.remove(ID);
        let mut app = by_repo();
        let mut snap = app.snapshot.clone();
        snap.repos.iter_mut().filter(|r| r.id == "r2").for_each(|r| r.id = ID.into());
        snap.reviews.iter_mut().flatten().filter(|r| r.repo_id == "r2").for_each(|r| r.repo_id = ID.into());
        app.apply(snap);
        app.on_key(Key::Char('v'));
        // #51 is pulled: refused, and the page stays put.
        assert!(!app.review_key_available(Key::Char('T')));
        assert_eq!(app.on_key(Key::Char('T')), Action::None);
        assert!(app.notice.as_deref().unwrap().contains("already pulled"), "{:?}", app.notice);
        assert!(app.reviews.is_some());
        // #902 (repo r2, renamed) without a review script: T asks to add one;
        // anything but y/⏎ cancels.
        app.on_key(Key::End);
        assert_eq!(app.selected_review().unwrap().pr.number, 902);
        assert!(app.review_key_available(Key::Char('T')), "available without a script too");
        assert_eq!(app.on_key(Key::Char('T')), Action::None);
        assert!(matches!(app.modal, Some(crate::modals::Modal::AddReviewScript { .. })));
        assert!(render(&app, 120, 20).contains("has no review script yet. Add one?"));
        assert_eq!(app.on_key(Key::Char('n')), Action::None);
        assert!(app.modal.is_none() && app.jobs.list.is_empty());
        // ctrl-⏎ is T on the page; y adds the script (main.rs writes and
        // opens it) and starts the pull.
        assert_eq!(app.on_key(Key::CtrlEnter), Action::None);
        assert!(matches!(app.modal, Some(crate::modals::Modal::AddReviewScript { .. })));
        let Action::AddReviewScriptAndPull(job, repo, _) = app.on_key(Key::Char('y')) else { panic!("expected an add + pull") };
        assert_eq!(repo.id, ID);
        assert!(app.jobs.get(job).unwrap().review_script().is_some());
        app.jobs.dismiss(job);
        // With a script: straight to a tmp-space pull whose job carries the PR.
        ScriptKind::Review.create(ID).unwrap();
        assert!(app.review_key_available(Key::Char('T')));
        app.notice = None;
        let bar = render(&app, 160, 10).lines().last().unwrap().to_string();
        assert!(bar.contains("T pull + run script"), "{bar}");
        let Action::Create(job, repo, req) = app.on_key(Key::Char('T')) else { panic!("expected a create") };
        assert_eq!((repo.id.as_str(), req.branch.as_str(), req.tmp_space), (ID, "feature/902", true));
        let env = app.jobs.get(job).unwrap().review_script().unwrap().to_vec();
        assert!(env.contains(&("AM_PR_NUMBER".into(), "902".into())), "{env:?}");
        assert!(env.iter().any(|(k, _)| k == "AM_PR_URL"));
        assert!(render(&app, 120, 10).contains("pulling…"));
        assert!(!app.review_key_available(Key::Char('T')), "already pulling");
        // Pulled: the row follows the script, then its failure.
        app.jobs.set_worktree(job, "w902".into());
        let mut snap = app.snapshot.clone();
        for r in snap.reviews.iter_mut().flatten().filter(|r| r.pr.number == 902) {
            r.worktree_id = Some("w902".into());
        }
        app.apply(snap);
        app.jobs.step(job, "Running review script".into());
        app.jobs.log(job, "reviewing #902".into());
        let screen = render(&app, 140, 10);
        assert!(screen.contains("running script… reviewing #902"), "{screen}");
        // Waiting for a template to be saved says so on the row.
        app.jobs.ask(job, crate::jobs::Question::SaveReviewScript { repo_id: ID.into(), worktree_path: "/w902".into() });
        assert!(render(&app, 140, 10).contains("save the review script to run it"));
        app.jobs.resume(job);
        app.jobs.finish(job, Err("Script exited with exit status: 1".into()), std::time::Instant::now());
        assert!(render(&app, 140, 10).contains("✗ script failed"));
        app.jobs.dismiss(job);
        let screen = render(&app, 140, 10);
        assert!(!screen.contains("running script") && !screen.contains("script failed"), "{screen}");
        ScriptKind::Review.remove(ID).unwrap();
    }

    #[test]
    fn ctrl_enter_is_enter_off_the_review_page() {
        let mut app = fixture();
        assert_eq!(app.on_key(Key::CtrlEnter), app.on_key(Key::Enter));
    }

    #[test]
    fn keys_listed_as_doing_nothing_on_the_review_page_do_nothing_there() {
        use crate::keymap::Command;
        for cmd in Command::ALL.into_iter().filter(|c| c.on_review_page().is_none()) {
            let mut app = by_repo();
            app.on_key(Key::Char('v'));
            let before = render(&app, 120, 20);
            let key = app.keymap.key(cmd).or_else(|| cmd.fixed_keys().first().copied()).unwrap();
            assert_eq!(app.on_key(key), crate::app::Action::None, "{cmd:?}");
            assert!(app.reviews.is_some() && app.modal.is_none() && app.settings_editor.is_none(), "{cmd:?}");
            assert_eq!(render(&app, 120, 20), before, "{cmd:?}");
        }
    }

    #[test]
    fn review_split_sits_beside_the_list_and_tab_moves_focus() {
        use crate::app::Action;
        let mut app = by_repo();
        assert_eq!(app.on_key(Key::Char('V')), Action::RefreshReviews);
        assert!(app.review_focused());
        let wide = render(&app, 120, 10);
        assert!(wide.contains("Worktrees") && wide.contains("Review requests"), "{wide}");
        insta::assert_snapshot!("review_split_wide", wide);
        insta::assert_snapshot!("review_split_narrow", render(&app, 80, 16));
        // Keys go to the review side: j moves its cursor, not the list's.
        let selected = app.selected;
        app.on_key(Key::Char('j'));
        assert_eq!(app.reviews.as_ref().unwrap().cursor, 1);
        assert_eq!(app.selected, selected);
        // ⇥ hands the keys to the list (the split stays), and back.
        app.on_key(Key::Tab);
        assert!(!app.review_focused() && app.reviews.is_some());
        app.on_key(Key::Char('j'));
        assert_eq!(app.reviews.as_ref().unwrap().cursor, 1);
        assert!(render(&app, 120, 10).lines().last().unwrap().contains("⇥ reviews"));
        app.on_key(Key::Tab);
        assert!(app.review_focused());
        // v takes the whole body; V splits again; esc closes.
        app.on_key(Key::Char('v'));
        assert!(app.reviews.as_ref().is_some_and(|p| !p.split));
        app.on_key(Key::Char('V'));
        assert!(app.reviews.as_ref().is_some_and(|p| p.split && p.focused));
        app.on_key(Key::Esc);
        assert!(app.reviews.is_none());
        // V from the list closes the split too.
        app.on_key(Key::Char('V'));
        app.on_key(Key::Tab);
        app.on_key(Key::Char('V'));
        assert!(app.reviews.is_none());
    }

    #[test]
    fn review_split_keeps_open_when_a_pulled_pr_is_shown_in_the_list() {
        let mut app = by_repo();
        app.on_key(Key::End);
        app.on_key(Key::Char('V'));
        app.on_key(Key::Char('t'));
        assert!(app.reviews.as_ref().is_some_and(|p| p.split), "the split stays");
        assert!(!app.review_focused(), "the list takes the keys");
        assert_eq!(app.selected_view().unwrap().worktree.id, "w1");
    }

    #[test]
    fn review_split_puts_the_jobs_pane_under_the_page() {
        let mut app = by_repo();
        app.on_key(Key::Char('V'));
        app.on_key(Key::Down);
        app.on_key(Key::Down);
        assert!(matches!(app.on_key(Key::Enter), crate::app::Action::Create(..)));
        insta::assert_snapshot!("review_split_jobs", render(&app, 120, 16));
        // ⇥: reviews → jobs → list → reviews.
        app.on_key(Key::Tab);
        assert!(app.jobs.focused && !app.review_focused());
        app.on_key(Key::Tab);
        assert!(!app.jobs.focused && !app.review_focused());
        app.on_key(Key::Tab);
        assert!(app.review_focused());
    }

    #[test]
    fn the_full_review_page_shows_the_jobs_pane_only_for_a_question() {
        let mut app = by_repo();
        app.on_key(Key::Char('v'));
        app.on_key(Key::Down);
        app.on_key(Key::Down);
        let crate::app::Action::Create(job, ..) = app.on_key(Key::Enter) else { panic!("expected a create") };
        // Pulling (or removing) shows on the row, not in a pane.
        let screen = render(&app, 120, 16);
        assert!(screen.contains("─Review requests") && screen.contains("pulling…") && !screen.contains("─Jobs"), "{screen}");
        assert!(!screen.lines().last().unwrap().contains("⇥"), "{screen}");
        app.on_key(Key::Tab);
        assert!(!app.jobs.focused && app.review_focused(), "nothing to tab to");
        // A question can only be answered in the pane, so then it shows.
        app.jobs.ask(job, crate::jobs::Question::BranchExists { local: true, remote: true });
        let screen = render(&app, 120, 16);
        assert!(screen.contains("─Jobs") && screen.lines().last().unwrap().contains("⇥ answer"), "{screen}");
        insta::assert_snapshot!("review_page_jobs", screen);
        // ⇥: page → jobs → page.
        app.on_key(Key::Tab);
        assert!(app.jobs.focused && !app.review_focused());
        assert!(render(&app, 120, 16).contains("back to reviews"));
        app.on_key(Key::Tab);
        assert!(!app.jobs.focused && app.review_focused());
        // Help and Settings leave it in view.
        app.on_key(Key::Char('h'));
        assert!(render(&app, 120, 30).contains("─Jobs"));
        app.on_key(Key::Esc);
        app.on_key(Key::Char('s'));
        assert!(app.settings_editor.is_some());
        let screen = render(&app, 120, 30);
        assert!(screen.contains("Settings") && screen.contains("─Jobs"), "{screen}");
        app.on_key(Key::Esc);
        // Answered: the pane goes, and the keys come back to the page.
        app.on_key(Key::Tab);
        assert!(app.jobs.focused);
        assert!(matches!(app.on_key(Key::Char('p')), crate::app::Action::Create(..)));
        assert!(!app.jobs.focused && app.review_focused());
        assert!(!render(&app, 120, 16).contains("─Jobs"));
        // On the dashboard the running job still shows its pane.
        app.on_key(Key::Esc);
        assert!(render(&app, 120, 16).contains("─Jobs"));
    }

    #[test]
    fn ctrl_v_opens_the_review_page_in_a_new_window() {
        use crate::app::Action;
        let mut app = by_repo();
        assert_eq!(app.on_key(Key::CtrlV), Action::OpenReviewWindow);
        app.settings.review_requests_enabled = false;
        assert_eq!(app.on_key(Key::CtrlV), Action::None);
        assert!(app.notice.as_deref().unwrap().contains("Review requests are off"));
    }

    #[test]
    fn a_review_only_window_quits_when_the_page_closes() {
        use crate::app::Action;
        let mut app = by_repo();
        app.reviews_window = true;
        app.on_key(Key::Char('v'));
        assert!(render(&app, 120, 10).lines().last().unwrap().contains("esc quit"));
        assert_eq!(app.on_key(Key::Char('V')), Action::None, "no split in a review-only window");
        assert!(app.reviews.as_ref().is_some_and(|p| !p.split));
        assert_eq!(app.on_key(Key::CtrlV), Action::None);
        // An already-pulled PR (#51, first) stays on the page: there's no list to show.
        app.on_key(Key::Home);
        assert_eq!(app.on_key(Key::Char('t')), Action::None);
        assert!(app.reviews.is_some());
        assert_eq!(app.on_key(Key::Esc), Action::Quit);
    }

    #[test]
    fn review_only_mode_shows_the_page_alone_with_its_own_settings_and_help() {
        use crate::app::Action;
        let mut app = by_repo();
        app.settings.app_mode = "reviews".into();
        app.update_available = Some(update("2.2.0"));
        app.enter_reviews_mode();
        assert!(std::mem::take(&mut app.review_fetch_due), "opening asks for a fetch");
        let screen = render(&app, 120, 14);
        // Review-only, the page names the tool.
        assert!(screen.contains("─am v0.0.0 · Review requests") && !screen.contains("Worktrees"), "{screen}");
        // No ─Worktrees border to carry it: the update notice is on the page's.
        assert!(screen.lines().next().unwrap().contains("⬆ v2.2.0 available"), "{screen}");
        assert!(screen.lines().last().unwrap().contains("esc quit"));
        insta::assert_snapshot!("review_only_mode_page", screen);
        // Help: the page's keys, nothing about worktrees or agents.
        app.on_key(Key::Char('h'));
        let help = render(&app, 120, 30);
        assert!(help.contains("pull into Reviewing") && help.contains("quit"), "{help}");
        assert!(!help.contains("delete worktree (asks first)") && !help.contains("✦ gemini"), "{help}");
        insta::assert_snapshot!("review_only_mode_help", help);
        app.on_key(Key::Esc);
        // Settings: the page's settings only, still under section headers.
        app.on_key(Key::Char('s'));
        let settings = render(&app, 120, 50);
        assert!(settings.contains("Mode") && settings.contains("Review requests") && settings.contains("Requested from"), "{settings}");
        assert!(!settings.contains("Sort order") && !settings.contains("Harness plugins") && !settings.contains("Fetch PRs"), "{settings}");
        insta::assert_snapshot!("review_only_mode_settings", settings);
        app.on_key(Key::Esc);
        assert_eq!(app.on_key(Key::Esc), Action::Quit);
    }

    #[test]
    fn switching_the_mode_live_opens_the_page_or_brings_the_dashboard_back() {
        use crate::app::Action;
        let mut app = by_repo();
        let mut s = app.settings.clone();
        s.app_mode = "reviews".into();
        app.set_settings(s.clone());
        assert!(app.reviews.as_ref().is_some_and(|p| !p.split) && app.review_fetch_due);
        assert!(app.reviews_only());
        // Back to full: the page stays, and leaving it shows the dashboard.
        s.app_mode = "full".into();
        app.set_settings(s);
        assert!(app.reviews.is_some() && !app.reviews_only());
        assert_eq!(app.on_key(Key::Esc), Action::None);
        assert!(app.reviews.is_none());
    }

    #[test]
    fn review_only_mode_with_review_requests_off_says_how_to_turn_them_on() {
        let mut app = fixture();
        app.settings.app_mode = "reviews".into();
        app.settings.gh_pr_status = false;
        app.enter_reviews_mode();
        assert!(!app.review_fetch_due, "nothing to fetch");
        let screen = render(&app, 120, 10);
        assert!(screen.contains("Review requests are off") && screen.contains("Run setup again"), "{screen}");
        assert!(!screen.contains("Worktrees"), "never the dashboard: {screen}");
    }

    #[test]
    fn setup_starts_by_asking_for_the_mode() {
        let mut app = fixture();
        let mut s = crate::setup::Setup::new(vec![], &Settings::parse("{}"));
        s.hook_binary = None;
        app.setup = Some(s);
        let full = render(&app, 100, 24);
        assert!(full.contains("● Agents & worktrees") && full.contains("○ Review requests only"), "{full}");
        assert!(full.contains("Mode › Editor › Hooks › Repos › Integrations › Reviews › Context"), "{full}");
        insta::assert_snapshot!("setup_mode_step", full);
        app.on_key(Key::Down);
        let reviews = render(&app, 100, 24);
        assert!(reviews.contains("Mode › Editor › Repos › GitHub › Reviews") && !reviews.contains("Hooks"), "{reviews}");
    }

    #[test]
    fn holding_shift_or_ctrl_renames_the_review_hint() {
        let mut app = by_repo();
        let bar = |app: &App| render(app, 200, 10).lines().last().unwrap().to_string();
        assert!(bar(&app).contains("v reviews"));
        app.held.shift = true;
        let shifted = bar(&app);
        assert!(shifted.contains("V reviews beside list") && !shifted.contains("v reviews"), "{shifted}");
        app.held.ctrl = true;
        let ctrl = bar(&app);
        assert!(ctrl.contains("ctrl-v reviews in new window") && !ctrl.contains("V reviews beside list"), "{ctrl}");
        app.held = crate::app::Held::default();
        assert!(bar(&app).contains("v reviews"));
    }

    #[test]
    fn held_keys_show_their_variants_where_the_terminal_reports_them() {
        let bar = |app: &App, w| render(app, w, 10).lines().last().unwrap().to_string();
        // Dashboard: V lives behind Shift, so it's off the plain bar.
        let mut app = by_repo();
        app.modifiers_reported = true;
        let plain = bar(&app, 200);
        assert!(plain.contains("v reviews") && !plain.contains("V split") && plain.contains("⇧/ctrl more"), "{plain}");
        app.held.shift = true;
        assert!(bar(&app, 200).contains("V reviews beside list"));
        app.held = crate::app::Held::default();
        // Review page: ⏎ says what it does; Shift and Ctrl add the script.
        app.on_key(Key::Char('v'));
        app.on_key(Key::End);
        let plain = bar(&app, 200);
        assert!(plain.contains("⏎ pull") && !plain.contains("pull into Reviewing") && !plain.contains("T pull"), "{plain}");
        app.held.shift = true;
        let shifted = bar(&app, 200);
        assert!(shifted.contains("T pull + run script") && shifted.contains("V beside list") && !shifted.contains("⏎ pull"), "{shifted}");
        app.held = crate::app::Held { ctrl: true, ..Default::default() };
        let ctrl = bar(&app, 200);
        assert!(ctrl.contains("ctrl-⏎ pull + run script") && ctrl.contains("ctrl-v new window"), "{ctrl}");
        // Clicking the held hint runs it: ctrl-⏎ asks to add a review script here.
        let at = find(&app, 200, 10, "ctrl-⏎");
        click(&mut app, 200, 10, at, false);
        assert!(matches!(app.modal, Some(crate::modals::Modal::AddReviewScript { .. })), "the fixture repo has no review script");
        // Without the protocol, T stays on the plain bar.
        let mut app = by_repo();
        app.on_key(Key::Char('v'));
        assert!(bar(&app, 200).contains("T pull + run script"));
    }

    #[test]
    fn shift_and_ctrl_swap_the_dashboard_bar() {
        let bar = |app: &App| render(app, 200, 10).lines().last().unwrap().to_string();
        let mut app = by_repo();
        app.modifiers_reported = true;
        let plain = bar(&app);
        for gone in ["F filters", "A add repo", "V reviews", "ctrl-v"] {
            assert!(!plain.contains(gone), "{gone}: {plain}");
        }
        assert!(plain.contains("f filter") && plain.contains("⇧/ctrl more"), "{plain}");
        app.held.shift = true;
        let shifted = bar(&app);
        assert!(shifted.contains("F filters off") && shifted.contains("A add repo") && !shifted.contains("n new"), "{shifted}");
        assert!(shifted.contains("h help") && shifted.contains("q quit"), "{shifted}");
        app.settings.worktree_filters_enabled = false;
        assert!(bar(&app).contains("F filters on"));
        app.held = crate::app::Held { ctrl: true, ..Default::default() };
        assert!(bar(&app).contains("ctrl-v reviews in new window"));
        // Without the protocol everything shares the plain bar.
        app.modifiers_reported = false;
        app.held = crate::app::Held::default();
        let all = render(&app, 260, 10).lines().last().unwrap().to_string();
        assert!(all.contains("n new") && all.contains("F filters on") && !all.contains("more"), "{all}");
    }

    #[test]
    fn delete_and_backspace_delete_like_d() {
        for key in [Key::Delete, Key::Backspace] {
            let mut app = by_repo();
            app.on_key(Key::Char('j'));
            app.on_key(key);
            assert!(matches!(app.modal, Some(crate::modals::Modal::Delete(_))), "{key:?}");
        }
    }

    #[test]
    fn page_title_names_the_tool_and_shortens() {
        assert_eq!(page_title_text("Worktrees", 40), "am v0.0.0 · Worktrees");
        assert_eq!(page_title_text("Worktrees", 18), "am · Worktrees");
        assert_eq!(page_title_text("Worktrees", 10), "Worktrees");
        let app = by_repo();
        assert!(render(&app, 120, 10).starts_with("╭─am v0.0.0 · Worktrees ─"));
    }

    #[test]
    fn logs_open_from_settings_not_l() {
        use crate::app::Action;
        let mut app = fixture();
        assert_eq!(app.on_key(Key::Char('L')), Action::None);
        app.on_key(Key::Char('s'));
        let ed = app.settings_editor.as_mut().unwrap();
        ed.cursor = crate::settings_ui::FIELDS.iter().position(|f| f.key == "viewLogs").unwrap();
        assert!(render(&app, 120, 40).contains("View logs…"));
        assert_eq!(app.on_key(Key::Enter), Action::ViewLogs);
        assert!(app.settings_editor.is_some(), "Settings stays open");
    }

    #[test]
    fn t_on_the_dashboard_does_nothing() {
        let mut app = by_repo();
        assert_eq!(app.on_key(Key::Char('t')), crate::app::Action::None);
        assert!(app.modal.is_none());
        assert!(!render(&app, 200, 10).lines().last().unwrap().contains("pull"));
    }

    #[test]
    fn review_page_puts_prs_asking_you_first_by_default() {
        let mut app = with_reviews();
        app.on_key(Key::Char('v'));
        // Newest first among those asking you, then the team-only fork.
        assert_eq!(app.review_list().iter().map(|r| r.pr.number).collect::<Vec<_>>(), [902, 51, 48]);
        let screen = render(&app, 120, 10);
        assert!(screen.contains("Asked you ─") && screen.contains("Asked your team ─"), "{screen}");
        assert!(screen.contains("(gridsight)"), "rows name their repo outside repo order:\n{screen}");
        insta::assert_snapshot!("review_page_direct_first", screen);
        // Other orders: one list, no headers.
        for (sort, want) in [("updated", [902, 51, 48]), ("oldest", [902, 51, 48])] {
            app.settings.review_default_group.sort = sort.into();
            assert_eq!(app.review_list().iter().map(|r| r.pr.number).collect::<Vec<_>>(), want, "{sort}");
            assert!(!render(&app, 120, 10).contains("Asked you"), "{sort}");
        }
    }

    #[test]
    fn review_page_puts_approved_pulled_prs_at_the_bottom() {
        let mut app = with_reviews();
        let before = app.review_badge_parts();
        let mut snap = app.snapshot.clone();
        let r = snap.reviews.as_mut().unwrap().iter_mut().find(|r| r.pr.number == 902).unwrap();
        r.approved_by_you = true;
        r.reviewed_by_you = true;
        r.pr.review_decision = "APPROVED".into();
        r.worktree_id = Some("wt-902".into());
        app.apply(snap);
        app.on_key(Key::Char('v'));
        assert_eq!(app.review_list().iter().map(|r| r.pr.number).collect::<Vec<_>>(), [51, 48, 902]);
        let screen = render(&app, 120, 12);
        assert!(screen.contains("Approved, still pulled ─"), "{screen}");
        insta::assert_snapshot!("review_page_approved_pulled", screen);
        // Not counted in the badge as waiting on a review.
        let count = |parts: Option<Vec<crate::app::BadgePart>>| match parts.unwrap()[0] {
            crate::app::BadgePart::Rest(n) => n,
            ref p => panic!("{p:?}"),
        };
        assert_eq!(count(app.review_badge_parts()), count(before) - 1);
    }

    #[test]
    fn review_page_f_opens_the_filters() {
        let mut app = by_repo();
        app.settings.review_default_group.draft = "ready".into();
        app.settings.review_default_group.reviewed = "no".into();
        app.on_key(Key::Char('v'));
        let screen = render(&app, 120, 12);
        assert!(screen.contains("you + your teams · no drafts · not yet reviewed"), "{screen}");
        assert!(screen.lines().last().unwrap().contains("f filter"), "{screen}");
        // The summary is clickable too.
        let at = find(&app, 120, 12, "you + your teams");
        assert_eq!(target_at(&app, 120, 12, at), Some(Target::Key(Key::Char('f'))));

        assert_eq!(app.on_key(Key::Char('f')), crate::app::Action::None);
        let field = |app: &App| app.settings_editor.as_ref().unwrap().field().key;
        assert_eq!(field(&app), "reviewRequestsScope");
        let screen = render(&app, 120, 24);
        assert!(screen.contains("Review filters") && screen.contains("Groups…"), "{screen}");
        // Order and the filters are each group's now.
        assert!(!screen.contains("Order") && !screen.contains("Hide drafts") && !screen.contains("Extra search"), "{screen}");
        assert!(!screen.contains("Theme") && !screen.contains("Review refresh"), "only the filters: {screen}");
        insta::assert_snapshot!("review_filters", screen);
        // Up stops at the first filter, down at the last.
        app.on_key(Key::Up);
        assert_eq!(field(&app), "reviewRequestsScope");
        for _ in 0..10 {
            app.on_key(Key::Char('j'));
        }
        assert_eq!(field(&app), "resetReviewGroups");
        // f (or esc) closes it, back on the page.
        app.on_key(Key::Char('f'));
        assert!(app.settings_editor.is_none() && app.reviews.is_some());
    }

    #[test]
    fn review_page_shows_stacked_prs_as_a_tree() {
        let mut app = by_repo();
        let mut snap = app.snapshot.clone();
        // The fork PR targets #51's branch: it sits under #51.
        snap.reviews.as_mut().unwrap()[0].pr.base_ref_name = Some("feature/refactor".into());
        app.apply(snap);
        app.on_key(Key::Char('v'));
        let screen = render(&app, 120, 10);
        let lines: Vec<&str> = screen.lines().collect();
        let parent = lines.iter().position(|l| l.contains("Batch the PR queries")).unwrap();
        assert!(lines[parent + 1].contains("└─ Typo in README"), "{screen}");
    }

    #[test]
    fn review_page_rows_are_clickable() {
        let mut app = by_repo();
        app.on_key(Key::Char('v'));
        let at = find(&app, 120, 10, "Feeder load");
        assert_eq!(click(&mut app, 120, 10, at, false), crate::app::Action::None);
        assert_eq!(app.selected_review().unwrap().pr.number, 902);
        // A second click (or a double-click) pulls it; once pulled, it opens the worktree.
        assert!(matches!(click(&mut app, 120, 10, at, true), crate::app::Action::Create(..)));
        assert!(matches!(click(&mut app, 120, 10, at, false), crate::app::Action::None));
    }

    #[test]
    fn review_page_shows_the_badge_too() {
        let mut app = with_reviews();
        app.on_key(Key::Char('v'));
        let top = render(&app, 120, 10).lines().next().unwrap().to_string();
        assert!(top.starts_with("╭─Review requests ─") && top.ends_with(" 3 to review ╮"), "{top}");
        // Narrow: the short form; beside a stale-list warning, after it.
        assert!(render(&app, 28, 10).lines().next().unwrap().ends_with(" ⚑3 ╮"), "{}", render(&app, 28, 10));
        let mut snap = app.snapshot.clone();
        snap.review_error = Some(am_core::model::ReviewFetchError {
            message: "rate limited".into(),
            rate_limited: true,
            resets_at: None,
        });
        app.apply(snap);
        let top = render(&app, 120, 10).lines().next().unwrap().to_string();
        assert!(top.ends_with(" ⚠ GitHub rate-limited — showing cached  3 to review ╮"), "{top}");
    }

    #[test]
    fn review_page_explains_a_rate_limit_instead_of_loading() {
        let mut app = with_reviews();
        app.on_key(Key::Char('v'));
        let mut snap = app.snapshot.clone();
        snap.reviews = None;
        snap.review_error = Some(am_core::model::ReviewFetchError {
            message: "graphql: API rate limit exceeded for user ID 1.".into(),
            rate_limited: true,
            resets_at: Some(NOW + 12 * 60),
        });
        snap.health = crate::app::Health { github_failing: true, github_rate_limited: true, ..Default::default() };
        app.apply(snap.clone());
        let screen = render(&app, 100, 12);
        assert!(!screen.contains("Loading…"), "{screen}");
        assert!(screen.contains("GitHub has rate-limited you — resets in 12m"), "{screen}");
        assert!(screen.contains("⚠ GitHub rate-limited — cached"), "{screen}");
        insta::assert_snapshot!("review_page_rate_limited", screen);

        // With a list from before: the list stays, the border says why it's stale.
        let mut cached = with_reviews();
        cached.on_key(Key::Char('v'));
        let mut snap2 = cached.snapshot.clone();
        snap2.review_error = snap.review_error.clone();
        cached.apply(snap2);
        let screen = render(&cached, 120, 10);
        assert!(screen.contains("Batch the PR queries"), "{screen}");
        assert!(screen.contains("⚠ GitHub rate-limited (resets in 12m) — showing cached"), "{screen}");

        // A cached empty list doesn't claim "nothing waiting" while GitHub refuses.
        let mut empty = app.snapshot.clone();
        empty.reviews = Some(vec![]);
        app.apply(empty);
        let screen = render(&app, 100, 12);
        assert!(screen.contains("GitHub has rate-limited you") && !screen.contains("Nothing waiting"), "{screen}");

        // Any other failure: gh's own words.
        snap.review_error = Some(am_core::model::ReviewFetchError {
            message: "To get started with GitHub CLI, please run: gh auth login".into(),
            rate_limited: false,
            resets_at: None,
        });
        app.apply(snap);
        let screen = render(&app, 100, 12);
        assert!(screen.contains("Couldn't load review requests") && screen.contains("gh auth login"), "{screen}");
    }

    #[test]
    fn review_page_empty_and_off() {
        let mut app = with_reviews();
        app.on_key(Key::Char('v'));
        let mut snap = app.snapshot.clone();
        snap.reviews = Some(vec![]);
        app.apply(snap.clone());
        assert!(render(&app, 100, 8).contains("Nothing waiting on your review"));
        snap.reviews = None;
        app.apply(snap);
        assert!(render(&app, 100, 8).contains("Loading…"));
        let mut off = with_reviews();
        off.settings.review_requests_enabled = false;
        assert_eq!(off.on_key(Key::Char('v')), crate::app::Action::None);
        assert!(off.reviews.is_none());
        assert!(!render(&off, 120, 6).contains("to review"));
    }

    #[test]
    fn setup_reviews_step() {
        let mut app = fixture();
        let mut s = crate::setup::Setup::new(vec![], &Settings::default());
        s.step = crate::setup::Step::Reviews;
        app.setup = Some(s);
        insta::assert_snapshot!(render(&app, 100, 22));
        let at = find(&app, 100, 22, "Only my teams");
        click(&mut app, 100, 22, at, false);
        click(&mut app, 100, 22, at, false);
        assert_eq!(app.setup.as_ref().unwrap().review.scope, 2);
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
    fn scrolling_up_keeps_the_page_until_the_top() {
        // One repo (no header) with 10 worktrees; 7 rows leave a 4-line list.
        let mut app = fixture();
        let worktrees = (0..10).map(|i| view(&format!("w{i}"), "r1", &format!("wt-{i:02}"), None)).collect();
        let mut snapshot = app.snapshot.clone();
        snapshot.groups = vec![WorktreeGroup {
            repo: Repository { id: "r1".into(), path: "/r1".into(), name: "agent-monitor".into() },
            worktrees,
        }];
        snapshot.subagents = vec![];
        app.apply(snapshot);
        let visible = |app: &App| -> Vec<String> {
            let screen = render(app, 120, 7);
            (0..10).map(|i| format!("wt-{i:02}")).filter(|b| screen.contains(b.as_str())).collect()
        };
        let selected = |app: &App| app.selected_view().unwrap().worktree.branch.clone();

        app.on_key(Key::End);
        assert_eq!(visible(&app), ["wt-06", "wt-07", "wt-08", "wt-09"]);
        // Moving up inside the page doesn't scroll it.
        for want in ["wt-08", "wt-07", "wt-06"] {
            app.on_key(Key::Up);
            assert_eq!(selected(&app), want);
            assert_eq!(visible(&app), ["wt-06", "wt-07", "wt-08", "wt-09"]);
        }
        // Past the top row, the page follows one row at a time.
        app.on_key(Key::Up);
        assert_eq!(selected(&app), "wt-05");
        assert_eq!(visible(&app), ["wt-05", "wt-06", "wt-07", "wt-08"]);
        // And going back down stays put until the bottom row.
        for _ in 0..3 {
            app.on_key(Key::Down);
            assert_eq!(visible(&app), ["wt-05", "wt-06", "wt-07", "wt-08"]);
        }
        app.on_key(Key::Down);
        assert_eq!(visible(&app), ["wt-06", "wt-07", "wt-08", "wt-09"]);
        app.on_key(Key::Home);
        assert_eq!(visible(&app), ["wt-00", "wt-01", "wt-02", "wt-03"]);
    }

    /// The labels of `labels` drawn as list rows (after `│ ` or `│▌`), in order.
    fn rows_shown(screen: &str, labels: &[&str]) -> Vec<String> {
        let line_label = |line: &str| labels.iter().find(|l| [" ", "▌"].iter().any(|bar| line.contains(&format!("│{bar}  {l}  ")))).map(|l| l.to_string());
        screen.lines().filter_map(line_label).collect()
    }

    #[test]
    fn settings_scroll_like_the_worktree_list() {
        use crate::settings_ui::FIELDS;
        let mut app = fixture();
        app.settings_editor = Some(crate::settings_ui::SettingsEditor::new(&app.settings));
        let labels: Vec<&str> = FIELDS.iter().map(|f| f.label).collect();
        let shown = |app: &App| rows_shown(&render(app, 100, 24), &labels);
        let selected = |app: &App| FIELDS[app.settings_editor.as_ref().unwrap().cursor].label;

        app.on_key(Key::End);
        let page = shown(&app);
        assert_eq!(page.last().map(String::as_str), Some(selected(&app)));
        // Moving up inside the page doesn't scroll it, up to its top field.
        while selected(&app) != page[0] {
            app.on_key(Key::Up);
            assert_eq!(shown(&app), page);
        }
        // One more scrolls, bringing the field above (and any section header) in.
        app.on_key(Key::Char('k'));
        let above = shown(&app);
        assert_eq!(above[0], selected(&app));
        assert_eq!(above[1], page[0]);
        // Going back down stays put until the bottom row.
        for _ in 1..above.len() {
            app.on_key(Key::Char('j'));
            assert_eq!(shown(&app), above);
        }
        // Page and end keys move the cursor too.
        app.on_key(Key::Home);
        assert_eq!(selected(&app), FIELDS[0].label);
        app.on_key(Key::PageDown);
        assert_eq!(app.settings_editor.as_ref().unwrap().cursor, crate::nav::PAGE as usize);
        app.on_key(Key::Char('G'));
        assert_eq!(selected(&app), FIELDS[FIELDS.len() - 1].label);
    }

    #[test]
    fn key_bindings_scroll_like_the_worktree_list() {
        use crate::keymap::Command;
        let mut app = fixture();
        let mut ed = crate::settings_ui::SettingsEditor::new(&app.settings);
        ed.keys = Some(crate::keys_ui::KeyEditor::new(app.keymap.clone()));
        app.settings_editor = Some(ed);
        let labels: Vec<&str> = Command::ALL.iter().map(|c| c.label()).collect();
        let shown = |app: &App| rows_shown(&render(app, 100, 20), &labels);
        let selected = |app: &App| Command::ALL[app.settings_editor.as_ref().unwrap().keys.as_ref().unwrap().cursor].label();

        app.on_key(Key::End);
        let page = shown(&app);
        assert!(page.len() < Command::ALL.len(), "the page should scroll");
        assert_eq!(page.last().map(String::as_str), Some(selected(&app)));
        while selected(&app) != page[0] {
            app.on_key(Key::Up);
            assert_eq!(shown(&app), page);
        }
        app.on_key(Key::Up);
        assert_eq!(shown(&app)[0], selected(&app));
    }

    #[test]
    fn wheel_moves_the_selection_and_clamps() {
        let mut app = fixture();
        app.on_mouse(crate::app::Mouse::Scroll(1));
        app.on_mouse(crate::app::Mouse::Scroll(1));
        app.on_mouse(crate::app::Mouse::Scroll(1));
        assert_eq!(app.selected, 2);
        app.on_mouse(crate::app::Mouse::Scroll(-1));
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn hovering_a_row_tints_it_but_not_the_selection_or_headers() {
        let mut app = fixture();
        let (w, h) = (120, 16);
        let theme = Theme::ansi();
        let bg_at = |app: &App, (x, y): (u16, u16)| {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| draw(f, app, &theme, NOW, &[])).unwrap();
            term.backend().buffer()[(x, y)].bg
        };
        let row = find(&app, w, h, "fix/offline-cache");
        assert_ne!(app.selected_view().unwrap().worktree.branch, "fix/offline-cache");
        app.on_mouse(crate::app::Mouse::Move { x: row.0, y: row.1 });
        assert_eq!(bg_at(&app, row), theme.hover_bg());
        // Moving the pointer selects nothing.
        assert_ne!(app.selected_view().unwrap().worktree.branch, "fix/offline-cache");

        // The selected row keeps its own highlight.
        click(&mut app, w, h, row, false);
        assert_ne!(bg_at(&app, row), theme.hover_bg());

        // A repo header isn't a row.
        let header = find(&app, w, h, "gridsight ─");
        app.on_mouse(crate::app::Mouse::Move { x: header.0, y: header.1 });
        assert_ne!(bg_at(&app, header), theme.hover_bg());
        // Key-bar hints light up like buttons.
        let help = find(&app, w, h, "h help");
        app.on_mouse(crate::app::Mouse::Move { x: help.0, y: help.1 });
        assert_eq!(bg_at(&app, help), theme.hover_bg());
        // An overlay takes the mouse: the row behind help doesn't light up.
        let other = find(&app, w, h, "feature/refactor");
        app.on_mouse(crate::app::Mouse::Move { x: other.0, y: other.1 });
        assert_eq!(bg_at(&app, other), theme.hover_bg());
        app.show_help = true;
        assert_eq!(target_at(&app, w, h, other), None);
        app.show_help = false;
        app.settings.mouse_enabled = false;
        assert_ne!(bg_at(&app, other), theme.hover_bg());
    }

    #[test]
    fn hover_colour_sits_between_background_and_selection() {
        use ratatui::style::Color;
        let mut t = Theme::ansi();
        assert_eq!(t.hover_bg(), Color::Indexed(238));
        t.selection = crate::theme::Selection::Bg(Color::Rgb(100, 100, 100));
        t.background = Some(Color::Rgb(200, 0, 100));
        assert_eq!(t.hover_bg(), Color::Rgb(150, 50, 100));
        t.background = None;
        assert_eq!(t.hover_bg(), Color::Rgb(75, 75, 75));
    }

    #[test]
    fn key_bar_hints_are_clickable() {
        let mut app = fixture();
        let help = find(&app, 120, 16, "h help");
        assert_eq!(target_at(&app, 120, 16, (help.0 + 3, help.1)), Some(Target::Command(Command::Help)));
        click(&mut app, 120, 16, help, false);
        assert!(app.show_help);
        // Any click closes help.
        click(&mut app, 120, 16, (0, 0), false);
        assert!(!app.show_help);
        let sep = find(&app, 120, 16, "│ g PR");
        assert_eq!(target_at(&app, 120, 16, sep), None);
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
        app.on_mouse(crate::app::Mouse::Scroll(1));
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
        app.on_mouse(crate::app::Mouse::Scroll(1));
        assert_eq!(app.settings_editor.as_ref().unwrap().field().key, "startupScriptMode");
    }

    #[test]
    fn hints_parse_into_clickable_keys() {
        let mut hits = Vec::new();
        hint_hits(&mut hits, " Sort order — space on/off · J/K move · ⏎ save  esc cancel", 10, 3);
        let keys: Vec<(u16, u16, Target)> = hits.iter().map(|h| (h.rect.x, h.rect.width, h.target)).collect();
        // ` Sort order` and `J/K move` aren't one key; the rest are.
        assert_eq!(
            keys,
            vec![
                (10 + 14, 12, Target::Key(Key::Char(' '))),
                (10 + 40, 6, Target::Key(Key::Enter)),
                (10 + 48, 10, Target::Key(Key::Esc)),
            ]
        );
        assert!(hits.iter().all(|h| h.rect.y == 3));
    }

    #[test]
    fn settings_hint_and_field_hover() {
        let mut app = fixture();
        app.on_key(Key::Char('s'));
        let theme = Theme::ansi();
        let esc = find(&app, 120, 50, "esc close");
        click(&mut app, 120, 50, esc, false);
        assert!(app.settings_editor.is_none());
        app.on_key(Key::Char('s'));
        // Hovering a field that isn't the cursor tints it.
        let at = find(&app, 120, 50, "Mouse");
        app.on_mouse(crate::app::Mouse::Move { x: at.0, y: at.1 });
        let mut term = Terminal::new(TestBackend::new(120, 50)).unwrap();
        term.draw(|f| draw(f, &app, &theme, NOW, &[])).unwrap();
        assert_eq!(term.backend().buffer()[at].bg, theme.hover_bg());
        // A second click on the selected field changes it, like ⏎.
        click(&mut app, 120, 50, at, false);
        assert!(app.settings.mouse_enabled);
        assert_eq!(app.settings_editor.as_ref().unwrap().field().key, "mouseEnabled");
    }

    #[test]
    fn settings_sub_pages_take_clicks() {
        use crate::repos_ui::{RepoEditor, RepoRow};
        let mut app = fixture();
        let mut ed = crate::settings_ui::SettingsEditor::new(&app.settings);
        let row = |id: &str| RepoRow { id: id.into(), name: id.into(), path: format!("~/{id}"), exists: true, worktrees: 1, script: false, review_script: false };
        ed.repos = Some(RepoEditor::new(vec![row("alpha"), row("beta")]));
        app.settings_editor = Some(ed);
        let beta = find(&app, 100, 20, "beta");
        click(&mut app, 100, 20, beta, false);
        assert_eq!(app.settings_editor.as_ref().unwrap().repos.as_ref().unwrap().cursor, 1);
        // A click on the selected repo opens its script chooser, like ⏎;
        // there a click picks a script and a second one edits it.
        assert_eq!(click(&mut app, 100, 20, beta, false), crate::app::Action::None);
        assert_eq!(app.settings_editor.as_ref().unwrap().repos.as_ref().unwrap().scripts, Some(0));
        let screen = render(&app, 100, 30);
        assert!(screen.contains("Scripts for beta") && screen.contains("in that PR's worktree"), "{screen}");
        let review = find(&app, 100, 30, "Review script");
        click(&mut app, 100, 30, review, false);
        assert_eq!(app.settings_editor.as_ref().unwrap().repos.as_ref().unwrap().scripts, Some(1));
        assert_eq!(click(&mut app, 100, 30, review, false), crate::app::Action::EditScript("beta".into(), am_core::worktrees::ScriptKind::Review));
        insta::assert_snapshot!("repository_scripts", render(&app, 100, 30));
        app.on_key(Key::Esc);
        // Header hints are buttons: `x remove` asks, `any other key cancels`.
        let x = find(&app, 100, 20, "x remove");
        click(&mut app, 100, 20, x, false);
        assert!(app.settings_editor.as_ref().unwrap().repos.as_ref().unwrap().confirm_remove.is_some());
        let cancel = find(&app, 100, 20, "any other key");
        click(&mut app, 100, 20, cancel, false);
        assert!(app.settings_editor.as_ref().unwrap().repos.as_ref().unwrap().confirm_remove.is_none());
        // The settings footer belongs to the field list, not this page.
        let back = find(&app, 100, 20, "esc back");
        click(&mut app, 100, 20, back, false);
        let ed = app.settings_editor.as_ref().unwrap();
        assert!(ed.repos.is_none());

        // Key bindings: a click on the selected row starts capturing, and
        // then clicks don't count as the new key.
        app.settings_editor.as_mut().unwrap().keys = Some(crate::keys_ui::KeyEditor::new(app.keymap.clone()));
        let row = find(&app, 100, 34, "Refresh now");
        click(&mut app, 100, 34, row, false);
        click(&mut app, 100, 34, row, false);
        assert!(app.settings_editor.as_ref().unwrap().keys.as_ref().unwrap().capturing);
        let esc = find(&app, 100, 34, "Settings");
        assert_eq!(target_at(&app, 100, 34, esc), None);
        app.on_mouse(crate::app::Mouse::Scroll(1));
        assert!(app.settings_editor.as_ref().unwrap().keys.as_ref().unwrap().capturing);
    }

    #[test]
    fn theme_builder_palette_takes_clicks() {
        use crate::theme_builder::Mode;
        let mut app = with_builder("catppuccin-mocha");
        let token = find(&app, 110, 28, " accent");
        click(&mut app, 110, 28, token, false);
        let b = app.settings_editor.as_ref().unwrap().builder.as_ref().unwrap();
        assert_eq!(b.token().key(), "accent");
        // Second click opens the palette; a swatch click moves its cursor, a
        // second one picks it.
        click(&mut app, 110, 28, token, false);
        assert!(matches!(app.settings_editor.as_ref().unwrap().builder.as_ref().unwrap().mode, Mode::Picker { .. }));
        render(&app, 110, 28);
        let swatch = app.hits.borrow().iter().find(|h| h.target == Target::Item(List::Palette, 3)).map(|h| (h.rect.x, h.rect.y)).unwrap();
        click(&mut app, 110, 28, swatch, false);
        let b = app.settings_editor.as_ref().unwrap().builder.as_ref().unwrap();
        assert_eq!(b.mode, Mode::Picker { cursor: 3 });
        let picked = b.palette()[3];
        click(&mut app, 110, 28, swatch, false);
        let b = app.settings_editor.as_ref().unwrap().builder.as_ref().unwrap();
        assert_eq!(b.mode, Mode::Tokens);
        assert_eq!(b.color(am_core::themes::Token::Accent).0, picked);
    }

    #[test]
    fn folder_browser_takes_clicks() {
        use crate::folder_browser::{Entry, FolderBrowser};
        let mut app = fixture();
        let dir = std::path::PathBuf::from("/src");
        let e = |name: &str, is_repo| Entry { name: name.into(), path: dir.join(name), is_repo, tracked: false };
        app.modal = Some(crate::modals::Modal::AddRepo(FolderBrowser::fixture(dir.clone(), vec![e("website", true), e("notes", false)])));
        let esc = find(&app, 100, 26, "esc cancel");
        let website = find(&app, 100, 26, "website/");
        click(&mut app, 100, 26, website, false);
        assert_eq!(click(&mut app, 100, 26, website, false), crate::app::Action::AddRepo("/src/website".into()));
        // While it adds, the browser waits; once that fails, esc closes it.
        if let Some(crate::modals::Modal::AddRepo(b)) = &mut app.modal {
            b.add_failed("nope".into());
        }
        click(&mut app, 100, 26, esc, false);
        assert!(app.modal.is_none());
    }

    #[test]
    fn modals_take_clicks_but_the_dashboard_behind_does_not() {
        let mut app = fixture();
        app.on_key(Key::Char('d'));
        assert!(matches!(app.modal, Some(crate::modals::Modal::Delete(_))));
        // A row behind the dialog does nothing.
        let behind = find(&app, 120, 30, "agent-monitor");
        click(&mut app, 120, 30, behind, false);
        assert!(app.modal.is_some());
        let cancel = find(&app, 120, 30, "Cancel");
        click(&mut app, 120, 30, cancel, false);
        assert!(app.modal.is_none());
        app.on_key(Key::Char('d'));
        let yes = find(&app, 120, 30, "Yes");
        assert!(matches!(click(&mut app, 120, 30, yes, false), crate::app::Action::Delete(..)));

        // New worktree: clicking a field moves the focus there.
        let repo = am_core::model::Repository { id: "r".into(), path: "/r".into(), name: "r".into() };
        app.modal = Some(crate::modals::Modal::new_worktree(vec![repo], None, "", "main"));
        let name = find(&app, 120, 30, "Name ");
        click(&mut app, 120, 30, name, false);
        assert!(matches!(&app.modal, Some(crate::modals::Modal::New(f)) if f.field == crate::modals::FormField::Name));
    }

    #[test]
    fn setup_wizard_takes_clicks() {
        let mut app = fixture();
        let mut s = crate::setup::Setup::new(vec![], &Settings::parse("{}"));
        s.hook_binary = None;
        app.setup = Some(s);
        // The mode: a click picks it, a second goes on.
        let reviews = find(&app, 100, 30, "Review requests only");
        click(&mut app, 100, 30, reviews, false);
        assert!(app.setup.as_ref().unwrap().reviews_mode);
        let full = find(&app, 100, 30, "Agents & worktrees");
        click(&mut app, 100, 30, full, false);
        click(&mut app, 100, 30, full, false);
        assert!(!app.setup.as_ref().unwrap().reviews_mode);
        assert_eq!(app.setup.as_ref().unwrap().step, crate::setup::Step::Editor);
        let zed = find(&app, 100, 30, "Zed");
        click(&mut app, 100, 30, zed, false);
        let zed_index = crate::setup::IDES.iter().position(|(_, l)| *l == "Zed").unwrap();
        assert_eq!(app.setup.as_ref().unwrap().ide, zed_index);
        // A second click on the chosen editor goes on, like ⏎.
        click(&mut app, 100, 30, zed, false);
        assert_eq!(app.setup.as_ref().unwrap().step, crate::setup::Step::Hooks);
        let skip = find(&app, 100, 30, "n skip");
        click(&mut app, 100, 30, skip, false);
        assert_eq!(app.setup.as_ref().unwrap().step, crate::setup::Step::Repos);
    }

    #[test]
    fn job_answers_are_clickable() {
        let mut app = fixture();
        let repo = am_core::model::Repository { id: "r".into(), path: "/r".into(), name: "r".into() };
        let job = app.jobs.start_create(repo, am_core::worktrees::CreateRequest { branch: "feat/x".into(), ..Default::default() });
        app.jobs.ask(job, crate::jobs::Question::BranchExists { local: true, remote: false });
        let cancel = find(&app, 120, 20, "Cancel");
        click(&mut app, 120, 20, cancel, false);
        assert!(app.jobs.get(job).is_none_or(|j| j.question.is_none()));
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
            reviews_mode: false,
            gh_found: true,
            ide: 0,
            ide_command: String::new(),
            ide_error: None,
            hooks_installed: true,
            agents: vec![(am_core::harness::HarnessKind::CLAUDE, true)],
            hook_binary: None,
            repo_input: "~/".into(),
            repos_added: vec![],
            repo_error: None,
            gh: true,
            linear_key: String::new(),
            integration_field: 0,
            review: Default::default(),
            ctx_choice: crate::setup::CONTEXT_CUSTOM,
            ctx_input: "128k".into(),
            ctx_error: None,
            context_tokens: None,
            telemetry: false,
            cancellable: false,
        });
        let out = render(&app, 100, 20);
        assert!(out.contains("✓ Reviews › Context"), "breadcrumbs cut off:\n{out}");
        insta::assert_snapshot!(out);
    }

    #[test]
    fn setup_privacy_step_says_what_is_sent_and_fits_every_breadcrumb() {
        let mut app = fixture();
        let mut setup = crate::setup::Setup::new(vec![], &app.settings);
        setup.step = crate::setup::Step::Privacy;
        setup.reviews_mode = false;
        app.setup = Some(setup);
        let out = render(&app, 100, 22);
        assert!(out.contains("✓ Context › Privacy"), "breadcrumbs cut off:\n{out}");
        assert!(out.contains("● Don't send anything"), "off unless picked:\n{out}");
        insta::assert_snapshot!(out);
    }

    #[test]
    fn telemetry_prompt_asks_once_and_saves_the_answer() {
        let mut app = fixture();
        app.modal = Some(crate::modals::Modal::Telemetry);
        insta::assert_snapshot!(render(&app, 100, 24));
        // A stray key doesn't decide.
        app.on_key(Key::Char('j'));
        assert!(matches!(app.modal, Some(crate::modals::Modal::Telemetry)));
    }

    #[test]
    fn setup_editor_step_takes_a_custom_command() {
        let mut app = fixture();
        app.setup = Some(crate::setup::Setup {
            step: crate::setup::Step::Editor,
            reviews_mode: false,
            gh_found: true,
            ide: crate::setup::IDE_CUSTOM,
            ide_command: "subl -n".into(),
            ide_error: None,
            hooks_installed: true,
            agents: vec![(am_core::harness::HarnessKind::CLAUDE, true)],
            hook_binary: None,
            repo_input: "~/".into(),
            repos_added: vec![],
            repo_error: None,
            gh: true,
            linear_key: String::new(),
            integration_field: 0,
            review: Default::default(),
            ctx_choice: 0,
            ctx_input: String::new(),
            ctx_error: None,
            context_tokens: None,
            telemetry: false,
            cancellable: false,
        });
        let out = render(&app, 100, 22);
        assert!(out.contains("● Custom command: subl -n▏"), "{out}");
        insta::assert_snapshot!(out);
    }

    #[test]
    fn rows_running_a_shell_command_are_highlighted() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        snap.groups[0].worktrees[1].shell_commands = 2;
        app.apply(snap);
        let row = |w: u16| render(&app, w, 12).lines().find(|l| l.contains("fix/offline-cache")).unwrap().to_string();
        assert!(row(120).contains("fix/offline-cache $ 2 commands"), "{}", row(120));
        assert!(row(90).contains(" $2") && !row(90).contains("commands"), "{}", row(90));
        assert!(!render(&app, 120, 12).lines().any(|l| l.contains("feature/refactor") && l.contains('$')));

        // The name and the badge are bold teal (a waiting row stays yellow).
        let theme = Theme::ansi();
        let i = app.flat.iter().position(|v| v.worktree.branch == "fix/offline-cache").unwrap();
        let line = worktree_line(&app, &theme, &app.flat[i], "", false, true, &cols_for(&app), 120, NOW);
        let style_of = |text: &str| line.spans.iter().find(|s| s.content.contains(text)).map(|s| s.style.fg);
        let expected = if app.flat[i].status() == Some(Status::Waiting) { theme.yellow } else { theme.teal };
        assert_eq!(style_of(app.flat[i].worktree.display_name()), Some(Some(expected)));
        assert_eq!(style_of("$ 2 commands"), Some(Some(theme.teal)));
    }

    #[test]
    fn context_bar_turns_yellow_then_red_as_the_window_fills() {
        let theme = Theme::ansi();
        let mut v = fixture().flat[0].clone();
        let at = |v: &mut WorktreeView, tokens| {
            v.context = Some(ContextUsage { tokens, model: None, window: None });
            let spans = ctx_spans(v.context.as_ref(), &theme, 8, 200_000, false);
            let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
            (text, spans[0].style.fg, spans[2].style.fg)
        };
        assert_eq!(at(&mut v, 100_000), ("████░░░░ 100k".into(), Some(theme.green), Some(theme.green)));
        assert_eq!(at(&mut v, 150_000), ("██████░░ 150k".into(), Some(theme.yellow), Some(theme.yellow)));
        assert_eq!(at(&mut v, 190_000), ("███████▒ 190k".into(), Some(theme.red), Some(theme.red)));
        // Past the configured window the scale doubles until it fits (400k) instead of pinning.
        assert_eq!(at(&mut v, 250_000), ("█████░░░ 250k".into(), Some(theme.green), Some(theme.green)));
        assert_eq!(at(&mut v, 700_000), ("███████░ 700k".into(), Some(theme.yellow), Some(theme.yellow)));
        v.context = None;
        let blank: String = ctx_spans(v.context.as_ref(), &theme, 8, 200_000, false).iter().map(|s| s.content.as_ref()).collect();
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
        let spans = pr_spans(v.pr.as_ref(), &theme, 16, true);
        assert_eq!(spans[0].content, "#127");
        assert_eq!(spans[0].style.fg, Some(theme.red));
        assert_eq!(spans[1].content, "✗");
        assert_eq!(spans[1].style.fg, Some(theme.red));
        v.pr.as_mut().unwrap().checks_status = ChecksStatus::Passing;
        let spans = pr_spans(v.pr.as_ref(), &theme, 16, true);
        assert_eq!(spans[0].style.fg, Some(theme.faint), "passing draft stays dim");
        assert_eq!(spans[1].style.fg, Some(theme.green));
    }

    #[test]
    fn pr_review_state_approved_colour_follows_ci() {
        let theme = Theme::ansi();
        let mut v = fixture().flat[0].clone();
        let label = |v: &WorktreeView, words: bool| {
            let spans = pr_spans(v.pr.as_ref(), &theme, 16, words);
            spans.iter().find(|s| s.content.starts_with(' ') && !s.content.trim().is_empty()).map(|s| (s.content.trim().to_string(), s.style.fg))
        };
        let set = |v: &mut WorktreeView, review: &str, checks: ChecksStatus, mergeable: Option<&str>| {
            let pr = v.pr.as_mut().unwrap();
            pr.review_decision = review.into();
            pr.checks_status = checks;
            pr.mergeable = mergeable.map(str::to_string);
        };
        set(&mut v, "APPROVED", ChecksStatus::Passing, Some("MERGEABLE"));
        assert_eq!(label(&v, true), Some(("approved".into(), Some(theme.green))));
        assert_eq!(pr_text(v.pr.as_ref(), true), "#127✓ approved");
        set(&mut v, "APPROVED", ChecksStatus::None, None);
        assert_eq!(label(&v, true), Some(("approved".into(), Some(theme.green))), "no CI configured is still green");
        set(&mut v, "APPROVED", ChecksStatus::Failing, Some("MERGEABLE"));
        assert_eq!(label(&v, true), Some(("approved".into(), Some(theme.yellow))));
        assert_eq!(pr_text(v.pr.as_ref(), true), "#127✗ approved");
        set(&mut v, "APPROVED", ChecksStatus::Pending, None);
        assert_eq!(label(&v, true).unwrap().0, "approved");
        set(&mut v, "APPROVED", ChecksStatus::Passing, Some("CONFLICTING"));
        assert_eq!(label(&v, true).unwrap().0, "conflicts");
        set(&mut v, "CHANGES_REQUESTED", ChecksStatus::Passing, None);
        assert_eq!(label(&v, true), Some(("changes".into(), Some(theme.red))));
        set(&mut v, "REVIEW_REQUIRED", ChecksStatus::Passing, None);
        assert_eq!(label(&v, true).unwrap().0, "review");
        set(&mut v, "", ChecksStatus::Passing, None);
        assert_eq!(pr_text(v.pr.as_ref(), true), "#127✓", "unknown review state shows nothing");
        // An approved draft shows the approval, CI failing or not.
        v.pr.as_mut().unwrap().is_draft = true;
        set(&mut v, "APPROVED", ChecksStatus::Passing, Some("MERGEABLE"));
        assert_eq!(label(&v, true), Some(("approved".into(), Some(theme.green))));
        set(&mut v, "APPROVED", ChecksStatus::Failing, Some("MERGEABLE"));
        assert_eq!(label(&v, true), Some(("approved".into(), Some(theme.yellow))));
        assert_eq!(pr_text(v.pr.as_ref(), true), "#127✗ approved");
        set(&mut v, "REVIEW_REQUIRED", ChecksStatus::Passing, None);
        assert_eq!(pr_text(v.pr.as_ref(), true), "#127✓ draft");
        set(&mut v, "", ChecksStatus::Passing, None);
        assert_eq!(pr_text(v.pr.as_ref(), true), "#127✓ draft");
        v.pr.as_mut().unwrap().is_draft = false;
        // Narrow terminals get a glyph instead of the word.
        set(&mut v, "APPROVED", ChecksStatus::Passing, None);
        assert_eq!(pr_text(v.pr.as_ref(), false), "#127✓ ✔");
        // Merged PRs have no review state; closed ones say so in red.
        v.pr.as_mut().unwrap().state = "MERGED".into();
        assert_eq!(pr_text(v.pr.as_ref(), true), "#127✓⇡");
        v.pr.as_mut().unwrap().state = "CLOSED".into();
        assert_eq!(pr_text(v.pr.as_ref(), true), "#127✓ closed");
        assert_eq!(label(&v, true), Some(("closed".into(), Some(theme.red))));
        assert_eq!(pr_text(v.pr.as_ref(), false), "#127✓ ⊘");
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
            [("w1", "▾ "), ("w4", "  ├─ "), ("w5", "  │  └─ "), ("w6", "  └─ "), ("w2", "  "), ("w3", "  ")],
            "children follow the parent, depth-first, with tree lines; the root has a fold arrow, every row its gutter"
        );
        insta::assert_snapshot!(render(&app, 100, 14));
    }

    #[test]
    fn tmp_space_group_names_each_rows_repo() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        snap.repos = snap.groups.iter().map(|g| g.repo.clone()).collect();
        let mut theirs = view("w7", "r2", "alice/fix-ci", None);
        theirs.worktree.tmp_space = true;
        snap.groups.push(WorktreeGroup {
            repo: Repository { id: TMP_SPACE_GROUP_ID.into(), path: String::new(), name: "Reviewing".into() },
            worktrees: vec![theirs],
        });
        app.apply(snap);
        insta::assert_snapshot!(render(&app, 120, 18));
        assert!(app.rows.contains(&crate::app::Row::Repo("Reviewing".into())));
    }

    #[test]
    fn a_tmp_space_create_waits_in_the_tmp_space() {
        let mut app = fixture();
        let repo = app.snapshot.groups[0].repo.clone();
        let req = am_core::worktrees::CreateRequest { branch: "bob/x".into(), tmp_space: true, ..Default::default() };
        let job = app.jobs.start_create(repo, req);
        app.rebuild_rows();
        let n = app.rows.len();
        assert_eq!(app.rows[n - 2..], [crate::app::Row::Repo("Reviewing".into()), crate::app::Row::Pending(job)]);
    }

    fn session(harness: HarnessKind, id: &str, status: Status, updated: &str, tokens: Option<u64>) -> SessionView {
        SessionView {
            agent: harness,
            session_id: id.into(),
            status,
            last_response: None,
            started_at: "2025-10-11 09:00:00".into(),
            updated_at: updated.into(),
            context: tokens.map(|tokens| ContextUsage { tokens, model: None, window: None }),
            cmux: None,
            waiting_on: None,
        }
    }

    #[test]
    fn an_agent_only_watching_ci_says_so_on_its_row_and_session() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        let w1 = &mut snap.groups[0].worktrees[0];
        let a = w1.agent.as_mut().unwrap();
        a.status = Status::Executing;
        a.session_id = Some("s".into());
        let mut watching = session(HarnessKind::CLAUDE, "s", Status::Executing, "2025-10-11 09:59:00", None);
        watching.waiting_on = Some("gh run watch 12".into());
        w1.sessions = vec![watching.clone()];
        app.apply(snap.clone());
        let out = render(&app, 120, 11);
        let row = out.lines().find(|l| l.contains("feature/refactor")).unwrap_or_default().to_string();
        assert!(row.contains("⧗ watching"), "{row}");
        assert!(render(&app, 90, 11).lines().any(|l| l.contains("feature/refactor") && l.contains("⧗") && !l.contains("watching")));

        // With two sessions, the session row says what it watches.
        let other = session(HarnessKind::CLAUDE, "t", Status::Done, "2025-10-11 09:58:00", None);
        snap.groups[0].worktrees[0].sessions = vec![watching, other];
        app.apply(snap.clone());
        let out = render(&app, 120, 11);
        assert!(out.lines().any(|l| l.contains("├ ") && l.contains("watching gh run watch 12")), "{out}");

        // Waiting on the user wins: no watching badge.
        snap.groups[0].worktrees[0].agent.as_mut().unwrap().status = Status::Waiting;
        app.apply(snap);
        assert!(!render(&app, 120, 11).lines().any(|l| l.contains("feature/refactor") && l.contains("⧗")));
    }

    #[test]
    fn three_sessions_in_a_worktree_are_listed_under_it() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        // feature/refactor runs two Claude sessions and a Gemini one at once,
        // each with its own subagents. (Rendering doesn't care which harness
        // reported a subagent; Gemini CLI's hooks don't report them today.)
        let w1 = &mut snap.groups[0].worktrees[0];
        w1.agent.as_mut().unwrap().status = Status::Waiting;
        w1.sessions = vec![
            session(HarnessKind::CLAUDE, "s", Status::Delegating, "2025-10-11 09:58:00", Some(121_000)),
            session(HarnessKind::CLAUDE, "7b1e-claude", Status::Executing, "2025-10-11 09:59:50", Some(310_000)),
            session(HarnessKind::GEMINI, "g", Status::Waiting, "2025-10-11 09:59:30", Some(40_000)),
        ];
        let mut in_session = |sid: &str, id: &str, desc: &str| {
            let (wt, mut s) = sub("w1", id, desc, true, Some(("Read", "src/db.rs")));
            s.session_id = sid.into();
            snap.subagents.push((wt, s));
        };
        in_session("7b1e-claude", "b1", "Write migration tests");
        in_session("g", "c1", "Investigate codebase");
        app.apply(snap);
        assert_eq!(app.flat.len(), 3, "session rows take no selection index");
        assert!(
            matches!(app.rows[1..5], [Row::Worktree(0), Row::Session(0, 0), Row::Session(0, 1), Row::Session(0, 2)]),
            "{:?}",
            app.rows
        );

        let out = render(&app, 120, 11);
        let line = |needle: &str| out.lines().find(|l| l.contains(needle)).unwrap_or_default().to_string();
        let parent = line("feature/refactor");
        assert!(parent.contains("⧉ 3 sessions") && !parent.contains("◆") && !parent.contains("121k"), "{parent}");
        let first = line("claude #s ");
        assert!(first.contains("delegating") && first.contains("◆ 2 subagents") && first.contains("121k"), "{first}");
        let second = line("claude #7b1e");
        assert!(second.contains("executing") && second.contains("◆ 1 subagent ") && second.contains("310k"), "{second}");
        let gemini = line("└ ");
        assert!(gemini.contains("gemini  waiting") && gemini.contains("◆ 1 subagent") && gemini.contains("40k"), "{gemini}");
        assert!(!gemini.contains('#'), "a harness with one session needs no id: {gemini}");
        insta::assert_snapshot!(out);
        insta::assert_snapshot!("three_sessions_narrow", render(&app, 90, 11));

        // A click on a session row selects its worktree.
        let y = out.lines().position(|l| l.contains("└ ")).unwrap() as u16;
        assert_eq!(target_at(&app, 120, 11, (10, y)), Some(Target::Worktree(0)));
    }

    #[test]
    fn a_single_session_is_not_listed_separately() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        snap.groups[0].worktrees[0].sessions = vec![session(HarnessKind::CLAUDE, "s", Status::Delegating, "2025-10-11 09:58:00", None)];
        app.apply(snap);
        assert!(!app.rows.iter().any(|r| matches!(r, Row::Session(..))));
        assert!(!render(&app, 120, 10).contains('⧉'));
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
        assert!(app.key_available(Command::OpenPr) && app.key_available(Command::OpenLinear) && app.key_available(Command::Delete));
        assert!(!app.key_available(Command::OpenProject), "ticket without a project");
        // w2: no PR, no ticket.
        app.on_key(Key::Char('j'));
        assert!(!app.key_available(Command::OpenPr) && !app.key_available(Command::OpenLinear));
        assert!(app.key_available(Command::Open) && app.key_available(Command::Delete));
        // w3: the main checkout on main can't be deleted.
        app.on_key(Key::Char('j'));
        assert!(!app.key_available(Command::Delete));
        assert!(app.key_available(Command::NewWorktree) && app.key_available(Command::Help), "global keys are always on");
        // A row being deleted takes no row actions at all.
        app.deleting.insert("w3".into(), false);
        assert!(!app.key_available(Command::Open) && !app.key_available(Command::OpenPr));

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
                Row::Base(_) => "base",
                Row::Worktree(_) => "wt",
                Row::Pending(_) => "pending",
                Row::Session(..) => "session",
            })
            .collect();
        // The last repo's worktree has no project: no `No project` header for it.
        assert_eq!(kinds, ["repo", "project", "wt", "spacer", "project", "wt", "spacer", "repo", "wt"]);
    }

    #[test]
    fn ticket_cluster_is_followed_by_a_blank_line() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        let ticket = snap.groups[0].worktrees[0].linear.clone();
        let mut twin = snap.groups[0].worktrees[0].clone();
        twin.worktree.id = "w4".into();
        twin.worktree.branch = "feature/refactor-ui".into();
        snap.groups[0].worktrees.insert(1, twin);
        snap.groups[0].worktrees[1].linear = ticket;
        app.apply(snap);
        let kinds: Vec<&str> = app
            .rows
            .iter()
            .map(|r| match r {
                Row::Spacer => "spacer",
                Row::Repo(_) => "repo",
                Row::Project(_) => "project",
                Row::Ticket(_) => "ticket",
                Row::Base(_) => "base",
                Row::Worktree(_) => "wt",
                Row::Pending(_) => "pending",
                Row::Session(..) => "session",
            })
            .collect();
        assert_eq!(kinds, ["repo", "ticket", "wt", "wt", "spacer", "wt", "spacer", "repo", "wt"]);
        assert_eq!(app.flat.len(), 4, "the spacer takes no selection index");
    }

    #[test]
    fn ticket_cluster_is_preceded_by_a_blank_line() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        let ticket = snap.groups[0].worktrees[0].linear.clone();
        let mut twin = snap.groups[0].worktrees[0].clone();
        twin.worktree.id = "w4".into();
        twin.worktree.branch = "feature/refactor-ui".into();
        snap.groups[0].worktrees.push(twin);
        snap.groups[0].worktrees[0].linear = None;
        let n = snap.groups[0].worktrees.len();
        snap.groups[0].worktrees[n - 2].linear = ticket;
        app.apply(snap);
        let kinds: Vec<&str> = app
            .rows
            .iter()
            .map(|r| match r {
                Row::Spacer => "spacer",
                Row::Repo(_) => "repo",
                Row::Project(_) => "project",
                Row::Ticket(_) => "ticket",
                Row::Base(_) => "base",
                Row::Worktree(_) => "wt",
                Row::Pending(_) => "pending",
                Row::Session(..) => "session",
            })
            .collect();
        assert_eq!(kinds, ["repo", "wt", "spacer", "ticket", "wt", "wt", "spacer", "repo", "wt"]);
    }

    fn row_kinds(app: &App) -> Vec<String> {
        app.rows
            .iter()
            .map(|r| match r {
                Row::Spacer => "spacer".to_string(),
                Row::Repo(_) => "repo".into(),
                Row::Project(_) => "project".into(),
                Row::Ticket(t) => format!("ticket {}", t.split(' ').next().unwrap_or("")),
                Row::Base(b) => format!("base {b}"),
                Row::Worktree(i) => app.flat[*i].worktree.id.clone(),
                Row::Pending(_) => "pending".into(),
                Row::Session(..) => "session".into(),
            })
            .collect()
    }

    /// Three stacks in one repo, listed interleaved: `a` (ENG-1 → ENG-2) and
    /// `d` (ENG-7) under epic EPIC-A, `b` (rooted on a branch named after
    /// another epic's ticket, ENG-5, with two EPIC-B tickets on it) and `c`
    /// (ENG-5 → ENG-6, epic EPIC-C).
    fn epic_stacks() -> App {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        let ticket = |id: &str, parent: Option<&str>| {
            let mut l = snap.groups[0].worktrees[0].linear.clone().unwrap();
            l.identifier = id.into();
            l.title = format!("Ticket {id}");
            l.parent = parent.map(|p| am_core::model::LinearParent { identifier: p.into(), title: format!("Epic {p}") });
            l
        };
        let mut rows = Vec::new();
        for (id, t, parent) in [
            ("a1", "ENG-1", Some("EPIC-A")),
            ("b1", "ENG-5", Some("EPIC-C")),
            ("c1", "ENG-5", Some("EPIC-C")),
            ("d1", "ENG-7", Some("EPIC-A")),
            ("a2", "ENG-2", Some("EPIC-A")),
            ("b2", "ENG-8", Some("EPIC-B")),
            ("b3", "ENG-9", Some("EPIC-B")),
            ("c2", "ENG-6", Some("EPIC-C")),
        ] {
            let mut v = view(id, "r1", &format!("feature/{id}"), None);
            v.linear = Some(ticket(t, parent));
            rows.push(v);
        }
        snap.groups[0].worktrees = rows;
        snap.stack = [("a2", "a1"), ("b2", "b1"), ("b3", "b2"), ("c2", "c1")]
            .into_iter()
            .map(|(c, p)| (c.to_string(), p.to_string()))
            .collect();
        app.apply(snap);
        app
    }

    #[test]
    fn stacks_group_under_the_parent_ticket_most_of_their_tickets_have() {
        let app = epic_stacks();
        assert_eq!(
            row_kinds(&app),
            [
                "repo", "ticket EPIC-A", "a1", "a2", "d1", "spacer",
                // b's root is on ENG-5 (EPIC-C) but its stack is EPIC-B's.
                "ticket EPIC-B", "b1", "b2", "b3", "spacer",
                "ticket EPIC-C", "c1", "c2", "spacer",
                "repo", "w3",
            ],
        );
        assert!(app.rows.contains(&Row::Ticket("EPIC-A Epic EPIC-A".into())));
        let order: Vec<&str> = app.flat.iter().map(|v| v.worktree.id.as_str()).collect();
        assert_eq!(order, ["a1", "a2", "d1", "b1", "b2", "b3", "c1", "c2", "w3"], "flat follows the rows");
    }

    #[test]
    fn grouping_stacks_by_ticket_can_be_turned_off() {
        let mut app = epic_stacks();
        let mut s = app.settings.clone();
        s.group_stacks_by_ticket = false;
        app.set_settings(s);
        assert_eq!(row_kinds(&app), ["repo", "a1", "a2", "b1", "b2", "b3", "c1", "c2", "d1", "spacer", "repo", "w3"]);
    }

    #[test]
    fn stack_ticket_without_parents_is_the_roots_own() {
        let app = fixture();
        let v = &app.snapshot.groups[0].worktrees[0];
        let t = crate::app::stack_ticket(&[v]).unwrap();
        assert_eq!((t.id.as_str(), t.parent), ("ENG-412", false));
        // A lone stack on its own ticket gets no header (nothing shares it).
        assert!(!row_kinds(&app).iter().any(|k| k.starts_with("ticket")));
        let mut child = v.clone();
        child.linear.as_mut().unwrap().parent =
            Some(am_core::model::LinearParent { identifier: "ENG-412".into(), title: "Refactor grouping core".into() });
        let t = crate::app::stack_ticket(&[v, &child]).unwrap();
        assert_eq!((t.id.as_str(), t.parent), ("ENG-412", true), "the root is the epic of its children");
    }

    #[test]
    fn a_stack_on_a_branch_without_a_worktree_gets_a_faint_base_line() {
        let mut app = fixture();
        let mut snap = app.snapshot.clone();
        snap.stack_base.insert("w1".into(), "feature/not-pulled".into());
        app.apply(snap);
        let kinds = row_kinds(&app);
        let at = kinds.iter().position(|k| k == "w1").unwrap();
        assert_eq!(kinds[at - 1], "base feature/not-pulled");
        assert_eq!(app.flat.len(), 3, "the base line takes no selection index");
        assert!(render(&app, 100, 10).contains("┌ feature/not-pulled · no worktree"));
    }

    #[test]
    fn left_and_right_fold_and_unfold_stacks_at_their_root() {
        let mut app = epic_stacks();
        let at = |app: &App, id: &str| app.flat.iter().position(|v| v.worktree.id == id).unwrap();
        let selected = |app: &App| app.selected_view().unwrap().worktree.id.clone();
        // ← anywhere in a stack folds it to its root.
        app.selected = at(&app, "b3");
        app.on_key(Key::Left);
        assert_eq!(app.folded.get("b1"), Some(&2));
        assert_eq!(selected(&app), "b1", "the root is selected");
        assert!(!app.flat.iter().any(|v| v.worktree.id == "b2" || v.worktree.id == "b3"));
        assert_eq!(app.tree[at(&app, "b1")], "▸ ");
        let screen = render(&app, 120, 24);
        assert!(screen.contains("+2 stacked hidden"), "{screen}");
        // Folded: survives a refresh; ← again changes nothing.
        app.apply(app.snapshot.clone());
        app.on_key(Key::Left);
        assert!(app.folded.contains_key("b1") && selected(&app) == "b1");
        // → unfolds, then steps into the stack; → on a stacked row does nothing.
        app.on_key(Key::Right);
        assert!(app.folded.is_empty());
        assert_eq!(app.tree[at(&app, "b1")], "▾ ");
        app.on_key(Key::Right);
        assert_eq!(selected(&app), "b2");
        app.on_key(Key::Right);
        assert_eq!(selected(&app), "b2");
        // Only roots fold: a row without a stack has no arrow and ← leaves it.
        app.selected = at(&app, "d1");
        app.on_key(Key::Left);
        assert!(app.folded.is_empty() && selected(&app) == "d1");
        assert_eq!(app.tree[at(&app, "d1")], "  ");
        // A worktree created inside a folded stack unfolds it.
        app.selected = at(&app, "c2");
        app.on_key(Key::Left);
        assert!(app.folded.contains_key("c1"));
        app.pending_select = Some("c2".into());
        app.apply(app.snapshot.clone());
        assert_eq!(selected(&app), "c2");
        assert!(app.folded.is_empty());
    }

    #[test]
    fn clicking_the_fold_arrow_or_pill_folds_and_unfolds() {
        use crate::app::Mouse;
        let mut app = epic_stacks();
        render(&app, 120, 24);
        let (x, y) = find(&app, 120, 24, "▾ · feature/b1");
        app.on_mouse(Mouse::Click { x, y, double: false });
        assert_eq!(app.folded.get("b1"), Some(&2), "the arrow folds");
        assert_eq!(app.selected_view().unwrap().worktree.id, "b1");
        render(&app, 120, 24);
        let (x, y) = find(&app, 120, 24, "+2 stacked hidden");
        app.on_mouse(Mouse::Click { x, y, double: false });
        assert!(app.folded.is_empty(), "the pill unfolds");
        // A click elsewhere on the row only selects it.
        render(&app, 120, 24);
        let (x, y) = find(&app, 120, 24, "feature/c1");
        app.on_mouse(Mouse::Click { x, y, double: false });
        assert!(app.folded.is_empty());
    }

    #[test]
    fn grouped_and_folded_stacks_render() {
        let mut app = epic_stacks();
        let mut snap = app.snapshot.clone();
        snap.stack_base.insert("c1".into(), "feature/connect-1787".into());
        app.apply(snap);
        app.selected = app.flat.iter().position(|v| v.worktree.id == "b1").unwrap();
        app.on_key(Key::Left);
        insta::assert_snapshot!(render(&app, 110, 20));
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
