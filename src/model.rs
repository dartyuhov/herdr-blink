//! Picker rows built from one `session.snapshot` plus blink's own state.

use std::{collections::HashMap, path::PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::{
    git::{GitCache, GitInfo},
    state::{PaneTimes, State},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Blocked,
    Done,
    Working,
    Idle,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Harness {
    Claude,
    Codex,
    OpenCode,
    Pi,
    Copilot,
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

#[derive(Debug, Clone)]
pub struct Item {
    pub pane_id: String,
    pub harness: Harness,
    /// Raw agent id from herdr (`claude`, `codex`, …), used for search.
    pub agent: String,
    pub status: Status,
    pub title: String,
    pub workspace: String,
    pub tab: String,
    pub cwd: String,
    pub git: Option<GitInfo>,
    pub times: PaneTimes,
    /// Position in the snapshot, the final stable tie-breaker.
    pub order: usize,
}

impl Item {
    /// Short folder hint shown in the row: project if in a repo, else the
    /// cwd basename.
    pub fn folder_hint(&self) -> &str {
        match &self.git {
            Some(git) if !git.project.is_empty() => &git.project,
            _ => basename(&self.cwd),
        }
    }
}

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
    agents: Vec<AgentRec>,
    #[serde(default)]
    panes: Vec<PaneRec>,
    #[serde(default)]
    tabs: Vec<TabRec>,
    #[serde(default)]
    workspaces: Vec<WorkspaceRec>,
}

#[derive(Debug, Default, Deserialize)]
struct AgentRec {
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
    name: Option<String>,
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
struct PaneRec {
    pane_id: String,
    #[serde(default)]
    label: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct TabRec {
    tab_id: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    number: Option<u64>,
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

/// Agent panes across all workspaces, minus the focused pane.
pub fn build_items(snapshot: &Value, state: &State, git: &mut GitCache) -> Vec<Item> {
    let snap = Snapshot::deserialize(snapshot).unwrap_or_default();
    let pane_labels: HashMap<&str, &str> = snap
        .panes
        .iter()
        .filter_map(|p| Some((p.pane_id.as_str(), non_empty(&p.label)?)))
        .collect();
    let tab_labels: HashMap<&str, String> = snap
        .tabs
        .iter()
        .map(|t| {
            let label = non_empty(&t.label)
                .map(str::to_string)
                .unwrap_or_else(|| format!("tab {}", t.number.unwrap_or(0)));
            (t.tab_id.as_str(), label)
        })
        .collect();
    let ws_labels: HashMap<&str, String> = snap
        .workspaces
        .iter()
        .map(|w| {
            let label = non_empty(&w.label)
                .map(str::to_string)
                .unwrap_or_else(|| format!("workspace {}", w.number.unwrap_or(0)));
            (w.workspace_id.as_str(), label)
        })
        .collect();
    let focused = snap.focused_pane_id.as_deref();

    snap.agents
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.focused && Some(a.pane_id.as_str()) != focused)
        .map(|(order, a)| {
            let agent = non_empty(&a.agent)
                .or(non_empty(&a.display_agent))
                .unwrap_or("agent")
                .to_string();
            let cwd = non_empty(&a.foreground_cwd)
                .or(non_empty(&a.cwd))
                .unwrap_or_default()
                .to_string();
            let title = non_empty(&a.terminal_title_stripped)
                .or_else(|| pane_labels.get(a.pane_id.as_str()).copied())
                .or(non_empty(&a.name))
                .unwrap_or(&agent)
                .to_string();
            Item {
                pane_id: a.pane_id.clone(),
                harness: Harness::detect(&agent),
                status: Status::parse(a.agent_status.as_deref().unwrap_or("unknown")),
                title,
                workspace: ws_labels
                    .get(a.workspace_id.as_str())
                    .cloned()
                    .unwrap_or_default(),
                tab: tab_labels
                    .get(a.tab_id.as_str())
                    .cloned()
                    .unwrap_or_default(),
                git: (!cwd.is_empty())
                    .then(|| git.lookup(&PathBuf::from(&cwd)))
                    .flatten(),
                cwd,
                times: state.get(&a.pane_id).cloned().unwrap_or_default(),
                agent,
                order,
            }
        })
        .collect()
}

/// Empty-query ordering: status tier, then MRU (last focus, then last status
/// change), then snapshot order.
pub fn tier_mru_cmp(a: &Item, b: &Item) -> std::cmp::Ordering {
    a.status
        .tier()
        .cmp(&b.status.tier())
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
            "agents": [
                {"pane_id": "w1:p1", "agent": "claude", "agent_status": "idle", "focused": true,
                 "tab_id": "w1:t1", "workspace_id": "w1"},
                {"pane_id": "w1:p2", "agent": "codex", "agent_status": "working",
                 "terminal_title_stripped": "Fix bug", "cwd": "/nope/x", "tab_id": "w1:t1", "workspace_id": "w1"},
                {"pane_id": "w2:p1", "agent": "opencode", "agent_status": "blocked",
                 "terminal_title_stripped": "", "tab_id": "w2:t1", "workspace_id": "w2"},
                {"pane_id": "w2:p2", "agent": "pi", "agent_status": "idle",
                 "terminal_title_stripped": "pi", "tab_id": "w2:t1", "workspace_id": "w2"}
            ],
            "panes": [{"pane_id": "w2:p1", "label": "OpenCode"}],
            "tabs": [{"tab_id": "w1:t1", "label": "main", "number": 1},
                     {"tab_id": "w2:t1", "label": null, "number": 3}],
            "workspaces": [{"workspace_id": "w1", "label": "blink"},
                           {"workspace_id": "w2", "label": "other"}]
        })
    }

    #[test]
    fn builds_items_without_focused_pane() {
        let items = build_items(&snapshot(), &State::new(), &mut GitCache::default());
        let ids: Vec<_> = items.iter().map(|i| i.pane_id.as_str()).collect();
        assert_eq!(ids, ["w1:p2", "w2:p1", "w2:p2"]);
        assert_eq!(items[0].harness, Harness::Codex);
        assert_eq!(items[0].title, "Fix bug");
        assert_eq!(items[0].workspace, "blink");
        assert_eq!(items[0].tab, "main");
        assert_eq!(items[0].folder_hint(), "x");
        assert_eq!(items[1].title, "OpenCode", "falls back to pane label");
        assert_eq!(items[1].tab, "tab 3");
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
            ..items[2].clone()
        });
        items.sort_by(tier_mru_cmp);
        let ids: Vec<_> = items.iter().map(|i| i.pane_id.as_str()).collect();
        assert_eq!(ids, ["w2:p1", "w1:p2", "w2:p2", "w3:p1"]);
    }
}
