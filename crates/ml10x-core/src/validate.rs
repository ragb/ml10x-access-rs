//! Schema + semantic validation for preset and controller documents.
//!
//! Two layers:
//!
//! - **Schema** (`validate_preset_schema` / `validate_controller_schema`): runs
//!   the shipped JSON Schema against the parsed document. Catches typos in
//!   connector slugs, out-of-range bank/preset numbers, bad enum values.
//!
//! - **Semantic** (`validate_preset_semantics`): rules JSON Schema cannot
//!   express, derived from the ML10X manual and the official editor.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use jsonschema::{Draft, JSONSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::presets::{ConnectorSlug, Preset, PresetBody, PresetMode, SpilloverTarget};

#[cfg(feature = "tsify")]
use tsify_next::Tsify;

const PRESET_SCHEMA_SRC: &str = include_str!("../../../schemas/preset.schema.json");
const CONTROLLER_SCHEMA_SRC: &str = include_str!("../../../schemas/controller.schema.json");

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "tsify", derive(Tsify))]
#[cfg_attr(feature = "tsify", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "tsify", derive(Tsify))]
#[cfg_attr(feature = "tsify", tsify(into_wasm_abi, from_wasm_abi))]
pub struct Issue {
    pub severity: Severity,
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}: {}",
            self.severity.as_str(),
            self.path,
            self.message
        )
    }
}

/// Combined result of running schema + semantic validation. Empty `errors`
/// means the preset is safe to encode; `warnings` are advisory.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "tsify", derive(Tsify))]
#[cfg_attr(feature = "tsify", tsify(into_wasm_abi, from_wasm_abi))]
pub struct ValidationReport {
    pub errors: Vec<Issue>,
    pub warnings: Vec<Issue>,
}

/// Run both the JSON-schema check (after re-encoding the preset to the
/// YAML's 1-indexed-bank shape) and the semantic check.
pub fn validate_preset_full(preset: &Preset) -> ValidationReport {
    let raw = match serde_json::to_value(preset) {
        Ok(v) => v,
        Err(_) => return ValidationReport::default(),
    };
    // The shipped JSON schema expects the YAML envelope: `{ preset: { bank: 1..4, ... } }`.
    let mut doc = serde_json::json!({ "preset": raw });
    if let Some(b) = doc["preset"]["bank"].as_u64() {
        doc["preset"]["bank"] = serde_json::json!(b + 1);
    }
    let schema_issues = validate_preset_schema(&doc, None);
    let sem_issues = validate_preset_semantics(preset);

    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for i in schema_issues.into_iter().chain(sem_issues.into_iter()) {
        match i.severity {
            Severity::Error => errors.push(i),
            Severity::Warning | Severity::Info => warnings.push(i),
        }
    }
    ValidationReport { errors, warnings }
}

fn preset_schema() -> &'static JSONSchema {
    static SCHEMA: OnceLock<JSONSchema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let v: Value = serde_json::from_str(PRESET_SCHEMA_SRC).expect("preset.schema.json parses");
        JSONSchema::options()
            .with_draft(Draft::Draft202012)
            .compile(&v)
            .expect("preset.schema.json compiles")
    })
}

fn controller_schema() -> &'static JSONSchema {
    static SCHEMA: OnceLock<JSONSchema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let v: Value =
            serde_json::from_str(CONTROLLER_SCHEMA_SRC).expect("controller.schema.json parses");
        JSONSchema::options()
            .with_draft(Draft::Draft202012)
            .compile(&v)
            .expect("controller.schema.json compiles")
    })
}

