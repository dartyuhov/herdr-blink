//! Picker items, one per pane, built from one `session.snapshot` plus
//! blink's own state.

use std::{collections::HashMap, path::PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::{
    git::{GitCache, GitInfo},
    state::{PaneTimes, State},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Blocked,
    Done,
    Working,
    Idle,
    #[default]
    Unknown,
}

impl Status {
    pub fn parse(s: &str) -> Status {
        match s {
            "blocked" => Status::Blocked,
            "done" => Status::Done,
            "working" => Status::Working,
            "idle" => Status::Idle,
            _ => Status::Unknown,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Status::Blocked => "blocked",
            Status::Done => "done",
            Status::Working => "working",
            Status::Idle => "idle",
            Status::Unknown => "unknown",
        }
    }

    /// Sort tier for the empty-query ordering; lower comes first.
    pub fn tier(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Harness {
    Claude,
    Codex,
    OpenCode,
    Pi,
    Copilot,
    #[default]
    Other,
}

impl Harness {
    pub fn detect(agent: &str) -> Harness {
        let a = agent.to_ascii_lowercase();
        match a.as_str() {
            "claude" | "claude-code" | "claude_code" => Harness::Claude,
            "codex" => Harness::Codex,
            "opencode" => Harness::OpenCode,
            "pi" => Harness::Pi,
            "copilot" | "github-copilot" | "copilot-cli" => Harness::Copilot,
            _ if a.contains("claude") => Harness::Claude,
            _ if a.contains("copilot") => Harness::Copilot,
            _ => Harness::Other,
        }
    }

    /// Asset file stem under `assets/logos/`.
    pub fn logo_name(self) -> Option<&'static str> {
        match self {
            Harness::Claude => Some("claude"),
            Harness::Codex => Some("codex"),
            Harness::OpenCode => Some("opencode"),
            Harness::Pi => Some("pi"),
            Harness::Copilot => Some("copilot"),
            Harness::Other => None,
        }
    }
}

/// One pane: an agent, or a plain shell when `agent` is `None`.
#[derive(Debug, Clone, Default)]
pub struct Item {
    pub pane_id: String,
    pub tab_id: String,
    pub workspace_id: String,
    pub harness: Harness,
    /// Raw agent id from herdr (`claude`, `codex`, …), used for search.
    pub agent: Option<String>,
    pub status: Status,
    pub title: String,
    /// Workspace label; empty when the workspace is missing from the
    /// snapshot.
    pub workspace: String,
    /// Tab label (`tab N` when unset); empty when the tab is missing.
    pub tab: String,
    pub workspace_number: Option<u64>,
    /// 1-based position of the tab in its workspace. herdr's own tab
    /// `number` is a creation counter, not a position.
    pub tab_number: Option<u64>,
    pub cwd: String,
    pub git: Option<GitInfo>,
    pub times: PaneTimes,
    /// The pane the popup was opened from.
    pub focused: bool,
    /// Reserved for remote machines; always `None` for now.
    #[allow(dead_code)]
    pub machine: Option<String>,
    /// Position in the snapshot, the final stable tie-breaker.
    pub order: usize,
}

impl Item {
    pub fn is_agent(&self) -> bool {
        self.agent.is_some()
    }

    /// Tab line label: the number, then the label when one is set
    /// (`2 api`). A label that already starts with the number (`2. api`)
    /// is shown as is.
    pub fn tab_line(&self) -> String {
        match self.tab_number {
            Some(n) if self.tab == format!("tab {n}") => n.to_string(),
            Some(n) if starts_with_number(&self.tab, n) => self.tab.clone(),
            Some(n) => format!("{n} {}", self.tab),
            None if self.tab.is_empty() => UNKNOWN.into(),
            None => self.tab.clone(),
        }
    }

    /// Short folder hint shown in the row: project if in a repo, else the
    /// cwd basename.
    pub fn folder_hint(&self) -> &str {
        match &self.git {
            Some(git) if !git.project.is_empty() => &git.project,
            _ => basename(&self.cwd),
        }
    }
}

fn starts_with_number(label: &str, n: u64) -> bool {
    label
        .strip_prefix(&n.to_string())
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_digit()))
}

/// Label of a group whose identity is missing from the snapshot.
pub const UNKNOWN: &str = "(unknown)";

pub fn basename(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
}

