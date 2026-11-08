//! The real probe against real loopback servers: every kind of answer a computer on the network
//! can give to `GET /health`, and what the probe makes of it. Nothing leaves 127.0.0.1.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use stagepilot_lights::lightkey_discovery::{
    scan, HelperAnswer, HttpProber, Probed, Prober, MAX_BODY_BYTES,
};
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

async fn serve(handler: Router) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, handler).await.unwrap();
    });
    port
}

fn json(status: StatusCode, body: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

async fn probe(router: Router) -> Probed {
    let port = serve(router).await;
    HttpProber::new().probe(Ipv4Addr::LOCALHOST, port).await
}

fn fixed(status: StatusCode, body: &'static str) -> Router {
    Router::new().fallback(move || async move { json(status, body) })
}

#[tokio::test]
async fn a_0_4_helper_and_a_0_5_helper_are_found() {
    let old = probe(fixed(
        StatusCode::OK,
        r#"{"ok": true, "lightkey": true, "error": null, "dmx_age_ms": 12, "reads": 3}"#,
    ))
    .await;
    assert_eq!(
        old,
        Probed::Helper(HelperAnswer {
            version: None,
            lightkey_running: true,
            project: None
        })
    );
    let new = probe(fixed(
        StatusCode::OK,
        r#"{"ok": true, "lightkey": false, "version": "0.5.0",
            "fixture_map": {"project": "Show.lightkeyproj", "count": 66, "read_at": 1, "map": 2}}"#,
    ))
    .await;
    assert_eq!(
        new,
        Probed::Helper(HelperAnswer {
            version: Some("0.5.0".into()),
            lightkey_running: false,
            project: Some("Show.lightkeyproj".into())
        })
    );
}

#[tokio::test]
async fn only_the_health_page_is_asked_and_only_with_get() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = seen.clone();
    let router = Router::new().fallback(move |method: axum::http::Method, uri: Uri| {
        let log = log.clone();
        async move {
            log.lock().unwrap().push(format!("{method} {uri}"));
            json(StatusCode::OK, r#"{"ok": true, "lightkey": true}"#)
        }
    });
    let found = probe(router).await;
    assert!(matches!(found, Probed::Helper(_)));
    assert_eq!(*seen.lock().unwrap(), vec!["GET /health".to_string()]);
}

