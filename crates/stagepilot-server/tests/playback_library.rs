//! The optional Playback library, against a fake helper (an axum server on a loopback port): every
//! state the real helper can be in, the request rules (off means off, at most one ask per 30 s, 3 s,
//! size cap, no redirect, no waveform) and the routes. No real helper, no rig.

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::NaiveDate;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::PlaybackLibrarySettings;
use stagepilot_core::settings::{PersistentSettings, SettingsService};
use stagepilot_core::state::{ServicePlan, Song};
use stagepilot_playback::library::{LengthSource, LibraryPlan, LibraryState, PlanMatch};
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
    /// What `/songs/<id>` says about `arrangement_detail.sections` (default: seven timed sections).
    sections: Option<Value>,
    /// Song ids whose own answer is an error.
    broken_songs: Vec<i64>,
    /// The cover the fake serves for every song (`None`: 404), and how.
    art: Option<Art>,
    /// What each cover request carried as `If-None-Match`.
    art_asks: Vec<(i64, Option<String>)>,
}

#[derive(Clone)]
struct Art {
    content_type: String,
    etag: String,
    bytes: Vec<u8>,
}

fn jpeg(len: usize) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes.resize(len, 7);
    bytes
}

fn png(len: usize) -> Vec<u8> {
    let mut bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.resize(len, 9);
    bytes
}

fn art(content_type: &str, etag: &str, bytes: Vec<u8>) -> Art {
    Art {
        content_type: content_type.into(),
        etag: etag.into(),
        bytes,
    }
}

