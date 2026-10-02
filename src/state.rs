//! Focus / status-change timestamps that the snapshot does not carry.
//!
//! `[[events]]` hooks run `herdr-blink event`, which records the time into
//! `$HERDR_PLUGIN_STATE_DIR/state.json`. Hooks can run concurrently, so every
//! read-modify-write holds an exclusive lock on a sibling lock file and the
//! new content is written to a temp file and renamed into place.

use std::{
    collections::{HashMap, HashSet},
    env,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneTimes {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub last_focused_ms: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub last_status_change_ms: u64,
    /// Last seen status word; herdr also emits `agent_status_changed` for
    /// presentation-only changes, which must not count as activity.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_status: String,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

impl PaneTimes {
    pub fn last_activity_ms(&self) -> u64 {
        self.last_focused_ms.max(self.last_status_change_ms)
    }
}

pub type State = HashMap<String, PaneTimes>;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn state_dir() -> PathBuf {
    if let Some(dir) = env::var_os("HERDR_PLUGIN_STATE_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let base = env::var_os("XDG_STATE_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_default();
    base.join("herdr-blink")
}

fn state_file(dir: &Path) -> PathBuf {
    dir.join("state.json")
}

/// Lock-free read for the popup's open path; a torn read is impossible
/// because writers rename complete files into place.
pub fn load(dir: &Path) -> State {
    fs::read(state_file(dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Applies `f` to the state under an exclusive lock and persists it if `f`
/// reports a change.
pub fn update(dir: &Path, f: impl FnOnce(&mut State) -> bool) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("state.lock"))?;
    lock.lock()?;
    let mut state = load(dir);
    if f(&mut state) {
        write_atomic(&state_file(dir), &serde_json::to_vec(&state)?)?;
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let mut file = File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_data()?;
    fs::rename(&tmp, path)
}

#[derive(Debug, PartialEq, Eq)]
pub enum EventKind {
    Focused,
    /// Carries the new status word when the payload has one.
    StatusChanged(String),
}

/// Extracts what we track from a hook's event name and payload. herdr sends
/// an `{event, data}` envelope; bare event data is accepted too.
pub fn parse_event(name: &str, payload: &str) -> Option<(EventKind, String)> {
    let value: Value = serde_json::from_str(payload).ok()?;
    let data = value.get("data").unwrap_or(&value);
    let field = |key: &str| data.get(key).and_then(Value::as_str);
    let kind = match name {
        "pane.focused" => EventKind::Focused,
        "pane.agent_status_changed" => {
            EventKind::StatusChanged(field("agent_status").unwrap_or_default().to_string())
        }
        _ => return None,
    };
    Some((kind, field("pane_id")?.to_string()))
}

/// Returns whether the state changed.
pub fn apply_event(state: &mut State, kind: EventKind, pane_id: String, now: u64) -> bool {
    let entry = state.entry(pane_id).or_default();
    match kind {
        EventKind::Focused => entry.last_focused_ms = now,
        EventKind::StatusChanged(status) => {
            if !status.is_empty() && status == entry.last_status {
                return false;
            }
            entry.last_status = status;
            entry.last_status_change_ms = now;
        }
    }
    true
}

/// Drops entries for panes that no longer exist. Returns whether anything
/// was removed.
pub fn prune(state: &mut State, live: &HashSet<&str>) -> bool {
    let before = state.len();
    state.retain(|id, _| live.contains(id.as_str()));
    state.len() != before
}

/// `herdr-blink event`: invoked by `[[events]]` hooks.
pub fn run_event_hook() -> std::io::Result<()> {
    let name = env::var("HERDR_PLUGIN_EVENT").unwrap_or_default();
    let payload = env::var("HERDR_PLUGIN_EVENT_JSON").unwrap_or_default();
    let Some((kind, pane_id)) = parse_event(&name, &payload) else {
        return Ok(());
    };
    let now = now_ms();
    update(&state_dir(), |state| apply_event(state, kind, pane_id, now))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_and_enveloped_payloads() {
        assert_eq!(
            parse_event(
                "pane.focused",
                r#"{"type":"pane_focused","pane_id":"w1:p2"}"#
            ),
            Some((EventKind::Focused, "w1:p2".into()))
        );
        assert_eq!(
            parse_event(
                "pane.agent_status_changed",
                r#"{"event":"pane_agent_status_changed","data":{"type":"pane_agent_status_changed","pane_id":"w1:p3","workspace_id":"w1","agent_status":"done"}}"#
            ),
            Some((EventKind::StatusChanged("done".into()), "w1:p3".into()))
        );
        assert_eq!(parse_event("tab.focused", r#"{"pane_id":"w1:p2"}"#), None);
        assert_eq!(parse_event("pane.focused", "not json"), None);
    }

    #[test]
    fn update_is_atomic_and_prunes() {
        let dir = env::temp_dir().join(format!("blink-state-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        update(&dir, |s| {
            apply_event(s, EventKind::Focused, "w1:p1".into(), 10)
        })
        .unwrap();
        let working = || EventKind::StatusChanged("working".into());
        update(&dir, |s| apply_event(s, working(), "w1:p1".into(), 20)).unwrap();
        // Presentation-only change: same status, no new activity.
        update(&dir, |s| apply_event(s, working(), "w1:p1".into(), 25)).unwrap();
        update(&dir, |s| {
            apply_event(s, EventKind::Focused, "w1:p2".into(), 30)
        })
        .unwrap();
        let state = load(&dir);
        assert_eq!(
            state["w1:p1"],
            PaneTimes {
                last_focused_ms: 10,
                last_status_change_ms: 20,
                last_status: "working".into()
            }
        );
        assert_eq!(state["w1:p1"].last_activity_ms(), 20);

        update(&dir, |s| prune(s, &HashSet::from(["w1:p2"]))).unwrap();
        let state = load(&dir);
        assert!(!state.contains_key("w1:p1"));
        assert!(state.contains_key("w1:p2"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
