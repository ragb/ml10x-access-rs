//! Compare two `Preset`s field by field.
//!
//! Used by the CLI's `diff` command to compare a local YAML against the
//! device's copy, and exposed to the WASM editor so the browser can show
//! the same kind of comparison without re-implementing it in JS.
//!
//! Returns a `Vec<Difference>`: an empty vec means the two presets are
//! field-identical. `bank` and `number` are deliberately *not* compared —
//! the caller picks which slot to compare against, so a mismatch there is
//! a caller bug, not a preset diff.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::presets::{Preset, PresetBody, PresetMode};

#[cfg(feature = "tsify")]
use tsify_next::Tsify;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "tsify", derive(Tsify))]
#[cfg_attr(feature = "tsify", tsify(into_wasm_abi, from_wasm_abi))]
pub struct Difference {
    pub field: String,
    /// On the TS side this is `unknown` — `local` is a string for scalar
    /// fields, an array for `chain` / `connections`.
    #[cfg_attr(feature = "tsify", tsify(type = "unknown"))]
    pub local: Value,
    #[cfg_attr(feature = "tsify", tsify(type = "unknown"))]
    pub device: Value,
}

pub fn diff_presets(local: &Preset, device: &Preset) -> Vec<Difference> {
    let mut diffs: Vec<Difference> = Vec::new();

    if local.name != device.name {
        diffs.push(Difference {
            field: "name".into(),
            local: json!(local.name),
            device: json!(device.name),
        });
    }

    let mode_label = |m: PresetMode| match m {
        PresetMode::Simple => "simple",
        PresetMode::Advanced => "advanced",
    };
    if local.mode() != device.mode() {
        diffs.push(Difference {
            field: "mode".into(),
            local: json!(mode_label(local.mode())),
            device: json!(mode_label(device.mode())),
        });
    }

    if local.spillover.output_tip != device.spillover.output_tip {
        diffs.push(Difference {
            field: "spillover.output_tip".into(),
            local: json!(local.spillover.output_tip.slug()),
            device: json!(device.spillover.output_tip.slug()),
        });
    }
    if local.spillover.output_ring != device.spillover.output_ring {
        diffs.push(Difference {
            field: "spillover.output_ring".into(),
            local: json!(local.spillover.output_ring.slug()),
            device: json!(device.spillover.output_ring.slug()),
        });
    }

    let local_hops = hop_tuples(&local.body);
    let device_hops = hop_tuples(&device.body);
    let mut sorted_local = local_hops.clone();
    let mut sorted_device = device_hops.clone();
    sorted_local.sort();
    sorted_device.sort();
    if sorted_local != sorted_device {
        let (field, render): (&str, fn(&(String, String, bool)) -> Value) = match &local.body {
            PresetBody::Simple { .. } => ("chain", |h| json!([h.0, h.1, h.2])),
            PresetBody::Advanced { .. } => ("connections", |h| json!([h.0, h.1])),
        };
        diffs.push(Difference {
            field: field.into(),
            local: Value::Array(local_hops.iter().map(render).collect()),
            device: Value::Array(device_hops.iter().map(render).collect()),
        });
    }

    diffs
}

fn hop_tuples(body: &PresetBody) -> Vec<(String, String, bool)> {
    match body {
        PresetBody::Simple { chain } => chain
            .iter()
            .map(|h| {
                (
                    h.from_connector.slug().to_string(),
                    h.to_connector.slug().to_string(),
                    h.bypass,
                )
            })
            .collect(),
        PresetBody::Advanced { connections } => connections
            .iter()
            .map(|c| {
                (
                    c.from_connector.slug().to_string(),
                    c.to_connector.slug().to_string(),
                    false,
                )
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::{
        Connection, ConnectorSlug, PresetBody, SimpleHop, Spillover, SpilloverTarget,
    };

    fn base() -> Preset {
        Preset {
            bank: 0,
            number: 0,
            name: "Same".into(),
            spillover: Spillover::default(),
            body: PresetBody::Simple {
                chain: vec![SimpleHop {
                    from_connector: ConnectorSlug::InputTip,
                    to_connector: ConnectorSlug::OutputTip,
                    bypass: false,
                }],
            },
        }
    }

    #[test]
    fn identical_presets_have_no_differences() {
        assert!(diff_presets(&base(), &base()).is_empty());
    }

    #[test]
    fn name_difference_surfaces() {
        let mut a = base();
        a.name = "Other".into();
        let diffs = diff_presets(&a, &base());
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].field, "name");
    }

    #[test]
    fn spillover_differences_surface_per_side() {
        let mut a = base();
        a.spillover.output_tip = SpilloverTarget::ATip;
        let diffs = diff_presets(&a, &base());
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].field, "spillover.output_tip");
    }

    #[test]
    fn mode_change_surfaces() {
        let mut a = base();
        a.body = PresetBody::Advanced {
            connections: vec![Connection {
                from_connector: ConnectorSlug::InputTip,
                to_connector: ConnectorSlug::OutputTip,
            }],
        };
        let diffs = diff_presets(&a, &base());
        let modes: Vec<&str> = diffs.iter().map(|d| d.field.as_str()).collect();
        assert!(modes.contains(&"mode"), "got: {modes:?}");
    }

    #[test]
    fn chain_reorder_is_not_a_difference() {
        let mut reordered = base();
        if let PresetBody::Simple { chain } = &mut reordered.body {
            chain.push(SimpleHop {
                from_connector: ConnectorSlug::InputRing,
                to_connector: ConnectorSlug::OutputRing,
                bypass: false,
            });
        }
        let mut original = base();
        if let PresetBody::Simple { chain } = &mut original.body {
            chain.insert(
                0,
                SimpleHop {
                    from_connector: ConnectorSlug::InputRing,
                    to_connector: ConnectorSlug::OutputRing,
                    bypass: false,
                },
            );
        }
        assert!(diff_presets(&reordered, &original).is_empty());
    }
}
