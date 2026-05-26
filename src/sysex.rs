//! SysEx framing for the ML10X.
//!
//! See `docs/sysex.md` for the full spec and source references in the
//! editor's main.js bundle.
//!
//! A frame is:
//!
//! ```text
//! [F0]                              SysEx start
//! [00 21 24]                        Morningstar manufacturer ID
//! [model_byte]                      0x07 for ML10X
//! [0]                               version
//! [P1 P2 P3 P4 P5 P6 P7 P8]         8 function-code bytes
//! [0 0]                             two reserved bytes (always zero)
//! [data segments ...]               each segment: 7F <id> <len> <bytes>
//! [checksum]                        XOR of bytes 0..N-3 masked to 7 bits
//! [F7]                              SysEx end
//! ```

use thiserror::Error;

use crate::device::{DeviceProfile, HEADER_LENGTH, HeaderPos, ML10X, SEGMENT_LEAD_IN};

pub const SOX: u8 = 0xF0;
pub const EOX: u8 = 0xF7;

#[derive(Debug, Error)]
pub enum SysexError {
    #[error("Frame is too short to have a checksum")]
    TooShortForChecksum,
    #[error("Message too short ({0} bytes)")]
    MessageTooShort(usize),
    #[error("Not a SysEx message (missing F0/F7 delimiters)")]
    MissingDelimiters,
    #[error("Wrong manufacturer ID. Expected {expected:02X?}, got {actual:02X?}")]
    WrongManufacturer { expected: [u8; 3], actual: [u8; 3] },
    #[error("Wrong model byte. Expected 0x{expected:02X}, got 0x{actual:02X}")]
    WrongModelByte { expected: u8, actual: u8 },
    #[error("Checksum mismatch. Expected 0x{expected:02X}, got 0x{actual:02X}")]
    BadChecksum { expected: u8, actual: u8 },
    #[error("Header must be exactly {expected} bytes, got {actual}")]
    BadHeaderLength { expected: usize, actual: usize },
    #[error("Header model byte 0x{header:02X} does not match device profile 0x{device:02X}")]
    HeaderDeviceMismatch { header: u8, device: u8 },
    #[error(
        "Length field mismatch. Header declares {declared} bytes (msb={msb:#04x}, lsb={lsb:#04x}), but the message is {actual} bytes."
    )]
    LengthFieldMismatch { declared: usize, actual: usize, msb: u8, lsb: u8 },
    #[error("Segment at offset {offset} declares length {declared} but only {available} bytes are available")]
    SegmentTruncated { offset: usize, declared: usize, available: usize },
}

/// XOR of every byte except the checksum byte and the trailing F7.
///
/// Faithful port of the editor's `qi` function:
///
/// ```text
/// function qi(l){let d=l[0], i=l.length-2;
///     for(let o=1; o<i; o++) d^=l[o]; return d&=127, d;}
/// ```
///
/// The first byte (F0) is *included* in the XOR. The last two bytes
/// (checksum slot + F7) are excluded.
pub fn checksum(frame_bytes: &[u8]) -> Result<u8, SysexError> {
    if frame_bytes.len() < 3 {
        return Err(SysexError::TooShortForChecksum);
    }
    let mut acc = frame_bytes[0];
    for &b in &frame_bytes[1..frame_bytes.len() - 2] {
        acc ^= b;
    }
    Ok(acc & 0x7F)
}

/// Construct the 16-byte SysEx header, masking each function byte to 7 bits.
pub fn build_header_with(
    device: DeviceProfile,
    p1: u8,
    p2: u8,
    p3: u8,
    p4: u8,
    p5: u8,
    p6: u8,
    p7: u8,
    p8: u8,
) -> [u8; HEADER_LENGTH] {
    let mut h = [0u8; HEADER_LENGTH];
    h[HeaderPos::SysexStart.idx()] = SOX;
    h[HeaderPos::ManfId1.idx()] = device.manufacturer_id[0];
    h[HeaderPos::ManfId2.idx()] = device.manufacturer_id[1];
    h[HeaderPos::ManfId3.idx()] = device.manufacturer_id[2];
    h[HeaderPos::ModelId.idx()] = device.model_byte;
    h[HeaderPos::VersionId1.idx()] = 0;
    h[HeaderPos::FunctionId1.idx()] = p1 & 0x7F;
    h[HeaderPos::FunctionId2.idx()] = p2 & 0x7F;
    h[HeaderPos::FunctionId3.idx()] = p3 & 0x7F;
    h[HeaderPos::FunctionId4.idx()] = p4 & 0x7F;
    h[HeaderPos::FunctionId5.idx()] = p5 & 0x7F;
    h[HeaderPos::FunctionId6.idx()] = p6 & 0x7F;
    h[HeaderPos::FunctionId7.idx()] = p7 & 0x7F;
    h[HeaderPos::FunctionId8.idx()] = p8 & 0x7F;
    h
}

