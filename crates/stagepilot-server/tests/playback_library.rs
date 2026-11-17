//! The optional Playback library, against a fake helper (an axum server on a loopback port): every
//! state the real helper can be in, the request rules (off means off, at most one ask per 30 s, 3 s,
//! size cap, no redirect, no waveform) and the routes. No real helper, no rig.

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::NaiveDate;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::PlaybackLibrarySettings;
use stagepilot_core::settings::{PersistentSettings, SettingsService};
use stagepilot_core::state::{ServicePlan, Song};
use stagepilot_playback::library::{LibraryPlan, LibraryState, PlanMatch};
use stagepilot_server::playback_library::{base_url, PlaybackLibrary};
use stagepilot_server::{build_app, AppOptions, OutputBackends, Runtime};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tower::ServiceExt;

#[derive(Clone, Default)]
struct Helper {
    inner: Arc<Mutex<Behaviour>>,
}

#[derive(Default)]
struct Behaviour {
    hits: Vec<String>,
    health: Option<Value>,
    /// Answer every request with this status instead (and nothing else).
    broken: Option<u16>,
    /// Wait this long before every answer.
    delay: Duration,
    /// The date the fake has a setlist for.
    setlist_date: String,
    /// The `performance_date` the fake puts in that setlist (to say another day).
    said_date: Option<String>,
    redirect: bool,
    huge_health: bool,
    items: Vec<Value>,
}

fn item(id: i64, order: u32, title: &str) -> Value {
    json!({
        "setlist_song_id": id, "order": order, "title": title, "artist": "Artist",
        "key": {"id": 3, "name": "Ab"}, "key_original": {"id": 1, "name": "C"},
        "bpm": 65.0, "bpm_original": 65.0, "length_s": 300.5, "scan_length_s": 297.25,
        "arrangement": "Default", "planning_center_song_id": null,
        "rental_expires_utc": null
    })
}

fn ok_health() -> Value {
    json!({"ok": true, "version": "0.1.1", "database": "ok", "detail": "", "songs": 12, "setlists": 3})
}

impl Helper {
    fn new() -> Self {
        let helper = Self::default();
        {
            let mut b = helper.inner.lock().unwrap();
            b.health = Some(ok_health());
            b.setlist_date = "2026-11-22".into();
            b.items = vec![
                item(11, 1, "Song One"),
                item(12, 2, "Song Two (Live)"),
                item(13, 3, "Song Three"),
            ];
        }
        helper
    }

    fn hits(&self) -> Vec<String> {
        self.inner.lock().unwrap().hits.clone()
    }

    fn set(&self, f: impl FnOnce(&mut Behaviour)) {
        f(&mut self.inner.lock().unwrap());
    }

    async fn serve(&self) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new().fallback(answer).with_state(self.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("127.0.0.1:{port}")
    }
}

async fn answer(State(helper): State<Helper>, uri: Uri) -> Response {
    let (delay, broken, redirect) = {
        let mut b = helper.inner.lock().unwrap();
        b.hits.push(uri.path().to_string());
        (b.delay, b.broken, b.redirect)
    };
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    if redirect {
        return (
            StatusCode::FOUND,
            [(header::LOCATION, "http://127.0.0.1:9/health")],
        )
            .into_response();
    }
    if let Some(status) = broken {
        return (StatusCode::from_u16(status).unwrap(), "{\"error\": \"no\"}").into_response();
    }
    let b = helper.inner.lock().unwrap();
    let path = uri.path();
    let json_ok = |v: Value| {
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            v.to_string(),
        )
            .into_response()
    };
    if path == "/health" {
        if b.huge_health {
            return (StatusCode::OK, "x".repeat(300 * 1024)).into_response();
        }
        return json_ok(b.health.clone().unwrap());
    }
    if let Some(date) = path.strip_prefix("/setlists/") {
        let setlist = |performance_date: &str| json!({"setlist_id": 1, "title": "Set For Test", "performance_date": performance_date, "items": b.items});
        if date == "latest" {
            return json_ok(
                json!({"setlist_id": 9, "title": "Newest Set", "performance_date": "2026-11-15", "items": [item(1, 1, "Other")]}),
            );
        }
        if date == b.setlist_date {
            let said = b.said_date.clone().unwrap_or_else(|| date.to_string());
            return json_ok(setlist(&said));
        }
        return (StatusCode::NOT_FOUND, "{\"error\": \"no setlist\"}").into_response();
    }
    if let Some(id) = path.strip_prefix("/songs/") {
        if id.ends_with("waveform") {
            return json_ok(json!({"peak": []}));
        }
        let sections: Vec<Value> = (0..7)
            .map(|n| json!({"order": n, "name": "Verse"}))
            .collect();
        return json_ok(
            json!({"setlist_song_id": id, "arrangement_detail": {"title": "Short", "sections": sections}}),
        );
    }
    StatusCode::NOT_FOUND.into_response()
}

