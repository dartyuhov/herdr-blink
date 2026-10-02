//! Picker state and key handling, independent of rendering.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    model::{Item, Status, tier_mru_cmp},
    search::{MatchResult, Searcher},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Search,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    Jump(String),
}

pub struct Row {
    pub item: usize,
    pub matched: Option<MatchResult>,
}

pub struct App {
    pub items: Vec<Item>,
    pub rows: Vec<Row>,
    pub mode: Mode,
    pub filter: Option<Status>,
    pub query: String,
    pub selected: usize,
    searcher: Searcher,
}

impl App {
    pub fn new(items: Vec<Item>) -> App {
        let mut app = App {
            items,
            rows: Vec::new(),
            mode: Mode::Normal,
            filter: None,
            query: String::new(),
            selected: 0,
            searcher: Searcher::default(),
        };
        app.refilter(false);
        app
    }

    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
        self.refilter(false);
    }

    pub fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }

    pub fn selected_item(&self) -> Option<&Item> {
        self.selected_row().map(|r| &self.items[r.item])
    }

    pub fn count(&self, status: Status) -> usize {
        self.items.iter().filter(|i| i.status == status).count()
    }

    /// Recomputes visible rows. With `keep_selection`, the selected pane
    /// stays selected if it survives; otherwise the top row is selected.
    fn refilter(&mut self, keep_selection: bool) {
        let previous = keep_selection
            .then(|| self.selected_item().map(|i| i.pane_id.clone()))
            .flatten();
        self.searcher.set_query(&self.query);
        let items = &self.items;
        let mut rows: Vec<Row> = items
            .iter()
            .enumerate()
            .filter(|(_, item)| self.filter.is_none_or(|f| item.status == f))
            .filter_map(|(idx, item)| {
                if self.searcher.is_empty() {
                    return Some(Row {
                        item: idx,
                        matched: None,
                    });
                }
                let matched = self.searcher.match_item(item)?;
                Some(Row {
                    item: idx,
                    matched: Some(matched),
                })
            })
            .collect();
        rows.sort_by(|a, b| {
            let score = |r: &Row| r.matched.as_ref().map_or(0, |m| m.score);
            score(b)
                .cmp(&score(a))
                .then_with(|| tier_mru_cmp(&items[a.item], &items[b.item]))
        });
        self.rows = rows;
        self.selected = previous
            .and_then(|id| {
                self.rows
                    .iter()
                    .position(|r| self.items[r.item].pane_id == id)
            })
            .unwrap_or(0);
    }

    fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            self.selected = 0;
            return;
        }
        let last = self.rows.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
    }

    fn toggle_filter(&mut self, status: Status) {
        self.filter = if self.filter == Some(status) {
            None
        } else {
            Some(status)
        };
        self.refilter(true);
    }

    fn jump(&self) -> Action {
        self.selected_item()
            .map(|i| Action::Jump(i.pane_id.clone()))
            .unwrap_or(Action::None)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            return Action::Quit;
        }
        match self.mode {
            Mode::Normal => self.handle_normal(key),
            Mode::Search => self.handle_search(key, ctrl),
        }
    }

    fn handle_normal(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            // Reserved for cycling views; no-op in v1.
            KeyCode::Char('h') | KeyCode::Char('l') => {}
            KeyCode::Char('/') => self.mode = Mode::Search,
            KeyCode::Char('b') => self.toggle_filter(Status::Blocked),
            KeyCode::Char('d') => self.toggle_filter(Status::Done),
            KeyCode::Char('w') => self.toggle_filter(Status::Working),
            KeyCode::Char('i') => self.toggle_filter(Status::Idle),
            KeyCode::Char('a') => {
                self.filter = None;
                self.refilter(true);
            }
            KeyCode::Enter => return self.jump(),
            KeyCode::Esc | KeyCode::Char('q') => return Action::Quit,
            _ => {}
        }
        Action::None
    }

    fn handle_search(&mut self, key: KeyEvent, ctrl: bool) -> Action {
        match key.code {
            KeyCode::Down => self.move_selection(1),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('n') if ctrl => self.move_selection(1),
            KeyCode::Char('p') if ctrl => self.move_selection(-1),
            KeyCode::Char('u') if ctrl => {
                self.query.clear();
                self.refilter(false);
            }
            KeyCode::Char('w') if ctrl => {
                let trimmed = self.query.trim_end().len();
                let cut = self.query[..trimmed].rfind(' ').map_or(0, |i| i + 1);
                self.query.truncate(cut);
                self.refilter(false);
            }
            KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                self.query.push(c);
                self.refilter(false);
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.refilter(false);
            }
            KeyCode::Enter => return self.jump(),
            KeyCode::Esc => self.mode = Mode::Normal,
            _ => {}
        }
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::Harness, state::PaneTimes};

    fn item(id: &str, status: Status, focused_ms: u64) -> Item {
        Item {
            pane_id: id.into(),
            harness: Harness::Claude,
            agent: "claude".into(),
            status,
            title: format!("task {id}"),
            workspace: "ws".into(),
            tab: "tab".into(),
            cwd: "/tmp".into(),
            git: None,
            times: PaneTimes {
                last_focused_ms: focused_ms,
                ..Default::default()
            },
            order: 0,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ids(app: &App) -> Vec<&str> {
        app.rows
            .iter()
            .map(|r| app.items[r.item].pane_id.as_str())
            .collect()
    }

    fn app() -> App {
        App::new(vec![
            item("idle-old", Status::Idle, 1),
            item("working", Status::Working, 0),
            item("idle-new", Status::Idle, 9),
            item("done", Status::Done, 0),
            item("blocked", Status::Blocked, 0),
            item("unknown", Status::Unknown, 99),
        ])
    }

    #[test]
    fn empty_query_orders_tiers_then_mru() {
        assert_eq!(
            ids(&app()),
            [
                "blocked", "done", "working", "idle-new", "idle-old", "unknown"
            ]
        );
    }

    #[test]
    fn filters_toggle_and_clear() {
        let mut app = app();
        app.handle_key(key(KeyCode::Char('i')));
        assert_eq!(ids(&app), ["idle-new", "idle-old"]);
        app.handle_key(key(KeyCode::Char('i')));
        assert_eq!(app.rows.len(), 6);
        app.handle_key(key(KeyCode::Char('w')));
        assert_eq!(ids(&app), ["working"]);
        app.handle_key(key(KeyCode::Char('a')));
        assert_eq!(app.filter, None);
    }

    #[test]
    fn search_mode_types_and_esc_keeps_query() {
        let mut app = app();
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected, 1);
        app.handle_key(key(KeyCode::Char('/')));
        for c in "old".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(ids(&app), ["idle-old"]);
        assert_eq!(app.selected, 0);
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.query, "old");
        assert_eq!(
            app.handle_key(key(KeyCode::Enter)),
            Action::Jump("idle-old".into())
        );
        assert_eq!(app.handle_key(key(KeyCode::Char('q'))), Action::Quit);
    }

    #[test]
    fn search_applies_inside_filter() {
        let mut app = app();
        app.handle_key(key(KeyCode::Char('i')));
        app.handle_key(key(KeyCode::Char('/')));
        for c in "work".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        assert!(ids(&app).is_empty(), "'working' is filtered out");
    }
}
