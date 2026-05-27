//! In-memory model for ML10X presets and the global controller settings.
//!
//! The structs here are the single source of truth for the YAML schema and
//! the SysEx (de)serializer. Field names are chosen to read well linearly.
//!
//! The ML10X has no per-preset MIDI message list (that's an MC-series
//! feature); there is no `MidiMessage` struct here, and the controller
//! struct deliberately omits `input_ring_alternate_pin` (referenced in
//! editor source but not exposed by v1.2 firmware).

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopState {
    On,
    Off,
}

/// Spillover can target any of the 10 loops or either input. The editor's
/// `j2` dropdown does NOT include `last_connected` for the ML10X — that's
/// an MC-series feature. The two outputs aren't valid targets (you can't
/// spill audio back into an output).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpilloverTarget {
    Nothing,
    InputTip,
    InputRing,
    ATip,
    ARing,
    BTip,
    BRing,
    CTip,
    CRing,
    DTip,
    DRing,
    ETip,
    ERing,
}

impl Default for SpilloverTarget {
    fn default() -> Self {
        SpilloverTarget::Nothing
    }
}

impl SpilloverTarget {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Nothing => "nothing",
            Self::InputTip => "input_tip",
            Self::InputRing => "input_ring",
            Self::ATip => "a_tip",
            Self::ARing => "a_ring",
            Self::BTip => "b_tip",
            Self::BRing => "b_ring",
            Self::CTip => "c_tip",
            Self::CRing => "c_ring",
            Self::DTip => "d_tip",
            Self::DRing => "d_ring",
            Self::ETip => "e_tip",
            Self::ERing => "e_ring",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        Some(match slug {
            "nothing" => Self::Nothing,
            "input_tip" => Self::InputTip,
            "input_ring" => Self::InputRing,
            "a_tip" => Self::ATip,
            "a_ring" => Self::ARing,
            "b_tip" => Self::BTip,
            "b_ring" => Self::BRing,
            "c_tip" => Self::CTip,
            "c_ring" => Self::CRing,
            "d_tip" => Self::DTip,
            "d_ring" => Self::DRing,
            "e_tip" => Self::ETip,
            "e_ring" => Self::ERing,
            _ => return None,
        })
    }
}

/// One of the 14 physical connectors on the device. Used as the
/// `from`/`to` of a chain hop and as a key into the connector name table.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorSlug {
    ATip,
    ARing,
    BTip,
    BRing,
    CTip,
    CRing,
    DTip,
    DRing,
    ETip,
    ERing,
    InputTip,
    InputRing,
    OutputTip,
    OutputRing,
}

impl ConnectorSlug {
    pub fn slug(self) -> &'static str {
        crate::device::CONNECTOR_SLUGS[self.value() as usize]
    }

    /// Index into the editor's "value" numbering (controller-data segments).
    pub fn value(self) -> u8 {
        use ConnectorSlug::*;
        match self {
            ATip => 0,
            ARing => 1,
            BTip => 2,
            BRing => 3,
            CTip => 4,
            CRing => 5,
            DTip => 6,
            DRing => 7,
            ETip => 8,
            ERing => 9,
            InputTip => 10,
            InputRing => 11,
            OutputTip => 12,
            OutputRing => 13,
        }
    }

    pub fn from_value(v: u8) -> Option<Self> {
        use ConnectorSlug::*;
        Some(match v {
            0 => ATip,
            1 => ARing,
            2 => BTip,
            3 => BRing,
            4 => CTip,
            5 => CRing,
            6 => DTip,
            7 => DRing,
            8 => ETip,
            9 => ERing,
            10 => InputTip,
            11 => InputRing,
            12 => OutputTip,
            13 => OutputRing,
            _ => return None,
        })
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        Some(match slug {
            "a_tip" => Self::ATip,
            "a_ring" => Self::ARing,
            "b_tip" => Self::BTip,
            "b_ring" => Self::BRing,
            "c_tip" => Self::CTip,
            "c_ring" => Self::CRing,
            "d_tip" => Self::DTip,
            "d_ring" => Self::DRing,
            "e_tip" => Self::ETip,
            "e_ring" => Self::ERing,
            "input_tip" => Self::InputTip,
            "input_ring" => Self::InputRing,
            "output_tip" => Self::OutputTip,
            "output_ring" => Self::OutputRing,
            _ => return None,
        })
    }

    pub fn is_input(self) -> bool {
        matches!(self, Self::InputTip | Self::InputRing)
    }

    pub fn is_output(self) -> bool {
        matches!(self, Self::OutputTip | Self::OutputRing)
    }
}

/// One link in a Simple-mode preset's signal chain. Carries `bypass`
/// because Simple mode supports per-loop bypass.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimpleHop {
    pub from_connector: ConnectorSlug,
    pub to_connector: ConnectorSlug,
    #[serde(default)]
    pub bypass: bool,
}

/// One edge in an Advanced-mode preset's routing graph. No `bypass` —
/// the v1.2 firmware ignores per-loop bypass in Advanced presets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub from_connector: ConnectorSlug,
    pub to_connector: ConnectorSlug,
}

/// Per-output spillover target for a preset.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spillover {
    #[serde(default)]
    pub output_tip: SpilloverTarget,
    #[serde(default)]
    pub output_ring: SpilloverTarget,
}

