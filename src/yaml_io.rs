//! YAML serialization for ML10X presets and controller settings.
//!
//! `serde_yml` doesn't preserve comments or key ordering on a load →
//! save round-trip; we accept that and instead guarantee:
//!
//! - Field ordering: explicit shim structs make the on-disk YAML read
//!   top-to-bottom under a screen reader (bank → number → name →
//!   spillover → body).
//! - YAML bank field is 1..4 (matches the editor UI); the in-memory
//!   `Preset.bank` stays 0..3 (matches the wire protocol). Translation
//!   happens in the shim conversions.
//! - A `# yaml-language-server: $schema=...` header line is prepended
//!   to each emitted file so VS Code's YAML extension attaches the JSON
//!   schema for autocomplete + validation.

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::presets::{
    Connection, Connector, Connectors, Controller, IncludeInTrails, Preset, PresetBody,
    SimpleHop, Spillover,
};

pub const PRESET_SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/ragb/ml10x-access-rs/main/schemas/preset.schema.json";
pub const CONTROLLER_SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/ragb/ml10x-access-rs/main/schemas/controller.schema.json";

#[derive(Debug, Error)]
pub enum YamlError {
    #[error("I/O error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("YAML parse error in {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_yml::Error,
    },
    #[error("YAML serialization error: {0}")]
    Serialize(#[from] serde_yml::Error),
    #[error("YAML file at {path}: preset.bank must be 1..4 (got {got})")]
    BadBank { path: String, got: u8 },
    #[error("YAML file at {path}: unknown preset field(s) {extras:?}.")]
    UnknownPresetField { path: String, extras: Vec<String> },
}

// ---------- Preset shim ----------

