//! YAML codec (string-only) for ML10X presets and controller settings.
//!
//! `serde_norway` doesn't preserve comments or key ordering on a load → save
//! round-trip; we accept that and instead guarantee:
//!
//! - Field ordering: explicit shim structs make the on-disk YAML read
//!   top-to-bottom under a screen reader (bank → number → name →
//!   spillover → body).
//! - YAML bank field is 1..4 (matches the editor UI); the in-memory
//!   `Preset.bank` stays 0..3 (matches the wire protocol). Translation
//!   happens in the shim conversions.
//!
//! This module is pure: no file I/O, no paths. Callers that want to read
//! or write `.yaml` files supply their own I/O — see `ml10x::yaml_io` on
//! the CLI side. The `# yaml-language-server: $schema=…` header line is
//! also a file-level concern and isn't emitted here.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::presets::{
    Connection, Connector, Connectors, Controller, IncludeInTrails, Preset, PresetBody, SimpleHop,
    Spillover,
};

/// Canonical URL of the published preset JSON Schema. The CLI prepends a
/// `# yaml-language-server: $schema=…` line referencing this so VS Code's
/// YAML extension picks it up.
pub const PRESET_SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/ragb/ml10x-access-rs/main/schemas/preset.schema.json";
pub const CONTROLLER_SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/ragb/ml10x-access-rs/main/schemas/controller.schema.json";

#[derive(Debug, Error)]
pub enum YamlError {
    #[error("YAML parse error in {source_label}: {source}")]
    Parse {
        source_label: String,
        #[source]
        source: serde_norway::Error,
    },
    #[error("YAML serialization error: {0}")]
    Serialize(#[from] serde_norway::Error),
    #[error("YAML at {source_label}: preset.bank must be 1..4 (got {got})")]
    BadBank { source_label: String, got: u8 },
}

// ---------- Preset shim ----------

/// ML10X preset YAML document.
///
/// One preset on the Morningstar ML10X loop switcher. Note: the ML10X has
/// no per-preset MIDI-message list (that's an MC-series feature); this
/// schema deliberately omits any `midi_messages` field.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "ML10X Preset"))]
pub(crate) struct PresetDoc {
    preset: PresetYaml,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct PresetYaml {
    /// Which of the 4 banks this preset lives in (matches the editor's
    /// 'Bank 1'..'Bank 4' labels).
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = 4)))]
    bank: u8, // 1..4 in YAML, mapped to/from 0..3 internal
    /// Position within the bank (0 through 127, matching the device's MIDI
    /// Program Change numbering).
    #[cfg_attr(feature = "schema", schemars(range(min = 0, max = 127)))]
    number: u8,
    /// Display name shown on the ML10X.
    #[cfg_attr(feature = "schema", schemars(length(max = 16)))]
    name: String,
    /// When the user switches to a different preset, the chosen loop's
    /// trail audio continues sounding (provided that loop is bypassed in
    /// the next preset). Set per output. 'nothing' is the default.
    #[serde(default)]
    spillover: SpilloverYaml,
    /// Mode-specific routing. Simple is a linear chain with per-loop
    /// bypass; Advanced is an arbitrary connection graph with no per-loop
    /// bypass (the firmware does not honour it in Advanced presets).
    body: PresetBodyYaml,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
