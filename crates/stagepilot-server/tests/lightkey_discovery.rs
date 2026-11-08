//! Finding the Lighting computer, end to end through the HTTP routes: real loopback fake helpers
//! (healthy, refusing 403, not a helper, slow), the real probe, the real link and the real saved
//! settings. The helpers live on 127.0.0.1 ports; a routing table makes them stand in for
//! `10.30.0.x` addresses (the probe and the link both go through it), so the tests run the same on
//! every platform and nothing leaves the machine.
//!
//! The timing is shortened (first scan 100 ms after a change, then every 400 ms); the real values
//! are 5 s and 60 s and are covered by the constants' unit tests.

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::{LightingMethod, LightkeySettings, LightsSettings};
use stagepilot_core::settings::{PersistentSettings, SettingsService};
use stagepilot_lights::lightkey_discovery::{
    DiscoveryOptions, HttpProber, Probed, Prober, TargetsFn,
};
use stagepilot_lights::lightkey_link::{
    HttpFailure, LightkeyHttp, PostAnswer, ReqwestLightkeyHttp,
};
use stagepilot_osc::fake::FakeOscBackend;
use stagepilot_osc::OscBackend;
use stagepilot_server::{build_app, AppOptions, OutputBackends, Runtime};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower::ServiceExt;

const LK: &str = "/api/v1/lights/lightkey";

// ------------------------------------------------------------------------------ fake helpers

#[derive(Clone, Copy)]
enum Kind {
    /// 0.5.0 helper, Lightkey running.
    Running,
    /// 0.4.x helper (no version), Lightkey not running.
    OldNotRunning,
    /// 403 `{"error": "not available on this interface"}`.
    Refusing,
    /// 200 with a web page.
    NotAHelper,
    /// Answers after 5 s.
    Slow,
}

struct Fake {
    port: u16,
    /// Requests the fake received for anything (probe or link).
    hits: Arc<AtomicUsize>,
}