#[derive(Debug, Default, Deserialize)]
struct Snapshot {
    #[serde(default)]
    focused_pane_id: Option<String>,
    #[serde(default)]
    panes: Vec<PaneRec>,
    #[serde(default)]
    tabs: Vec<TabRec>,
    #[serde(default)]
    workspaces: Vec<WorkspaceRec>,
}

#[derive(Debug, Default, Deserialize)]
struct PaneRec {
    pane_id: String,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    display_agent: Option<String>,
    #[serde(default)]
    agent_status: Option<String>,
    #[serde(default)]
    focused: bool,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    terminal_title_stripped: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    foreground_cwd: Option<String>,
    #[serde(default)]
    tab_id: String,
    #[serde(default)]
    workspace_id: String,
}

#[derive(Debug, Default, Deserialize)]
struct TabRec {
    tab_id: String,
    #[serde(default)]
    workspace_id: String,
    #[serde(default)]
    label: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct WorkspaceRec {
    workspace_id: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    number: Option<u64>,
}

fn non_empty(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// Every pane across all workspaces, plain shells and the focused pane
/// included; each view decides what to show.
pub fn build_items(snapshot: &Value, state: &State, git: &mut GitCache) -> Vec<Item> {
    let snap = Snapshot::deserialize(snapshot).unwrap_or_default();
    let mut positions: HashMap<&str, u64> = HashMap::new();
    let tabs: HashMap<&str, (String, Option<u64>)> = snap
        .tabs
        .iter()
        .map(|t| {
            let position = positions.entry(t.workspace_id.as_str()).or_default();
            *position += 1;
            let label = non_empty(&t.label)
                .map(str::to_string)
                .unwrap_or_else(|| format!("tab {position}"));
            (t.tab_id.as_str(), (label, Some(*position)))
        })
        .collect();
    let workspaces: HashMap<&str, (String, Option<u64>)> = snap
        .workspaces
        .iter()
        .map(|w| {
            let label = non_empty(&w.label)
                .map(str::to_string)
                .unwrap_or_else(|| format!("workspace {}", w.number.unwrap_or(0)));
            (w.workspace_id.as_str(), (label, w.number))
        })
        .collect();
    let focused = snap.focused_pane_id.as_deref();

    snap.panes
        .iter()
        .enumerate()
        .map(|(order, p)| {
            let agent = non_empty(&p.agent)
                .or(non_empty(&p.display_agent))
                .map(str::to_string);
            let cwd = non_empty(&p.foreground_cwd)
                .or(non_empty(&p.cwd))
                .unwrap_or_default()
                .to_string();
            let title = non_empty(&p.terminal_title_stripped)
                .or(non_empty(&p.label))
                .or(agent.as_deref())
                .unwrap_or("shell")
                .to_string();
            let (tab, tab_number) = tabs.get(p.tab_id.as_str()).cloned().unwrap_or_default();
            let (workspace, workspace_number) = workspaces
                .get(p.workspace_id.as_str())
                .cloned()
                .unwrap_or_default();
            Item {
                pane_id: p.pane_id.clone(),
                tab_id: p.tab_id.clone(),
                workspace_id: p.workspace_id.clone(),
                harness: agent.as_deref().map_or(Harness::Other, Harness::detect),
                status: match &agent {
                    Some(_) => Status::parse(p.agent_status.as_deref().unwrap_or("unknown")),
                    None => Status::Unknown,
                },
                title,
                workspace,
                tab,
                workspace_number,
                tab_number,
                git: (!cwd.is_empty())
                    .then(|| git.lookup(&PathBuf::from(&cwd)))
                    .flatten(),
                cwd,
                times: state.get(&p.pane_id).cloned().unwrap_or_default(),
                focused: p.focused || Some(p.pane_id.as_str()) == focused,
                machine: None,
                agent,
                order,
            }
        })
        .collect()
}

/// Empty-query ordering: agents before plain shells, then status tier, then
/// MRU (last focus, then last status change), then snapshot order.
pub fn tier_mru_cmp(a: &Item, b: &Item) -> std::cmp::Ordering {
    (!a.is_agent())
        .cmp(&!b.is_agent())
        .then(a.status.tier().cmp(&b.status.tier()))
        .then(b.times.last_focused_ms.cmp(&a.times.last_focused_ms))
        .then(
            b.times
                .last_status_change_ms
                .cmp(&a.times.last_status_change_ms),
        )
        .then(a.order.cmp(&b.order))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot() -> Value {
        json!({
            "focused_pane_id": "w1:p1",
            "panes": [
                {"pane_id": "w1:p1", "agent": "claude", "agent_status": "idle", "focused": true,
                 "tab_id": "w1:t1", "workspace_id": "w1"},
                {"pane_id": "w1:p2", "agent": "codex", "agent_status": "working",
                 "terminal_title_stripped": "Fix bug", "cwd": "/nope/x", "tab_id": "w1:t1", "workspace_id": "w1"},
                {"pane_id": "w2:p1", "agent": "opencode", "agent_status": "blocked", "label": "OpenCode",
                 "terminal_title_stripped": "", "tab_id": "w2:t1", "workspace_id": "w2"},
                {"pane_id": "w2:p2", "agent": "pi", "agent_status": "idle",
                 "terminal_title_stripped": "pi", "tab_id": "w2:t1", "workspace_id": "w2"},
                {"pane_id": "w2:p3", "agent": null, "agent_status": "unknown",
                 "terminal_title_stripped": "zsh", "tab_id": "w2:t9", "workspace_id": "w9"}
            ],
            "tabs": [{"tab_id": "w1:t1", "workspace_id": "w1", "label": "main", "number": 1},
                     {"tab_id": "w2:t0", "workspace_id": "w2", "label": "2. logs", "number": 5},
                     {"tab_id": "w2:t1", "workspace_id": "w2", "label": null, "number": 9}],
            "workspaces": [{"workspace_id": "w1", "label": "blink", "number": 1},
                           {"workspace_id": "w2", "label": "other", "number": 2}]
        })
    }

    fn ids(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.pane_id.as_str()).collect()
    }

    #[test]
    fn builds_one_item_per_pane() {
        let items = build_items(&snapshot(), &State::new(), &mut GitCache::default());
        assert_eq!(ids(&items), ["w1:p1", "w1:p2", "w2:p1", "w2:p2", "w2:p3"]);
        assert!(items[0].focused, "focused pane is kept and marked");
        assert!(!items[1].focused);
        assert_eq!(items[1].harness, Harness::Codex);
        assert_eq!(items[1].agent.as_deref(), Some("codex"));
        assert_eq!(items[1].title, "Fix bug");
        assert_eq!(items[1].workspace, "blink");
        assert_eq!(items[1].workspace_number, Some(1));
        assert_eq!(items[1].tab, "main");
        assert_eq!(items[1].tab_line(), "1 main");
        assert_eq!(items[1].folder_hint(), "x");
        assert_eq!(items[2].title, "OpenCode", "falls back to pane label");
        assert_eq!(items[2].tab, "tab 2", "position, not herdr's number");
        assert_eq!(items[2].tab_line(), "2");
        let logs = Item {
            tab: "1. logs".into(),
            tab_number: Some(1),
            ..Default::default()
        };
        assert_eq!(logs.tab_line(), "1. logs", "label already numbered");
        let big = Item {
            tab: "12 logs".into(),
            tab_number: Some(1),
            ..Default::default()
        };
        assert_eq!(big.tab_line(), "1 12 logs");
    }

    #[test]
    fn plain_shells_have_no_agent() {
        let items = build_items(&snapshot(), &State::new(), &mut GitCache::default());
        let shell = &items[4];
        assert_eq!(shell.agent, None);
        assert_eq!(shell.status, Status::Unknown);
        assert_eq!(shell.title, "zsh");
        assert_eq!(shell.workspace, "", "workspace missing from the snapshot");
        assert_eq!(shell.tab_line(), UNKNOWN);
    }

    #[test]
    fn orders_by_tier_then_mru() {
        let mut state = State::new();
        state.insert(
            "w2:p2".into(),
            PaneTimes {
                last_focused_ms: 5,
                ..Default::default()
            },
        );
        let mut items = build_items(&snapshot(), &state, &mut GitCache::default());
        items.push(Item {
            pane_id: "w3:p1".into(),
            order: 9,
            ..items[3].clone()
        });
        items.sort_by(tier_mru_cmp);
        assert_eq!(
            ids(&items),
            ["w2:p1", "w1:p2", "w2:p2", "w3:p1", "w1:p1", "w2:p3"],
            "plain shells sort after every agent tier"
        );
    }
}