fn settings_for(address: &str) -> PlaybackLibrarySettings {
    PlaybackLibrarySettings {
        enabled: true,
        address: address.to_string(),
    }
}

fn plan(date: &str, titles: &[&str]) -> ServicePlan {
    ServicePlan {
        id: "plan-1".into(),
        title: "Sunday".into(),
        date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
        service_type: "Sunday Service".into(),
        service_type_id: None,
        service_times: vec!["10:00".into()],
        duration_source: "Scheduled item length".into(),
        songs: titles
            .iter()
            .enumerate()
            .map(|(i, t)| Song::new(&format!("item-{i}"), t, Some(300), i as u32 + 1))
            .collect(),
    }
}

#[tokio::test]
async fn a_ready_helper_pairs_the_plan_and_reads_each_song_without_a_waveform() {
    let helper = Helper::new();
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let settings = settings_for(&address);

    let status = library.status(&settings, false).await;
    assert_eq!(status.state, LibraryState::Ready);
    assert_eq!(
        status.message,
        "Playback library 0.1.1 is ready: 12 songs in 3 setlists."
    );
    assert_eq!((status.songs, status.setlists), (Some(12), Some(3)));

    let loaded = plan(
        "2026-11-22",
        &["Song Three", "Song One", "Song Two", "Song Four"],
    );
    let result = library.plan(&settings, Some(&loaded), false).await;
    assert_eq!(result.state, LibraryState::Ready);
    assert_eq!(result.message, None);
    assert_eq!(result.plan_date.as_deref(), Some("2026-11-22"));
    assert_eq!(result.setlist.as_ref().map(|s| s.songs), Some(3));
    let titles: Vec<&str> = result.songs.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles, ["Song Three", "Song One", "Song Two", "Song Four"]);
    let status: Vec<PlanMatch> = result.songs.iter().map(|s| s.status).collect();
    assert_eq!(
        status,
        [
            PlanMatch::Matched,
            PlanMatch::Matched,
            PlanMatch::Matched,
            PlanMatch::Missing
        ]
    );
    let one = result.songs[1].library.as_ref().unwrap();
    assert_eq!(
        (
            one.position,
            one.key.as_deref(),
            one.key_original.as_deref()
        ),
        (1, Some("Ab"), Some("C"))
    );
    assert_eq!(
        (one.bpm, one.scan_length_s, one.sections),
        (Some(65.0), Some(297.25), Some(7))
    );
    assert_eq!(one.arrangement.as_deref(), Some("Default"));
    assert!(result.songs[3].library.is_none());

    let hits = helper.hits();
    assert!(hits.contains(&"/setlists/2026-11-22".to_string()));
    assert!(hits.iter().any(|h| h == "/songs/11") && hits.iter().any(|h| h == "/songs/13"));
    assert!(
        hits.iter()
            .all(|h| !h.contains("waveform") && !h.contains("beats")),
        "{hits:?}"
    );
}

