//! Encode `Preset` / `Controller` structs back to SysEx bytes.
//!
//! Inverse of `decode.rs`. The format matches what the official editor
//! emits when the user clicks Save — verified by byte-exact round-trip
//! against captured device-side writes.

use log::debug;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

use crate::device::{
    CONNECTOR_SLUGS, DeviceProfile, ML10X, UNROUTED, segment_id, slug_to_groupnumber,
};
use crate::presets::{Controller, Preset, PresetBody, SimpleHop, SpilloverTarget};
use crate::sysex::{SysexError, build_header_with, encode_segment, frame_with};

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("Unknown connector slug {0:?}")]
    UnknownConnector(String),
    #[error("Sysex framing error: {0}")]
    Sysex(#[from] SysexError),
}

/// Spillover WRITE-form byte mapping. NOT a linear 0..9 table — the
/// editor's `j2` dropdown uses this specific scrambled mapping.
pub fn spillover_write_byte(target: SpilloverTarget) -> u8 {
    use SpilloverTarget::*;
    match target {
        Nothing => 127,
        InputTip => 2,
        InputRing => 3,
        ATip => 1,
        ARing => 0,
        BTip => 15,
        BRing => 4,
        CTip => 11,
        CRing => 6,
        DTip => 13,
        DRing => 12,
        ETip => 9,
        ERing => 10,
    }
}

fn gn_of(slug: &str) -> Result<u8, EncodeError> {
    slug_to_groupnumber(slug).ok_or_else(|| EncodeError::UnknownConnector(slug.to_string()))
}

/// Build a Simple-mode preset write SysEx message.
///
/// Matches the editor's Save Preset output exactly: chain hops become
/// 3-byte segments `<from_gn> <to_gn> <bypass>`; connectors not
/// appearing in the chain emit 2-byte "unrouted" segments `<gn> 7F`.
///
/// `save_to_current=true` uses the editor's P3=127 sentinel ("save to
/// currently selected preset"). The current device firmware appears to
/// only accept the sentinel form.
pub fn encode_simple_preset_with(
    device: DeviceProfile,
    preset: &Preset,
    save_to_current: bool,
) -> Result<Vec<u8>, EncodeError> {
    let chain: &[SimpleHop] = match &preset.body {
        PresetBody::Simple { chain } => chain,
        PresetBody::Advanced { .. } => &[],
    };
    let p3 = if save_to_current {
        0x7F
    } else {
        preset.number & 0x7F
    };
    let p4 = if save_to_current {
        0
    } else {
        preset.bank & 0x7F
    };
    let header = build_header_with(device, 6, 0, p3, p4, 0, 0, 0, 0);

    let mut segments: Vec<Vec<u8>> = Vec::new();

    // Build a map from connector groupNumber -> chain hop, so we can
    // emit one segment per "FROM-able" connector.
    let mut linked: std::collections::HashMap<u8, &SimpleHop> = std::collections::HashMap::new();
    for hop in chain {
        let from_gn = gn_of(hop.from_connector.slug())?;
        linked.insert(from_gn, hop);
    }

    // Two connectors are never used as the FROM side of a chain hop and the
    // device rejects messages that include segments for them: Input Ring
    // (gn=1) and Output Tip (gn=2). The other 12 are emitted in a specific
    // per-preset order: unrouted first in fixed gn order, then routed in
    // chain order. The device's parser is order-sensitive.
    const FIXED_FROMABLE_ORDER: [u8; 12] = [4, 9, 5, 10, 6, 11, 7, 12, 8, 13, 3, 0];

    let routed_gns_in_chain_order: Vec<u8> = chain
        .iter()
        .map(|hop| gn_of(hop.from_connector.slug()))
        .collect::<Result<Vec<u8>, _>>()?;
    let routed_set: std::collections::HashSet<u8> =
        routed_gns_in_chain_order.iter().copied().collect();
    let unrouted: Vec<u8> = FIXED_FROMABLE_ORDER
        .iter()
        .copied()
        .filter(|gn| !routed_set.contains(gn))
        .collect();

    let emit_order: Vec<u8> = unrouted
        .into_iter()
        .chain(routed_gns_in_chain_order.into_iter())
        .collect();

    for gn in emit_order {
        if let Some(hop) = linked.get(&gn) {
            let to_gn = gn_of(hop.to_connector.slug())?;
            let bypass = if hop.bypass { 1 } else { 0 };
            segments.push(encode_segment(gn, &[gn, to_gn, bypass]));
        } else {
            segments.push(encode_segment(gn, &[gn, UNROUTED]));
        }
    }

    // Spillover targets.
    segments.push(encode_segment(
        segment_id::SPILLOVER_OUTPUT_TIP,
        &[spillover_write_byte(preset.spillover.output_tip)],
    ));
    segments.push(encode_segment(
        segment_id::SPILLOVER_OUTPUT_RING,
        &[spillover_write_byte(preset.spillover.output_ring)],
    ));

    // Simple-only flag at segment 20. Always 0 in captured saves.
    segments.push(encode_segment(segment_id::SIMPLE_FLAG_20, &[0]));

    // Preset name.
    let name_bytes = ascii_trim_right(&preset.name, 12);
    segments.push(encode_segment(segment_id::PRESET_NAME, &name_bytes));

    Ok(frame_with(device, &header, &segments)?)
}

