//! ML10X codec: SysEx framing, preset/controller encode/decode, validation.
//!
//! Pure (no I/O, no threads, no MIDI backend). Embeds into native CLIs,
//! into the WASM editor build, and into anything else that wants to talk
//! the ML10X wire protocol without owning the transport.

// tsify 0.5.8 deprecates `#[tsify(into_wasm_abi, from_wasm_abi)]` in favour
// of `tsify::Ts<T>`, which would change the wasm ABI the web editor relies
// on. The warning can only be silenced at module scope, so allow it on the
// modules that derive `Tsify`.
pub mod decode;
pub mod device;
#[cfg_attr(feature = "tsify", allow(deprecated))]
pub mod diff;
pub mod encode;
pub mod handshake;
#[cfg_attr(feature = "tsify", allow(deprecated))]
pub mod inbound;
#[cfg_attr(feature = "tsify", allow(deprecated))]
pub mod presets;
#[cfg(feature = "schema")]
pub mod schema;
#[cfg_attr(feature = "tsify", allow(deprecated))]
pub mod sysex;
#[cfg_attr(feature = "tsify", allow(deprecated))]
pub mod validate;
pub mod yaml;