#[tokio::test]
async fn every_state_of_the_helper_has_one_plain_sentence() {
    for (database, detail, state, needle) in [
        ("waiting", "", LibraryState::Waiting, "not ready yet"),
        (
            "denied",
            "Allow it in System Settings.",
            LibraryState::Problem,
            "Allow it in System Settings",
        ),
        (
            "missing",
            "Open Playback once.",
            LibraryState::Problem,
            "Open Playback once",
        ),
        (
            "unreadable",
            "The file is locked.",
            LibraryState::Problem,
            "The file is locked",
        ),
    ] {
        let helper = Helper::new();
        helper.set(|b| b.health = Some(json!({"ok": database == "waiting", "version": "0.1.1", "database": database, "detail": detail})));
        let address = helper.serve().await;
        let library = PlaybackLibrary::new();
        let status = library.status(&settings_for(&address), false).await;
        assert_eq!(status.state, state, "{database}");
        assert!(
            status.message.contains(needle),
            "{database}: {}",
            status.message
        );
        // A plan asks nothing more of a helper that is not ready.
        let result = library
            .plan(
                &settings_for(&address),
                Some(&plan("2026-11-22", &["Song One"])),
                false,
            )
            .await;
        assert_eq!(result.state, state, "{database}");
        assert!(result.songs.is_empty());
        assert!(
            helper.hits().iter().all(|h| h == "/health"),
            "{database}: {:?}",
            helper.hits()
        );
    }
}

#[tokio::test]
async fn a_computer_that_is_not_there_is_one_sentence() {
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let library = PlaybackLibrary::new();
    let settings = settings_for(&format!("127.0.0.1:{closed}"));
    let status = library.status(&settings, false).await;
    assert_eq!(status.state, LibraryState::Problem);
    assert_eq!(status.message, "The Playback computer did not answer.");
    let result = library
        .plan(&settings, Some(&plan("2026-11-22", &["Song One"])), false)
        .await;
    assert_eq!(result.state, LibraryState::Problem);
    assert_eq!(
        result.message.as_deref(),
        Some("The Playback computer did not answer.")
    );
}

#[tokio::test]
async fn a_helper_that_never_answers_is_given_up_on_after_about_three_seconds() {
    let helper = Helper::new();
    helper.set(|b| b.delay = Duration::from_secs(8));
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let started = Instant::now();
    let status = library.status(&settings_for(&address), false).await;
    let took = started.elapsed();
    assert_eq!(status.message, "The Playback computer did not answer.");
    assert!(
        took >= Duration::from_secs(2) && took < Duration::from_secs(6),
        "{took:?}"
    );
}

#[tokio::test]
async fn a_slow_but_answering_helper_is_still_read() {
    let helper = Helper::new();
    helper.set(|b| b.delay = Duration::from_millis(900));
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let result = library
        .plan(
            &settings_for(&address),
            Some(&plan("2026-11-22", &["Song One"])),
            false,
        )
        .await;
    assert_eq!(result.state, LibraryState::Ready);
    assert_eq!(result.songs[0].status, PlanMatch::Matched);
}

#[tokio::test]
async fn no_setlist_for_the_date_is_said_with_the_newest_and_never_filled_in_silently() {
    let helper = Helper::new();
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let result = library
        .plan(
            &settings_for(&address),
            Some(&plan("2026-11-29", &["Song One"])),
            false,
        )
        .await;
    assert_eq!(result.state, LibraryState::NoSetlist);
    assert_eq!(
        result.message.as_deref(),
        Some("Playback has no setlist for 2026-11-29; its newest is Newest Set from 2026-11-15.")
    );
    assert!(result.songs.is_empty());
    assert_eq!(
        result.newest.as_ref().map(|n| n.title.as_str()),
        Some("Newest Set")
    );
    assert!(result.setlist.is_none());
    assert!(helper.hits().contains(&"/setlists/latest".to_string()));
}

#[tokio::test]
async fn a_setlist_that_says_another_day_is_not_used() {
    let helper = Helper::new();
    helper.set(|b| b.said_date = Some("2026-11-15".into()));
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let result = library
        .plan(
            &settings_for(&address),
            Some(&plan("2026-11-22", &["Song One"])),
            false,
        )
        .await;
    assert_eq!(result.state, LibraryState::NoSetlist);
    assert!(result.message.unwrap().starts_with(
        "Playback has no setlist for 2026-11-22; its newest is Set For Test from 2026-11-15"
    ));
    assert!(result.songs.is_empty());
}

#[tokio::test]
async fn a_plan_with_no_songs_and_no_plan_ask_nothing() {
    let helper = Helper::new();
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let settings = settings_for(&address);
    let empty = library
        .plan(&settings, Some(&plan("2026-11-22", &[])), false)
        .await;
    assert_eq!((empty.state, empty.songs.len()), (LibraryState::Ready, 0));
    let none = library.plan(&settings, None, false).await;
    assert_eq!(
        (none.state, none.songs.len(), none.plan_date),
        (LibraryState::Ready, 0, None)
    );
    assert!(helper.hits().is_empty(), "{:?}", helper.hits());
}

