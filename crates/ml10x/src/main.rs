use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use log::LevelFilter;

use ml10x::commands::{self, Context, GlobalArgs};
use ml10x::config::Config;
use ml10x::exit_codes;
use ml10x::output::{Out, Verbosity};

#[derive(Parser, Debug)]
#[command(
    name = "ml10x",
    version,
    about = "Edit Morningstar ML10X presets from the command line",
    long_about = "Accessible CLI + YAML workflow for the Morningstar ML10X loop switcher."
)]
struct Cli {
    /// Show extra detail in human output.
    #[arg(long, short = 'v', global = true)]
    verbose: bool,
    /// Suppress everything but errors.
    #[arg(long, short = 'q', global = true)]
    quiet: bool,
    /// Emit machine-readable JSON instead of human text.
    #[arg(long = "json", global = true)]
    json_mode: bool,
    /// Path to a TOML config file. Defaults to $ML10X_CONFIG or ~/.ml10x.toml.
    #[arg(long = "config", global = true, value_name = "PATH")]
    config_path: Option<PathBuf>,
    /// Substring of the MIDI port name. Defaults to the value in your config
    /// file, then "ML10X".
    #[arg(long = "port", global = true, value_name = "SUBSTRING")]
    port: Option<String>,
    /// Override the diagnostic log level (`error`, `warn`, `info`, `debug`,
    /// `trace`). Defaults to the level implied by --verbose/--quiet, or to
    /// `RUST_LOG` if set.
    #[arg(long = "log", global = true, value_name = "LEVEL")]
    log_level: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// List MIDI input and output ports, flagging the ML10X.
    Ports,
    /// Read presets from the device and write them as YAML files.
    Dump(commands::dump::Args),
    /// Write a YAML preset (or a directory of them) to the device.
    Sync(commands::sync::Args),
    /// Show what would change if you synced this YAML to the device.
    Diff(commands::diff::Args),
    /// Validate and print a normalised copy of a preset YAML.
    Show(commands::show::Args),
    /// Activate a preset on the device (like pressing a footswitch).
    Select(commands::select::Args),
    /// List presets, either on the device or from a directory of YAML files.
    List(commands::list_cmd::Args),
    /// Validate one or many preset YAMLs without touching the device.
    Lint(commands::lint::Args),
}

/// Initialize the diagnostic logger. Precedence: explicit `--log`,
/// then `RUST_LOG`, then a default derived from verbosity.
fn init_logger(verbosity: Verbosity, log_override: Option<&str>) {
    let default = match verbosity {
        Verbosity::Quiet => LevelFilter::Error,
        Verbosity::Normal => LevelFilter::Warn,
        Verbosity::Verbose => LevelFilter::Info,
    };
    let mut builder = env_logger::Builder::new();
    if let Some(s) = log_override {
        builder.parse_filters(s);
    } else if let Ok(env) = std::env::var("RUST_LOG") {
        builder.parse_filters(&env);
    } else {
        builder.filter_level(default);
    }
    // Compact format suitable for a CLI: `[LEVEL target] message`.
    builder
        .format(|buf, record| {
            writeln!(
                buf,
                "[{:5} {}] {}",
                record.level(),
                record.target(),
                record.args()
            )
        })
        .init();
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.verbose && cli.quiet {
        eprintln!("--verbose and --quiet are mutually exclusive.");
        return ExitCode::from(exit_codes::USAGE_ERROR as u8);
    }
    let verbosity = if cli.verbose {
        Verbosity::Verbose
    } else if cli.quiet || cli.json_mode {
        Verbosity::Quiet
    } else {
        Verbosity::Normal
    };

    init_logger(verbosity, cli.log_level.as_deref());

    let _global = GlobalArgs {
        verbose: cli.verbose,
        quiet: cli.quiet,
        json_mode: cli.json_mode,
        config_path: cli.config_path.clone(),
        port: cli.port.clone(),
    };

    let mut out = Out::new(verbosity, cli.json_mode);
    let config = match Config::load(cli.config_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            out.error(&format!("{e}"), None);
            return ExitCode::from(exit_codes::INPUT_FILE_ERROR as u8);
        }
    };

    let mut ctx = Context {
        out,
        config,
        port_override: cli.port,
    };

    let code: i32 = match cli.command {
        Commands::Ports => commands::ports::run(&mut ctx),
        Commands::Dump(a) => commands::dump::run(&mut ctx, a),
        Commands::Sync(a) => commands::sync::run(&mut ctx, a),
        Commands::Diff(a) => commands::diff::run(&mut ctx, a),
        Commands::Show(a) => commands::show::run(&mut ctx, a),
        Commands::Select(a) => commands::select::run(&mut ctx, a),
        Commands::List(a) => commands::list_cmd::run(&mut ctx, a),
        Commands::Lint(a) => commands::lint::run(&mut ctx, a),
    };
    ExitCode::from(code as u8)
}
