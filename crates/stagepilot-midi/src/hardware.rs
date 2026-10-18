//! `midir` backends (feature `midir-backend`). Never compiled on the self-hosted Linux runner.
//!
//! Matches the Python `Mido*` boundary: exact-name open, ambiguity is an error, the same public
//! messages. A midir `MidiInput` / `MidiOutput` is created per operation (cheap, and it keeps the
//! backend `Send + Sync` on every platform); the connection owns its own client.
//! Nothing here sends anything by itself: output ports send only what the lights plugin asks for.

use crate::output::{note_off_bytes, note_on_bytes, MidiOutputBackend, MidiOutputPort};
use crate::{normalise_bytes, MidiBackend, MidiError, MidiInputPort, MidiMessageCallback};
use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use std::sync::{Arc, Mutex};

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

const CLIENT: &str = "StagePilot";

/// Ports are matched by the display name midir reports for each port.
fn input_names(input: &MidiInput) -> Vec<String> {
    input
        .ports()
        .iter()
        .filter_map(|p| input.port_name(p).ok())
        .collect()
}

fn output_names(output: &MidiOutput) -> Vec<String> {
    output
        .ports()
        .iter()
        .filter_map(|p| output.port_name(p).ok())
        .collect()
}

pub struct MidirBackend;

pub struct MidirInputPort {
    connection: Mutex<Option<MidiInputConnection<()>>>,
}

impl MidiInputPort for MidirInputPort {
    fn closed(&self) -> bool {
        lock(&self.connection).is_none()
    }
    fn close(&self) {
        if let Some(connection) = lock(&self.connection).take() {
            connection.close();
        }
    }
}

impl MidiBackend for MidirBackend {
    fn list_input_names(&self) -> Result<Vec<String>, MidiError> {
        let input = MidiInput::new(CLIENT).map_err(|e| MidiError::internal(e.to_string()))?;
        Ok(input_names(&input))
    }

    fn open_input(
        &self,
        name: &str,
        callback: MidiMessageCallback,
    ) -> Result<Arc<dyn MidiInputPort>, MidiError> {
        let mut input = MidiInput::new(CLIENT).map_err(|e| MidiError::internal(e.to_string()))?;
        // Note messages only: sysex, timing and active sensing are never wanted.
        input.ignore(midir::Ignore::All);
        let matching: Vec<_> = input
            .ports()
            .into_iter()
            .filter(|p| input.port_name(p).is_ok_and(|n| n == name))
            .collect();
        match matching.len() {
            0 => {
                return Err(MidiError::public(
                    "The configured MIDI input is unavailable.",
                ))
            }
            1 => {}
            _ => {
                return Err(MidiError::public(
                    "The configured MIDI input name is ambiguous.",
                ))
            }
        }
        let connection = input
            .connect(
                &matching[0],
                "stagepilot-in",
                move |_stamp, bytes, _| {
                    if let Some(message) = normalise_bytes(bytes) {
                        callback(message);
                    }
                },
                (),
            )
            .map_err(|e| MidiError::internal(e.to_string()))?;
        Ok(Arc::new(MidirInputPort {
            connection: Mutex::new(Some(connection)),
        }))
    }
}

pub struct MidirOutputBackend;

pub struct MidirOutputPort {
    connection: Mutex<Option<MidiOutputConnection>>,
}

impl MidirOutputPort {
    fn send(&self, bytes: [u8; 3]) -> Result<(), MidiError> {
        let mut guard = lock(&self.connection);
        let Some(connection) = guard.as_mut() else {
            return Err(MidiError::public("The MIDI output is closed."));
        };
        connection
            .send(&bytes)
            .map_err(|e| MidiError::internal(e.to_string()))
    }
}

impl MidiOutputPort for MidirOutputPort {
    fn closed(&self) -> bool {
        lock(&self.connection).is_none()
    }
    fn send_note_on(&self, channel: u8, note: u8, velocity: u8) -> Result<(), MidiError> {
        self.send(note_on_bytes(channel, note, velocity))
    }
    fn send_note_off(&self, channel: u8, note: u8) -> Result<(), MidiError> {
        self.send(note_off_bytes(channel, note))
    }
    fn close(&self) {
        if let Some(connection) = lock(&self.connection).take() {
            connection.close();
        }
    }
}

impl MidiOutputBackend for MidirOutputBackend {
    fn list_output_names(&self) -> Result<Vec<String>, MidiError> {
        let output = MidiOutput::new(CLIENT).map_err(|e| MidiError::internal(e.to_string()))?;
        Ok(output_names(&output))
    }

    fn open_output(&self, name: &str) -> Result<Arc<dyn MidiOutputPort>, MidiError> {
        let output = MidiOutput::new(CLIENT).map_err(|e| MidiError::internal(e.to_string()))?;
        let matching: Vec<_> = output
            .ports()
            .into_iter()
            .filter(|p| output.port_name(p).is_ok_and(|n| n == name))
            .collect();
        match matching.len() {
            0 => {
                return Err(MidiError::public(
                    "The configured lighting MIDI output is unavailable.",
                ))
            }
            1 => {}
            _ => {
                return Err(MidiError::public(
                    "The configured lighting MIDI output name is ambiguous.",
                ))
            }
        }
        let connection = output
            .connect(&matching[0], "stagepilot-out")
            .map_err(|e| MidiError::internal(e.to_string()))?;
        Ok(Arc::new(MidirOutputPort {
            connection: Mutex::new(Some(connection)),
        }))
    }
}