impl Default for Spillover {
    fn default() -> Self {
        Self {
            output_tip: SpilloverTarget::Nothing,
            output_ring: SpilloverTarget::Nothing,
        }
    }
}

/// Discriminator value used in YAML (`body.mode: simple|advanced`) and in
/// human-readable error messages. The in-memory `Preset.body` enum is the
/// source of truth; this is derived via [`Preset::mode`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetMode {
    Simple,
    Advanced,
}

/// The mode-specific shape of a preset's routing.
///
/// Simple is a linear chain with per-loop bypass; Advanced is an
/// arbitrary graph and the firmware ignores per-loop bypass. Encoding
/// these as distinct variants makes invalid combinations unrepresentable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PresetBody {
    Simple {
        #[serde(default)]
        chain: Vec<SimpleHop>,
    },
    Advanced {
        #[serde(default)]
        connections: Vec<Connection>,
    },
}

impl PresetBody {
    pub fn mode(&self) -> PresetMode {
        match self {
            PresetBody::Simple { .. } => PresetMode::Simple,
            PresetBody::Advanced { .. } => PresetMode::Advanced,
        }
    }
}

impl Default for PresetBody {
    fn default() -> Self {
        PresetBody::Simple { chain: Vec::new() }
    }
}

/// One preset on the ML10X.
///
/// `bank` is 0..3 (matches the SysEx wire form). The YAML layer maps it
/// to 1..4 to match the editor's "Bank 1".."Bank 4" labels and the
/// folder convention. `number` is 0..127 within the bank (matches the
/// device's MIDI Program Change addressing).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    pub bank: u8,
    pub number: u8,
    pub name: String,
    pub spillover: Spillover,
    pub body: PresetBody,
}

impl Preset {
    pub fn mode(&self) -> PresetMode {
        self.body.mode()
    }
}

/// One of the 14 physical connectors on the device. Names are
/// user-editable in the official editor under Controller Settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connector {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub short_name: String,
    #[serde(default)]
    pub input_name: String,
    #[serde(default)]
    pub output_name: String,
}

/// Per-loop spillover-enable flag from Controller Settings. Only the 10 loops
/// have toggles in the editor UI — the 4 fixed endpoints have no
/// Enable-Spillover control. Bits 10..13 of segment 33 are unused on v1.2.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncludeInTrails {
    #[serde(default)]
    pub a_tip: bool,
    #[serde(default)]
    pub a_ring: bool,
    #[serde(default)]
    pub b_tip: bool,
    #[serde(default)]
    pub b_ring: bool,
    #[serde(default)]
    pub c_tip: bool,
    #[serde(default)]
    pub c_ring: bool,
    #[serde(default)]
    pub d_tip: bool,
    #[serde(default)]
    pub d_ring: bool,
    #[serde(default)]
    pub e_tip: bool,
    #[serde(default)]
    pub e_ring: bool,
}

/// The 14-connector name table — one entry per physical jack.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connectors {
    #[serde(default)]
    pub a_tip: Connector,
    #[serde(default)]
    pub a_ring: Connector,
    #[serde(default)]
    pub b_tip: Connector,
    #[serde(default)]
    pub b_ring: Connector,
    #[serde(default)]
    pub c_tip: Connector,
    #[serde(default)]
    pub c_ring: Connector,
    #[serde(default)]
    pub d_tip: Connector,
    #[serde(default)]
    pub d_ring: Connector,
    #[serde(default)]
    pub e_tip: Connector,
    #[serde(default)]
    pub e_ring: Connector,
    #[serde(default)]
    pub input_tip: Connector,
    #[serde(default)]
    pub input_ring: Connector,
    #[serde(default)]
    pub output_tip: Connector,
    #[serde(default)]
    pub output_ring: Connector,
}

/// Global, device-wide settings that aren't per-preset.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Controller {
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub midi_channel: u8,
    #[serde(default)]
    pub device_id: u8,
    #[serde(default)]
    pub input_split: bool,
    #[serde(default)]
    pub loop_bypass_persistent: bool,
    #[serde(default)]
    pub include_in_trails: IncludeInTrails,
    #[serde(default)]
    pub connectors: Connectors,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connector_slug_value_round_trip() {
        for v in 0u8..14 {
            let c = ConnectorSlug::from_value(v).expect("known value");
            assert_eq!(c.value(), v);
            assert_eq!(ConnectorSlug::from_slug(c.slug()), Some(c));
        }
    }

    #[test]
    fn spillover_target_slug_round_trip() {
        let all = [
            SpilloverTarget::Nothing,
            SpilloverTarget::InputTip,
            SpilloverTarget::InputRing,
            SpilloverTarget::ATip,
            SpilloverTarget::ARing,
            SpilloverTarget::BTip,
            SpilloverTarget::BRing,
            SpilloverTarget::CTip,
            SpilloverTarget::CRing,
            SpilloverTarget::DTip,
            SpilloverTarget::DRing,
            SpilloverTarget::ETip,
            SpilloverTarget::ERing,
        ];
        for t in all {
            assert_eq!(SpilloverTarget::from_slug(t.slug()), Some(t));
        }
    }

    #[test]
    fn preset_default_body_is_simple() {
        let p = Preset {
            bank: 1,
            number: 0,
            name: "empty".to_string(),
            spillover: Spillover::default(),
            body: PresetBody::default(),
        };
        assert_eq!(p.mode(), PresetMode::Simple);
    }
}
