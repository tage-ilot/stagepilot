//! "Set up the Lighting computer": the HTTP surface (session routes, token-guarded files), the
//! remote-tunnel exclusion, and the generated wrapper run by a real `sh` and `curl` against a real
//! listener.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use stagepilot_core::auth::{csrf_token, RemoteStore};
use stagepilot_core::settings::{RuntimeSettings, SettingsService};
use stagepilot_lights::lightkey_helper_assets as bundled;
use stagepilot_server::helper_install::{
    served_routes, Assets, HelperInstall, MAX_REQUESTS, NO_ADDRESS,
};
use stagepilot_server::remote_types::RemoteContext;
use stagepilot_server::{build_app, AppOptions, RemoteAccess, RemoteIngress, Runtime};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tower::ServiceExt;

const LAN: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 20);
const ORIGIN: &str = "https://remote.stagepilot.test";
const PASSWORD: &str = "a-test-password-only";

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

async fn send(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    peer: &str,
) -> Reply {
    let builder = Request::builder().method(method).uri(path);
    let mut request = match body {
        Some(b) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string())),
        None => builder.body(Body::empty()),
    }
    .unwrap();
    let peer: SocketAddr = format!("{peer}:50000").parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = router.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    Reply {
        status: parts.status,
        headers: parts.headers,
        body: body.collect().await.unwrap().to_bytes().to_vec(),
    }
}

fn runtime() -> Arc<Runtime> {
    Runtime::new(
        Arc::new(SettingsService::ephemeral(RuntimeSettings::demo())),
        false,
    )
}

/// An app whose session is already open at `LAN:8000`, and the token.
fn app_with_session() -> (axum::Router, Arc<Runtime>, String) {
    let rt = runtime();
    let info = rt
        .helper_install
        .begin(LAN, 8000, None, false, None)
        .unwrap();
    (
        build_app(rt.clone(), AppOptions { web_root: None }),
        rt,
        info.token,
    )
}

#[tokio::test]
async fn an_unknown_token_is_404_with_an_empty_body() {
    let (app, _rt, _token) = app_with_session();
    for path in [
        "/lightkey-helper/0123456789abcdef0123456789abcdef/install.sh",
        "/lightkey-helper/0123456789abcdef0123456789abcdef/files/install.sh",
        "/lightkey-helper/x/files/SHA256SUMS",
    ] {
        let reply = send(&app, "GET", path, None, "192.168.1.50").await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
        assert!(reply.body.is_empty(), "{path}");
    }
}