/// What a song item says about its cover (`artwork`), the way helper 0.2 does.
fn artwork_info(id: i64, content_type: &str, etag: &str, bytes: usize) -> Value {
    json!({"path": format!("/songs/{id}/artwork"), "type": content_type, "bytes": bytes, "etag": etag})
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

    fn count(&self, prefix: &str) -> usize {
        self.hits().iter().filter(|h| h.starts_with(prefix)).count()
    }

    fn art_asks(&self) -> Vec<(i64, Option<String>)> {
        self.inner.lock().unwrap().art_asks.clone()
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

async fn answer(State(helper): State<Helper>, uri: Uri, headers: HeaderMap) -> Response {
    let (delay, broken, redirect) = {
        let mut b = helper.inner.lock().unwrap();
        b.hits.push(uri.path().to_string());
        if let Some(id) = uri
            .path()
            .strip_prefix("/songs/")
            .and_then(|r| r.strip_suffix("/artwork"))
            .and_then(|r| r.parse::<i64>().ok())
        {
            let inm = headers
                .get(header::IF_NONE_MATCH)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            b.art_asks.push((id, inm));
        }
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
        if let Some(number) = id.strip_suffix("/artwork") {
            let Some(art) = b.art.clone() else {
                return (StatusCode::NOT_FOUND, "{\"error\": \"no picture\"}").into_response();
            };
            let _ = number;
            if headers
                .get(header::IF_NONE_MATCH)
                .and_then(|v| v.to_str().ok())
                == Some(art.etag.as_str())
            {
                return (StatusCode::NOT_MODIFIED, [(header::ETAG, art.etag.clone())])
                    .into_response();
            }
            return (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, art.content_type.clone()),
                    (header::ETAG, art.etag.clone()),
                    (header::CACHE_CONTROL, "private, max-age=86400".to_string()),
                ],
                art.bytes,
            )
                .into_response();
        }
        if id.parse::<i64>().is_ok_and(|n| b.broken_songs.contains(&n)) {
            return (StatusCode::INTERNAL_SERVER_ERROR, "{\"error\": \"no\"}").into_response();
        }
        let sections = b.sections.clone().unwrap_or_else(|| {
            Value::Array(
                (0..7)
                    .map(|n| json!({"order": n, "name": "Verse", "performed_start": n as f64 * 10.0, "performed_end": n as f64 * 10.0 + 10.0}))
                    .collect(),
            )
        });
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
        (one.bpm, one.scan_length_s, one.section_count),
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
async fn the_countdown_length_is_the_last_measure_unless_no_grid_or_too_long_before_the_end_else_none(
) {
    let helper = Helper::new();
    let mut no_grid = item(12, 2, "Song Two"); // `length_s` 300.5, no beat grid
    no_grid["scan_length_s"] = Value::Null;
    let mut neither = item(13, 3, "Song Three");
    neither["scan_length_s"] = Value::Null;
    neither["length_s"] = Value::Null;
    let mut junk_end = item(14, 4, "Song Four");
    junk_end["scan_length_s"] = Value::Null;
    junk_end["length_s"] = json!("300.5");
    let mut junk_scan = item(15, 5, "Song Five");
    junk_scan["scan_length_s"] = json!(-3);
    let mut long_last = item(16, 6, "Song Six");
    long_last["scan_length_s"] = json!(290.0);
    helper.set(|b| {
        b.items = vec![
            item(11, 1, "Song One"),
            no_grid,
            neither,
            junk_end,
            junk_scan,
            long_last,
        ];
    });
    let address = helper.serve().await;
    let result = PlaybackLibrary::new()
        .plan(
            &settings_for(&address),
            Some(&plan(
                "2026-11-22",
                &[
                    "Song One",
                    "Song Two",
                    "Song Three",
                    "Song Four",
                    "Song Five",
                    "Song Six",
                ],
            )),
            false,
        )
        .await;
    let got: Vec<(Option<f64>, Option<LengthSource>)> = result
        .songs
        .iter()
        .map(|s| {
            let l = s.library.as_ref().unwrap();
            (l.countdown_length_s, l.length_source)
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (Some(297.25), Some(LengthSource::LastMeasure)), // the start of the last measure (a scan measures this)
            (Some(300.5), Some(LengthSource::SongEndNoGrid)), // no beat grid: the end of the song
            (None, None),                                    // neither
            (None, None),                                    // a junk end of the song is none
            (Some(300.5), Some(LengthSource::SongEndNoGrid)), // a junk last-measure start falls back to the end
            (Some(300.5), Some(LengthSource::SongEndLongLastMeasure)), // the last measure starts 10.5 s before the end
        ]
    );
    // The saved scan result and the other numbers are untouched: `scan_length_s` stays what the helper said.
    let first = result.songs[0].library.as_ref().unwrap();
    assert_eq!(first.scan_length_s, Some(297.25));
    assert_eq!(
        result.songs[1].library.as_ref().unwrap().scan_length_s,
        None
    );
}

// ---- sections and cover info in the plan (F-P1c) ---------------------------------------------

#[tokio::test]
async fn the_plan_carries_each_songs_sections_and_cover_description_but_never_the_picture() {
    let helper = Helper::new();
    helper.set(|b| {
        let mut one = item(11, 1, "Song One");
        one["artwork"] = artwork_info(11, "image/jpeg", "\"aaaa1111bbbb2222\"", 4096);
        b.items = vec![one, item(12, 2, "Song Two")];
        b.sections = Some(json!([
            {"order": 1, "name": "Count Off", "performed_start": 0.0, "performed_end": 3.5},
            {"order": 2, "name": "  Verse 1  ", "performed_start": 3.5, "performed_end": 20.0},
            {"order": 3, "name": "Broken", "performed_start": 30.0, "performed_end": 25.0},
            {"order": 4, "name": "Chorus", "performed_start": 20.0, "performed_end": 44.25},
        ]));
    });
    let address = helper.serve().await;
    let (router, runtime) = app().await;
    runtime
        .store
        .mutate(|s| s.plan = Some(plan("2026-11-22", &["Song One", "Song Two"])));
    let _ = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"enabled": true, "address": address})),
    )
    .await;
    let (status, body) = call(&router, "GET", "/api/v1/playback-library/plan", None).await;
    assert_eq!(status, StatusCode::OK);
    let one = &body["songs"][0]["library"];
    assert_eq!(
        one["artwork"],
        json!({"type": "image/jpeg", "bytes": 4096, "etag": "\"aaaa1111bbbb2222\""})
    );
    assert_eq!(one["section_count"], json!(4));
    assert_eq!(
        one["sections"],
        json!([
            {"name": "Count Off", "start": 0.0, "end": 3.5},
            {"name": "Verse 1", "start": 3.5, "end": 20.0},
            {"name": "Chorus", "start": 20.0, "end": 44.25},
        ])
    );
    // A song with no cover says null, and the plan never holds a picture.
    assert_eq!(body["songs"][1]["library"]["artwork"], Value::Null);
    assert!(!body.to_string().contains("path"), "{body}");
}

#[tokio::test]
async fn a_song_with_no_usable_section_has_an_empty_list_and_an_old_helper_has_no_cover() {
    let helper = Helper::new();
    helper.set(|b| b.sections = Some(json!([{"name": "x"}, null, {"performed_start": 1}])));
    let address = helper.serve().await;
    let result = PlaybackLibrary::new()
        .plan(
            &settings_for(&address),
            Some(&plan("2026-11-22", &["Song One"])),
            false,
        )
        .await;
    let one = result.songs[0].library.as_ref().unwrap();
    assert!(one.sections.is_empty());
    assert_eq!(one.section_count, Some(3));
    assert!(one.artwork.is_none());
}

// ---- the cheap change check (F-P1c) -----------------------------------------------------------

fn with_revision(token: Option<&str>) -> Value {
    let mut health = ok_health();
    if let Some(token) = token {
        health["library_revision"] = json!(token);
    }
    health
}

