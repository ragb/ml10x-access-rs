//! ML10X codec: SysEx framing, preset/controller encode/decode, validation.
//!
//! Pure (no I/O, no threads, no MIDI backend). Embeds into native CLIs,
//! into the WASM editor build, and into anything else that wants to talk
//! the ML10X wire protocol without owning the transport.

pub mod decode;
pub mod device;
pub mod diff;
pub mod encode;
pub mod handshake;
pub mod inbound;
pub mod presets;
pub mod sysex;
pub mod validate;
pub mod yaml;
