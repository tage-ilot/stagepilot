//! Planning Center for StagePilot: the OAuth sign-in, the Services API
//! client, the plan-loading plugin, the setup service and the song-length writer, ported from the
//! Python `planning_center` package. Nothing here contacts the real Planning Center unless a
//! caller builds a client against the default base URL.

pub mod client;
pub mod errors;
pub mod lengths;
pub mod models;
pub mod oauth;
pub mod plan_cache;
pub mod plugin;
pub mod runtime;
pub mod settings;
pub mod setup;
