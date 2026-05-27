//! Decoder pinned against the real-device captured fixtures.
//!
//! Anything that asserts a specific property of bytes the device
//! actually produced lives here.

mod common;

use ml10x_core::decode::{decode_controller, decode_preset};
use ml10x_core::device::{HeaderPos, InboundClass, segment_id};
use ml10x_core::presets::{ConnectorSlug, PresetBody, PresetMode, SpilloverTarget};
use ml10x_core::sysex::{
    checksum, decode_ascii_name, decode_uuid_nibbles, iter_segments, parse_header_with,
};
use pretty_assertions::assert_eq;
use serde_json::Value;

const CONNECT_FIXTURE: &str = include_str!("fixtures/real-device-connect-bank0.json");
const BYPASS_FIXTURE: &str = include_str!("fixtures/real-device-bypass-toggle-probe.json");
const ADVANCED_FIXTURE: &str = include_str!("fixtures/real-device-advanced-mode-probe.json");
const SAVE_FIXTURE: &str = include_str!("fixtures/real-device-save-preset-0.json");

fn load(text: &str) -> Value {
    serde_json::from_str(text).expect("valid JSON fixture")
}

fn msg_bytes(m: &Value) -> Vec<u8> {
    m["bytes"]
        .as_array()
        .expect("'bytes' array")
        .iter()
        .map(|v| v.as_u64().expect("byte int") as u8)
        .collect()
}

fn in_full(cap: &Value) -> Vec<Value> {
    cap["in_full"].as_array().expect("'in_full' array").clone()
}

fn out_full(cap: &Value) -> Vec<Value> {
    cap["out_full"]
        .as_array()
        .expect("'out_full' array")
        .clone()
}

fn first_with_p1_p2(messages: &[Value], p1: u8, p2: u8) -> Vec<u8> {
    let m = messages
        .iter()
        .find(|m| {
            let b = msg_bytes(m);
            b.len() > 16
                && b[HeaderPos::FunctionId1 as usize] == p1
                && b[HeaderPos::FunctionId2 as usize] == p2
        })
        .unwrap_or_else(|| panic!("no message with p1={p1} p2={p2}"));
    msg_bytes(m)
}

// ----- Outbound / inbound framing (connect fixture) -----

#[test]
fn every_outbound_handshake_message_parses() {
    let cap = load(CONNECT_FIXTURE);
    let out = out_full(&cap);
    assert_eq!(out.len(), 4, "Expected 4 outbound handshake messages");
    let p2_codes: Vec<u8> = out
        .iter()
        .map(|m| msg_bytes(m)[HeaderPos::FunctionId2 as usize])
        .collect();
    assert_eq!(p2_codes, vec![0, 24, 19, 21]);
    for m in &out {
        let b = msg_bytes(m);
        let h = parse_header_with(ml10x_core::device::ML10X, &b, false).unwrap();
        assert_eq!(h.model_id, 0x07);
    }
}

#[test]
fn every_inbound_message_parses_with_length() {
    let cap = load(CONNECT_FIXTURE);
    for m in in_full(&cap) {
        let b = msg_bytes(&m);
        let h = parse_header_with(ml10x_core::device::ML10X, &b, true)
            .unwrap_or_else(|e| panic!("inbound message rejected: {e}\nbytes: {b:?}"));
        assert_eq!(h.declared_length, b.len());
        assert_eq!(h.model_id, 0x07);
    }
}

#[test]
fn inbound_checksums_match() {
    let cap = load(CONNECT_FIXTURE);
    for m in in_full(&cap) {
        let b = msg_bytes(&m);
        assert_eq!(b[b.len() - 2], checksum(&b).unwrap(), "checksum mismatch");
    }
}

// ----- Controller decode against the user's real rig -----

