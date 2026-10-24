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

    /// Every frame mido 1.3.3 produced for 2.41.1's calls (see tests/fixtures/generate_midi_golden.py),
    /// all 16 channels, several notes and velocities.
    #[test]
    fn wire_bytes_match_the_committed_mido_golden_file() {
        let golden: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/midi_frames_golden.json"))
                .unwrap();
        let frames = golden["frames"].as_array().unwrap();
        assert_eq!(frames.len(), 16 * 5 * 5); // 4 note-on velocities + 1 note-off per note
        let mut channels = std::collections::BTreeSet::new();
        for frame in frames {
            let channel = frame["channel"].as_u64().unwrap() as u8;
            let note = frame["note"].as_u64().unwrap() as u8;
            let velocity = frame["velocity"].as_u64().unwrap() as u8;
            let expected: Vec<u8> = frame["bytes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b.as_u64().unwrap() as u8)
                .collect();
            let actual = match frame["kind"].as_str().unwrap() {
                "note_on" => note_on_bytes(channel, note, velocity),
                "note_off" => note_off_bytes(channel, note),
                other => panic!("unknown frame kind {other}"),
            };
            assert_eq!(actual.to_vec(), expected, "{frame}");
            channels.insert(channel);
        }
        assert_eq!(channels.len(), 16);
    }
}