async fn ask(
    library: &PlaybackLibrary,
    settings: &PlaybackLibrarySettings,
    titles: &[&str],
) -> LibraryPlan {
    // Past the 30 s between two asks, without waiting.
    library.advance_clock(Duration::from_secs(31));
    library
        .plan(settings, Some(&plan("2026-11-22", titles)), false)
        .await
}

#[tokio::test]
async fn an_unchanged_revision_costs_one_health_request_per_check() {
    let helper = Helper::new();
    helper.set(|b| b.health = Some(with_revision(Some("aaaaaaaaaaaa"))));
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let settings = settings_for(&address);
    let titles = ["Song One", "Song Two", "Song Three"];
    let first = ask(&library, &settings, &titles).await;
    assert_eq!(first.state, LibraryState::Ready);
    assert_eq!(
        (
            helper.count("/health"),
            helper.count("/setlists/"),
            helper.count("/songs/")
        ),
        (1, 1, 3)
    );
    for round in 1..=3 {
        let again = ask(&library, &settings, &titles).await;
        assert_eq!(again.state, LibraryState::Ready);
        assert!(!again.stale);
        // What the window sees is the same, and it is as new as the check.
        assert_eq!(again.songs, first.songs);
        assert_eq!(helper.hits().len(), 5 + round, "{:?}", helper.hits());
    }
    assert_eq!(
        (
            helper.count("/health"),
            helper.count("/setlists/"),
            helper.count("/songs/")
        ),
        (4, 1, 3)
    );
}

#[tokio::test]
async fn a_new_revision_reads_everything_again_and_a_flip_back_does_too() {
    let helper = Helper::new();
    helper.set(|b| b.health = Some(with_revision(Some("aaaaaaaaaaaa"))));
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let settings = settings_for(&address);
    let titles = ["Song One", "Song Two", "Song Three"];
    let _ = ask(&library, &settings, &titles).await;
    // Playback changed the key of song one.
    helper.set(|b| {
        b.health = Some(with_revision(Some("bbbbbbbbbbbb")));
        let mut changed = item(11, 1, "Song One");
        changed["key"] = json!({"id": 5, "name": "D"});
        b.items = vec![changed, item(12, 2, "Song Two"), item(13, 3, "Song Three")];
    });
    let after = ask(&library, &settings, &titles).await;
    assert_eq!(
        after.songs[0].library.as_ref().unwrap().key.as_deref(),
        Some("D")
    );
    assert_eq!(
        (
            helper.count("/health"),
            helper.count("/setlists/"),
            helper.count("/songs/")
        ),
        (2, 2, 6)
    );
    // Back to the first token: still a change, so still a full read.
    helper.set(|b| b.health = Some(with_revision(Some("aaaaaaaaaaaa"))));
    let _ = ask(&library, &settings, &titles).await;
    assert_eq!(
        (
            helper.count("/health"),
            helper.count("/setlists/"),
            helper.count("/songs/")
        ),
        (3, 3, 9)
    );
    // And the same token again costs one request.
    let _ = ask(&library, &settings, &titles).await;
    assert_eq!(helper.count("/setlists/"), 3);
    assert_eq!(helper.count("/health"), 4);
}

#[tokio::test]
async fn a_helper_without_a_revision_is_read_in_full_every_time() {
    let helper = Helper::new(); // helper 0.1.x: no `library_revision`
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let settings = settings_for(&address);
    let titles = ["Song One", "Song Two", "Song Three"];
    for round in 1..=3 {
        let result = ask(&library, &settings, &titles).await;
        assert_eq!(result.state, LibraryState::Ready);
        assert_eq!(helper.count("/setlists/"), round);
        assert_eq!(helper.count("/songs/"), 3 * round);
        assert!(result
            .songs
            .iter()
            .all(|s| s.library.as_ref().unwrap().artwork.is_none()));
    }
}

#[tokio::test]
async fn a_helper_that_restarts_with_the_same_revision_is_not_read_again() {
    let helper = Helper::new();
    helper.set(|b| b.health = Some(with_revision(Some("aaaaaaaaaaaa"))));
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let settings = settings_for(&address);
    let titles = ["Song One", "Song Two", "Song Three"];
    let _ = ask(&library, &settings, &titles).await;
    // After a restart the helper has a new start time and version, and the same token.
    helper.set(|b| {
        let mut health = with_revision(Some("aaaaaaaaaaaa"));
        health["version"] = json!("0.2.1");
        b.health = Some(health);
    });
    let after = ask(&library, &settings, &titles).await;
    assert_eq!(after.state, LibraryState::Ready);
    assert_eq!(helper.count("/setlists/"), 1);
    assert_eq!(helper.count("/songs/"), 3);
    assert_eq!(helper.count("/health"), 2);
}

