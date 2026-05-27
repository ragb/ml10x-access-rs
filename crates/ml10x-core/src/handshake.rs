//! ML10X connect-handshake — byte builders only.
//!
//! The official editor's connect sequence is four outbound SysEx frames
//! with P2 = 0, 24, 19, 21. After receiving them the device streams the
//! active preset + controller dump unprompted. There's no inbound
//! validation step on connect.
//!
//! Transport (sending, receiving, draining) lives wherever the caller
//! owns the MIDI port — `midir` on the CLI side, the Web MIDI API in the
//! browser. This module only builds the bytes.

use crate::device::{DeviceProfile, ML10X};
use crate::sysex::{SysexError, build_header_with, frame_with};

/// The four P2 codes the editor sends right after a port is opened.
pub const HANDSHAKE_P2_SEQUENCE: [u8; 4] = [0, 24, 19, 21];

/// Build the four connect-handshake frames for a given device profile.
/// Callers send them in order; the device replies unprompted.
pub fn handshake_messages_with(device: DeviceProfile) -> Result<[Vec<u8>; 4], SysexError> {
    let make = |p2: u8| -> Result<Vec<u8>, SysexError> {
        let header = build_header_with(device, 0, p2, 0, 0, 0, 0, 0, 0);
        frame_with(device, &header, &[])
    };
    Ok([
        make(HANDSHAKE_P2_SEQUENCE[0])?,
        make(HANDSHAKE_P2_SEQUENCE[1])?,
        make(HANDSHAKE_P2_SEQUENCE[2])?,
        make(HANDSHAKE_P2_SEQUENCE[3])?,
    ])
}

pub fn handshake_messages() -> Result<[Vec<u8>; 4], SysexError> {
    handshake_messages_with(ML10X)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysex::parse_header;

    #[test]
    fn handshake_emits_four_frames() {
        let frames = handshake_messages().unwrap();
        assert_eq!(frames.len(), 4);
        for (i, frame) in frames.iter().enumerate() {
            let h = parse_header(frame).unwrap();
            assert_eq!(h.p2, HANDSHAKE_P2_SEQUENCE[i], "frame {i}");
        }
    }
}
