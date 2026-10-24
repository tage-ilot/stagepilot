#![allow(dead_code)]
//! Shared fakes: an in-memory Cloudflare API and a closure transport.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use stagepilot_remote::files::ControlConfig;
use stagepilot_remote::http::{
    HttpClient, HttpRequest, HttpResponse, HttpTransport, TransportError,
};
use uuid::Uuid;

#[derive(Default)]
pub struct CloudflareState {
    pub tunnels: Vec<Value>,
    pub records: Vec<Value>,
    pub configs: std::collections::BTreeMap<String, Value>,
    pub created: u32,
    pub dns_created: u32,
    pub writes: u32,
    pub lose_after: u32,
    pub offline: bool,
    pub bad_readback: bool,
}

#[derive(Clone, Default)]
pub struct FakeCloudflare {
    pub state: Arc<Mutex<CloudflareState>>,
}

impl HttpTransport for FakeCloudflare {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let mut s = self.state.lock().unwrap();
        if s.offline {
            return Err(TransportError("test outage".into()));
        }
        let path = request.path();
        let method = request.method.as_str();
        let data = request.json.clone().unwrap_or_else(|| json!({}));
        let result: Value;
        if path.ends_with("/cfd_tunnel") {
            if method == "GET" {
                let name = request
                    .query
                    .iter()
                    .find(|(k, _)| k == "name")
                    .map(|(_, v)| v.clone());
                result = Value::Array(
                    s.tunnels
                        .iter()
                        .filter(|t| t["name"].as_str() == name.as_deref())
                        .cloned()
                        .collect(),
                );
            } else {
                s.created += 1;
                let mut tunnel = data.clone();
                tunnel["id"] = json!(Uuid::new_v4().to_string());
                s.tunnels.push(tunnel.clone());
                result = tunnel;
            }
        } else if path.contains("/dns_records") {
            if method == "GET" {
                result = Value::Array(s.records.clone());
            } else if method == "POST" {
                s.dns_created += 1;
                let mut record = data.clone();
                record["id"] = json!(Uuid::new_v4().to_string());
                s.records.push(record.clone());
                result = record;
            } else {
                let id = path.rsplit('/').next().unwrap().to_owned();
                s.records.retain(|r| r["id"].as_str() != Some(id.as_str()));
                result = json!({});
            }
        } else if path.ends_with("/configurations") {
            let tid = path.rsplit('/').nth(1).unwrap().to_owned();
            if method == "PUT" {
                s.configs.insert(tid.clone(), data.clone());
            }
            result = if s.bad_readback {
                json!({})
            } else {
                s.configs[&tid].clone()
            };
        } else if path.ends_with("/token") {
            result = json!("test-installation-token-not-an-account-secret");
        } else if path.ends_with("/connections") {
            result = json!({});
        } else if method == "DELETE" {
            let id = path.rsplit('/').next().unwrap().to_owned();
            s.tunnels.retain(|t| t["id"].as_str() != Some(id.as_str()));
            result = json!({});
        } else {
            panic!("Unexpected provider route {path}");
        }
        if method != "GET" {
            s.writes += 1;
            if s.writes == s.lose_after {
                return Err(TransportError(
                    "test lost response after successful write".into(),
                ));
            }
        }
        Ok(HttpResponse::json_response(
            200,
            &json!({"success": true, "result": result}),
        ))
    }
}

/// A transport backed by a closure.
pub struct FnTransport<F>(pub F);

impl<F> HttpTransport for FnTransport<F>
where
    F: Fn(&HttpRequest) -> Result<HttpResponse, TransportError> + Send + Sync,
{
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        (self.0)(request)
    }
}

pub fn control_config(root: &Path) -> ControlConfig {
    ControlConfig {
        provider: "cloudflare".into(),
        account_id: "a".repeat(32),
        zone_id: "b".repeat(32),
        hostname: "remote.example.com".into(),
        api_token_file: root.join("account-token"),
        control_dir: root.join("control"),
        installation_dir: root.join("installation"),
        remote_port: 8766,
        lan_port: 8765,
    }
}

pub fn cloudflare_client(fake: &FakeCloudflare) -> HttpClient {
    HttpClient::new(
        "https://api.cloudflare.com/client/v4",
        20.0,
        Arc::new(fake.clone()),
    )
}

// ---------------------------------------------------------------- control plane fake