async fn fake(kind: Kind) -> Fake {
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = hits.clone();
    let router = axum::Router::new().fallback(get(move |uri: axum::http::Uri| {
        let counted = counted.clone();
        async move {
            counted.fetch_add(1, Ordering::SeqCst);
            let path = uri.path().to_string();
            let json = |status: StatusCode, body: Value| {
                (
                    status,
                    [(header::CONTENT_TYPE, "application/json")],
                    body.to_string(),
                )
                    .into_response()
            };
            match kind {
                Kind::Refusing => json(
                    StatusCode::FORBIDDEN,
                    json!({"error": "not available on this interface"}),
                ),
                Kind::NotAHelper => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/html")],
                    "<html><body>It works!</body></html>",
                )
                    .into_response(),
                Kind::Slow => {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    json(StatusCode::OK, json!({"ok": true, "lightkey": true}))
                }
                Kind::Running | Kind::OldNotRunning => match path.as_str() {
                    "/health" if matches!(kind, Kind::Running) => json(
                        StatusCode::OK,
                        json!({"ok": true, "lightkey": true, "version": "0.5.0",
                               "fixture_map": {"project": "Show.lightkeyproj", "count": 3,
                                               "read_at": 1, "map": 7}}),
                    ),
                    "/health" => json(
                        StatusCode::OK,
                        json!({"ok": true, "lightkey": false, "error": null}),
                    ),
                    "/time" => json(StatusCode::OK, json!({"t": 1000.0})),
                    _ => json(
                        StatusCode::OK,
                        json!({"events": [], "state": {"online": matches!(kind, Kind::Running),
                                                       "active": [], "seq": 1}}),
                    ),
                },
            }
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Fake { port, hits }
}

/// Which loopback port stands in for which 10.30.0.x address. An address that is not in the table
/// does not answer.
#[derive(Clone, Default)]
struct Routes(Arc<HashMap<Ipv4Addr, u16>>);

impl Routes {
    fn of(entries: &[(&str, u16)]) -> Self {
        Self(Arc::new(
            entries
                .iter()
                .map(|(ip, port)| (ip.parse().unwrap(), *port))
                .collect(),
        ))
    }

    fn ips(&self) -> Vec<Ipv4Addr> {
        let mut all: Vec<_> = self.0.keys().copied().collect();
        // Plus a few that nobody answers, so a scan has silence to deal with.
        all.extend((1..=5).map(|n| Ipv4Addr::new(10, 30, 0, n)));
        all.sort();
        all.dedup();
        all
    }
}

/// The real probe, aimed at the loopback port that stands in for the address.
struct RoutedProber {
    routes: Routes,
    real: HttpProber,
    probes: Arc<AtomicUsize>,
}

#[async_trait]
impl Prober for RoutedProber {
    async fn probe(&self, ip: Ipv4Addr, _port: u16) -> Probed {
        self.probes.fetch_add(1, Ordering::SeqCst);
        match self.routes.0.get(&ip) {
            Some(port) => self.real.probe(Ipv4Addr::LOCALHOST, *port).await,
            None => Probed::Ignore,
        }
    }
}

/// The real HTTP client of the link, aimed the same way.
struct RoutedHttp {
    routes: Routes,
    real: ReqwestLightkeyHttp,
}

impl RoutedHttp {
    fn aim(&self, url: &str) -> Result<String, HttpFailure> {
        let rest = url.strip_prefix("http://").unwrap_or(url);
        let (host_port, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = host_port.split(':').next().unwrap_or(host_port);
        let ip: Ipv4Addr = host
            .parse()
            .map_err(|_| HttpFailure::Other("no route".into()))?;
        let port = self
            .routes
            .0
            .get(&ip)
            .ok_or_else(|| HttpFailure::Other("no route".into()))?;
        Ok(format!("http://127.0.0.1:{port}/{path}"))
    }
}

#[async_trait]
impl LightkeyHttp for RoutedHttp {
    async fn get_json(&self, url: &str, query: &[(&str, String)]) -> Result<Value, HttpFailure> {
        self.real.get_json(&self.aim(url)?, query).await
    }

    async fn post_json(&self, url: &str) -> Result<PostAnswer, HttpFailure> {
        self.real.post_json(&self.aim(url)?).await
    }
}

// ------------------------------------------------------------------------------ the app

struct Rig {
    router: axum::Router,
    rt: Arc<Runtime>,
    probes: Arc<AtomicUsize>,
}

fn lights(api_url: &str, osc_host: &str) -> LightsSettings {
    LightsSettings {
        enabled: true,
        method: LightingMethod::Osc,
        osc_host: osc_host.into(),
        lightkey: LightkeySettings {
            api_url: api_url.into(),
            ..LightkeySettings::default()
        },
        ..LightsSettings::default()
    }
}

async fn rig(lights: LightsSettings, routes: &Routes) -> Rig {
    let service = SettingsService::ephemeral_persistent(PersistentSettings {
        lights,
        ..PersistentSettings::default()
    });
    let osc = Arc::new(FakeOscBackend::default());
    let probes = Arc::new(AtomicUsize::new(0));
    let targets: TargetsFn = {
        let routes = routes.clone();
        Arc::new(move |_| routes.ips())
    };
    let http_routes = routes.clone();
    let backends = OutputBackends {
        osc: Arc::new(move || osc.clone() as Arc<dyn OscBackend>),
        lightkey_http: Arc::new(move || {
            Arc::new(RoutedHttp {
                routes: http_routes.clone(),
                real: ReqwestLightkeyHttp::new(),
            })
        }),
        lightkey_discovery: DiscoveryOptions {
            enabled: true,
            settle: Duration::from_millis(100),
            every: Duration::from_millis(400),
            prober: Some(Arc::new(RoutedProber {
                routes: routes.clone(),
                real: HttpProber::new(),
                probes: probes.clone(),
            })),
            targets: Some(targets),
            persist: None,
        },
        ..OutputBackends::fake_midi()
    };
    let rt = Runtime::with_backends(Arc::new(service), false, backends);
    rt.start().await;
    Rig {
        router: build_app(rt.clone(), AppOptions { web_root: None }),
        rt,
        probes,
    }
}

async fn call(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(path);
    let mut request = match body {
        Some(b) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string())),
        None => builder.body(Body::empty()),
    }
    .unwrap();
    let peer: SocketAddr = "127.0.0.1:50000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = router.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    (
        parts.status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn status(rig: &Rig) -> Value {
    call(&rig.router, "GET", LK, None).await.1
}

/// Poll the status until `done` says so (up to 20 s, which a slow CI runner needs).
async fn until(rig: &Rig, what: &str, done: impl Fn(&Value) -> bool) -> Value {
    let started = Instant::now();
    loop {
        let now = status(rig).await;
        if done(&now) {
            return now;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}; last status: {now}"
        );
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

async fn saved(rig: &Rig) -> Value {
    call(&rig.router, "GET", "/api/v1/settings", None).await.1["settings"]["lights"].clone()
}

fn state(status: &Value) -> &str {
    status["discovery"]["state"].as_str().unwrap_or("")
}

// ------------------------------------------------------------------------------ the tests

#[tokio::test]
async fn automatic_and_one_helper_found_connects_by_itself_and_remembers_it() {
    let helper = fake(Kind::Running).await;
    let routes = Routes::of(&[("10.30.0.211", helper.port)]);
    // The Replay host is set (and not the helper), so it must be left alone and the difference said.
    let rig = rig(lights("", "10.30.0.99"), &routes).await;
    let connected = until(&rig, "the automatic connection", |s| {
        s["link"] == "connected" && s["discovery"]["message"].is_string()
    })
    .await;
    assert_eq!(connected["api_url"], "http://10.30.0.211:8770");
    assert_eq!(
        connected["discovery"]["message"],
        "Found a Lighting computer at 10.30.0.211 and connected."
    );
    assert_eq!(
        connected["discovery"]["note"],
        "Capture is from 10.30.0.211; Replay sends to 10.30.0.99."
    );
    assert_eq!(state(&connected), "connected");
    let lights = saved(&rig).await;
    assert_eq!(lights["lightkey"]["found_url"], "http://10.30.0.211:8770");
    assert!(lights["lightkey"].get("api_url").is_none() || lights["lightkey"]["api_url"] == "");
    assert_eq!(
        lights["osc_host"], "10.30.0.99",
        "a set Replay host is never changed"
    );
    rig.rt.stop().await;
}

#[tokio::test]
async fn a_blank_replay_host_is_filled_with_the_found_computer() {
    let helper = fake(Kind::OldNotRunning).await;
    let routes = Routes::of(&[("10.30.0.211", helper.port)]);
    let rig = rig(lights("", ""), &routes).await;
    let seen = until(&rig, "the automatic connection", |s| {
        s["discovery"]["message"].is_string()
    })
    .await;
    assert!(seen["discovery"].get("note").is_none(), "{seen}");
    assert_eq!(saved(&rig).await["osc_host"], "10.30.0.211");
    assert_eq!(
        saved(&rig).await["lightkey"]["found_url"],
        "http://10.30.0.211:8770"
    );
    rig.rt.stop().await;
}

#[tokio::test]
async fn a_replay_host_equal_to_the_found_computer_needs_no_note() {
    let helper = fake(Kind::Running).await;
    let routes = Routes::of(&[("10.30.0.211", helper.port)]);
    // The Replay host is the helper's own address, but the link derives 10.30.0.211:8770 anyway:
    // it connects without any search, so nothing is said.
    let rig = rig(lights("", "10.30.0.211"), &routes).await;
    let s = until(&rig, "the link", |s| s["link"] == "connected").await;
    assert_eq!(state(&s), "connected");
    assert!(s["discovery"].get("message").is_none());
    rig.rt.stop().await;
}

#[tokio::test]
async fn automatic_and_several_found_connects_to_none_until_one_is_chosen() {
    let a = fake(Kind::Running).await;
    let b = fake(Kind::OldNotRunning).await;
    let routes = Routes::of(&[("10.30.0.211", a.port), ("10.30.0.212", b.port)]);
    let rig = rig(lights("", "10.30.0.99"), &routes).await;
    let choose = until(&rig, "the list", |s| state(s) == "choose").await;
    let rows: Vec<(&str, bool)> = choose["discovery"]["found"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["address"].as_str().unwrap(),
                f["lightkey_running"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(rows, vec![("10.30.0.211", true), ("10.30.0.212", false)]);
    assert_eq!(choose["discovery"]["found"][0]["version"], "0.5.0");
    assert_eq!(
        choose["discovery"]["found"][0]["project"],
        "Show.lightkeyproj"
    );
    assert!(choose["discovery"]["found"][1].get("version").is_none());
    // It did not connect to either, and waiting changes nothing.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let still = status(&rig).await;
    assert_eq!(state(&still), "choose");
    assert_ne!(still["link"], "connected");
    assert!(saved(&rig).await["lightkey"].get("found_url").is_none());

    // The operator picks the second one.
    let (code, used) = call(
        &rig.router,
        "POST",
        &format!("{LK}/discovery/use"),
        Some(json!({"address": "10.30.0.212"})),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{used}");
    assert_eq!(used["accepted"], true);
    let s = until(&rig, "the chosen computer", |s| {
        s["link"] == "lightkey_closed"
    })
    .await;
    assert_eq!(s["api_url"], "http://10.30.0.212:8770");
    assert_eq!(state(&s), "connected");
    assert_eq!(
        s["discovery"]["message"],
        "Found a Lighting computer at 10.30.0.212 and connected."
    );
    let lights = saved(&rig).await;
    assert_eq!(lights["lightkey"]["found_url"], "http://10.30.0.212:8770");
    assert_eq!(
        lights["lightkey"]["api_url"], "",
        "Automatic stays Automatic"
    );
    rig.rt.stop().await;
}

#[tokio::test]
async fn a_down_chosen_address_is_offered_a_found_computer_and_nothing_switches_until_use() {
    let helper = fake(Kind::Running).await;
    let routes = Routes::of(&[("10.30.0.211", helper.port)]);
    let rig = rig(lights("10.30.0.50", "10.30.0.50"), &routes).await;
    let offer = until(&rig, "the offer", |s| state(s) == "offer").await;
    assert_eq!(offer["discovery"]["chosen"], "10.30.0.50");
    assert_eq!(offer["discovery"]["found"][0]["address"], "10.30.0.211");
    assert!(offer["discovery"]["chosen_down_for_s"].is_u64());
    assert!(offer["discovery"]["last_scan_at"].is_string());
    // Several scans later the chosen address is still the chosen address.
    tokio::time::sleep(Duration::from_millis(1300)).await;
    let later = status(&rig).await;
    assert_eq!(state(&later), "offer");
    assert_eq!(later["api_url"], "http://10.30.0.50:8770");
    assert_ne!(later["link"], "connected");
    assert_eq!(
        saved(&rig).await["lightkey"]["api_url"],
        "http://10.30.0.50"
    );

    let (code, used) = call(
        &rig.router,
        "POST",
        &format!("{LK}/discovery/use"),
        Some(json!({"address": "10.30.0.211"})),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{used}");
    let s = until(&rig, "the new address", |s| s["link"] == "connected").await;
    assert_eq!(s["api_url"], "http://10.30.0.211:8770");
    assert!(
        s["discovery"].get("message").is_none() && s["discovery"].get("note").is_none(),
        "a chosen address that is connected says nothing: {s}"
    );
    let lights = saved(&rig).await;
    assert_eq!(lights["lightkey"]["api_url"], "http://10.30.0.211:8770");
    assert_eq!(lights["lightkey"]["found_url"], "http://10.30.0.211:8770");
    assert_eq!(lights["osc_host"], "10.30.0.50", "the Replay host stays");
    rig.rt.stop().await;
}

#[tokio::test]
async fn keep_waiting_silences_the_offer_and_the_chosen_address_stays() {
    let helper = fake(Kind::Running).await;
    let routes = Routes::of(&[("10.30.0.211", helper.port)]);
    let rig = rig(lights("10.30.0.50", "10.30.0.50"), &routes).await;
    until(&rig, "the offer", |s| state(s) == "offer").await;
    let (code, _) = call(
        &rig.router,
        "POST",
        &format!("{LK}/discovery/dismiss"),
        Some(json!({"address": "10.30.0.211"})),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let silenced = until(&rig, "the silence", |s| state(s) == "none").await;
    assert!(silenced["discovery"]["found"]
        .as_array()
        .unwrap()
        .is_empty());
    // Two more scans find it again and stay quiet.
    let before = rig.probes.load(Ordering::SeqCst);
    until(&rig, "another scan", |_| {
        rig.probes.load(Ordering::SeqCst) >= before + 2 * routes.ips().len()
    })
    .await;
    assert_eq!(state(&status(&rig).await), "none");
    assert_eq!(
        saved(&rig).await["lightkey"]["api_url"],
        "http://10.30.0.50"
    );

    // Changing the chosen address ends the silence.
    let (code, _) = call(
        &rig.router,
        "PUT",
        &format!("{LK}/settings"),
        Some(json!({"api_url": "10.30.0.60"})),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let offer = until(&rig, "the offer again", |s| state(s) == "offer").await;
    assert_eq!(offer["discovery"]["chosen"], "10.30.0.60");
    rig.rt.stop().await;
}

#[tokio::test]
async fn nothing_found_is_an_ordinary_state_and_search_again_looks_at_once() {
    let routes = Routes::of(&[]);
    let rig = rig(lights("", "10.30.0.99"), &routes).await;
    let none = until(&rig, "the empty answer", |s| state(s) == "none").await;
    assert!(none["discovery"]["found"].as_array().unwrap().is_empty());
    assert!(none["discovery"]["refusing"].as_array().unwrap().is_empty());
    assert!(none["discovery"].get("message").is_none());
    let scans_before = rig.probes.load(Ordering::SeqCst);
    let (code, body) = call(&rig.router, "POST", &format!("{LK}/discovery/scan"), None).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["accepted"], true, "{body}");
    until(&rig, "the extra scan", |_| {
        rig.probes.load(Ordering::SeqCst) > scans_before
    })
    .await;
    rig.rt.stop().await;
}

#[tokio::test]
async fn a_refusing_helper_is_reported_and_is_not_a_found_computer() {
    let refusing = fake(Kind::Refusing).await;
    let routes = Routes::of(&[("10.30.0.211", refusing.port)]);
    let rig = rig(lights("", "10.30.0.99"), &routes).await;
    let s = until(&rig, "the refusing helper", |s| {
        s["discovery"]["refusing"]
            .as_array()
            .is_some_and(|r| !r.is_empty())
    })
    .await;
    assert_eq!(s["discovery"]["refusing"], json!(["10.30.0.211"]));
    assert_eq!(state(&s), "none");
    assert!(s["discovery"]["found"].as_array().unwrap().is_empty());
    rig.rt.stop().await;
}

#[tokio::test]
async fn a_page_that_is_not_a_helper_and_a_slow_one_are_not_found() {
    let page = fake(Kind::NotAHelper).await;
    let slow = fake(Kind::Slow).await;
    let routes = Routes::of(&[("10.30.0.213", page.port), ("10.30.0.214", slow.port)]);
    let rig = rig(lights("", "10.30.0.99"), &routes).await;
    let s = until(&rig, "the empty answer", |s| {
        s["discovery"]["last_scan_at"].is_string()
    })
    .await;
    assert_eq!(state(&s), "none");
    assert!(s["discovery"]["found"].as_array().unwrap().is_empty());
    assert!(s["discovery"]["refusing"].as_array().unwrap().is_empty());
    rig.rt.stop().await;
}

#[tokio::test]
async fn use_with_an_address_that_was_not_found_is_a_plain_422() {
    let helper = fake(Kind::Running).await;
    let routes = Routes::of(&[("10.30.0.211", helper.port)]);
    let rig = rig(lights("10.30.0.50", "10.30.0.50"), &routes).await;
    until(&rig, "the offer", |s| state(s) == "offer").await;
    for address in ["10.30.0.7", "8.8.8.8", "not an address"] {
        let (code, body) = call(
            &rig.router,
            "POST",
            &format!("{LK}/discovery/use"),
            Some(json!({"address": address})),
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{address}");
        assert!(
            body["detail"].as_str().unwrap().contains("not found"),
            "{body}"
        );
    }
    let (code, _) = call(
        &rig.router,
        "POST",
        &format!("{LK}/discovery/use"),
        Some(json!({})),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    // Nothing changed.
    assert_eq!(
        saved(&rig).await["lightkey"]["api_url"],
        "http://10.30.0.50"
    );
    rig.rt.stop().await;
}

#[tokio::test]
async fn nothing_is_searched_while_a_chosen_address_is_connected() {
    let chosen = fake(Kind::Running).await;
    let other = fake(Kind::Running).await;
    let routes = Routes::of(&[("10.30.0.211", chosen.port), ("10.30.0.212", other.port)]);
    let rig = rig(lights("10.30.0.211", "10.30.0.211"), &routes).await;
    until(&rig, "the link", |s| s["link"] == "connected").await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let s = status(&rig).await;
    assert_eq!(state(&s), "connected");
    assert!(s["discovery"].get("message").is_none() && s["discovery"].get("note").is_none());
    assert_eq!(
        rig.probes.load(Ordering::SeqCst),
        0,
        "not one probe while connected"
    );
    assert_eq!(
        other.hits.load(Ordering::SeqCst),
        0,
        "the other helper was never asked"
    );
    rig.rt.stop().await;
}

#[tokio::test]
async fn the_search_stops_the_moment_the_link_connects() {
    let helper = fake(Kind::Running).await;
    let routes = Routes::of(&[("10.30.0.211", helper.port)]);
    let rig = rig(lights("", "10.30.0.99"), &routes).await;
    until(&rig, "the link", |s| s["link"] == "connected").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after_connect = rig.probes.load(Ordering::SeqCst);
    assert!(after_connect > 0);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        rig.probes.load(Ordering::SeqCst),
        after_connect,
        "no probe after the link connected"
    );
    // Search again is refused while connected.
    let (_, body) = call(&rig.router, "POST", &format!("{LK}/discovery/scan"), None).await;
    assert_eq!(body["accepted"], false);
    rig.rt.stop().await;
}

#[tokio::test]
async fn lighting_off_or_midi_never_searches() {
    let helper = fake(Kind::Running).await;
    let routes = Routes::of(&[("10.30.0.211", helper.port)]);
    let mut off = lights("", "10.30.0.99");
    off.enabled = false;
    let mut midi = lights("", "10.30.0.99");
    midi.method = LightingMethod::Midi;
    for settings in [off, midi] {
        let rig = rig(settings, &routes).await;
        tokio::time::sleep(Duration::from_millis(1200)).await;
        let s = status(&rig).await;
        assert_eq!(state(&s), "off", "{s}");
        assert_eq!(rig.probes.load(Ordering::SeqCst), 0);
        let (_, body) = call(&rig.router, "POST", &format!("{LK}/discovery/scan"), None).await;
        assert_eq!(body["accepted"], false);
        rig.rt.stop().await;
    }
}

#[tokio::test]
async fn a_build_without_the_search_reports_it_off() {
    let routes = Routes::of(&[]);
    let service = SettingsService::ephemeral_persistent(PersistentSettings {
        lights: lights("", "10.30.0.99"),
        ..PersistentSettings::default()
    });
    let rt = Runtime::with_backends(Arc::new(service), false, OutputBackends::fake_midi());
    rt.start().await;
    let router = build_app(rt.clone(), AppOptions { web_root: None });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (_, s) = call(&router, "GET", LK, None).await;
    assert_eq!(s["discovery"]["state"], "off");
    drop(routes);
    rt.stop().await;
}

#[tokio::test]
async fn a_restart_tries_the_last_found_computer_first() {
    let helper = fake(Kind::Running).await;
    let routes = Routes::of(&[("10.30.0.211", helper.port)]);
    let mut settings = lights("", "10.30.0.99");
    settings.lightkey.found_url = "http://10.30.0.211:8770".into();
    let rig = rig(settings, &routes).await;
    // Connected from the saved address with no search at all.
    let s = until(&rig, "the link", |s| s["link"] == "connected").await;
    assert_eq!(s["api_url"], "http://10.30.0.211:8770");
    assert_eq!(rig.probes.load(Ordering::SeqCst), 0);
    rig.rt.stop().await;
}
