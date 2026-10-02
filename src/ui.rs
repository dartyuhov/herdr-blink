//! Rendering. Catppuccin Macchiato on the `#1e2030` popup background.

use std::collections::HashSet;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{App, Mode},
    model::{Harness, Item, Status},
    search::Field,
    state::now_ms,
    view::{BADGES, Node, Row, Summary, View},
};

const BG: Color = Color::Rgb(0x1e, 0x20, 0x30);
const SURFACE0: Color = Color::Rgb(0x36, 0x3a, 0x4f);
const SURFACE1: Color = Color::Rgb(0x49, 0x4d, 0x64);
const OVERLAY0: Color = Color::Rgb(0x6e, 0x73, 0x8d);
const SUBTEXT0: Color = Color::Rgb(0xa5, 0xad, 0xcb);
const TEXT: Color = Color::Rgb(0xca, 0xd3, 0xf5);
const BLUE: Color = Color::Rgb(0x8a, 0xad, 0xf4);
const LAVENDER: Color = Color::Rgb(0xb7, 0xbd, 0xf8);
const MAUVE: Color = Color::Rgb(0xc6, 0xa0, 0xf6);
const RED: Color = Color::Rgb(0xed, 0x87, 0x96);
const PEACH: Color = Color::Rgb(0xf5, 0xa9, 0x7f);
const YELLOW: Color = Color::Rgb(0xee, 0xd4, 0x9f);
const GREEN: Color = Color::Rgb(0xa6, 0xda, 0x95);
const TEAL: Color = Color::Rgb(0x8b, 0xd5, 0xca);
const CLAUDE: Color = Color::Rgb(0xd9, 0x77, 0x57);

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Width of the logo column in cells; graphics are composited over it.
pub const LOGO_COLS: u16 = 2;
/// Left padding before the logo column.
pub const PAD: u16 = 1;
/// Cells each tree level indents by.
const INDENT: usize = 2;

pub fn status_color(status: Status) -> Color {
    match status {
        Status::Blocked => RED,
        Status::Done => GREEN,
        Status::Working => YELLOW,
        Status::Idle => OVERLAY0,
        Status::Unknown => SURFACE1,
    }
}

fn status_glyph(status: Status, tick: usize) -> &'static str {
    match status {
        Status::Working => SPINNER[tick % SPINNER.len()],
        Status::Unknown => "○",
        _ => "●",
    }
}

fn fallback_logo(item: &Item) -> Span<'static> {
    let Some(agent) = &item.agent else {
        return Span::styled(format!("{:<2}", "$"), Style::new().fg(OVERLAY0));
    };
    let (glyph, color) = match item.harness {
        Harness::Claude => ("✻".to_string(), CLAUDE),
        Harness::Codex => ("◎".to_string(), TEXT),
        Harness::OpenCode => ("⌬".to_string(), TEAL),
        Harness::Pi => ("π".to_string(), MAUVE),
        Harness::Copilot => ("◉".to_string(), BLUE),
        Harness::Other => (
            agent
                .chars()
                .next()
                .map(|c| c.to_ascii_uppercase().to_string())
                .unwrap_or_else(|| "?".into()),
            LAVENDER,
        ),
    };
    Span::styled(format!("{glyph:<2}"), Style::new().fg(color).bold())
}

