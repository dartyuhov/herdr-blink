//! Picker state and key handling, independent of rendering.

use std::cmp::Reverse;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    model::{Item, Status},
    search::Searcher,
    view::{self, Row, Target, View},
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
    Jump(Target),
}

pub struct App {
    pub items: Vec<Item>,
    pub rows: Vec<Row>,
    pub view: View,
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
            view: View::Agents,
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

    pub fn set_view(&mut self, view: View) {
        self.view = view;
        self.refilter(true);
    }

    pub fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }

    #[cfg(test)]
    pub fn selected_item(&self) -> Option<&Item> {
        self.selected_row()?.item().map(|i| &self.items[i])
    }

    /// Agent panes other than the focused one: the filter chips' universe,
    /// identical in every view.
    fn other_agents(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().filter(|i| i.is_agent() && !i.focused)
    }

    pub fn agent_count(&self) -> usize {
        self.other_agents().count()
    }

    pub fn count(&self, status: Status) -> usize {
        self.other_agents().filter(|i| i.status == status).count()
    }

    /// Whether any visible row shows the working spinner.
    pub fn animating(&self) -> bool {
        let working = view::BADGES
            .iter()
            .position(|&s| s == Status::Working)
            .unwrap_or_default();
        self.rows.iter().any(|r| match (r.item(), &r.summary) {
            (Some(i), _) => self.items[i].is_agent() && self.items[i].status == Status::Working,
            (None, Some(s)) => s.counts[working] > 0,
            (None, None) => false,
        })
    }

    /// Recomputes visible rows. With `keep_selection`, the selected node
    /// stays selected if it survives. Otherwise the best-scoring pane row is
    /// selected when a query is active, and the first row when it is not.
    fn refilter(&mut self, keep_selection: bool) {
        let previous = keep_selection
            .then(|| self.selected_row().map(|r| r.node.clone()))
            .flatten();
        self.searcher.set_query(&self.query);
        let matches: Option<Vec<_>> = (!self.searcher.is_empty()).then(|| {
            self.items
                .iter()
                .map(|i| self.searcher.match_item(i))
                .collect()
        });
        self.rows = view::build_rows(self.view, &self.items, matches.as_deref(), self.filter);
        self.selected = previous
            .and_then(|node| self.rows.iter().position(|r| r.node == node))
            .unwrap_or_else(|| self.best_row());
    }

    /// First row among those with the highest match score; 0 without a
    /// query.
    fn best_row(&self) -> usize {
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(idx, r)| Some((idx, r.matched.as_ref()?.score)))
            .min_by_key(|&(_, score)| Reverse(score))
            .map_or(0, |(idx, _)| idx)
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
        view::target(&self.items, &self.rows, self.selected)
            .map(Action::Jump)
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
            KeyCode::Char('h' | '[') => self.set_view(self.view.prev()),
            KeyCode::Char('l' | ']') => self.set_view(self.view.next()),
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
            // Brackets switch views in search mode too, so they never reach
            // the query.
            KeyCode::Char('[') => self.set_view(self.view.prev()),
            KeyCode::Char(']') => self.set_view(self.view.next()),
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
    use crate::{model::Harness, state::PaneTimes, view::Node};

    fn item(id: &str, status: Status, focused_ms: u64) -> Item {
        Item {
            pane_id: id.into(),
            workspace_id: "w1".into(),
            tab_id: "w1:t1".into(),
            harness: Harness::Claude,
            agent: Some("claude".into()),
            status,
            title: format!("task {id}"),
            workspace: "ws".into(),
            tab: "tab".into(),
            cwd: "/tmp".into(),
            times: PaneTimes {
                last_focused_ms: focused_ms,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_str(app: &mut App, s: &str) {
        for c in s.chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
    }

    fn ids(app: &App) -> Vec<&str> {
        app.rows
            .iter()
            .filter_map(|r| r.item())
            .map(|i| app.items[i].pane_id.as_str())
            .collect()
    }

    fn items() -> Vec<Item> {
        vec![
            item("idle-old", Status::Idle, 1),
            item("working", Status::Working, 0),
            item("idle-new", Status::Idle, 9),
            item("done", Status::Done, 0),
            item("blocked", Status::Blocked, 0),
            item("unknown", Status::Unknown, 99),
        ]
    }

    fn app() -> App {
        App::new(items())
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
        type_str(&mut app, "old");
        assert_eq!(ids(&app), ["idle-old"]);
        assert_eq!(app.selected, 0);
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.query, "old");
        assert_eq!(
            app.handle_key(key(KeyCode::Enter)),
            Action::Jump(Target::Agent("idle-old".into()))
        );
        assert_eq!(app.handle_key(key(KeyCode::Char('q'))), Action::Quit);
    }

    #[test]
    fn search_applies_inside_filter() {
        let mut app = app();
        app.handle_key(key(KeyCode::Char('i')));
        app.handle_key(key(KeyCode::Char('/')));
        type_str(&mut app, "work");
        assert!(ids(&app).is_empty(), "'working' is filtered out");
    }

    #[test]
    fn h_and_l_cycle_views_in_normal_mode_only() {
        let mut app = app();
        app.handle_key(key(KeyCode::Char('l')));
        assert_eq!(app.view, View::Workspaces);
        app.handle_key(key(KeyCode::Char('l')));
        app.handle_key(key(KeyCode::Char('l')));
        assert_eq!(app.view, View::Agents, "wraps forward");
        app.handle_key(key(KeyCode::Char('h')));
        assert_eq!(app.view, View::Projects, "wraps backward");

        app.handle_key(key(KeyCode::Char('/')));
        type_str(&mut app, "hl");
        assert_eq!(app.view, View::Projects);
        assert_eq!(app.query, "hl");
    }

    #[test]
    fn brackets_cycle_views_in_both_modes() {
        let mut app = app();
        app.handle_key(key(KeyCode::Char(']')));
        assert_eq!(app.view, View::Workspaces);
        app.handle_key(key(KeyCode::Char('[')));
        app.handle_key(key(KeyCode::Char('[')));
        assert_eq!(app.view, View::Projects, "wraps backward");

        app.handle_key(key(KeyCode::Char('/')));
        type_str(&mut app, "ab");
        app.handle_key(key(KeyCode::Char(']')));
        assert_eq!(app.view, View::Agents, "wraps forward in search mode");
        assert_eq!(app.mode, Mode::Search);
        assert_eq!(app.query, "ab", "brackets are not typed into the query");
    }

    #[test]
    fn query_and_filter_carry_across_views() {
        let mut app = app();
        app.handle_key(key(KeyCode::Char('i')));
        app.set_query("new");
        app.handle_key(key(KeyCode::Char('l')));
        assert_eq!(app.filter, Some(Status::Idle));
        assert_eq!(app.query, "new");
        assert_eq!(ids(&app), ["idle-new"]);
    }

    #[test]
    fn selection_follows_the_pane_across_views() {
        let mut app = app();
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected_item().unwrap().pane_id, "done");
        app.handle_key(key(KeyCode::Char('l')));
        assert_eq!(app.selected_item().unwrap().pane_id, "done");
        app.handle_key(key(KeyCode::Char('h')));
        assert_eq!(app.selected_item().unwrap().pane_id, "done");
    }

    #[test]
    fn selection_falls_back_to_first_row_or_best_match() {
        let mut items = items();
        items[0].focused = true;
        let mut app = App::new(items);
        app.set_view(View::Workspaces);
        // Select the focused pane, which the agents view does not show.
        app.selected = app
            .rows
            .iter()
            .position(|r| r.node == Node::Pane(0))
            .unwrap();
        app.set_view(View::Agents);
        assert_eq!(app.selected, 0, "first row without a query");

        app.set_view(View::Workspaces);
        app.set_query("idle new");
        let best = app.selected_item().unwrap();
        assert_eq!(
            best.pane_id, "idle-new",
            "best-scoring pane, not the group line"
        );
        assert!(app.selected > 0);
    }

    #[test]
    fn chips_count_other_agents() {
        let mut items = items();
        items[0].focused = true;
        items.push(Item {
            agent: None,
            status: Status::Unknown,
            ..item("shell", Status::Unknown, 0)
        });
        let app = App::new(items);
        assert_eq!(app.agent_count(), 5);
        assert_eq!(app.count(Status::Idle), 1);
    }
}
