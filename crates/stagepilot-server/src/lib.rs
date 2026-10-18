//! StagePilot HTTP/WebSocket server (Rust migration phase 2): the `/api/v1/*` routes for health,
//! state, settings, dashboard auth, actions and the `/ws` state stream, plus static dashboard
//! serving. Integrations that later phases port are stubbed with what Python returns while the
//! plugin is disabled.

pub mod app;
pub mod demo;
pub mod diagnostics;
pub mod embedded;
pub mod launch;
pub mod outputs;
pub mod pco_routes;
pub mod playback_routes;
pub mod remote_fakes;
pub mod remote_feature;
pub mod remote_ingress;
pub mod remote_routes;
pub mod remote_types;
pub mod routes_outputs;
pub mod runtime;
pub mod startup_activation;
pub mod static_files;
pub mod validation;

pub use app::{build_app, default_web_root, AppOptions};
pub use outputs::OutputBackends;
pub use remote_feature::RemoteFeature;
pub use remote_ingress::RemoteIngress;
pub use remote_types::{
    ActiveEnrollment, DiagnosticsBootstrap, ManagerError, RemoteAccess, RemoteContext,
    RemoteManager, RemoteWiring,
};
pub use runtime::Runtime;
