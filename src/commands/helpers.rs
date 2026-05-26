//! Shared helpers for the device-touching subcommands.

use std::time::{Duration, Instant};

use crate::decode::decode_preset;
use crate::device::{HeaderPos, InboundClass, segment_id};
use crate::midi_io::MidiIo;
use crate::output::Out;
use crate::presets::Preset;
use crate::sysex::{decode_uuid_nibbles, iter_segments, parse_header_with};

/// Drain the inbound stream after a handshake or navigation, returning when
/// no message has arrived for `idle_for`.
pub fn drain_settle(io: &MidiIo, idle_for: Duration, max_total: Duration) {
    let deadline = Instant::now() + max_total;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now()).min(idle_for);
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
pub fn try_read_preset(io: &mut MidiIo, bank: u8, number: u8, total: Duration) -> Option<Preset> {
    for attempt in 0..2 {
        io.drain();
        let select_msg =
            crate::encode::encode_select_preset(number).expect("select preset encodes");
        let _ = io.send_sysex(&select_msg);
        let deadline =
            Instant::now() + if attempt == 0 { total } else { total + Duration::from_millis(500) };
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let Some(reply) = io.receive_sysex(remaining.min(Duration::from_millis(500))) else {
                continue;
            };
            if reply.len() < 16 {
                continue;
            }
            if reply[HeaderPos::FunctionId1 as usize] == InboundClass::Data as u8
                && reply[HeaderPos::FunctionId2 as usize] == 0
            {
                if let Ok(p) = decode_preset(&reply, bank, number) {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Try to find a controller UUID in any subsequent CONTROLLER_INFO message
/// from the captured response stream.
pub fn extract_uuid(messages: &[Vec<u8>]) -> Option<String> {
    for msg in messages {
        if msg.len() < 16 {
            continue;
        }
        if msg[HeaderPos::FunctionId1 as usize] != InboundClass::ControllerInfo as u8 {
            continue;
        }
        let Ok(segs) = iter_segments(msg) else { continue };
        for (id, data) in segs {
            if id == segment_id::UUID_OR_FIRMWARE {
                if let Ok(uuid) = decode_uuid_nibbles(&data) {
                    return Some(uuid);
                }
            }
        }
    }
    None
}

/// Print a short summary of what the device just sent.
pub fn announce_inbound(out: &mut Out, header_p1: u8, header_p2: u8, header_p3: u8, msg_len: usize) {
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
        out.detail(&format!("  received controller info ({msg_len} bytes)."), None);
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
