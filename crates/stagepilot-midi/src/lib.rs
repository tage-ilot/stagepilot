//! MIDI for StagePilot: the Playback MIDI input plugin
//! (`plugins/midi_playback`) and the MIDI **output** boundary the lights plugin uses
//! (`plugins/lights/client.py`).
//!
//! Hardware sits behind two traits, [`MidiBackend`] (inputs) and [`MidiOutputBackend`] (outputs).
//! Everything in the plugin is tested against the in-memory [`fake`] backends; the real
//! implementation ([`hardware`], `midir`) is compiled only with the `midir-backend` feature.
//!
//! Platform notes (also in `docs/archive/rust-migration/phase4.md`):
//!
//! * **Windows** (WinMM): needs nothing, but a port can be opened by one process at a time. The
//!   device list only changes when a device is plugged in; names are the WinMM product names and
//!   are truncated to 31 characters, so two similar devices can collide ("ambiguous").
//! * **macOS** (CoreMIDI): needs nothing. Port names are `Endpoint name` (the device name is not
//!   prefixed as mido/rtmidi did), so a saved `input_name` from the Python build may differ.
//! * **Linux** (ALSA seq): names are `Client:Port client_id:port`; the numeric suffix changes
//!   when the device is replugged. Building needs `libasound2-dev` and `pkg-config`.
//! * CI runners have no MIDI devices; listing ports is expected to return nothing there.

pub mod fake;
#[cfg(feature = "midir-backend")]
pub mod hardware;
pub mod models;
pub mod output;
pub mod plugin;

pub use models::*;
pub use output::{MidiOutputBackend, MidiOutputPort};
pub use plugin::{ActionDispatcher, MidiPlaybackPlugin, MidiPluginOptions};

use std::sync::Arc;

/// A failed backend operation. `public` mirrors Python's `ValueError`/`RuntimeError`: only those
/// messages reach the operator, everything else collapses to the caller's fallback text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiError {
    pub public: bool,
    pub message: String,
}

impl MidiError {
    pub fn public(message: impl Into<String>) -> Self {
        Self {
            public: true,
            message: message.into(),
        }
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            public: false,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for MidiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MidiError {}

/// Callback a backend calls (on its own thread) for every note message it receives.
pub type MidiMessageCallback = Box<dyn Fn(MidiMessage) + Send + Sync>;

/// `MidiInputPortContract`: the lifecycle surface of one open input.
pub trait MidiInputPort: Send + Sync {
    fn closed(&self) -> bool;
    fn close(&self);
}

/// `MidiBackendContract`: discover and open MIDI inputs without exposing vendor objects.
pub trait MidiBackend: Send + Sync {
    fn list_input_names(&self) -> Result<Vec<String>, MidiError>;
    /// Open exactly one input by name; a missing or ambiguous name is an error.
    fn open_input(
        &self,
        name: &str,
        callback: MidiMessageCallback,
    ) -> Result<Arc<dyn MidiInputPort>, MidiError>;
}

/// `MidiBackendFactory`: building the backend is itself fallible (and may be slow).
pub type MidiBackendFactory =
    Arc<dyn Fn() -> Result<Arc<dyn MidiBackend>, MidiError> + Send + Sync>;

/// Translate raw MIDI bytes the way `_normalize_message` does: note on/off only, a note-on with
/// velocity 0 is a note-off, channel becomes one-based, anything malformed is dropped.
pub fn normalise_bytes(bytes: &[u8]) -> Option<MidiMessage> {
    let [status, note, velocity] = *bytes else {
        return None;
    };
    if note > 127 || velocity > 127 {
        return None;
    }
    let channel = (status & 0x0F) + 1;
    let kind = match status & 0xF0 {
        0x90 if velocity == 0 => NoteType::NoteOff,
        0x90 => NoteType::NoteOn,
        0x80 => NoteType::NoteOff,
        _ => return None,
    };
    Some(MidiMessage {
        kind,
        channel,
        note,
        velocity,
    })
}

/// True when this build can talk to MIDI hardware (the `midir-backend` feature).
pub const HARDWARE_SUPPORT: bool = cfg!(feature = "midir-backend");

/// Name of the compiled-in MIDI backend, for `--version` and startup logs: `midir`, or `none`.
pub fn backend_name() -> &'static str {
    if HARDWARE_SUPPORT {
        "midir"
    } else {
        "none"
    }
}

/// The MIDI backend selected at build time: `midir` with the `midir-backend` feature, otherwise
/// an error saying the build has no MIDI hardware support (the plugin then reports an error and
/// the supervisor retries, exactly as it would for any backend that cannot start).
pub fn default_backend_factory() -> MidiBackendFactory {
    #[cfg(feature = "midir-backend")]
    {
        Arc::new(|| Ok(Arc::new(hardware::MidirBackend) as Arc<dyn MidiBackend>))
    }
    #[cfg(not(feature = "midir-backend"))]
    {
        Arc::new(|| {
            Err(MidiError::internal(
                "This build has no MIDI hardware support (built without the midir-backend feature).",
            ))
        })
    }
}

/// Same for the output side (lights).
pub fn default_output_backend_factory() -> output::MidiOutputBackendFactory {
    #[cfg(feature = "midir-backend")]
    {
        Arc::new(|| Ok(Arc::new(hardware::MidirOutputBackend) as Arc<dyn MidiOutputBackend>))
    }
    #[cfg(not(feature = "midir-backend"))]
    {
        Arc::new(|| {
            Err(MidiError::internal(
                "This build has no MIDI hardware support (built without the midir-backend feature).",
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_on_zero_velocity_is_a_note_off_and_channels_are_one_based() {
        let m = normalise_bytes(&[0x90, 112, 0]).unwrap();
        assert_eq!(
            (m.kind, m.channel, m.note, m.velocity),
            (NoteType::NoteOff, 1, 112, 0)
        );
        let m = normalise_bytes(&[0x9F, 1, 100]).unwrap();
        assert_eq!((m.kind, m.channel), (NoteType::NoteOn, 16));
        let m = normalise_bytes(&[0x80, 5, 64]).unwrap();
        assert_eq!((m.kind, m.velocity), (NoteType::NoteOff, 64));
    }

    #[test]
    fn non_note_malformed_and_out_of_range_messages_are_dropped() {
        assert!(normalise_bytes(&[0xB0, 1, 2]).is_none()); // control change
        assert!(normalise_bytes(&[0xF8]).is_none()); // clock
        assert!(normalise_bytes(&[0x90, 200, 10]).is_none());
        assert!(normalise_bytes(&[0x90, 10, 200]).is_none());
        assert!(normalise_bytes(&[0x90, 10]).is_none());
        assert!(normalise_bytes(&[0xF0, 1, 2]).is_none());
    }
}