pub fn encode_simple_preset(
    preset: &Preset,
    save_to_current: bool,
) -> Result<Vec<u8>, EncodeError> {
    encode_simple_preset_with(ML10X, preset, save_to_current)
}

/// Build an Advanced-mode preset write SysEx message (P2=2). Different
/// segment layout from Simple: per chain hop we emit one 1-byte segment
/// where `id = target gn` and `data[0] = source gn`. Plus the common
/// segments (spillover 16/17, name 32) and Advanced-only segments 18/19.
pub fn encode_advanced_preset_with(
    device: DeviceProfile,
    preset: &Preset,
    save_to_current: bool,
) -> Result<Vec<u8>, EncodeError> {
    let connections = match &preset.body {
        PresetBody::Advanced { connections } => connections.as_slice(),
        PresetBody::Simple { .. } => &[],
    };
    let p3 = if save_to_current {
        0x7F
    } else {
        preset.number & 0x7F
    };
    let p4 = if save_to_current {
        0
    } else {
        preset.bank & 0x7F
    };
    let header = build_header_with(device, 6, 2, p3, p4, 0, 0, 0, 0);

    let mut segments: Vec<Vec<u8>> = Vec::new();

    for conn in connections {
        let from_gn = gn_of(conn.from_connector.slug())?;
        let to_gn = gn_of(conn.to_connector.slug())?;
        segments.push(encode_segment(to_gn, &[from_gn]));
    }

    segments.push(encode_segment(
        segment_id::SPILLOVER_OUTPUT_TIP,
        &[spillover_write_byte(preset.spillover.output_tip)],
    ));
    segments.push(encode_segment(
        segment_id::SPILLOVER_OUTPUT_RING,
        &[spillover_write_byte(preset.spillover.output_ring)],
    ));

    // Advanced-only: muted-switch option (0..2). Not modeled in Preset
    // yet — emit 0 to match the captured baseline.
    segments.push(encode_segment(segment_id::ADV_FLAG_18, &[0]));

    // bypassLoopStatus bitmap: Advanced mode has no per-loop bypass in v1.2 firmware.
    segments.push(encode_segment(segment_id::ADV_FLAG_19, &[0, 0, 0]));

    let name_bytes = ascii_trim_right(&preset.name, 12);
    segments.push(encode_segment(segment_id::PRESET_NAME, &name_bytes));

    Ok(frame_with(device, &header, &segments)?)
}