use stagepilot_remote::bootstrap::{
    DesktopBootstrapStore, MemoryCredentials, RemoteCredentialStore,
};
use stagepilot_remote::platform::{
    FakePortProbe, FakeSpawner, InMemoryLocker, ManualClock, Platform, SystemClock,
};
use std::sync::atomic::{AtomicU32, Ordering};

pub const ORIGIN: &str = "https://control.example.com";

pub fn bundle_payload(id: &str) -> Value {
    json!({
        "installationId": id,
        "hostname": format!("sp-{id}.remote.example.com"),
        "installationCredential": format!("spi_{id}.{}", "s".repeat(43)),
    })
}

#[derive(Default)]
pub struct ControlPlaneState {
    pub payload: Value,
    pub generation: String,
    pub revoked: bool,
    pub reject_credential: bool,
    pub fail_revoke: bool,
    pub fail_reenroll: bool,
    pub reenrolled: bool,
    pub reactivated: bool,
    pub offline: bool,
    pub enrollment_nonce: String,
    pub nonce_to_id: std::collections::BTreeMap<String, String>,
    pub credential_generation: u32,
    pub log: Vec<String>,
}

#[derive(Clone)]
pub struct FakeControlPlane {
    pub state: Arc<Mutex<ControlPlaneState>>,
}

fn pad(prefix: &str, width: usize, fill: char) -> String {
    let mut out = prefix.to_owned();
    while out.len() < width {
        out.push(fill);
    }
    out
}

impl FakeControlPlane {
    pub fn new(payload: Value) -> Self {
        Self {
            state: Arc::new(Mutex::new(ControlPlaneState {
                payload,
                ..Default::default()
            })),
        }
    }

    pub fn count(&self, suffix: &str) -> usize {
        self.state
            .lock()
            .unwrap()
            .log
            .iter()
            .filter(|p| p.ends_with(suffix))
            .count()
    }
}

impl HttpTransport for FakeControlPlane {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let mut s = self.state.lock().unwrap();
        let path = request.path();
        s.log.push(path.clone());
        if s.offline {
            return Err(TransportError("test outage".into()));
        }
        let ok = |status: u16, v: Value| Ok(HttpResponse::json_response(status, &v));
        if path.ends_with("/v1/installations/enroll") {
            let nonce = request.json.as_ref().unwrap()["nonce"]
                .as_str()
                .unwrap()
                .to_owned();
            s.enrollment_nonce = nonce.clone();
            let installation_id = match s.nonce_to_id.get(&nonce).cloned() {
                None => {
                    let id = if s.nonce_to_id.is_empty() {
                        s.payload["installationId"].as_str().unwrap().to_owned()
                    } else {
                        Uuid::new_v4().simple().to_string()
                    };
                    s.nonce_to_id.insert(nonce, id.clone());
                    id
                }
                Some(id) => {
                    if s.revoked {
                        s.revoked = false;
                        s.credential_generation += 1;
                    }
                    id
                }
            };
            let mut credential = format!("spi_{installation_id}.{}", "s".repeat(43));
            if s.credential_generation > 0 {
                credential = format!(
                    "spi_{installation_id}.{}",
                    pad(&format!("g{}", s.credential_generation), 43, 's')
                );
            }
            s.payload = json!({
                "installationId": installation_id,
                "hostname": format!("sp-{installation_id}.remote.example.com"),
                "installationCredential": credential,
            });
            return ok(201, s.payload.clone());
        }
        if s.reject_credential {
            return ok(401, json!({"error": "unauthorized"}));
        }
        let expected = format!(
            "Bearer {}",
            s.payload["installationCredential"].as_str().unwrap()
        );
        if request.header("authorization") != Some(expected.as_str()) {
            return ok(401, json!({"error": "unauthorized"}));
        }
        if path.ends_with("/provision") {
            s.generation = request.json.as_ref().unwrap()["generation"]
                .as_str()
                .unwrap()
                .to_owned();
            return ok(
                200,
                json!({
                    "installationId": s.payload["installationId"],
                    "hostname": s.payload["hostname"],
                    "generation": s.generation,
                    "phase": "provisioned",
                    "tunnelToken": "desktop-installation-cloudflared-token",
                }),
            );
        }
        if path.ends_with("/reconcile") {
            return ok(
                200,
                json!({
                    "installationId": s.payload["installationId"],
                    "hostname": s.payload["hostname"],
                    "generation": s.generation,
                    "phase": "provisioned",
                    "tunnelToken": "desktop-installation-cloudflared-token",
                }),
            );
        }
        if path.ends_with("/revoke") {
            if s.fail_revoke {
                return Err(TransportError("test outage".into()));
            }
            s.revoked = true;
            s.generation = String::new();
            return ok(
                200,
                json!({
                    "installationId": s.payload["installationId"],
                    "hostname": s.payload["hostname"],
                    "phase": "disabled",
                    "revoked": true,
                }),
            );
        }
        if path.ends_with("/disable") {
            if s.fail_revoke {
                return Err(TransportError("test outage".into()));
            }
            s.generation = String::new();
            return ok(
                200,
                json!({
                    "installationId": s.payload["installationId"],
                    "hostname": s.payload["hostname"],
                    "phase": "disabled",
                }),
            );
        }
        if path.ends_with("/reenroll") {
            if s.fail_reenroll {
                return Err(TransportError("test outage".into()));
            }
            s.reenrolled = true;
            let new_id = Uuid::new_v4().simple().to_string();
            s.payload = json!({
                "installationId": new_id,
                "hostname": format!("sp-{new_id}.remote.example.com"),
                "installationCredential": format!("spi_{new_id}.{}", "r".repeat(43)),
            });
            s.revoked = false;
            s.generation = String::new();
            return ok(201, s.payload.clone());
        }
        if path.ends_with("/reactivate") {
            s.reactivated = true;
            s.credential_generation += 1;
            let suffix = pad(&format!("a{}", s.credential_generation), 43, 'a');
            let credential = format!(
                "spi_{}.{suffix}",
                s.payload["installationId"].as_str().unwrap()
            );
            s.payload["installationCredential"] = json!(credential);
            s.revoked = false;
            s.generation = String::new();
            return ok(200, s.payload.clone());
        }
        panic!("unexpected route {path}");
    }
}

