//! Thin presentation layer for CLI output.
//!
//! Three verbosity levels (Quiet / Normal / Verbose) plus a separate
//! JSON-output mode. Each command builds its result as a `serde_json::Value`
//! and calls `emit_result` at the end. In JSON mode that goes to stdout;
//! in human mode the optional summary line is printed.

use serde::Serialize;
use serde_json::{Map, Value, json};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum Verbosity {
    Quiet,
    #[default]
    Normal,
    Verbose,
}

#[derive(Debug, Default)]
pub struct Out {
    pub verbosity: Verbosity,
    pub json_mode: bool,
    /// Buffered progress events for JSON mode.
    pub events: Vec<Value>,
}

impl Out {
    pub fn new(verbosity: Verbosity, json_mode: bool) -> Self {
        Self {
            verbosity,
            json_mode,
            events: Vec::new(),
        }
    }

    pub fn is_quiet(&self) -> bool {
        self.verbosity == Verbosity::Quiet
    }

    pub fn is_verbose(&self) -> bool {
        self.verbosity == Verbosity::Verbose
    }

    /// Normal-priority status line. Suppressed in quiet/JSON modes; in
    /// JSON mode the optional `event` value is appended to the result.
    pub fn info(&mut self, message: &str, event: Option<Value>) {
        if self.json_mode {
            if let Some(e) = event {
                self.events.push(e);
            }
            return;
        }
        if self.is_quiet() {
            return;
        }
        println!("{message}");
    }

    /// Verbose-only detail. Suppressed unless verbose.
    pub fn detail(&mut self, message: &str, event: Option<Value>) {
        if self.json_mode {
            if let Some(e) = event {
                self.events.push(e);
            }
            return;
        }
        if !self.is_verbose() {
            return;
        }
        println!("{message}");
    }

    /// Warning to stderr. Always shown (even in quiet) unless JSON, where
    /// it's recorded with a `level: warning` tag in the events list.
    pub fn warn(&mut self, message: &str, event: Option<Value>) {
        if self.json_mode {
            if let Some(mut e) = event {
                if let Some(obj) = e.as_object_mut() {
                    obj.insert("level".into(), json!("warning"));
                }
                self.events.push(e);
            }
            return;
        }
        eprintln!("{message}");
    }

    /// Error to stderr. Always shown.
    pub fn error(&mut self, message: &str, event: Option<Value>) {
        if self.json_mode {
            if let Some(mut e) = event {
                if let Some(obj) = e.as_object_mut() {
                    obj.insert("level".into(), json!("error"));
                }
                self.events.push(e);
            }
            return;
        }
        eprintln!("{message}");
    }

    /// Final command output. In JSON mode emits `{...result, events: [...]}`
    /// on stdout. In human mode prints the `summary` line if provided.
    pub fn emit_result(&mut self, result: &impl Serialize, summary: Option<&str>) {
        if self.json_mode {
            let mut value = serde_json::to_value(result).unwrap_or(Value::Null);
            if let Some(obj) = value.as_object_mut() {
                obj.insert(
                    "events".into(),
                    Value::Array(std::mem::take(&mut self.events)),
                );
            } else {
                let mut wrap = Map::new();
                wrap.insert("result".into(), value);
                wrap.insert(
                    "events".into(),
                    Value::Array(std::mem::take(&mut self.events)),
                );
                value = Value::Object(wrap);
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_default()
            );
            return;
        }
        if let Some(s) = summary {
            if !self.is_quiet() {
                println!("{s}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quiet_mode_suppresses_info() {
        let mut out = Out::new(Verbosity::Quiet, false);
        out.info("hello", None);
        // No panic; nothing observable since stdout isn't captured by default.
    }

    #[test]
    fn json_mode_collects_events() {
        let mut out = Out::new(Verbosity::Normal, true);
        out.info("hi", Some(json!({"step": 1})));
        out.warn("careful", Some(json!({"step": 2})));
        assert_eq!(out.events.len(), 2);
        assert_eq!(out.events[1]["level"], json!("warning"));
    }
}