#[tokio::test]
async fn off_means_nothing_is_asked_and_no_address_is_a_sentence() {
    let helper = Helper::new();
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let off = PlaybackLibrarySettings {
        enabled: false,
        address: address.clone(),
    };
    let status = library.status(&off, true).await;
    assert_eq!((status.enabled, status.state), (false, LibraryState::Off));
    let result = library
        .plan(&off, Some(&plan("2026-11-22", &["Song One"])), true)
        .await;
    assert_eq!(result.state, LibraryState::Off);
    assert!(result.songs.is_empty());
    let blank = PlaybackLibrarySettings {
        enabled: true,
        address: String::new(),
    };
    assert_eq!(
        library.status(&blank, false).await.message,
        "Enter the address of the Playback computer."
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(helper.hits().is_empty(), "{:?}", helper.hits());
}

#[tokio::test]
async fn windows_asking_over_and_over_cause_one_ask_per_thirty_seconds() {
    let helper = Helper::new();
    let address = helper.serve().await;
    let library = Arc::new(PlaybackLibrary::new());
    let settings = settings_for(&address);
    let loaded = plan("2026-11-22", &["Song One", "Song Two"]);
    let mut asks = Vec::new();
    for _ in 0..8 {
        let (library, settings, loaded) = (library.clone(), settings.clone(), loaded.clone());
        asks.push(tokio::spawn(async move {
            let _ = library.status(&settings, false).await;
            library.plan(&settings, Some(&loaded), false).await
        }));
    }
    for ask in asks {
        assert_eq!(ask.await.unwrap().state, LibraryState::Ready);
    }
    let hits = helper.hits();
    assert_eq!(
        hits.iter().filter(|h| *h == "/health").count(),
        1,
        "{hits:?}"
    );
    assert_eq!(
        hits.iter().filter(|h| h.starts_with("/setlists/")).count(),
        1,
        "{hits:?}"
    );
    assert_eq!(
        hits.iter().filter(|h| h.starts_with("/songs/")).count(),
        3,
        "{hits:?}"
    );
    // The Check button is a little quicker, but not instant: 2 s apart at least.
    let before = helper.hits().len();
    let _ = library.status(&settings, true).await;
    assert_eq!(helper.hits().len(), before);
    tokio::time::sleep(Duration::from_millis(2200)).await;
    let _ = library.status(&settings, true).await;
    assert_eq!(helper.hits().len(), before + 1);
}

#[tokio::test]
async fn the_last_good_answer_stands_in_for_up_to_a_minute_and_says_so() {
    let helper = Helper::new();
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let settings = settings_for(&address);
    let loaded = plan("2026-11-22", &["Song One"]);
    assert_eq!(
        library.plan(&settings, Some(&loaded), false).await.state,
        LibraryState::Ready
    );
    helper.set(|b| b.broken = Some(500));
    tokio::time::sleep(Duration::from_millis(2200)).await;
    let status = library.status(&settings, true).await;
    assert!(
        status.stale && status.state == LibraryState::Ready,
        "{status:?}"
    );
    assert!(status
        .message
        .starts_with("Playback did not answer just now"));
    let again = library.plan(&settings, Some(&loaded), true).await;
    assert_eq!(again.state, LibraryState::Ready);
    assert!(again.stale);
    assert_eq!(again.songs[0].status, PlanMatch::Matched);
    assert!(again
        .message
        .unwrap()
        .starts_with("Playback did not answer just now"));
}

#[tokio::test]
async fn a_refusal_a_redirect_and_an_oversized_or_odd_answer_are_one_sentence_each() {
    let words = |m: &str| m.to_string();
    // 403: this computer is not on the helper's list.
    let helper = Helper::new();
    helper.set(|b| b.broken = Some(403));
    let address = helper.serve().await;
    let status = PlaybackLibrary::new()
        .status(&settings_for(&address), false)
        .await;
    assert!(
        status
            .message
            .starts_with("The Playback library refused this computer"),
        "{}",
        words(&status.message)
    );
    // 405 and other answers it does not know.
    helper.set(|b| b.broken = Some(405));
    let status = PlaybackLibrary::new()
        .status(&settings_for(&address), false)
        .await;
    assert_eq!(
        status.message,
        "The Playback library answered, but not in a way StagePilot understands."
    );
    // A redirect is never followed.
    helper.set(|b| {
        b.broken = None;
        b.redirect = true;
    });
    let status = PlaybackLibrary::new()
        .status(&settings_for(&address), false)
        .await;
    assert_eq!(status.state, LibraryState::Problem);
    assert!(helper.hits().iter().all(|h| h == "/health"));
    // Larger than the cap.
    helper.set(|b| {
        b.redirect = false;
        b.huge_health = true;
    });
    let status = PlaybackLibrary::new()
        .status(&settings_for(&address), false)
        .await;
    assert_eq!(
        status.message,
        "The Playback library answered, but not in a way StagePilot understands."
    );
    // Not what a helper says.
    helper.set(|b| {
        b.huge_health = false;
        b.health = Some(json!(["a", 1, null]));
    });
    let status = PlaybackLibrary::new()
        .status(&settings_for(&address), false)
        .await;
    assert_eq!(status.state, LibraryState::Problem);
}

#[tokio::test]
async fn odd_songs_in_a_setlist_never_stop_the_rest() {
    let helper = Helper::new();
    helper.set(|b| {
        b.items = vec![
            json!(null),
            json!("x"),
            item(11, 1, "Song One"),
            json!({"title": 9}),
            json!({"title": "Song Two", "bpm": "fast", "key": 5}),
        ]
    });
    let address = helper.serve().await;
    let result = PlaybackLibrary::new()
        .plan(
            &settings_for(&address),
            Some(&plan("2026-11-22", &["Song One", "Song Two"])),
            false,
        )
        .await;
    assert_eq!(result.state, LibraryState::Ready);
    assert!(result.songs.iter().all(|s| s.status == PlanMatch::Matched));
    assert_eq!(result.songs[1].library.as_ref().unwrap().bpm, None);
}

#[test]
fn a_saved_address_gets_the_default_port_only_when_it_has_none() {
    assert_eq!(
        base_url("10.20.0.102").as_deref(),
        Some("http://10.20.0.102:8780")
    );
    assert_eq!(
        base_url("http://10.20.0.102").as_deref(),
        Some("http://10.20.0.102:8780")
    );
    assert_eq!(
        base_url("10.20.0.102:9000").as_deref(),
        Some("http://10.20.0.102:9000")
    );
    assert_eq!(
        base_url(" playback-mac.local/ ").as_deref(),
        Some("http://playback-mac.local:8780")
    );
    assert_eq!(base_url(""), None);
    assert_eq!(base_url("10.0.0.1/path"), None);
    assert_eq!(base_url("bad host"), None);
}

// ---- the routes and the setting ---------------------------------------------------------------

async fn app() -> (axum::Router, Arc<Runtime>) {
    let service = SettingsService::ephemeral_persistent(PersistentSettings::default());
    let runtime = Runtime::with_backends(Arc::new(service), false, OutputBackends::fake_midi());
    runtime.start().await;
    (
        build_app(runtime.clone(), AppOptions { web_root: None }),
        runtime,
    )
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
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:50000".parse::<SocketAddr>().unwrap(),
    ));
    let response = router.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    (
        parts.status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn the_setting_is_off_by_default_and_the_routes_then_ask_nothing() {
    let (router, runtime) = app().await;
    assert!(runtime
        .settings_service
        .snapshot()
        .playback_library
        .is_default());
    let (status, body) = call(&router, "GET", "/api/v1/playback-library/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (body["enabled"].clone(), body["state"].clone()),
        (json!(false), json!("off"))
    );
    let (_, body) = call(
        &router,
        "GET",
        "/api/v1/playback-library/plan?check=1",
        None,
    )
    .await;
    assert_eq!(
        (body["state"].clone(), body["songs"].clone()),
        (json!("off"), json!([]))
    );
    // The settings the window reads do not even mention it while it is off.
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    assert!(settings["settings"].get("playback_library").is_none());
}

#[tokio::test]
async fn turning_it_on_saves_only_that_and_checks_once() {
    let helper = Helper::new();
    let address = helper.serve().await;
    let (router, runtime) = app().await;
    let before = runtime.settings_service.snapshot();
    let (status, body) = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"enabled": true, "address": address})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], json!("ready"));
    assert_eq!(body["address"], json!(format!("http://{address}")));
    let after = runtime.settings_service.snapshot();
    assert_eq!(
        after.playback_library,
        PlaybackLibrarySettings {
            enabled: true,
            address: format!("http://{address}")
        }
    );
    let mut restored = after.clone();
    restored.playback_library = PlaybackLibrarySettings::default();
    assert_eq!(restored, before, "nothing but the one setting changed");
    assert_eq!(helper.hits(), ["/health"]);
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(
        settings["settings"]["playback_library"]["enabled"],
        json!(true)
    );
    assert_eq!(settings["restart_required"], json!(false));
}

