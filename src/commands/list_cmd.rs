use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::commands::{Context, helpers};
use crate::encode::encode_select_bank;
use crate::exit_codes;
use crate::handshake;
use crate::midi_io::MidiIo;
use crate::yaml_io::load_preset_yaml;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory to walk (instead of querying the device).
    pub dir_arg: Option<PathBuf>,
    /// Read names from the device. Default if no DIR is given.
    #[arg(long = "device")]
    pub from_device: bool,
    /// Limit to one bank (1..4).
    #[arg(long = "bank")]
    pub only_bank: Option<u8>,
    /// Include slots whose name is empty or "Empty".
    #[arg(long = "empty")]
    pub include_empty: bool,
}

pub fn run(ctx: &mut Context, args: Args) -> i32 {
    let from_device = args.from_device || args.dir_arg.is_none();
    if from_device && args.dir_arg.is_some() {
        ctx.out.error("Pass either a DIR or --device, not both.", None);
        return exit_codes::USAGE_ERROR;
    }
    if from_device {
        run_device(ctx, args.only_bank, args.include_empty)
    } else {
        run_directory(ctx, args.dir_arg.unwrap(), args.only_bank, args.include_empty)
    }
}

fn run_directory(ctx: &mut Context, dir: PathBuf, only_bank: Option<u8>, include_empty: bool) -> i32 {
    let mut rows: Vec<Value> = Vec::new();
    let mut files = crate::commands::lint::walk_preset_files(&dir);
    files.sort();
    for f in files {
        let preset = match load_preset_yaml(&f) {
            Ok(p) => p,
            Err(e) => {
                ctx.out.warn(&format!("  could not load {}: {e}", f.display()), None);
                continue;
            }
        };
        let yaml_bank = preset.bank + 1;
        if let Some(b) = only_bank {
            if yaml_bank != b {
                continue;
            }
        }
        let trimmed = preset.name.trim();
        if !include_empty && (trimmed.is_empty() || trimmed.eq_ignore_ascii_case("empty")) {
            continue;
        }
        rows.push(json!({
            "path": f.strip_prefix(&dir).map(|p| p.display().to_string()).unwrap_or_else(|_| f.display().to_string()),
            "bank": yaml_bank,
            "number": preset.number,
            "name": preset.name,
            "mode": match preset.mode {
                crate::presets::PresetMode::Simple => "simple",
                crate::presets::PresetMode::Advanced => "advanced",
            },
        }));
    }
    rows.sort_by_key(|r| (r["bank"].as_u64().unwrap_or(0), r["number"].as_u64().unwrap_or(0)));

    if ctx.out.json_mode {
        ctx.out.emit_result(
            &json!({"ok": true, "source": dir.display().to_string(), "presets": rows}),
            None,
        );
        return exit_codes::OK;
    }

    if rows.is_empty() {
        let suffix = only_bank.map(|b| format!(" (bank {b})")).unwrap_or_default();
        ctx.out.info(&format!("No presets found in {}{}.", dir.display(), suffix), None);
        ctx.out.emit_result(&json!({"ok": true, "presets": []}), None);
        return exit_codes::OK;
    }

    ctx.out.info(&format!("{} preset(s) in {}:", rows.len(), dir.display()), None);
    ctx.out.info(&format!("  {:<5}{:<5}{:<10}NAME", "BANK", "NUM", "MODE"), None);
    for r in &rows {
        let bank = r["bank"].as_u64().unwrap_or(0);
        let num = r["number"].as_u64().unwrap_or(0);
        let mode = r["mode"].as_str().unwrap_or("");
        let name = r["name"].as_str().unwrap_or("");
        ctx.out.info(&format!("  {bank:<5}{num:<5}{mode:<10}{name}"), None);
    }
    ctx.out.emit_result(&json!({"ok": true, "presets": rows}), None);
    exit_codes::OK
}

fn run_device(ctx: &mut Context, only_bank: Option<u8>, include_empty: bool) -> i32 {
    let port = ctx.resolved_port();
    let mut io = match MidiIo::open(&port) {
        Ok(io) => io,
        Err(e) => {
            ctx.out.error(&format!("Could not open the device: {e}"), None);
            return exit_codes::DEVICE_UNAVAILABLE;
        }
    };

    let banks_to_query: Vec<u8> = if let Some(b) = only_bank {
        if !(1..=4).contains(&b) {
            ctx.out.error("--bank must be 1..4.", None);
            return exit_codes::USAGE_ERROR;
        }
        vec![b - 1]
    } else {
        (0..4).collect()
    };

    let mut all_names: Vec<(u8, u8, String)> = Vec::new();

    let estimated = 30 * banks_to_query.len();
    ctx.out.info(
        &format!("Walking {port} (this takes about {estimated} seconds on the current firmware)."),
        None,
    );

    if let Err(e) = handshake::connect(&mut io) {
        ctx.out.error(&format!("Handshake failed: {e}"), None);
        return exit_codes::DEVICE_UNAVAILABLE;
    }
    // Drain initial connect stream.
    let until = Instant::now() + Duration::from_secs(6);
    while Instant::now() < until {
        if io.receive_sysex(Duration::from_millis(1500)).is_none() {
            break;
        }
    }

    for &bank in &banks_to_query {
        io.drain();
        if let Ok(msg) = encode_select_bank(bank) {
            let _ = io.send_sysex(&msg);
        }
        helpers::drain_settle(&io, Duration::from_secs(1), Duration::from_secs(6));

        for preset_num in 0u8..128 {
            if let Some(p) = helpers::try_read_preset(&mut io, bank, preset_num, Duration::from_millis(1500)) {
                all_names.push((bank, preset_num, p.name));
            }
        }
        ctx.out.detail(&format!("Bank {}: {} preset(s) read.", bank + 1, all_names.iter().filter(|(b, _, _)| *b == bank).count()), None);
    }

    let mut rows: Vec<Value> = Vec::new();
    for (bank, number, name) in &all_names {
        let trimmed = name.trim();
        if !include_empty && (trimmed.is_empty() || trimmed.eq_ignore_ascii_case("empty")) {
            continue;
        }
        rows.push(json!({
            "bank": bank + 1,
            "number": number,
            "name": name,
        }));
    }
    rows.sort_by_key(|r| (r["bank"].as_u64().unwrap_or(0), r["number"].as_u64().unwrap_or(0)));

    if ctx.out.json_mode {
        ctx.out.emit_result(&json!({"ok": true, "source": "device", "presets": rows}), None);
        return exit_codes::OK;
    }

    if rows.is_empty() {
        let suffix = only_bank.map(|b| format!(" in bank {b}")).unwrap_or_default();
        ctx.out.info(
            &format!("No named presets on the device{suffix} (use --empty to include 'Empty' slots)."),
            None,
        );
        ctx.out.emit_result(&json!({"ok": true, "presets": []}), None);
        return exit_codes::OK;
    }

    ctx.out.info(&format!("{} named preset(s) on the device:", rows.len()), None);
    ctx.out.info(&format!("  {:<5}{:<5}NAME", "BANK", "NUM"), None);
    for r in &rows {
        let bank = r["bank"].as_u64().unwrap_or(0);
        let num = r["number"].as_u64().unwrap_or(0);
        let name = r["name"].as_str().unwrap_or("");
        ctx.out.info(&format!("  {bank:<5}{num:<5}{name}"), None);
    }
    ctx.out.emit_result(&json!({"ok": true, "presets": rows}), None);
    exit_codes::OK
}