pub fn relative_time(then_ms: u64, now_ms: u64) -> String {
    if then_ms == 0 {
        return String::new();
    }
    let secs = now_ms.saturating_sub(then_ms) / 1000;
    match secs {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86_399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

/// Where a logo should be drawn, in popup-local cell coordinates. Tree rows
/// put logos at several indents, so `x` varies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogoSlot {
    pub x: u16,
    pub y: u16,
    pub harness: Harness,
}

#[derive(Default)]
pub struct UiState {
    pub offset: usize,
    pub tick: usize,
    /// Harnesses drawn as graphics; their rows leave the logo column blank.
    pub image_logos: HashSet<Harness>,
    /// Filled during render: visible logo slots, top to bottom.
    pub logo_slots: Vec<LogoSlot>,
}

pub fn render(frame: &mut Frame, app: &App, ui: &mut UiState) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(BG).fg(TEXT)), area);
    if area.height < 4 || area.width < 20 {
        return;
    }
    let header = Rect { height: 1, ..area };
    let tabs = Rect {
        y: area.y + 1,
        height: 1,
        ..area
    };
    let footer = Rect {
        y: area.bottom() - 1,
        height: 1,
        ..area
    };
    let list = Rect {
        y: area.y + 2,
        height: area.height - 3,
        ..area
    };
    render_header(frame, app, header);
    render_tabs(frame, app, tabs);
    render_list(frame, app, ui, list);
    render_footer(frame, app, footer);
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::raw(" ".repeat(PAD as usize))];
    match app.mode {
        Mode::Search => {
            spans.push(Span::styled("› ", Style::new().fg(MAUVE).bold()));
            spans.push(Span::styled(app.query.clone(), Style::new().fg(TEXT)));
            spans.push(Span::styled("▏", Style::new().fg(MAUVE)));
        }
        Mode::Normal if app.query.is_empty() => {
            spans.push(Span::styled("/ search", Style::new().fg(OVERLAY0)));
        }
        Mode::Normal => {
            spans.push(Span::styled("/ ", Style::new().fg(OVERLAY0)));
            spans.push(Span::styled(app.query.clone(), Style::new().fg(SUBTEXT0)));
        }
    }

    let chip = |label: String, active: bool, color: Color| {
        if active {
            Span::styled(format!(" {label} "), Style::new().fg(BG).bg(color).bold())
        } else {
            Span::styled(format!(" {label} "), Style::new().fg(OVERLAY0))
        }
    };
    let mut chips = vec![chip(
        format!("all {}", app.agent_count()),
        app.filter.is_none(),
        LAVENDER,
    )];
    for (status, key) in [
        (Status::Blocked, 'b'),
        (Status::Done, 'd'),
        (Status::Working, 'w'),
        (Status::Idle, 'i'),
    ] {
        let label = format!("{key}·{} {}", status.word(), app.count(status));
        chips.push(chip(
            label,
            app.filter == Some(status),
            status_color(status),
        ));
    }
    let chips_width: usize = chips.iter().map(|s| s.width()).sum::<usize>() + PAD as usize;
    let left_width: usize = spans.iter().map(|s| s.width()).sum();
    let gap = (area.width as usize).saturating_sub(left_width + chips_width);
    if gap > 0 {
        spans.push(Span::raw(" ".repeat(gap)));
        spans.extend(chips);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The rule under the header, doubling as the view tab bar:
/// `─ agents ─┤ workspaces ├─ projects ───`.
fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let rule = Style::new().fg(SURFACE0);
    let mut spans = vec![Span::styled("─", rule)];
    for (i, view) in View::ALL.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("─", rule));
        }
        let name = format!(" {} ", view.name());
        if view == app.view {
            spans.push(Span::styled("┤", rule));
            spans.push(Span::styled(name, Style::new().fg(LAVENDER).bold()));
            spans.push(Span::styled("├", rule));
        } else {
            spans.push(Span::styled(name, Style::new().fg(OVERLAY0)));
        }
    }
    let used: usize = spans.iter().map(|s| s.width()).sum();
    spans.push(Span::styled(
        "─".repeat((area.width as usize).saturating_sub(used)),
        rule,
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let keys: &[(&str, &str)] = match app.mode {
        Mode::Normal => &[
            ("j/k", "move"),
            ("h/l", "view"),
            ("/", "search"),
            ("b d w i", "filter"),
            ("a", "all"),
            ("⏎", "jump"),
            ("esc", "close"),
        ],
        Mode::Search => &[("↑/↓ ^n/^p", "move"), ("⏎", "jump"), ("esc", "normal")],
    };
    let mut spans = vec![Span::raw(" ".repeat(PAD as usize))];
    for (key, what) in keys {
        spans.push(Span::styled(*key, Style::new().fg(SUBTEXT0).bold()));
        spans.push(Span::styled(
            format!(" {what}   "),
            Style::new().fg(OVERLAY0),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_list(frame: &mut Frame, app: &App, ui: &mut UiState, area: Rect) {
    ui.logo_slots.clear();
    if app.rows.is_empty() {
        let msg = if app.query.is_empty() && app.filter.is_none() {
            app.view.empty_message()
        } else {
            "no matches"
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(" ".repeat(PAD as usize + 1)),
                Span::styled(msg, Style::new().fg(OVERLAY0).italic()),
            ])),
            area,
        );
        return;
    }

    let hint = app.selected_row().and_then(|r| hint_line(app, r));
    let height = area.height as usize;
    // Keep the selected row (and its hint line) inside the viewport. Group
    // lines can't be selected, so scrolling up also reveals the ones
    // directly above the selected pane.
    let needed = if hint.is_some() { 2 } else { 1 };
    let headers = app.rows[..app.selected.min(app.rows.len())]
        .iter()
        .rposition(|r| r.item().is_some())
        .map_or(0, |idx| idx + 1);
    let top = headers.max((app.selected + needed).saturating_sub(height));
    if top < ui.offset {
        ui.offset = top;
    } else if app.selected + needed > ui.offset + height {
        ui.offset = (app.selected + needed).saturating_sub(height);
    }
    ui.offset = ui.offset.min(app.rows.len().saturating_sub(1));

    let now = now_ms();
    let visible = || app.rows.iter().skip(ui.offset).take(height);
    let items: Vec<&Item> = visible()
        .filter_map(Row::item)
        .map(|i| &app.items[i])
        .collect();
    let badges = visible()
        .filter_map(|r| r.summary.as_ref())
        .map(|s| badge_spans(s, 0).iter().map(Span::width).sum())
        .max()
        .unwrap_or(0);
    let cols = Columns::fit(app.view, &items, badges, area.width);
    let mut y = area.y;
    for (idx, row) in app.rows.iter().enumerate().skip(ui.offset) {
        if y >= area.bottom() {
            break;
        }
        let selected = idx == app.selected;
        let line = match (row.item(), &row.summary) {
            (Some(i), _) => {
                let item = &app.items[i];
                if ui.image_logos.contains(&item.harness) && item.is_agent() {
                    ui.logo_slots.push(LogoSlot {
                        x: area.x + PAD + (INDENT * row.depth as usize) as u16,
                        y,
                        harness: item.harness,
                    });
                }
                pane_line(item, row, selected, ui, now, &cols)
            }
            (None, Some(summary)) => group_line(row, summary, ui.tick, now, &cols, area.width),
            (None, None) => Line::default(),
        };
        let mut style = Style::new();
        if selected {
            style = style.bg(SURFACE0);
        }
        frame.render_widget(
            Paragraph::new(line).style(style),
            Rect {
                y,
                height: 1,
                ..area
            },
        );
        y += 1;
        if selected
            && let Some(hint) = &hint
            && y < area.bottom()
        {
            frame.render_widget(
                Paragraph::new(hint.clone()).style(Style::new().bg(SURFACE0)),
                Rect {
                    y,
                    height: 1,
                    ..area
                },
            );
            y += 1;
        }
    }
}

/// Splits `text` into spans, highlighting matched char indices, truncated to
/// `width` cells (with an ellipsis) and padded to exactly `width`.
fn highlighted(text: &str, indices: &[u32], width: usize, base: Style) -> Vec<Span<'static>> {
    let hl = base.fg(PEACH).add_modifier(Modifier::BOLD);
    let full = text.width();
    let budget = if full > width {
        width.saturating_sub(1)
    } else {
        width
    };
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    let mut current = String::new();
    let mut current_hl = false;
    for (i, ch) in text.chars().enumerate() {
        let w = ch.width().unwrap_or(0);
        if used + w > budget {
            break;
        }
        used += w;
        let is_hl = indices.binary_search(&(i as u32)).is_ok();
        if is_hl != current_hl && !current.is_empty() {
            spans.push(Span::styled(
                std::mem::take(&mut current),
                if current_hl { hl } else { base },
            ));
        }
        current_hl = is_hl;
        current.push(ch);
    }
    if !current.is_empty() {
        spans.push(Span::styled(current, if current_hl { hl } else { base }));
    }
    if full > width && width > 0 {
        spans.push(Span::styled("…", base.fg(OVERLAY0)));
        used += 1;
    }
    if used < width {
        spans.push(Span::raw(" ".repeat(width - used)));
    }
    spans
}

/// Column widths shared by every row in a frame so columns line up. Each
/// view drops the columns its tree already conveys.
struct Columns {
    /// Title width at depth 0; deeper rows give up their indent.
    title: usize,
    /// `workspace › tab` widths (agents and projects views).
    location: Option<(usize, usize)>,
    /// Folder width (agents and workspaces views).
    folder: Option<usize>,
    /// Badge column width on group lines.
    badges: usize,
}

impl Columns {
    /// `[pad][indent][logo][sp][dot][sp] title␣␣workspace␣›␣tab␣␣folder␣time[pad]`
    fn fit(view: View, items: &[&Item], badges: usize, width: u16) -> Columns {
        let has_location = matches!(view, View::Agents | View::Projects);
        let has_folder = matches!(view, View::Agents | View::Workspaces);
        let mut fixed = PAD as usize + LOGO_COLS as usize + 3 + 5 + PAD as usize;
        if has_location {
            fixed += 2 + 3;
        }
        if has_folder {
            fixed += 2;
        }
        let flex = (width as usize).saturating_sub(fixed);
        let widest = |f: fn(&Item) -> usize| items.iter().map(|i| f(i)).max().unwrap_or(0);
        let location = has_location.then(|| {
            (
                widest(|i| i.workspace.width()).min(18).min(flex / 6),
                widest(|i| i.tab.width()).min(30).min(flex / 4),
            )
        });
        let folder = has_folder.then(|| widest(|i| i.folder_hint().width()).min(20).min(flex / 6));
        let (workspace, tab) = location.unwrap_or_default();
        Columns {
            title: flex.saturating_sub(workspace + tab + folder.unwrap_or_default()),
            location,
            folder,
            badges,
        }
    }
}

fn pane_line(
    item: &Item,
    row: &Row,
    selected: bool,
    ui: &UiState,
    now: u64,
    cols: &Columns,
) -> Line<'static> {
    let matched = row.matched.as_ref();
    let idx = |f: Field| matched.map(|m| m.indices(f)).unwrap_or(&[]);
    let indent = INDENT * row.depth as usize;
    let time = if item.focused {
        "here".to_string()
    } else {
        relative_time(item.times.last_activity_ms(), now)
    };
    let folder_field = if item.git.is_some() {
        Field::Project
    } else {
        Field::Folder
    };
    let title_style = match (item.focused, selected) {
        (true, _) => Style::new().fg(OVERLAY0),
        (false, true) => Style::new().fg(TEXT).bold(),
        (false, false) => Style::new().fg(TEXT),
    };
    let dim = Style::new().fg(SUBTEXT0);
    let faint = Style::new().fg(OVERLAY0);

    let mut spans = vec![Span::raw(" ".repeat(PAD as usize + indent))];
    if item.is_agent() && ui.image_logos.contains(&item.harness) {
        spans.push(Span::raw(" ".repeat(LOGO_COLS as usize)));
    } else {
        spans.push(fallback_logo(item));
    }
    spans.push(Span::raw(" "));
    if item.is_agent() {
        spans.push(Span::styled(
            status_glyph(item.status, ui.tick),
            Style::new().fg(status_color(item.status)),
        ));
    } else {
        spans.push(Span::raw(" "));
    }
    spans.push(Span::raw(" "));
    spans.extend(highlighted(
        &item.title,
        idx(Field::Title),
        cols.title.saturating_sub(indent),
        title_style,
    ));
    if let Some((workspace, tab)) = cols.location {
        spans.push(Span::raw("  "));
        spans.extend(highlighted(
            &item.workspace,
            idx(Field::Workspace),
            workspace,
            dim,
        ));
        spans.push(Span::styled(" › ", faint));
        spans.extend(highlighted(&item.tab, idx(Field::Tab), tab, dim));
    }
    if let Some(folder) = cols.folder {
        spans.push(Span::raw("  "));
        spans.extend(highlighted(
            item.folder_hint(),
            idx(folder_field),
            folder,
            faint,
        ));
    }
    spans.push(Span::styled(format!(" {time:>4}"), faint));
    Line::from(spans)
}

