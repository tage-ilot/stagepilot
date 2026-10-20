//! The Lightkey link against a fake `lightkey-api`: ported from `backend/tests/test_lightkey_link.py`.
//!
//! The fake agent is a real axum HTTP server on loopback (the Python test uses an httpx mock
//! transport) that reports cue changes with a timestamp on ITS clock, which runs a fixed offset
//! ahead of ours, so the clock-offset conversion is exercised too. The link uses its real
//! `reqwest` client and the real clock; OSC goes to an in-memory recorder (nothing is sent
//! anywhere). Mapping: `docs/rust-migration/lightkey-test-map.md`.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::{json, Value};
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::LightkeySettings;
use stagepilot_core::events::{new_event, EventPayload, EventType, TimerPayload};
use stagepilot_core::state::Song;
use stagepilot_lights::lightkey::{LightkeyEdge, LightkeyLink as Link, LightkeyTake};
use stagepilot_lights::lightkey_link::{
    FileTakeStore, LightkeyLink, LinkOptions, LinkSettings, MemoryTakeStore, TakeStore,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The Lightkey Mac's clock runs this far ahead of ours.
const OFFSET_MS: f64 = 5_000.0;

fn now_ms() -> f64 {
    let now = Utc::now();
    (now.timestamp() as f64 + f64::from(now.timestamp_subsec_micros()) / 1e6) * 1000.0 + OFFSET_MS
}

struct Agent {
    online: bool,
    active: Vec<String>,
    seq: i64,
    events: Vec<Value>,
    status_code: u16,
    /// Extra delay before a `/time` answer, per call index (lowest-RTT test).
    time_delay_ms: Vec<u64>,
    time_calls: usize,
    log_calls: usize,
    log_delay_ms: u64,
}

type Shared = Arc<Mutex<Agent>>;

fn agent_lock(agent: &Shared) -> std::sync::MutexGuard<'_, Agent> {
    agent.lock().unwrap_or_else(|p| p.into_inner())
}

impl Agent {
    fn new(active: &[&str]) -> Self {
        let mut active: Vec<String> = active.iter().map(ToString::to_string).collect();
        active.sort();
        Self {
            online: true,
            active,
            seq: 1,
            events: Vec::new(),
            status_code: 200,
            time_delay_ms: Vec::new(),
            time_calls: 0,
            log_calls: 0,
            log_delay_ms: 0,
        }
    }

    fn change(&mut self, added: &[&str], removed: &[&str]) {
        let mut active: std::collections::BTreeSet<String> = self.active.iter().cloned().collect();
        for name in removed {
            active.remove(*name);
        }
        active.extend(added.iter().map(ToString::to_string));
        self.active = active.into_iter().collect();
        self.seq += 1;
        let mut added: Vec<String> = added.iter().map(ToString::to_string).collect();
        added.sort();
        let mut removed: Vec<String> = removed.iter().map(ToString::to_string).collect();
        removed.sort();
        self.events.push(json!({
            "seq": self.seq, "online": true, "added": added, "removed": removed,
            "effective_ms": now_ms(), "effective_src": "dmx",
        }));
    }
}

async fn time_handler(State(agent): State<Shared>) -> Response {
    let (status, delay) = {
        let mut a = agent_lock(&agent);
        let index = a.time_calls;
        a.time_calls += 1;
        (
            a.status_code,
            a.time_delay_ms.get(index).copied().unwrap_or(0),
        )
    };
    if delay > 0 {
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
    if status != 200 {
        return (StatusCode::from_u16(status).unwrap(), "{\"error\":\"no\"}").into_response();
    }
    axum::Json(json!({"t": now_ms()})).into_response()
}

async fn log_handler(
    State(agent): State<Shared>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let delay = {
        let mut a = agent_lock(&agent);
        a.log_calls += 1;
        a.log_delay_ms
    };
    if delay > 0 {
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
    let a = agent_lock(&agent);
    if a.status_code != 200 {
        return (
            StatusCode::from_u16(a.status_code).unwrap(),
            "{\"error\":\"no\"}",
        )
            .into_response();
    }
    let since: i64 = query.get("since").and_then(|s| s.parse().ok()).unwrap_or(0);
    let events: Vec<&Value> = a
        .events
        .iter()
        .filter(|e| e["seq"].as_i64().unwrap_or(0) > since)
        .collect();
    axum::Json(json!({
        "events": events,
        "state": {
            "online": a.online,
            "active": if a.online { json!(a.active) } else { Value::Null },
            "seq": a.seq,
            "server_time": now_ms(),
        },
    }))
    .into_response()
}

async fn serve(agent: Shared) -> u16 {
    let app = Router::new()
        .route("/time", get(time_handler))
        .route("/log", get(log_handler))
        .with_state(agent);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    port
}

/// A loopback port whose listener accepts every connection and drops it at once: the link sees a
/// connection that dies without an answer (never a timeout), the same on every OS. (A closed
/// port is not usable here: Windows retries a refused connect for about 2 s, which is the link's
/// own connect timeout.)
async fn dead_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            drop(socket);
        }
    });
    port
}

