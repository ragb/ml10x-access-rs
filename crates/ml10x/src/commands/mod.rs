//! Subcommand implementations + shared context.

use std::path::PathBuf;

use crate::config::Config;
use crate::output::Out;

pub mod diff;
pub mod dump;
pub mod helpers;
pub mod lint;
pub mod list_cmd;
pub mod ports;
pub mod select;
pub mod show;
pub mod sync;

pub struct Context {
    pub out: Out,
    pub config: Config,
    pub port_override: Option<String>,
}

impl Context {
    pub fn resolved_port(&self) -> String {
        self.port_override
            .clone()
            .unwrap_or_else(|| self.config.port.clone())
    }
}

#[derive(Debug, Clone)]
pub struct GlobalArgs {
    pub verbose: bool,
    pub quiet: bool,
    pub json_mode: bool,
    pub config_path: Option<PathBuf>,
    pub port: Option<String>,
}
