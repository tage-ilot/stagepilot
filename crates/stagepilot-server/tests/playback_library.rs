//! The optional Playback library, against a fake helper (an axum server on a loopback port): every
//! state the real helper can be in, the key it is asked with, the request rules (at most one ask per 30 s,
//! 3 s, size cap, no redirect, no waveform) and the routes. No real
//! helper, no rig.

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
use stagepilot_playback::library::{
    HelperState, LengthSource, LibraryPlan, LibraryState, MatchConfidence, PlanMatch,
};
use stagepilot_server::playback_library::{base_url, sign, PlaybackLibrary};
use stagepilot_server::{build_app, AppOptions, OutputBackends, Runtime};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tower::ServiceExt;

/// The helper version this StagePilot carries: a stand-in at this version is `ready`; an older one is `outdated`.
const BUNDLED: &str = stagepilot_playback::helper_assets::BUNDLED_HELPER_VERSION;

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
    /// What `/songs/<id>` says as `midi` (`None`: the key is not there).
    midi: Option<Value>,
    /// Song ids whose own answer is an error.
    broken_songs: Vec<i64>,
    /// The cover the fake serves for every song (`None`: 404), and how.
    art: Option<Art>,
    /// What each cover request carried as `If-None-Match`.
    art_asks: Vec<(i64, Option<String>)>,
    /// What the fake answers to a waveform ask (`None`: 404), as text, so a test can send any bytes.
    waveform: Option<String>,
    /// Answer every waveform ask with this status instead.
    wave_status: Option<u16>,
    /// The text the fake sends instead of `{"error": "no"}` with that status (any bytes).
    wave_body: Option<String>,
    /// Wait this long before a waveform answer only.
    wave_delay: Duration,
    /// The query string of each waveform ask.
    wave_queries: Vec<String>,
    /// Other setlists the fake holds, by Playback's number: (id, title, date, items).
    others: Vec<(i64, String, String, Vec<Value>)>,
    /// The helper's key: when set it answers the way helper 0.3.0 does (a signed request, or 401; the bare
    /// `/health` answers the minimal body).
    key: Option<String>,
    /// The time the helper believes it is, seconds since the epoch.
    clock: u64,
    /// Each request's `X-PBL-Time`, `X-PBL-Sig` and request target (path and query) as the helper saw them.
    signed: Vec<(Option<String>, Option<String>, String)>,
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
        "setlist_song_id": id, "content_id": 5_000_000 + id, "order": order, "title": title, "artist": "Artist",
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
        let text = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let target = uri
            .path_and_query()
            .map_or(uri.path().to_string(), |t| t.as_str().to_string());
        b.signed
            .push((text("x-pbl-time"), text("x-pbl-sig"), target));
        let mut delay_extra = Duration::ZERO;
        if uri.path().ends_with("/waveform") {
            b.wave_queries
                .push(uri.query().unwrap_or_default().to_string());
            delay_extra = b.wave_delay;
        }
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
        (b.delay + delay_extra, b.broken, b.redirect)
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
    let mut authenticated = false;
    if let Some(key) = &b.key {
        let (stamp, sig, target) = b.signed.last().cloned().unwrap();
        let valid = match (&stamp, &sig) {
            (Some(stamp), Some(sig)) => stamp.parse::<u64>().is_ok_and(|time| {
                time.abs_diff(b.clock) <= 120 && sign(key, &target, time) == *sig
            }),
            _ => false,
        };
        if !valid {
            if path == "/health" && stamp.is_none() && sig.is_none() {
                return (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/json")],
                    json!({"ok": true, "name": "playback-library", "version": "0.3.0", "auth": "required"}).to_string(),
                )
                    .into_response();
            }
            return (StatusCode::UNAUTHORIZED, "{\"error\": \"key required\"}").into_response();
        }
        authenticated = true;
    }
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
        let mut health = b.health.clone().unwrap();
        if authenticated {
            health["auth"] = json!("key");
        }
        return json_ok(health);
    }
    if path == "/setlists" {
        let mut list = vec![
            json!({"setlist_id": 1, "title": "Set For Test", "performance_date": b.setlist_date, "songs": b.items.len()}),
        ];
        for (id, title, date, items) in &b.others {
            list.push(json!({"setlist_id": id, "title": title, "performance_date": date, "songs": items.len()}));
        }
        return json_ok(json!({"setlists": list}));
    }
    if let Some((id, title, date, items)) = path
        .strip_prefix("/setlists/")
        .and_then(|r| r.parse::<i64>().ok())
        .and_then(|n| b.others.iter().find(|o| o.0 == n))
    {
        return json_ok(
            json!({"setlist_id": id, "title": title, "performance_date": date, "items": items}),
        );
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
        if id.ends_with("/waveform") {
            if let Some(status) = b.wave_status {
                let text = b
                    .wave_body
                    .clone()
                    .unwrap_or_else(|| "{\"error\": \"no\"}".to_string());
                return (StatusCode::from_u16(status).unwrap(), text).into_response();
            }
            return match b.waveform.clone() {
                Some(text) => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/json")],
                    text,
                )
                    .into_response(),
                None => (StatusCode::NOT_FOUND, "{\"error\": \"no waveform\"}").into_response(),
            };
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
        let mut answer = json!({"setlist_song_id": id, "arrangement_detail": {"title": "Short", "sections": sections}});
        if let Some(midi) = &b.midi {
            answer["midi"] = midi.clone();
        }
        return json_ok(answer);
    }
    StatusCode::NOT_FOUND.into_response()
}

fn settings_for(address: &str) -> PlaybackLibrarySettings {
    PlaybackLibrarySettings {
        enabled: None,
        address: address.to_string(),
    }
}

/// The same settings once the helper has been in use (`enabled: true` is saved the first time it answers).
fn in_use(settings: &PlaybackLibrarySettings) -> PlaybackLibrarySettings {
    PlaybackLibrarySettings {
        enabled: Some(true),
        ..settings.clone()
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
    // The MultiTracks number comes through, and it is the song's own, not the setlist-song number.
    assert_eq!(
        (one.setlist_song_id, one.content_id),
        (Some(11), Some(5_000_011))
    );
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
    // Never in use: nothing is installed there, and that is not a warning anywhere.
    let settings = settings_for(&format!("127.0.0.1:{closed}"));
    let status = library.status(&settings, false).await;
    assert_eq!(status.helper.state, HelperState::NotInstalled);
    assert_eq!((status.enabled, status.active), (false, false));
    assert_eq!(status.state, LibraryState::Off);
    let result = library
        .plan(&settings, Some(&plan("2026-11-22", &["Song One"])), false)
        .await;
    assert_eq!((result.state, result.enabled), (LibraryState::Off, false));
    // It was in use: now it has gone quiet, which is the one thing that may be said.
    let library = PlaybackLibrary::new();
    let in_use = in_use(&settings);
    let status = library.status(&in_use, false).await;
    assert_eq!(status.helper.state, HelperState::Unreachable);
    assert_eq!(status.state, LibraryState::Problem);
    assert_eq!(status.message, "The Playback computer did not answer.");
    let result = library
        .plan(&in_use, Some(&plan("2026-11-22", &["Song One"])), false)
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
    let status = library
        .status(&in_use(&settings_for(&address)), false)
        .await;
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
    assert_eq!(waves_asked(&helper), 0, "{:?}", helper.hits());
}

#[tokio::test]
async fn no_address_is_a_sentence_and_nothing_is_asked() {
    let helper = Helper::new();
    let _address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let blank = PlaybackLibrarySettings {
        enabled: None,
        address: String::new(),
    };
    let status = library.status(&blank, false).await;
    assert_eq!(status.message, "Choose the Playback computer first.");
    assert_eq!(status.helper.state, HelperState::NotInstalled);
    let result = library
        .plan(&blank, Some(&plan("2026-11-22", &["Song One"])), true)
        .await;
    assert_eq!(result.state, LibraryState::Off);
    assert!(result.songs.is_empty());
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
    // 403: this computer is not on the helper's list (an old install). Run the setup command again.
    let helper = Helper::new();
    helper.set(|b| b.broken = Some(403));
    let address = helper.serve().await;
    let status = PlaybackLibrary::new()
        .status(&settings_for(&address), false)
        .await;
    assert_eq!(status.helper.state, HelperState::NeedsKey);
    assert_eq!(
        status.state,
        LibraryState::Off,
        "quiet: no warning for this"
    );
    assert!(
        status.message.contains("setup command again"),
        "{}",
        words(&status.message)
    );
    // 405 and other answers it does not know.
    helper.set(|b| b.broken = Some(405));
    let status = PlaybackLibrary::new()
        .status(&in_use(&settings_for(&address)), false)
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
        .status(&in_use(&settings_for(&address)), false)
        .await;
    assert_eq!(status.state, LibraryState::Problem);
    assert!(helper.hits().iter().all(|h| h == "/health"));
    // Larger than the cap.
    helper.set(|b| {
        b.redirect = false;
        b.huge_health = true;
    });
    let status = PlaybackLibrary::new()
        .status(&in_use(&settings_for(&address)), false)
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
        .status(&in_use(&settings_for(&address)), false)
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
async fn with_no_playback_computer_chosen_the_routes_ask_nothing_and_say_nothing() {
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
async fn naming_the_helper_computer_saves_only_that_and_checks_once() {
    let helper = Helper::new();
    let address = helper.serve().await;
    let (router, runtime) = app().await;
    let before = runtime.settings_service.snapshot();
    let (status, body) = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"address": address})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], json!("ready"));
    assert_eq!(body["address"], json!(format!("http://{address}")));
    let after = runtime.settings_service.snapshot();
    assert_eq!(
        after.playback_library,
        PlaybackLibrarySettings {
            enabled: None,
            address: format!("http://{address}")
        }
    );
    let mut restored = after.clone();
    restored.playback_library = PlaybackLibrarySettings::default();
    assert_eq!(restored, before, "nothing but the one setting changed");
    assert_eq!(helper.hits(), ["/health"]);
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(
        settings["settings"]["playback_library"]["address"],
        json!(format!("http://{address}"))
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
        Some(json!({"address": "10.0.0.5/path"})),
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
        Some(json!({"address": 5})),
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
        Some(json!({"address": "127.0.0.1:9"})),
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
    assert_eq!(saved.playback_library.address, "http://127.0.0.1:9");
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
        Some(json!({"address": address})),
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
        playback_only: vec![],
        chosen: false,
        setlists: vec![],
        stale: false,
        checked_at: None,
    })
    .unwrap();
    assert_eq!(value["state"], json!("no_setlist"));
    assert_eq!(value["playback_only"], json!([]));
    assert_eq!(value["chosen"], json!(false));
    assert_eq!(value["setlists"], json!([]));
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
        Some(json!({"address": address})),
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

