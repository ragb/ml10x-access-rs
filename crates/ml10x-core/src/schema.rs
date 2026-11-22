//! JSON Schema generation for the YAML envelope shapes.
//!
//! Gated behind `feature = "schema"`. The CLI enables it and exposes the
//! schemas via `ml10x schema [preset|controller]`. The output is the source
//! of the checked-in `schemas/{preset,controller}.schema.json` files; CI
//! re-runs the command and `git diff`s to catch drift between the derives
//! and the committed copy.
//!
//! schemars 0.8 emits draft-07 with a top-level `definitions` map. The
//! checked-in files declare draft 2020-12 and use `$defs`, both because
//! that's what `validate.rs` already loads at runtime and because the
//! `yaml-language-server` extension is happy with either. We post-process
//! the schemars output to the 2020-12 shape so the committed copy stays
//! stable.

use serde_json::Value;

const PRESET_SCHEMA_ID: &str = "https://github.com/ragb/ml10x-access-rs/schemas/preset.schema.json";
const CONTROLLER_SCHEMA_ID: &str =
    "https://github.com/ragb/ml10x-access-rs/schemas/controller.schema.json";

const META_SCHEMA: &str = "https://json-schema.org/draft/2020-12/schema";

/// JSON Schema for the preset YAML envelope (`{ preset: { … } }`).
pub fn preset_schema() -> Value {
    let root = schemars::schema_for!(crate::yaml::PresetDoc);
    to_draft_2020_12(root, PRESET_SCHEMA_ID)
}

/// JSON Schema for the controller YAML envelope (`{ controller: { … } }`).
pub fn controller_schema() -> Value {
    let root = schemars::schema_for!(crate::yaml::ControllerDoc);
    to_draft_2020_12(root, CONTROLLER_SCHEMA_ID)
}

fn to_draft_2020_12(root: schemars::schema::RootSchema, id: &str) -> Value {
    let mut v = serde_json::to_value(root).expect("RootSchema serializes");
    if let Some(obj) = v.as_object_mut() {
        obj.insert("$schema".into(), Value::String(META_SCHEMA.into()));
        obj.insert("$id".into(), Value::String(id.into()));
        if let Some(defs) = obj.remove("definitions") {
            obj.insert("$defs".into(), defs);
        }
    }
    rewrite_refs(&mut v);
    v
}

fn rewrite_refs(v: &mut Value) {
    match v {
        Value::Object(map) => {
            for (k, val) in map.iter_mut() {
                if k == "$ref" {
                    if let Some(s) = val.as_str()
                        && let Some(rest) = s.strip_prefix("#/definitions/")
                    {
                        *val = Value::String(format!("#/$defs/{rest}"));
                    }
                } else {
                    rewrite_refs(val);
                }
            }
        }
        Value::Array(arr) => arr.iter_mut().for_each(rewrite_refs),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_schema_uses_draft_2020_12() {
        let s = preset_schema();
        assert_eq!(s["$schema"], Value::String(META_SCHEMA.into()));
        assert_eq!(s["$id"], Value::String(PRESET_SCHEMA_ID.into()));
        assert!(s.get("definitions").is_none());
        // Top-level envelope check.
        assert_eq!(s["title"], Value::String("ML10X Preset".into()));
    }

    #[test]
    fn controller_schema_uses_draft_2020_12() {
        let s = controller_schema();
        assert_eq!(s["$schema"], Value::String(META_SCHEMA.into()));
        assert_eq!(s["$id"], Value::String(CONTROLLER_SCHEMA_ID.into()));
    }

    #[test]
    fn refs_rewritten_to_defs() {
        let s = preset_schema();
        let dumped = serde_json::to_string(&s).unwrap();
        assert!(
            !dumped.contains("#/definitions/"),
            "leftover definitions ref:\n{dumped}"
        );
    }
}