pub fn encode_advanced_preset(
    preset: &Preset,
    save_to_current: bool,
) -> Result<Vec<u8>, EncodeError> {
    encode_advanced_preset_with(ML10X, preset, save_to_current)
}

/// Dispatch to the right encoder based on `preset.body`.
pub fn encode_preset(preset: &Preset, save_to_current: bool) -> Result<Vec<u8>, EncodeError> {
    let bytes = match &preset.body {
        PresetBody::Simple { .. } => encode_simple_preset(preset, save_to_current),
        PresetBody::Advanced { .. } => encode_advanced_preset(preset, save_to_current),
    }?;
    debug!(
        "encoded preset {:?} (bank {} preset {}, mode {:?}, save_to_current={save_to_current}) → {} bytes",
        preset.name,
        preset.bank,
        preset.number,
        preset.body.mode(),
        bytes.len()
    );
    Ok(bytes)
}

fn ascii_trim_right(s: &str, max_len: usize) -> Vec<u8> {
    let mut bytes: Vec<u8> = s
        .chars()
        .map(|c| if c.is_ascii() { c as u8 } else { b'?' })
        .collect();
    while bytes.last() == Some(&b' ') {
        bytes.pop();
    }
    if bytes.len() > max_len {
        bytes.truncate(max_len);
    }
    bytes
}

fn is_combining_mark(c: char) -> bool {
    let v = c as u32;
    (0x0300..=0x036F).contains(&v)
        || (0x1AB0..=0x1AFF).contains(&v)
        || (0x1DC0..=0x1DFF).contains(&v)
        || (0x20D0..=0x20FF).contains(&v)
        || (0xFE20..=0xFE2F).contains(&v)
}

/// NFKD-fold a string and produce a 7-bit ASCII byte sequence.
///
/// The device's display is 7-bit ASCII only. The official editor just
/// masks `& 127`, which would corrupt 'í' (0xED) into 'm' (0x6D). We do
/// the friendlier thing: NFKD-decompose, drop combining marks, then
/// keep ASCII chars; anything still non-ASCII becomes '?'.
fn ascii_fold_bytes(s: &str) -> Vec<u8> {
    let folded: String = s.nfkd().collect();
    folded
        .chars()
        .filter(|c| !is_combining_mark(*c))
        .map(|c| {
            if c.is_ascii() {
                (c as u8) & 0x7F
            } else {
                b'?' & 0x7F
            }
        })
        .collect()
}

