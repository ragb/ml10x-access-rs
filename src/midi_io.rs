//! MIDI I/O for the ML10X CLI, built on `midir` + a `crossbeam-channel`
//! bridge.
//!
//! Public surface:
//!     list_ports()                       -> (inputs, outputs)
//!     find_port(ports, substring)        -> &PortInfo
//!     MidiIo::open(port_substring)       -> MidiIo
//!     MidiIo::send_sysex(&[u8])
//!     MidiIo::receive_sysex(timeout)
//!
//! Unlike `mido`, `midir` delivers the complete SysEx (including
//! F0/F7) into the callback — we don't reattach delimiters.

use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError, unbounded};
use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MidiError {
    #[error("MIDI backend init failed: {0}")]
    Init(String),
    #[error("No {direction} port name contains {needle:?}. Available: {available}.")]
    NoPort {
        direction: &'static str,
        needle: String,
        available: String,
    },
    #[error("{count} {direction} ports match {needle:?} — be more specific. Matches: {matches}.")]
    AmbiguousPort {
        direction: &'static str,
        count: usize,
        needle: String,
        matches: String,
    },
    #[error("Could not open {direction} port {name:?}: {source}")]
    Open {
        direction: &'static str,
        name: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("send_sysex: message must be a complete SysEx (F0 ... F7)")]
    BadSysExFrame,
    #[error("Send failed: {0}")]
    Send(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortInfo {
    pub name: String,
    pub direction: &'static str, // "input" or "output"
}

impl std::fmt::Display for PortInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:<6} {}", self.direction, self.name)
    }
}

pub fn list_ports() -> Result<(Vec<PortInfo>, Vec<PortInfo>), MidiError> {
    let midi_in = MidiInput::new("ml10x-list")
        .map_err(|e| MidiError::Init(e.to_string()))?;
    let midi_out = MidiOutput::new("ml10x-list")
        .map_err(|e| MidiError::Init(e.to_string()))?;

    let mut inputs = Vec::new();
    for port in midi_in.ports() {
        let name = midi_in
            .port_name(&port)
            .map_err(|e| MidiError::Init(e.to_string()))?;
        inputs.push(PortInfo {
            name,
            direction: "input",
        });
    }
    let mut outputs = Vec::new();
    for port in midi_out.ports() {
        let name = midi_out
            .port_name(&port)
            .map_err(|e| MidiError::Init(e.to_string()))?;
        outputs.push(PortInfo {
            name,
            direction: "output",
        });
    }
    Ok((inputs, outputs))
}

/// Resolve a substring to exactly one port, case-insensitively.
pub fn find_port<'a>(ports: &'a [PortInfo], substring: &str) -> Result<&'a PortInfo, MidiError> {
    let needle = substring.to_lowercase();
    let hits: Vec<&PortInfo> = ports
        .iter()
        .filter(|p| p.name.to_lowercase().contains(&needle))
        .collect();
    let direction = ports.first().map(|p| p.direction).unwrap_or("MIDI");
    let available = if ports.is_empty() {
        "(none enumerated)".to_string()
    } else {
        ports
            .iter()
            .map(|p| p.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match hits.as_slice() {
        [] => Err(MidiError::NoPort {
            direction,
            needle: substring.to_string(),
            available,
        }),
        [one] => Ok(*one),
        many => Err(MidiError::AmbiguousPort {
            direction,
            count: many.len(),
            needle: substring.to_string(),
            matches: many.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", "),
        }),
    }
}

/// Paired input + output for one MIDI device.
pub struct MidiIo {
    _in_conn: MidiInputConnection<Sender<Vec<u8>>>,
    out: MidiOutputConnection,
    rx: Receiver<Vec<u8>>,
}

