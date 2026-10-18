//! StagePilot HTTP/WebSocket server (Rust migration phase 2): the `/api/v1/*` routes for health,
//! state, settings, dashboard auth, actions and the `/ws` state stream, plus static dashboard
//! serving. Integrations that later phases port are stubbed with what Python returns while the
//! plugin is disabled.

pub mod app;
pub mod demo;
pub mod outputs;
pub mod routes_outputs;
pub mod runtime;
pub mod validation;

pub use app::{build_app, default_web_root, AppOptions};
pub use outputs::OutputBackends;
pub use runtime::Runtime;