#[tokio::test]
async fn ten_minutes_after_the_last_full_read_it_is_done_again_even_with_the_same_revision() {
    let helper = Helper::new();
    helper.set(|b| b.health = Some(with_revision(Some("aaaaaaaaaaaa"))));
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let settings = settings_for(&address);
    let titles = ["Song One", "Song Two", "Song Three"];
    let _ = ask(&library, &settings, &titles).await;
    // 9 minutes later (a check every few minutes keeps asking only /health).
    for _ in 0..4 {
        library.advance_clock(Duration::from_secs(135));
        let _ = library
            .plan(&settings, Some(&plan("2026-11-22", &titles)), false)
            .await;
    }
    assert_eq!(helper.count("/setlists/"), 1, "{:?}", helper.hits());
    // Just past ten minutes since the full read.
    library.advance_clock(Duration::from_secs(70));
    let result = library
        .plan(&settings, Some(&plan("2026-11-22", &titles)), false)
        .await;
    assert_eq!(result.state, LibraryState::Ready);
    assert_eq!(helper.count("/setlists/"), 2, "{:?}", helper.hits());
    assert_eq!(helper.count("/songs/"), 6);
    // The clock for the next ten minutes starts again.
    library.advance_clock(Duration::from_secs(120));
    let _ = library
        .plan(&settings, Some(&plan("2026-11-22", &titles)), false)
        .await;
    assert_eq!(helper.count("/setlists/"), 2);
}

#[tokio::test]
async fn a_paired_song_without_its_data_is_read_again_without_waiting_for_a_new_revision() {
    let helper = Helper::new();
    helper.set(|b| {
        b.health = Some(with_revision(Some("aaaaaaaaaaaa")));
        b.broken_songs = vec![12];
    });
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let settings = settings_for(&address);
    let titles = ["Song One", "Song Two", "Song Three"];
    let first = ask(&library, &settings, &titles).await;
    assert_eq!(first.songs[1].library.as_ref().unwrap().section_count, None);
    // The same token, but song two has no data: the next check reads again.
    let second = ask(&library, &settings, &titles).await;
    assert_eq!(helper.count("/setlists/"), 2);
    assert_eq!(
        second.songs[1].library.as_ref().unwrap().section_count,
        None
    );
    // It recovers: read once more, and then it is quiet.
    helper.set(|b| b.broken_songs.clear());
    let third = ask(&library, &settings, &titles).await;
    assert_eq!(
        third.songs[1].library.as_ref().unwrap().section_count,
        Some(7)
    );
    let before = helper.hits().len();
    let _ = ask(&library, &settings, &titles).await;
    assert_eq!(helper.hits().len(), before + 1);
}

#[tokio::test]
async fn a_different_plan_or_a_different_address_reads_everything_again() {
    let helper = Helper::new();
    helper.set(|b| b.health = Some(with_revision(Some("aaaaaaaaaaaa"))));
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let settings = settings_for(&address);
    let _ = ask(&library, &settings, &["Song One", "Song Two"]).await;
    let _ = ask(&library, &settings, &["Song One", "Song Two"]).await;
    assert_eq!(helper.count("/setlists/"), 1);
    // The plan lost a song.
    let _ = ask(&library, &settings, &["Song One"]).await;
    assert_eq!(helper.count("/setlists/"), 2);
    // The plan moved to another day (the fake has no setlist for it).
    library.advance_clock(Duration::from_secs(31));
    let other_day = library
        .plan(&settings, Some(&plan("2026-11-29", &["Song One"])), false)
        .await;
    assert_eq!(other_day.state, LibraryState::NoSetlist);
    assert_eq!(helper.count("/setlists/2026-11-29"), 1);
    // The address was changed (same helper, written another way).
    let renamed = settings_for(&address.replace("127.0.0.1", "localhost"));
    library.advance_clock(Duration::from_secs(31));
    let _ = library
        .plan(&renamed, Some(&plan("2026-11-22", &["Song One"])), false)
        .await;
    assert_eq!(helper.count("/setlists/2026-11-22"), 3);
    // Switched off and on again: nothing is remembered.
    let off = PlaybackLibrarySettings {
        enabled: false,
        ..renamed.clone()
    };
    let _ = library.status(&off, false).await;
    let hits = helper.hits().len();
    library
        .plan(&renamed, Some(&plan("2026-11-22", &["Song One"])), false)
        .await;
    assert_eq!(helper.count("/setlists/2026-11-22"), 4);
    assert!(helper.hits().len() > hits);
}

