//! VerseVision as an always-on, in-process add-on: `versevision-web`'s router answers at `/versevision/` on StagePilot's own port,
//! behind the same PIN, Origin and remote rules as the rest of the app (see `docs/versevision.md`).
//!
//! What StagePilot gives it, and what it keeps for itself:
//! - **Files** live in `<StagePilot's settings folder>/versevision/{config,data,logs,keys}`; `keys/` is owner-only (the key store makes
//!   its file `0600` and, on Unix, the folder `0700`). Nothing is created until the page is first opened.
//! - **ProPresenter's address** is the Presentation connection, read again for every piece of work (the transport factory below and the
//!   theme picture), never copied into `config.toml`. The files are the ones of THIS computer (`LocalTransport`).
//! - **The StagePilot installation** (for the shared Bible service) is the one the always-on check-in enrols: the credential is read from
//!   the credential store when needed, kept in memory only, and never printed, logged or written anywhere else.
//! - **Start at login** is off: VerseVision runs whenever StagePilot runs.
//!
//! Remote access: the remote listener answers `404` for this prefix (see `remote_ingress`), and so does this handler for any request that
//! carries a remote context. The page's own scripts cannot do the remote CSRF handshake, and "apply" writes to the ProPresenter library
//! of the computer StagePilot runs on, so VerseVision is for the computer itself and the local network (behind the PIN), never more open.

use crate::remote_types::RemoteContext;
use crate::runtime::Runtime;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;
use versevision_bible::{FileKeyStore, InstallationAuth, ScriptureCache, Secret};
use versevision_core::config::AppConfig;
use versevision_pp::clock::{Clock, SystemClock};
use versevision_pp::dirs::{Dirs, DirsEnv};
use versevision_pp::process::SystemProbe;
use versevision_pp::transport::LocalTransport;
use versevision_web::{mount, Autostart, TransportFactory, WebState};

/// Where the page is served.
pub const PREFIX: &str = "/versevision";

/// Is this request for VerseVision (the page, its files or its API)?
pub fn is_versevision_path(path: &str) -> bool {
    path == PREFIX || path.starts_with("/versevision/")
}

/// The one VerseVision of a running StagePilot: built when the page is first asked for.
pub struct VerseVision {
    rt: Arc<Runtime>,
    built: Mutex<Option<Router>>,
    /// The folder of an in-memory instance (demo, tests): removed when StagePilot stops.
    scratch: Mutex<Option<tempfile::TempDir>>,
}

impl VerseVision {
    pub fn new(rt: Arc<Runtime>) -> Arc<Self> {
        Arc::new(Self {
            rt,
            built: Mutex::new(None),
            scratch: Mutex::new(None),
        })
    }

    /// `<settings folder>/versevision`, or a scratch folder while the settings are in memory.
    fn root(&self) -> std::io::Result<PathBuf> {
        if let Some(path) = self.rt.settings_service.settings_path() {
            let beside = path.parent().map(Path::to_path_buf).unwrap_or_default();
            return Ok(beside.join("versevision"));
        }
        let mut scratch = self.scratch.lock().unwrap_or_else(|p| p.into_inner());
        if scratch.is_none() {
            *scratch = Some(tempfile::Builder::new().prefix("versevision-").tempdir()?);
        }
        Ok(scratch
            .as_ref()
            .map(|d| d.path().to_path_buf())
            .unwrap_or_default())
    }

    fn router(&self) -> Result<Router, String> {
        let mut built = self.built.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(router) = built.as_ref() {
            return Ok(router.clone());
        }
        let router = self.build().map_err(|e| e.to_string())?;
        *built = Some(router.clone());
        Ok(router)
    }

