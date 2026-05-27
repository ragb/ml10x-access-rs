//! `wasm-bindgen` wrappers around `ml10x-core`.
//!
//! Exposes the codec — byte builders, byte parsers, YAML, validation,
//! diff — to JavaScript with real TypeScript types (via `tsify-next`)
//! rather than `any`. The browser-side editor owns the MIDI transport via
//! the Web MIDI API; this crate is purely a codec.

use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

use ml10x_core::device::CONNECTOR_SLUGS;
use ml10x_core::diff::{self, Difference};
use ml10x_core::encode;
use ml10x_core::handshake;
use ml10x_core::inbound::{self, InboundMessage};
use ml10x_core::presets::{Controller, Preset};
use ml10x_core::sysex::{self, HeaderInfo};
use ml10x_core::validate::{self, ValidationReport};
use ml10x_core::yaml;

/// Install a panic hook that pipes Rust panics to `console.error`. Called
/// automatically when the WASM module loads.
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

fn js_err<E: std::fmt::Display>(e: E) -> JsValue {
    JsValue::from_str(&e.to_string())
}

fn bytes_to_uint8array(bytes: &[u8]) -> Uint8Array {
    let arr = Uint8Array::new_with_length(bytes.len() as u32);
    arr.copy_from(bytes);
    arr
}

// ─────────────────────────── outbound builders ───────────────────────────

/// The four connect-handshake SysEx frames the editor sends after opening
/// the MIDI port. Send them in order; the device replies with the active
/// preset + controller dump unprompted.
#[wasm_bindgen(js_name = handshakeMessages)]
pub fn handshake_messages() -> Result<Vec<Uint8Array>, JsValue> {
    let frames = handshake::handshake_messages().map_err(js_err)?;
    Ok(frames.iter().map(|f| bytes_to_uint8array(f)).collect())
}

#[wasm_bindgen(js_name = encodeSelectBank)]
pub fn encode_select_bank(bank: u8) -> Result<Uint8Array, JsValue> {
    encode::encode_select_bank(bank)
        .map(|b| bytes_to_uint8array(&b))
        .map_err(js_err)
}

#[wasm_bindgen(js_name = encodeSelectPreset)]
pub fn encode_select_preset(preset: u8) -> Result<Uint8Array, JsValue> {
    encode::encode_select_preset(preset)
        .map(|b| bytes_to_uint8array(&b))
        .map_err(js_err)
}

/// Returns `[bankMsg, presetMsg]`. Send them in that order to move the
/// device's pointer to (bank, preset).
#[wasm_bindgen(js_name = encodeNavigateTo)]
pub fn encode_navigate_to(bank: u8, preset: u8) -> Result<Vec<Uint8Array>, JsValue> {
    let (b, p) = encode::encode_navigate_to(bank, preset).map_err(js_err)?;
    Ok(vec![bytes_to_uint8array(&b), bytes_to_uint8array(&p)])
}

/// Encode a preset for writing to the device. Dispatches to Simple or
/// Advanced based on the preset's `body.mode`.
///
/// `save_to_current=true` uses the P3=127 sentinel ("save to currently
/// selected preset") that the official editor uses — the current firmware
/// only accepts that form.
#[wasm_bindgen(js_name = encodePreset)]
pub fn encode_preset(preset: Preset, save_to_current: bool) -> Result<Uint8Array, JsValue> {
    encode::encode_preset(&preset, save_to_current)
        .map(|b| bytes_to_uint8array(&b))
        .map_err(js_err)
}

#[wasm_bindgen(js_name = encodeController)]
pub fn encode_controller(controller: Controller) -> Result<Uint8Array, JsValue> {
    encode::encode_controller(&controller)
        .map(|b| bytes_to_uint8array(&b))
        .map_err(js_err)
}

#[wasm_bindgen(js_name = encodeRequestPresetNames)]
pub fn encode_request_preset_names(bank: u8) -> Result<Uint8Array, JsValue> {
    encode::encode_request_preset_names(bank)
        .map(|b| bytes_to_uint8array(&b))
        .map_err(js_err)
}

// ─────────────────────────── inbound parsers ─────────────────────────────

/// Identify what the device just sent and decode it in one call.
#[wasm_bindgen(js_name = classifyInbound)]
pub fn classify_inbound(bytes: &[u8]) -> Result<InboundMessage, JsValue> {
    inbound::classify_inbound(bytes).map_err(js_err)
}

/// Low-level header parse. Use `classifyInbound` for routing — this is
/// for cases that need the raw header bytes.
#[wasm_bindgen(js_name = parseHeader)]
pub fn parse_header(bytes: &[u8]) -> Result<HeaderInfo, JsValue> {
    sysex::parse_header(bytes).map_err(js_err)
}

// ─────────────────────────── YAML codec ──────────────────────────────────

#[wasm_bindgen(js_name = presetFromYaml)]
pub fn preset_from_yaml(text: &str) -> Result<Preset, JsValue> {
    yaml::preset_from_yaml_str(text, "<input>").map_err(js_err)
}

#[wasm_bindgen(js_name = presetToYaml)]
pub fn preset_to_yaml(preset: Preset) -> Result<String, JsValue> {
    yaml::preset_to_yaml_string(&preset).map_err(js_err)
}

#[wasm_bindgen(js_name = controllerFromYaml)]
pub fn controller_from_yaml(text: &str) -> Result<Controller, JsValue> {
    yaml::controller_from_yaml_str(text, "<input>").map_err(js_err)
}

#[wasm_bindgen(js_name = controllerToYaml)]
pub fn controller_to_yaml(controller: Controller) -> Result<String, JsValue> {
    yaml::controller_to_yaml_string(&controller).map_err(js_err)
}

// ─────────────────────────── validation + diff ───────────────────────────

/// Run the JSON-schema validator plus the semantic checks against a
/// preset. Empty `errors` → safe to encode.
#[wasm_bindgen(js_name = validatePreset)]
pub fn validate_preset(preset: Preset) -> ValidationReport {
    validate::validate_preset_full(&preset)
}

/// Diff two presets field by field. Empty array → field-identical.
#[wasm_bindgen(js_name = diffPresets)]
pub fn diff_presets(local: Preset, device: Preset) -> Vec<Difference> {
    diff::diff_presets(&local, &device)
}

// ─────────────────────────── constants ───────────────────────────────────

#[wasm_bindgen(js_name = connectorSlugs)]
pub fn connector_slugs() -> Vec<String> {
    CONNECTOR_SLUGS.iter().map(|s| s.to_string()).collect()
}

#[wasm_bindgen(js_name = presetSchemaUrl)]
pub fn preset_schema_url() -> String {
    yaml::PRESET_SCHEMA_URL.to_string()
}

#[wasm_bindgen(js_name = controllerSchemaUrl)]
pub fn controller_schema_url() -> String {
    yaml::CONTROLLER_SCHEMA_URL.to_string()
}
