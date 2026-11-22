//! Emit a JSON Schema for one of the YAML document shapes.
//!
//! The schemas are generated from the `#[derive(JsonSchema)]` annotations
//! on the YAML shim types in `ml10x-core::yaml`. The checked-in copies at
//! `schemas/{preset,controller}.schema.json` are the output of this
//! command; CI re-runs it and `git diff`s to catch drift.

use crate::commands::Context;
use crate::exit_codes;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Which schema to emit: `preset` or `controller`.
    pub kind: String,
}

pub fn run(ctx: &mut Context, args: Args) -> i32 {
    let schema = match args.kind.to_ascii_lowercase().as_str() {
        "preset" => ml10x_core::schema::preset_schema(),
        "controller" => ml10x_core::schema::controller_schema(),
        other => {
            ctx.out.error(
                &format!("unknown schema kind {other:?} (valid: preset, controller)"),
                None,
            );
            return exit_codes::USAGE_ERROR;
        }
    };
    match serde_json::to_string_pretty(&schema) {
        Ok(s) => {
            println!("{s}");
            exit_codes::OK
        }
        Err(e) => {
            ctx.out
                .error(&format!("failed to serialize schema: {e}"), None);
            exit_codes::GENERIC_ERROR
        }
    }
}