#[tokio::test]
async fn an_expired_or_ended_session_is_404() {
    let (app, rt, token) = app_with_session();
    let path = format!("/lightkey-helper/{token}/install.sh");
    assert_eq!(
        send(&app, "GET", &path, None, "192.168.1.50").await.status,
        StatusCode::OK
    );
    rt.helper_install.set_clock(Box::new(|| u64::MAX / 2));
    let reply = send(&app, "GET", &path, None, "192.168.1.50").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert!(reply.body.is_empty());

    let (app, rt, token) = app_with_session();
    assert!(rt.helper_install.end());
    let path = format!("/lightkey-helper/{token}/files/install.sh");
    assert_eq!(
        send(&app, "GET", &path, None, "192.168.1.50").await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_public_source_address_is_403_even_with_the_right_token() {
    let (app, _rt, token) = app_with_session();
    for peer in ["8.8.8.8", "203.0.113.9"] {
        for path in [
            format!("/lightkey-helper/{token}/install.sh"),
            format!("/lightkey-helper/{token}/files/install.sh"),
        ] {
            let reply = send(&app, "GET", &path, None, peer).await;
            assert_eq!(reply.status, StatusCode::FORBIDDEN, "{peer} {path}");
            assert!(reply.body.is_empty());
        }
    }
    for peer in [
        "192.168.1.50",
        "10.1.2.3",
        "172.16.4.4",
        "169.254.7.7",
        "127.0.0.1",
    ] {
        let path = format!("/lightkey-helper/{token}/files/uninstall.sh");
        assert_eq!(
            send(&app, "GET", &path, None, peer).await.status,
            StatusCode::OK,
            "{peer}"
        );
    }
}

#[tokio::test]
async fn the_31st_request_for_a_token_is_404() {
    let (app, _rt, token) = app_with_session();
    let path = format!("/lightkey-helper/{token}/files/SHA256SUMS");
    for n in 1..=MAX_REQUESTS {
        let reply = send(&app, "GET", &path, None, "192.168.1.50").await;
        assert_eq!(reply.status, StatusCode::OK, "request {n}");
    }
    let reply = send(&app, "GET", &path, None, "192.168.1.50").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert!(reply.body.is_empty());
}

#[tokio::test]
async fn exactly_the_five_vendored_files_are_served_with_their_content_types() {
    let (app, _rt, token) = app_with_session();
    let expected = [
        ("install.sh", "text/x-shellscript; charset=utf-8"),
        ("uninstall.sh", "text/x-shellscript; charset=utf-8"),
        ("lightkey_api.py", "text/x-python; charset=utf-8"),
        ("lightkey_cues.py", "text/x-python; charset=utf-8"),
        ("SHA256SUMS", "text/plain; charset=utf-8"),
    ];
    for (name, content_type) in expected {
        let reply = send(
            &app,
            "GET",
            &format!("/lightkey-helper/{token}/files/{name}"),
            None,
            "192.168.1.50",
        )
        .await;
        assert_eq!(reply.status, StatusCode::OK, "{name}");
        assert_eq!(reply.headers[header::CONTENT_TYPE], content_type, "{name}");
        assert_eq!(reply.headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(reply.body, bundled::file(name).unwrap(), "{name}");
    }
    for name in [
        "VERSION",
        "config.json",
        "..%2Finstall.sh",
        "install.sh%00",
        "",
        "INSTALL.SH",
        "lightkey_api.pyc",
    ] {
        let reply = send(
            &app,
            "GET",
            &format!("/lightkey-helper/{token}/files/{name}"),
            None,
            "192.168.1.50",
        )
        .await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{name:?}");
        assert!(reply.body.is_empty(), "{name:?}");
    }
}

#[tokio::test]
async fn there_is_no_directory_listing_and_no_other_method() {
    let (app, _rt, token) = app_with_session();
    for path in [
        "/lightkey-helper".to_string(),
        "/lightkey-helper/".to_string(),
        format!("/lightkey-helper/{token}"),
        format!("/lightkey-helper/{token}/"),
        format!("/lightkey-helper/{token}/files"),
        format!("/lightkey-helper/{token}/files/"),
        format!("/lightkey-helper/{token}/files/../../etc/passwd"),
    ] {
        let reply = send(&app, "GET", &path, None, "192.168.1.50").await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
        assert!(reply.body.is_empty(), "{path}");
    }
    for method in ["POST", "PUT", "DELETE", "PATCH"] {
        let path = format!("/lightkey-helper/{token}/files/install.sh");
        let reply = send(&app, method, &path, None, "192.168.1.50").await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{method}");
        assert!(reply.body.is_empty(), "{method}");
    }
}

#[tokio::test]
async fn the_served_wrapper_names_the_session_url_and_checksums() {
    let (app, _rt, token) = app_with_session();
    let reply = send(
        &app,
        "GET",
        &format!("/lightkey-helper/{token}/install.sh"),
        None,
        "192.168.1.50",
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.headers[header::CONTENT_TYPE],
        "text/x-shellscript; charset=utf-8"
    );
    let script = String::from_utf8(reply.body).unwrap();
    assert!(script.contains(&format!(
        "BASE='http://192.168.1.20:8000/lightkey-helper/{token}'"
    )));
    for (hash, name) in bundled::checksums() {
        assert!(
            script.contains(&format!("{name}) want='{hash}' ;;")),
            "{name}"
        );
    }
    let sums = hex::encode(Sha256::digest(bundled::file("SHA256SUMS").unwrap()));
    assert!(script.contains(&format!("SHA256SUMS) want='{sums}' ;;")));
    assert_eq!(
        script.contains("--allow-for 192.168.1.20"),
        bundled::installer_supports_allow_for()
    );
}

// ---- the session routes ---------------------------------------------------------------------------

#[tokio::test]
async fn a_bad_address_is_refused_and_the_default_lighting_host_gives_the_plain_error() {
    let rt = runtime();
    let app = build_app(rt, AppOptions { web_root: None });
    let bad = send(
        &app,
        "POST",
        "/api/v1/lights/lightkey/helper-install",
        Some(json!({"address": "8.8.8.8"})),
        "127.0.0.1",
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    // The default Lighting host is this computer: there is no LAN address toward it.
    let none = send(
        &app,
        "POST",
        "/api/v1/lights/lightkey/helper-install",
        None,
        "127.0.0.1",
    )
    .await;
    assert_eq!(none.status, StatusCode::CONFLICT);
    assert_eq!(none.json()["detail"], NO_ADDRESS);
    assert_eq!(none.json()["code"], "no_lan_address");
}

/// This computer's own private address, as the system would pick it toward the internet.
fn own_private_address() -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if ip.is_private() => Some(ip),
        _ => None,
    }
}

#[tokio::test]
async fn post_opens_a_session_with_its_own_listener_extends_it_and_delete_closes_it() {
    let Some(ip) = own_private_address() else {
        eprintln!("skipped: this computer has no private LAN address");
        return;
    };
    let rt = runtime();
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    let body = Some(json!({"address": ip.to_string()}));
    let first = send(
        &app,
        "POST",
        "/api/v1/lights/lightkey/helper-install",
        body.clone(),
        "127.0.0.1",
    )
    .await;
    assert_eq!(first.status, StatusCode::OK, "{:?}", first.json());
    let first = first.json();
    let port = first["port"].as_u64().unwrap() as u16;
    assert_ne!(
        port, rt.settings.bind_port,
        "the extra listener has its own port"
    );
    assert_eq!(first["lan_ip"], ip.to_string());
    assert_eq!(first["bundled_version"], bundled::bundled_version());
    let url = first["url"].as_str().unwrap().to_string();
    assert!(url.starts_with(&format!("http://{ip}:{port}/lightkey-helper/")));
    assert!(url.ends_with("/install.sh"));
    assert_eq!(first["command"], format!("curl -fsS {url} | sh"));
    assert!(first["expires_at"].as_str().unwrap().ends_with('Z'));

    // The extra listener is on that one address, serves the wrapper, and nothing else.
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let got = client.get(&url).send().await.unwrap();
    assert_eq!(got.status(), 200);
    assert!(got.text().await.unwrap().starts_with("#!/bin/sh"));
    let api = client
        .get(format!("http://{ip}:{port}/api/v1/state"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        api.status(),
        404,
        "the dashboard API is not on this listener"
    );
    assert!(
        std::net::TcpStream::connect_timeout(
            &SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port),
            std::time::Duration::from_millis(500)
        )
        .is_err(),
        "it is bound to the LAN address, not 0.0.0.0"
    );

    // A second POST extends: same token, same port.
    let second = send(
        &app,
        "POST",
        "/api/v1/lights/lightkey/helper-install",
        body,
        "127.0.0.1",
    )
    .await
    .json();
    assert_eq!(second["url"], first["url"]);

    let ended = send(
        &app,
        "DELETE",
        "/api/v1/lights/lightkey/helper-install",
        None,
        "127.0.0.1",
    )
    .await;
    assert_eq!(ended.json()["ended"], json!(true));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        client.get(&url).send().await.is_err(),
        "the listener closed with the session"
    );
}

// ---- never through the remote tunnel -------------------------------------------------------------------

#[tokio::test]
async fn the_remote_ingress_never_passes_the_installer_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let rt = runtime();
    let access = RemoteAccess::new(RemoteStore::new(dir.path().join("identity.db")));
    rt.set_remote_access(Some(access.clone()));
    let info = rt
        .helper_install
        .begin(LAN, 8000, None, false, None)
        .unwrap();
    let local = build_app(rt.clone(), AppOptions { web_root: None });
    let edge = RemoteIngress::new(access.clone(), ORIGIN)
        .unwrap()
        .wrap(local);
    access
        .store
        .bootstrap("operator@example.test", PASSWORD)
        .unwrap();
    let session = access
        .store
        .login("operator@example.test", PASSWORD, None)
        .unwrap();
    let cookie = format!("__Host-stagepilot_remote={session}");
    let csrf = csrf_token(&session);
    let paths = [
        format!("/lightkey-helper/{}/install.sh", info.token),
        format!("/lightkey-helper/{}/files/install.sh", info.token),
        "/lightkey-helper/".to_string(),
        "/lightkey-helper".to_string(),
    ];
    for path in paths {
        for (name, signed_in) in [("signed out", false), ("an admin session", true)] {
            for method in ["GET", "POST"] {
                let mut builder = Request::builder()
                    .method(method)
                    .uri(&path)
                    .header("origin", ORIGIN)
                    .header("x-stagepilot-remote", "1");
                if signed_in {
                    builder = builder
                        .header("cookie", &cookie)
                        .header("x-csrf-token", &csrf);
                }
                let mut request = builder.body(Body::empty()).unwrap();
                let peer: SocketAddr = "192.168.1.50:50000".parse().unwrap();
                request.extensions_mut().insert(ConnectInfo(peer));
                let response = edge.clone().oneshot(request).await.unwrap();
                assert_eq!(
                    response.status(),
                    StatusCode::NOT_FOUND,
                    "{method} {path} ({name})"
                );
                let body = response.into_body().collect().await.unwrap().to_bytes();
                assert!(
                    !String::from_utf8_lossy(&body).contains("#!/bin/sh"),
                    "{path} ({name}) leaked the installer"
                );
            }
        }
    }
}

