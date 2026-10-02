//! Minimal herdr socket client: newline-delimited JSON over a Unix socket.
//!
//! Talking to the socket directly (instead of spawning `herdr` CLI processes)
//! keeps the open path to a single round-trip.

use std::{
    env,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

use serde_json::{Value, json};

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    /// The server answered with an error object (`code`, `message`).
    Api {
        code: String,
        message: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "herdr socket: {e}"),
            Error::Json(e) => write!(f, "herdr response: {e}"),
            Error::Api { code, message } => write!(f, "herdr: {code}: {message}"),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Socket resolution mirrors herdr: `HERDR_SOCKET_PATH`, then
/// `HERDR_SESSION`, then the default session socket.
pub fn socket_path() -> PathBuf {
    if let Some(path) = env::var_os("HERDR_SOCKET_PATH").filter(|p| !p.is_empty()) {
        return PathBuf::from(path);
    }
    let config = config_dir();
    match env::var("HERDR_SESSION") {
        Ok(name) if !name.is_empty() => config.join("sessions").join(name).join("herdr.sock"),
        _ => config.join("herdr.sock"),
    }
}

/// herdr's config root: `$XDG_CONFIG_HOME/herdr`, else `~/.config/herdr`.
pub fn config_dir() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default()
        .join("herdr")
}

/// Sends one request on a fresh connection and returns its `result` object.
pub fn request(method: &str, params: Value) -> Result<Value> {
    let mut stream = UnixStream::connect(socket_path())?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut line = serde_json::to_vec(&json!({
        "id": format!("blink:{method}"),
        "method": method,
        "params": params,
    }))?;
    line.push(b'\n');
    stream.write_all(&line)?;

    let mut reader = BufReader::new(stream);
    let mut response = Vec::new();
    reader.read_until(b'\n', &mut response)?;
    let mut value: Value = serde_json::from_slice(&response)?;
    if let Some(error) = value.get("error") {
        return Err(Error::Api {
            code: error
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        });
    }
    Ok(value
        .get_mut("result")
        .map(Value::take)
        .unwrap_or(Value::Null))
}

pub fn snapshot() -> Result<Value> {
    let mut result = request("session.snapshot", json!({}))?;
    Ok(result
        .get_mut("snapshot")
        .map(Value::take)
        .unwrap_or(Value::Null))
}

pub fn focus_agent(pane_id: &str) -> Result<()> {
    request("agent.focus", json!({ "target": pane_id })).map(drop)
}

pub fn focus_pane(pane_id: &str) -> Result<()> {
    request("pane.focus", json!({ "pane_id": pane_id })).map(drop)
}

pub fn focus_tab(tab_id: &str) -> Result<()> {
    request("tab.focus", json!({ "tab_id": tab_id })).map(drop)
}

pub fn focus_workspace(workspace_id: &str) -> Result<()> {
    request("workspace.focus", json!({ "workspace_id": workspace_id })).map(drop)
}

pub fn graphics_info(pane_id: &str) -> Result<Value> {
    request("pane.graphics.info", json!({ "pane_id": pane_id }))
}

pub fn open_plugin_pane(plugin_id: &str, entrypoint: &str) -> Result<()> {
    request(
        "plugin.pane.open",
        json!({ "plugin_id": plugin_id, "entrypoint": entrypoint, "focus": true }),
    )
    .map(drop)
}