// ---- lyric cues (follow lyrics, stage 1) --------------------------------------------------------

fn midi_event(bus: i64, note: i64, velocity: i64, time_s: f64) -> Value {
    json!({"time_s": time_s, "note": note, "velocity": velocity, "channel": 0, "type": 1,
           "bus_id": bus, "duration_s": 0.0, "description": "x"})
}

#[tokio::test]
async fn the_plan_says_how_many_lyric_cues_a_song_has_and_never_lists_them() {
    let helper = Helper::new();
    helper.set(|b| {
        b.midi = Some(json!([
            midi_event(1, 19, 2, 8.5),
            midi_event(1, 19, 1, 3.5),
            midi_event(4, 112, 0, 1.0),
            midi_event(1, 19, 1, 3.5),
            midi_event(1, 19, 9, 20.0),
            {"note": 19},
            "junk"
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
        Some(json!({"address": address})),
    )
    .await;
    let (status, body) = call(&router, "GET", "/api/v1/playback-library/plan", None).await;
    assert_eq!(status, StatusCode::OK);
    let one = &body["songs"][0]["library"];
    assert_eq!(one["lyric_cue_count"], json!(3));
    assert_eq!(one["lyric_cue_max_slide"], json!(9));
    assert!(!body.to_string().contains("lyric_cues"), "{body}");
    assert!(!body.to_string().contains("velocity"), "{body}");
    // The cues themselves are kept for following, sorted and with the doubled one once.
    let result = runtime
        .playback_library
        .plan(
            &settings_for(&address),
            Some(&plan("2026-11-22", &["Song One"])),
            false,
        )
        .await;
    let cues = &first_song(&result).lyric_cues;
    let got: Vec<(f64, u32)> = cues.iter().map(|c| (c.time_s, c.slide)).collect();
    assert_eq!(got, vec![(3.5, 1), (8.5, 2), (20.0, 9)]);
}

#[tokio::test]
async fn a_song_with_no_midi_has_no_cues_and_an_answer_over_the_size_cap_is_still_refused() {
    let helper = Helper::new();
    let address = helper.serve().await;
    let result = PlaybackLibrary::new()
        .plan(
            &settings_for(&address),
            Some(&plan("2026-11-22", &["Song One"])),
            false,
        )
        .await;
    let one = result.songs[0].library.as_ref().unwrap();
    assert_eq!((one.lyric_cue_count, one.lyric_cue_max_slide), (0, None));
    assert_eq!(one.section_count, Some(7));
    // An answer over the helper size cap (256 KB) is the same as no answer for that song: no sections, no cues.
    let big: Vec<Value> = (0..4000)
        .map(|n| midi_event(1, 19, i64::from(n % 50), f64::from(n)))
        .collect();
    let helper = Helper::new();
    helper.set(|b| b.midi = Some(Value::Array(big)));
    let address = helper.serve().await;
    let result = PlaybackLibrary::new()
        .plan(
            &settings_for(&address),
            Some(&plan("2026-11-22", &["Song One"])),
            false,
        )
        .await;
    let one = result.songs[0].library.as_ref().unwrap();
    assert_eq!((one.lyric_cue_count, one.section_count), (0, None));
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
    // A new key (or another helper) resets the library: nothing is remembered.
    library.reset().await;
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
        Some(json!({"address": address})),
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
        Some(json!({"address": address})),
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
async fn without_a_computer_the_cover_route_is_a_404_and_the_helper_sees_nothing() {
    let helper = Helper::new();
    helper.set(|b| {
        b.art = Some(art("image/jpeg", ETAG_A, jpeg(300)));
        b.items = vec![{
            let mut song = item(11, 1, "Song One");
            song["artwork"] = artwork_info(11, "image/jpeg", ETAG_A, 300);
            song
        }];
    });
    let _address = helper.serve().await;
    let (router, runtime) = app().await;
    runtime
        .store
        .mutate(|s| s.plan = Some(plan("2026-11-22", &["Song One"])));
    // No Playback computer chosen: nothing to ask.
    assert_eq!(
        raw(&router, COVER_ONE, Some(ETAG_A)).await.0,
        StatusCode::NOT_FOUND
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(helper.hits().is_empty(), "{:?}", helper.hits());
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
        Some(json!({"address": address})),
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
        Some(json!({"address": address})),
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

// ---- waveforms ------------------------------------------------------------------------------------

const WAVE_ONE: &str = "/api/v1/playback-library/songs/11/waveform";

/// What helper 0.2 says (`mean` and the other fields are there to be left out).
fn helper_wave(step: f64, peak: &[f64]) -> String {
    json!({"setlist_song_id": 11, "clock": "performed", "source": "Waveform1x.32.png", "step_s": step,
           "count": peak.len(), "peak": peak, "mean": peak.iter().map(|p| p / 2.0).collect::<Vec<_>>()})
    .to_string()
}

/// Three paired songs and a helper that has this waveform for them.
async fn waves(text: Option<String>) -> (Helper, axum::Router, Arc<Runtime>) {
    let (helper, router, runtime) = covers(None).await;
    helper.set(|b| b.waveform = text);
    (helper, router, runtime)
}

#[tokio::test]
async fn a_waveform_reaches_the_window_as_whole_numbers_and_nothing_else() {
    let (helper, router, _) = waves(Some(helper_wave(0.125, &[0.0, 0.5, 1.0, 0.2]))).await;
    let (status, headers, body) = raw(&router, WAVE_ONE, None).await;
    assert_eq!(status, StatusCode::OK);
    let sent: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(sent, json!({"step_s": 0.125, "peak": [0, 128, 255, 51]}));
    assert_eq!(sent.as_object().unwrap().len(), 2, "{sent}");
    let head = |name: header::HeaderName| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    assert_eq!(
        head(header::CONTENT_TYPE).as_deref(),
        Some("application/json")
    );
    assert_eq!(
        head(header::CACHE_CONTROL).as_deref(),
        Some("private, max-age=300")
    );
    assert_eq!(
        head(header::X_CONTENT_TYPE_OPTIONS).as_deref(),
        Some("nosniff")
    );
    assert!(head(header::ETAG).is_some_and(|tag| tag.starts_with('"') && tag.ends_with('"')));
    // Asked once, for that song, the way the spec says.
    assert_eq!(helper.count("/songs/11/waveform"), 1);
    assert_eq!(
        helper.inner.lock().unwrap().wave_queries,
        ["points_per_second=8&clock=performed"]
    );
}

#[tokio::test]
async fn the_check_and_the_plan_never_ask_for_a_waveform() {
    let (helper, router, _) = waves(Some(helper_wave(0.125, &[0.1, 0.2]))).await;
    for path in ["status?check=1", "plan?check=1", "plan", "status"] {
        let (status, _) = call(
            &router,
            "GET",
            &format!("/api/v1/playback-library/{path}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    assert_eq!(helper.count("/songs/11/waveform"), 0, "{:?}", helper.hits());
}

#[tokio::test]
async fn wrong_shapes_are_no_waveform_and_the_limits_themselves_are_fine() {
    let many = |n: usize| vec![0.5; n];
    let bad: Vec<(&str, String)> = vec![
        ("too many points", helper_wave(0.125, &many(20_001))),
        ("negative value", helper_wave(0.125, &[0.1, -0.1])),
        ("value above 1", helper_wave(0.125, &[0.1, 1.01])),
        ("no points", helper_wave(0.125, &[])),
        ("step 0", helper_wave(0.0, &[0.1])),
        ("negative step", helper_wave(-0.5, &[0.1])),
        ("step over 10", helper_wave(10.5, &[0.1])),
        ("huge step", helper_wave(1.0e300, &[0.1])),
        (
            "text instead of numbers",
            r#"{"step_s": 0.25, "peak": ["NaN", "0.5"]}"#.into(),
        ),
        (
            "null in the list",
            r#"{"step_s": 0.25, "peak": [0.1, null]}"#.into(),
        ),
        (
            "a number past f64",
            r#"{"step_s": 0.25, "peak": [1e999]}"#.into(),
        ),
        ("no step", r#"{"peak": [0.1, 0.2]}"#.into()),
        (
            "step as text",
            r#"{"step_s": "0.25", "peak": [0.1]}"#.into(),
        ),
        ("no peak", r#"{"step_s": 0.25}"#.into()),
        ("peak not a list", r#"{"step_s": 0.25, "peak": 0.5}"#.into()),
        ("not JSON", "<html>".into()),
        ("a list", "[0.1, 0.2]".into()),
    ];
    for (what, text) in bad {
        let (helper, router, runtime) = waves(Some(text)).await;
        let (status, _, _) = raw(&router, WAVE_ONE, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{what}");
        assert_eq!(helper.count("/songs/11/waveform"), 1, "{what}");
        assert_eq!(runtime.playback_library.waveforms_held(), 0, "{what}");
    }
    // 20,000 points and a 10 second step are the largest allowed.
    let (_, router, _) = waves(Some(helper_wave(10.0, &many(20_000)))).await;
    let (status, _, body) = raw(&router, WAVE_ONE, None).await;
    assert_eq!(status, StatusCode::OK);
    let sent: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(sent["peak"].as_array().unwrap().len(), 20_000);
    assert_eq!(sent["step_s"], json!(10.0));
}

#[tokio::test]
async fn an_answer_over_600000_bytes_is_refused_and_exactly_that_many_is_fine() {
    let pad = |total: usize| {
        let base = helper_wave(0.25, &[0.5, 0.25]);
        format!("{base}{}", " ".repeat(total - base.len()))
    };
    let (_, router, _) = waves(Some(pad(600_000))).await;
    assert_eq!(raw(&router, WAVE_ONE, None).await.0, StatusCode::OK);
    let (_, router, _) = waves(Some(pad(600_001))).await;
    assert_eq!(raw(&router, WAVE_ONE, None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_redirect_is_not_followed_and_an_error_from_the_helper_is_a_503() {
    let (helper, router, runtime) = waves(Some(helper_wave(0.25, &[0.5]))).await;
    // Read the plan once so that the route does not need to ask for it again.
    let (status, _) = call(&router, "GET", "/api/v1/playback-library/plan", None).await;
    assert_eq!(status, StatusCode::OK);
    helper.set(|b| b.redirect = true);
    assert_eq!(
        raw(&router, WAVE_ONE, None).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        helper.count("/health"),
        1,
        "a redirect target was not followed"
    );
    helper.set(|b| {
        b.redirect = false;
        b.wave_status = Some(500);
    });
    assert_eq!(
        raw(&router, WAVE_ONE, None).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(runtime.playback_library.waveforms_held(), 0);
}

/// How many waveforms the helper was asked for, of any song.
fn waves_asked(helper: &Helper) -> usize {
    helper
        .hits()
        .iter()
        .filter(|hit| hit.ends_with("/waveform") || hit.contains("/waveform?"))
        .count()
}

/// What a Lightkey recording asks when a song starts: the Library Link in use, the plan, the song's Lighting key.
async fn recording_asks(runtime: &Runtime, settings: &PlaybackLibrarySettings, key: &str) -> bool {
    let loaded = runtime.store.snapshot().plan;
    runtime
        .playback_library
        .song_has_waveform(settings, loaded.as_ref(), key)
        .await
}

#[tokio::test]
async fn a_recording_learns_a_songs_waveform_and_the_plan_says_so_only_once_it_is_held() {
    let (helper, _router, runtime) = waves(Some(helper_wave(0.25, &[0.5, 1.0]))).await;
    let settings = in_use(&stagepilot_server::playback_library::effective_settings(
        &runtime,
    ));
    let loaded = runtime.store.snapshot().plan;
    let held = |plan: &LibraryPlan| -> Vec<bool> {
        plan.songs
            .iter()
            .map(|song| song.library.as_ref().is_some_and(|l| l.has_waveform))
            .collect()
    };
    // Reading the plan never asks for a waveform, so nothing is known yet.
    let before = runtime
        .playback_library
        .plan(&settings, loaded.as_ref(), false)
        .await;
    assert_eq!(held(&before), [false, false, false]);
    assert_eq!(waves_asked(&helper), 0, "{:?}", helper.hits());
    // The recording of song one asks: the helper has a waveform for it (item-0 is Playback's song 11).
    assert!(recording_asks(&runtime, &settings, "item-0").await);
    assert_eq!(helper.count("/songs/11/waveform"), 1);
    // Asked again, it is answered from memory.
    assert!(recording_asks(&runtime, &settings, "item-0").await);
    assert_eq!(helper.count("/songs/11/waveform"), 1);
    // The plan now says it for that song alone.
    let after = runtime
        .playback_library
        .plan(&settings, loaded.as_ref(), false)
        .await;
    assert_eq!(held(&after), [true, false, false]);
    // A song that is not in the plan is not a question for the helper.
    assert!(!recording_asks(&runtime, &settings, "no-such-song").await);
    assert_eq!(waves_asked(&helper), 1, "{:?}", helper.hits());
}

#[tokio::test]
async fn a_song_with_no_waveform_stays_a_song_with_no_waveform_for_a_recording() {
    let (helper, _router, runtime) = waves(None).await;
    let settings = in_use(&stagepilot_server::playback_library::effective_settings(
        &runtime,
    ));
    assert!(!recording_asks(&runtime, &settings, "item-1").await);
    assert_eq!(helper.count("/songs/12/waveform"), 1);
    let loaded = runtime.store.snapshot().plan;
    let plan = runtime
        .playback_library
        .plan(&settings, loaded.as_ref(), false)
        .await;
    assert!(plan
        .songs
        .iter()
        .all(|song| song.library.as_ref().is_some_and(|l| !l.has_waveform)));
}

#[tokio::test]
async fn a_recording_never_asks_a_computer_that_never_had_the_library_link() {
    let (helper, _router, runtime) = waves(Some(helper_wave(0.25, &[0.5, 1.0]))).await;
    // `enabled` is saved only once the helper has answered with a key: until then nothing is asked from here.
    let settings = PlaybackLibrarySettings {
        enabled: None,
        ..stagepilot_server::playback_library::effective_settings(&runtime)
    };
    assert!(!recording_asks(&runtime, &settings, "item-0").await);
    assert_eq!(waves_asked(&helper), 0, "{:?}", helper.hits());
}

#[tokio::test]
async fn a_helper_without_a_waveform_is_a_404() {
    let (helper, router, _) = waves(None).await;
    assert_eq!(raw(&router, WAVE_ONE, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(helper.count("/songs/11/waveform"), 1);
}

#[tokio::test]
async fn a_slow_helper_gets_six_seconds_not_three_and_then_is_given_up_on() {
    let (helper, router, _) = waves(Some(helper_wave(0.25, &[0.5, 1.0]))).await;
    let (status, _) = call(&router, "GET", "/api/v1/playback-library/plan", None).await;
    assert_eq!(status, StatusCode::OK);
    // Longer than the 3 s of every other ask, shorter than 6 s: still answered.
    helper.set(|b| b.wave_delay = Duration::from_millis(4500));
    let started = Instant::now();
    let (status, _, body) = raw(&router, WAVE_ONE, None).await;
    assert_eq!(status, StatusCode::OK, "{:?}", started.elapsed());
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["peak"],
        json!([128, 255])
    );
    // Longer than 6 s: given up on after about 6 s.
    helper.set(|b| b.wave_delay = Duration::from_secs(9));
    let started = Instant::now();
    let (status, _, _) = raw(&router, "/api/v1/playback-library/songs/12/waveform", None).await;
    let took = started.elapsed();
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        took >= Duration::from_secs(5) && took < Duration::from_secs(8),
        "{took:?}"
    );
}

#[tokio::test]
async fn only_a_paired_song_of_the_current_plan_has_a_waveform_route() {
    let (helper, router, runtime) = waves(Some(helper_wave(0.25, &[0.5]))).await;
    for path in [
        "abc",
        "-11",
        "11.0",
        "1e1",
        "%2e%2e%2fhealth",
        "11%2f..%2f..%2fhealth",
        "99999999999999999999999",
        "0",
        "10",
        "14",
    ] {
        let uri = format!("/api/v1/playback-library/songs/{path}/waveform");
        assert_eq!(
            raw(&router, &uri, None).await.0,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    assert_eq!(
        helper.count("/songs/") - 3,
        0,
        "only the three detail reads: {:?}",
        helper.hits()
    );
    assert!(helper.inner.lock().unwrap().wave_queries.is_empty());
    // In Playback's setlist, not in the plan on screen.
    runtime
        .store
        .mutate(|s| s.plan = Some(plan("2026-11-22", &["Song One"])));
    assert_eq!(raw(&router, WAVE_ONE, None).await.0, StatusCode::OK);
    assert_eq!(
        raw(&router, "/api/v1/playback-library/songs/12/waveform", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(helper.inner.lock().unwrap().wave_queries.len(), 1);
    // A plan song Playback does not have.
    runtime
        .store
        .mutate(|s| s.plan = Some(plan("2026-11-22", &["Song Nine"])));
    runtime
        .playback_library
        .advance_clock(Duration::from_secs(31));
    assert_eq!(raw(&router, WAVE_ONE, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(helper.inner.lock().unwrap().wave_queries.len(), 1);
}

#[tokio::test]
async fn without_a_computer_the_waveform_route_is_a_404_and_the_helper_sees_nothing() {
    let helper = Helper::new();
    helper.set(|b| b.waveform = Some(helper_wave(0.25, &[0.5, 0.7])));
    let _address = helper.serve().await;
    let (router, runtime) = app().await;
    runtime
        .store
        .mutate(|s| s.plan = Some(plan("2026-11-22", &["Song One"])));
    // No Playback computer chosen: nothing to ask.
    assert_eq!(
        raw(&router, WAVE_ONE, Some("\"x\"")).await.0,
        StatusCode::NOT_FOUND
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(helper.hits().is_empty(), "{:?}", helper.hits());
    assert_eq!(runtime.playback_library.waveforms_held(), 0);
}

/// A fake helper with a revision token, three paired songs and a waveform for each.
async fn revised(token: Option<&str>) -> (Helper, axum::Router, Arc<Runtime>) {
    let (helper, router, runtime) = waves(Some(helper_wave(0.25, &[0.5, 1.0]))).await;
    helper.set(|b| b.health = Some(with_revision(token)));
    // A fresh read of the helper's health, so the token is the one held.
    runtime
        .playback_library
        .advance_clock(Duration::from_secs(31));
    (helper, router, runtime)
}

#[tokio::test]
async fn a_waveform_is_kept_while_the_revision_is_unchanged_and_fetched_again_when_it_changes() {
    let (helper, router, runtime) = revised(Some("aaaaaaaaaaaa")).await;
    let (first, headers, body) = raw(&router, WAVE_ONE, None).await;
    assert_eq!(first, StatusCode::OK);
    assert_eq!(helper.count("/songs/11/waveform"), 1);
    // Same token: from memory, however many windows ask and however many checks pass.
    for _ in 0..3 {
        runtime
            .playback_library
            .advance_clock(Duration::from_secs(31));
        let (status, _, again) = raw(&router, WAVE_ONE, None).await;
        assert_eq!((status, again), (StatusCode::OK, body.clone()));
    }
    assert_eq!(helper.count("/songs/11/waveform"), 1);
    // Another token (the library changed): the next ask goes to the helper, with new numbers.
    helper.set(|b| {
        b.health = Some(with_revision(Some("bbbbbbbbbbbb")));
        b.waveform = Some(helper_wave(0.25, &[1.0, 0.0]));
    });
    runtime
        .playback_library
        .advance_clock(Duration::from_secs(31));
    let (status, new_headers, new_body) = raw(&router, WAVE_ONE, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(helper.count("/songs/11/waveform"), 2);
    assert_eq!(
        serde_json::from_slice::<Value>(&new_body).unwrap()["peak"],
        json!([255, 0])
    );
    assert_ne!(headers.get(header::ETAG), new_headers.get(header::ETAG));
    // Flipping back to the first token fetches once more (only the last good token counts).
    helper.set(|b| b.health = Some(with_revision(Some("aaaaaaaaaaaa"))));
    runtime
        .playback_library
        .advance_clock(Duration::from_secs(31));
    assert_eq!(raw(&router, WAVE_ONE, None).await.0, StatusCode::OK);
    assert_eq!(helper.count("/songs/11/waveform"), 3);
    assert_eq!(runtime.playback_library.waveforms_held(), 1);
}

#[tokio::test]
async fn a_helper_with_no_revision_is_trusted_for_ten_minutes_then_asked_again() {
    let (helper, router, runtime) = revised(None).await;
    assert_eq!(raw(&router, WAVE_ONE, None).await.0, StatusCode::OK);
    assert_eq!(helper.count("/songs/11/waveform"), 1);
    // Nine minutes on (the clock is faked), in checks of 31 s or more: still held.
    for _ in 0..9 {
        runtime
            .playback_library
            .advance_clock(Duration::from_secs(60));
        assert_eq!(raw(&router, WAVE_ONE, None).await.0, StatusCode::OK);
    }
    assert_eq!(helper.count("/songs/11/waveform"), 1);
    // Past ten minutes in all: asked again, once.
    runtime
        .playback_library
        .advance_clock(Duration::from_secs(90));
    assert_eq!(raw(&router, WAVE_ONE, None).await.0, StatusCode::OK);
    assert_eq!(helper.count("/songs/11/waveform"), 2);
    assert_eq!(raw(&router, WAVE_ONE, None).await.0, StatusCode::OK);
    assert_eq!(helper.count("/songs/11/waveform"), 2);
}

#[tokio::test]
async fn at_most_sixteen_songs_are_held_and_the_oldest_goes_first() {
    let helper = Helper::new();
    helper.set(|b| {
        b.health = Some(with_revision(Some("cccccccccccc")));
        b.waveform = Some(helper_wave(0.25, &[0.5]));
        b.items = (11..31)
            .map(|id| item(id, id as u32 - 10, &format!("Song {id}")))
            .collect();
    });
    let address = helper.serve().await;
    let (router, runtime) = app().await;
    let titles: Vec<String> = (11..31).map(|id| format!("Song {id}")).collect();
    let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
    runtime
        .store
        .mutate(|s| s.plan = Some(plan("2026-11-22", &titles)));
    let _ = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"address": address})),
    )
    .await;
    for id in 11..31 {
        let uri = format!("/api/v1/playback-library/songs/{id}/waveform");
        assert_eq!(raw(&router, &uri, None).await.0, StatusCode::OK, "{id}");
    }
    assert_eq!(
        helper.count("/songs/") - 20,
        20,
        "20 reads of songs and 20 waveforms"
    );
    assert_eq!(runtime.playback_library.waveforms_held(), 16);
    // The newest are held, the oldest went.
    let before = helper.inner.lock().unwrap().wave_queries.len();
    assert_eq!(
        raw(&router, "/api/v1/playback-library/songs/30/waveform", None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(helper.inner.lock().unwrap().wave_queries.len(), before);
    assert_eq!(
        raw(&router, "/api/v1/playback-library/songs/11/waveform", None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(helper.inner.lock().unwrap().wave_queries.len(), before + 1);
    assert_eq!(runtime.playback_library.waveforms_held(), 16);
}

#[tokio::test]
async fn the_window_gets_304_when_it_already_holds_the_waveform() {
    let (helper, router, _) = waves(Some(helper_wave(0.25, &[0.5, 1.0]))).await;
    let (_, headers, body) = raw(&router, WAVE_ONE, None).await;
    let etag = headers
        .get(header::ETAG)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let (status, headers, again) = raw(&router, WAVE_ONE, Some(&etag)).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(again.is_empty());
    assert_eq!(headers.get(header::ETAG).unwrap().to_str().unwrap(), etag);
    assert_eq!(
        headers
            .get(header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap(),
        "private, max-age=300"
    );
    // A weak form and a list are understood; another tag gets the numbers.
    assert_eq!(
        raw(&router, WAVE_ONE, Some(&format!("\"zz\", W/{etag}")))
            .await
            .0,
        StatusCode::NOT_MODIFIED
    );
    let (status, _, other) = raw(&router, WAVE_ONE, Some("\"0000000000000000\"")).await;
    assert_eq!((status, other), (StatusCode::OK, body));
    assert_eq!(helper.count("/songs/11/waveform"), 1);
}

// ---- the key, the signature and the helper's state ------------------------------------------------------

const KEY: &str = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE";
const NOW: u64 = 1_794_945_365;

/// A library that signs with `KEY` and believes it is `NOW`, and a helper that wants that key.
async fn keyed(version: &str) -> (Helper, String, PlaybackLibrary) {
    let helper = Helper::new();
    helper.set(|b| {
        b.key = Some(KEY.to_string());
        b.clock = NOW;
        let mut health = ok_health();
        health["version"] = json!(version);
        b.health = Some(health);
    });
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    library.set_key_for_test(KEY);
    library.set_wall_clock(Box::new(|| NOW));
    (helper, address, library)
}

#[tokio::test]
async fn every_ask_carries_the_time_and_the_signature_of_the_key_and_never_the_key() {
    let (helper, address, library) = keyed(BUNDLED).await;
    helper.set(|b| b.waveform = Some(helper_wave(0.25, &[0.5, 1.0])));
    helper.set(|b| {
        b.art = Some(art("image/jpeg", "\"a\"", jpeg(64)));
        for song in b.items.iter_mut() {
            let id = song["setlist_song_id"].as_i64().unwrap();
            song["artwork"] = artwork_info(id, "image/jpeg", "\"a\"", 64);
        }
    });
    let settings = settings_for(&address);
    let loaded = plan("2026-11-22", &["Song One", "Song Two"]);
    let status = library.status(&settings, false).await;
    assert_eq!(status.helper.state, HelperState::Ready, "{status:?}");
    let result = library.plan(&settings, Some(&loaded), false).await;
    assert_eq!(result.state, LibraryState::Ready);
    let id = result.songs[0]
        .library
        .as_ref()
        .unwrap()
        .setlist_song_id
        .unwrap();
    let art_info = result.songs[0]
        .library
        .as_ref()
        .unwrap()
        .artwork
        .clone()
        .unwrap();
    library
        .picture(&settings, id as u64, &art_info)
        .await
        .expect("the cover is asked for with a signature too");
    library
        .waveform(&settings, id as u64)
        .await
        .expect("and the waveform");
    let asked = helper.inner.lock().unwrap().signed.clone();
    assert!(asked.len() >= 5, "{asked:?}");
    for (time, sig, target) in &asked {
        assert_eq!(time.as_deref(), Some(NOW.to_string().as_str()), "{target}");
        let want = sign(KEY, target, NOW);
        assert_eq!(sig.as_deref(), Some(want.as_str()), "{target}");
        assert!(!target.contains(KEY), "the key is never in a URL");
    }
    let targets: Vec<&str> = asked.iter().map(|(_, _, t)| t.as_str()).collect();
    assert!(targets.contains(&"/health"));
    assert!(targets
        .iter()
        .any(|t| t.starts_with("/songs/")
            && t.contains("/waveform?points_per_second=8&clock=performed")));
    assert!(targets.iter().any(|t| t.ends_with("/artwork")));
}

#[tokio::test]
async fn a_helper_that_answers_to_the_key_is_ready_and_an_older_one_is_outdated() {
    let (_, address, library) = keyed(BUNDLED).await;
    let status = library.status(&settings_for(&address), false).await;
    assert_eq!(status.helper.state, HelperState::Ready);
    assert!(status.active && status.enabled);
    assert_eq!(status.helper.version.as_deref(), Some(BUNDLED));
    assert_eq!(status.helper.bundled_version, BUNDLED);
    assert!(status.helper.paired);
    assert_eq!(status.state, LibraryState::Ready);

    let (_, address, library) = keyed("0.2.9").await;
    let status = library.status(&settings_for(&address), false).await;
    assert_eq!(
        status.helper.state,
        HelperState::Outdated,
        "older than the bundled one"
    );
    assert!(status.active, "an outdated helper still feeds the library");
    assert_eq!(status.state, LibraryState::Ready);
}

#[tokio::test]
async fn a_helper_without_a_key_works_but_offers_the_update() {
    // The fake with no key answers everyone and has no `auth` field: helper 0.2.0.
    let helper = Helper::new();
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    library.set_key_for_test(KEY);
    let status = library.status(&settings_for(&address), false).await;
    assert_eq!(status.helper.state, HelperState::Outdated);
    assert!(status.active);
    assert!(
        status.helper.message.contains("can be updated"),
        "{}",
        status.helper.message
    );
}

#[tokio::test]
async fn a_helper_that_refuses_the_key_or_the_time_needs_the_command_again_and_says_nothing_louder()
{
    // Another key.
    let (helper, address, library) = keyed(BUNDLED).await;
    library.set_key_for_test("ANOTHER-key-ANOTHER-key-ANOTHER-key-0123");
    let status = library.status(&settings_for(&address), false).await;
    assert_eq!(status.helper.state, HelperState::NeedsKey);
    assert!(!status.active);
    assert_eq!(status.state, LibraryState::Off, "quiet");
    assert!(status.helper.message.contains("setup command again"));
    assert!(
        status.helper.paired,
        "there is a key, it is just not the one the helper has"
    );
    // The plan is empty and says nothing.
    let result = library
        .plan(
            &settings_for(&address),
            Some(&plan("2026-11-22", &["Song One"])),
            false,
        )
        .await;
    assert_eq!(
        (result.state, result.enabled, result.songs.len()),
        (LibraryState::Off, false, 0)
    );
    // The right key but a clock that is more than two minutes off.
    let (helper2, address2, library2) = keyed(BUNDLED).await;
    helper2.set(|b| b.clock = NOW + 500);
    let status = library2.status(&settings_for(&address2), false).await;
    assert_eq!(status.helper.state, HelperState::NeedsKey);
    drop(helper);
    // No key at all against a keyed helper: its bare answer says a key is required.
    let (_, address3, _) = keyed(BUNDLED).await;
    let bare = PlaybackLibrary::new();
    let status = bare.status(&settings_for(&address3), false).await;
    assert_eq!(status.helper.state, HelperState::NeedsKey);
    assert!(!status.helper.paired);
}

#[tokio::test]
async fn a_helper_that_refused_is_not_pressed_with_more_bad_signatures() {
    let (helper, address, library) = keyed(BUNDLED).await;
    library.set_key_for_test("ANOTHER-key-ANOTHER-key-ANOTHER-key-0123");
    let settings = settings_for(&address);
    let _ = library.status(&settings, true).await;
    assert_eq!(helper.count("/health"), 1);
    // The Check button asks again after two seconds, but not for a helper that has just said no.
    library.advance_clock(Duration::from_secs(3));
    let _ = library.status(&settings, true).await;
    assert_eq!(
        helper.count("/health"),
        1,
        "ten bad signatures a minute lock this address out"
    );
    library.advance_clock(Duration::from_secs(8));
    let _ = library.status(&settings, true).await;
    assert_eq!(helper.count("/health"), 2);
    // Once the key is right it is read again at the next ask.
    library.set_key_for_test(KEY);
    library.advance_clock(Duration::from_secs(11));
    let status = library.status(&settings, true).await;
    assert_eq!(status.helper.state, HelperState::Ready);
}

/// An older settings.json may hold `playback_library.enabled: false` (an off switch, or the "Forget the Library Link" that no
/// longer exists). It changes nothing: the state is what the helper's `/health` says, as with a file that has no such field.
#[tokio::test]
async fn an_old_file_with_the_switch_off_behaves_like_one_without_it() {
    let off = |address: &str| PlaybackLibrarySettings {
        enabled: Some(false),
        address: address.to_string(),
    };
    // 1. A helper that is not installed: not installed, and no error.
    let nothing = PlaybackLibrary::new();
    let status = nothing.status(&off("http://127.0.0.1:9"), true).await;
    assert_eq!(status.helper.state, HelperState::NotInstalled);
    assert_eq!(
        status.helper.message,
        "The Library Link is not installed on the Playback computer."
    );
    // 2. A keyed helper that does not know this StagePilot (no key): it says so, like for any file.
    let (helper, address, _) = keyed(BUNDLED).await;
    let no_key = PlaybackLibrary::new();
    let status = no_key.status(&off(&address), true).await;
    assert_eq!(status.helper.state, HelperState::NeedsKey);
    assert!(!status.active);
    // 3. The key it was set up with: ready, and the saved `enabled: false` makes no difference to the answer.
    let (_, address, library) = keyed(BUNDLED).await;
    let with_off = library.status(&off(&address), true).await;
    let plain = PlaybackLibrary::new();
    plain.set_key_for_test(KEY);
    plain.set_wall_clock(Box::new(|| NOW));
    let without = plain.status(&settings_for(&address), true).await;
    assert_eq!(with_off.helper.state, HelperState::Ready);
    assert_eq!(with_off.helper, without.helper);
    assert_eq!(
        (with_off.active, with_off.state),
        (without.active, without.state)
    );
    assert!(!helper.hits().is_empty());
}

/// The same through the real routes: a settings.json from a version that could forget the key loads, and the Library Link
/// answers from `/health`.
#[tokio::test]
async fn a_settings_file_that_once_forgot_the_key_loads_and_reads_the_helper() {
    let (_, address, _) = keyed(BUNDLED).await;
    let file = json!({
        "schema_version": 2,
        "playback_library": {"enabled": false, "address": format!("http://{address}")},
    });
    let (settings, _) = PersistentSettings::from_value(file).expect("the old file loads");
    let service = SettingsService::ephemeral_persistent(settings);
    let runtime = Runtime::with_backends(Arc::new(service), false, OutputBackends::fake_midi());
    runtime.start().await;
    let router = build_app(runtime.clone(), AppOptions { web_root: None });
    // A key StagePilot has not made yet: the helper is there but does not know this StagePilot.
    let (status, body) = call(&router, "GET", "/api/v1/playback-library/status", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["helper"]["state"], json!("needs_key"));
    assert_eq!(body["helper"]["paired"], json!(false));
    // The setup command makes the key (and reuses it every time after); the helper then answers to it.
    runtime.playback_library.set_key_for_test(KEY);
    runtime.playback_library.set_wall_clock(Box::new(|| NOW));
    runtime.playback_library.reset().await;
    let (_, body) = call(
        &router,
        "GET",
        "/api/v1/playback-library/status?check=1",
        None,
    )
    .await;
    assert_eq!(body["helper"]["state"], json!("ready"));
    assert_eq!(body["helper"]["paired"], json!(true));
    // There is no way to forget the key any more.
    let (status, _) = call(&router, "POST", "/api/v1/playback-library/forget", None).await;
    assert!(status.is_client_error(), "{status}");
    assert!(runtime.playback_library.is_paired());
}

fn route_helper_settings(runtime: &Runtime, host: Option<&str>, address: &str) {
    let mut next = runtime.settings_service.snapshot();
    next.playback_api.host = host.map(str::to_string);
    next.playback_library.address = address.to_string();
    runtime.settings_service.save(next).unwrap();
}

#[tokio::test]
async fn the_helper_is_asked_at_the_saved_playback_computer_unless_advanced_names_another() {
    let (_, runtime) = app().await;
    let address_of = |runtime: &Runtime| {
        stagepilot_server::playback_library::effective_settings(runtime).address
    };
    assert_eq!(
        address_of(&runtime),
        "",
        "no Playback computer, no helper address"
    );
    route_helper_settings(&runtime, Some("10.30.0.141"), "");
    assert_eq!(address_of(&runtime), "http://10.30.0.141");
    assert_eq!(
        base_url(&address_of(&runtime)).as_deref(),
        Some("http://10.30.0.141:8780"),
        "the helper's own port"
    );
    route_helper_settings(&runtime, Some("10.20.0.102"), "");
    assert_eq!(address_of(&runtime), "http://10.20.0.102");
    // Advanced: another address wins.
    route_helper_settings(&runtime, Some("10.30.0.141"), "http://10.20.0.102:9000");
    assert_eq!(address_of(&runtime), "http://10.20.0.102:9000");
    // Blank host text is no host.
    route_helper_settings(&runtime, Some("  "), "");
    assert_eq!(address_of(&runtime), "");
}

#[tokio::test]
async fn the_status_route_remembers_the_helper_was_in_use_and_a_later_silence_is_said() {
    let (helper, address, library) = keyed(BUNDLED).await;
    let (router, runtime) = app().await;
    runtime.playback_library.set_key_for_test(KEY);
    runtime.playback_library.set_wall_clock(Box::new(|| NOW));
    drop(library);
    route_helper_settings(&runtime, None, &format!("http://{address}"));
    let (_, body) = call(&router, "GET", "/api/v1/playback-library/status", None).await;
    assert_eq!(body["helper"]["state"], json!("ready"), "{body}");
    assert_eq!(body["helper"]["version"], json!(BUNDLED));
    assert_eq!(body["helper"]["bundled_version"], json!(BUNDLED));
    assert_eq!(body["helper"]["paired"], json!(true));
    assert_eq!(body["active"], json!(true));
    assert_eq!(
        runtime.settings_service.snapshot().playback_library.enabled,
        Some(true),
        "saved the first time the helper answers with the key"
    );
    // Silent now: after the minute the last good answer stands, it is said.
    helper.set(|b| b.broken = Some(500));
    runtime
        .playback_library
        .advance_clock(Duration::from_secs(120));
    let (_, body) = call(
        &router,
        "GET",
        "/api/v1/playback-library/status?check=1",
        None,
    )
    .await;
    assert_eq!(body["helper"]["state"], json!("unreachable"), "{body}");
    assert_eq!(body["active"], json!(false));
    assert_eq!(body["state"], json!("problem"));
}

#[tokio::test]
async fn a_helper_that_was_never_there_says_nothing_anywhere() {
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let (router, runtime) = app().await;
    route_helper_settings(&runtime, None, &format!("http://127.0.0.1:{closed}"));
    let (_, body) = call(&router, "GET", "/api/v1/playback-library/status", None).await;
    assert_eq!(body["helper"]["state"], json!("not_installed"), "{body}");
    assert_eq!(
        (
            body["enabled"].clone(),
            body["active"].clone(),
            body["state"].clone()
        ),
        (json!(false), json!(false), json!("off"))
    );
    let (_, plan_body) = call(&router, "GET", "/api/v1/playback-library/plan", None).await;
    assert_eq!(
        (
            plan_body["enabled"].clone(),
            plan_body["state"].clone(),
            plan_body["songs"].clone()
        ),
        (json!(false), json!("off"), json!([]))
    );
    assert_eq!(
        runtime.settings_service.snapshot().playback_library.enabled,
        None,
        "nothing was written"
    );
}

#[tokio::test]
async fn the_key_is_in_no_answer_no_setting_and_no_log_line() {
    let (_, address, _) = keyed(BUNDLED).await;
    let (router, runtime) = app().await;
    runtime.playback_library.set_key_for_test(KEY);
    runtime.playback_library.set_wall_clock(Box::new(|| NOW));
    route_helper_settings(&runtime, Some("127.0.0.1"), &format!("http://{address}"));
    runtime
        .store
        .mutate(|s| s.plan = Some(plan("2026-11-22", &["Song One"])));
    let mut seen = String::new();
    for (method, path) in [
        ("GET", "/api/v1/playback-library/status?check=1"),
        ("GET", "/api/v1/playback-library/plan?check=1"),
        ("GET", "/api/v1/settings"),
        ("GET", "/api/v1/state"),
        ("GET", "/api/v1/health"),
        ("PUT", "/api/v1/playback-library/settings"),
    ] {
        let body = (method == "PUT").then(|| json!({"address": format!("http://{address}")}));
        let (_, answer) = call(&router, method, path, body).await;
        seen.push_str(&answer.to_string());
    }
    seen.push_str(&serde_json::to_string(&runtime.settings_service.snapshot()).unwrap());
    seen.push_str(
        &runtime
            .settings_service
            .snapshot()
            .to_file_value()
            .to_string(),
    );
    seen.push_str(&format!("{:?}", runtime.settings_service.snapshot()));
    let with_report =
        stagepilot_server::diagnostics::with_playback_section("{\"app\": 1}", &runtime);
    seen.push_str(&with_report);
    assert!(!seen.contains(KEY), "the key leaked");
    assert!(!seen.to_lowercase().contains("x-pbl-sig"));
    // The shipped redaction masks it by name too, wherever a log line names it.
    assert!(
        !stagepilot_core::redact::redact_text(&format!("helper_key={KEY} x-pbl-sig: {KEY}"))
            .contains(KEY)
    );
}

// ---- the matchup: how sure each pair is, songs only Playback has, and the setlist chooser -------------

fn other_setlist(helper: &Helper) {
    helper.set(|b| {
        b.others = vec![
            (
                7,
                "Saturday Night".into(),
                "2026-11-21".into(),
                vec![
                    item(31, 1, "Song One"),
                    item(32, 2, "Extra Song"),
                    item(33, 3, "Song Three (Feat. Guest)"),
                ],
            ),
            (
                6,
                "Old Set".into(),
                "2026-11-08".into(),
                vec![item(41, 1, "Song One")],
            ),
        ];
    });
}

#[tokio::test]
async fn the_plan_says_how_sure_each_pair_is_and_which_songs_only_playback_has() {
    let helper = Helper::new();
    helper.set(|b| {
        b.items = vec![
            item(11, 1, "Song One"),
            item(12, 2, "Way Maker"),
            item(13, 3, "Song Three (Live at Home)"),
            item(14, 4, "Only Here"),
        ];
    });
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let mut sunday = plan(
        "2026-11-22",
        &["Song One", "Waymaker", "Song Three Reprise", "Missing One"],
    );
    sunday.songs[0].key_name = Some("A".into());
    let result = library
        .plan(&settings_for(&address), Some(&sunday), false)
        .await;
    assert_eq!(result.state, LibraryState::Ready);
    let seen: Vec<(PlanMatch, Option<MatchConfidence>)> = result
        .songs
        .iter()
        .map(|s| (s.status, s.confidence))
        .collect();
    assert_eq!(
        seen,
        [
            (PlanMatch::Matched, Some(MatchConfidence::Exact)),
            (PlanMatch::Uncertain, Some(MatchConfidence::Uncertain)),
            (PlanMatch::Uncertain, Some(MatchConfidence::Uncertain)),
            (PlanMatch::Missing, None),
        ]
    );
    // An uncertain pair names the Playback song but never hands it over as the song's library.
    assert!(result.songs[1].library.is_none());
    assert_eq!(
        result.songs[1].candidate.as_ref().map(|c| c.title.as_str()),
        Some("Way Maker")
    );
    let only: Vec<&str> = result
        .playback_only
        .iter()
        .map(|s| s.title.as_str())
        .collect();
    assert_eq!(only, ["Only Here"]);
    assert!(!result.chosen);
    assert_eq!(result.songs[0].plan_key.as_deref(), Some("A"));
    assert_eq!(result.songs[1].plan_key, None);
    assert!(
        result.setlists.is_empty(),
        "no chooser while the date has a setlist"
    );
}

#[tokio::test]
async fn with_no_setlist_on_the_date_the_other_setlists_are_offered_by_number_name_date_and_song_count(
) {
    let helper = Helper::new();
    other_setlist(&helper);
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
    assert!(
        result.songs.is_empty() && result.setlist.is_none(),
        "nothing is paired until the operator chooses"
    );
    let offered: Vec<(Option<i64>, &str, Option<&str>, u32)> = result
        .setlists
        .iter()
        .map(|s| (s.id, s.title.as_str(), s.date.as_deref(), s.songs))
        .collect();
    assert_eq!(
        offered,
        [
            (Some(1), "Set For Test", Some("2026-11-22"), 3),
            (Some(7), "Saturday Night", Some("2026-11-21"), 3),
            (Some(6), "Old Set", Some("2026-11-08"), 1),
        ]
    );
}

#[tokio::test]
async fn a_chosen_setlist_is_used_for_that_plan_only_and_null_goes_back_to_the_date() {
    let helper = Helper::new();
    other_setlist(&helper);
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let settings = settings_for(&address);
    let sunday = plan("2026-11-29", &["Song One", "Song Three"]);
    assert_eq!(
        library.plan(&settings, Some(&sunday), false).await.state,
        LibraryState::NoSetlist
    );

    library.choose(&sunday.id, Some(7));
    let chosen = library.plan(&settings, Some(&sunday), true).await;
    assert_eq!(chosen.state, LibraryState::Ready);
    assert!(chosen.chosen);
    assert_eq!(
        chosen.setlist.as_ref().map(|s| (s.id, s.title.as_str())),
        Some((Some(7), "Saturday Night"))
    );
    let seen: Vec<(PlanMatch, Option<MatchConfidence>)> = chosen
        .songs
        .iter()
        .map(|s| (s.status, s.confidence))
        .collect();
    assert_eq!(
        seen,
        [
            (PlanMatch::Matched, Some(MatchConfidence::Exact)),
            (PlanMatch::Matched, Some(MatchConfidence::Close))
        ]
    );
    assert_eq!(
        chosen
            .playback_only
            .iter()
            .map(|s| s.title.as_str())
            .collect::<Vec<_>>(),
        ["Extra Song"]
    );
    assert_eq!(
        chosen.setlists.len(),
        3,
        "the chooser stays so the choice can be changed"
    );
    assert!(helper.hits().contains(&"/setlists/7".to_string()));

    // Another plan has its own say: it is read by its date, and the choice is still there for the first.
    let other = ServicePlan {
        id: "plan-2".into(),
        ..plan("2026-11-29", &["Song One"])
    };
    assert_eq!(
        library.plan(&settings, Some(&other), true).await.state,
        LibraryState::NoSetlist
    );
    assert_eq!(library.chosen_for("plan-1"), Some(7));
    assert_eq!(library.chosen_for("plan-2"), None);

    library.choose(&sunday.id, None);
    let back = library.plan(&settings, Some(&sunday), true).await;
    assert_eq!(back.state, LibraryState::NoSetlist);
    assert!(!back.chosen);
}

#[tokio::test]
async fn a_chosen_setlist_that_playback_no_longer_has_is_forgotten() {
    let helper = Helper::new();
    other_setlist(&helper);
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let settings = settings_for(&address);
    let sunday = plan("2026-11-29", &["Song One"]);
    library.choose(&sunday.id, Some(99));
    let result = library.plan(&settings, Some(&sunday), true).await;
    assert_eq!(result.state, LibraryState::NoSetlist);
    assert!(!result.chosen);
    assert_eq!(
        library.chosen_for(&sunday.id),
        None,
        "the next read goes by the date again"
    );
}

#[tokio::test]
async fn a_new_address_forgets_the_choice_because_the_numbers_belong_to_the_old_helper() {
    let library = PlaybackLibrary::new();
    library.choose("plan-1", Some(7));
    library.reset().await;
    assert_eq!(library.chosen_for("plan-1"), None);
}

#[tokio::test]
async fn the_setlist_route_chooses_and_changes_nothing_that_is_saved() {
    let helper = Helper::new();
    other_setlist(&helper);
    let address = helper.serve().await;
    let (router, runtime) = app().await;
    let _ = call(
        &router,
        "PUT",
        "/api/v1/playback-library/settings",
        Some(json!({"address": address})),
    )
    .await;
    let before = (
        runtime.settings_service.snapshot().playback_api,
        runtime.settings_service.snapshot().playback_library.address,
    );

    // Nothing but a whole number or null is a choice.
    for bad in [
        json!({}),
        json!({"setlist_id": "7"}),
        json!({"setlist_id": -3}),
        json!({"setlist_id": 1.5}),
    ] {
        let (status, _) = call(
            &router,
            "PUT",
            "/api/v1/playback-library/setlist",
            Some(bad.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
    let (status, body) = call(
        &router,
        "PUT",
        "/api/v1/playback-library/setlist",
        Some(json!({"setlist_id": 7})),
    )
    .await;
    // The demo plan is loaded: either it has songs (and the choice is read) or it says so in words; never a 5xx.
    assert!(
        status == StatusCode::OK || status == StatusCode::CONFLICT,
        "{status} {body}"
    );
    if status == StatusCode::OK {
        assert!(
            matches!(body["state"].as_str(), Some("ready" | "no_setlist")),
            "{body}"
        );
    }
    let (status, _) = call(
        &router,
        "PUT",
        "/api/v1/playback-library/setlist",
        Some(json!({"setlist_id": null})),
    )
    .await;
    assert!(status == StatusCode::OK || status == StatusCode::CONFLICT);
    assert_eq!(
        (
            runtime.settings_service.snapshot().playback_api,
            runtime.settings_service.snapshot().playback_library.address
        ),
        before
    );
}

// ---- a key changed in Playback ----------------------------------------------------------------

const TITLES: [&str; 3] = ["Song One", "Song Two", "Song Three"];

/// What the fake says about song `id` once Playback's key for it is `key`.
fn set_key(helper: &Helper, id: i64, key: &str) {
    helper.set(|b| {
        for item in &mut b.items {
            if item["setlist_song_id"] == json!(id) {
                item["key"] = json!({"id": 9, "name": key});
            }
        }
    });
}

fn first_song(plan: &LibraryPlan) -> &stagepilot_playback::library::LibrarySong {
    plan.songs[0].library.as_ref().expect("song one is paired")
}

#[tokio::test]
async fn a_freshly_paired_setlist_has_no_key_change_and_a_changed_key_says_what_it_was() {
    let helper = Helper::new();
    set_key(&helper, 11, "G");
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let settings = settings_for(&address);
    let first = ask(&library, &settings, &TITLES).await;
    for song in &first.songs {
        let library = song.library.as_ref().unwrap();
        assert_eq!((&library.key_was, &library.key_changed_at), (&None, &None));
    }
    // Playback changes the key of song one to Ab; the fake answers it on the next read.
    set_key(&helper, 11, "Ab");
    let changed = ask(&library, &settings, &TITLES).await;
    let song = first_song(&changed);
    assert_eq!(song.key.as_deref(), Some("Ab"));
    assert_eq!(song.key_was.as_deref(), Some("G"));
    let at = chrono::DateTime::parse_from_rfc3339(song.key_changed_at.as_deref().unwrap()).unwrap();
    assert!((chrono::Utc::now().timestamp() - at.timestamp()).abs() < 120);
    assert!(song.key_changed_at.as_deref().unwrap().ends_with('Z'));
    // The other songs did not change.
    assert!(changed.songs[1].library.as_ref().unwrap().key_was.is_none());
    // It stays recorded, with the same time, while the key stays.
    let again = ask(&library, &settings, &TITLES).await;
    assert_eq!(first_song(&again).key_was.as_deref(), Some("G"));
    assert_eq!(first_song(&again).key_changed_at, song.key_changed_at);
    // The JSON a window gets says it.
    let json = serde_json::to_value(&again).unwrap();
    assert_eq!(json["songs"][0]["library"]["key_was"], json!("G"));
    assert_eq!(json["songs"][1]["library"]["key_was"], Value::Null);
}

#[tokio::test]
async fn a_key_changed_back_is_a_new_change() {
    let helper = Helper::new();
    set_key(&helper, 11, "G");
    let address = helper.serve().await;
    let library = PlaybackLibrary::new();
    let settings = settings_for(&address);
    let _ = ask(&library, &settings, &TITLES).await;
    set_key(&helper, 11, "Ab");
    let _ = ask(&library, &settings, &TITLES).await;
    set_key(&helper, 11, "G");
    let back = ask(&library, &settings, &TITLES).await;
    assert_eq!(first_song(&back).key.as_deref(), Some("G"));
    assert_eq!(first_song(&back).key_was.as_deref(), Some("Ab"));
}

#[tokio::test]
async fn a_restart_keeps_a_key_change_and_an_ordinary_read_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("playback-keys.json");
    let helper = Helper::new();
    set_key(&helper, 11, "G");
    let address = helper.serve().await;
    let settings = settings_for(&address);
    let library = PlaybackLibrary::new().with_key_memory_file(&file);
    let _ = ask(&library, &settings, &TITLES).await;
    assert!(file.exists(), "the first sight of the songs is saved");
    set_key(&helper, 11, "Ab");
    let _ = ask(&library, &settings, &TITLES).await;
    // Plain reads that see the same keys leave the file alone.
    std::fs::remove_file(&file).unwrap();
    let _ = ask(&library, &settings, &TITLES).await;
    let _ = ask(&library, &settings, &TITLES).await;
    assert!(!file.exists());
    drop(library);
    // Start again from a file with the change in it.
    let library = PlaybackLibrary::new().with_key_memory_file(&file);
    set_key(&helper, 11, "G");
    let _ = ask(&library, &settings, &TITLES).await;
    set_key(&helper, 11, "Ab");
    let _ = ask(&library, &settings, &TITLES).await;
    drop(library);
    let restarted = PlaybackLibrary::new().with_key_memory_file(&file);
    let plan = ask(&restarted, &settings, &TITLES).await;
    assert_eq!(first_song(&plan).key.as_deref(), Some("Ab"));
    assert_eq!(first_song(&plan).key_was.as_deref(), Some("G"));
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        // The setlist's date is a field of its own (`"2026-11-22"` in quotes); a saved time ("2026-11-22T..") is allowed to fall on the same
        // day as the fixture's date, which it does on that very day.
        !text.contains("Song One") && !text.contains("\"2026-11-22\""),
        "only numbers, keys and times: {text}"
    );
}

#[tokio::test]
async fn a_damaged_key_file_is_ignored_and_a_reset_clears_what_was_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("playback-keys.json");
    std::fs::write(&file, "][ not json").unwrap();
    let helper = Helper::new();
    let address = helper.serve().await;
    let library = PlaybackLibrary::new().with_key_memory_file(&file);
    let first = ask(&library, &settings_for(&address), &TITLES).await;
    assert_eq!(first.state, LibraryState::Ready);
    assert!(first_song(&first).key_was.is_none());
    // Resetting the Library Link (a new key) clears what was remembered.
    library.reset().await;
    assert!(!file.exists());
}

/// What the real helper (`playback_library.py`) answers: 503 with `{"error": <the LookupError text>}`.
fn helper_error(text: &str) -> String {
    json!({ "error": text }).to_string()
}

#[tokio::test]
async fn a_song_the_helper_has_no_waveform_for_is_a_404_not_a_connection_fault() {
    for (what, text) in [
        (
            "no folder",
            "this song's folder was not found in Playback's Songs folder",
        ),
        ("no pictures", "no waveform pictures stored for this song"),
    ] {
        let (helper, router, runtime) = waves(None).await;
        helper.set(|b| {
            b.wave_status = Some(503);
            b.wave_body = Some(helper_error(text));
        });
        let (status, _, body) = raw(&router, WAVE_ONE, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{what}");
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({"detail": "No waveform."}),
            "{what}"
        );
        assert_eq!(helper.count("/songs/11/waveform"), 1, "{what}");
        assert_eq!(runtime.playback_library.waveforms_held(), 0, "{what}");
    }
}

#[tokio::test]
async fn a_helper_that_cannot_read_playbacks_library_is_still_a_503() {
    let cannot = helper_error("the Playback library is not readable");
    let long = format!(
        "{{\"error\": \"no waveform pictures stored{}\"}}",
        " x".repeat(2000)
    );
    for (what, status, text) in [
        ("other words", 503, cannot.clone()),
        (
            "not JSON",
            503,
            "<html>folder was not found</html>".to_string(),
        ),
        (
            "no error field",
            503,
            "{\"detail\": \"no waveform pictures stored\"}".to_string(),
        ),
        ("error is not text", 503, "{\"error\": 3}".to_string()),
        ("an answer too long to be the helper's", 503, long),
        (
            "another status with the same words",
            500,
            helper_error("no waveform pictures stored for this song"),
        ),
    ] {
        let (helper, router, runtime) = waves(None).await;
        helper.set(|b| {
            b.wave_status = Some(status);
            b.wave_body = Some(text);
        });
        let (code, _, body) = raw(&router, WAVE_ONE, None).await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{what}");
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({"detail": "The Playback computer did not answer."}),
            "{what}"
        );
        assert_eq!(runtime.playback_library.waveforms_held(), 0, "{what}");
    }
}