#[test]
fn decode_controller_loop_names_match_real_rig() {
    let cap = load(CONNECT_FIXTURE);
    let bytes = first_with_p1_p2(&in_full(&cap), InboundClass::Data as u8, 1);
    let ctrl = decode_controller(&bytes).unwrap();

    // User's actual rig at capture time. If this changes meaningfully, the
    // names below should be updated to the new fixture's values — these are
    // intentionally tied to the captured ground truth.
    assert_eq!(ctrl.connectors.a_tip.name, "Ego 76");
    assert_eq!(ctrl.connectors.a_ring.name, "archer Icon");
    assert_eq!(ctrl.connectors.b_tip.name, "ODR Mini");
    assert_eq!(ctrl.connectors.b_ring.name, "SD1");
    assert_eq!(ctrl.connectors.c_tip.name, "AT+");
    assert_eq!(ctrl.connectors.c_ring.name, "Big Muff");
    assert_eq!(ctrl.connectors.d_tip.name, "Soul Press");
    assert_eq!(ctrl.connectors.d_ring.name, "Duke of tone");
    assert_eq!(ctrl.connectors.e_tip.name, "OC-5");
    assert_eq!(ctrl.connectors.e_ring.name, "EQ-7");
    assert_eq!(ctrl.connectors.input_tip.name, "guitar");
    assert_eq!(ctrl.connectors.input_ring.name, "Guitar aux");
    assert_eq!(ctrl.connectors.output_tip.name, "Output");
    assert_eq!(ctrl.connectors.output_ring.name, "Oupput aux"); // sic — user's literal label
}

#[test]
fn decode_controller_short_names() {
    let cap = load(CONNECT_FIXTURE);
    let bytes = first_with_p1_p2(&in_full(&cap), InboundClass::Data as u8, 1);
    let ctrl = decode_controller(&bytes).unwrap();
    assert_eq!(ctrl.connectors.a_tip.short_name, "76");
    assert_eq!(ctrl.connectors.a_ring.short_name, "AR");
    assert_eq!(ctrl.connectors.e_ring.short_name, "EQ");
    assert_eq!(ctrl.connectors.input_tip.short_name, "I+");
    assert_eq!(ctrl.connectors.output_ring.short_name, "O-");
}

#[test]
fn decode_controller_global_settings() {
    let cap = load(CONNECT_FIXTURE);
    let bytes = first_with_p1_p2(&in_full(&cap), InboundClass::Data as u8, 1);
    let ctrl = decode_controller(&bytes).unwrap();
    assert_eq!(ctrl.midi_channel, 9);
    assert_eq!(ctrl.device_id, 6);
    assert_eq!(ctrl.input_split, true);
    assert_eq!(ctrl.loop_bypass_persistent, false);
    // include_in_trails bitmap was 00 00 at capture time — all 10 loop toggles off.
    assert!(!ctrl.include_in_trails.a_tip);
    assert!(!ctrl.include_in_trails.a_ring);
    assert!(!ctrl.include_in_trails.e_tip);
    assert!(!ctrl.include_in_trails.e_ring);
}

#[test]
fn controller_uuid_decodes() {
    let cap = load(CONNECT_FIXTURE);
    let info = in_full(&cap)
        .iter()
        .find(|m| {
            msg_bytes(m).get(HeaderPos::FunctionId1 as usize)
                == Some(&(InboundClass::ControllerInfo as u8))
        })
        .map(msg_bytes)
        .expect("CONTROLLER_INFO message");
    let segs = iter_segments(&info).unwrap();
    let (_id, data) = segs
        .iter()
        .find(|(id, _)| *id == segment_id::UUID_OR_FIRMWARE)
        .expect("UUID segment");
    let uuid = decode_uuid_nibbles(data).unwrap();
    assert_eq!(uuid, "64fdfbee-2e38-79d7-0000-000000000000");
}

// ----- Preset name + segment shape (connect fixture) -----

