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
    app::{App, Mode, Row},
    model::{Harness, Item, Status},
    search::{Field, MatchResult},
    state::now_ms,
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
    let (glyph, color) = match item.harness {
        Harness::Claude => ("✻".to_string(), CLAUDE),
        Harness::Codex => ("◎".to_string(), TEXT),
        Harness::OpenCode => ("⌬".to_string(), TEAL),
        Harness::Pi => ("π".to_string(), MAUVE),
        Harness::Copilot => ("◉".to_string(), BLUE),
        Harness::Other => (
            item.agent
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

/// Where a logo should be drawn, in popup-local cell coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogoSlot {
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
    /// Filled during render: x/y of the logo column's first list row.
    pub logo_origin: (u16, u16),
}

pub fn render(frame: &mut Frame, app: &App, ui: &mut UiState) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(BG).fg(TEXT)), area);
    if area.height < 4 || area.width < 20 {
        return;
    }
    let header = Rect { height: 1, ..area };
    let rule = Rect {
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
    frame.render_widget(
        Paragraph::new("─".repeat(area.width as usize)).style(Style::new().fg(SURFACE0)),
        rule,
    );
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
        format!("all {}", app.items.len()),
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

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let keys: &[(&str, &str)] = match app.mode {
        Mode::Normal => &[
            ("j/k", "move"),
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
    ui.logo_origin = (area.x + PAD, area.y);
    if app.rows.is_empty() {
        let msg = if app.items.is_empty() {
            "no other agents"
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
    // Keep the selected row (and its hint line) inside the viewport.
    let needed = if hint.is_some() { 2 } else { 1 };
    if app.selected < ui.offset {
        ui.offset = app.selected;
    } else if app.selected + needed > ui.offset + height {
        ui.offset = (app.selected + needed).saturating_sub(height);
    }
    ui.offset = ui.offset.min(app.rows.len().saturating_sub(1));

    let now = now_ms();
    let visible: Vec<&Item> = app
        .rows
        .iter()
        .skip(ui.offset)
        .take(height)
        .map(|r| &app.items[r.item])
        .collect();
    let cols = Columns::fit(&visible, area.width);
    let mut y = area.y;
    for (idx, row) in app.rows.iter().enumerate().skip(ui.offset) {
        if y >= area.bottom() {
            break;
        }
        let item = &app.items[row.item];
        let selected = idx == app.selected;
        let line = row_line(item, row.matched.as_ref(), selected, ui, now, &cols);
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
        ui.logo_slots.push(LogoSlot {
            y: y - area.y,
            harness: item.harness,
        });
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

/// Column widths shared by every row in a frame so columns line up.
struct Columns {
    title: usize,
    workspace: usize,
    tab: usize,
    folder: usize,
}

impl Columns {
    /// `[pad][logo][sp][dot][sp] title␣␣workspace␣›␣tab␣␣folder␣time[pad]`
    fn fit(items: &[&Item], width: u16) -> Columns {
        let fixed = PAD as usize + LOGO_COLS as usize + 3 + 2 + 3 + 2 + 5 + PAD as usize;
        let flex = (width as usize).saturating_sub(fixed);
        let widest = |f: fn(&Item) -> usize| items.iter().map(|i| f(i)).max().unwrap_or(0);
        let workspace = widest(|i| i.workspace.width()).min(18).min(flex / 6);
        let tab = widest(|i| i.tab.width()).min(30).min(flex / 4);
        let folder = widest(|i| i.folder_hint().width()).min(20).min(flex / 6);
        Columns {
            title: flex.saturating_sub(workspace + tab + folder),
            workspace,
            tab,
            folder,
        }
    }
}

fn row_line(
    item: &Item,
    matched: Option<&MatchResult>,
    selected: bool,
    ui: &UiState,
    now: u64,
    cols: &Columns,
) -> Line<'static> {
    let idx = |f: Field| matched.map(|m| m.indices(f)).unwrap_or(&[]);
    let time = relative_time(item.times.last_activity_ms(), now);
    let folder_field = if item.git.is_some() {
        Field::Project
    } else {
        Field::Folder
    };
    let title_style = if selected {
        Style::new().fg(TEXT).bold()
    } else {
        Style::new().fg(TEXT)
    };
    let dim = Style::new().fg(SUBTEXT0);
    let faint = Style::new().fg(OVERLAY0);

    let mut spans = vec![Span::raw(" ".repeat(PAD as usize))];
    if ui.image_logos.contains(&item.harness) {
        spans.push(Span::raw(" ".repeat(LOGO_COLS as usize)));
    } else {
        spans.push(fallback_logo(item));
    }
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        status_glyph(item.status, ui.tick),
        Style::new().fg(status_color(item.status)),
    ));
    spans.push(Span::raw(" "));
    spans.extend(highlighted(
        &item.title,
        idx(Field::Title),
        cols.title,
        title_style,
    ));
    spans.push(Span::raw("  "));
    spans.extend(highlighted(
        &item.workspace,
        idx(Field::Workspace),
        cols.workspace,
        dim,
    ));
    spans.push(Span::styled(" › ", faint));
    spans.extend(highlighted(&item.tab, idx(Field::Tab), cols.tab, dim));
    spans.push(Span::raw("  "));
    spans.extend(highlighted(
        item.folder_hint(),
        idx(folder_field),
        cols.folder,
        faint,
    ));
    spans.push(Span::styled(format!(" {time:>4}"), faint));
    Line::from(spans)
}

/// A second line under the selected row when a query term matched best in a
/// field the row does not show (agent kind, branch, full path, …).
fn hint_line(app: &App, row: &Row) -> Option<Line<'static>> {
    let matched = row.matched.as_ref()?;
    let field = matched.hidden_hit()?;
    let item = &app.items[row.item];
    let text = crate::search::fields(item)
        .into_iter()
        .find(|(f, _)| *f == field)?
        .1
        .to_string();
    let indent = PAD as usize + LOGO_COLS as usize + 3;
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
