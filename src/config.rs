//! User settings from `config.toml` in the plugin's config directory.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::{PLUGIN_ID, herdr, view::View};

#[derive(Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The view the picker opens on.
    pub default_view: View,
}

/// `HERDR_PLUGIN_CONFIG_DIR`, else the directory `herdr plugin config-dir`
/// prints, so `list` run from a shell sees the same file.
pub fn config_dir() -> PathBuf {
    if let Some(dir) = env::var_os("HERDR_PLUGIN_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    herdr::config_dir().join("plugins/config").join(PLUGIN_ID)
}

/// A missing file is the default config.
pub fn load(dir: &Path) -> Result<Config, String> {
    let path = dir.join("config.toml");
    match fs::read_to_string(&path) {
        Ok(text) => parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn parse(text: &str) -> Result<Config, toml::de::Error> {
    toml::from_str(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_default() {
        assert_eq!(parse("").unwrap(), Config::default());
        assert_eq!(Config::default().default_view, View::Agents);
    }

    #[test]
    fn default_view_parses() {
        let config = parse("default_view = \"workspaces\"").unwrap();
        assert_eq!(config.default_view, View::Workspaces);
        let config = parse("default_view = \"projects\"").unwrap();
        assert_eq!(config.default_view, View::Projects);
    }

    #[test]
    fn unknown_view_is_an_error() {
        assert!(parse("default_view = \"tabs\"").is_err());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert_eq!(parse("future = 1").unwrap(), Config::default());
    }

    #[test]
    fn missing_file_is_default() {
        let dir = env::temp_dir().join(format!("blink-config-{}", std::process::id()));
        assert_eq!(load(&dir).unwrap(), Config::default());
    }
}
