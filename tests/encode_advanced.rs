//! Tests for Advanced-mode preset encoding.

mod common;

use ml10x::decode::decode_preset;
use ml10x::device::segment_id;
use ml10x::encode::{encode_advanced_preset, encode_preset};
use ml10x::presets::{ChainHop, ConnectorSlug, Preset, PresetMode};
use ml10x::sysex::{iter_segments, parse_header};
use pretty_assertions::assert_eq;
use std::collections::HashMap;

const CLEAN_FIXTURE: &str = include_str!("fixtures/real-device-clean-advanced-probe.json");

fn empty_advanced() -> Preset {
    Preset {
        bank: 0,
        number: 2,
        name: "Empty".into(),
        mode: PresetMode::Advanced,
        chain: vec![],
        spillover: Default::default(),
    }
}

fn segments_map(msg: &[u8]) -> HashMap<u8, Vec<u8>> {
    iter_segments(msg).unwrap().into_iter().collect()
}

#[test]
fn advanced_uses_p2_equal_2() {
    let msg = encode_advanced_preset(&empty_advanced(), true).unwrap();
    let h = parse_header(&msg).unwrap();
    assert_eq!(h.p1, 6);
    assert_eq!(h.p2, 2);
    assert_eq!(h.p3, 0x7F); // save-current sentinel
}

#[test]
fn advanced_emits_chain_as_target_id_source_data() {
    let mut p = empty_advanced();
    p.chain = vec![
        ChainHop { from_connector: ConnectorSlug::InputTip, to_connector: ConnectorSlug::ARing, bypass: false },
        ChainHop { from_connector: ConnectorSlug::ARing, to_connector: ConnectorSlug::ATip, bypass: false },
        ChainHop { from_connector: ConnectorSlug::ATip, to_connector: ConnectorSlug::OutputTip, bypass: false },
    ];
    let msg = encode_advanced_preset(&p, true).unwrap();
    let segs = segments_map(&msg);
    assert_eq!(segs[&9], vec![0u8]); // A Ring <- Input Tip (gn 0)
    assert_eq!(segs[&4], vec![9u8]); // A Tip  <- A Ring (gn 9)
    assert_eq!(segs[&2], vec![4u8]); // Output Tip <- A Tip (gn 4)
}

#[test]
fn advanced_includes_required_segments() {
    let msg = encode_advanced_preset(&empty_advanced(), true).unwrap();
    let segs = segments_map(&msg);
    assert!(segs.contains_key(&segment_id::SPILLOVER_OUTPUT_TIP));
    assert!(segs.contains_key(&segment_id::SPILLOVER_OUTPUT_RING));
    assert!(segs.contains_key(&segment_id::ADV_FLAG_18));
    assert!(segs.contains_key(&segment_id::ADV_FLAG_19));
    assert!(segs.contains_key(&segment_id::PRESET_NAME));
    assert_eq!(segs[&segment_id::ADV_FLAG_19].len(), 3);
}

#[test]
fn advanced_ignores_bypass_flags() {
    let mut p = empty_advanced();
    p.chain = vec![
        ChainHop { from_connector: ConnectorSlug::ATip, to_connector: ConnectorSlug::OutputTip, bypass: true },
        ChainHop { from_connector: ConnectorSlug::InputTip, to_connector: ConnectorSlug::ATip, bypass: true },
    ];
    let msg = encode_advanced_preset(&p, true).unwrap();
    let segs = segments_map(&msg);
    assert_eq!(segs[&segment_id::ADV_FLAG_19], vec![0u8, 0, 0]);
}

#[test]
fn advanced_byte_exact_against_captured_clean() {
    let cap: serde_json::Value = serde_json::from_str(CLEAN_FIXTURE).unwrap();
    let original = common::captured(&cap, "clean_advanced");
    let mut preset = decode_preset(&original, 0, 1).unwrap();
    // Force Advanced (the captured P5 may not be reliable as a mode flag).
    preset.mode = PresetMode::Advanced;
    let encoded = encode_advanced_preset(&preset, true).unwrap();
    assert_eq!(encoded, original);
}

#[test]
fn encode_preset_dispatches_by_mode() {
    let simple = Preset { bank: 0, number: 0, name: "S".into(), mode: PresetMode::Simple, chain: vec![], spillover: Default::default() };
    let advanced = Preset { bank: 0, number: 0, name: "A".into(), mode: PresetMode::Advanced, chain: vec![], spillover: Default::default() };
    let sm = encode_preset(&simple, true).unwrap();
    let am = encode_preset(&advanced, true).unwrap();
    assert_eq!(parse_header(&sm).unwrap().p2, 0);
    assert_eq!(parse_header(&am).unwrap().p2, 2);
}
