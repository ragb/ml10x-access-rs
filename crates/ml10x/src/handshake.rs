//! ML10X connect handshake — transport layer.
//!
//! Byte builders live in `ml10x_core::handshake`. This module wires them
//! up to the `midir`-backed `MidiIo` we use for the CLI.

use log::{debug, info};

use crate::midi_io::{MidiError, MidiIo};
use ml10x_core::device::{DeviceProfile, ML10X};
use ml10x_core::handshake::handshake_messages_with;
use ml10x_core::sysex::SysexError;

pub use ml10x_core::handshake::HANDSHAKE_P2_SEQUENCE;

#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    #[error(transparent)]
    Sysex(#[from] SysexError),
    #[error(transparent)]
    Midi(#[from] MidiError),
}

/// Send the four-message handshake. Doesn't wait for or validate responses
/// — the caller is expected to consume the inbound stream afterward.
pub fn connect_with(io: &mut MidiIo, device: DeviceProfile) -> Result<(), HandshakeError> {
    info!(
        "sending {}-message connect handshake (P2 = {:?})",
        HANDSHAKE_P2_SEQUENCE.len(),
        HANDSHAKE_P2_SEQUENCE
    );
    io.drain();
    for (i, frame) in handshake_messages_with(device)?.into_iter().enumerate() {
        debug!("handshake frame {}/{}", i + 1, HANDSHAKE_P2_SEQUENCE.len());
        io.send_sysex(&frame)?;
    }
    Ok(())
}

pub fn connect(io: &mut MidiIo) -> Result<(), HandshakeError> {
    connect_with(io, ML10X)
}