struct Rig {
    agent: Shared,
    port: u16,
    bus: EventBus,
    sent: Arc<Mutex<Vec<(f64, String)>>>,
    t0: Arc<Mutex<Instant>>,
    settings: Arc<Mutex<LinkSettings>>,
    store: Arc<MemoryTakeStore>,
    link: LightkeyLink,
}

impl Rig {
    async fn new(active: &[&str]) -> Self {
        Self::with(active, true).await
    }

    async fn with(active: &[&str], replay: bool) -> Self {
        let agent: Shared = Arc::new(Mutex::new(Agent::new(active)));
        let port = serve(agent.clone()).await;
        let settings = Arc::new(Mutex::new((
            true,
            true,
            "10.30.0.211".to_string(),
            LightkeySettings {
                api_url: format!("http://127.0.0.1:{port}"),
                replay,
            },
        )));
        let bus = EventBus::new();
        let sent: Arc<Mutex<Vec<(f64, String)>>> = Arc::default();
        let t0 = Arc::new(Mutex::new(Instant::now()));
        let store = Arc::new(MemoryTakeStore::new());
        let (sent_in, t0_in) = (sent.clone(), t0.clone());
        let send: stagepilot_lights::lightkey_link::SendOsc = Arc::new(move |address: String| {
            let (sent, t0) = (sent_in.clone(), t0_in.clone());
            Box::pin(async move {
                let at = t0.lock().unwrap().elapsed().as_secs_f64();
                sent.lock().unwrap().push((at, address));
                Ok(())
            })
        });
        let provider_settings = settings.clone();
        let provider: stagepilot_lights::lightkey_link::SettingsProvider =
            Arc::new(move || provider_settings.lock().unwrap().clone());
        let link = LightkeyLink::new(
            bus.clone(),
            provider,
            send,
            LinkOptions {
                store: store.clone() as Arc<dyn TakeStore>,
                ..LinkOptions::default()
            },
        );
        Self {
            agent,
            port,
            bus,
            sent,
            t0,
            settings,
            store,
            link,
        }
    }

    fn addresses(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|(_, a)| a.clone())
            .collect()
    }

    fn sent_at(&self, index: usize) -> f64 {
        self.sent.lock().unwrap()[index].0
    }

    fn set_settings(&self, edit: impl FnOnce(&mut LinkSettings)) {
        edit(&mut self.settings.lock().unwrap());
    }

    async fn connect(&self) {
        self.link.start().await;
        wait_for(|| self.link.status().link == Link::Connected, 3.0).await;
    }

    async fn song_start(&self, song: &Song, at: Option<DateTime<Utc>>) {
        self.bus
            .publish(new_event(
                EventType::SongStarted,
                "t",
                EventPayload::Song {
                    song: song.clone(),
                    index: 0,
                },
            ))
            .await;
        self.bus
            .publish(new_event(
                EventType::TimerStarted,
                "t",
                EventPayload::Timer(TimerPayload {
                    duration_seconds: song.duration_seconds.unwrap_or(0),
                    song: Some(song.clone()),
                    started_at: Some(at.unwrap_or_else(Utc::now)),
                    ..TimerPayload::default()
                }),
            ))
            .await;
    }

    async fn song_stop(&self, song: &Song, position: f64) {
        self.bus
            .publish(new_event(
                EventType::TimerStopped,
                "t",
                EventPayload::Timer(TimerPayload {
                    duration_seconds: 0,
                    song: Some(song.clone()),
                    position_seconds: Some(position),
                    remaining_seconds: Some(0),
                    ..TimerPayload::default()
                }),
            ))
            .await;
    }

    async fn seek(&self, song: &Song, position: f64, duration: u32) {
        self.bus
            .publish(new_event(
                EventType::TimerSeeked,
                "t",
                EventPayload::Timer(TimerPayload {
                    duration_seconds: duration,
                    song: Some(song.clone()),
                    position_seconds: Some(position),
                    remaining_seconds: Some(duration),
                    running: true,
                    started_at: Some(Utc::now()),
                    ..TimerPayload::default()
                }),
            ))
            .await;
    }
}

