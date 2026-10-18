//! Event-driven OSC or MIDI output timeline for lighting applications such as Lightkey:
//! port of `plugins/lights/plugin.py` (rosc replaces python-osc, midir replaces mido).
//!
//! Only the saved method fires: OSC cues are one-shot UDP messages, MIDI cues are short note
//! pulses, the inactive method's cues stay saved but are never scheduled and the two are never
//! sent together. Every output sits behind a trait, so all of it is tested without hardware.

pub mod models;
pub mod plugin;

pub use models::*;
pub use plugin::{LightingOutcome, LightsOptions, LightsPlugin, OscBackendFactory};
