//! CLI library: re-exports `ml10x_core` plus the transport / file-I/O /
//! command modules that aren't usable in a WASM build.
//!
//! Re-exporting core means the existing `use crate::sysex::…` style inside
//! `commands/*.rs` keeps working without per-file rewrites.

pub use ml10x_core::{decode, device, encode, presets, sysex, validate};

pub mod commands;
pub mod config;
pub mod exit_codes;
pub mod handshake;
pub mod midi_io;
pub mod output;
pub mod yaml_io;