#[tokio::test]
async fn the_revision_check_keeps_the_throttle_and_the_old_answer_when_health_fails() {
    let helper = Helper::new();
    helper.set(|b| b.health = Some(with_revision(Some("aaaaaaaaaaaa"))));
    let address = helper.serve().await;
    let library = Arc::new(PlaybackLibrary::new());
    let settings = settings_for(&address);
    let loaded = plan("2026-11-22", &["Song One", "Song Two"]);
    // Eight windows at once: one /health, one full read.
    let mut asks = Vec::new();
    for _ in 0..8 {
        let (library, settings, loaded) = (library.clone(), settings.clone(), loaded.clone());
        asks.push(tokio::spawn(async move {
            library.plan(&settings, Some(&loaded), false).await
        }));
    }
    for ask in asks {
        assert_eq!(ask.await.unwrap().state, LibraryState::Ready);
    }
    assert_eq!(helper.count("/health"), 1);
    assert_eq!(helper.count("/setlists/"), 1);
    // The helper stops answering: the last good answer stands in, marked old, and nothing but /health is tried.
    helper.set(|b| b.broken = Some(500));
    library.advance_clock(Duration::from_secs(31));
    let old = library.plan(&settings, Some(&loaded), false).await;
    assert_eq!(old.state, LibraryState::Ready);
    assert!(old.stale);
    assert_eq!(old.songs[0].status, PlanMatch::Matched);
    assert_eq!(helper.count("/setlists/"), 1);
    assert_eq!(helper.count("/songs/"), 3);
}

// ---- the cover route (F-P1c) ------------------------------------------------------------------

const ETAG_A: &str = "\"0123456789abcdef\"";

async fn raw(
    router: &axum::Router,
    path: &str,
    if_none_match: Option<&str>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut builder = Request::builder().method("GET").uri(path);
    if let Some(tag) = if_none_match {
        builder = builder.header(header::IF_NONE_MATCH, tag);
    }
    let mut request = builder.body(Body::empty()).unwrap();
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:50000".parse::<SocketAddr>().unwrap(),
    ));
    let response = router.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    (
        parts.status,
        parts.headers,
        body.collect().await.unwrap().to_bytes().to_vec(),
    )
}

/// A fake helper whose songs 11, 12 and 13 all say they have a cover, a window with the setting on and
/// a plan holding those three songs (and whatever else `extra` adds).
async fn covers(art: Option<Art>) -> (Helper, axum::Router, Arc<Runtime>) {
    let helper = Helper::new();
    helper.set(|b| {
        b.items = (11..=13)
            .map(|id| {
                let mut song = item(
                    id,
                    id as u32 - 10,
                    ["Song One", "Song Two", "Song Three"][id as usize - 11],
                );
                song["artwork"] = artwork_info(id, "image/jpeg", ETAG_A, 4096);
                song
            })
            .collect();
        b.art = art;
    });
    let address = helper.serve().await;
    let (router, runtime) = app().await;
    runtime
        .store
        .mutate(|s| s.plan = Some(plan("2026-11-22", &["Song One", "Song Two", "Song Three"])));
    let (status, _) = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"enabled": true, "address": address})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    (helper, router, runtime)
}

const COVER_ONE: &str = "/api/v1/playback-library/songs/11/artwork";

#[tokio::test]
async fn a_jpeg_cover_reaches_the_window_with_its_etag_and_the_cache_headers() {
    let picture = jpeg(4096);
    let (helper, router, _) = covers(Some(art("image/jpeg", ETAG_A, picture.clone()))).await;
    let (status, headers, body) = raw(&router, COVER_ONE, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, picture);
    let head = |name: header::HeaderName| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    assert_eq!(head(header::CONTENT_TYPE).as_deref(), Some("image/jpeg"));
    assert_eq!(head(header::ETAG).as_deref(), Some(ETAG_A));
    assert_eq!(
        head(header::CACHE_CONTROL).as_deref(),
        Some("private, max-age=3600")
    );
    assert_eq!(
        head(header::X_CONTENT_TYPE_OPTIONS).as_deref(),
        Some("nosniff")
    );
    // The helper was asked for the picture of that song, once, with nothing held yet.
    assert_eq!(helper.art_asks(), [(11, None)]);
    // The next ask is answered from memory.
    let (status, _, again) = raw(&router, COVER_ONE, None).await;
    assert_eq!((status, again), (StatusCode::OK, picture));
    assert_eq!(helper.art_asks().len(), 1);
}

