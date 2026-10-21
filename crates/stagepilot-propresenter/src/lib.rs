//! ProPresenter countdown output: port of `plugins/propresenter/` (HTTP client, models, plugin)
//! with `reqwest` in place of `httpx`. Endpoints, timeouts, error texts and the stale-UUID retry
//! are the Python ones; `tests/` run against a local mock HTTP server using the Python tests'
//! expectations.

pub mod client;
pub mod errors;
pub mod models;
pub mod plugin;
pub mod snapshot;

pub use client::{HttpProPresenterClient, ProPresenterClient, Tuning};
pub use errors::PpError;
pub use models::{Countdown, Identifier, Look, Timer};
pub use plugin::{ClientFactory, ProPresenterPlugin};
pub use snapshot::*;
