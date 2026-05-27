//! Tests for `encode_controller`.

use ml10x_core::decode::decode_controller;
use ml10x_core::encode::encode_controller;
use ml10x_core::presets::{Connector, Connectors, Controller, IncludeInTrails};
use ml10x_core::sysex::iter_segments;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

#[test]
fn header_matches_editor_format() {
    let ctrl = Controller {
        midi_channel: 9,
        device_id: 6,
        ..Controller::default()
    };
    let msg = encode_controller(&ctrl).unwrap();
    assert_eq!(msg[0], 0xF0);
    assert_eq!(*msg.last().unwrap(), 0xF7);
    assert_eq!(msg[6], 6); // P1
    assert_eq!(msg[7], 1); // P2
    for i in 8..14 {
        assert_eq!(msg[i], 0, "header byte {i} should be 0 (got {})", msg[i]);
    }
}

#[test]
fn roundtrips_settings_and_trails() {
    let ctrl = Controller {
        midi_channel: 9,
        device_id: 6,
        input_split: true,
        loop_bypass_persistent: false,
        include_in_trails: IncludeInTrails {
            a_tip: true,
            c_ring: true,
            d_tip: true,
            d_ring: true,
            e_tip: true,
            ..IncludeInTrails::default()
        },
        ..Controller::default()
    };
    let decoded = decode_controller(&encode_controller(&ctrl).unwrap()).unwrap();
    assert_eq!(decoded.midi_channel, 9);
    assert_eq!(decoded.device_id, 6);
    assert_eq!(decoded.input_split, true);
    assert_eq!(decoded.loop_bypass_persistent, false);
    assert_eq!(decoded.include_in_trails.a_tip, true);
    assert_eq!(decoded.include_in_trails.a_ring, false);
    assert_eq!(decoded.include_in_trails.c_ring, true);
    assert_eq!(decoded.include_in_trails.d_tip, true);
    assert_eq!(decoded.include_in_trails.d_ring, true);
    assert_eq!(decoded.include_in_trails.e_tip, true);
    assert_eq!(decoded.include_in_trails.e_ring, false);
}

#[test]
fn folds_diacritics_to_ascii() {
    let conns = Connectors {
        a_tip: Connector {
            name: "Saída".into(),
            short_name: "Sa".into(),
            input_name: "Entrada".into(),
            output_name: "Saída".into(),
        },
        ..Connectors::default()
    };
    let ctrl = Controller {
        connectors: conns,
        ..Controller::default()
    };
    let decoded = decode_controller(&encode_controller(&ctrl).unwrap()).unwrap();
    assert_eq!(decoded.connectors.a_tip.name, "Saida");
    assert_eq!(decoded.connectors.a_tip.input_name, "Entrada");
    assert_eq!(decoded.connectors.a_tip.output_name, "Saida");
}

#[test]
fn booleans_emitted_as_127() {
    let ctrl = Controller {
        input_split: true,
        loop_bypass_persistent: true,
        ..Controller::default()
    };
    let msg = encode_controller(&ctrl).unwrap();
    let segs: HashMap<u8, Vec<u8>> = iter_segments(&msg).unwrap().into_iter().collect();
    assert_eq!(segs[&35], vec![127u8]); // input_split
    assert_eq!(segs[&36], vec![127u8]); // loop_bypass_persistent
}
