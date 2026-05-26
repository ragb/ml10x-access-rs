//! No-hardware CLI smoke tests using `cargo run --bin ml10x`.
//!
//! These exercise the paths that don't need the device: --help, lint,
//! show, sync --dry-run. Hardware-touching commands (ports/dump/sync/
//! diff/select/list --device) need a real ML10X and live in a manual
//! smoke checklist instead.

use std::path::PathBuf;
use std::process::Command;

fn bin_path() -> PathBuf {
    let mut p = std::env::current_exe().unwrap();
    p.pop(); // strip test binary name
    if p.ends_with("deps") {
        p.pop();
    }
    p.join(if cfg!(windows) { "ml10x.exe" } else { "ml10x" })
}

const SAMPLE: &str = "tests/fixtures/sample-preset.yaml";

fn write_sample() {
    let body = r#"# yaml-language-server: $schema=https://example/preset.schema.json
preset:
  bank: 1
  number: 0
  name: Base
  mode: simple
  spillover:
    output_tip: nothing
    output_ring: nothing
  chain:
    - { from_connector: input_tip, to_connector: a_tip, bypass: false }
    - { from_connector: a_tip,     to_connector: output_tip, bypass: false }
"#;
    let p = PathBuf::from(SAMPLE);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

#[test]
fn help_lists_all_subcommands() {
    let out = Command::new(bin_path())
        .arg("--help")
        .output()
        .expect("ml10x --help runs");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    for sub in &["ports", "dump", "sync", "diff", "show", "select", "list", "lint"] {
        assert!(text.contains(sub), "help missing subcommand {sub:?}: {text}");
    }
}

#[test]
fn show_emits_canonical_yaml() {
    write_sample();
    let out = Command::new(bin_path())
        .args(["show", SAMPLE])
        .output()
        .expect("ml10x show runs");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("preset:"), "got: {text}");
    assert!(text.contains("bank: 1"));
    assert!(text.contains("from_connector: input_tip"));
}

#[test]
fn lint_reports_clean_file() {
    write_sample();
    let out = Command::new(bin_path())
        .args(["lint", SAMPLE])
        .output()
        .expect("ml10x lint runs");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("0 error(s)"), "expected zero errors in: {text}");
}

#[test]
fn sync_dry_run_shows_byte_count() {
    write_sample();
    let out = Command::new(bin_path())
        .args(["sync", "--dry-run", SAMPLE])
        .output()
        .expect("ml10x sync --dry-run runs");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Dry run"), "expected 'Dry run' header in: {text}");
    assert!(text.contains("bytes"), "expected byte count in: {text}");
}

#[test]
fn show_rejects_bad_yaml() {
    let p = "tests/fixtures/bad-preset.yaml";
    std::fs::write(p, "preset:\n  bank: 5\n  number: 0\n  name: x\n").unwrap();
    let out = Command::new(bin_path()).args(["show", p]).output().unwrap();
    assert!(!out.status.success(), "expected non-zero exit for bad bank");
}