#[derive(Debug, Serialize, Deserialize)]
struct PresetDoc {
    preset: PresetYaml,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetYaml {
    bank: u8, // 1..4 in YAML, mapped to/from 0..3 internal
    number: u8,
    name: String,
    #[serde(default)]
    spillover: SpilloverYaml,
    body: PresetBodyYaml,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
enum PresetBodyYaml {
    Simple {
        #[serde(default)]
        chain: Vec<SimpleHopYaml>,
    },
    Advanced {
        #[serde(default)]
        connections: Vec<ConnectionYaml>,
    },
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpilloverYaml {
    #[serde(default)]
    output_tip: crate::presets::SpilloverTarget,
    #[serde(default)]
    output_ring: crate::presets::SpilloverTarget,
}

impl From<Spillover> for SpilloverYaml {
    fn from(s: Spillover) -> Self {
        Self {
            output_tip: s.output_tip,
            output_ring: s.output_ring,
        }
    }
}

impl From<SpilloverYaml> for Spillover {
    fn from(s: SpilloverYaml) -> Self {
        Spillover {
            output_tip: s.output_tip,
            output_ring: s.output_ring,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SimpleHopYaml {
    from_connector: crate::presets::ConnectorSlug,
    to_connector: crate::presets::ConnectorSlug,
    #[serde(default)]
    bypass: bool,
}

impl From<&SimpleHop> for SimpleHopYaml {
    fn from(h: &SimpleHop) -> Self {
        Self {
            from_connector: h.from_connector,
            to_connector: h.to_connector,
            bypass: h.bypass,
        }
    }
}

impl From<SimpleHopYaml> for SimpleHop {
    fn from(h: SimpleHopYaml) -> Self {
        SimpleHop {
            from_connector: h.from_connector,
            to_connector: h.to_connector,
            bypass: h.bypass,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectionYaml {
    from_connector: crate::presets::ConnectorSlug,
    to_connector: crate::presets::ConnectorSlug,
}

impl From<&Connection> for ConnectionYaml {
    fn from(c: &Connection) -> Self {
        Self {
            from_connector: c.from_connector,
            to_connector: c.to_connector,
        }
    }
}

impl From<ConnectionYaml> for Connection {
    fn from(c: ConnectionYaml) -> Self {
        Connection {
            from_connector: c.from_connector,
            to_connector: c.to_connector,
        }
    }
}

fn preset_to_yaml(preset: &Preset) -> PresetDoc {
    let body = match &preset.body {
        PresetBody::Simple { chain } => PresetBodyYaml::Simple {
            chain: chain.iter().map(SimpleHopYaml::from).collect(),
        },
        PresetBody::Advanced { connections } => PresetBodyYaml::Advanced {
            connections: connections.iter().map(ConnectionYaml::from).collect(),
        },
    };
    PresetDoc {
        preset: PresetYaml {
            bank: preset.bank + 1, // 0..3 internal → 1..4 in YAML
            number: preset.number,
            name: preset.name.clone(),
            spillover: preset.spillover.into(),
            body,
        },
    }
}

fn yaml_to_preset(doc: PresetDoc, source_for_errs: &str) -> Result<Preset, YamlError> {
    if !(1..=4).contains(&doc.preset.bank) {
        return Err(YamlError::BadBank {
            path: source_for_errs.to_string(),
            got: doc.preset.bank,
        });
    }
    let body = match doc.preset.body {
        PresetBodyYaml::Simple { chain } => PresetBody::Simple {
            chain: chain.into_iter().map(Into::into).collect(),
        },
        PresetBodyYaml::Advanced { connections } => PresetBody::Advanced {
            connections: connections.into_iter().map(Into::into).collect(),
        },
    };
    Ok(Preset {
        bank: doc.preset.bank - 1,
        number: doc.preset.number,
        name: doc.preset.name,
        spillover: doc.preset.spillover.into(),
        body,
    })
}

pub fn preset_yaml_string(preset: &Preset) -> Result<String, YamlError> {
    Ok(serde_yml::to_string(&preset_to_yaml(preset))?)
}

pub fn dump_preset_yaml(preset: &Preset, path: &Path) -> Result<(), YamlError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| YamlError::Io {
                path: parent.display().to_string(),
                source: e,
            })?;
        }
    }
    let body = preset_yaml_string(preset)?;
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
    load_preset_yaml_str(&text, &path.display().to_string())
}

pub fn load_preset_yaml_str(text: &str, source: &str) -> Result<Preset, YamlError> {
    let doc: PresetDoc = serde_yml::from_str(text).map_err(|e| YamlError::Parse {
        path: source.to_string(),
        source: e,
    })?;
    yaml_to_preset(doc, source)
}

// ---------- Controller shim ----------

#[derive(Debug, Serialize, Deserialize)]
struct ControllerDoc {
    controller: ControllerYaml,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerYaml {
    #[serde(default)]
    uuid: String,
    #[serde(default)]
    midi_channel: u8,
    #[serde(default)]
    device_id: u8,
    #[serde(default)]
    input_split: bool,
    #[serde(default)]
    loop_bypass_persistent: bool,
    #[serde(default)]
    include_in_trails: IncludeInTrails,
    #[serde(default)]
    connectors: ConnectorsYaml,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectorsYaml {
    #[serde(default)]
    a_tip: ConnectorYaml,
    #[serde(default)]
    a_ring: ConnectorYaml,
    #[serde(default)]
    b_tip: ConnectorYaml,
    #[serde(default)]
    b_ring: ConnectorYaml,
    #[serde(default)]
    c_tip: ConnectorYaml,
    #[serde(default)]
    c_ring: ConnectorYaml,
    #[serde(default)]
    d_tip: ConnectorYaml,
    #[serde(default)]
    d_ring: ConnectorYaml,
    #[serde(default)]
    e_tip: ConnectorYaml,
    #[serde(default)]
    e_ring: ConnectorYaml,
    #[serde(default)]
    input_tip: ConnectorYaml,
    #[serde(default)]
    input_ring: ConnectorYaml,
    #[serde(default)]
    output_tip: ConnectorYaml,
    #[serde(default)]
    output_ring: ConnectorYaml,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectorYaml {
    #[serde(default)]
    name: String,
    #[serde(default)]
    short_name: String,
    #[serde(default)]
    input_name: String,
    #[serde(default)]
    output_name: String,
}

impl From<&Connector> for ConnectorYaml {
    fn from(c: &Connector) -> Self {
        Self {
            name: c.name.clone(),
            short_name: c.short_name.clone(),
            input_name: c.input_name.clone(),
            output_name: c.output_name.clone(),
        }
    }
}

impl From<ConnectorYaml> for Connector {
    fn from(c: ConnectorYaml) -> Self {
        Connector {
            name: c.name,
            short_name: c.short_name,
            input_name: c.input_name,
            output_name: c.output_name,
        }
    }
}

impl From<&Connectors> for ConnectorsYaml {
    fn from(c: &Connectors) -> Self {
        Self {
            a_tip: (&c.a_tip).into(),
            a_ring: (&c.a_ring).into(),
            b_tip: (&c.b_tip).into(),
            b_ring: (&c.b_ring).into(),
            c_tip: (&c.c_tip).into(),
            c_ring: (&c.c_ring).into(),
            d_tip: (&c.d_tip).into(),
            d_ring: (&c.d_ring).into(),
            e_tip: (&c.e_tip).into(),
            e_ring: (&c.e_ring).into(),
            input_tip: (&c.input_tip).into(),
            input_ring: (&c.input_ring).into(),
            output_tip: (&c.output_tip).into(),
            output_ring: (&c.output_ring).into(),
        }
    }
}

impl From<ConnectorsYaml> for Connectors {
    fn from(c: ConnectorsYaml) -> Self {
        Connectors {
            a_tip: c.a_tip.into(),
            a_ring: c.a_ring.into(),
            b_tip: c.b_tip.into(),
            b_ring: c.b_ring.into(),
            c_tip: c.c_tip.into(),
            c_ring: c.c_ring.into(),
            d_tip: c.d_tip.into(),
            d_ring: c.d_ring.into(),
            e_tip: c.e_tip.into(),
            e_ring: c.e_ring.into(),
            input_tip: c.input_tip.into(),
            input_ring: c.input_ring.into(),
            output_tip: c.output_tip.into(),
            output_ring: c.output_ring.into(),
        }
    }
}

fn controller_to_yaml(controller: &Controller) -> ControllerDoc {
    ControllerDoc {
        controller: ControllerYaml {
            uuid: controller.uuid.clone(),
            midi_channel: controller.midi_channel,
            device_id: controller.device_id,
            input_split: controller.input_split,
            loop_bypass_persistent: controller.loop_bypass_persistent,
            include_in_trails: controller.include_in_trails,
            connectors: (&controller.connectors).into(),
        },
    }
}

pub fn controller_yaml_string(controller: &Controller) -> Result<String, YamlError> {
    Ok(serde_yml::to_string(&controller_to_yaml(controller))?)
}

pub fn dump_controller_yaml(controller: &Controller, path: &Path) -> Result<(), YamlError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| YamlError::Io {
                path: parent.display().to_string(),
                source: e,
            })?;
        }
    }
    let body = controller_yaml_string(controller)?;
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
    load_controller_yaml_str(&text, &path.display().to_string())
}

pub fn load_controller_yaml_str(text: &str, source: &str) -> Result<Controller, YamlError> {
    let doc: ControllerDoc = serde_yml::from_str(text).map_err(|e| YamlError::Parse {
        path: source.to_string(),
        source: e,
    })?;
    Ok(Controller {
        uuid: doc.controller.uuid,
        midi_channel: doc.controller.midi_channel,
        device_id: doc.controller.device_id,
        input_split: doc.controller.input_split,
        loop_bypass_persistent: doc.controller.loop_bypass_persistent,
        include_in_trails: doc.controller.include_in_trails,
        connectors: doc.controller.connectors.into(),
    })
}

// suppress unused-import warning for the IncludeInTrails import (the serde
// derive on ControllerYaml consumes the type but rustc still flags the import
// as unused when it's only used by field type)
#[allow(dead_code)]
fn _suppress_unused() {
    let _: IncludeInTrails = IncludeInTrails::default();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::{ConnectorSlug, SpilloverTarget};

    fn sample_preset() -> Preset {
        Preset {
            bank: 0, // internal 0..3
            number: 0,
            name: "Base".into(),
            spillover: Spillover::default(),
            body: PresetBody::Simple {
                chain: vec![
                    SimpleHop {
                        from_connector: ConnectorSlug::InputTip,
                        to_connector: ConnectorSlug::ATip,
                        bypass: false,
                    },
                    SimpleHop {
                        from_connector: ConnectorSlug::ATip,
                        to_connector: ConnectorSlug::OutputTip,
                        bypass: false,
                    },
                ],
            },
        }
    }

    fn sample_advanced_preset() -> Preset {
        Preset {
            bank: 0,
            number: 1,
            name: "Adv".into(),
            spillover: Spillover::default(),
            body: PresetBody::Advanced {
                connections: vec![
                    Connection {
                        from_connector: ConnectorSlug::InputTip,
                        to_connector: ConnectorSlug::OutputTip,
                    },
                    Connection {
                        from_connector: ConnectorSlug::InputTip,
                        to_connector: ConnectorSlug::OutputRing,
                    },
                ],
            },
        }
    }

    #[test]
    fn preset_yaml_starts_with_preset_top_level() {
        let yaml = preset_yaml_string(&sample_preset()).unwrap();
        assert!(yaml.starts_with("preset:"), "got: {yaml}");
    }

    #[test]
    fn preset_yaml_contains_expected_fields() {
        let yaml = preset_yaml_string(&sample_preset()).unwrap();
        for needle in ["bank:", "number:", "name:", "spillover:", "body:", "mode:", "chain:"] {
            assert!(yaml.contains(needle), "missing {needle:?} in:\n{yaml}");
        }
        // ML10X has no per-preset MIDI messages — the field must not appear.
        assert!(!yaml.contains("midi_messages"), "midi_messages leaked into YAML:\n{yaml}");
    }

    #[test]
    fn advanced_preset_yaml_uses_connections_field() {
        let yaml = preset_yaml_string(&sample_advanced_preset()).unwrap();
        assert!(yaml.contains("mode: advanced"), "got: {yaml}");
        assert!(yaml.contains("connections:"), "got: {yaml}");
        assert!(!yaml.contains("chain:"), "advanced preset should not have a chain field: {yaml}");
        assert!(!yaml.contains("bypass:"), "advanced preset should not have bypass: {yaml}");
    }

    #[test]
    fn advanced_preset_yaml_round_trips() {
        let p = sample_advanced_preset();
        let text = preset_yaml_string(&p).unwrap();
        let loaded = load_preset_yaml_str(&text, "<test>").unwrap();
        assert_eq!(loaded, p);
    }

    #[test]
    fn preset_yaml_bank_is_1_indexed() {
        // internal bank=0 should appear as "bank: 1" in YAML.
        let yaml = preset_yaml_string(&sample_preset()).unwrap();
        assert!(yaml.contains("bank: 1"), "expected 1-indexed bank in:\n{yaml}");
    }

    #[test]
    fn preset_yaml_chain_uses_readable_field_names() {
        let yaml = preset_yaml_string(&sample_preset()).unwrap();
        assert!(yaml.contains("from_connector:"));
        assert!(yaml.contains("to_connector:"));
        assert!(yaml.contains("bypass:"));
    }

    #[test]
    fn preset_yaml_round_trips_through_load() {
        let p = sample_preset();
        let text = preset_yaml_string(&p).unwrap();
        let loaded = load_preset_yaml_str(&text, "<test>").unwrap();
        assert_eq!(loaded, p);
    }

    #[test]
    fn load_preset_yaml_rejects_unknown_field() {
        let text = r#"
preset:
  bank: 1
  number: 0
  name: x
  body:
    mode: simple
    chain: []
  bogus_field: hi
"#;
        let err = load_preset_yaml_str(text, "<test>").unwrap_err();
        match err {
            YamlError::Parse { .. } => {}
            other => panic!("expected Parse, got {other:?}"),
        }
    }

    #[test]
    fn load_preset_yaml_rejects_bank_out_of_range() {
        let text = "preset:\n  bank: 5\n  number: 0\n  name: x\n  body:\n    mode: simple\n    chain: []\n";
        let err = load_preset_yaml_str(text, "<test>").unwrap_err();
        match err {
            YamlError::BadBank { got: 5, .. } => {}
            other => panic!("expected BadBank, got {other:?}"),
        }
    }

    #[test]
    fn load_preset_yaml_rejects_bypass_in_advanced() {
        // bypass is no longer a field on Connection — feeding it in
        // advanced mode is a parse error, not a lint warning.
        let text = r#"
preset:
  bank: 1
  number: 0
  name: x
  body:
    mode: advanced
    connections:
      - from_connector: input_tip
        to_connector: output_tip
        bypass: true
"#;
        let err = load_preset_yaml_str(text, "<test>").unwrap_err();
        match err {
            YamlError::Parse { .. } => {}
            other => panic!("expected Parse for bypass-in-advanced, got {other:?}"),
        }
    }

    #[test]
    fn controller_yaml_round_trips() {
        let ctrl = Controller {
            uuid: "abc".into(),
            midi_channel: 9,
            device_id: 6,
            input_split: true,
            loop_bypass_persistent: false,
            include_in_trails: IncludeInTrails {
                a_tip: true,
                e_ring: true,
                ..IncludeInTrails::default()
            },
            connectors: Connectors {
                a_tip: Connector {
                    name: "Ego 76".into(),
                    short_name: "76".into(),
                    input_name: "guitar".into(),
                    output_name: "EQ-7".into(),
                },
                ..Connectors::default()
            },
        };
        let text = controller_yaml_string(&ctrl).unwrap();
        assert!(text.contains("controller:"));
        assert!(text.contains("midi_channel: 9"));
        assert!(text.contains("device_id: 6"));
        assert!(text.contains("input_split: true"));
        assert!(text.contains("Ego 76"));
        let loaded = load_controller_yaml_str(&text, "<test>").unwrap();
        assert_eq!(loaded, ctrl);
    }

    #[test]
    fn spillover_target_serde_roundtrip() {
        // Sanity: SpilloverTarget enum serializes as the snake_case slug.
        let p = Preset {
            spillover: Spillover {
                output_tip: SpilloverTarget::DTip,
                output_ring: SpilloverTarget::Nothing,
            },
            ..sample_preset()
        };
        let text = preset_yaml_string(&p).unwrap();
        assert!(text.contains("output_tip: d_tip"), "got: {text}");
        let loaded = load_preset_yaml_str(&text, "<test>").unwrap();
        assert_eq!(loaded.spillover.output_tip, SpilloverTarget::DTip);
    }
}
