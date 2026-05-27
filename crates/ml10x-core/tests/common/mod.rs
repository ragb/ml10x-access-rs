//! Shared helpers for integration tests.

use serde_json::Value;

/// Find the first `out` message whose `phase` matches and return its `bytes` as a Vec<u8>.
///
/// `#[allow(dead_code)]` because Rust compiles each integration test as
/// its own crate; tests that don't reference `captured` still pull in the
/// shared module and see the function as unused.
#[allow(dead_code)]
pub fn captured(capture: &Value, phase: &str) -> Vec<u8> {
    let out = capture["out"].as_array().expect("fixture has 'out' array");
    let msg = out
        .iter()
        .find(|m| m["phase"].as_str() == Some(phase))
        .unwrap_or_else(|| panic!("fixture has no phase {phase:?}"));
    msg["bytes"]
        .as_array()
        .expect("message has 'bytes' array")
        .iter()
        .map(|v| v.as_u64().expect("byte is integer") as u8)
        .collect()
}