/// Build the SysEx the editor sends when the user clicks Save in
/// Controller Settings. Mirrors `getControllerSysex()` exactly.
pub fn encode_controller_with(
    device: DeviceProfile,
    controller: &Controller,
) -> Result<Vec<u8>, EncodeError> {
    let header = build_header_with(device, 6, 1, 0, 0, 0, 0, 0, 0);

    let mut segments: Vec<Vec<u8>> = Vec::new();

    let connectors_by_slug: [(&str, &crate::presets::Connector); 14] = [
        ("a_tip", &controller.connectors.a_tip),
        ("a_ring", &controller.connectors.a_ring),
        ("b_tip", &controller.connectors.b_tip),
        ("b_ring", &controller.connectors.b_ring),
        ("c_tip", &controller.connectors.c_tip),
        ("c_ring", &controller.connectors.c_ring),
        ("d_tip", &controller.connectors.d_tip),
        ("d_ring", &controller.connectors.d_ring),
        ("e_tip", &controller.connectors.e_tip),
        ("e_ring", &controller.connectors.e_ring),
        ("input_tip", &controller.connectors.input_tip),
        ("input_ring", &controller.connectors.input_ring),
        ("output_tip", &controller.connectors.output_tip),
        ("output_ring", &controller.connectors.output_ring),
    ];
    // Sanity check that the table order matches CONNECTOR_SLUGS.
    debug_assert!(
        connectors_by_slug
            .iter()
            .map(|(s, _)| *s)
            .eq(CONNECTOR_SLUGS.iter().copied())
    );

    // 1. Long names — segment ids 0..13.
    for (i, (_, conn)) in connectors_by_slug.iter().enumerate() {
        segments.push(encode_segment(i as u8, &ascii_fold_bytes(&conn.name)));
    }
    // 2. Short names — segment ids 48..61.
    for (i, (_, conn)) in connectors_by_slug.iter().enumerate() {
        segments.push(encode_segment(
            (i as u8) + segment_id::SHORT_NAMES_FIRST,
            &ascii_fold_bytes(&conn.short_name),
        ));
    }
    // 3. Input-side labels — segment ids 64..77.
    for (i, (_, conn)) in connectors_by_slug.iter().enumerate() {
        segments.push(encode_segment(
            (i as u8) + segment_id::INPUT_LABEL_FIRST,
            &ascii_fold_bytes(&conn.input_name),
        ));
    }
    // 4. Output-side labels — segment ids 80..93.
    for (i, (_, conn)) in connectors_by_slug.iter().enumerate() {
        segments.push(encode_segment(
            (i as u8) + segment_id::OUTPUT_LABEL_FIRST,
            &ascii_fold_bytes(&conn.output_name),
        ));
    }

    // 5. MIDI channel.
    segments.push(encode_segment(
        segment_id::MIDI_CHANNEL,
        &[controller.midi_channel & 0x7F],
    ));

    // 6. include_in_trails bitmap. 2 packed 7-bit bytes: data[0] = HIGH 7 bits,
    //    data[1] = LOW 7 bits. Only bits 0..9 (the 10 loops) are meaningful.
    let trails_flags: [bool; 10] = [
        controller.include_in_trails.a_tip,
        controller.include_in_trails.a_ring,
        controller.include_in_trails.b_tip,
        controller.include_in_trails.b_ring,
        controller.include_in_trails.c_tip,
        controller.include_in_trails.c_ring,
        controller.include_in_trails.d_tip,
        controller.include_in_trails.d_ring,
        controller.include_in_trails.e_tip,
        controller.include_in_trails.e_ring,
    ];
    let mut bits: u16 = 0;
    for (i, &flag) in trails_flags.iter().enumerate() {
        if flag {
            bits |= 1u16 << i;
        }
    }
    segments.push(encode_segment(
        segment_id::INCLUDE_IN_TRAILS,
        &[((bits >> 7) as u8) & 0x7F, (bits as u8) & 0x7F],
    ));

    // 7. Device ID.
    segments.push(encode_segment(
        segment_id::DEVICE_ID,
        &[controller.device_id & 0x7F],
    ));
    // 8. Input split. Booleans go on the wire as 127, not 1.
    segments.push(encode_segment(
        segment_id::INPUT_SPLIT,
        &[if controller.input_split { 0x7F } else { 0 }],
    ));
    // 9. Loop bypass persistent.
    segments.push(encode_segment(
        segment_id::LOOP_BYPASS_PERSIST,
        &[if controller.loop_bypass_persistent {
            0x7F
        } else {
            0
        }],
    ));

    Ok(frame_with(device, &header, &segments)?)
}

pub fn encode_controller(controller: &Controller) -> Result<Vec<u8>, EncodeError> {
    encode_controller_with(ML10X, controller)
}

/// Build the SysEx the editor sends when the user clicks a bank button:
/// `(P2=22, P3=bank)`. Selecting a bank implicitly resets the device's
/// current preset to preset 0 of that bank.
pub fn encode_select_bank_with(device: DeviceProfile, bank: u8) -> Result<Vec<u8>, EncodeError> {
    let header = build_header_with(device, 0, 22, bank & 0x7F, 0, 0, 0, 0, 0);
    Ok(frame_with(device, &header, &[])?)
}

