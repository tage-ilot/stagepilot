//! MIDI **output** boundary used by the lights plugin: port of `plugins/lights/client.py`.

use crate::MidiError;
use std::sync::Arc;

/// `MidiOutputPortContract`. Channels are one-based, as the operator sees them.
pub trait MidiOutputPort: Send + Sync {
    fn closed(&self) -> bool;
    fn send_note_on(&self, channel: u8, note: u8, velocity: u8) -> Result<(), MidiError>;
    /// Note off always carries velocity 0, like the Python port.
    fn send_note_off(&self, channel: u8, note: u8) -> Result<(), MidiError>;
    fn close(&self);
}

/// `MidiOutputBackendContract`.
pub trait MidiOutputBackend: Send + Sync {
    fn list_output_names(&self) -> Result<Vec<String>, MidiError>;
    /// Open exactly one output by name; a missing or ambiguous name is an error.
    fn open_output(&self, name: &str) -> Result<Arc<dyn MidiOutputPort>, MidiError>;
}

pub type MidiOutputBackendFactory =
    Arc<dyn Fn() -> Result<Arc<dyn MidiOutputBackend>, MidiError> + Send + Sync>;

/// The three raw bytes the Python `MidoOutputPort` puts on the wire.
pub fn note_on_bytes(channel: u8, note: u8, velocity: u8) -> [u8; 3] {
    [0x90 | (channel.saturating_sub(1) & 0x0F), note, velocity]
}

pub fn note_off_bytes(channel: u8, note: u8) -> [u8; 3] {
    [0x80 | (channel.saturating_sub(1) & 0x0F), note, 0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_bytes_match_mido_for_every_channel() {
        for channel in 1..=16u8 {
            // mido.Message("note_on", channel=channel-1, note=72, velocity=110).bytes()
            assert_eq!(
                note_on_bytes(channel, 72, 110),
                [0x90 + (channel - 1), 72, 110]
            );
            assert_eq!(note_off_bytes(channel, 72), [0x80 + (channel - 1), 72, 0]);
        }
    }
}
