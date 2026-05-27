//! Decode captured device messages into the in-memory model.
//!
//! Used by the `dump` command to turn captured device responses into
//! YAML, and by the round-trip tests that prove the encoder writes what
//! the device expects.

use std::collections::{HashMap, HashSet};

use thiserror::Error;

use crate::device::{CONNECTOR_SLUGS, HeaderPos, UNROUTED, groupnumber_to_slug, segment_id};
use crate::presets::{
    Connection, Connector, ConnectorSlug, Connectors, Controller, IncludeInTrails, Preset,
    PresetBody, SimpleHop, Spillover, SpilloverTarget,
};
use crate::sysex::{SysexError, decode_ascii_name, iter_segments};

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("Sysex framing error: {0}")]
    Sysex(#[from] SysexError),
    #[error("Message too short for header lookup ({0} bytes)")]
    HeaderTooShort(usize),
}

/// Map a spillover segment to a `SpilloverTarget`.
///
/// READ form is 2 bytes `[is_set, value]`. The editor's parser treats
/// `[0, _]` as Nothing regardless of the value byte; otherwise uses the
/// j2 value at `data[1]`. WRITE form is 1 byte, j2 value directly.
pub fn decode_spillover(seg_data: &[u8]) -> SpilloverTarget {
    if seg_data.is_empty() {
        return SpilloverTarget::Nothing;
    }
    let raw = if seg_data.len() == 1 {
        seg_data[0]
    } else if seg_data[0] == 0 {
        127
    } else {
        seg_data[1]
    };
    match raw {
        127 => SpilloverTarget::Nothing,
        2 => SpilloverTarget::InputTip,
        3 => SpilloverTarget::InputRing,
        1 => SpilloverTarget::ATip,
        0 => SpilloverTarget::ARing,
        15 => SpilloverTarget::BTip,
        4 => SpilloverTarget::BRing,
        11 => SpilloverTarget::CTip,
        6 => SpilloverTarget::CRing,
        13 => SpilloverTarget::DTip,
        12 => SpilloverTarget::DRing,
        9 => SpilloverTarget::ETip,
        10 => SpilloverTarget::ERing,
        _ => SpilloverTarget::Nothing,
    }
}

/// Turn a preset-data SysEx message into a `Preset`.
///
/// Handles three formats:
/// - READ from device (P1=6, P2=0, P5 flags Advanced): 3-byte connection records.
/// - Simple WRITE (P1=6, P2=0, P3=127): 2- or 3-byte records at segments 0..13.
/// - Advanced WRITE (P1=6, P2=2, P3=127): 1-byte segments where the segment id
///   is the target's gn and the data byte is the source's gn.
pub fn decode_preset(message: &[u8], bank: u8, number: u8) -> Result<Preset, DecodeError> {
    if message.len() <= HeaderPos::FunctionId5 as usize {
        return Err(DecodeError::HeaderTooShort(message.len()));
    }
    let segments: HashMap<u8, Vec<u8>> = iter_segments(message)?.into_iter().collect();

    let p2 = message[HeaderPos::FunctionId2 as usize];
    let p5 = message[HeaderPos::FunctionId5 as usize];
    let advanced_flag = p2 == 2 || p5 > 0;

    let name = decode_ascii_name(
        segments
            .get(&segment_id::PRESET_NAME)
            .map(|v| v.as_slice())
            .unwrap_or(&[]),
    );

    let tip_seg = segments.get(&segment_id::SPILLOVER_OUTPUT_TIP);
    let ring_seg = segments.get(&segment_id::SPILLOVER_OUTPUT_RING);
    let spillover = Spillover {
        output_tip: decode_spillover(tip_seg.map(|v| v.as_slice()).unwrap_or(&[])),
        output_ring: decode_spillover(ring_seg.map(|v| v.as_slice()).unwrap_or(&[])),
    };

    let body = if advanced_flag {
        // Advanced: 1-byte segments at ids 0..13.
        // id = target gn, data[0] = source gn.
        // The 3-byte segments at ids 48..63 are the matrixArray which we ignore.
        // Walk segments in id order for deterministic output.
        let mut connections: Vec<Connection> = Vec::new();
        let mut keys: Vec<u8> = segments.keys().copied().collect();
        keys.sort();
        for seg_id in keys {
            if seg_id > 13 {
                continue;
            }
            let data = &segments[&seg_id];
            if data.len() != 1 {
                continue;
            }
            let target_gn = seg_id;
            let source_gn = data[0];
            if let (Some(fc), Some(tc)) =
                (connector_from_gn(source_gn), connector_from_gn(target_gn))
            {
                connections.push(Connection {
                    from_connector: fc,
                    to_connector: tc,
                });
            }
        }
        let order = hop_order(
            &connections
                .iter()
                .map(|c| (c.from_connector, c.to_connector))
                .collect::<Vec<_>>(),
        );
        let ordered: Vec<Connection> = order.into_iter().map(|i| connections[i].clone()).collect();
        PresetBody::Advanced {
            connections: ordered,
        }
    } else {
        // Simple / READ form: 2- or 3-byte segments at ids 0..13.
        let mut chain: Vec<SimpleHop> = Vec::new();
        let mut keys: Vec<u8> = segments.keys().copied().collect();
        keys.sort();
        for seg_id in keys {
            if seg_id > 13 {
                continue;
            }
            let data = &segments[&seg_id];
            if data.len() < 3 {
                continue;
            }
            let from_gn = data[0];
            let to_gn = data[1];
            let bypass_flag = data[2];
            if to_gn == UNROUTED {
                continue;
            }
            if let (Some(fc), Some(tc)) = (connector_from_gn(from_gn), connector_from_gn(to_gn)) {
                chain.push(SimpleHop {
                    from_connector: fc,
                    to_connector: tc,
                    bypass: bypass_flag != 0,
                });
            }
        }
        let order = hop_order(
            &chain
                .iter()
                .map(|h| (h.from_connector, h.to_connector))
                .collect::<Vec<_>>(),
        );
        let ordered: Vec<SimpleHop> = order.into_iter().map(|i| chain[i].clone()).collect();
        PresetBody::Simple { chain: ordered }
    };

    Ok(Preset {
        bank,
        number,
        name,
        spillover,
        body,
    })
}

