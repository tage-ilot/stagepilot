//! Event-driven OSC or MIDI output timeline for lighting applications such as Lightkey:
//! port of `plugins/lights/plugin.py` (rosc replaces python-osc, midir replaces mido).
//!
//! Only the saved method fires: OSC cues are one-shot UDP messages, MIDI cues are short note
//! pulses, the inactive method's cues stay saved but are never scheduled and the two are never
//! sent together. Every output sits behind a trait, so all of it is tested without hardware.

pub mod audio;
#[cfg(feature = "cpal-backend")]
pub mod audio_cpal;
pub mod lightkey;
pub mod lightkey_auto;
pub mod lightkey_curve;
pub mod lightkey_discovery;
pub mod lightkey_effects;
pub mod lightkey_fit;
pub mod lightkey_fixtures;
pub mod lightkey_helper_assets;
pub mod lightkey_link;
pub mod lightkey_looks;
pub mod lightkey_operator;
pub mod lightkey_recording;
pub mod lightkey_relay;
pub mod lightkey_replay;
pub mod lightkey_smooth;
pub mod lightkey_start;
pub mod lightkey_yield_check;
pub mod loudness;
pub mod models;
pub mod plugin;

pub use models::*;
pub use plugin::{LightingOutcome, LightsOptions, LightsPlugin, OscBackendFactory};
