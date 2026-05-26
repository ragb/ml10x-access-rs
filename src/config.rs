//! TOML configuration loader for the `ml10x` CLI.
//!
//! Reads `~/.ml10x.toml` (or the file given to `--config`, or
//! `$ML10X_CONFIG`) and exposes named defaults the CLI then merges with
//! command-line options.
//!
//! Missing file is **not** an error — falls back to built-in defaults.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Could not read configuration file {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Could not read configuration file {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
}

/// Resolved configuration. Fields use the same names as the matching CLI
/// option so callers can pass them through without name translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub port: String,
    pub config_path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: "ML10X".to_string(),
            config_path: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    #[serde(default)]
    device: DeviceSection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceSection {
    #[serde(default)]
    port: Option<String>,
}

pub fn default_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".ml10x.toml"))
}

impl Config {
    pub fn load(explicit_path: Option<&Path>) -> Result<Self, ConfigError> {
        let path: Option<PathBuf> = explicit_path.map(|p| p.to_path_buf()).or_else(|| {
            if let Some(env) = std::env::var_os("ML10X_CONFIG") {
                Some(PathBuf::from(env))
            } else {
                default_config_path()
            }
        });

        let Some(path) = path else {
            return Ok(Self::default());
        };

        if !path.exists() {
            // Silent miss for the default path is fine; explicit path
            // missing is the same — caller can check `config_path` to
            // tell.
            return Ok(Self::default());
        }

        let text = std::fs::read_to_string(&path).map_err(|e| ConfigError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        let file: ConfigFile = toml::from_str(&text).map_err(|e| ConfigError::Parse {
            path: path.display().to_string(),
            source: e,
        })?;

        let mut cfg = Self::default();
        cfg.config_path = Some(path);
        if let Some(p) = file.device.port {
            cfg.port = p;
        }
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp(name: &str, body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ml10x-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        p
    }

    #[test]
    fn default_when_file_missing() {
        let nope = std::env::temp_dir().join("ml10x-no-such-file.toml");
        let _ = std::fs::remove_file(&nope);
        let cfg = Config::load(Some(&nope)).unwrap();
        assert_eq!(cfg.port, "ML10X");
        assert_eq!(cfg.config_path, None);
    }

    #[test]
    fn loads_device_port() {
        let p = write_tmp("ml10x-load.toml", "[device]\nport = \"Morningstar\"\n");
        let cfg = Config::load(Some(&p)).unwrap();
        assert_eq!(cfg.port, "Morningstar");
        assert_eq!(cfg.config_path, Some(p.clone()));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn rejects_unknown_keys() {
        let p = write_tmp("ml10x-unknown.toml", "[device]\nport = \"X\"\nfoo = \"bar\"\n");
        let err = Config::load(Some(&p)).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn rejects_wrong_type() {
        let p = write_tmp("ml10x-wrongtype.toml", "[device]\nport = 42\n");
        let err = Config::load(Some(&p)).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn rejects_malformed_toml() {
        let p = write_tmp("ml10x-malformed.toml", "[device\nport = \"X\"\n");
        let err = Config::load(Some(&p)).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        let _ = std::fs::remove_file(&p);
    }
}