fn format_path(path: &jsonschema::paths::JSONPointer) -> String {
    let s = path.to_string();
    if s.is_empty() || s == "/" {
        return "(root)".to_string();
    }
    // Convert "/preset/body/chain/0/from_connector"
    //     → "preset.body.chain[0].from_connector".
    let mut out = String::new();
    for part in s.trim_start_matches('/').split('/') {
        if let Ok(idx) = part.parse::<usize>() {
            out.push('[');
            out.push_str(&idx.to_string());
            out.push(']');
        } else {
            if !out.is_empty() {
                out.push('.');
            }
            out.push_str(part);
        }
    }
    if out.is_empty() {
        "(root)".to_string()
    } else {
        out
    }
}

fn humanise(error: &jsonschema::ValidationError<'_>) -> String {
    use jsonschema::error::ValidationErrorKind as K;
    match &error.kind {
        K::Enum { options } => format!(
            "{} is not one of the allowed values ({}).",
            error.instance, options
        ),
        K::Maximum { limit } => {
            format!("{} is greater than the maximum {}.", error.instance, limit)
        }
        K::Minimum { limit } => format!("{} is less than the minimum {}.", error.instance, limit),
        K::MaxLength { limit } => format!(
            "{} is longer than the maximum length of {} characters.",
            error.instance, limit
        ),
        K::Required { property } => format!("missing required field {}.", property),
        K::AdditionalProperties { unexpected } => {
            format!(
                "Additional properties are not allowed ({} unexpected).",
                unexpected
                    .iter()
                    .map(|s| format!("'{s}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        K::Type { kind } => format!("expected type {:?}, got {}.", kind, error.instance),
        _ => error.to_string(),
    }
}

fn run_schema(schema: &JSONSchema, doc: &Value, source: Option<&str>) -> Vec<Issue> {
    let prefix = source.map(|s| format!("{s}: ")).unwrap_or_default();
    let mut issues: Vec<Issue> = Vec::new();
    if let Err(errors) = schema.validate(doc) {
        for err in errors {
            let path = format_path(&err.instance_path);
            issues.push(Issue {
                severity: Severity::Error,
                path,
                message: format!("{prefix}{}", humanise(&err)),
            });
        }
    }
    issues.sort_by(|a, b| a.path.cmp(&b.path));
    issues
}

pub fn validate_preset_schema(doc: &Value, source: Option<&str>) -> Vec<Issue> {
    run_schema(preset_schema(), doc, source)
}

pub fn validate_controller_schema(doc: &Value, source: Option<&str>) -> Vec<Issue> {
    run_schema(controller_schema(), doc, source)
}

/// Check the rules JSON Schema can't express. Returns issues; the caller
/// decides what to do with warnings.
pub fn validate_preset_semantics(preset: &Preset) -> Vec<Issue> {
    let mut issues: Vec<Issue> = Vec::new();
    let mut from_seen: HashMap<ConnectorSlug, u32> = HashMap::new();
    let mut to_seen: HashMap<ConnectorSlug, u32> = HashMap::new();
    let mut hop_pairs_seen: HashSet<(ConnectorSlug, ConnectorSlug)> = HashSet::new();

    let (hop_list_path, hops): (&str, Vec<(ConnectorSlug, ConnectorSlug)>) = match &preset.body {
        PresetBody::Simple { chain } => (
            "preset.body.chain",
            chain
                .iter()
                .map(|h| (h.from_connector, h.to_connector))
                .collect(),
        ),
        PresetBody::Advanced { connections } => (
            "preset.body.connections",
            connections
                .iter()
                .map(|c| (c.from_connector, c.to_connector))
                .collect(),
        ),
    };

    for (i, &(f, t)) in hops.iter().enumerate() {
        let path = format!("{hop_list_path}[{i}]");

        if f.is_output() {
            issues.push(Issue {
                severity: Severity::Error,
                path: format!("{path}.from_connector"),
                message: format!(
                    "{:?} is an output endpoint and cannot be a chain SOURCE. Signal flows TO outputs, never FROM them.",
                    f.slug()
                ),
            });
        }
        if t.is_input() {
            issues.push(Issue {
                severity: Severity::Error,
                path: format!("{path}.to_connector"),
                message: format!(
                    "{:?} is an input endpoint and cannot be a chain DESTINATION. Signal flows FROM inputs, never TO them.",
                    t.slug()
                ),
            });
        }
        if f == t {
            issues.push(Issue {
                severity: Severity::Error,
                path: path.clone(),
                message: format!(
                    "hop loops back on itself ({:?} -> {:?}); a connector can't be both source and destination of the same hop.",
                    f.slug(),
                    f.slug()
                ),
            });
        }

        *from_seen.entry(f).or_default() += 1;
        *to_seen.entry(t).or_default() += 1;
        let pair = (f, t);
        if !hop_pairs_seen.insert(pair) {
            issues.push(Issue {
                severity: Severity::Warning,
                path: path.clone(),
                message: format!(
                    "duplicate hop {:?} -> {:?}; the second one has no effect.",
                    f.slug(),
                    t.slug()
                ),
            });
        }
    }

    // Spillover in Advanced mode → firmware drops segments 16/17 on save.
    if preset.mode() == PresetMode::Advanced {
        for (side, val) in [
            ("output_tip", preset.spillover.output_tip),
            ("output_ring", preset.spillover.output_ring),
        ] {
            if val != SpilloverTarget::Nothing {
                issues.push(Issue {
                    severity: Severity::Warning,
                    path: format!("preset.spillover.{side}"),
                    message: "Advanced-mode presets do not persist spillover on this firmware. The device drops segments 16/17 on save; set to `nothing` or use Simple mode if you want trails across preset changes.".to_string(),
                });
            }
        }
    }

    // At least one input → output path.
    if !hops.is_empty() && reachable_outputs(&hops).is_empty() {
        issues.push(Issue {
            severity: Severity::Warning,
            path: hop_list_path.to_string(),
            message: "no path from an input (input_tip / input_ring) reaches an output (output_tip / output_ring) — no audio will flow.".to_string(),
        });
    }

    // Simple mode is linear: no branching, no merging. (Advanced bypass
    // is unrepresentable in the model, so no warning is needed for it.)
    if let PresetBody::Simple { .. } = &preset.body {
        let mut from_branches: Vec<(ConnectorSlug, u32)> = from_seen
            .iter()
            .filter(|&(_, &n)| n > 1)
            .map(|(c, n)| (*c, *n))
            .collect();
        from_branches.sort_by_key(|(c, _)| c.slug());
        for (slug, n) in from_branches {
            issues.push(Issue {
                severity: Severity::Error,
                path: hop_list_path.to_string(),
                message: format!(
                    "Simple mode: {:?} appears {} times as a `from_connector`, which would branch the signal. Switch to Advanced (`body.mode: advanced`) to allow branching.",
                    slug.slug(),
                    n
                ),
            });
        }
        let mut to_merges: Vec<(ConnectorSlug, u32)> = to_seen
            .iter()
            .filter(|&(_, &n)| n > 1)
            .map(|(c, n)| (*c, *n))
            .collect();
        to_merges.sort_by_key(|(c, _)| c.slug());
        for (slug, n) in to_merges {
            issues.push(Issue {
                severity: Severity::Error,
                path: hop_list_path.to_string(),
                message: format!(
                    "Simple mode: {:?} appears {} times as a `to_connector`, which would merge multiple signals. Switch to Advanced (`body.mode: advanced`) to allow merging.",
                    slug.slug(),
                    n
                ),
            });
        }
    }

    issues
}

fn reachable_outputs(hops: &[(ConnectorSlug, ConnectorSlug)]) -> HashSet<ConnectorSlug> {
    let mut by_from: HashMap<ConnectorSlug, Vec<ConnectorSlug>> = HashMap::new();
    for &(f, t) in hops {
        by_from.entry(f).or_default().push(t);
    }
    let mut reached: HashSet<ConnectorSlug> = HashSet::new();
    let mut stack: Vec<ConnectorSlug> = vec![ConnectorSlug::InputTip, ConnectorSlug::InputRing];
    let mut visited: HashSet<ConnectorSlug> = HashSet::new();
    while let Some(node) = stack.pop() {
        if !visited.insert(node) {
            continue;
        }
        if node.is_output() {
            reached.insert(node);
        }
        if let Some(nexts) = by_from.get(&node) {
            for n in nexts {
                stack.push(*n);
            }
        }
    }
    reached
}

pub fn filter_blocking(issues: &[Issue]) -> Vec<&Issue> {
    issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::{Connection, PresetBody, SimpleHop, Spillover};
    use serde_json::json;

    fn good_yaml() -> Value {
        json!({
            "preset": {
                "bank": 1, "number": 0, "name": "Test",
                "spillover": {"output_tip": "nothing", "output_ring": "nothing"},
                "body": {
                    "mode": "simple",
                    "chain": [
                        {"from_connector": "input_tip", "to_connector": "output_tip", "bypass": false}
                    ]
                }
            }
        })
    }

    fn good_preset() -> Preset {
        Preset {
            bank: 1,
            number: 0,
            name: "OK".into(),
            spillover: Spillover::default(),
            body: PresetBody::Simple {
                chain: vec![
                    SimpleHop {
                        from_connector: ConnectorSlug::InputTip,
                        to_connector: ConnectorSlug::ATip,
                        bypass: false,
                    },
                    SimpleHop {
                        from_connector: ConnectorSlug::ATip,
                        to_connector: ConnectorSlug::OutputTip,
                        bypass: false,
                    },
                ],
            },
        }
    }

    fn simple_preset(chain: Vec<SimpleHop>) -> Preset {
        Preset {
            bank: 0,
            number: 0,
            name: "T".into(),
            spillover: Spillover::default(),
            body: PresetBody::Simple { chain },
        }
    }

    fn advanced_preset(connections: Vec<Connection>, spillover: Spillover) -> Preset {
        Preset {
            bank: 0,
            number: 0,
            name: "T".into(),
            spillover,
            body: PresetBody::Advanced { connections },
        }
    }

    #[test]
    fn schema_accepts_good_yaml() {
        assert!(validate_preset_schema(&good_yaml(), None).is_empty());
    }

    #[test]
    fn schema_rejects_bad_connector_slug() {
        let mut doc = good_yaml();
        doc["preset"]["body"]["chain"][0]["from_connector"] = json!("a_tipp");
        let issues = validate_preset_schema(&doc, None);
        // With the oneOf body discriminator, the bad slug surfaces as a
        // failure to match either variant. The offending literal still
        // appears in the error text — that's what users see.
        assert!(
            issues.iter().any(|i| i.message.contains("a_tipp")),
            "expected 'a_tipp' in some issue message; got: {issues:?}"
        );
    }

    #[test]
    fn schema_rejects_bank_out_of_range() {
        let mut doc = good_yaml();
        doc["preset"]["bank"] = json!(5);
        let issues = validate_preset_schema(&doc, None);
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("greater than the maximum"))
        );
    }

    #[test]
    fn schema_rejects_name_too_long() {
        let mut doc = good_yaml();
        doc["preset"]["name"] = json!("x".repeat(17));
        let issues = validate_preset_schema(&doc, None);
        assert!(issues.iter().any(|i| i.message.contains("maximum length")));
    }

    #[test]
    fn schema_rejects_unknown_field() {
        let mut doc = good_yaml();
        doc["preset"]["bogus_field"] = json!("x");
        let issues = validate_preset_schema(&doc, None);
        assert!(issues.iter().any(|i| i.message.contains("bogus_field")));
    }

    #[test]
    fn schema_rejects_bad_mode() {
        let mut doc = good_yaml();
        doc["preset"]["body"]["mode"] = json!("fancy");
        let issues = validate_preset_schema(&doc, None);
        assert!(issues.iter().any(|i| i.message.contains("fancy")));
    }

    #[test]
    fn semantics_accepts_good_preset() {
        let issues = validate_preset_semantics(&good_preset());
        assert!(filter_blocking(&issues).is_empty(), "issues: {issues:?}");
    }

    #[test]
    fn semantics_outputs_cannot_be_from() {
        let p = simple_preset(vec![SimpleHop {
            from_connector: ConnectorSlug::OutputTip,
            to_connector: ConnectorSlug::ATip,
            bypass: false,
        }]);
        let issues = validate_preset_semantics(&p);
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("cannot be a chain SOURCE"))
        );
    }

    #[test]
    fn semantics_inputs_cannot_be_to() {
        let p = simple_preset(vec![SimpleHop {
            from_connector: ConnectorSlug::ATip,
            to_connector: ConnectorSlug::InputTip,
            bypass: false,
        }]);
        let issues = validate_preset_semantics(&p);
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("cannot be a chain DESTINATION"))
        );
    }

    #[test]
    fn semantics_rejects_self_loops() {
        let p = simple_preset(vec![SimpleHop {
            from_connector: ConnectorSlug::ATip,
            to_connector: ConnectorSlug::ATip,
            bypass: false,
        }]);
        let issues = validate_preset_semantics(&p);
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("loops back on itself"))
        );
    }

    #[test]
    fn semantics_advanced_spillover_warns() {
        let p = advanced_preset(
            vec![],
            Spillover {
                output_tip: SpilloverTarget::DTip,
                output_ring: SpilloverTarget::Nothing,
            },
        );
        let issues = validate_preset_semantics(&p);
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("do not persist spillover")
                    && i.severity == Severity::Warning)
        );
    }

    #[test]
    fn semantics_no_input_output_path_warns() {
        let p = simple_preset(vec![SimpleHop {
            from_connector: ConnectorSlug::ATip,
            to_connector: ConnectorSlug::BTip,
            bypass: false,
        }]);
        let issues = validate_preset_semantics(&p);
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("no path from an input"))
        );
    }

    #[test]
    fn semantics_simple_mode_rejects_branching() {
        let p = simple_preset(vec![
            SimpleHop {
                from_connector: ConnectorSlug::InputTip,
                to_connector: ConnectorSlug::OutputTip,
                bypass: false,
            },
            SimpleHop {
                from_connector: ConnectorSlug::InputTip,
                to_connector: ConnectorSlug::OutputRing,
                bypass: false,
            },
        ]);
        let issues = validate_preset_semantics(&p);
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("branch the signal"))
        );
        assert!(issues.iter().any(|i| i.severity == Severity::Error));
    }

    #[test]
    fn semantics_advanced_mode_allows_branching() {
        let p = advanced_preset(
            vec![
                Connection {
                    from_connector: ConnectorSlug::InputTip,
                    to_connector: ConnectorSlug::OutputTip,
                },
                Connection {
                    from_connector: ConnectorSlug::InputTip,
                    to_connector: ConnectorSlug::OutputRing,
                },
            ],
            Spillover::default(),
        );
        let issues = validate_preset_semantics(&p);
        assert!(filter_blocking(&issues).is_empty(), "issues: {issues:?}");
    }

    #[test]
    fn semantics_duplicate_hop_warns() {
        let p = advanced_preset(
            vec![
                Connection {
                    from_connector: ConnectorSlug::InputTip,
                    to_connector: ConnectorSlug::ATip,
                },
                Connection {
                    from_connector: ConnectorSlug::InputTip,
                    to_connector: ConnectorSlug::ATip,
                },
                Connection {
                    from_connector: ConnectorSlug::ATip,
                    to_connector: ConnectorSlug::OutputTip,
                },
            ],
            Spillover::default(),
        );
        let issues = validate_preset_semantics(&p);
        assert!(issues.iter().any(|i| i.message.contains("duplicate hop")));
    }
}