fn connector_from_gn(gn: u8) -> Option<ConnectorSlug> {
    let slug = groupnumber_to_slug_checked(gn)?;
    ConnectorSlug::from_slug(slug)
}

fn groupnumber_to_slug_checked(gn: u8) -> Option<&'static str> {
    if (gn as usize) >= 14 {
        return None;
    }
    Some(groupnumber_to_slug(gn))
}

/// Compute a permutation of hop indices so reading top-to-bottom follows
/// signal flow from inputs through the device.
///
/// A single connector can fan out to multiple destinations (e.g. mono
/// input splitting to both sides of a stereo loop), so the by-source
/// index is a list per node. We walk from the input roots emitting
/// indices we haven't visited yet; any leftover indices trail at the
/// end. Operates on (from, to) tuples so it works for both Simple
/// SimpleHop and Advanced Connection lists.
fn hop_order(hops: &[(ConnectorSlug, ConnectorSlug)]) -> Vec<usize> {
    if hops.is_empty() {
        return Vec::new();
    }
    let mut by_from: HashMap<ConnectorSlug, Vec<usize>> = HashMap::new();
    for (i, &(f, _)) in hops.iter().enumerate() {
        by_from.entry(f).or_default().push(i);
    }
    let mut visited: HashSet<usize> = HashSet::new();
    let mut ordered: Vec<usize> = Vec::with_capacity(hops.len());

    fn walk(
        node: ConnectorSlug,
        hops: &[(ConnectorSlug, ConnectorSlug)],
        by_from: &HashMap<ConnectorSlug, Vec<usize>>,
        visited: &mut HashSet<usize>,
        ordered: &mut Vec<usize>,
    ) {
        if let Some(idxs) = by_from.get(&node) {
            for &i in idxs {
                if visited.insert(i) {
                    ordered.push(i);
                    walk(hops[i].1, hops, by_from, visited, ordered);
                }
            }
        }
    }

    walk(
        ConnectorSlug::InputTip,
        hops,
        &by_from,
        &mut visited,
        &mut ordered,
    );
    walk(
        ConnectorSlug::InputRing,
        hops,
        &by_from,
        &mut visited,
        &mut ordered,
    );

    for i in 0..hops.len() {
        if !visited.contains(&i) {
            ordered.push(i);
        }
    }
    ordered
}

/// Parse an inbound f1=6 f2=2 ('all preset names for the current bank')
/// message into a {preset_number: name} map.
pub fn decode_preset_names(message: &[u8]) -> Result<HashMap<u8, String>, DecodeError> {
    let mut out = HashMap::new();
    for (seg_id, data) in iter_segments(message)? {
        if seg_id <= 127 {
            out.insert(seg_id, decode_ascii_name(&data));
        }
    }
    Ok(out)
}