impl MidiIo {
    /// Open the input + output whose names contain `port_substring`.
    pub fn open(port_substring: &str) -> Result<Self, MidiError> {
        let midi_in = MidiInput::new("ml10x")
            .map_err(|e| MidiError::Init(e.to_string()))?;
        let midi_out = MidiOutput::new("ml10x")
            .map_err(|e| MidiError::Init(e.to_string()))?;

        // Resolve input port.
        let in_ports: Vec<_> = midi_in.ports();
        let mut in_named = Vec::with_capacity(in_ports.len());
        for p in &in_ports {
            in_named.push(PortInfo {
                name: midi_in.port_name(p).map_err(|e| MidiError::Init(e.to_string()))?,
                direction: "input",
            });
        }
        let in_info = find_port(&in_named, port_substring)?;
        let in_port = in_ports
            .iter()
            .find(|p| midi_in.port_name(p).map(|n| n == in_info.name).unwrap_or(false))
            .ok_or_else(|| MidiError::NoPort {
                direction: "input",
                needle: port_substring.to_string(),
                available: in_info.name.clone(),
            })?
            .clone();

        // Resolve output port.
        let out_ports: Vec<_> = midi_out.ports();
        let mut out_named = Vec::with_capacity(out_ports.len());
        for p in &out_ports {
            out_named.push(PortInfo {
                name: midi_out.port_name(p).map_err(|e| MidiError::Init(e.to_string()))?,
                direction: "output",
            });
        }
        let out_info = find_port(&out_named, port_substring)?;
        let out_port = out_ports
            .iter()
            .find(|p| midi_out.port_name(p).map(|n| n == out_info.name).unwrap_or(false))
            .ok_or_else(|| MidiError::NoPort {
                direction: "output",
                needle: port_substring.to_string(),
                available: out_info.name.clone(),
            })?
            .clone();

        let (tx, rx) = unbounded::<Vec<u8>>();

        let in_conn = midi_in
            .connect(
                &in_port,
                "ml10x-in",
                move |_timestamp, data, tx_inner| {
                    if data.first() == Some(&0xF0) {
                        let _ = tx_inner.send(data.to_vec());
                    }
                },
                tx,
            )
            .map_err(|e| MidiError::Open {
                direction: "input",
                name: in_info.name.clone(),
                source: Box::new(e),
            })?;

        let out_conn = midi_out.connect(&out_port, "ml10x-out").map_err(|e| MidiError::Open {
            direction: "output",
            name: out_info.name.clone(),
            source: Box::new(e),
        })?;

        Ok(Self {
            _in_conn: in_conn,
            out: out_conn,
            rx,
        })
    }

    /// Send a complete SysEx frame (must start with F0 and end with F7).
    pub fn send_sysex(&mut self, message: &[u8]) -> Result<(), MidiError> {
        if message.is_empty() || message[0] != 0xF0 || *message.last().unwrap() != 0xF7 {
            return Err(MidiError::BadSysExFrame);
        }
        self.out.send(message).map_err(|e| MidiError::Send(e.to_string()))
    }

    /// Block up to `timeout` and return the next inbound SysEx, or None on timeout.
    pub fn receive_sysex(&self, timeout: Duration) -> Option<Vec<u8>> {
        match self.rx.recv_timeout(timeout) {
            Ok(msg) => Some(msg),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => None,
        }
    }

    /// Discard any queued inbound messages.
    pub fn drain(&self) {
        loop {
            match self.rx.try_recv() {
                Ok(_) => {}
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_port_substring_match() {
        let ports = vec![
            PortInfo { name: "Morningstar ML10X 0".into(), direction: "input" },
            PortInfo { name: "Other Device 1".into(), direction: "input" },
        ];
        let p = find_port(&ports, "ml10x").unwrap();
        assert_eq!(p.name, "Morningstar ML10X 0");
    }

    #[test]
    fn find_port_no_match_lists_available() {
        let ports = vec![
            PortInfo { name: "Other Device".into(), direction: "input" },
        ];
        let err = find_port(&ports, "ml10x").unwrap_err();
        match err {
            MidiError::NoPort { available, .. } => {
                assert!(available.contains("Other Device"), "got: {available}");
            }
            other => panic!("expected NoPort, got {other:?}"),
        }
    }

    #[test]
    fn find_port_ambiguous() {
        let ports = vec![
            PortInfo { name: "ML10X A".into(), direction: "input" },
            PortInfo { name: "ML10X B".into(), direction: "input" },
        ];
        let err = find_port(&ports, "ml10x").unwrap_err();
        match err {
            MidiError::AmbiguousPort { count: 2, .. } => {}
            other => panic!("expected AmbiguousPort, got {other:?}"),
        }
    }
}
