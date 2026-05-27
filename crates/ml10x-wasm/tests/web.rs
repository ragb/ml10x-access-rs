//! Round-trip smoke tests for the wasm-bindgen surface.
//!
//! Run with:
//!
//!     wasm-pack test --node crates/ml10x-wasm
//!
//! These prove that the encode → decode and YAML round-trips work the
//! same in the wasm32 target as they do natively.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

#[wasm_bindgen_test]
fn handshake_emits_four_frames() {
    let frames = ml10x_wasm::handshake_messages().unwrap();
    assert_eq!(frames.len(), 4);
}

#[wasm_bindgen_test]
fn select_bank_returns_bytes() {
    let bytes = ml10x_wasm::encode_select_bank(2).unwrap();
    assert!(bytes.length() > 0);
    assert_eq!(bytes.get_index(0), 0xF0);
    assert_eq!(bytes.get_index(bytes.length() - 1), 0xF7);
}

#[wasm_bindgen_test]
fn preset_yaml_round_trip() {
    let src = r#"
preset:
  bank: 1
  number: 0
  name: Test
  spillover:
    output_tip: nothing
    output_ring: nothing
  body:
    mode: simple
    chain:
      - { from_connector: input_tip, to_connector: output_tip, bypass: false }
"#;
    let parsed = ml10x_wasm::preset_from_yaml(src).unwrap();
    let back = ml10x_wasm::preset_to_yaml(parsed).unwrap();
    assert!(back.contains("name: Test"));
    assert!(back.contains("mode: simple"));
}

#[wasm_bindgen_test]
fn classify_inbound_rejects_short_input() {
    let res = ml10x_wasm::classify_inbound(&[0xF0, 0xF7]);
    assert!(res.is_err());
}

#[wasm_bindgen_test]
fn validate_preset_runs() {
    let src = r#"
preset:
  bank: 1
  number: 0
  name: OK
  spillover: { output_tip: nothing, output_ring: nothing }
  body:
    mode: simple
    chain:
      - { from_connector: input_tip, to_connector: output_tip, bypass: false }
"#;
    let preset = ml10x_wasm::preset_from_yaml(src).unwrap();
    let report = ml10x_wasm::validate_preset(preset);
    assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
}

#[wasm_bindgen_test]
fn diff_presets_typed() {
    let src = r#"
preset:
  bank: 1
  number: 0
  name: A
  spillover: { output_tip: nothing, output_ring: nothing }
  body: { mode: simple, chain: [{ from_connector: input_tip, to_connector: output_tip, bypass: false }] }
"#;
    let mut a = ml10x_wasm::preset_from_yaml(src).unwrap();
    let b = ml10x_wasm::preset_from_yaml(src).unwrap();
    let same = ml10x_wasm::diff_presets(a.clone(), b.clone());
    assert!(same.is_empty());
    a.name = "B".into();
    let differ = ml10x_wasm::diff_presets(a, b);
    assert_eq!(differ.len(), 1);
    assert_eq!(differ[0].field, "name");
}
