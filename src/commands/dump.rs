use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::commands::{Context, helpers};
use crate::decode::{decode_controller, decode_preset};
use crate::device::{HeaderPos, InboundClass, segment_id};
use crate::encode::{encode_select_bank, encode_select_preset};
use crate::exit_codes;
use crate::handshake;
use crate::midi_io::MidiIo;
use crate::sysex::{decode_uuid_nibbles, iter_segments, parse_header_with};
use crate::yaml_io::{dump_controller_yaml, dump_preset_yaml};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory to write YAML output (and a raw JSON log).
    #[arg(long = "out")]
    pub out_dir: PathBuf,
    /// Seconds to wait after the handshake for inbound data (default 10).
    #[arg(long, default_value_t = 10.0)]
    pub timeout: f64,
    /// Walk every bank/preset. Takes ~2 minutes.
    #[arg(long = "all")]
    pub all_presets: bool,
}

pub fn run(ctx: &mut Context, args: Args) -> i32 {
    let port = ctx.resolved_port();

    if let Err(e) = std::fs::create_dir_all(&args.out_dir) {
        ctx.out.error(&format!("Could not create {}: {e}", args.out_dir.display()), None);
        return exit_codes::INPUT_FILE_ERROR;
    }
    let raw_file = args.out_dir.join("dump-raw.json");

    ctx.out.info(&format!("Opening MIDI ports matching {port:?}."), None);
    let mut io = match MidiIo::open(&port) {
        Ok(io) => io,
        Err(e) => {
            ctx.out.error(&format!("Could not open the device: {e}"), None);
            return exit_codes::DEVICE_UNAVAILABLE;
        }
    };

    let started = Instant::now();
    let mut received: Vec<Vec<u8>> = Vec::new();
    let mut received_with_t: Vec<Value> = Vec::new();

    ctx.out.info("Sending 4-message connect handshake.", None);
    if let Err(e) = handshake::connect(&mut io) {
        ctx.out.error(&format!("Handshake failed: {e}"), None);
        return exit_codes::DEVICE_UNAVAILABLE;
    }
    ctx.out.info(
        &format!(
            "Waiting up to {:.0} seconds for inbound data (stops early after 2 idle seconds).",
            args.timeout
        ),
        None,
    );
    let deadline = started + Duration::from_secs_f64(args.timeout);
    let idle_timeout = Duration::from_secs(2);
    let mut last = Instant::now();
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now()).min(idle_timeout);
        if remaining.is_zero() {
            break;
        }
        let Some(msg) = io.receive_sysex(remaining) else {
            if !received.is_empty() && Instant::now() - last >= idle_timeout {
                break;
            }
            continue;
        };
        received_with_t.push(json!({
            "t": started.elapsed().as_secs_f64(),
            "bytes": msg.clone(),
        }));
        last = Instant::now();
        let _ = parse_header_with(crate::device::ML10X, &msg, true)
            .map(|h| {
                helpers::announce_inbound(&mut ctx.out, h.p1, h.p2, h.p3, msg.len());
            })
            .map_err(|e| {
                ctx.out.warn(&format!("  malformed SysEx kept in raw capture: {e}"), None);
            });
        received.push(msg);
    }

    let raw_doc = json!({
        "port": port,
        "duration_s": started.elapsed().as_secs_f64(),
        "count": received.len(),
        "messages": received_with_t,
    });
    let _ = std::fs::write(&raw_file, serde_json::to_string_pretty(&raw_doc).unwrap_or_default());
    ctx.out.detail(
        &format!("Saved {} raw inbound messages to {}.", received.len(), raw_file.display()),
        None,
    );

    let mut presets_written: u32 = 0;
    let mut controller_written = false;
    let mut written_paths: Vec<String> = Vec::new();

    for msg in &received {
        if msg.len() < 16 {
            continue;
        }
        let p1 = msg[HeaderPos::FunctionId1 as usize];
        let p2 = msg[HeaderPos::FunctionId2 as usize];
        if p1 == InboundClass::Data as u8 && p2 == 0 {
            let bank = msg[HeaderPos::FunctionId4 as usize];
            let number = msg[HeaderPos::FunctionId3 as usize];
            match decode_preset(msg, bank, number) {
                Ok(p) => {
                    let path = args.out_dir.join(format!("bank-{}", bank + 1)).join(format!("preset-{:03}.yaml", number));
                    if let Err(e) = dump_preset_yaml(&p, &path) {
                        ctx.out.warn(&format!("  could not write {}: {e}", path.display()), None);
                        continue;
                    }
                    presets_written += 1;
                    let rel = path.strip_prefix(&args.out_dir).unwrap_or(&path).display().to_string();
                    written_paths.push(rel.clone());
                    ctx.out.detail(&format!("  wrote {} ({:?}).", rel, p.name), None);
                }
                Err(e) => {
                    ctx.out.warn(&format!("  skipping malformed preset data: {e}"), None);
                }
            }
        } else if p1 == InboundClass::Data as u8 && p2 == 1 {
            match decode_controller(msg) {
                Ok(mut ctrl) => {
                    if let Some(uuid_msg) = received.iter().find(|m| {
                        m.len() >= 16 && m[HeaderPos::FunctionId1 as usize] == InboundClass::ControllerInfo as u8
                    }) {
                        if let Ok(segs) = iter_segments(uuid_msg) {
                            for (id, data) in segs {
                                if id == segment_id::UUID_OR_FIRMWARE {
                                    if let Ok(u) = decode_uuid_nibbles(&data) {
                                        ctrl.uuid = u;
                                    }
                                }
                            }
                        }
                    }
                    let path = args.out_dir.join("controller.yaml");
                    if let Err(e) = dump_controller_yaml(&ctrl, &path) {
                        ctx.out.warn(&format!("  could not write {}: {e}", path.display()), None);
                        continue;
                    }
                    controller_written = true;
                    ctx.out.detail(&format!("  wrote {}.", path.strip_prefix(&args.out_dir).unwrap_or(&path).display()), None);
                }
                Err(e) => {
                    ctx.out.warn(&format!("  skipping malformed controller data: {e}"), None);
                }
            }
        }
    }

    if presets_written > 0 || controller_written {
        let part = if controller_written {
            "controller written"
        } else {
            "no controller"
        };
        ctx.out.info(&format!("Initial dump: {presets_written} preset(s), {part}."), None);
    }

    let mut skipped: Vec<(u8, u8)> = Vec::new();

    if args.all_presets {
        ctx.out.info("Walking all 4 banks x 128 presets. Takes about 2 minutes.", None);
        // Reopen for a clean session.
        drop(io);
        let mut io = match MidiIo::open(&port) {
            Ok(io) => io,
            Err(e) => {
                ctx.out.error(&format!("Could not reopen the device for full walk: {e}"), None);
                return exit_codes::DEVICE_UNAVAILABLE;
            }
        };
        if let Err(e) = handshake::connect(&mut io) {
            ctx.out.error(&format!("Handshake failed: {e}"), None);
            return exit_codes::DEVICE_UNAVAILABLE;
        }
        helpers::drain_settle(&io, Duration::from_millis(1500), Duration::from_secs(6));

        for bank in 0u8..4 {
            io.drain();
            if let Ok(msg) = encode_select_bank(bank) {
                let _ = io.send_sysex(&msg);
            }
            helpers::drain_settle(&io, Duration::from_secs(1), Duration::from_secs(6));

            for preset_num in 0u8..128 {
                let mut got = false;
                for attempt in 0..2 {
                    io.drain();
                    if let Ok(msg) = encode_select_preset(preset_num) {
                        let _ = io.send_sysex(&msg);
                    }
                    let deadline = Instant::now()
                        + if attempt == 0 {
                            Duration::from_secs(2)
                        } else {
                            Duration::from_millis(2500)
                        };
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
                            if let Ok(p) = decode_preset(&reply, bank, preset_num) {
                                let path = args.out_dir.join(format!("bank-{}", bank + 1)).join(format!("preset-{:03}.yaml", preset_num));
                                if dump_preset_yaml(&p, &path).is_ok() {
                                    presets_written += 1;
                                    let rel = path.strip_prefix(&args.out_dir).unwrap_or(&path).display().to_string();
                                    written_paths.push(rel);
                                    got = true;
                                    break;
                                }
                            }
                        }
                    }
                    if got {
                        break;
                    }
                }
                if !got {
                    skipped.push((bank + 1, preset_num));
                }
            }
            ctx.out.info(
                &format!("Bank {} done: {presets_written} written so far.", bank + 1),
                None,
            );
        }
    }

    let skipped_clause = if skipped.is_empty() {
        String::new()
    } else {
        format!(" ({} skipped)", skipped.len())
    };
    let summary = format!(
        "Wrote {presets_written} preset(s) + {} under {}.{}",
        if controller_written { "1 controller" } else { "no controller" },
        args.out_dir.display(),
        skipped_clause
    );
    if !skipped.is_empty() && !ctx.out.json_mode {
        for (bank, preset_num) in skipped.iter().take(20) {
            ctx.out.warn(
                &format!("  skipped: bank {bank} preset {preset_num} (no response after retries)"),
                None,
            );
        }
    }
    ctx.out.emit_result(
        &json!({
            "ok": true,
            "out_dir": args.out_dir.display().to_string(),
            "presets_written": presets_written,
            "controller_written": controller_written,
            "paths": written_paths,
            "skipped": skipped.iter().map(|(b, p)| json!({"bank": b, "preset": p})).collect::<Vec<_>>(),
        }),
        Some(&summary),
    );
    exit_codes::OK
}
