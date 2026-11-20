//! Finding ProPresenter's network API by looking (`stagepilot_propresenter::scan`):
//!
//! * `POST /api/v1/propresenter/scan` starts the scan (a body, when there is one, must be an empty JSON object). A scan that is
//!   already running keeps going. Answers the scan's state.
//! * `POST /api/v1/propresenter/scan/cancel` stops it. Harmless when nothing runs. Answers the state.
//! * `GET  /api/v1/propresenter/scan` the state: `idle`, `scanning`, `found` or `not_found`, the computers that answered
//!   (`host`, `port`, `name`, `version`), how far it is, and why it found nothing when macOS blocks the local network.
//!
//! The scan asks every private address ONE thing, `GET /version`, and saves nothing: choosing a computer and pressing Connect is
//! the person's, through the same settings route as before. The two POSTs are writes like every other: the dashboard PIN, the
//! origin check and the remote-access rules sit in front of the whole router (a remote viewer cannot start a scan).

use crate::runtime::Runtime;
use crate::validation::{http_error, ok, Json};
use axum::body::Bytes;
use axum::extract::State;
use axum::routing::{get, post};
use axum::Router;
use serde_json::Value;
use std::sync::Arc;

type Rt = Arc<Runtime>;

fn state(rt: &Runtime) -> Json {
    ok(serde_json::to_value(rt.propresenter_scan.snapshot()).unwrap_or(Value::Null))
}

/// Nothing is asked of the person but "scan": an empty body or an empty object, nothing else.
fn empty_body(body: &Bytes) -> bool {
    if body.iter().all(u8::is_ascii_whitespace) {
        return true;
    }
    matches!(serde_json::from_slice::<Value>(body), Ok(Value::Object(map)) if map.is_empty())
}

async fn scan_state(State(rt): State<Rt>) -> Json {
    state(&rt)
}

async fn start_scan(State(rt): State<Rt>, body: Bytes) -> Json {
    if !empty_body(&body) {
        return http_error(422, "Send no fields: the scan looks on its own.");
    }
    let saved = rt
        .settings_service
        .effective_runtime_settings()
        .propresenter;
    rt.propresenter_scan.start(&saved.host, saved.port);
    state(&rt)
}

async fn cancel_scan(State(rt): State<Rt>) -> Json {
    rt.propresenter_scan.cancel().await;
    state(&rt)
}

pub fn routes() -> Router<Rt> {
    Router::new()
        .route(
            "/api/v1/propresenter/scan",
            get(scan_state).post(start_scan),
        )
        .route("/api/v1/propresenter/scan/cancel", post(cancel_scan))
}
