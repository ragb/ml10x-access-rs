use std::path::PathBuf;

use serde_json::{json, Value};

use crate::commands::Context;
use crate::exit_codes;
use crate::output::Out;
use crate::validate::{
    filter_blocking, validate_preset_schema, validate_preset_semantics, Issue, Severity,
};
use crate::yaml_io::{load_preset_yaml, preset_yaml_string};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Path to the preset YAML file.
    pub preset_file: PathBuf,
    /// Treat warnings as errors (exit non-zero).
    #[arg(long)]
    pub strict: bool,
}

pub fn run(ctx: &mut Context, args: Args) -> i32 {
    let text = match std::fs::read_to_string(&args.preset_file) {
        Ok(t) => t,
        Err(e) => {
            ctx.out.error(
                &format!("{}: cannot read: {e}", args.preset_file.display()),
                None,
            );
            return exit_codes::INPUT_FILE_ERROR;
        }
    };

    let raw: Value = match serde_yml::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            ctx.out.error(
                &format!("{}: not valid YAML: {e}", args.preset_file.display()),
                None,
            );
            return exit_codes::INPUT_FILE_ERROR;
        }
    };

    let source_str = args.preset_file.display().to_string();
    let mut all_issues = validate_preset_schema(&raw, Some(&source_str));
    let mut maybe_preset = None;

    if filter_blocking(&all_issues).is_empty() {
        match load_preset_yaml(&args.preset_file) {
            Ok(p) => {
                all_issues.extend(validate_preset_semantics(&p));
                maybe_preset = Some(p);
            }
            Err(e) => {
                all_issues.push(Issue {
                    severity: Severity::Error,
                    path: "preset".into(),
                    message: e.to_string(),
                });
            }
        }
    }

    report_issues(&mut ctx.out, &all_issues);

    let blocking = all_issues.iter().any(|i| {
        i.severity == Severity::Error || (args.strict && i.severity == Severity::Warning)
    });

    if blocking {
        if ctx.out.json_mode {
            ctx.out.emit_result(
                &json!({
                    "ok": false,
                    "issues": all_issues,
                }),
                None,
            );
        }
        return exit_codes::INPUT_FILE_ERROR;
    }

    if ctx.out.json_mode {
        let preset_value = maybe_preset.as_ref().and_then(|p| serde_json::to_value(p).ok());
        ctx.out.emit_result(
            &json!({
                "ok": true,
                "preset": preset_value,
                "issues": all_issues,
            }),
            None,
        );
        return exit_codes::OK;
    }

    if let Some(p) = maybe_preset {
        match preset_yaml_string(&p) {
            Ok(s) => print!("{s}"),
            Err(e) => {
                ctx.out.error(&format!("Could not render canonical YAML: {e}"), None);
                return exit_codes::GENERIC_ERROR;
            }
        }
    }
    exit_codes::OK
}

fn report_issues(out: &mut Out, issues: &[Issue]) {
    if out.json_mode || issues.is_empty() {
        return;
    }
    let errors: Vec<&Issue> = issues.iter().filter(|i| i.severity == Severity::Error).collect();
    let warnings: Vec<&Issue> = issues.iter().filter(|i| i.severity == Severity::Warning).collect();
    let infos: Vec<&Issue> = issues.iter().filter(|i| i.severity == Severity::Info).collect();
    if !errors.is_empty() {
        out.warn(&format!("{} error(s):", errors.len()), None);
        for i in &errors {
            out.warn(&format!("  {}: {}", i.path, i.message), None);
        }
    }
    if !warnings.is_empty() {
        out.warn(&format!("{} warning(s):", warnings.len()), None);
        for i in &warnings {
            out.warn(&format!("  {}: {}", i.path, i.message), None);
        }
    }
    for i in &infos {
        out.warn(&format!("  note {}: {}", i.path, i.message), None);
    }
}
