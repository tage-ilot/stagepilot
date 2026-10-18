//! A tiny in-process HTTP server for the behaviour tests: the Rust counterpart of
//! `httpx.MockTransport`. Every request is recorded; a handler closure decides the answer.
//! It binds 127.0.0.1 on an ephemeral port, so nothing here can reach a real service.
#![allow(dead_code)]

use axum::body::Bytes;
use axum::extract::{Request, State};
use axum::response::Response;
use serde_json::{json, Value};
use stagepilot_pco::models::{PcoSettings, Secret};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use url::Url;

#[derive(Clone, Debug)]
pub struct Req {
    pub method: String,
    pub path: String,
    pub query: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Req {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_lowercase()).map(String::as_str)
    }
}

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub delay: Duration,
}

impl Resp {
    pub fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            body: body.to_string().into_bytes(),
            delay: Duration::ZERO,
        }
    }
    pub fn raw(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![("content-type".into(), "text/html".into())],
            body: body.as_bytes().to_vec(),
            delay: Duration::ZERO,
        }
    }
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
    pub fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

type Handler = Arc<dyn Fn(&Req) -> Resp + Send + Sync>;

#[derive(Clone)]
struct AppState {
    handler: Handler,
    log: Arc<Mutex<Vec<Req>>>,
}

pub struct Mock {
    pub url: Url,
    pub origin: String,
    log: Arc<Mutex<Vec<Req>>>,
}

async fn serve(State(state): State<AppState>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let bytes: Bytes = axum::body::to_bytes(body, 1 << 20)
        .await
        .unwrap_or_default();
    let query = parts
        .uri
        .query()
        .map(|q| {
            url::form_urlencoded::parse(q.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect()
        })
        .unwrap_or_default();
    let headers = parts
        .headers
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_lowercase(),
                v.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    let req = Req {
        method: parts.method.to_string(),
        path: parts.uri.path().to_string(),
        query,
        headers,
        body: bytes.to_vec(),
    };
    state.log.lock().unwrap().push(req.clone());
    let resp = (state.handler)(&req);
    if !resp.delay.is_zero() {
        tokio::time::sleep(resp.delay).await;
    }
    let mut builder = Response::builder().status(resp.status);
    for (k, v) in &resp.headers {
        builder = builder.header(k, v);
    }
    builder.body(axum::body::Body::from(resp.body)).unwrap()
}

impl Mock {
    pub async fn start(handler: impl Fn(&Req) -> Resp + Send + Sync + 'static) -> Mock {
        let log = Arc::new(Mutex::new(Vec::new()));
        let state = AppState {
            handler: Arc::new(handler),
            log: log.clone(),
        };
        let app = axum::Router::new().fallback(serve).with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let origin = format!("http://127.0.0.1:{port}");
        Mock {
            url: Url::parse(&format!("{origin}/")).unwrap(),
            origin,
            log,
        }
    }

    pub fn requests(&self) -> Vec<Req> {
        self.log.lock().unwrap().clone()
    }
}

// ---- JSON:API builders (the Python tests' `service_type_resource`, `plan_resource`, ...) ------------

pub fn service_type(id: &str, name: &str, sequence: i64) -> Value {
    json!({"type": "ServiceType", "id": id, "attributes": {"name": name, "sequence": sequence},
        "future_top_level_field": "ignored"})
}

pub fn archived_service_type(id: &str, name: &str, sequence: i64) -> Value {
    json!({"type": "ServiceType", "id": id,
        "attributes": {"name": name, "sequence": sequence, "archived_at": "2026-01-01T00:00:00Z"}})
}

pub fn plan(id: &str, title: &str) -> Value {
    json!({"type": "Plan", "id": id, "attributes": {"title": title, "dates": "", "sort_date": null}})
}

pub fn plan_time(id: &str, starts_at: &str, time_type: &str) -> Value {
    json!({"type": "PlanTime", "id": id,
        "attributes": {"starts_at": starts_at, "time_type": time_type, "name": ""}})
}

pub fn item(
    id: &str,
    item_type: &str,
    title: &str,
    sequence: i64,
    length: Option<i64>,
    song: Option<&str>,
) -> Value {
    json!({"type": "Item", "id": id,
        "attributes": {"title": title, "item_type": item_type, "sequence": sequence, "length": length},
        "relationships": {"song": {"data": song.map(|s| json!({"type": "Song", "id": s}))}}})
}

pub fn page(data: Vec<Value>) -> Value {
    json!({"data": data, "meta": {"total_count": null}})
}

pub fn pat_settings() -> PcoSettings {
    PcoSettings {
        app_id: Some(Secret::new("test-app-id")),
        secret: Some(Secret::new("test-secret")),
        request_timeout_seconds: 3.0,
        user_agent: "StagePilot tests (https://github.com/tage-ilot/stagepilot)".into(),
        ..PcoSettings::default()
    }
}

pub fn oauth_settings() -> PcoSettings {
    PcoSettings {
        connection_method: stagepilot_core::config::ConnectionMethod::Oauth,
        access_token: Some(Secret::new("oauth-access-token")),
        request_timeout_seconds: 3.0,
        user_agent: "StagePilot tests (https://github.com/tage-ilot/stagepilot)".into(),
        ..PcoSettings::default()
    }
}