/// Convenience: default device (ML10X), all-zero Ps unless specified via
/// the builder helpers below.
pub fn build_header(p1: u8, p2: u8, p3: u8, p4: u8, p5: u8, p6: u8, p7: u8, p8: u8) -> [u8; HEADER_LENGTH] {
    build_header_with(ML10X, p1, p2, p3, p4, p5, p6, p7, p8)
}

/// A data segment is `7F <id> <len> <data...>` with all payload bytes
/// masked to 7 bits. The editor's `oi.addData` enforces a fixed declared
/// length and stops early if `data` is longer.
pub fn encode_segment(segment_id: u8, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(3 + data.len());
    out.push(SEGMENT_LEAD_IN);
    out.push(segment_id & 0x7F);
    out.push((data.len() as u8) & 0x7F);
    for &b in data {
        out.push(b & 0x7F);
    }
    out
}

/// Wrap an already-built header + body segments into a complete SysEx
/// message including checksum and EOX.
pub fn frame_with(
    device: DeviceProfile,
    header: &[u8],
    segments: &[Vec<u8>],
) -> Result<Vec<u8>, SysexError> {
    if header.len() != HEADER_LENGTH {
        return Err(SysexError::BadHeaderLength {
            expected: HEADER_LENGTH,
            actual: header.len(),
        });
    }
    if header[HeaderPos::ModelId.idx()] != device.model_byte {
        return Err(SysexError::HeaderDeviceMismatch {
            header: header[HeaderPos::ModelId.idx()],
            device: device.model_byte,
        });
    }
    let mut body: Vec<u8> = Vec::with_capacity(header.len() + segments.iter().map(|s| s.len()).sum::<usize>() + 2);
    body.extend_from_slice(header);
    for seg in segments {
        body.extend_from_slice(seg);
    }
    // Checksum slot (placeholder 0) + EOX. Matches oi.endBuild():
    //   addByte(0); addByte(247); arr[arr.length-2] = checksum.
    body.push(0);
    body.push(EOX);
    let sum = checksum(&body)?;
    let n = body.len();
    body[n - 2] = sum;
    Ok(body)
}

pub fn frame(header: &[u8], segments: &[Vec<u8>]) -> Result<Vec<u8>, SysexError> {
    frame_with(ML10X, header, segments)
}

/// Parsed SysEx header fields.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct HeaderInfo {
    pub model_id: u8,
    pub version_id: u8,
    pub p1: u8,
    pub p2: u8,
    pub p3: u8,
    pub p4: u8,
    pub p5: u8,
    pub p6: u8,
    pub p7: u8,
    pub p8: u8,
    pub declared_length: usize,
    pub is_last_chunk: bool,
}

/// Validate a SysEx frame's framing and checksum and return its header fields.
pub fn parse_header_with(
    device: DeviceProfile,
    message: &[u8],
    validate_length: bool,
) -> Result<HeaderInfo, SysexError> {
    if message.len() < HEADER_LENGTH + 2 {
        return Err(SysexError::MessageTooShort(message.len()));
    }
    if message[0] != SOX || *message.last().unwrap() != EOX {
        return Err(SysexError::MissingDelimiters);
    }
    let actual_manuf = [message[1], message[2], message[3]];
    if actual_manuf != device.manufacturer_id {
        return Err(SysexError::WrongManufacturer {
            expected: device.manufacturer_id,
            actual: actual_manuf,
        });
    }
    if message[HeaderPos::ModelId.idx()] != device.model_byte {
        return Err(SysexError::WrongModelByte {
            expected: device.model_byte,
            actual: message[HeaderPos::ModelId.idx()],
        });
    }
    let expected_sum = checksum(message)?;
    let actual_sum = message[message.len() - 2];
    if actual_sum != expected_sum {
        return Err(SysexError::BadChecksum {
            expected: expected_sum,
            actual: actual_sum,
        });
    }
    let msb = message[HeaderPos::LengthMsb.idx()];
    let lsb = message[HeaderPos::LengthLsb.idx()];
    let declared = ((msb as usize & 0x3F) << 7) | (lsb as usize & 0x7F);
    let is_last_chunk = msb & 0x40 != 0;
    if validate_length && declared != message.len() {
        return Err(SysexError::LengthFieldMismatch {
            declared,
            actual: message.len(),
            msb,
            lsb,
        });
    }
    Ok(HeaderInfo {
        model_id: message[HeaderPos::ModelId.idx()],
        version_id: message[HeaderPos::VersionId1.idx()],
        p1: message[HeaderPos::FunctionId1.idx()],
        p2: message[HeaderPos::FunctionId2.idx()],
        p3: message[HeaderPos::FunctionId3.idx()],
        p4: message[HeaderPos::FunctionId4.idx()],
        p5: message[HeaderPos::FunctionId5.idx()],
        p6: message[HeaderPos::FunctionId6.idx()],
        p7: message[HeaderPos::FunctionId7.idx()],
        p8: message[HeaderPos::FunctionId8.idx()],
        declared_length: declared,
        is_last_chunk,
    })
}

