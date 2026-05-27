//! Shared helpers for the device-touching subcommands.

use std::time::{Duration, Instant};

use crate::device::InboundClass;
use crate::midi_io::MidiIo;
use crate::output::Out;
use crate::presets::Preset;
use crate::sysex::parse_header_with;
use ml10x_core::inbound::{InboundMessage, classify_inbound};

/// Drain the inbound stream after a handshake or navigation, returning when
/// no message has arrived for `idle_for`.
pub fn drain_settle(io: &MidiIo, idle_for: Duration, max_total: Duration) {
    let deadline = Instant::now() + max_total;
    while Instant::now() < deadline {
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .min(idle_for);
        if remaining.is_zero() {
            break;
        }
        if io.receive_sysex(remaining).is_none() {
            return;
        }
    }
}

/// Try to read a preset response from the device for the given (bank, number)
/// within `total` time, retrying once if the first attempt times out.
///
/// We force `bank` / `number` onto the returned `Preset` rather than trust
/// the wire bytes — under repeated selects the device occasionally echoes a
/// stale slot number, and we want the preset labelled with what we asked
/// for.
pub fn try_read_preset(io: &mut MidiIo, bank: u8, number: u8, total: Duration) -> Option<Preset> {
    for attempt in 0..2 {
        io.drain();
        let select_msg =
            crate::encode::encode_select_preset(number).expect("select preset encodes");
        let _ = io.send_sysex(&select_msg);
        let deadline = Instant::now()
            + if attempt == 0 {
                total
            } else {
                total + Duration::from_millis(500)
            };
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let Some(reply) = io.receive_sysex(remaining.min(Duration::from_millis(500))) else {
                continue;
            };
            if let Ok(InboundMessage::Preset { preset, .. }) = classify_inbound(&reply) {
                let mut p = preset;
                p.bank = bank;
                p.number = number;
                return Some(p);
            }
        }
    }
    None
}

/// Walk a captured response stream and return the first UUID it finds.
pub fn extract_uuid(messages: &[Vec<u8>]) -> Option<String> {
    messages.iter().find_map(|m| match classify_inbound(m) {
        Ok(InboundMessage::ControllerInfo { uuid }) => uuid,
        _ => None,
    })
}

/// Print a short summary of what the device just sent.
pub fn announce_inbound(
    out: &mut Out,
    header_p1: u8,
    header_p2: u8,
    header_p3: u8,
    msg_len: usize,
) {
    if header_p1 == InboundClass::EditorEvent as u8 {
        let label = match header_p2 {
            0 => "device connected".to_string(),
            2 => "preset saved (ack)".to_string(),
            5 => "loading start".to_string(),
            6 => "loading end".to_string(),
            7 => format!("loading progress {header_p3}"),
            other => format!("editor event {other}"),
        };
        out.detail(&format!("  device says: {label}."), None);
    } else if header_p1 == InboundClass::Data as u8 {
        let kind = match header_p2 {
            0 => "preset data",
            1 => "controller data",
            2 => "all preset names",
            _ => "data",
        };
        out.detail(&format!("  received {kind} ({msg_len} bytes)."), None);
    } else if header_p1 == InboundClass::ControllerInfo as u8 {
        out.detail(
            &format!("  received controller info ({msg_len} bytes)."),
            None,
        );
    } else {
        out.detail(
            &format!("  received message class {header_p1} subtype {header_p2} ({msg_len} bytes)."),
            None,
        );
    }
}

/// Parse + classify an inbound message (returns p1/p2/p3 if valid).
pub fn classify(message: &[u8]) -> Option<(u8, u8, u8)> {
    let h = parse_header_with(crate::device::ML10X, message, false).ok()?;
    Some((h.p1, h.p2, h.p3))
}