    fn build(&self) -> Result<Router, Box<dyn std::error::Error>> {
        let root = self.root()?;
        let dirs = Dirs {
            config: root.join("config"),
            data: root.join("data"),
            logs: root.join("logs"),
            keys: root.join("keys"),
        };
        for dir in [&dirs.config, &dirs.data, &dirs.logs] {
            std::fs::create_dir_all(dir)?;
        }
        create_private_dir(&dirs.keys)?;
        let config = load_config(&dirs.config_file())?;
        let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
        let home = DirsEnv::from_process().home;
        let rt = self.rt.clone();
        let transport: TransportFactory = {
            let clock = clock.clone();
            Arc::new(move |config: &AppConfig| {
                // The Presentation connection as it is NOW: a change made on the Presentation page is used by the next piece of work.
                let mut propresenter = config.propresenter.clone();
                let (host, port) = presentation_address(&rt);
                propresenter.host = host;
                propresenter.port = port;
                Box::new(LocalTransport::new(
                    &propresenter,
                    &config.node,
                    clock.clone(),
                    Arc::new(SystemProbe),
                    home.clone(),
                ))
            })
        };
        let state = WebState::builder(dirs.clone(), config)
            .app_version(stagepilot_core::settings::APP_VERSION)
            .clock(clock)
            .transport(transport)
            .keys(Arc::new(FileKeyStore::new(dirs.keys_file())))
            .cache(Arc::new(ScriptureCache::new(
                dirs.data.join("scripture-cache"),
            )))
            .installation_auth(Arc::new(Installation::new(self.rt.clone())))
            .autostart(Arc::new(WithStagePilot))
            .build();
        Ok(mount(PREFIX, state))
    }

    /// Answer one request for `/versevision...`.
    pub async fn handle(self: Arc<Self>, request: Request) -> Response {
        if request.extensions().get::<RemoteContext>().is_some() {
            return plain_json(StatusCode::NOT_FOUND, json!({"detail": "Not Found"}));
        }
        if request.method() == axum::http::Method::GET
            && request.uri().path() == "/versevision/api/themethumb"
        {
            return self.theme_thumbnail(request.uri().query()).await;
        }
        let router = match self.router() {
            Ok(router) => router,
            Err(message) => {
                tracing::warn!(error = %message, "versevision_unavailable");
                return plain_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({"detail": format!("VerseVision could not start: {message}")}),
                );
            }
        };
        router
            .oneshot(request)
            .await
            .unwrap_or_else(|never| match never {})
    }

    /// `GET /api/themethumb` of the page, answered here so the picture comes from the CURRENT Presentation connection: the page's own
    /// handler would use the address held in VerseVision's `config.toml`, which StagePilot does not copy the connection into.
    async fn theme_thumbnail(&self, query: Option<&str>) -> Response {
        let mut theme = String::new();
        let mut slide = 1_i64;
        for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
            match key.as_ref() {
                "theme" => theme = value.into_owned(),
                "slide" if !value.is_empty() => slide = value.trim().parse().unwrap_or(1),
                _ => {}
            }
        }
        let (host, port) = presentation_address(&self.rt);
        let picture =
            tokio::task::spawn_blocking(move || fetch_thumbnail(&host, port, &theme, slide))
                .await
                .ok()
                .flatten();
        let mut response = Response::new(match &picture {
            Some(bytes) => Body::from(bytes.clone()),
            None => Body::empty(),
        });
        let (status, kind) = if picture.is_some() {
            (StatusCode::OK, "image/jpeg")
        } else {
            (StatusCode::NOT_FOUND, "text/plain")
        };
        *response.status_mut() = status;
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(kind));
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}

/// The saved Presentation connection (what the Presentation page edits and the timer output uses).
fn presentation_address(rt: &Runtime) -> (String, u16) {
    let saved = rt.settings_service.effective_snapshot().propresenter;
    (saved.host, saved.port)
}

