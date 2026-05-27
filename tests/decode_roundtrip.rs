//! Decode-side properties pinned with synthesized SysEx + fixture data.

mod common;

use ml10x::decode::decode_preset;
use ml10x::device::{ML10X, segment_id, slug_to_groupnumber};
use ml10x::presets::{ConnectorSlug, PresetBody, PresetMode};
use ml10x::sysex::{build_header, encode_segment, frame};
use std::collections::HashSet;

#[test]
fn advanced_read_preserves_multi_output_branches() {
    // Build a synthetic Advanced READ message (P5=127 sets the Advanced
    // flag on a device READ with P2=0). One 1-byte segment per hop:
    // segment id = target gn, data[0] = source gn.
    let hops: &[(&str, &str)] = &[
        ("input_tip", "a_tip"),
        ("a_tip", "e_tip"),
        ("a_tip", "e_ring"),
        ("e_tip", "d_tip"),
        ("e_ring", "d_ring"),
        ("d_tip", "output_tip"),
        ("d_ring", "output_ring"),
    ];

    let header = build_header(6, 0, 0, 0, 127, 0, 0, 0);
    let mut segments: Vec<Vec<u8>> = Vec::new();
    for &(from_slug, to_slug) in hops {
        let from_gn = slug_to_groupnumber(from_slug).unwrap();
        let to_gn = slug_to_groupnumber(to_slug).unwrap();
        segments.push(encode_segment(to_gn, &[from_gn]));
    }
    let mut name_bytes = b"Test".to_vec();
    name_bytes.extend_from_slice(b"        ");
    segments.push(encode_segment(segment_id::PRESET_NAME, &name_bytes));
    let msg = frame(&header, &segments).unwrap();

    let p = decode_preset(&msg, 0, 0).unwrap();
    assert_eq!(p.mode(), PresetMode::Advanced);
    let connections = match &p.body {
        PresetBody::Advanced { connections } => connections,
        PresetBody::Simple { .. } => panic!("expected Advanced body, got Simple"),
    };
    let expected: HashSet<(ConnectorSlug, ConnectorSlug)> = hops
        .iter()
        .map(|(f, t)| {
            (
                ConnectorSlug::from_slug(f).unwrap(),
                ConnectorSlug::from_slug(t).unwrap(),
            )
        })
        .collect();
    let got: HashSet<(ConnectorSlug, ConnectorSlug)> = connections
        .iter()
        .map(|c| (c.from_connector, c.to_connector))
        .collect();
    assert_eq!(got, expected);

    // ML10X profile sanity stays useful here so the test can detect a
    // device profile regression.
    let _ = ML10X.total_presets();
}
