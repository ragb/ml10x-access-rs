//! ML10X device profile and SysEx header layout.
//!
//! All constants here are confirmed from the official editor's bundle —
//! see docs/sysex.md for the source references.

pub const MANUFACTURER_ID: [u8; 3] = [0x00, 0x21, 0x24];
pub const MODEL_BYTE: u8 = 0x07;

pub const NUM_BANKS: u8 = 4;
pub const PRESETS_PER_BANK: u8 = 128;
pub const NUM_LOOPS: u8 = 10;

/// Byte positions inside the 16-byte SysEx header. Matches the `BZ`
/// enum the editor's parser uses (see docs/sysex.md).
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum HeaderPos {
    SysexStart = 0,
    ManfId1 = 1,
    ManfId2 = 2,
    ManfId3 = 3,
    ModelId = 4,
    VersionId1 = 5,
    FunctionId1 = 6,
    FunctionId2 = 7,
    FunctionId3 = 8,
    FunctionId4 = 9,
    FunctionId5 = 10,
    FunctionId6 = 11,
    FunctionId7 = 12,
    FunctionId8 = 13,
    // Outbound: always zero. Inbound (device→host): packed length field.
    // length = ((byte14 & 0x3F) << 7) | byte15.
    // Top bit of byte14 is a flag (suspected "is-last-chunk").
    LengthMsb = 14,
    LengthLsb = 15,
    ArrayStart = 16,
}

impl HeaderPos {
    pub const fn idx(self) -> usize {
        self as usize
    }
}

pub const HEADER_LENGTH: usize = 16;

/// Named values for FUNCTION_ID_2 (P2) — the editor's outbound request opcodes.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FunctionCode {
    Dummy = 0,
    EngagePreset = 29,
    EngageExp = 30,
    RequestControllerSettingsAll = 35,
    RequestControllerGeneralConfig = 36,
    RequestWaveformEngine = 37,
    RequestSequencerEngine = 38,
    RequestScrollSlots = 39,
    RequestMidiChannelNames = 40,
    RequestBankArrangement = 41,
    RequestOmniportData = 42,
    RequestBankPresetNames = 43,
    RequestControllerFirmwareVersion = 44,
    RequestEventProcessor = 45,
    RequestControllerUuid = 46,
    ToggleLooperMode = 47,
    RequestPresetNames = 64,
    RequestExpressionCalibration = 65,
    RequestResistorLadderCalibration = 66,
    RequestMidiClockSlots = 80,
}

pub const SEGMENT_LEAD_IN: u8 = 0x7F;

/// Top-level classification of inbound messages by their f1 (FUNCTION_ID_1) byte.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum InboundClass {
    EditorEvent = 1,
    Data = 6,
    DataDump = 7,
    DataUploadAck = 8,
    PresetNames = 9,
    ControllerInfo = 17,
}

/// Per-preset routing mode. The user toggles this in the editor's Edit Preset tab;
/// in the wire protocol it's not a segment but a header field (P5 for READ, P2 for WRITE).
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PresetMode {
    Simple = 0,
    Advanced = 1,
}

// Two numbering systems are at play in the ML10X SysEx protocol:
//
//   - "value"       — 0..13, indexes the controller-data name segments.
//                     A Tip=0, A Ring=1, ..., E Ring=9,
//                     Input Tip=10, Input Ring=11, Output Tip=12, Output Ring=13.
//
//   - "groupNumber" — 0..13 with a DIFFERENT ordering, used in the preset
//                     connection records (segment id + data bytes).
//
// Both go to the same 14 physical connectors; they're just two different
// indexings the device firmware uses.

pub const CONNECTOR_LABELS: [&str; 14] = [
    "A Tip", "A Ring", "B Tip", "B Ring", "C Tip", "C Ring", "D Tip", "D Ring", "E Tip", "E Ring",
    "Input Tip", "Input Ring", "Output Tip", "Output Ring",
];

pub const CONNECTOR_SLUGS: [&str; 14] = [
    "a_tip",
    "a_ring",
    "b_tip",
    "b_ring",
    "c_tip",
    "c_ring",
    "d_tip",
    "d_ring",
    "e_tip",
    "e_ring",
    "input_tip",
    "input_ring",
    "output_tip",
    "output_ring",
];

/// groupNumber → value mapping (from the editor's Hw array).
pub const GROUPNUMBER_TO_VALUE: [u8; 14] = [
    10, // gn=0  Input Tip
    11, // gn=1  Input Ring
    12, // gn=2  Output Tip
    13, // gn=3  Output Ring
    0,  // gn=4  A Tip
    2,  // gn=5  B Tip
    4,  // gn=6  C Tip
    6,  // gn=7  D Tip
    8,  // gn=8  E Tip
    1,  // gn=9  A Ring
    3,  // gn=10 B Ring
    5,  // gn=11 C Ring
    7,  // gn=12 D Ring
    9,  // gn=13 E Ring
];

/// Inverse: value → groupNumber. Computed at compile time so it can never drift.
pub const VALUE_TO_GROUPNUMBER: [u8; 14] = {
    let mut out = [0u8; 14];
    let mut i = 0;
    while i < 14 {
        out[GROUPNUMBER_TO_VALUE[i] as usize] = i as u8;
        i += 1;
    }
    out
};