fn load_config(file: &Path) -> Result<AppConfig, String> {
    if !file.exists() {
        return Ok(AppConfig::default());
    }
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    AppConfig::from_toml_str(&text).map_err(|e| {
        format!(
            "VerseVision's settings file {} is not valid: {e}",
            file.display()
        )
    })
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

fn fetch_thumbnail(host: &str, port: u16, theme: &str, number: i64) -> Option<Vec<u8>> {
    const SAFE: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'_')
        .remove(b'.')
        .remove(b'-')
        .remove(b'~');
    let url = format!(
        "http://{host}:{port}/v1/theme/{}/slides/{}/thumbnail?quality=200",
        utf8_percent_encode(theme, SAFE),
        (number - 1).max(0)
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(3)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent.get(&url).call().ok()?;
    if response.status().as_u16() != 200 {
        return None;
    }
    response.body_mut().read_to_vec().ok()
}

fn plain_json(status: StatusCode, body: Value) -> Response {
    let mut response = (status, body.to_string()).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

/// The answer of the PIN guard for a VerseVision request without a signed-in session: a short page for a browser tab (the Presentation
/// button opens one), plain JSON for the page's own requests.
pub fn pin_required(headers: &HeaderMap) -> Response {
    let wants_page = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|accept| accept.contains("text/html"));
    if !wants_page {
        return plain_json(
            StatusCode::UNAUTHORIZED,
            json!({"detail": "Dashboard PIN required."}),
        );
    }
    let page = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>VerseVision</title>\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><meta name=\"color-scheme\" content=\"dark light\"></head>\
<body style=\"font-family:system-ui,sans-serif;max-width:32rem;margin:4rem auto;padding:0 1rem;line-height:1.5\">\
<h1 style=\"font-size:1.25rem\">Enter the StagePilot PIN first</h1>\
<p>VerseVision opens after you sign in to StagePilot in this browser. <a href=\"/\">Open StagePilot</a>, enter the dashboard PIN, then open VerseVision again.</p>\
</body></html>";
    let mut response = (StatusCode::UNAUTHORIZED, page).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Start at login belongs to StagePilot: VerseVision runs whenever it does, so the page offers nothing to switch.
struct WithStagePilot;

impl Autostart for WithStagePilot {
    fn status(&self) -> Result<Value, String> {
        Ok(json!({
            "supported": false,
            "enabled": false,
            "platform": std::env::consts::OS,
            "detail": "VerseVision runs whenever StagePilot runs",
        }))
    }

    fn enable(&self, _host: &str, _port: u16) -> Result<Value, String> {
        Err("VerseVision runs whenever StagePilot runs, so there is nothing to switch on".into())
    }

    fn disable(&self) -> Result<Value, String> {
        Err("VerseVision runs whenever StagePilot runs, so there is nothing to switch off".into())
    }
}

/// The installation StagePilot enrols with the control plane (the always-on check-in), as the Bible service wants it. Read from the
/// same bootstrap store the diagnostics upload uses, whenever it is asked: no enrolment is `None` (VerseVision then needs its own keys).
pub struct Installation {
    rt: Arc<Runtime>,
    /// The credential of the enrolment last read, in memory only (reading the credential store can wait on a system dialog).
    held: Mutex<Option<(String, Secret)>>,
}

impl Installation {
    pub fn new(rt: Arc<Runtime>) -> Self {
        Self {
            rt,
            held: Mutex::new(None),
        }
    }

    fn enrolment(
        &self,
    ) -> Option<(
        Arc<dyn crate::remote_types::DiagnosticsBootstrap>,
        crate::remote_types::ActiveEnrollment,
    )> {
        let bootstrap = self.rt.remote_wiring().manager?.bootstrap()?;
        let active = bootstrap.active().ok()??;
        // Only ever the control plane StagePilot itself trusts.
        bootstrap
            .trusted_origins()
            .contains(&active.control_plane_origin)
            .then_some((bootstrap, active))
    }
}

impl InstallationAuth for Installation {
    fn origin(&self) -> String {
        self.enrolment()
            .map(|(_, active)| active.control_plane_origin)
            .unwrap_or_default()
    }

    fn installation_id(&self) -> Option<String> {
        self.enrolment().map(|(_, active)| active.installation_id)
    }

    fn credential(&self) -> Option<Secret> {
        let (bootstrap, active) = self.enrolment()?;
        if let Some((id, secret)) = self.held.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            if *id == active.installation_id {
                return Some(secret.clone());
            }
        }
        // A keychain read can wait; do not hold a runtime worker while it does.
        let read = move || {
            bootstrap
                .credential(&active)
                .map(|c| (active.installation_id, c))
        };
        let found = match tokio::runtime::Handle::try_current() {
            Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(read)
            }
            _ => read(),
        };
        let (id, credential) = found.ok().filter(|(_, c)| !c.is_empty())?;
        let secret = Secret::new(credential);
        *self.held.lock().unwrap_or_else(|p| p.into_inner()) = Some((id, secret.clone()));
        Some(secret)
    }
}