/// Non-zero agent counts as colored badges in tier order: `●1 ⠹2 ●3`.
fn badge_spans(summary: &Summary, tick: usize) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (status, &n) in BADGES.iter().zip(&summary.counts) {
        if n == 0 {
            continue;
        }
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            format!("{}{n}", status_glyph(*status, tick)),
            Style::new().fg(status_color(*status)),
        ));
    }
    spans
}

/// `[pad][indent]label  detail … badges time[pad]`, with badges and time
/// aligned to the pane rows' time column.
fn group_line(
    row: &Row,
    summary: &Summary,
    tick: usize,
    now: u64,
    cols: &Columns,
    width: u16,
) -> Line<'static> {
    let indent = INDENT * row.depth as usize;
    let label_style = match row.node {
        Node::Workspace(_) | Node::Repo(_) => Style::new().fg(TEXT).bold(),
        Node::Tab(_) | Node::Worktree(_) => Style::new().fg(SUBTEXT0),
        Node::Folder(_) | Node::Pane(_) => Style::new().fg(OVERLAY0),
    };
    let right = cols.badges + 5;
    let avail = (width as usize).saturating_sub(2 * PAD as usize + indent + right + 1);

    let mut spans = vec![Span::raw(" ".repeat(PAD as usize + indent))];
    let mut used = summary.label.width().min(avail);
    spans.extend(highlighted(&summary.label, &[], used, label_style));
    if let Some(detail) = &summary.detail {
        let room = avail.saturating_sub(used + 2);
        if room > 0 {
            let w = detail.width().min(room);
            spans.push(Span::raw("  "));
            spans.extend(highlighted(detail, &[], w, Style::new().fg(SUBTEXT0)));
            used += 2 + w;
        }
    }
    spans.push(Span::raw(" ".repeat(avail - used + 1)));
    let badges = badge_spans(summary, tick);
    let badges_width: usize = badges.iter().map(Span::width).sum();
    spans.extend(badges);
    spans.push(Span::raw(
        " ".repeat(cols.badges.saturating_sub(badges_width)),
    ));
    let time = relative_time(summary.last_activity_ms, now);
    spans.push(Span::styled(
        format!(" {time:>4}"),
        Style::new().fg(OVERLAY0),
    ));
    Line::from(spans)
}