pub fn parse_header(message: &[u8]) -> Result<HeaderInfo, SysexError> {
    parse_header_with(ML10X, message, false)
}

/// Yield (segment_id, data) pairs from a validated SysEx message.
pub fn iter_segments(message: &[u8]) -> Result<Vec<(u8, Vec<u8>)>, SysexError> {
    let mut out = Vec::new();
    let mut i = HEADER_LENGTH;
    let end = message.len().saturating_sub(2); // stop before [checksum, F7]
    while i < end {
        if message[i] != SEGMENT_LEAD_IN {
            // Trailing zero padding or junk — stop.
            break;
        }
        if i + 2 >= end {
            break;
        }
        let seg_id = message[i + 1];
        let length = message[i + 2] as usize;
        let data_start = i + 3;
        let data_end = data_start + length;
        if data_end > end {
            return Err(SysexError::SegmentTruncated {
                offset: i,
                declared: length,
                available: end - data_start,
            });
        }
        out.push((seg_id, message[data_start..data_end].to_vec()));
        i = data_end;
    }
    Ok(out)
}

/// Connector and preset names are ASCII padded with spaces. Trim trailing
/// spaces (and any embedded NULs) so the string reads cleanly.
pub fn decode_ascii_name(data: &[u8]) -> String {
    let trimmed: &[u8] = {
        let mut end = data.len();
        while end > 0 && (data[end - 1] == b' ' || data[end - 1] == 0) {
            end -= 1;
        }
        &data[..end]
    };
    // Replace any non-ASCII byte with U+FFFD, matching Python's `errors="replace"`.
    trimmed
        .iter()
        .map(|&b| if b.is_ascii() { b as char } else { '\u{FFFD}' })
        .collect()
}