#[tokio::test]
async fn a_bad_address_is_refused_with_the_same_words_as_the_lightkey_helper() {
    let (router, runtime) = app().await;
    let (status, body) = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"enabled": true, "address": "10.0.0.5/path"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["detail"][0]["msg"],
        json!("Enter the Playback library address as host or host:port, e.g. 10.20.0.102:8780.")
    );
    let (status, _) = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"enabled": "yes"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!([1])),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(runtime
        .settings_service
        .snapshot()
        .playback_library
        .is_default());
}

#[tokio::test]
async fn saving_the_general_settings_never_switches_the_library() {
    let (router, runtime) = app().await;
    let (_, _) = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"enabled": true, "address": "127.0.0.1:9"})),
    )
    .await;
    // An older window sends its whole copy, which has no library in it.
    let mut stale = serde_json::to_value(runtime.settings_service.snapshot()).unwrap();
    stale.as_object_mut().unwrap().remove("playback_library");
    stale["timezone"] = json!("America/Chicago");
    let (status, body) = call(&router, "PUT", "/api/v1/settings", Some(stale)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let saved = runtime.settings_service.snapshot();
    assert_eq!(saved.timezone, "America/Chicago");
    assert!(saved.playback_library.enabled);
}

#[tokio::test]
async fn the_plan_route_pairs_the_loaded_plan_and_leaves_the_playback_settings_alone() {
    let helper = Helper::new();
    let address = helper.serve().await;
    let (router, runtime) = app().await;
    let playback_before = runtime.settings_service.snapshot().playback_api;
    let _ = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"enabled": true, "address": address})),
    )
    .await;
    // The demo plan is what is loaded; its songs are not in the fake's setlist, whatever the date.
    let (status, body) = call(&router, "GET", "/api/v1/playback-library/plan", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        matches!(body["state"].as_str(), Some("ready" | "no_setlist")),
        "{body}"
    );
    assert_eq!(
        runtime.settings_service.snapshot().playback_api,
        playback_before
    );
}

