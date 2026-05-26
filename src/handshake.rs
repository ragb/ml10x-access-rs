//! ML10X connect handshake.
//!
//! The official editor's connect sequence is just four outbound SysEx
//! messages, after which the device streams the bank dump unsolicited.
//! We replay the same four messages.

use crate::device::{DeviceProfile, ML10X};
use crate::midi_io::{MidiError, MidiIo};
use crate::sysex::{SysexError, build_header_with, frame_with};

/// The four P2 codes the editor sends right after a port is opened.
/// Meaning is inferred from the captured handshake — they are not in the
/// named function-code enum.
pub const HANDSHAKE_P2_SEQUENCE: [u8; 4] = [0, 24, 19, 21];

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
    io.drain();
    for &p2 in &HANDSHAKE_P2_SEQUENCE {
        let header = build_header_with(device, 0, p2, 0, 0, 0, 0, 0, 0);
        let message = frame_with(device, &header, &[])?;
        io.send_sysex(&message)?;
    }
    Ok(())
}

pub fn connect(io: &mut MidiIo) -> Result<(), HandshakeError> {
    connect_with(io, ML10X)
}