// ---------------------------------------------------------------- platform

pub struct TestPlatform {
    pub platform: Platform,
    pub inspector: Arc<stagepilot_remote::orphan::FakeInspector>,
    pub clock: Arc<ManualClock>,
    pub spawner: Arc<FakeSpawner>,
    pub ports: Arc<FakePortProbe>,
}

pub fn test_platform(http: Arc<dyn HttpTransport>) -> TestPlatform {
    let clock = Arc::new(ManualClock::new(1_000_000.0));
    let spawner = Arc::new(FakeSpawner::new());
    let ports = Arc::new(FakePortProbe::default());
    let inspector = Arc::new(stagepilot_remote::orphan::FakeInspector::default());
    let platform = Platform {
        inspector: inspector.clone(),
        clock: clock.clone(),
        locker: Arc::new(InMemoryLocker::default()),
        spawner: spawner.clone(),
        ports: ports.clone(),
        http,
    };
    TestPlatform {
        platform,
        inspector,
        clock,
        spawner,
        ports,
    }
}

pub fn real_clock_platform(http: Arc<dyn HttpTransport>) -> (Platform, Arc<FakeSpawner>) {
    let spawner = Arc::new(FakeSpawner::new());
    let platform = Platform {
        inspector: Arc::new(stagepilot_remote::orphan::FakeInspector::default()),
        clock: Arc::new(SystemClock::new()),
        locker: Arc::new(InMemoryLocker::default()),
        spawner: spawner.clone(),
        ports: Arc::new(FakePortProbe::default()),
        http,
    };
    (platform, spawner)
}

pub fn verified_store(
    path: &Path,
    credentials: Arc<dyn RemoteCredentialStore>,
) -> Arc<DesktopBootstrapStore> {
    Arc::new(
        DesktopBootstrapStore::new(
            path.to_path_buf(),
            Some(credentials),
            Some(vec![ORIGIN.to_owned()]),
        )
        .unwrap(),
    )
}

pub fn memory_credentials() -> Arc<MemoryCredentials> {
    Arc::new(MemoryCredentials::default())
}

/// Counts calls then delegates.
pub struct Counting {
    pub inner: Arc<dyn HttpTransport>,
    pub enroll: Arc<AtomicU32>,
}

impl HttpTransport for Counting {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        if request.path().ends_with("/v1/installations/enroll") {
            self.enroll.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.send(request)
    }
}

/// Make a file private (0600 on Unix; no-op elsewhere).
pub fn make_private(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = path;
}

pub fn make_mode(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
}

pub fn make_executable(path: &Path) {
    make_mode(path, 0o700);
}
