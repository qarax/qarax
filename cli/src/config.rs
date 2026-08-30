use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    pub server: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("qarax").join("config.toml"))
}

pub fn load_from(path: &Path) -> Config {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Config::default();
    };
    toml::from_str(&contents).unwrap_or_default()
}

pub fn save_to(path: &Path, config: &Config) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let contents = toml::to_string_pretty(config).context("serialize config")?;
    std::fs::write(path, &contents).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn load() -> Config {
    config_path().map(|p| load_from(&p)).unwrap_or_default()
}

pub fn save(config: &Config) -> Result<()> {
    let path = config_path().context("could not determine config directory")?;
    save_to(&path, config)
}

pub fn path_display() -> String {
    config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unknown>".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn round_trips_server_url() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let cfg = Config {
            server: Some("http://example.com:8000".to_string()),
            token: None,
        };
        save_to(&path, &cfg).unwrap();

        let loaded = load_from(&path);
        assert_eq!(loaded.server.as_deref(), Some("http://example.com:8000"));
    }

    #[test]
    fn returns_default_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.toml");

        let cfg = load_from(&path);
        assert!(cfg.server.is_none());
    }

    #[test]
    fn returns_default_on_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "not [ valid toml !!!").unwrap();

        let cfg = load_from(&path);
        assert!(cfg.server.is_none());
    }

    #[test]
    fn creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("config.toml");

        let cfg = Config {
            server: Some("http://example.com".to_string()),
            token: None,
        };
        save_to(&path, &cfg).unwrap();
        assert!(path.exists());
    }
}