#[tokio::test]
async fn a_403_with_the_helpers_words_is_a_refusing_helper() {
    let refusing = probe(fixed(
        StatusCode::FORBIDDEN,
        r#"{"error": "not available on this interface"}"#,
    ))
    .await;
    assert_eq!(refusing, Probed::Refusing);
    let other = probe(fixed(StatusCode::FORBIDDEN, r#"{"error": "go away"}"#)).await;
    assert_eq!(other, Probed::Ignore);
}

#[tokio::test]
async fn things_that_are_not_a_helper_are_ignored() {
    for (status, body) in [
        (StatusCode::OK, "<html><body>It works!</body></html>"),
        (StatusCode::OK, "{\"ok\": true, \"lightkey\": tru"),
        (StatusCode::OK, "[]"),
        (StatusCode::OK, r#"{"ok": true}"#),
        (StatusCode::OK, r#"{"ok": false, "lightkey": true}"#),
        (StatusCode::NOT_FOUND, r#"{"ok": true, "lightkey": true}"#),
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"ok": true, "lightkey": true}"#,
        ),
        (
            StatusCode::UNAUTHORIZED,
            r#"{"error": "not available on this interface"}"#,
        ),
    ] {
        assert_eq!(
            probe(fixed(status, body)).await,
            Probed::Ignore,
            "{status} {body}"
        );
    }
}

#[tokio::test]
async fn a_redirect_is_not_followed_and_is_ignored() {
    let target = serve(fixed(StatusCode::OK, r#"{"ok": true, "lightkey": true}"#)).await;
    let hops = Arc::new(AtomicUsize::new(0));
    let counted = hops.clone();
    let router = Router::new().fallback(move || {
        let counted = counted.clone();
        async move {
            counted.fetch_add(1, Ordering::SeqCst);
            (
                StatusCode::FOUND,
                [(
                    header::LOCATION,
                    format!("http://127.0.0.1:{target}/health"),
                )],
            )
                .into_response()
        }
    });
    assert_eq!(probe(router).await, Probed::Ignore);
    assert_eq!(
        hops.load(Ordering::SeqCst),
        1,
        "one attempt, no second request"
    );
}

#[tokio::test]
async fn a_body_over_64_kb_is_ignored_even_when_it_starts_like_a_helper() {
    let mut body = String::from(r#"{"ok": true, "lightkey": true, "pad": ""#);
    body.push_str(&"x".repeat(MAX_BODY_BYTES + 100));
    body.push_str("\"}");
    let router = Router::new().fallback(move || {
        let body = body.clone();
        async move { json(StatusCode::OK, &body) }
    });
    assert_eq!(probe(router).await, Probed::Ignore);
}

#[tokio::test]
async fn a_helper_that_answers_slowly_is_given_up_on_after_600_ms() {
    let router = Router::new().fallback(|| async {
        tokio::time::sleep(Duration::from_secs(3)).await;
        json(StatusCode::OK, r#"{"ok": true, "lightkey": true}"#)
    });
    let port = serve(router).await;
    let started = Instant::now();
    let answer = HttpProber::new().probe(Ipv4Addr::LOCALHOST, port).await;
    let took = started.elapsed();
    assert_eq!(answer, Probed::Ignore);
    assert!(took >= Duration::from_millis(500), "{took:?}");
    assert!(took < Duration::from_millis(2500), "{took:?}");
}

#[tokio::test]
async fn a_body_that_stalls_part_way_is_given_up_on_too() {
    // Headers at once, then the body never finishes.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        use std::io::Write;
        for socket in listener.incoming() {
            let Ok(mut socket) = socket else { return };
            std::thread::spawn(move || {
                let _ = socket.write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 500\r\n\r\n{\"ok\": true,",
                );
                std::thread::sleep(Duration::from_secs(5));
            });
        }
    });
    let started = Instant::now();
    let answer = HttpProber::new().probe(Ipv4Addr::LOCALHOST, port).await;
    assert_eq!(answer, Probed::Ignore);
    assert!(started.elapsed() < Duration::from_millis(2500));
}

#[tokio::test]
async fn nothing_listening_is_ignored() {
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let answer = HttpProber::new().probe(Ipv4Addr::LOCALHOST, port).await;
    assert_eq!(answer, Probed::Ignore);
}

#[tokio::test]
async fn a_scan_over_the_real_probe_finds_the_helper_and_the_refusing_one() {
    let healthy = serve(fixed(StatusCode::OK, r#"{"ok": true, "lightkey": true}"#)).await;
    // The same IP and port can only be one server; the scan asks one address, so ask twice with
    // two ports through two scans.
    let flag: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(|| true);
    let found = scan(
        Arc::new(HttpProber::new()),
        vec![Ipv4Addr::LOCALHOST],
        healthy,
        flag.clone(),
    )
    .await;
    assert_eq!(found.found.len(), 1);
    assert_eq!(found.found[0].address(), format!("127.0.0.1:{healthy}"));
    assert!(found.refusing.is_empty());
    let refusing = serve(fixed(
        StatusCode::FORBIDDEN,
        r#"{"error": "not available on this interface"}"#,
    ))
    .await;
    let found = scan(
        Arc::new(HttpProber::new()),
        vec![Ipv4Addr::LOCALHOST],
        refusing,
        flag,
    )
    .await;
    assert!(found.found.is_empty());
    assert_eq!(found.refusing, vec![Ipv4Addr::LOCALHOST]);
}