#[tokio::test]
async fn a_png_cover_is_served_as_a_png() {
    let picture = png(900);
    let (_, router, _) = covers(Some(art("image/png", ETAG_A, picture.clone()))).await;
    let (status, headers, body) = raw(&router, COVER_ONE, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, picture);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    // A type with parameters and another case still counts.
    let (_, router, _) = covers(Some(art("IMAGE/JPEG; charset=binary", ETAG_A, jpeg(300)))).await;
    assert_eq!(raw(&router, COVER_ONE, None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn the_window_gets_304_when_it_already_holds_the_picture() {
    let (helper, router, _) = covers(Some(art("image/jpeg", ETAG_A, jpeg(4096)))).await;
    // It names the etag the plan gave it: nothing is fetched, nothing is sent.
    let (status, headers, body) = raw(&router, COVER_ONE, Some(ETAG_A)).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(body.is_empty());
    assert_eq!(headers[header::ETAG], ETAG_A);
    assert_eq!(headers[header::CACHE_CONTROL], "private, max-age=3600");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert!(helper.art_asks().is_empty());
    // Weak form and a list count; another etag gets the picture.
    assert_eq!(
        raw(
            &router,
            COVER_ONE,
            Some("W/\"zzz\", W/\"0123456789abcdef\"")
        )
        .await
        .0,
        StatusCode::NOT_MODIFIED
    );
    let (status, _, body) = raw(&router, COVER_ONE, Some("\"old\"")).await;
    assert_eq!((status, body.len()), (StatusCode::OK, 4096));
    // And once held in memory the same rule applies.
    assert_eq!(
        raw(&router, COVER_ONE, Some("*")).await.0,
        StatusCode::NOT_MODIFIED
    );
}

#[tokio::test]
async fn a_song_the_helper_has_no_picture_for_is_a_404() {
    let (helper, router, _) = covers(None).await; // the plan says it has one, the helper says 404
    let (status, _, body) = raw(&router, COVER_ONE, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!body.starts_with(&[0xFF, 0xD8]));
    assert_eq!(helper.art_asks().len(), 1);
    // A song the plan says has none is not even asked for.
    let helper = Helper::new();
    let address = helper.serve().await;
    let (router, runtime) = app().await;
    runtime
        .store
        .mutate(|s| s.plan = Some(plan("2026-11-22", &["Song One"])));
    let _ = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"enabled": true, "address": address})),
    )
    .await;
    assert_eq!(raw(&router, COVER_ONE, None).await.0, StatusCode::NOT_FOUND);
    assert!(helper.art_asks().is_empty());
}