pub fn encode_select_bank(bank: u8) -> Result<Vec<u8>, EncodeError> {
    encode_select_bank_with(ML10X, bank)
}

/// Build the SysEx the editor sends when the user clicks a preset button
/// in the current bank: `(P2=18, P3=preset)`.
pub fn encode_select_preset_with(
    device: DeviceProfile,
    preset: u8,
) -> Result<Vec<u8>, EncodeError> {
    let header = build_header_with(device, 0, 18, preset & 0x7F, 0, 0, 0, 0, 0);
    Ok(frame_with(device, &header, &[])?)
}

pub fn encode_select_preset(preset: u8) -> Result<Vec<u8>, EncodeError> {
    encode_select_preset_with(ML10X, preset)
}

/// Ask the device to send the preset-names dump for a specific bank
/// (P2=64). The device responds with one f1=6 f2=2 message containing
/// all 128 preset names for that bank.
pub fn encode_request_preset_names_with(
    device: DeviceProfile,
    bank: u8,
) -> Result<Vec<u8>, EncodeError> {
    let header = build_header_with(device, 0, 64, bank & 0x7F, 0, 0, 0, 0, 0);
    Ok(frame_with(device, &header, &[])?)
}

pub fn encode_request_preset_names(bank: u8) -> Result<Vec<u8>, EncodeError> {
    encode_request_preset_names_with(ML10X, bank)
}

/// Two-message navigation sequence to move the device's current-preset
/// pointer to (bank, preset). Bank first, preset second.
pub fn encode_navigate_to(bank: u8, preset: u8) -> Result<(Vec<u8>, Vec<u8>), EncodeError> {
    Ok((encode_select_bank(bank)?, encode_select_preset(preset)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spillover_byte_table_is_scrambled() {
        // Spot-check the values that the user reported as broken when
        // a previous linear-table version shipped.
        assert_eq!(spillover_write_byte(SpilloverTarget::Nothing), 127);
        assert_eq!(spillover_write_byte(SpilloverTarget::ARing), 0);
        assert_eq!(spillover_write_byte(SpilloverTarget::BTip), 15);
        assert_eq!(spillover_write_byte(SpilloverTarget::DTip), 13);
    }

    #[test]
    fn ascii_fold_strips_diacritics() {
        assert_eq!(ascii_fold_bytes("Saída"), b"Saida".to_vec());
        assert_eq!(ascii_fold_bytes("Entrada"), b"Entrada".to_vec());
    }

    #[test]
    fn navigate_to_emits_p2_22_and_p2_18() {
        use crate::sysex::parse_header;
        let (bm, pm) = encode_navigate_to(2, 42).unwrap();
        let bh = parse_header(&bm).unwrap();
        let ph = parse_header(&pm).unwrap();
        assert_eq!((bh.p2, bh.p3), (22, 2));
        assert_eq!((ph.p2, ph.p3), (18, 42));
    }

    #[test]
    fn encode_preset_dispatches_on_body() {
        use crate::presets::{Connection, PresetBody};
        use crate::sysex::parse_header;

        let simple = Preset {
            bank: 0,
            number: 0,
            name: "S".into(),
            spillover: Default::default(),
            body: PresetBody::Simple { chain: vec![] },
        };
        let bytes = encode_preset(&simple, true).unwrap();
        assert_eq!(parse_header(&bytes).unwrap().p2, 0, "simple writes p2=0");

        let advanced = Preset {
            bank: 0,
            number: 0,
            name: "A".into(),
            spillover: Default::default(),
            body: PresetBody::Advanced {
                connections: vec![Connection {
                    from_connector: crate::presets::ConnectorSlug::InputTip,
                    to_connector: crate::presets::ConnectorSlug::OutputTip,
                }],
            },
        };
        let bytes = encode_preset(&advanced, true).unwrap();
        assert_eq!(parse_header(&bytes).unwrap().p2, 2, "advanced writes p2=2");
    }
}
