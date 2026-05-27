//! Classify and decode an inbound SysEx message in one call.
//!
//! Every command that talks to the device repeats the same routing dance:
//!
//! ```ignore
//! let p1 = msg[HeaderPos::FunctionId1 as usize];
//! let p2 = msg[HeaderPos::FunctionId2 as usize];
//! if p1 == InboundClass::Data as u8 && p2 == 0 {
//!     let bank = msg[HeaderPos::FunctionId4 as usize];
//!     let number = msg[HeaderPos::FunctionId3 as usize];
//!     match decode_preset(msg, bank, number) { ... }
//! } else if p1 == InboundClass::Data as u8 && p2 == 1 {
//!     match decode_controller(msg) { ... }
//! }
//! ```
//!
//! `classify_inbound` collapses that into one function so the routing
//! lives in exactly one place — and so the WASM editor side doesn't have
//! to re-implement it in TypeScript.

use std::collections::HashMap;

use log::{debug, trace};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::decode::{DecodeError, decode_controller, decode_preset, decode_preset_names};
use crate::device::{HeaderPos, InboundClass, segment_id};
use crate::presets::{Controller, Preset};
use crate::sysex::{SysexError, decode_uuid_nibbles, iter_segments};

#[cfg(feature = "tsify")]
use tsify_next::Tsify;

/// Tagged enum describing what the device just sent.
///
/// The `Other` variant carries the raw header bytes so callers can decide
/// what to do with messages this helper doesn't know about (yet) without
/// re-parsing the header.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "tsify", derive(Tsify))]
#[cfg_attr(feature = "tsify", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InboundMessage {
    /// A preset's contents. `bank` and `number` are pulled from the header
    /// (P4 / P3 respectively) — these are the device's idea of which slot
    /// the data belongs to.
    Preset {
        bank: u8,
        number: u8,
        preset: Preset,
    },
    /// Global controller settings (`Data` class, P2 = 1).
    Controller(Controller),
    /// The 128 preset names for one bank (`PresetNames` class). The bank
    /// number is taken from P3 of the header.
    PresetNames {
        bank: u8,
        /// `serde_json` flattens this to a plain JS object keyed by the
        /// preset number, so override the TS shape to match.
        #[cfg_attr(feature = "tsify", tsify(type = "Record<number, string>"))]
        names: HashMap<u8, String>,
    },
    /// Controller info: device UUID and (eventually) firmware. Sent
    /// unsolicited after the connect handshake.
    ControllerInfo { uuid: Option<String> },
    /// Editor event. `p2 = 2` is the "Preset Settings Saved" ack the
    /// device emits after a successful preset write; other P2 codes are
    /// connection / loading lifecycle events.
    EditorEvent { p2: u8, p3: u8 },
    /// Anything else. Callers fall through to their own handling.
    Other {
        p1: u8,
        p2: u8,
        p3: u8,
        p4: u8,
        p5: u8,
    },
}

#[derive(Debug, Error)]
pub enum ClassifyError {
    #[error("Message too short ({0} bytes) to contain a SysEx header")]
    TooShort(usize),
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error(transparent)]
    Sysex(#[from] SysexError),
}

/// Inspect a complete SysEx frame (`F0 … F7`) and decode it into the
/// `InboundMessage` shape that fits its header.
///
/// Does not validate framing beyond what the called decoder needs — call
/// `parse_header_with(.., strict=true)` first if you need that.
pub fn classify_inbound(message: &[u8]) -> Result<InboundMessage, ClassifyError> {
    if message.len() <= HeaderPos::FunctionId5 as usize {
        return Err(ClassifyError::TooShort(message.len()));
    }
    let p1 = message[HeaderPos::FunctionId1 as usize];
    let p2 = message[HeaderPos::FunctionId2 as usize];
    let p3 = message[HeaderPos::FunctionId3 as usize];
    let p4 = message[HeaderPos::FunctionId4 as usize];
    let p5 = message[HeaderPos::FunctionId5 as usize];
    trace!(
        "classify_inbound: {} bytes, p1={p1} p2={p2} p3={p3} p4={p4} p5={p5}",
        message.len()
    );

    if p1 == InboundClass::EditorEvent as u8 {
        return Ok(InboundMessage::EditorEvent { p2, p3 });
    }

    if p1 == InboundClass::Data as u8 {
        // P2 = 0 is Simple-mode preset READ; P2 = 2 carries the Advanced
        // variant. `decode_preset` handles both.
        if p2 == 0 || p2 == 2 {
            // The wire layout puts the preset number in P3 and the bank
            // in P4.
            let number = p3;
            let bank = p4;
            let preset = decode_preset(message, bank, number)?;
            debug!(
                "decoded preset bank {} preset {} ({:?}, mode {:?})",
                bank,
                number,
                preset.name,
                preset.mode()
            );
            return Ok(InboundMessage::Preset {
                bank,
                number,
                preset,
            });
        }
        if p2 == 1 {
            let controller = decode_controller(message)?;
            debug!(
                "decoded controller (midi_channel={}, device_id={})",
                controller.midi_channel, controller.device_id
            );
            return Ok(InboundMessage::Controller(controller));
        }
    }

    if p1 == InboundClass::PresetNames as u8 {
        let names = decode_preset_names(message)?;
        debug!("decoded {} preset names for bank {}", names.len(), p3);
        return Ok(InboundMessage::PresetNames { bank: p3, names });
    }

    if p1 == InboundClass::ControllerInfo as u8 {
        let uuid = extract_uuid(message)?;
        debug!("decoded controller info (uuid present: {})", uuid.is_some());
        return Ok(InboundMessage::ControllerInfo { uuid });
    }

    debug!("inbound message did not match any known class: p1={p1} p2={p2}");
    Ok(InboundMessage::Other { p1, p2, p3, p4, p5 })
}

fn extract_uuid(message: &[u8]) -> Result<Option<String>, SysexError> {
    for (id, data) in iter_segments(message)? {
        if id == segment_id::UUID_OR_FIRMWARE {
            if let Ok(uuid) = decode_uuid_nibbles(&data) {
                return Ok(Some(uuid));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::ML10X;
    use crate::sysex::{build_header_with, frame_with};

    fn frame(p1: u8, p2: u8, p3: u8, p4: u8, p5: u8) -> Vec<u8> {
        let header = build_header_with(ML10X, p1, p2, p3, p4, p5, 0, 0, 0);
        frame_with(ML10X, &header, &[]).unwrap()
    }

    #[test]
    fn editor_event_is_classified() {
        let bytes = frame(InboundClass::EditorEvent as u8, 2, 0, 0, 0);
        match classify_inbound(&bytes).unwrap() {
            InboundMessage::EditorEvent { p2: 2, .. } => {}
            other => panic!("expected EditorEvent, got {other:?}"),
        }
    }

    #[test]
    fn unknown_message_falls_through_to_other() {
        let bytes = frame(99, 88, 77, 66, 55);
        match classify_inbound(&bytes).unwrap() {
            InboundMessage::Other {
                p1: 99,
                p2: 88,
                p3: 77,
                p4: 66,
                p5: 55,
            } => {}
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn too_short_returns_error() {
        let res = classify_inbound(&[0xF0, 0xF7]);
        assert!(matches!(res, Err(ClassifyError::TooShort(_))));
    }
}