async fn wait_for(check: impl Fn() -> bool, timeout: f64) {
    let end = Instant::now() + Duration::from_secs_f64(timeout);
    while Instant::now() < end {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condition not reached in time");
}

async fn pause(seconds: f64) {
    tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
}

fn song(duration: u32) -> Song {
    Song {
        id: "item-1".into(),
        title: "Holy Forever".into(),
        duration_seconds: Some(duration),
        order: 1,
        service_sequence: None,
        is_generic: false,
        source_song_id: Some("song-1".into()),
        description: None,
        formatted_duration: None,
    }
}

fn names(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

fn stored_take() -> LightkeyTake {
    LightkeyTake::new(
        "song-1".into(),
        "Holy Forever".into(),
        Utc::now(),
        10.0,
        names(&["Warm"]),
        vec![
            LightkeyEdge::new(0.5, names(&["Stars"]), vec![]).unwrap(),
            LightkeyEdge::new(1.0, names(&["Strobe"]), names(&["Stars"])).unwrap(),
        ],
    )
    .unwrap()
}

fn with_takes(rig: &Rig, takes: Vec<LightkeyTake>) {
    rig.store
        .set(takes.into_iter().map(|t| (t.song_key.clone(), t)).collect());
}

// ----------------------------------------------------------------------------- link status

#[tokio::test(flavor = "multi_thread")]
async fn connects_and_reports_status() {
    let rig = Rig::new(&["A", "B"]).await;
    rig.link.start().await;
    wait_for(|| rig.link.status().link == Link::Connected, 3.0).await;
    let status = rig.link.status();
    assert_eq!(status.link, Link::Connected);
    assert_eq!(status.active_count, Some(2));
    assert_eq!(status.api_url, format!("http://127.0.0.1:{}", rig.port));
    assert!(status.replay);
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn api_url_is_derived_from_the_osc_host_when_unset() {
    let rig = Rig::new(&["A"]).await;
    rig.set_settings(|s| s.3.api_url = String::new());
    assert_eq!(rig.link.status().api_url, "http://10.30.0.211:8770");
}

#[tokio::test(flavor = "multi_thread")]
async fn unreachable_then_recovers() {
    let rig = Rig::new(&["A"]).await;
    let live = rig.settings.lock().unwrap().3.api_url.clone();
    let dead = format!("http://127.0.0.1:{}", dead_port().await);
    rig.set_settings(|s| s.3.api_url = dead);
    rig.link.start().await;
    wait_for(|| rig.link.status().link == Link::Unreachable, 3.0).await;
    assert_eq!(
        rig.link.status().detail.as_deref(),
        Some("Cannot reach the Lightkey API.")
    );
    rig.set_settings(|s| s.3.api_url = live);
    rig.link.settings_changed();
    wait_for(|| rig.link.status().link == Link::Connected, 5.0).await;
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn refused_address_says_how_to_fix_it() {
    let rig = Rig::new(&["A"]).await;
    agent_lock(&rig.agent).status_code = 403;
    rig.link.start().await;
    wait_for(|| rig.link.status().link == Link::Unreachable, 3.0).await;
    let detail = rig.link.status().detail.unwrap_or_default();
    assert_eq!(
        detail,
        "Lightkey API refused this address. Allow the wired interface in its config."
    );
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_silent_agent_is_a_timeout_not_a_refusal() {
    let rig = Rig::new(&["A"]).await;
    agent_lock(&rig.agent).time_delay_ms = vec![3_000];
    rig.link.start().await;
    wait_for(|| rig.link.status().link == Link::Unreachable, 5.0).await;
    assert_eq!(
        rig.link.status().detail.as_deref(),
        Some("Lightkey API did not answer.")
    );
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn lightkey_closed_is_its_own_state() {
    let rig = Rig::new(&["A"]).await;
    agent_lock(&rig.agent).online = false;
    rig.link.start().await;
    wait_for(|| rig.link.status().link == Link::LightkeyClosed, 3.0).await;
    assert_eq!(
        rig.link.status().detail.as_deref(),
        Some("Lightkey is not running.")
    );
    assert_eq!(rig.link.status().active_count, None);
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn lighting_off_reports_off() {
    let rig = Rig::new(&["A"]).await;
    rig.set_settings(|s| s.0 = false);
    rig.link.start().await;
    pause(0.3).await;
    let status = rig.link.status();
    assert_eq!(status.link, Link::Off);
    assert_eq!(status.replay_blocked.as_deref(), Some("Lighting is off."));
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn clock_offset_is_measured_with_seven_samples_on_connect() {
    let rig = Rig::new(&["A"]).await;
    rig.connect().await;
    pause(0.6).await;
    // Seven samples on connect, no refresh inside 120 s.
    assert_eq!(agent_lock(&rig.agent).time_calls, 7);
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn offset_uses_the_lowest_round_trip_sample() {
    // Six samples are slow (their midpoint is skewed by the delay); one is fast. A recording
    // made afterwards must still land on song time, which only holds with the fast sample.
    let rig = Rig::new(&["Warm"]).await;
    agent_lock(&rig.agent).time_delay_ms = vec![300, 300, 300, 0, 300, 300, 300];
    rig.connect().await;
    rig.link.set_recording(true).await;
    let s = song(10);
    rig.song_start(&s, None).await;
    pause(0.5).await;
    agent_lock(&rig.agent).change(&["Stars"], &[]);
    pause(0.6).await;
    rig.song_stop(&s, 10.0).await;
    wait_for(|| rig.link.has_take("song-1"), 3.0).await;
    let take = rig.store.takes()["song-1"].clone();
    assert!(
        (take.edges[0].at_seconds - 0.5).abs() < 0.25,
        "edge at {}",
        take.edges[0].at_seconds
    );
    rig.link.stop().await;
}

// ----------------------------------------------------------------------------- recording

#[tokio::test(flavor = "multi_thread")]
async fn cannot_record_when_not_connected_or_not_osc() {
    let rig = Rig::new(&["A"]).await;
    let live = rig.settings.lock().unwrap().3.api_url.clone();
    let dead = format!("http://127.0.0.1:{}", dead_port().await);
    rig.set_settings(|s| s.3.api_url = dead);
    rig.link.start().await;
    wait_for(|| rig.link.status().link == Link::Unreachable, 3.0).await;
    assert!(rig
        .link
        .set_recording(true)
        .await
        .contains("Connect to Lightkey first"));
    assert!(!rig.link.status().recording);
    rig.set_settings(|s| s.3.api_url = live);
    rig.link.settings_changed();
    wait_for(|| rig.link.status().link == Link::Connected, 5.0).await;
    rig.set_settings(|s| s.1 = false);
    assert!(rig.link.set_recording(true).await.contains("OSC"));
    assert!(!rig.link.status().recording);
    rig.set_settings(|s| s.0 = false);
    assert_eq!(
        rig.link.set_recording(true).await,
        "Turn Lighting on first."
    );
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn records_changes_with_song_times_and_saves_them() {
    let rig = Rig::new(&["Warm"]).await;
    rig.connect().await;
    rig.link.set_recording(true).await;
    assert!(rig.link.status().recording);
    let s = song(10);
    rig.song_start(&s, None).await;
    pause(0.5).await;
    agent_lock(&rig.agent).change(&["Stars"], &[]);
    pause(0.6).await;
    agent_lock(&rig.agent).change(&["Strobe"], &["Stars"]);
    pause(0.5).await;
    rig.song_stop(&s, 10.0).await; // played to the end
    wait_for(|| rig.link.has_take("song-1"), 3.0).await;
    let take = rig.store.takes()["song-1"].clone();
    assert_eq!(take.initial, names(&["Warm"]));
    let edges: Vec<_> = take
        .edges
        .iter()
        .map(|e| (e.added.clone(), e.removed.clone()))
        .collect();
    assert_eq!(
        edges,
        vec![
            (names(&["Stars"]), vec![]),
            (names(&["Strobe"]), names(&["Stars"]))
        ]
    );
    // Times are song time on OUR clock: the 5 s Lightkey-clock offset must have been removed.
    assert!((take.edges[0].at_seconds - 0.5).abs() < 0.25);
    assert!((take.edges[1].at_seconds - 1.1).abs() < 0.25);
    assert!(rig
        .link
        .status()
        .message
        .unwrap_or_default()
        .contains("Saved"));
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn short_recording_does_not_overwrite_a_good_take() {
    let rig = Rig::new(&["A"]).await;
    let good = LightkeyTake::new(
        "song-1".into(),
        "Holy Forever".into(),
        Utc::now(),
        10.0,
        names(&["A"]),
        vec![LightkeyEdge::new(2.0, names(&["B"]), vec![]).unwrap()],
    )
    .unwrap();
    with_takes(&rig, vec![good.clone()]);
    rig.connect().await;
    rig.link.set_recording(true).await;
    let s = song(10);
    rig.song_start(&s, None).await;
    rig.song_stop(&s, 1.0).await; // 10% of the song
    pause(0.1).await;
    assert_eq!(rig.store.takes()["song-1"], good);
    let status = rig.link.status();
    assert!(status.message.unwrap_or_default().contains("Paused") || status.recording);
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_recording_turned_off_short_says_not_saved() {
    let rig = Rig::new(&["A"]).await;
    rig.connect().await;
    rig.link.set_recording(true).await;
    rig.song_start(&song(100), None).await;
    pause(0.1).await;
    let message = rig.link.set_recording(false).await;
    assert!(message.starts_with("Not saved: only "), "{message}");
    assert!(
        message.ends_with("seconds of \"Holy Forever\" were recorded."),
        "{message}"
    );
    assert!(rig.store.takes().is_empty());
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn seek_while_recording_discards_the_take() {
    let rig = Rig::new(&["A"]).await;
    rig.connect().await;
    rig.link.set_recording(true).await;
    let s = song(10);
    rig.song_start(&s, None).await;
    rig.seek(&s, 7.0, 3).await;
    pause(0.1).await;
    let status = rig.link.status();
    assert_eq!(status.recording_song, None);
    assert!(status.message.unwrap_or_default().contains("moved"));
    assert!(rig.store.takes().is_empty());
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn timer_reset_while_recording_discards_the_take() {
    let rig = Rig::new(&["A"]).await;
    rig.connect().await;
    rig.link.set_recording(true).await;
    rig.song_start(&song(10), None).await;
    assert!(rig.link.status().recording_song.is_some());
    rig.bus
        .publish(new_event(
            EventType::TimerResetRequested,
            "t",
            EventPayload::Empty,
        ))
        .await;
    let status = rig.link.status();
    assert_eq!(status.recording_song, None);
    assert_eq!(
        status.message.as_deref(),
        Some("Recording discarded: the countdown was reset.")
    );
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn recording_turned_off_mid_song_saves_if_enough_was_captured() {
    let rig = Rig::new(&["A"]).await;
    rig.connect().await;
    rig.link.set_recording(true).await;
    let s = song(2);
    rig.song_start(&s, Some(Utc::now() - ChronoDuration::milliseconds(1500)))
        .await;
    pause(0.05).await;
    rig.link.set_recording(false).await;
    assert!(rig.link.has_take("song-1"));
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paused_recording_resumes_in_place_and_backfills_changes() {
    let rig = Rig::new(&["A"]).await;
    rig.connect().await;
    rig.link.set_recording(true).await;
    let s = song(10);
    rig.song_start(&s, None).await;
    pause(0.4).await;
    rig.song_stop(&s, 0.4).await; // paused, far short of half the song
    assert!(rig
        .link
        .status()
        .message
        .unwrap_or_default()
        .starts_with("Paused \"Holy Forever\"."));
    agent_lock(&rig.agent).change(&["Stars"], &[]); // changed while paused
    pause(0.6).await;
    rig.bus
        .publish(new_event(
            EventType::TimerResumed,
            "t",
            EventPayload::Timer(TimerPayload {
                duration_seconds: 10,
                song: Some(s.clone()),
                position_seconds: Some(0.5),
                running: true,
                started_at: Some(Utc::now()),
                ..TimerPayload::default()
            }),
        ))
        .await;
    assert!(rig.link.status().recording_song.is_some());
    rig.song_stop(&s, 10.0).await;
    wait_for(|| rig.link.has_take("song-1"), 3.0).await;
    let take = rig.store.takes()["song-1"].clone();
    assert_eq!(take.edges.len(), 1);
    assert_eq!(take.edges[0].added, names(&["Stars"]));
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_song_finishes_the_one_before() {
    let rig = Rig::new(&["A"]).await;
    rig.connect().await;
    rig.link.set_recording(true).await;
    rig.song_start(&song(100), None).await;
    pause(0.05).await;
    let mut next = song(100);
    next.id = "item-2".into();
    next.source_song_id = Some("song-2".into());
    next.title = "Gratitude".into();
    rig.song_start(&next, None).await;
    let status = rig.link.status();
    assert_eq!(status.recording_song.as_deref(), Some("Gratitude"));
    assert!(rig.store.takes().is_empty()); // the first was far too short to keep
    rig.link.stop().await;
}

// ------------------------------------------------------------------------------- replay

#[tokio::test(flavor = "multi_thread")]
async fn replay_syncs_then_fires_edges_slightly_early() {
    let rig = Rig::new(&["Warm"]).await;
    with_takes(&rig, vec![stored_take()]);
    rig.connect().await;
    *rig.t0.lock().unwrap() = Instant::now();
    rig.song_start(&song(10), None).await;
    wait_for(|| rig.sent.lock().unwrap().len() >= 6, 4.0).await;
    assert_eq!(
        rig.addresses(),
        vec![
            "/live/*/cue/Stars/deactivate",
            "/live/*/cue/Strobe/deactivate",
            "/live/*/cue/Warm/activate",
            "/live/*/cue/Stars/activate",
            "/live/*/cue/Stars/deactivate",
            "/live/*/cue/Strobe/activate",
        ]
    );
    assert!((rig.sent_at(3) - 0.46).abs() < 0.15); // 0.5 s edge, 40 ms lead
    assert!((rig.sent_at(4) - 0.96).abs() < 0.15);
    assert_eq!(
        rig.link.status().replaying_song.as_deref(),
        Some("Holy Forever")
    );
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_switch_off_sends_nothing() {
    let rig = Rig::with(&["Warm"], false).await;
    with_takes(&rig, vec![stored_take()]);
    rig.connect().await;
    rig.song_start(&song(10), None).await;
    pause(0.8).await;
    assert!(rig.addresses().is_empty());
    assert_eq!(
        rig.link.status().replay_blocked.as_deref(),
        Some("Replay is switched off.")
    );
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_does_not_run_while_recording_armed() {
    let rig = Rig::new(&["Warm"]).await;
    with_takes(&rig, vec![stored_take()]);
    rig.connect().await;
    rig.link.set_recording(true).await;
    rig.song_start(&song(10), None).await;
    pause(0.8).await;
    assert!(rig.addresses().is_empty());
    assert_eq!(
        rig.link.status().replay_blocked.as_deref(),
        Some("Paused while recording.")
    );
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_blocked_strings_are_exact_and_ordered() {
    let rig = Rig::new(&["Warm"]).await;
    rig.link.start().await;
    rig.set_settings(|s| {
        s.0 = false;
        s.1 = false;
    });
    assert_eq!(
        rig.link.status().replay_blocked.as_deref(),
        Some("Lighting is off.")
    );
    rig.set_settings(|s| s.0 = true);
    assert_eq!(
        rig.link.status().replay_blocked.as_deref(),
        Some("Replay needs Lighting set to OSC.")
    );
    rig.set_settings(|s| {
        s.1 = true;
        s.3.replay = false;
    });
    assert_eq!(
        rig.link.status().replay_blocked.as_deref(),
        Some("Replay is switched off.")
    );
    rig.set_settings(|s| s.3.replay = true);
    assert_eq!(rig.link.status().replay_blocked, None);
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_stands_back_from_a_cue_the_operator_just_changed() {
    let rig = Rig::new(&["Warm"]).await;
    with_takes(&rig, vec![stored_take()]);
    rig.connect().await;
    // The operator grabs "Strobe" shortly before replay would turn it on.
    agent_lock(&rig.agent).change(&["Strobe"], &[]);
    pause(0.6).await;
    rig.song_start(&song(10), None).await;
    pause(1.4).await;
    let addresses = rig.addresses();
    assert!(!addresses.iter().any(|a| a.contains("Strobe"))); // held, so skipped entirely
    assert!(addresses.contains(&"/live/*/cue/Stars/activate".to_string())); // untouched cues follow
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_cancels_remaining_replay() {
    let rig = Rig::new(&["Warm"]).await;
    with_takes(&rig, vec![stored_take()]);
    rig.connect().await;
    let s = song(10);
    rig.song_start(&s, None).await;
    pause(0.2).await;
    rig.song_stop(&s, 0.2).await;
    let count = rig.addresses().len();
    pause(1.0).await;
    assert_eq!(rig.addresses().len(), count);
    assert_eq!(rig.link.status().replaying_song, None);
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn seek_replays_from_the_new_position() {
    let rig = Rig::new(&["Warm"]).await;
    with_takes(&rig, vec![stored_take()]);
    rig.connect().await;
    let s = song(10);
    rig.seek(&s, 5.0, 5).await;
    wait_for(|| rig.sent.lock().unwrap().len() >= 3, 3.0).await;
    // At 5 s the take says: Warm and Strobe on, Stars off.
    let mut first_three: Vec<String> = rig.addresses().into_iter().take(3).collect();
    first_three.sort();
    assert_eq!(
        first_three,
        vec![
            "/live/*/cue/Stars/deactivate",
            "/live/*/cue/Strobe/activate",
            "/live/*/cue/Warm/activate",
        ]
    );
    pause(0.3).await;
    assert_eq!(rig.addresses().len(), 3); // nothing left to schedule after 5 s
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_song_without_a_take_sends_nothing() {
    let rig = Rig::new(&["Warm"]).await;
    rig.connect().await;
    rig.song_start(&song(10), None).await;
    pause(0.4).await;
    assert!(rig.addresses().is_empty());
    assert_eq!(rig.link.status().replaying_song, None);
    rig.link.stop().await;
}

// ------------------------------------------------------------------- agent and take store

#[tokio::test(flavor = "multi_thread")]
async fn agent_restart_resets_the_sequence_without_a_false_change() {
    let rig = Rig::new(&["A"]).await;
    agent_lock(&rig.agent).seq = 40;
    rig.connect().await;
    {
        let mut agent = agent_lock(&rig.agent);
        agent.seq = 1; // agent restarted
        agent.active = names(&["A", "B"]);
    }
    pause(0.8).await;
    let status = rig.link.status();
    assert_eq!(status.active_count, Some(2));
    assert_eq!(status.link, Link::Connected);
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn clear_take_removes_it_and_reports_a_missing_one() {
    let rig = Rig::new(&["A"]).await;
    with_takes(&rig, vec![stored_take()]);
    rig.link.start().await;
    wait_for(|| rig.link.has_take("song-1"), 3.0).await;
    assert!(!rig.link.clear_take("nope").await);
    assert!(rig.link.clear_take("song-1").await);
    assert!(!rig.link.has_take("song-1"));
    assert!(rig.store.takes().is_empty());
    assert_eq!(
        rig.link.status().message.as_deref(),
        Some("Capture cleared.")
    );
    rig.link.stop().await;
}

#[test]
fn file_store_round_trip_and_bad_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.json");
    let store = FileTakeStore::new(&path);
    assert!(store.load().unwrap().is_empty());
    let mut takes = BTreeMap::new();
    takes.insert("song-1".to_string(), stored_take());
    store.save(&takes).unwrap();
    assert_eq!(
        store.load().unwrap()["song-1"].edges[1].removed,
        names(&["Stars"])
    );
    std::fs::write(&path, "{not json").unwrap();
    assert!(store.load().unwrap().is_empty());
    assert!(dir.path().join("t.json.bad").exists());
    assert!(!path.exists());
}

#[test]
fn file_store_writes_atomically_in_the_python_layout() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lightkey-takes.json");
    let store = FileTakeStore::new(&path);
    let mut take = stored_take();
    take.recorded_at = "2026-10-20T20:00:00Z".parse().unwrap();
    let mut takes = BTreeMap::new();
    takes.insert("song-1".to_string(), take);
    store.save(&takes).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(value["takes"][0]["recorded_at"], "2026-10-20T20:00:00Z");
    assert!(
        text.starts_with("{\n \"version\": 1,"),
        "indent=1 layout: {text}"
    );
    // No temp file is left behind.
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, vec!["lightkey-takes.json"]);
}

#[test]
fn a_file_with_an_invalid_take_is_renamed_bad_and_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.json");
    std::fs::write(
        &path,
        r#"{"version":1,"takes":[{"song_key":"","song_title":"x","recorded_at":"2026-10-20T20:00:00Z","duration_seconds":10}]}"#,
    )
    .unwrap();
    let store = FileTakeStore::new(&path);
    assert!(store.load().unwrap().is_empty());
    assert!(dir.path().join("t.json.bad").exists());
}
