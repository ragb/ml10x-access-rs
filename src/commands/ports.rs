use serde_json::json;

use crate::commands::Context;
use crate::exit_codes;
use crate::midi_io;

pub fn run(ctx: &mut Context) -> i32 {
    let (inputs, outputs) = match midi_io::list_ports() {
        Ok(p) => p,
        Err(e) => {
            ctx.out.error(&format!("Could not enumerate MIDI ports: {e}"), None);
            return exit_codes::DEVICE_UNAVAILABLE;
        }
    };

    fn is_ml10x(name: &str) -> bool {
        let n = name.to_lowercase();
        n.contains("ml10x") || n.contains("morningstar")
    }

    if inputs.is_empty() && outputs.is_empty() {
        ctx.out.warn(
            "No MIDI ports found. Connect the ML10X over USB and try again.",
            None,
        );
        ctx.out.emit_result(
            &json!({
                "inputs": [], "outputs": [], "ml10x_present": false,
            }),
            Some("No MIDI ports found."),
        );
        return exit_codes::DEVICE_UNAVAILABLE;
    }

    ctx.out.info(&format!("MIDI input ports ({}):", inputs.len()), None);
    for p in &inputs {
        let marker = if is_ml10x(&p.name) { "   <-- ML10X" } else { "" };
        ctx.out.info(&format!("  {}{marker}", p.name), None);
    }
    ctx.out.info("", None);
    ctx.out.info(&format!("MIDI output ports ({}):", outputs.len()), None);
    for p in &outputs {
        let marker = if is_ml10x(&p.name) { "   <-- ML10X" } else { "" };
        ctx.out.info(&format!("  {}{marker}", p.name), None);
    }

    let ml10x_in: Vec<&String> = inputs.iter().filter(|p| is_ml10x(&p.name)).map(|p| &p.name).collect();
    let ml10x_out: Vec<&String> = outputs.iter().filter(|p| is_ml10x(&p.name)).map(|p| &p.name).collect();

    ctx.out.info("", None);
    if let (Some(i), Some(o)) = (ml10x_in.first(), ml10x_out.first()) {
        ctx.out.info(&format!("Found ML10X: input \"{i}\" + output \"{o}\"."), None);
    }

    ctx.out.emit_result(
        &json!({
            "inputs": inputs.iter().map(|p| &p.name).collect::<Vec<_>>(),
            "outputs": outputs.iter().map(|p| &p.name).collect::<Vec<_>>(),
            "ml10x_input": ml10x_in.first().map(|s| s.as_str()),
            "ml10x_output": ml10x_out.first().map(|s| s.as_str()),
            "ml10x_present": !ml10x_in.is_empty() && !ml10x_out.is_empty(),
        }),
        None,
    );
    exit_codes::OK
}