/// Sentinel byte in a connection record's data[1] meaning 'this connector
/// has no outgoing link in this preset's chain'.
pub const UNROUTED: u8 = 0x7F;

/// Selected segment IDs used inside DATA / CONTROLLER_INFO payloads.
///
/// Inside controller data (f1=6 f2=1) and preset data (f1=6 f2=0), segments
/// are keyed by their `id` byte rather than by position.
pub mod segment_id {
    // ---- f1=17 CONTROLLER_INFO ----
    pub const UUID_OR_FIRMWARE: u8 = 0;

    // ---- f1=6 f2=1 CONTROLLER DATA — long names (16 chars, space-padded) ----
    pub const LOOP_A_TIP: u8 = 0;
    pub const LOOP_A_RING: u8 = 1;
    pub const LOOP_B_TIP: u8 = 2;
    pub const LOOP_B_RING: u8 = 3;
    pub const LOOP_C_TIP: u8 = 4;
    pub const LOOP_C_RING: u8 = 5;
    pub const LOOP_D_TIP: u8 = 6;
    pub const LOOP_D_RING: u8 = 7;
    pub const LOOP_E_TIP: u8 = 8;
    pub const LOOP_E_RING: u8 = 9;
    pub const INPUT_TIP: u8 = 10;
    pub const INPUT_RING: u8 = 11;
    pub const OUTPUT_TIP: u8 = 12;
    pub const OUTPUT_RING: u8 = 13;

    // ---- f1=6 f2=1 CONTROLLER DATA — global settings (small ints) ----
    pub const MIDI_CHANNEL: u8 = 32;
    pub const INCLUDE_IN_TRAILS: u8 = 33;
    pub const DEVICE_ID: u8 = 34;
    pub const INPUT_SPLIT: u8 = 35;
    pub const LOOP_BYPASS_PERSIST: u8 = 36;

    // ---- f1=6 f2=1 CONTROLLER DATA — short names (4 chars, space-padded) ----
    pub const SHORT_NAMES_FIRST: u8 = 48;
    pub const INPUT_LABEL_FIRST: u8 = 64;
    pub const OUTPUT_LABEL_FIRST: u8 = 80;

    // ---- PRESET DATA — common to Simple + Advanced ----
    pub const SPILLOVER_OUTPUT_TIP: u8 = 16;
    pub const SPILLOVER_OUTPUT_RING: u8 = 17;
    pub const PRESET_NAME: u8 = 32;

    // Advanced-only segments (observed only in Advanced WRITE):
    pub const ADV_FLAG_18: u8 = 18; // 1 byte, observed 0
    pub const ADV_FLAG_19: u8 = 19; // 3 bytes, observed 00 00 00

    // Simple-only segment:
    pub const SIMPLE_FLAG_20: u8 = 20;

    // Routing matrix bytes (segment ids 48..63). The device's internal
    // `matrixArray` per the editor's `addToMatrix(x.id, x.data)`. We
    // don't emit these on write (the device rebuilds the matrix from
    // segments 0..13) and we don't decode them on read.
    pub const MATRIX_ARRAY_FIRST: u8 = 48;
    pub const MATRIX_ARRAY_LAST: u8 = 63;
}

pub fn groupnumber_to_slug(gn: u8) -> &'static str {
    CONNECTOR_SLUGS[GROUPNUMBER_TO_VALUE[gn as usize] as usize]
}

pub fn slug_to_groupnumber(slug: &str) -> Option<u8> {
    let value = CONNECTOR_SLUGS.iter().position(|s| *s == slug)?;
    Some(VALUE_TO_GROUPNUMBER[value])
}

/// Static facts about an ML10X. A struct (rather than module constants only)
/// so tests can swap in a fake profile.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DeviceProfile {
    pub manufacturer_id: [u8; 3],
    pub model_byte: u8,
    pub num_banks: u8,
    pub presets_per_bank: u8,
    pub num_loops: u8,
}

impl DeviceProfile {
    pub const fn total_presets(&self) -> u16 {
        self.num_banks as u16 * self.presets_per_bank as u16
    }
}

pub const ML10X: DeviceProfile = DeviceProfile {
    manufacturer_id: MANUFACTURER_ID,
    model_byte: MODEL_BYTE,
    num_banks: NUM_BANKS,
    presets_per_bank: PRESETS_PER_BANK,
    num_loops: NUM_LOOPS,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_to_groupnumber_is_inverse_of_groupnumber_to_value() {
        for v in 0u8..14 {
            assert_eq!(GROUPNUMBER_TO_VALUE[VALUE_TO_GROUPNUMBER[v as usize] as usize], v);
        }
        for gn in 0u8..14 {
            assert_eq!(VALUE_TO_GROUPNUMBER[GROUPNUMBER_TO_VALUE[gn as usize] as usize], gn);
        }
    }

    #[test]
    fn slug_groupnumber_round_trip() {
        for &slug in &CONNECTOR_SLUGS {
            let gn = slug_to_groupnumber(slug).expect("known slug");
            assert_eq!(groupnumber_to_slug(gn), slug);
        }
    }

    #[test]
    fn total_presets() {
        assert_eq!(ML10X.total_presets(), 512);
    }
}
