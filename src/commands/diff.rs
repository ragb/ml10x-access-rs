use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use serde_yml::from_str as yaml_from_str;

use crate::commands::{Context, helpers};
use crate::encode::encode_select_bank;
use crate::exit_codes;
use crate::handshake;
use crate::midi_io::MidiIo;
use crate::presets::Preset;
use crate::validate::{
    Severity, filter_blocking, validate_preset_schema, validate_preset_semantics,
};
use crate::yaml_io::load_preset_yaml;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Path to the preset YAML to compare against the device.
    pub preset_file: PathBuf,
}

pub fn run(ctx: &mut Context, args: Args) -> i32 {
    let text = match std::fs::read_to_string(&args.preset_file) {
        Ok(t) => t,
        Err(e) => {
            ctx.out.error(&format!("{}: cannot read: {e}", args.preset_file.display()), None);
            return exit_codes::INPUT_FILE_ERROR;
        }
    };
    let raw: Value = match yaml_from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            ctx.out.error(&format!("{}: not valid YAML: {e}", args.preset_file.display()), None);
            return exit_codes::INPUT_FILE_ERROR;
        }
    };
    let source_str = args.preset_file.display().to_string();
    let schema_issues = validate_preset_schema(&raw, Some(&source_str));
    if !filter_blocking(&schema_issues).is_empty() {
        for i in &schema_issues {
            ctx.out.error(&format!("{}: {}", i.path, i.message), None);
        }
        return exit_codes::INPUT_FILE_ERROR;
    }
    let local = match load_preset_yaml(&args.preset_file) {
        Ok(p) => p,
        Err(e) => {
            ctx.out.error(&format!("{}: {e}", args.preset_file.display()), None);
            return exit_codes::INPUT_FILE_ERROR;
        }
    };
    let sem_issues = validate_preset_semantics(&local);
    if !filter_blocking(&sem_issues).is_empty() {
        for i in &sem_issues {
            ctx.out.error(&format!("{}: {}", i.path, i.message), None);
        }
        return exit_codes::INPUT_FILE_ERROR;
    }
    for i in &sem_issues {
        if i.severity == Severity::Warning {
            ctx.out.warn(&format!("{}: {}", i.path, i.message), None);
        }
    }

    let port = ctx.resolved_port();
    let mut io = match MidiIo::open(&port) {
        Ok(io) => io,
        Err(e) => {
            ctx.out.error(&format!("Could not open the device: {e}"), None);
            return exit_codes::DEVICE_UNAVAILABLE;
        }
    };

    ctx.out.info("Connecting and fetching the device's copy.", None);
    if let Err(e) = handshake::connect(&mut io) {
        ctx.out.error(&format!("Handshake failed: {e}"), None);
        return exit_codes::DEVICE_UNAVAILABLE;
    }
    helpers::drain_settle(&io, Duration::from_millis(1500), Duration::from_secs(6));

    io.drain();
    if let Ok(msg) = encode_select_bank(local.bank) {
        let _ = io.send_sysex(&msg);
    }
    helpers::drain_settle(&io, Duration::from_secs(1), Duration::from_secs(6));

    let remote = helpers::try_read_preset(&mut io, local.bank, local.number, Duration::from_secs(2));

    let yaml_bank = local.bank + 1;
    let Some(remote) = remote else {
        ctx.out.error(
            &format!(
                "Did not receive preset data for bank {} preset {} from the device.",
                yaml_bank, local.number
            ),
            None,
        );
        return exit_codes::DEVICE_UNAVAILABLE;
    };

    let diffs = diff_presets(&local, &remote);
    if diffs.is_empty() {
        ctx.out.emit_result(
            &json!({
                "ok": true, "differences": [],
                "bank": yaml_bank, "number": local.number,
            }),
            Some(&format!(
                "No differences. Local and device copies of bank {} preset {} are identical.",
                yaml_bank, local.number
            )),
        );
        return exit_codes::OK;
    }

    ctx.out.info(
        &format!(
            "{} difference(s) between {} and device bank {} preset {}:",
            diffs.len(),
            args.preset_file.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
            yaml_bank,
            local.number
        ),
        None,
    );
    for d in &diffs {
        ctx.out.info(
            &format!(
                "  {}: local={}  device={}",
                d["field"].as_str().unwrap_or("?"),
                d["local"],
                d["device"]
            ),
            None,
        );
    }
    ctx.out.emit_result(
        &json!({
            "ok": true, "differences": diffs,
            "bank": yaml_bank, "number": local.number,
        }),
        Some(&format!("{} difference(s).", diffs.len())),
    );
    exit_codes::OK
}

fn diff_presets(local: &Preset, remote: &Preset) -> Vec<Value> {
    use crate::presets::PresetBody;
    let mut diffs: Vec<Value> = Vec::new();
    if local.name != remote.name {
        diffs.push(json!({"field": "name", "local": local.name, "device": remote.name}));
    }
    let mode_label = |m: crate::presets::PresetMode| match m {
        crate::presets::PresetMode::Simple => "simple",
        crate::presets::PresetMode::Advanced => "advanced",
    };
    if local.mode() != remote.mode() {
        diffs.push(json!({
            "field": "mode",
            "local": mode_label(local.mode()),
            "device": mode_label(remote.mode()),
        }));
    }
    if local.spillover.output_tip != remote.spillover.output_tip {
        diffs.push(json!({
            "field": "spillover.output_tip",
            "local": local.spillover.output_tip.slug(),
            "device": remote.spillover.output_tip.slug(),
        }));
    }
    if local.spillover.output_ring != remote.spillover.output_ring {
        diffs.push(json!({
            "field": "spillover.output_ring",
            "local": local.spillover.output_ring.slug(),
            "device": remote.spillover.output_ring.slug(),
        }));
    }
    fn hop_tuples(body: &PresetBody) -> Vec<(String, String, bool)> {
        match body {
            PresetBody::Simple { chain } => chain
                .iter()
                .map(|h| {
                    (
                        h.from_connector.slug().to_string(),
                        h.to_connector.slug().to_string(),
                        h.bypass,
                    )
                })
                .collect(),
            PresetBody::Advanced { connections } => connections
                .iter()
                .map(|c| {
                    (
                        c.from_connector.slug().to_string(),
                        c.to_connector.slug().to_string(),
                        false,
                    )
                })
                .collect(),
        }
    }
    let mut local_hops = hop_tuples(&local.body);
    let mut remote_hops = hop_tuples(&remote.body);
    let mut sorted_local = local_hops.clone();
    let mut sorted_remote = remote_hops.clone();
    sorted_local.sort();
    sorted_remote.sort();
    if sorted_local != sorted_remote {
        let (field, render): (&str, fn(&(String, String, bool)) -> Value) = match &local.body {
            PresetBody::Simple { .. } => (
                "chain",
                |h| json!([h.0, h.1, h.2]),
            ),
            PresetBody::Advanced { .. } => (
                "connections",
                |h| json!([h.0, h.1]),
            ),
        };
        // Suppress the unused-write lints if both are empty.
        let _ = (&mut local_hops, &mut remote_hops);
        diffs.push(json!({
            "field": field,
            "local": local_hops.iter().map(render).collect::<Vec<_>>(),
            "device": remote_hops.iter().map(render).collect::<Vec<_>>(),
        }));
    }
    diffs
}

// Suppress unused import in case Instant unused due to compiler optimization
#[allow(dead_code)]
fn _suppress() -> Instant {
    Instant::now()
}
