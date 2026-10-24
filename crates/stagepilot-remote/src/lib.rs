//! Remote Access for StagePilot: the Rust port of the `remote_*` Python modules.
//!
//! OS-dependent parts sit behind traits with in-memory fakes in [`platform`]: the clock, the
//! exclusive file lock, child-process supervision of `cloudflared`, port probing and the
//! credential broker ([`bootstrap::RemoteCredentialStore`]). Everything else is pure logic.
//! The HTTP ingress middleware (`RemoteIngress`) belongs to the server crate; this crate takes it
//! as an [`runtime::IngressFactory`].

pub mod beta_control;
pub mod bootstrap;
pub mod connector;
pub mod control;
pub mod desktop;
pub mod error;
pub mod feature;
pub mod files;
pub mod health;
pub mod health_alert;
pub mod http;
pub mod orphan;
pub mod platform;
pub mod provider;
pub mod quick;
pub mod runtime;
pub mod server;

pub use error::{RemoteError, Res};
