//! File-I/O wrappers around `ml10x_core::yaml`.
//!
//! Read a preset or controller YAML from disk, or dump one to disk —
//! prepending the `# yaml-language-server: $schema=…` header line so VS
//! Code's YAML extension attaches the JSON schema for autocomplete and
//! validation. The string-only codec lives in core so the WASM build can
//! use it without dragging in `std::fs`.

use std::path::Path;

use thiserror::Error;

use ml10x_core::presets::{Controller, Preset};
use ml10x_core::yaml::{
    self, CONTROLLER_SCHEMA_URL, PRESET_SCHEMA_URL, YamlError as CoreYamlError,
};

#[derive(Debug, Error)]
pub enum YamlError {
    #[error("I/O error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Codec(#[from] CoreYamlError),
}

pub fn dump_preset_yaml(preset: &Preset, path: &Path) -> Result<(), YamlError> {
    create_parents(path)?;
    let body = yaml::preset_to_yaml_string(preset)?;
    let full = format!("# yaml-language-server: $schema={PRESET_SCHEMA_URL}\n{body}");
    std::fs::write(path, full).map_err(|e| YamlError::Io {
        path: path.display().to_string(),
        source: e,
    })
}

pub fn load_preset_yaml(path: &Path) -> Result<Preset, YamlError> {
    let text = std::fs::read_to_string(path).map_err(|e| YamlError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    Ok(yaml::preset_from_yaml_str(
        &text,
        &path.display().to_string(),
    )?)
}

pub fn dump_controller_yaml(controller: &Controller, path: &Path) -> Result<(), YamlError> {
    create_parents(path)?;
    let body = yaml::controller_to_yaml_string(controller)?;
    let full = format!("# yaml-language-server: $schema={CONTROLLER_SCHEMA_URL}\n{body}");
    std::fs::write(path, full).map_err(|e| YamlError::Io {
        path: path.display().to_string(),
        source: e,
    })
}

pub fn load_controller_yaml(path: &Path) -> Result<Controller, YamlError> {
    let text = std::fs::read_to_string(path).map_err(|e| YamlError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    Ok(yaml::controller_from_yaml_str(
        &text,
        &path.display().to_string(),
    )?)
}

fn create_parents(path: &Path) -> Result<(), YamlError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| YamlError::Io {
                path: parent.display().to_string(),
                source: e,
            })?;
        }
    }
    Ok(())
}
