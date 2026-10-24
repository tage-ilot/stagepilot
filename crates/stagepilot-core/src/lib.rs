//! StagePilot core: models, settings, state service, event bus, plugin supervisor and auth.
//!
//! No HTTP here (that is `stagepilot-server`). Everything is a port of the Python backend and keeps
//! its JSON names, defaults and messages; see `docs/rust-migration/phase2.md`.

pub mod alerts;
pub mod api;
pub mod auth;
pub mod bus;
pub mod config;
pub mod events;
pub mod logging;
pub mod plugin;
pub mod redact;
pub mod settings;
pub mod settings_errors;
pub mod settings_file;
pub mod state;
pub mod state_service;
pub mod store;
