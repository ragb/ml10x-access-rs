use std::time::{Duration, Instant};

use serde_json::json;

use crate::commands::{Context, helpers};
use crate::decode::decode_preset;
use crate::device::{HeaderPos, InboundClass};
use crate::encode::{encode_select_bank, encode_select_preset};
use crate::exit_codes;
use crate::handshake;
use crate::midi_io::MidiIo;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Bank number (1..4).
    pub bank: u8,
    /// Preset number within the bank (0..127).
    pub preset: u8,
}

pub fn run(ctx: &mut Context, args: Args) -> i32 {
    if !(1..=4).contains(&args.bank) {
        ctx.out.error("BANK must be 1..4.", None);
        return exit_codes::USAGE_ERROR;
    }
    if args.preset > 127 {
        ctx.out.error("PRESET must be 0..127.", None);
        return exit_codes::USAGE_ERROR;
    }
    let port = ctx.resolved_port();
    let bank_msg = match encode_select_bank(args.bank - 1) {
        Ok(m) => m,
        Err(e) => {
            ctx.out.error(&format!("encode_select_bank: {e}"), None);
            return exit_codes::ENCODE_ERROR;
        }
    };
    let preset_msg = match encode_select_preset(args.preset) {
        Ok(m) => m,
        Err(e) => {
            ctx.out.error(&format!("encode_select_preset: {e}"), None);
            return exit_codes::ENCODE_ERROR;
        }
    };

    let mut io = match MidiIo::open(&port) {
        Ok(io) => io,
        Err(e) => {
            ctx.out.error(&format!("Could not open the device: {e}"), None);
            return exit_codes::DEVICE_UNAVAILABLE;
        }
    };

    ctx.out.info(
        &format!("Activating bank {} preset {}.", args.bank, args.preset),
        None,
    );

    if let Err(e) = handshake::connect(&mut io) {
        ctx.out.error(&format!("Handshake failed: {e}"), None);
        return exit_codes::DEVICE_UNAVAILABLE;
    }
    helpers::drain_settle(&io, Duration::from_millis(1500), Duration::from_secs(6));

    io.drain();
    let _ = io.send_sysex(&bank_msg);
    helpers::drain_settle(&io, Duration::from_secs(1), Duration::from_secs(4));
    let _ = io.send_sysex(&preset_msg);

    // Try to read back the new preset name for the summary.
    let mut name: Option<String> = None;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let Some(reply) = io.receive_sysex(Duration::from_millis(500)) else {
            continue;
        };
        if reply.len() < 16 {
            continue;
        }
        if reply[HeaderPos::FunctionId1 as usize] == InboundClass::Data as u8
            && reply[HeaderPos::FunctionId2 as usize] == 0
        {
            if let Ok(p) = decode_preset(&reply, args.bank - 1, args.preset) {
                name = Some(p.name);
                break;
            }
        }
    }

    let summary = match &name {
        Some(n) => format!("Activated bank {} preset {} ({n:?}).", args.bank, args.preset),
        None => format!("Activated bank {} preset {}.", args.bank, args.preset),
    };
    ctx.out.emit_result(
        &json!({"ok": true, "bank": args.bank, "preset": args.preset, "name": name}),
        Some(&summary),
    );
    exit_codes::OK
}