/// Turn an inbound f1=6 f2=1 controller-data message into a `Controller`.
pub fn decode_controller(message: &[u8]) -> Result<Controller, DecodeError> {
    let segments: HashMap<u8, Vec<u8>> = iter_segments(message)?.into_iter().collect();
    let mut ctrl = Controller::default();

    // Long names: segments 0..13.
    let long_name_ids = [
        segment_id::LOOP_A_TIP,
        segment_id::LOOP_A_RING,
        segment_id::LOOP_B_TIP,
        segment_id::LOOP_B_RING,
        segment_id::LOOP_C_TIP,
        segment_id::LOOP_C_RING,
        segment_id::LOOP_D_TIP,
        segment_id::LOOP_D_RING,
        segment_id::LOOP_E_TIP,
        segment_id::LOOP_E_RING,
        segment_id::INPUT_TIP,
        segment_id::INPUT_RING,
        segment_id::OUTPUT_TIP,
        segment_id::OUTPUT_RING,
    ];
    for (slug, &seg_id) in CONNECTOR_SLUGS.iter().zip(long_name_ids.iter()) {
        if let Some(data) = segments.get(&seg_id) {
            set_connector_field(&mut ctrl.connectors, slug, |c| {
                c.name = decode_ascii_name(data);
            });
        }
    }

    // Short names: 48..61.
    for (i, slug) in CONNECTOR_SLUGS.iter().enumerate() {
        let seg_id = (i as u8) + segment_id::SHORT_NAMES_FIRST;
        if let Some(data) = segments.get(&seg_id) {
            set_connector_field(&mut ctrl.connectors, slug, |c| {
                c.short_name = decode_ascii_name(data);
            });
        }
    }

    // Input labels: 64..77.
    for (i, slug) in CONNECTOR_SLUGS.iter().enumerate() {
        let seg_id = (i as u8) + segment_id::INPUT_LABEL_FIRST;
        if let Some(data) = segments.get(&seg_id) {
            set_connector_field(&mut ctrl.connectors, slug, |c| {
                c.input_name = decode_ascii_name(data);
            });
        }
    }

    // Output labels: 80..93.
    for (i, slug) in CONNECTOR_SLUGS.iter().enumerate() {
        let seg_id = (i as u8) + segment_id::OUTPUT_LABEL_FIRST;
        if let Some(data) = segments.get(&seg_id) {
            set_connector_field(&mut ctrl.connectors, slug, |c| {
                c.output_name = decode_ascii_name(data);
            });
        }
    }

    if let Some(b) = segments.get(&segment_id::MIDI_CHANNEL) {
        if !b.is_empty() {
            ctrl.midi_channel = b[0];
        }
    }
    if let Some(b) = segments.get(&segment_id::INCLUDE_IN_TRAILS) {
        if b.len() >= 2 {
            // data[0] = HIGH 7 bits, data[1] = LOW 7 bits.
            let bits = ((b[0] as u16) << 7) | (b[1] as u16);
            ctrl.include_in_trails = IncludeInTrails {
                a_tip: bits & (1 << 0) != 0,
                a_ring: bits & (1 << 1) != 0,
                b_tip: bits & (1 << 2) != 0,
                b_ring: bits & (1 << 3) != 0,
                c_tip: bits & (1 << 4) != 0,
                c_ring: bits & (1 << 5) != 0,
                d_tip: bits & (1 << 6) != 0,
                d_ring: bits & (1 << 7) != 0,
                e_tip: bits & (1 << 8) != 0,
                e_ring: bits & (1 << 9) != 0,
            };
        }
    }
    if let Some(b) = segments.get(&segment_id::DEVICE_ID) {
        if !b.is_empty() {
            ctrl.device_id = b[0];
        }
    }
    if let Some(b) = segments.get(&segment_id::INPUT_SPLIT) {
        if !b.is_empty() {
            ctrl.input_split = b[0] != 0;
        }
    }
    if let Some(b) = segments.get(&segment_id::LOOP_BYPASS_PERSIST) {
        if !b.is_empty() {
            ctrl.loop_bypass_persistent = b[0] != 0;
        }
    }

    Ok(ctrl)
}

fn set_connector_field<F: FnOnce(&mut Connector)>(connectors: &mut Connectors, slug: &str, f: F) {
    let target: &mut Connector = match slug {
        "a_tip" => &mut connectors.a_tip,
        "a_ring" => &mut connectors.a_ring,
        "b_tip" => &mut connectors.b_tip,
        "b_ring" => &mut connectors.b_ring,
        "c_tip" => &mut connectors.c_tip,
        "c_ring" => &mut connectors.c_ring,
        "d_tip" => &mut connectors.d_tip,
        "d_ring" => &mut connectors.d_ring,
        "e_tip" => &mut connectors.e_tip,
        "e_ring" => &mut connectors.e_ring,
        "input_tip" => &mut connectors.input_tip,
        "input_ring" => &mut connectors.input_ring,
        "output_tip" => &mut connectors.output_tip,
        "output_ring" => &mut connectors.output_ring,
        _ => return,
    };
    f(target);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spillover_2byte_zero_pair_is_nothing() {
        assert_eq!(decode_spillover(&[0, 0]), SpilloverTarget::Nothing);
        assert_eq!(decode_spillover(&[0, 99]), SpilloverTarget::Nothing);
        assert_eq!(decode_spillover(&[1, 13]), SpilloverTarget::DTip);
        assert_eq!(decode_spillover(&[1, 12]), SpilloverTarget::DRing);
        assert_eq!(decode_spillover(&[127]), SpilloverTarget::Nothing);
        assert_eq!(decode_spillover(&[13]), SpilloverTarget::DTip);
    }
}