#[test]
fn decode_preset_name_and_mode() {
    let cap = load(CONNECT_FIXTURE);
    let preset_msg = in_full(&cap)
        .iter()
        .find(|m| {
            let b = msg_bytes(m);
            b.len() > 16
                && b[HeaderPos::FunctionId1 as usize] == InboundClass::Data as u8
                && b[HeaderPos::FunctionId2 as usize] == 0
        })
        .map(msg_bytes)
        .expect("preset DATA message");
    let bank = preset_msg[HeaderPos::FunctionId4 as usize];
    let number = preset_msg[HeaderPos::FunctionId3 as usize];
    let p = decode_preset(&preset_msg, bank, number).unwrap();
    assert_eq!(p.name, "Base");
    assert_eq!(p.mode(), PresetMode::Simple);
    assert_eq!(p.bank, 0);
    assert_eq!(p.number, 0);
    assert_eq!(p.spillover.output_tip, SpilloverTarget::Nothing);
    assert_eq!(p.spillover.output_ring, SpilloverTarget::Nothing);
}

#[test]
fn preset_data_carries_preset_name_segment() {
    let cap = load(CONNECT_FIXTURE);
    let bytes = first_with_p1_p2(&in_full(&cap), InboundClass::Data as u8, 0);
    let segs = iter_segments(&bytes).unwrap();
    let name_seg = segs
        .iter()
        .find(|(id, _)| *id == segment_id::PRESET_NAME)
        .expect("name seg");
    assert_eq!(decode_ascii_name(&name_seg.1), "Base");
}

// ----- Bypass toggle differential -----

#[test]
fn decode_preset_2_bypass_toggle_is_single_hop_delta() {
    let cap = load(BYPASS_FIXTURE);
    let out = cap["out"].as_array().unwrap();
    let baseline = msg_bytes(
        out.iter()
            .find(|m| m["phase"] == "baseline_before_bypass")
            .unwrap(),
    );
    let after = msg_bytes(
        out.iter()
            .find(|m| m["phase"] == "after_bypass_toggle")
            .unwrap(),
    );

    let p_before = decode_preset(&baseline, 0, 2).unwrap();
    let p_after = decode_preset(&after, 0, 2).unwrap();

    let chain_of = |p: &ml10x_core::presets::Preset| -> Vec<ml10x_core::presets::SimpleHop> {
        match &p.body {
            PresetBody::Simple { chain } => chain.clone(),
            PresetBody::Advanced { .. } => panic!("bypass fixture should decode as Simple"),
        }
    };
    let before_chain = chain_of(&p_before);
    let after_chain = chain_of(&p_after);

    // The chain shape (ignoring bypass) is identical before/after.
    let shape = |hops: &[ml10x_core::presets::SimpleHop]| -> Vec<(ConnectorSlug, ConnectorSlug)> {
        hops.iter()
            .map(|h| (h.from_connector, h.to_connector))
            .collect()
    };
    assert_eq!(shape(&before_chain), shape(&after_chain));

    // Exactly one hop's bypass changed, and it's the one feeding A Tip.
    let diff: Vec<_> = before_chain
        .iter()
        .zip(after_chain.iter())
        .filter(|(a, b)| a.bypass != b.bypass)
        .collect();
    assert_eq!(diff.len(), 1);
    let (before, after) = diff[0];
    assert_eq!(before.bypass, false);
    assert_eq!(after.bypass, true);
    assert_eq!(before.to_connector, ConnectorSlug::ATip);
}

// ----- Advanced WRITE shape -----

