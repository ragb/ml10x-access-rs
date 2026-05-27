use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use serde_yml::from_str as yaml_from_str;

use crate::commands::{Context, helpers};
use crate::device::{HeaderPos, InboundClass};
use crate::encode::{encode_controller, encode_navigate_to, encode_preset};
use crate::exit_codes;
use crate::handshake;
use crate::midi_io::MidiIo;
use crate::presets::Preset;
use crate::sysex::parse_header_with;
use crate::validate::{
    Severity, filter_blocking, validate_preset_schema, validate_preset_semantics,
};
use crate::yaml_io::{load_controller_yaml, load_preset_yaml};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Preset YAML file, controller.yaml, or a directory of them.
    pub target: PathBuf,
    /// Show what would be sent without opening the device.
    #[arg(long = "dry-run")]
    pub dry_run: bool,
    /// Stop on the first failure in a batch sync.
    #[arg(long)]
    pub strict: bool,
}

pub fn run(ctx: &mut Context, args: Args) -> i32 {
    let port = ctx.resolved_port();

    // Discover what we're syncing.
    let mut controller_file: Option<PathBuf> = None;
    let preset_files: Vec<PathBuf>;
    if args.target.is_dir() {
        let cand = args.target.join("controller.yaml");
        if cand.is_file() {
            controller_file = Some(cand);
        }
        let mut files = crate::commands::lint::walk_preset_files(&args.target);
        files.sort();
        if files.is_empty() && controller_file.is_none() {
            ctx.out.error(
                &format!(
                    "No controller.yaml or preset-*.yaml files found under {}.",
                    args.target.display()
                ),
                None,
            );
            return exit_codes::INPUT_FILE_ERROR;
        }
        let mut parts: Vec<String> = Vec::new();
        if controller_file.is_some() {
            parts.push("controller.yaml".to_string());
        }
        if !files.is_empty() {
            parts.push(format!("{} preset YAML(s)", files.len()));
        }
        ctx.out.info(
            &format!(
                "Found {} under {}.",
                parts.join(" + "),
                args.target.display()
            ),
            None,
        );
        preset_files = files;
    } else if args.target.file_name().and_then(|n| n.to_str()) == Some("controller.yaml") {
        controller_file = Some(args.target.clone());
        preset_files = Vec::new();
    } else {
        preset_files = vec![args.target.clone()];
    }

    // Validate + load every preset before touching the device.
    let mut loaded: Vec<(PathBuf, Preset)> = Vec::new();
    let mut had_error = false;
    for f in &preset_files {
        let text = match std::fs::read_to_string(f) {
            Ok(t) => t,
            Err(e) => {
                ctx.out
                    .error(&format!("{}: cannot read: {e}", f.display()), None);
                had_error = true;
                if args.strict {
                    return exit_codes::INPUT_FILE_ERROR;
                }
                continue;
            }
        };
        let raw: Value = match yaml_from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                ctx.out
                    .error(&format!("{}: not valid YAML: {e}", f.display()), None);
                had_error = true;
                if args.strict {
                    return exit_codes::INPUT_FILE_ERROR;
                }
                continue;
            }
        };
        let source_str = f.display().to_string();
        let issues = validate_preset_schema(&raw, Some(&source_str));
        if !filter_blocking(&issues).is_empty() {
            for i in &issues {
                ctx.out.error(&format!("{}: {}", i.path, i.message), None);
            }
            had_error = true;
            if args.strict {
                return exit_codes::INPUT_FILE_ERROR;
            }
            continue;
        }
        let preset = match load_preset_yaml(f) {
            Ok(p) => p,
            Err(e) => {
                ctx.out.error(&format!("{}", e), None);
                had_error = true;
                if args.strict {
                    return exit_codes::INPUT_FILE_ERROR;
                }
                continue;
            }
        };
        let sem = validate_preset_semantics(&preset);
        if !filter_blocking(&sem).is_empty() {
            for i in &sem {
                if i.severity == Severity::Error {
                    ctx.out
                        .error(&format!("{}: {}: {}", f.display(), i.path, i.message), None);
                }
            }
            had_error = true;
            if args.strict {
                return exit_codes::INPUT_FILE_ERROR;
            }
            continue;
        }
        for i in &sem {
            if i.severity != Severity::Error {
                ctx.out
                    .warn(&format!("{}: {}: {}", f.display(), i.path, i.message), None);
            }
        }
        loaded.push((f.clone(), preset));
    }

    // Pre-encode the controller if it's part of the target set.
    let controller_encoded: Option<(PathBuf, Vec<u8>)> = if let Some(p) = &controller_file {
        match load_controller_yaml(p).and_then(|c| {
            encode_controller(&c).map_err(|e| crate::yaml_io::YamlError::Io {
                path: p.display().to_string(),
                source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
            })
        }) {
            Ok(bytes) => Some((p.clone(), bytes)),
            Err(e) => {
                ctx.out
                    .error(&format!("Could not encode {}: {e}", p.display()), None);
                had_error = true;
                if args.strict {
                    return exit_codes::ENCODE_ERROR;
                }
                None
            }
        }
    } else {
        None
    };

    if had_error && loaded.is_empty() && controller_encoded.is_none() {
        ctx.out.error("Nothing syncable after validation.", None);
        return exit_codes::INPUT_FILE_ERROR;
    }
    if loaded.is_empty() && controller_encoded.is_none() {
        ctx.out.error("Nothing to sync.", None);
        return exit_codes::INPUT_FILE_ERROR;
    }

    // Pre-encode every preset.
    let mut encoded: Vec<(PathBuf, Preset, Vec<u8>, Vec<u8>, Vec<u8>)> = Vec::new();
    for (f, preset) in loaded {
        let (bank_msg, preset_msg) = match encode_navigate_to(preset.bank, preset.number) {
            Ok(t) => t,
            Err(e) => {
                ctx.out
                    .error(&format!("Could not encode {}: {e}", f.display()), None);
                if args.strict {
                    return exit_codes::ENCODE_ERROR;
                }
                continue;
            }
        };
        let save_msg = match encode_preset(&preset, true) {
            Ok(m) => m,
            Err(e) => {
                ctx.out
                    .error(&format!("Could not encode {}: {e}", f.display()), None);
                if args.strict {
                    return exit_codes::ENCODE_ERROR;
                }
                continue;
            }
        };
        encoded.push((f, preset, bank_msg, preset_msg, save_msg));
    }

    if args.dry_run {
        ctx.out.info("Dry run -- no MIDI ports opened.", None);
        let mut total_bytes = 0usize;
        if let Some((f, msg)) = &controller_encoded {
            total_bytes += msg.len();
            ctx.out.info(
                &format!(
                    "  {}: controller settings ({} bytes)",
                    f.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                    msg.len()
                ),
                None,
            );
        }
        for (f, preset, bank_msg, preset_msg, save_msg) in &encoded {
            let n = bank_msg.len() + preset_msg.len() + save_msg.len();
            total_bytes += n;
            ctx.out.info(
                &format!(
                    "  {}: bank {} preset {} ({n} bytes total, save {} b)",
                    f.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                    preset.bank + 1,
                    preset.number,
                    save_msg.len()
                ),
                None,
            );
        }
        let item_count = encoded.len() + if controller_encoded.is_some() { 1 } else { 0 };
        ctx.out.emit_result(
            &json!({"ok": true, "dry_run": true, "count": item_count, "total_bytes": total_bytes}),
            Some(&format!(
                "Dry run: {item_count} item(s), {total_bytes} bytes total."
            )),
        );
        return exit_codes::OK;
    }

    // Real sync: open device once for the whole batch.
    let mut io = match MidiIo::open(&port) {
        Ok(io) => io,
        Err(e) => {
            ctx.out
                .error(&format!("Could not open the device: {e}"), None);
            return exit_codes::DEVICE_UNAVAILABLE;
        }
    };

    let mut results: Vec<Value> = Vec::new();
    let mut failures: u32 = 0;

    ctx.out.info("Sending 4-message connect handshake.", None);
    if let Err(e) = handshake::connect(&mut io) {
        ctx.out.error(&format!("Handshake failed: {e}"), None);
        return exit_codes::DEVICE_UNAVAILABLE;
    }
    helpers::drain_settle(&io, Duration::from_millis(1500), Duration::from_secs(6));

    // Sync controller first.
    if let Some((f, msg)) = &controller_encoded {
        io.drain();
        if let Err(e) = io.send_sysex(msg) {
            ctx.out.error(&format!("send_sysex failed: {e}"), None);
            return exit_codes::SYNC_NO_ACK;
        }
        // No dedicated controller-saved opcode in v1.2 firmware; treat
        // brief silence or any reply as success.
        sleep(Duration::from_millis(500));
        while io.receive_sysex(Duration::from_millis(400)).is_some() {}
        ctx.out.info(
            &format!(
                "  sent {} ({} bytes)",
                f.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                msg.len()
            ),
            None,
        );
        results.push(json!({
            "path": f.display().to_string(),
            "kind": "controller",
            "ok": true,
            "bytes": msg.len(),
        }));
    }

    let total = encoded.len();
    let mut last_bank: Option<u8> = None;
    for (idx, (f, preset, bank_msg, preset_msg, save_msg)) in encoded.iter().enumerate() {
        let idx = idx + 1;
        let need_bank_change = last_bank != Some(preset.bank);
        if need_bank_change {
            io.drain();
            let _ = io.send_sysex(bank_msg);
            helpers::drain_settle(&io, Duration::from_millis(1200), Duration::from_secs(6));
            last_bank = Some(preset.bank);
        }

        let _ = io.send_sysex(preset_msg);
        helpers::drain_settle(
            &io,
            Duration::from_secs(1),
            if need_bank_change {
                Duration::from_secs(4)
            } else {
                Duration::from_millis(1500)
            },
        );

        io.drain();
        let _ = io.send_sysex(save_msg);

        let mut acked = false;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let Some(reply) = io.receive_sysex(Duration::from_secs(1)) else {
                continue;
            };
            let Ok(h) = parse_header_with(crate::device::ML10X, &reply, true) else {
                continue;
            };
            if h.p1 == InboundClass::EditorEvent as u8 && h.p2 == 2 {
                acked = true;
                break;
            }
        }

        let label = format!(
            "[{idx}/{total}] bank {} preset {} {:?}",
            preset.bank + 1,
            preset.number,
            preset.name
        );
        if acked {
            ctx.out.info(&format!("  ok  {label}"), None);
            results.push(json!({
                "path": f.display().to_string(),
                "bank": preset.bank + 1,
                "number": preset.number,
                "name": preset.name,
                "ok": true,
            }));
        } else {
            failures += 1;
            ctx.out
                .warn(&format!("  fail {label} -- no acknowledgement"), None);
            results.push(json!({
                "path": f.display().to_string(),
                "bank": preset.bank + 1,
                "number": preset.number,
                "name": preset.name,
                "ok": false,
                "reason": "no_ack",
            }));
            if args.strict {
                break;
            }
        }
    }

    let total_items = total + if controller_encoded.is_some() { 1 } else { 0 };
    let synced = results.len() as u32 - failures;
    let suffix = if failures > 0 {
        format!(" ({failures} failure(s))")
    } else {
        String::new()
    };
    let summary = format!("{synced}/{total_items} item(s) synced{suffix}.");
    ctx.out.emit_result(
        &json!({
            "ok": failures == 0,
            "synced": synced,
            "failed": failures,
            "total": total_items,
            "results": results,
        }),
        Some(&summary),
    );
    if failures > 0 {
        exit_codes::SYNC_NO_ACK
    } else {
        exit_codes::OK
    }
}

#[allow(dead_code)]
fn _silence_unused_path<P: AsRef<Path>>(p: P) -> std::path::PathBuf {
    p.as_ref().to_path_buf()
}

#[allow(dead_code)]
fn _silence_unused_header_pos() -> u8 {
    HeaderPos::FunctionId2 as u8
}