enum PresetBodyYaml {
    Simple {
        /// Linear signal chain: ordered hops from sources through loops to
        /// outputs. Each hop may set `bypass: true` to skip that loop.
        #[serde(default)]
        chain: Vec<SimpleHopYaml>,
    },
    Advanced {
        /// Routing graph: each edge connects a source to a destination.
        /// Branching and merging are allowed. No `bypass` field — the
        /// firmware ignores per-loop bypass in Advanced presets.
        #[serde(default)]
        connections: Vec<ConnectionYaml>,
    },
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

fn yaml_to_preset(doc: PresetDoc, source_label: &str) -> Result<Preset, YamlError> {
    if !(1..=4).contains(&doc.preset.bank) {
        return Err(YamlError::BadBank {
            source_label: source_label.to_string(),
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

pub fn preset_to_yaml_string(preset: &Preset) -> Result<String, YamlError> {
    Ok(serde_norway::to_string(&preset_to_yaml(preset))?)
}

pub fn preset_from_yaml_str(text: &str, source_label: &str) -> Result<Preset, YamlError> {
    let doc: PresetDoc = serde_norway::from_str(text).map_err(|e| YamlError::Parse {
        source_label: source_label.to_string(),
        source: e,
    })?;
    yaml_to_preset(doc, source_label)
}

// ---------- Controller shim ----------

/// ML10X Controller (global settings) YAML document.
///
/// Device-wide settings that aren't per-preset: MIDI channel, device ID,
/// connector names, per-loop include-in-trails toggles, input split and
/// loop bypass persistent flags. One of these per ML10X.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "ML10X Controller (global settings)")
)]
pub(crate) struct ControllerDoc {
    controller: ControllerYaml,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct ControllerYaml {
    /// Read-only device serial reported on connect. Editing this in the
    /// YAML has no effect.
    #[serde(default)]
    uuid: String,
    /// MIDI channel the device listens on. 0 means 'ignore MIDI in'.
    #[cfg_attr(feature = "schema", schemars(range(min = 0, max = 16)))]
    #[serde(default)]
    midi_channel: u8,
    /// Per-device routing ID (0..16).
    #[cfg_attr(feature = "schema", schemars(range(min = 0, max = 16)))]
    #[serde(default)]
    device_id: u8,
    /// When true, Input Tip and Input Ring are routed independently
    /// instead of summed.
    #[serde(default)]
    input_split: bool,
    /// When true, loop bypass state persists across preset changes for any
    /// loop not explicitly set by the new preset.
    #[serde(default)]
    loop_bypass_persistent: bool,
    /// Per-loop 'Enable Spillover' flag from Controller Settings. Only the
    /// 10 loops have these toggles in the editor UI — the 4 fixed
    /// endpoints (Input Tip/Ring, Output Tip/Ring) have no toggle.
    #[serde(default)]
    include_in_trails: IncludeInTrails,
    /// Names for each of the 14 connectors. The 'name' (long, up to 16
    /// chars) is what's shown most places; 'short_name' (up to 4) is for
    /// compact displays; 'input_name' / 'output_name' (up to 16 each)
    /// label the connector when it's the input or output of a chain.
    #[serde(default)]
    connectors: ConnectorsYaml,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct ConnectorYaml {
    #[cfg_attr(feature = "schema", schemars(length(max = 16)))]
    #[serde(default)]
    name: String,
    #[cfg_attr(feature = "schema", schemars(length(max = 4)))]
    #[serde(default)]
    short_name: String,
    #[cfg_attr(feature = "schema", schemars(length(max = 16)))]
    #[serde(default)]
    input_name: String,
    #[cfg_attr(feature = "schema", schemars(length(max = 16)))]
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

pub fn controller_to_yaml_string(controller: &Controller) -> Result<String, YamlError> {
    Ok(serde_norway::to_string(&controller_to_yaml(controller))?)
}

pub fn controller_from_yaml_str(text: &str, source_label: &str) -> Result<Controller, YamlError> {
    let doc: ControllerDoc = serde_norway::from_str(text).map_err(|e| YamlError::Parse {
        source_label: source_label.to_string(),
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
        let yaml = preset_to_yaml_string(&sample_preset()).unwrap();
        assert!(yaml.starts_with("preset:"), "got: {yaml}");
    }

    #[test]
    fn preset_yaml_contains_expected_fields() {
        let yaml = preset_to_yaml_string(&sample_preset()).unwrap();
        for needle in [
            "bank:",
            "number:",
            "name:",
            "spillover:",
            "body:",
            "mode:",
            "chain:",
        ] {
            assert!(yaml.contains(needle), "missing {needle:?} in:\n{yaml}");
        }
        assert!(
            !yaml.contains("midi_messages"),
            "midi_messages leaked into YAML:\n{yaml}"
        );
    }

    #[test]
    fn advanced_preset_yaml_uses_connections_field() {
        let yaml = preset_to_yaml_string(&sample_advanced_preset()).unwrap();
        assert!(yaml.contains("mode: advanced"), "got: {yaml}");
        assert!(yaml.contains("connections:"), "got: {yaml}");
        assert!(
            !yaml.contains("chain:"),
            "advanced preset should not have a chain field: {yaml}"
        );
        assert!(
            !yaml.contains("bypass:"),
            "advanced preset should not have bypass: {yaml}"
        );
    }

    #[test]
    fn advanced_preset_yaml_round_trips() {
        let p = sample_advanced_preset();
        let text = preset_to_yaml_string(&p).unwrap();
        let loaded = preset_from_yaml_str(&text, "<test>").unwrap();
        assert_eq!(loaded, p);
    }

    #[test]
    fn preset_yaml_bank_is_1_indexed() {
        let yaml = preset_to_yaml_string(&sample_preset()).unwrap();
        assert!(
            yaml.contains("bank: 1"),
            "expected 1-indexed bank in:\n{yaml}"
        );
    }

    #[test]
    fn preset_yaml_chain_uses_readable_field_names() {
        let yaml = preset_to_yaml_string(&sample_preset()).unwrap();
        assert!(yaml.contains("from_connector:"));
        assert!(yaml.contains("to_connector:"));
        assert!(yaml.contains("bypass:"));
    }

    #[test]
    fn preset_yaml_round_trips_through_load() {
        let p = sample_preset();
        let text = preset_to_yaml_string(&p).unwrap();
        let loaded = preset_from_yaml_str(&text, "<test>").unwrap();
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
        let err = preset_from_yaml_str(text, "<test>").unwrap_err();
        match err {
            YamlError::Parse { .. } => {}
            other => panic!("expected Parse, got {other:?}"),
        }
    }

    #[test]
    fn load_preset_yaml_rejects_bank_out_of_range() {
        let text = "preset:\n  bank: 5\n  number: 0\n  name: x\n  body:\n    mode: simple\n    chain: []\n";
        let err = preset_from_yaml_str(text, "<test>").unwrap_err();
        match err {
            YamlError::BadBank { got: 5, .. } => {}
            other => panic!("expected BadBank, got {other:?}"),
        }
    }

    #[test]
    fn load_preset_yaml_rejects_bypass_in_advanced() {
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
        let err = preset_from_yaml_str(text, "<test>").unwrap_err();
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
        let text = controller_to_yaml_string(&ctrl).unwrap();
        assert!(text.contains("controller:"));
        assert!(text.contains("midi_channel: 9"));
        assert!(text.contains("device_id: 6"));
        assert!(text.contains("input_split: true"));
        assert!(text.contains("Ego 76"));
        let loaded = controller_from_yaml_str(&text, "<test>").unwrap();
        assert_eq!(loaded, ctrl);
    }

    #[test]
    fn spillover_target_serde_roundtrip() {
        let p = Preset {
            spillover: Spillover {
                output_tip: SpilloverTarget::DTip,
                output_ring: SpilloverTarget::Nothing,
            },
            ..sample_preset()
        };
        let text = preset_to_yaml_string(&p).unwrap();
        assert!(text.contains("output_tip: d_tip"), "got: {text}");
        let loaded = preset_from_yaml_str(&text, "<test>").unwrap();
        assert_eq!(loaded.spillover.output_tip, SpilloverTarget::DTip);
    }
}