#[tokio::test]
async fn what_is_not_a_picture_is_not_served() {
    for (what, answer) in [
        ("a web page", art("text/html", ETAG_A, jpeg(500))),
        ("a gif", art("image/gif", ETAG_A, b"GIF89a....".to_vec())),
        (
            "jpeg type, other bytes",
            art("image/jpeg", ETAG_A, b"<html>nope</html>".to_vec()),
        ),
        ("jpeg type, png bytes", art("image/jpeg", ETAG_A, png(500))),
        ("png type, jpeg bytes", art("image/png", ETAG_A, jpeg(500))),
        (
            "png type, cut signature",
            art("image/png", ETAG_A, vec![0x89, 0x50, 0x4E, 0x47]),
        ),
        ("nothing", art("image/jpeg", ETAG_A, vec![])),
    ] {
        let (helper, router, runtime) = covers(Some(answer)).await;
        let (status, _, body) = raw(&router, COVER_ONE, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{what}");
        assert!(body.len() < 200, "{what}");
        assert_eq!(runtime.playback_library.artwork_held(), (0, 0), "{what}");
        assert_eq!(helper.art_asks().len(), 1, "{what}");
    }
}

#[tokio::test]
async fn a_picture_over_two_million_bytes_is_refused_and_exactly_that_many_is_fine() {
    let (_, router, runtime) = covers(Some(art("image/jpeg", ETAG_A, jpeg(2_000_001)))).await;
    assert_eq!(raw(&router, COVER_ONE, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(runtime.playback_library.artwork_held(), (0, 0));
    let (_, router, runtime) = covers(Some(art("image/jpeg", ETAG_A, jpeg(2_000_000)))).await;
    let (status, _, body) = raw(&router, COVER_ONE, None).await;
    assert_eq!((status, body.len()), (StatusCode::OK, 2_000_000));
    assert_eq!(runtime.playback_library.artwork_held(), (1, 2_000_000));
}

#[tokio::test]
async fn a_redirect_is_not_followed_and_a_slow_helper_is_given_up_on() {
    let (helper, router, _) = covers(Some(art("image/jpeg", ETAG_A, jpeg(300)))).await;
    // Read the plan once so that the cover route does not need to ask for it again.
    let (status, _) = call(&router, "GET", "/api/v1/playback-library/plan", None).await;
    assert_eq!(status, StatusCode::OK);
    helper.set(|b| b.redirect = true);
    let (status, _, _) = raw(&router, COVER_ONE, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    helper.set(|b| {
        b.redirect = false;
        b.delay = Duration::from_secs(8);
    });
    let started = Instant::now();
    let (status, _, _) = raw(&router, "/api/v1/playback-library/songs/12/artwork", None).await;
    let took = started.elapsed();
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        took >= Duration::from_secs(2) && took < Duration::from_secs(6),
        "{took:?}"
    );
}

#[tokio::test]
async fn a_304_from_the_helper_keeps_the_held_picture() {
    let picture = jpeg(4096);
    let (helper, router, runtime) = covers(Some(art("image/jpeg", ETAG_A, picture.clone()))).await;
    assert_eq!(raw(&router, COVER_ONE, None).await.0, StatusCode::OK);
    assert_eq!(helper.art_asks(), [(11, None)]);
    // Eleven minutes later the held copy is checked, with the etag it holds.
    runtime
        .playback_library
        .advance_clock(Duration::from_secs(11 * 60));
    let (status, headers, body) = raw(&router, COVER_ONE, None).await;
    assert_eq!((status, body), (StatusCode::OK, picture.clone()));
    assert_eq!(headers[header::ETAG], ETAG_A);
    assert_eq!(
        helper.art_asks(),
        [(11, None), (11, Some(ETAG_A.to_string()))]
    );
    assert_eq!(runtime.playback_library.artwork_held(), (1, 4096));
    // The check counts: no further ask for another ten minutes.
    assert_eq!(raw(&router, COVER_ONE, None).await.0, StatusCode::OK);
    assert_eq!(helper.art_asks().len(), 2);
}

#[tokio::test]
async fn a_changed_picture_replaces_the_held_one() {
    let (helper, router, runtime) = covers(Some(art("image/jpeg", ETAG_A, jpeg(1000)))).await;
    assert_eq!(raw(&router, COVER_ONE, None).await.0, StatusCode::OK);
    // Playback's picture changed; the helper's new setlist item says so.
    let second = "\"fedcba9876543210\"";
    helper.set(|b| {
        b.art = Some(art("image/png", second, png(2000)));
        b.health = Some(with_revision(Some("bbbbbbbbbbbb")));
        b.items = (11..=13)
            .map(|id| {
                let mut song = item(
                    id,
                    id as u32 - 10,
                    ["Song One", "Song Two", "Song Three"][id as usize - 11],
                );
                song["artwork"] = artwork_info(id, "image/png", second, 2000);
                song
            })
            .collect();
    });
    runtime
        .playback_library
        .advance_clock(Duration::from_secs(31));
    let (status, headers, body) = raw(&router, COVER_ONE, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.len(), 2000);
    assert_eq!(headers[header::ETAG], second);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    // The helper was asked with the etag that was held.
    assert_eq!(helper.art_asks().last().unwrap().1.as_deref(), Some(ETAG_A));
    assert_eq!(runtime.playback_library.artwork_held(), (1, 2000));
}

#[tokio::test]
async fn only_a_paired_song_of_the_current_plan_has_a_cover_route() {
    let (helper, router, _) = covers(Some(art("image/jpeg", ETAG_A, jpeg(300)))).await;
    for path in [
        "/api/v1/playback-library/songs/abc/artwork",
        "/api/v1/playback-library/songs/-11/artwork",
        "/api/v1/playback-library/songs/11.0/artwork",
        "/api/v1/playback-library/songs/1e1/artwork",
        "/api/v1/playback-library/songs/%2e%2e%2fhealth/artwork",
        "/api/v1/playback-library/songs/11%2f..%2f..%2fhealth/artwork",
        "/api/v1/playback-library/songs/99999999999999999999999/artwork",
        "/api/v1/playback-library/songs/0/artwork",
        "/api/v1/playback-library/songs/10/artwork",
        "/api/v1/playback-library/songs/14/artwork",
    ] {
        assert_eq!(
            raw(&router, path, None).await.0,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    assert!(helper.art_asks().is_empty(), "{:?}", helper.art_asks());
    // A song that is in Playback's setlist but not in the plan on screen is not answered either.
    let (helper, router, runtime) = covers(Some(art("image/jpeg", ETAG_A, jpeg(300)))).await;
    runtime
        .store
        .mutate(|s| s.plan = Some(plan("2026-11-22", &["Song One"])));
    assert_eq!(raw(&router, COVER_ONE, None).await.0, StatusCode::OK);
    assert_eq!(
        raw(&router, "/api/v1/playback-library/songs/12/artwork", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(helper.art_asks().len(), 1);
    // A plan song Playback does not have: no id, no route.
    runtime
        .store
        .mutate(|s| s.plan = Some(plan("2026-11-22", &["Song Nine"])));
    runtime
        .playback_library
        .advance_clock(Duration::from_secs(31));
    assert_eq!(raw(&router, COVER_ONE, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(helper.art_asks().len(), 1);
}

#[tokio::test]
async fn with_the_setting_off_the_cover_route_is_a_404_and_the_helper_sees_nothing() {
    let helper = Helper::new();
    helper.set(|b| {
        b.art = Some(art("image/jpeg", ETAG_A, jpeg(300)));
        b.items = vec![{
            let mut song = item(11, 1, "Song One");
            song["artwork"] = artwork_info(11, "image/jpeg", ETAG_A, 300);
            song
        }];
    });
    let address = helper.serve().await;
    let (router, runtime) = app().await;
    runtime
        .store
        .mutate(|s| s.plan = Some(plan("2026-11-22", &["Song One"])));
    // Off by default.
    assert_eq!(raw(&router, COVER_ONE, None).await.0, StatusCode::NOT_FOUND);
    // Saved address but switched off.
    let _ = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"enabled": false, "address": address})),
    )
    .await;
    assert_eq!(
        raw(&router, COVER_ONE, Some(ETAG_A)).await.0,
        StatusCode::NOT_FOUND
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(helper.hits().is_empty(), "{:?}", helper.hits());
    // On, a picture held, then off again: the picture is let go.
    let _ = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"enabled": true})),
    )
    .await;
    assert_eq!(raw(&router, COVER_ONE, None).await.0, StatusCode::OK);
    assert_eq!(runtime.playback_library.artwork_held().0, 1);
    let _ = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"enabled": false})),
    )
    .await;
    assert_eq!(raw(&router, COVER_ONE, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(runtime.playback_library.artwork_held(), (0, 0));
}

#[tokio::test]
async fn the_cache_holds_at_most_64_pictures_and_8_mb_and_lets_the_oldest_go_first() {
    // 70 songs, each with a small cover.
    let helper = Helper::new();
    let titles: Vec<String> = (0..70).map(|n| format!("Long Song {n}")).collect();
    helper.set(|b| {
        b.items = titles
            .iter()
            .enumerate()
            .map(|(n, title)| {
                let id = 100 + n as i64;
                let mut song = item(id, n as u32 + 1, title);
                song["artwork"] = artwork_info(id, "image/jpeg", ETAG_A, 100);
                song
            })
            .collect();
        b.art = Some(art("image/jpeg", ETAG_A, jpeg(100)));
    });
    let address = helper.serve().await;
    let (router, runtime) = app().await;
    let names: Vec<&str> = titles.iter().map(String::as_str).collect();
    runtime
        .store
        .mutate(|s| s.plan = Some(plan("2026-11-22", &names)));
    let _ = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"enabled": true, "address": address})),
    )
    .await;
    for n in 0..70 {
        let (status, _, _) = raw(
            &router,
            &format!("/api/v1/playback-library/songs/{}/artwork", 100 + n),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "song {n}");
    }
    assert_eq!(runtime.playback_library.artwork_held(), (64, 6400));
    assert_eq!(helper.art_asks().len(), 70);
    // The newest are still held; the oldest six are not.
    assert_eq!(
        raw(&router, "/api/v1/playback-library/songs/169/artwork", None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(helper.art_asks().len(), 70);
    assert_eq!(
        raw(&router, "/api/v1/playback-library/songs/100/artwork", None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(helper.art_asks().len(), 71);
    assert_eq!(runtime.playback_library.artwork_held().0, 64);
}

#[tokio::test]
async fn big_pictures_stop_at_eight_megabytes_in_all() {
    let helper = Helper::new();
    let titles: Vec<String> = (0..6).map(|n| format!("Big Song {n}")).collect();
    helper.set(|b| {
        b.items = titles
            .iter()
            .enumerate()
            .map(|(n, title)| {
                let id = 200 + n as i64;
                let mut song = item(id, n as u32 + 1, title);
                song["artwork"] = artwork_info(id, "image/jpeg", ETAG_A, 1_900_000);
                song
            })
            .collect();
        b.art = Some(art("image/jpeg", ETAG_A, jpeg(1_900_000)));
    });
    let address = helper.serve().await;
    let (router, runtime) = app().await;
    let names: Vec<&str> = titles.iter().map(String::as_str).collect();
    runtime
        .store
        .mutate(|s| s.plan = Some(plan("2026-11-22", &names)));
    let _ = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"enabled": true, "address": address})),
    )
    .await;
    for id in 200..206 {
        let (status, _, body) = raw(
            &router,
            &format!("/api/v1/playback-library/songs/{id}/artwork"),
            None,
        )
        .await;
        assert_eq!((status, body.len()), (StatusCode::OK, 1_900_000));
        let (held, bytes) = runtime.playback_library.artwork_held();
        assert!(held <= 64 && bytes <= 8 * 1024 * 1024, "{held} {bytes}");
    }
    // Four fit (7.6 MB); the fifth and sixth each pushed the oldest out.
    assert_eq!(runtime.playback_library.artwork_held(), (4, 7_600_000));
    assert_eq!(helper.art_asks().len(), 6);
    // The newest is held, the oldest is not.
    assert_eq!(
        raw(&router, "/api/v1/playback-library/songs/205/artwork", None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(helper.art_asks().len(), 6);
    assert_eq!(
        raw(&router, "/api/v1/playback-library/songs/200/artwork", None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(helper.art_asks().len(), 7);
}
