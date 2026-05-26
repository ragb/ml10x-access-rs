use std::path::PathBuf;

use serde_json::{Value, json};

use crate::commands::Context;
use crate::exit_codes;
use crate::validate::{
    Issue, Severity, filter_blocking, validate_preset_schema, validate_preset_semantics,
};
use crate::yaml_io::load_preset_yaml;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// A preset YAML file or a directory of them.
    pub target: PathBuf,
    /// Exit non-zero if any file has a warning, not just errors.
    #[arg(long)]
    pub strict: bool,
}

pub fn run(ctx: &mut Context, args: Args) -> i32 {
    let files: Vec<PathBuf> = if args.target.is_dir() {
        let mut v: Vec<PathBuf> = walk_preset_files(&args.target);
        v.sort();
        if v.is_empty() {
            ctx.out.error(
                &format!("No preset-*.yaml files found under {}.", args.target.display()),
                None,
            );
            return exit_codes::INPUT_FILE_ERROR;
        }
        v
    } else {
        vec![args.target.clone()]
    };

    let mut summary: Vec<Value> = Vec::new();
    let mut total_errors: usize = 0;
    let mut total_warnings: usize = 0;

    for f in &files {
        let text = match std::fs::read_to_string(f) {
            Ok(t) => t,
            Err(e) => {
                let msg = format!("cannot read: {e}");
                ctx.out.info(&format!("  fail {} -- {msg}", f.display()), None);
                total_errors += 1;
                summary.push(json!({
                    "path": f.display().to_string(),
                    "errors": [{"path": "(file)", "message": msg}],
                    "warnings": [],
                }));
                continue;
            }
        };

        let mut issues: Vec<Issue> = match serde_yml::from_str::<Value>(&text) {
            Ok(raw) => validate_preset_schema(&raw, Some(&f.display().to_string())),
            Err(e) => vec![Issue {
                severity: Severity::Error,
                path: "(yaml)".to_string(),
                message: format!("not valid YAML: {e}"),
            }],
        };

        if filter_blocking(&issues).is_empty() {
            match load_preset_yaml(f) {
                Ok(p) => issues.extend(validate_preset_semantics(&p)),
                Err(e) => issues.push(Issue {
                    severity: Severity::Error,
                    path: "preset".into(),
                    message: e.to_string(),
                }),
            }
        }

        let errs: Vec<&Issue> = issues.iter().filter(|i| i.severity == Severity::Error).collect();
        let warns: Vec<&Issue> = issues.iter().filter(|i| i.severity == Severity::Warning).collect();
        total_errors += errs.len();
        total_warnings += warns.len();

        if issues.is_empty() {
            ctx.out.detail(&format!("  ok    {}", f.display()), None);
        } else {
            let tag = if !errs.is_empty() { "fail " } else { "warn " };
            ctx.out.info(
                &format!(
                    "  {tag}{} -- {} error(s), {} warning(s)",
                    f.display(),
                    errs.len(),
                    warns.len()
                ),
                None,
            );
            for i in &issues {
                ctx.out.info(
                    &format!("    [{}] {}: {}", i.severity.as_str(), i.path, i.message),
                    None,
                );
            }
        }

        summary.push(json!({
            "path": f.display().to_string(),
            "errors": errs.iter().map(|i| json!({"path": i.path, "message": i.message})).collect::<Vec<_>>(),
            "warnings": warns.iter().map(|i| json!({"path": i.path, "message": i.message})).collect::<Vec<_>>(),
        }));
    }

    let blocking = total_errors > 0 || (args.strict && total_warnings > 0);
    ctx.out.emit_result(
        &json!({
            "ok": !blocking,
            "files": files.len(),
            "total_errors": total_errors,
            "total_warnings": total_warnings,
            "results": summary,
        }),
        Some(&format!(
            "{} file(s) checked: {} error(s), {} warning(s).",
            files.len(),
            total_errors,
            total_warnings
        )),
    );

    if blocking {
        exit_codes::INPUT_FILE_ERROR
    } else {
        exit_codes::OK
    }
}

pub fn walk_preset_files(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_inner(root, &mut out);
    out
}

fn walk_inner(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for ent in entries.flatten() {
        let p = ent.path();
        if p.is_dir() {
            walk_inner(&p, out);
        } else if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("preset-") && name.ends_with(".yaml") {
                out.push(p);
            }
        }
    }
}