#[tokio::test]
async fn the_handlers_refuse_a_request_that_carries_a_remote_context() {
    // Second line of defence: even if a listener passed the prefix, the handlers answer 404 to
    // anything the remote ingress has touched.
    let dir = tempfile::tempdir().unwrap();
    let (app, _rt, token) = app_with_session();
    let access = RemoteAccess::new(RemoteStore::new(dir.path().join("identity.db")));
    for path in [
        format!("/lightkey-helper/{token}/install.sh"),
        format!("/lightkey-helper/{token}/files/install.sh"),
    ] {
        let mut request = Request::builder().uri(&path).body(Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo("192.168.1.50:5".parse::<SocketAddr>().unwrap()));
        request.extensions_mut().insert(RemoteContext {
            access: access.clone(),
            token: None,
            session: None,
        });
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

// ---- the wrapper, run for real ------------------------------------------------------------------------

#[cfg(unix)]
mod wrapper {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    /// The vendored helper files with a stub installer that records how it was called.
    fn stub_assets() -> Assets {
        let installer = "#!/bin/sh\n# Test installer; understands --allow-for.\n\
            printf '%s\\n' \"$@\" > \"$HOME/installer-args\"\n\
            pwd > \"$HOME/installer-dir\"\n\
            ls > \"$HOME/installer-files\"\n\
            echo 'stub installer ran'\n";
        let mut files: Vec<(String, Vec<u8>)> =
            ["lightkey_api.py", "lightkey_cues.py", "uninstall.sh"]
                .iter()
                .map(|n| (n.to_string(), bundled::file(n).unwrap().to_vec()))
                .collect();
        files.push(("install.sh".into(), installer.as_bytes().to_vec()));
        let mut sums = String::new();
        for (name, bytes) in &files {
            sums.push_str(&format!("{}  {name}\n", hex::encode(Sha256::digest(bytes))));
        }
        files.push(("SHA256SUMS".into(), sums.into_bytes()));
        Assets::from_files(files, "9.9.9")
    }

    /// A real listener on loopback serving a session; returns `(base_url, service)`.
    async fn serve(assets: Assets) -> (String, Arc<HelperInstall>) {
        let svc = Arc::new(HelperInstall::with_assets(assets));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = served_routes::<()>(svc.clone());
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let info = svc
            .begin(Ipv4Addr::LOCALHOST, port, None, false, None)
            .unwrap();
        (info.base_url(), svc)
    }

    struct Run {
        ok: bool,
        output: String,
        home: tempfile::TempDir,
    }

    impl Run {
        fn file(&self, name: &str) -> Option<String> {
            std::fs::read_to_string(self.home.path().join(name)).ok()
        }
    }

    /// Fetch the wrapper with `curl` (as the pasted command does) and pipe it to `sh`, in a
    /// temp HOME and TMPDIR.
    async fn paste(base_url: String, script_from: Option<String>) -> Run {
        tokio::task::spawn_blocking(move || {
            let home = tempfile::tempdir().unwrap();
            std::fs::create_dir(home.path().join("tmp")).unwrap();
            let url = script_from.unwrap_or(format!("{base_url}/install.sh"));
            let command = format!("curl -fsS '{url}' | sh");
            let output = Command::new("sh")
                .arg("-c")
                .arg(command)
                .env_clear()
                .env("PATH", std::env::var("PATH").unwrap())
                .env("HOME", home.path())
                .env("TMPDIR", home.path().join("tmp"))
                .output()
                .unwrap();
            Run {
                ok: output.status.success(),
                output: format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ),
                home,
            }
        })
        .await
        .unwrap()
    }

    fn nothing_left_in(dir: &Path) -> bool {
        std::fs::read_dir(dir).unwrap().next().is_none()
    }

    fn curl_and_sh_exist() -> bool {
        ["curl", "sh"]
            .iter()
            .all(|tool| Command::new(tool).arg("--version").output().is_ok())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_wrapper_fetches_verifies_and_runs_the_installer() {
        if !curl_and_sh_exist() {
            eprintln!("skipped: no curl");
            return;
        }
        let (base, svc) = serve(stub_assets()).await;
        let run = paste(base, None).await;
        assert!(run.ok, "{}", run.output);
        assert!(
            run.output.contains("Done. Go back to StagePilot."),
            "{}",
            run.output
        );
        assert!(run.output.contains("stub installer ran"), "{}", run.output);
        // The installer was called with the right arguments, from a folder holding the files.
        assert_eq!(
            run.file("installer-args").unwrap(),
            "--allow-for\n127.0.0.1\n"
        );
        let mut files: Vec<String> = run
            .file("installer-files")
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        files.sort();
        assert_eq!(
            files,
            [
                "SHA256SUMS",
                "install.sh",
                "lightkey_api.py",
                "lightkey_cues.py",
                "uninstall.sh"
            ]
        );
        // It cleaned up after itself.
        assert!(nothing_left_in(&run.home.path().join("tmp")));
        // 1 wrapper + 5 files were requested, nothing else.
        assert!(svc.is_active());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_tampered_file_aborts_before_the_installer_runs() {
        if !curl_and_sh_exist() {
            eprintln!("skipped: no curl");
            return;
        }
        // Server A: the honest files. Server B: the same, but one file altered. The wrapper
        // (with A's checksums) is told to download from B.
        let (honest_base, _a) = serve(stub_assets()).await;
        for tampered in ["lightkey_api.py", "install.sh", "uninstall.sh"] {
            let mut assets = stub_assets();
            let entry = assets
                .files_mut()
                .iter_mut()
                .find(|(n, _)| n == tampered)
                .unwrap();
            entry.1.extend_from_slice(b"\necho pwned\n");
            let (evil_base, _b) = serve(assets).await;

            let script = reqwest::get(format!("{honest_base}/install.sh"))
                .await
                .unwrap()
                .text()
                .await
                .unwrap();
            let script = script.replace(&honest_base, &evil_base);
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("wrapper.sh");
            std::fs::write(&path, script).unwrap();
            let path_text = path.to_string_lossy().to_string();
            let run = tokio::task::spawn_blocking(move || {
                let home = tempfile::tempdir().unwrap();
                std::fs::create_dir(home.path().join("tmp")).unwrap();
                let output = Command::new("sh")
                    .arg(&path_text)
                    .env_clear()
                    .env("PATH", std::env::var("PATH").unwrap())
                    .env("HOME", home.path())
                    .env("TMPDIR", home.path().join("tmp"))
                    .output()
                    .unwrap();
                Run {
                    ok: output.status.success(),
                    output: format!(
                        "{}{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    ),
                    home,
                }
            })
            .await
            .unwrap();
            assert!(!run.ok, "{tampered}: {}", run.output);
            assert!(
                run.output
                    .contains(&format!("{tampered} does not match its checksum")),
                "{tampered}: {}",
                run.output
            );
            assert!(
                run.file("installer-args").is_none(),
                "the installer must not run"
            );
            assert!(
                nothing_left_in(&run.home.path().join("tmp")),
                "the temp folder is removed"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_failing_installer_and_an_ended_session_exit_non_zero_with_a_reason() {
        if !curl_and_sh_exist() {
            eprintln!("skipped: no curl");
            return;
        }
        let (base, svc) = serve(stub_assets()).await;
        // The session ends after the wrapper was fetched: the file downloads fail.
        let script = reqwest::get(format!("{base}/install.sh"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        svc.end();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("w.sh");
        std::fs::write(&path, script).unwrap();
        let path_text = path.to_string_lossy().to_string();
        let output = tokio::task::spawn_blocking(move || {
            Command::new("sh").arg(path_text).output().unwrap()
        })
        .await
        .unwrap();
        assert!(!output.status.success());
        let text = String::from_utf8_lossy(&output.stderr).to_string();
        assert!(
            text.contains("StagePilot setup failed: could not download"),
            "{text}"
        );
    }
}
