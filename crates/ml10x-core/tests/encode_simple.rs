//! Byte-exact round-trip tests for Simple-mode preset encoding.

mod common;

use ml10x_core::decode::decode_preset;
use ml10x_core::encode::encode_simple_preset;
use ml10x_core::presets::{ConnectorSlug, PresetBody};
use pretty_assertions::assert_eq;

const SAVE_FIXTURE: &str = include_str!("fixtures/real-device-save-preset-0.json");
const BYPASS_FIXTURE: &str = include_str!("fixtures/real-device-bypass-toggle-probe.json");

fn load(json: &str) -> serde_json::Value {
    serde_json::from_str(json).expect("valid JSON fixture")
}

#[test]
fn encode_roundtrips_captured_save_byte_exact() {
    // The editor's save of preset 0 (`Base`) should round-trip through
    // decode → encode and emerge byte-for-byte identical.
    let cap = load(SAVE_FIXTURE);
    let original = common::captured(&cap, "save_preset_0_unmodified");
    let preset = decode_preset(&original, 0, 0).unwrap();
    let encoded = encode_simple_preset(&preset, true).unwrap();
    assert_eq!(encoded, original);
}

#[test]
fn encode_roundtrips_bypass_toggle_baseline_byte_exact() {
    let cap = load(BYPASS_FIXTURE);
    let original = common::captured(&cap, "baseline_before_bypass");
    let preset = decode_preset(&original, 0, 2).unwrap();
    let encoded = encode_simple_preset(&preset, true).unwrap();
    assert_eq!(encoded, original);
}

#[test]
fn encode_reflects_bypass_change_byte_exact() {
    let cap = load(BYPASS_FIXTURE);
    let baseline = common::captured(&cap, "baseline_before_bypass");
    let after = common::captured(&cap, "after_bypass_toggle");

    let mut preset = decode_preset(&baseline, 0, 2).unwrap();
    match &mut preset.body {
        PresetBody::Simple { chain } => {
            for hop in chain {
                if hop.to_connector == ConnectorSlug::ATip {
                    hop.bypass = true;
                    break;
                }
            }
        }
        PresetBody::Advanced { .. } => panic!("simple-mode bypass fixture decoded as Advanced"),
    }
    let encoded = encode_simple_preset(&preset, true).unwrap();
    assert_eq!(encoded, after);
}
