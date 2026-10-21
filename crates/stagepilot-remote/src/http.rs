//! Blocking HTTP seam. Production uses reqwest (rustls/ring, no redirects, no environment
//! proxies, as `httpx.Client(trust_env=False, follow_redirects=False)`); tests plug in fakes.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::error::{RemoteError, Res};

#[derive(Debug, Clone, Default)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub query: Vec<(String, String)>,
    pub json: Option<Value>,
    pub body: Option<String>,
    pub timeout_secs: f64,
}

impl HttpRequest {
    /// Path (without origin) of the request URL.
    pub fn path(&self) -> String {
        match url::Url::parse(&self.url) {
            Ok(parsed) => parsed.path().to_owned(),
            Err(_) => self.url.clone(),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug, Clone, Default)]
pub struct HttpResponse {
    pub status: u16,
    /// Lower-cased names.
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

impl HttpResponse {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body: body.into(),
        }
    }

    pub fn json_response(status: u16, value: &Value) -> Self {
        Self::new(status, value.to_string())
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn json(&self) -> Res<Value> {
        serde_json::from_str(&self.body).map_err(RemoteError::from)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

/// `httpx.HTTPError`: connection failure, timeout, protocol error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TransportError(pub String);

pub trait HttpTransport: Send + Sync {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError>;
}

/// A tiny `httpx.Client` stand-in: base URL, default headers and timeout over a transport.
#[derive(Clone)]
pub struct HttpClient {
    pub base_url: String,
    pub headers: Vec<(String, String)>,
    pub timeout_secs: f64,
    transport: Arc<dyn HttpTransport>,
}

impl HttpClient {
    pub fn new(base_url: &str, timeout_secs: f64, transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            headers: Vec::new(),
            timeout_secs,
            transport,
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    pub fn request(
        &self,
        method: &str,
        path: &str,
        headers: &[(String, String)],
        query: &[(String, String)],
        json: Option<Value>,
    ) -> Result<HttpResponse, TransportError> {
        let mut all = self.headers.clone();
        all.extend_from_slice(headers);
        self.transport.send(&HttpRequest {
            method: method.to_owned(),
            url: format!("{}{path}", self.base_url),
            headers: all,
            query: query.to_vec(),
            json,
            body: None,
            timeout_secs: self.timeout_secs,
        })
    }

    pub fn get(&self, path: &str) -> Result<HttpResponse, TransportError> {
        self.request("GET", path, &[], &[], None)
    }
}

/// The production transport. The blocking client owns a private runtime, which tokio refuses to
/// drop inside an async context, so it is built on first use and dropped on its own thread.
pub struct ReqwestTransport {
    client: std::sync::OnceLock<reqwest::blocking::Client>,
}

impl ReqwestTransport {
    pub fn new() -> Self {
        Self {
            client: std::sync::OnceLock::new(),
        }
    }

    fn client(&self) -> &reqwest::blocking::Client {
        self.client.get_or_init(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
            reqwest::blocking::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new())
        })
    }
}

impl Default for ReqwestTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ReqwestTransport {
    fn drop(&mut self) {
        if let Some(client) = self.client.take() {
            std::thread::spawn(move || drop(client));
        }
    }
}

impl HttpTransport for ReqwestTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let fail = |e: reqwest::Error| TransportError(e.without_url().to_string());
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|e| TransportError(e.to_string()))?;
        let mut url = url::Url::parse(&request.url).map_err(|e| TransportError(e.to_string()))?;
        if !request.query.is_empty() {
            url.query_pairs_mut().extend_pairs(request.query.iter());
        }
        let mut builder = self
            .client()
            .request(method, url)
            .timeout(Duration::from_secs_f64(request.timeout_secs.max(0.001)));
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(value) = &request.json {
            builder = builder.json(value);
        } else if let Some(body) = &request.body {
            builder = builder.body(body.clone());
        }
        let response = builder.send().map_err(fail)?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|v| (k.as_str().to_ascii_lowercase(), v.to_owned()))
            })
            .collect();
        let body = response.text().map_err(fail)?;
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}