#[test]
fn the_plan_json_is_the_shape_the_window_reads() {
    let value = serde_json::to_value(LibraryPlan {
        enabled: true,
        state: LibraryState::NoSetlist,
        message: None,
        plan_date: None,
        setlist: None,
        newest: None,
        songs: vec![],
        stale: false,
        checked_at: None,
    })
    .unwrap();
    assert_eq!(value["state"], json!("no_setlist"));
}

#[tokio::test]
async fn the_length_is_the_scan_length_and_a_song_without_one_has_none() {
    let helper = Helper::new();
    let mut no_grid = item(12, 2, "Song Two");
    no_grid["scan_length_s"] = Value::Null; // no beat grid; `length_s` is still 300.5
    helper.set(|b| b.items = vec![item(11, 1, "Song One"), no_grid]);
    let address = helper.serve().await;
    let result = PlaybackLibrary::new()
        .plan(
            &settings_for(&address),
            Some(&plan("2026-11-22", &["Song One", "Song Two"])),
            false,
        )
        .await;
    let one = result.songs[0].library.as_ref().unwrap();
    let two = result.songs[1].library.as_ref().unwrap();
    // 297.25 is what a scan measures (the start of the last measure); the sections' total, 300.5, is never used.
    assert_eq!(one.scan_length_s, Some(297.25));
    assert_eq!(two.scan_length_s, None);
    let text = serde_json::to_string(&result).unwrap();
    assert!(
        !text.contains("\"length_s\"") && !text.contains("300.5"),
        "{text}"
    );
}