#[test]
fn advanced_write_uses_p2_equal_2_and_omits_simple_segment() {
    let cap = load(ADVANCED_FIXTURE);
    let out = cap["out"].as_array().unwrap();
    let simple = msg_bytes(
        out.iter()
            .find(|m| m["phase"] == "save_preset_2_simple")
            .unwrap(),
    );
    let advanced = msg_bytes(
        out.iter()
            .find(|m| m["phase"] == "save_preset_2_advanced")
            .unwrap(),
    );

    assert_eq!(simple[HeaderPos::FunctionId1 as usize], 6);
    assert_eq!(simple[HeaderPos::FunctionId2 as usize], 0);
    assert_eq!(advanced[HeaderPos::FunctionId1 as usize], 6);
    assert_eq!(advanced[HeaderPos::FunctionId2 as usize], 2);

    let segs: std::collections::HashMap<u8, Vec<u8>> =
        iter_segments(&advanced).unwrap().into_iter().collect();
    assert!(!segs.contains_key(&segment_id::SIMPLE_FLAG_20));
    assert!(segs.contains_key(&segment_id::ADV_FLAG_18));
    assert!(segs.contains_key(&segment_id::ADV_FLAG_19));
    assert!(segs.contains_key(&segment_id::PRESET_NAME));
    assert!(segs.contains_key(&segment_id::SPILLOVER_OUTPUT_TIP));
    assert!(segs.contains_key(&segment_id::SPILLOVER_OUTPUT_RING));
}

// ----- Save preset header shape -----

#[test]
fn save_preset_is_single_p3_127_message() {
    let cap = load(SAVE_FIXTURE);
    let saves: Vec<_> = cap["out"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["phase"] == "save_preset_0_unmodified")
        .collect();
    assert_eq!(saves.len(), 1);
    let b = msg_bytes(saves[0]);
    assert_eq!(b[HeaderPos::FunctionId1 as usize], 6);
    assert_eq!(b[HeaderPos::FunctionId2 as usize], 0);
    assert_eq!(b[HeaderPos::FunctionId3 as usize], 127); // save-current sentinel
    assert_eq!(b[HeaderPos::FunctionId4 as usize], 0);
    // Outbound messages leave the length field zeroed.
    assert_eq!(b[HeaderPos::LengthMsb as usize], 0);
    assert_eq!(b[HeaderPos::LengthLsb as usize], 0);
}

#[test]
fn save_preset_name_segment_strips_trailing_spaces() {
    let cap = load(SAVE_FIXTURE);
    let save = cap["out"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["phase"] == "save_preset_0_unmodified")
        .unwrap();
    let b = msg_bytes(save);
    let segs: std::collections::HashMap<u8, Vec<u8>> =
        iter_segments(&b).unwrap().into_iter().collect();
    assert_eq!(segs[&segment_id::PRESET_NAME], b"Base".to_vec());
}

#[test]
fn device_acknowledges_write_with_editor_event_2() {
    let cap = load(SAVE_FIXTURE);
    let acks: Vec<_> = cap["in"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| {
            let b = msg_bytes(m);
            m["phase"] == "save_preset_0_unmodified"
                && b.len() > 16
                && b[HeaderPos::FunctionId1 as usize] == InboundClass::EditorEvent as u8
                && b[HeaderPos::FunctionId2 as usize] == 2
        })
        .collect();
    assert!(!acks.is_empty(), "expected at least one PresetSaved ack");
}

// ----- All preset names dump -----

#[test]
fn all_preset_names_dump_contains_128_entries() {
    let cap = load(CONNECT_FIXTURE);
    let bytes = first_with_p1_p2(&in_full(&cap), InboundClass::Data as u8, 2);
    let segs = iter_segments(&bytes).unwrap();
    assert_eq!(segs.len(), 128);
    let by_id: std::collections::HashMap<u8, Vec<u8>> = segs.into_iter().collect();
    assert_eq!(decode_ascii_name(&by_id[&0]), "Base");
    assert_eq!(decode_ascii_name(&by_id[&1]), "Clean");
    assert_eq!(decode_ascii_name(&by_id[&2]), "Empty");
}

// Keep `common` referenced so the import isn't flagged.
#[allow(dead_code)]
fn _silence_unused_common() -> Vec<u8> {
    let cap = load(SAVE_FIXTURE);
    common::captured(&cap, "save_preset_0_unmodified")
}