/// The controller UUID arrives as 32 bytes, each carrying one hex nibble
/// in its low 4 bits. Formatted as 8-4-4-4-12 separated by dashes.
pub fn decode_uuid_nibbles(data: &[u8]) -> Result<String, SysexError> {
    if data.len() != 32 {
        return Err(SysexError::MessageTooShort(data.len()));
    }
    let hex: String = data.iter().map(|&b| format!("{:x}", b & 0x0F)).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{HeaderPos, MODEL_BYTE};

    #[test]
    fn checksum_matches_editor_definition() {
        // XOR over [F0, 00, 01, 02, 03] = 0xF0 ^ 0 ^ 1 ^ 2 ^ 3 = 0xF0
        // masked to 7 bits = 0x70.
        let msg = [0xF0u8, 0x00, 0x01, 0x02, 0x03, 0x00, 0xF7];
        assert_eq!(checksum(&msg).unwrap(), 0x70);
    }

    #[test]
    fn build_header_layout() {
        let h = build_header(1, 2, 3, 4, 5, 6, 7, 8);
        assert_eq!(h.len(), 16);
        assert_eq!(h[HeaderPos::SysexStart.idx()], SOX);
        assert_eq!(h[HeaderPos::ManfId1.idx()], 0x00);
        assert_eq!(h[HeaderPos::ManfId2.idx()], 0x21);
        assert_eq!(h[HeaderPos::ManfId3.idx()], 0x24);
        assert_eq!(h[HeaderPos::ModelId.idx()], MODEL_BYTE);
        assert_eq!(h[HeaderPos::VersionId1.idx()], 0);
        assert_eq!(h[HeaderPos::FunctionId1.idx()], 1);
        assert_eq!(h[HeaderPos::FunctionId8.idx()], 8);
        assert_eq!(h[14], 0);
        assert_eq!(h[15], 0);
    }

    #[test]
    fn function_bytes_masked_to_7_bits() {
        let h = build_header(0xFF, 0x80, 0, 0, 0, 0, 0, 0);
        assert_eq!(h[HeaderPos::FunctionId1.idx()], 0x7F);
        assert_eq!(h[HeaderPos::FunctionId2.idx()], 0x00);
    }

    #[test]
    fn frame_round_trip_no_segments() {
        let h = build_header(0, 29, 0, 0, 0, 0, 0, 0);
        let msg = frame(&h, &[]).unwrap();
        assert_eq!(msg[0], SOX);
        assert_eq!(*msg.last().unwrap(), EOX);
        let parsed = parse_header(&msg).unwrap();
        assert_eq!(parsed.p2, 29);
        assert_eq!(parsed.model_id, MODEL_BYTE);
    }

    #[test]
    fn frame_round_trip_with_segments() {
        let h = build_header(0, 29, 2, 0, 0, 0, 0, 0);
        let segs = vec![encode_segment(1, &[0x10, 0x20, 0x30]), encode_segment(2, b"hi")];
        let msg = frame(&h, &segs).unwrap();
        let parsed = parse_header(&msg).unwrap();
        assert_eq!(parsed.p2, 29);
        let extracted = iter_segments(&msg).unwrap();
        assert_eq!(extracted, vec![(1u8, vec![0x10, 0x20, 0x30]), (2, b"hi".to_vec())]);
    }

    #[test]
    fn parse_rejects_wrong_manufacturer() {
        let mut bad = frame(&build_header(0, 0, 0, 0, 0, 0, 0, 0), &[]).unwrap();
        bad[1] = 0x11;
        match parse_header(&bad) {
            Err(SysexError::WrongManufacturer { .. }) => {}
            other => panic!("expected WrongManufacturer, got {other:?}"),
        }
    }

    #[test]
    fn parse_rejects_wrong_model_byte() {
        let mut bad = frame(&build_header(0, 0, 0, 0, 0, 0, 0, 0), &[]).unwrap();
        bad[HeaderPos::ModelId.idx()] = 0x04; // MC8, not ML10X
        match parse_header(&bad) {
            Err(SysexError::WrongModelByte { .. }) => {}
            other => panic!("expected WrongModelByte, got {other:?}"),
        }
    }

    #[test]
    fn parse_rejects_bad_checksum() {
        let mut bad = frame(&build_header(0, 0, 0, 0, 0, 0, 0, 0), &[]).unwrap();
        let n = bad.len();
        bad[n - 2] ^= 0x01;
        match parse_header(&bad) {
            Err(SysexError::BadChecksum { .. }) => {}
            other => panic!("expected BadChecksum, got {other:?}"),
        }
    }

    #[test]
    fn segment_payload_masked_to_7_bits() {
        let seg = encode_segment(1, &[0xFF, 0x80, 0x42]);
        assert_eq!(seg, vec![0x7F, 0x01, 0x03, 0x7F, 0x00, 0x42]);
    }

    #[test]
    fn frame_validates_header_length() {
        let result = frame(&[0xF0], &[]);
        match result {
            Err(SysexError::BadHeaderLength { expected: 16, .. }) => {}
            other => panic!("expected BadHeaderLength, got {other:?}"),
        }
    }

    #[test]
    fn iter_segments_stops_at_trailing_padding() {
        let msg = frame(&build_header(0, 0, 0, 0, 0, 0, 0, 0), &[]).unwrap();
        assert_eq!(iter_segments(&msg).unwrap(), vec![]);
    }

    #[test]
    fn decode_ascii_strips_padding() {
        assert_eq!(decode_ascii_name(b"hi      "), "hi");
        assert_eq!(decode_ascii_name(b"hi\0\0\0"), "hi");
    }

    #[test]
    fn decode_uuid_nibbles_formats_correctly() {
        let data: Vec<u8> = (0..32u8).map(|i| i & 0x0F).collect();
        let uuid = decode_uuid_nibbles(&data).unwrap();
        assert_eq!(uuid, "01234567-89ab-cdef-0123-456789abcdef");
    }
}