/// A second line under the selected row when a query term matched best in a
/// field the row does not show (agent kind, branch, full path, …).
fn hint_line(app: &App, row: &Row) -> Option<Line<'static>> {
    let matched = row.matched.as_ref()?;
    let field = matched.hidden_hit()?;
    let item = &app.items[row.item()?];
    let text = crate::search::fields(item)
        .into_iter()
        .find(|(f, _)| *f == field)?
        .1
        .to_string();
    let indent = PAD as usize + INDENT * row.depth as usize + LOGO_COLS as usize + 3;
    let mut spans = vec![
        Span::raw(" ".repeat(indent)),
        Span::styled(format!("↳ {}: ", field.label()), Style::new().fg(OVERLAY0)),
    ];
    spans.extend(highlighted(
        &text,
        matched.indices(field),
        200,
        Style::new().fg(SUBTEXT0),
    ));
    Some(Line::from(spans))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_relative_time() {
        let now = 10_000_000;
        assert_eq!(relative_time(0, now), "");
        assert_eq!(relative_time(now - 5_000, now), "now");
        assert_eq!(relative_time(now - 120_000, now), "2m");
        assert_eq!(relative_time(now - 7_200_000, now), "2h");
    }

    #[test]
    fn highlight_truncates_and_pads() {
        let spans = highlighted("abcdef", &[1], 4, Style::new());
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "abc…");
        let spans = highlighted("ab", &[], 4, Style::new());
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "ab  ");
    }
}
