//! Project awareness against a fake `lightkey-api` helper (CONTRACT.md): a UDP relay whose hello
//! carries a project, fixture count and map id that tests can change or repeat mid-run, and an
//! HTTP side that serves `GET /health` and `POST /fixtures/refresh` with every code in the
//! contract. Covers: the project stored with a capture, the recording thrown away when the project
//! or its fixture map changes (and kept when a hello is merely repeated), the helper's version
//! and watch state, Read fixtures, and the warn-only notice that never touches replay. Fixture OSC
//! goes to an in-memory recorder; nothing is sent anywhere.

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use chrono::Utc;
use rosc::{encoder, OscBundle, OscMessage, OscPacket, OscTime, OscType};
use serde_json::json;
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{LightkeySettings, OscCueSource};
use stagepilot_core::events::{new_event, EventPayload, EventType, TimerPayload};
use stagepilot_core::state::Song;
use stagepilot_lights::lightkey::{FixtureLink, LightkeyLink as Link};
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand};
use stagepilot_lights::lightkey_link::{
    LightkeyLink, LinkOptions, LinkSettings, LooksSink, SongLooks,
};

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;

/// The Lightkey Mac's clock runs this far ahead of ours.
const OFFSET: f64 = 5.0;
const NTP: u64 = 2_208_988_800;

fn wall() -> f64 {
    let now = Utc::now();
    now.timestamp() as f64 + f64::from(now.timestamp_subsec_micros()) / 1e6
}

// ------------------------------------------------------------------------------ fake relay

struct FakeRelay {
    socket: Arc<UdpSocket>,
    port: u16,
    subscriber: Arc<Mutex<Option<SocketAddr>>>,
    seq: Arc<Mutex<i64>>,
    /// Everything it was asked for: "subscribe", "ping", "keyframe", "unsubscribe".
    log: Arc<Mutex<Vec<String>>>,
    state: Arc<Mutex<Vec<FixtureCommand>>>,
    answer_pings: Arc<Mutex<bool>>,
    /// While true the relay process is "frozen" (SIGSTOP): datagrams queue up unread, and are
    /// answered and stamped with the time they are finally processed after it thaws.
    frozen: Arc<Mutex<bool>>,
    /// What the hello says: `(project, fixtures, map)`.
    info: Arc<Mutex<(String, i32, i32)>>,
}

fn hello_message(info: &(String, i32, i32)) -> OscMessage {
    OscMessage {
        addr: "/relay/hello".into(),
        args: vec![
            OscType::Int(1),
            OscType::Int(777),
            OscType::String(info.0.clone()),
            OscType::Int(info.1),
            OscType::Int(info.2),
        ],
    }
}

fn packet(time: f64, head: [OscType; 4], messages: Vec<OscMessage>) -> Vec<u8> {
    let tag = OscTime {
        seconds: (time.trunc() as u64 + NTP) as u32,
        fractional: (time.fract() * 4_294_967_296.0) as u32,
    };
    let mut content = vec![OscPacket::Message(OscMessage {
        addr: "/relay/seq".into(),
        args: head.to_vec(),
    })];
    content.extend(messages.into_iter().map(OscPacket::Message));
    encoder::encode(&OscPacket::Bundle(OscBundle {
        timetag: tag,
        content,
    }))
    .unwrap()
}

fn to_message(c: &FixtureCommand) -> OscMessage {
    OscMessage {
        addr: c.a.clone(),
        args: c
            .v
            .iter()
            .map(|a| match a {
                FixtureArg::Float(f) => OscType::Float(*f as f32),
                FixtureArg::Int(i) => OscType::Int(*i as i32),
                FixtureArg::Text(t) => OscType::String(t.clone()),
                FixtureArg::Bool(b) => OscType::Bool(*b),
            })
            .collect(),
    }
}

impl FakeRelay {
    async fn start(initial: Vec<FixtureCommand>) -> Self {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let port = socket.local_addr().unwrap().port();
        let me = Self {
            socket: socket.clone(),
            port,
            subscriber: Arc::default(),
            seq: Arc::default(),
            log: Arc::default(),
            state: Arc::new(Mutex::new(initial)),
            answer_pings: Arc::new(Mutex::new(true)),
            frozen: Arc::new(Mutex::new(false)),
            info: Arc::new(Mutex::new(("Show".into(), 3, 1))),
        };
        let frozen = me.frozen.clone();
        let info = me.info.clone();
        let (sub, seq, log, state, answer) = (
            me.subscriber.clone(),
            me.seq.clone(),
            me.log.clone(),
            me.state.clone(),
            me.answer_pings.clone(),
        );
        tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];
            while let Ok((n, from)) = socket.recv_from(&mut buf).await {
                let Ok((_, OscPacket::Message(m))) = rosc::decoder::decode_udp(&buf[..n]) else {
                    continue;
                };
                if m.addr == "/relay/ping" {
                    log.lock().unwrap().push("ping".into());
                }
                while *frozen.lock().unwrap() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                let t2 = wall() + OFFSET;
                match m.addr.as_str() {
                    "/relay/ping" => {
                        if !*answer.lock().unwrap() {
                            continue;
                        }
                        if let Some(OscType::Double(t1)) = m.args.first() {
                            let reply = encoder::encode(&OscPacket::Message(OscMessage {
                                addr: "/relay/pong".into(),
                                args: vec![
                                    OscType::Double(*t1),
                                    OscType::Double(t2),
                                    OscType::Double(wall() + OFFSET),
                                ],
                            }))
                            .unwrap();
                            let _ = socket.send_to(&reply, from).await;
                        }
                    }
                    "/relay/subscribe" => {
                        let new = sub.lock().unwrap().is_none();
                        log.lock().unwrap().push("subscribe".into());
                        *sub.lock().unwrap() = Some(from);
                        if new {
                            let hello = hello_message(&info.lock().unwrap());
                            let bytes = packet(
                                wall() + OFFSET,
                                [
                                    OscType::Int(0),
                                    OscType::String("hello".into()),
                                    OscType::Int(1),
                                    OscType::Int(1),
                                ],
                                vec![hello],
                            );
                            let _ = socket.send_to(&bytes, from).await;
                            let commands = state.lock().unwrap().clone();
                            let number = {
                                let mut s = seq.lock().unwrap();
                                *s += 1;
                                *s
                            };
                            let bytes = packet(
                                wall() + OFFSET,
                                [
                                    OscType::Int(number as i32),
                                    OscType::String("keyframe".into()),
                                    OscType::Int(1),
                                    OscType::Int(1),
                                ],
                                commands.iter().map(to_message).collect(),
                            );
                            let _ = socket.send_to(&bytes, from).await;
                        }
                    }
                    "/relay/keyframe" => {
                        log.lock().unwrap().push("keyframe".into());
                        let commands = state.lock().unwrap().clone();
                        let number = {
                            let mut s = seq.lock().unwrap();
                            *s += 1;
                            *s
                        };
                        let bytes = packet(
                            wall() + OFFSET,
                            [
                                OscType::Int(number as i32),
                                OscType::String("keyframe".into()),
                                OscType::Int(1),
                                OscType::Int(1),
                            ],
                            commands.iter().map(to_message).collect(),
                        );
                        let _ = socket.send_to(&bytes, from).await;
                    }
                    "/relay/unsubscribe" => {
                        log.lock().unwrap().push("unsubscribe".into());
                        *sub.lock().unwrap() = None;
                    }
                    _ => {}
                }
            }
        });
        me
    }

    /// Send a change that "appeared on DMX" `ago` seconds ago. `skip` burns sequence numbers to
    /// fake a lost packet.
    async fn change(&self, commands: Vec<FixtureCommand>, ago: f64, skip: i64) {
        {
            let mut state = self.state.lock().unwrap();
            for c in &commands {
                match state.iter_mut().find(|e| e.a == c.a) {
                    Some(e) => *e = c.clone(),
                    None => state.push(c.clone()),
                }
            }
        }
        let Some(to) = *self.subscriber.lock().unwrap() else {
            return;
        };
        let seq = {
            let mut s = self.seq.lock().unwrap();
            *s += 1 + skip;
            *s
        };
        let bytes = packet(
            wall() + OFFSET - ago,
            [
                OscType::Int(seq as i32),
                OscType::String("event".into()),
                OscType::Int(1),
                OscType::Int(1),
            ],
            commands.iter().map(to_message).collect(),
        );
        self.socket.send_to(&bytes, to).await.unwrap();
    }

    /// The helper says hello again with this project, fixture count and map id (a repeat when
    /// they are the ones it already said). The sequence number moves on like any packet.
    async fn say_hello(&self, project: &str, fixtures: i32, map: i32) {
        *self.info.lock().unwrap() = (project.to_string(), fixtures, map);
        let Some(to) = *self.subscriber.lock().unwrap() else {
            return;
        };
        let seq = {
            let mut s = self.seq.lock().unwrap();
            *s += 1;
            *s
        };
        let bytes = packet(
            wall() + OFFSET,
            [
                OscType::Int(seq as i32),
                OscType::String("hello".into()),
                OscType::Int(1),
                OscType::Int(1),
            ],
            vec![hello_message(&self.info.lock().unwrap())],
        );
        self.socket.send_to(&bytes, to).await.unwrap();
    }

    #[allow(dead_code)]
    fn asked(&self, what: &str) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|w| *w == what)
            .count()
    }
}

// ---------------------------------------------------------------------------- fake http agent

type Active = Arc<Mutex<Vec<String>>>;

/// What the fake helper answers to `GET /health` and `POST /fixtures/refresh`.
struct Helper {
    health: Mutex<(u16, serde_json::Value)>,
    refresh: Mutex<(u16, serde_json::Value)>,
    refreshes: Mutex<usize>,
}

fn health_0_5_0() -> serde_json::Value {
    json!({
        "ok": true, "lightkey": true, "error": null, "dmx_age_ms": 20, "reads": 5,
        "version": "0.5.0",
        "fixture_map": {"project": "Show.lightkeyproj", "count": 3, "read_at": 1794027080123_i64, "map": 1},
        "watch": {"enabled": true, "interval_s": 180, "last_check_at": 1794027200000_i64,
                   "open_project": "Show.lightkeyproj", "state": "ok", "folder": null, "detail": null}
    })
}

async fn serve_http(active: Active, helper: Arc<Helper>) -> u16 {
    async fn health(State(h): State<(Active, Arc<Helper>)>) -> Response {
        let (code, body) = h.1.health.lock().unwrap().clone();
        (
            axum::http::StatusCode::from_u16(code).unwrap(),
            axum::Json(body),
        )
            .into_response()
    }
    async fn refresh(State(h): State<(Active, Arc<Helper>)>) -> Response {
        *h.1.refreshes.lock().unwrap() += 1;
        let (code, body) = h.1.refresh.lock().unwrap().clone();
        (
            axum::http::StatusCode::from_u16(code).unwrap(),
            axum::Json(body),
        )
            .into_response()
    }
    async fn time() -> Response {
        axum::Json(json!({"t": (wall() + OFFSET) * 1000.0})).into_response()
    }
    async fn log(
        State(h): State<(Active, Arc<Helper>)>,
        Query(_q): Query<BTreeMap<String, String>>,
    ) -> Response {
        axum::Json(json!({
            "events": [],
            "state": {
                "online": true,
                "active": *h.0.lock().unwrap(),
                "seq": 1,
                "server_time": (wall() + OFFSET) * 1000.0,
            },
        }))
        .into_response()
    }
    let app = Router::new()
        .route("/time", get(time))
        .route("/log", get(log))
        .route("/health", get(health))
        .route("/fixtures/refresh", post(refresh))
        .with_state((active, helper));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    port
}

// ---------------------------------------------------------------------------------------- rig

struct Rig {
    relay: FakeRelay,
    bus: EventBus,
    saved: Arc<Mutex<Vec<SongLooks>>>,
    #[allow(dead_code)]
    attempts: Arc<Mutex<usize>>,
    settings: Arc<Mutex<LinkSettings>>,
    helper: Arc<Helper>,
    link: LightkeyLink,
}

fn cmd(a: &str, v: Vec<FixtureArg>) -> FixtureCommand {
    FixtureCommand::new(a, v)
}
fn f(x: f64) -> FixtureArg {
    FixtureArg::Float(x)
}
fn dim(name: &str, x: f64) -> FixtureCommand {
    cmd(&format!("/fixture/{name}/overrides/dimmer"), vec![f(x)])
}
fn color(name: &str, r: f64, g: f64, b: f64) -> FixtureCommand {
    cmd(
        &format!("/fixture/{name}/overrides/color"),
        vec![f(r), f(g), f(b)],
    )
}

impl Rig {
    async fn new(initial: Vec<FixtureCommand>) -> Self {
        Self::with_row_limit(initial, usize::MAX).await
    }

    /// A rig whose timeline refuses a song of more than `limit` rows (`LooksError::TooLarge`),
    /// as the real one does past its caps. Every attempt is counted in `attempts`.
    async fn with_row_limit(initial: Vec<FixtureCommand>, limit: usize) -> Self {
        let relay = FakeRelay::start(initial).await;
        let active: Active = Arc::new(Mutex::new(vec!["Warm".into()]));
        let helper = Arc::new(Helper {
            health: Mutex::new((200, health_0_5_0())),
            refresh: Mutex::new((200, json!({}))),
            refreshes: Mutex::new(0),
        });
        let http_port = serve_http(active, helper.clone()).await;
        let settings = Arc::new(Mutex::new((
            true,
            true,
            "127.0.0.1".to_string(),
            LightkeySettings {
                api_url: format!("http://127.0.0.1:{http_port}"),
                replay: true,
                ..LightkeySettings::default()
            },
        )));
        let bus = EventBus::new();
        let provider_settings = settings.clone();
        let provider: stagepilot_lights::lightkey_link::SettingsProvider =
            Arc::new(move || provider_settings.lock().unwrap().clone());
        let saved: Arc<Mutex<Vec<SongLooks>>> = Arc::new(Mutex::new(Vec::new()));
        let sink_saved = saved.clone();
        let attempts = Arc::new(Mutex::new(0usize));
        let sink_attempts = attempts.clone();
        let sink: LooksSink = Arc::new(move |looks| {
            *sink_attempts.lock().unwrap() += 1;
            let refused = looks.rows.len() > limit;
            if !refused {
                sink_saved.lock().unwrap().push(looks);
            }
            Box::pin(async move {
                if refused {
                    Err(stagepilot_lights::lightkey_link::LooksError::TooLarge)
                } else {
                    Ok(())
                }
            })
        });
        let link = LightkeyLink::new(
            bus.clone(),
            provider,
            LinkOptions {
                looks_sink: Some(sink),
                relay_port: relay.port,
                ..LinkOptions::default()
            },
        );
        Self {
            relay,
            bus,
            saved,
            attempts,
            settings,
            helper,
            link,
        }
    }

    async fn connect(&self) {
        self.link.start().await;
        wait_for(|| self.link.status().link == Link::Connected, 3.0).await;
        wait_for(
            || {
                self.link
                    .status()
                    .fixtures
                    .is_some_and(|s| s.link == FixtureLink::Live)
            },
            3.0,
        )
        .await;
    }

    async fn song_start(&self, song: &Song) {
        self.bus
            .publish(new_event(
                EventType::SongStarted,
                "t",
                EventPayload::Song {
                    song: song.clone(),
                    index: 0,
                    position_seconds: None,
                    observed_at: None,
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
                    started_at: Some(Utc::now()),
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

// ----------------------------------------------------------------------------------- tests

const CHANGED_MESSAGE: &str =
    "Not saved: the Lightkey project or its fixtures changed during the recording";

/// Record `Holy Forever` (2 s) with `during` run half a second in, then stop it at 2 s.
async fn record_with<F, Fut>(rig: &Rig, during: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    rig.connect().await;
    rig.link.set_recording(true).await;
    rig.song_start(&song(2)).await;
    pause(0.5).await;
    rig.relay.change(vec![dim("P1", 1.0)], 0.0, 0).await;
    pause(0.4).await;
    during().await;
    pause(0.4).await;
    rig.relay
        .change(vec![color("P1", 1.0, 0.0, 0.0)], 0.0, 0)
        .await;
    pause(0.8).await;
    rig.song_stop(&song(2), 2.0).await;
    pause(0.6).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_project_in_the_hello_is_shown_and_stored_with_the_capture() {
    let rig = Rig::new(vec![dim("P1", 0.0), color("P1", 0.0, 0.0, 0.0)]).await;
    record_with(&rig, || async {}).await;
    assert_eq!(rig.link.status().project.as_deref(), Some("Show"));
    {
        let message = rig.link.status().message;
        let saved = rig.saved.lock().unwrap();
        assert_eq!(saved.len(), 1, "{message:?}");
        assert_eq!(saved[0].status.project.as_deref(), Some("Show"));
    }
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hello_with_no_project_name_stores_none_and_shows_none() {
    let rig = Rig::new(vec![dim("P1", 0.0)]).await;
    *rig.relay.info.lock().unwrap() = ("".into(), 3, 1);
    record_with(&rig, || async {}).await;
    // the hello names nothing, so the capture records no project and never warns; the strip
    // falls back to the name in the helper's own health answer
    assert_eq!(rig.saved.lock().unwrap()[0].status.project, None);
    assert_eq!(
        rig.link.status().project.as_deref(),
        Some("Show.lightkeyproj")
    );
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_project_change_during_a_recording_discards_it_with_the_reason() {
    let rig = Rig::new(vec![dim("P1", 0.0), color("P1", 0.0, 0.0, 0.0)]).await;
    record_with(&rig, || rig.relay.say_hello("Other", 3, 2)).await;
    assert!(rig.saved.lock().unwrap().is_empty(), "nothing is saved");
    assert_eq!(
        rig.link.status().message.as_deref(),
        Some(&*format!(
            "{CHANGED_MESSAGE} (Show -> Other). It captures again next time."
        ))
    );
    // the strip follows the project that is open now
    assert_eq!(rig.link.status().project.as_deref(), Some("Other"));
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn new_fixtures_in_the_same_project_also_discard_the_recording() {
    let rig = Rig::new(vec![dim("P1", 0.0), color("P1", 0.0, 0.0, 0.0)]).await;
    record_with(&rig, || rig.relay.say_hello("Show", 4, 2)).await;
    assert!(rig.saved.lock().unwrap().is_empty());
    assert_eq!(
        rig.link.status().message.as_deref(),
        Some(&*format!("{CHANGED_MESSAGE} (Show, 3 fixtures -> Show, 4 fixtures). It captures again next time."))
    );
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hello_sent_again_with_the_same_map_is_not_a_change() {
    let rig = Rig::new(vec![dim("P1", 0.0), color("P1", 0.0, 0.0, 0.0)]).await;
    record_with(&rig, || async {
        rig.relay.say_hello("Show", 3, 1).await;
        pause(0.3).await;
        rig.relay.say_hello("Show", 3, 1).await;
    })
    .await;
    {
        let message = rig.link.status().message;
        let saved = rig.saved.lock().unwrap();
        assert_eq!(saved.len(), 1, "{message:?}");
        assert_eq!(saved[0].status.project.as_deref(), Some("Show"));
    }
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_change_while_nothing_records_only_changes_what_the_next_capture_stores() {
    let rig = Rig::new(vec![dim("P1", 0.0), color("P1", 0.0, 0.0, 0.0)]).await;
    rig.connect().await;
    rig.relay.say_hello("Other", 5, 2).await;
    wait_for(
        || rig.link.status().project.as_deref() == Some("Other"),
        3.0,
    )
    .await;
    assert_eq!(
        rig.link.status().message,
        None,
        "no recording, nothing to discard"
    );
    rig.link.set_recording(true).await;
    rig.song_start(&song(2)).await;
    pause(0.5).await;
    rig.relay.change(vec![dim("P1", 1.0)], 0.0, 0).await;
    pause(1.2).await;
    rig.relay.change(vec![dim("P1", 0.5)], 0.0, 0).await;
    pause(0.3).await;
    rig.song_stop(&song(2), 2.0).await;
    wait_for(|| !rig.saved.lock().unwrap().is_empty(), 3.0).await;
    assert_eq!(
        rig.saved.lock().unwrap()[0].status.project.as_deref(),
        Some("Other")
    );
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_helper_version_fixture_map_and_watch_state_are_in_the_status() {
    let rig = Rig::new(vec![]).await;
    assert!(rig.link.status().helper.is_none());
    rig.connect().await;
    wait_for(|| rig.link.status().helper.is_some(), 3.0).await;
    let helper = rig.link.status().helper.unwrap();
    assert_eq!(helper.version.as_deref(), Some("0.5.0"));
    assert!(!helper.outdated);
    assert_eq!(helper.project.as_deref(), Some("Show.lightkeyproj"));
    assert_eq!(helper.fixtures, Some(3));
    assert_eq!(helper.map, Some(1));
    assert_eq!(helper.watch_state.as_deref(), Some("ok"));
    assert_eq!(helper.watch_detail, None);
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_helper_that_sends_a_permission_problem_shows_its_sentence() {
    let rig = Rig::new(vec![]).await;
    let mut health = health_0_5_0();
    health["watch"]["state"] = json!("permission_denied");
    health["watch"]["detail"] =
        json!("macOS has not allowed this helper to read your Documents folder.");
    *rig.helper.health.lock().unwrap() = (200, health);
    rig.connect().await;
    wait_for(|| rig.link.status().helper.is_some(), 3.0).await;
    let helper = rig.link.status().helper.unwrap();
    assert_eq!(helper.watch_state.as_deref(), Some("permission_denied"));
    assert!(helper.watch_detail.unwrap().contains("Documents"));
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_helper_with_no_version_is_older_than_0_5_0_and_everything_else_works() {
    let rig = Rig::new(vec![dim("P1", 0.0), color("P1", 0.0, 0.0, 0.0)]).await;
    *rig.helper.health.lock().unwrap() = (
        200,
        json!({"ok": true, "lightkey": true, "error": null, "dmx_age_ms": 20, "reads": 5}),
    );
    record_with(&rig, || async {}).await;
    let helper = rig.link.status().helper.unwrap();
    assert_eq!(helper.version, None);
    assert!(helper.outdated);
    assert_eq!(helper.project, None);
    // the project still comes from the relay hello, and capture is untouched
    assert_eq!(rig.link.status().project.as_deref(), Some("Show"));
    assert_eq!(rig.saved.lock().unwrap().len(), 1);
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_helper_with_no_health_page_claims_nothing() {
    let rig = Rig::new(vec![]).await;
    *rig.helper.health.lock().unwrap() = (404, json!({"error": "not found"}));
    rig.connect().await;
    pause(0.8).await;
    // no version was reported, so no "older than 0.5.0" is claimed either
    assert!(rig.link.status().helper.is_none());
    assert_eq!(rig.link.status().link, Link::Connected);
    rig.link.stop().await;
}

async fn refresh_answer(
    rig: &Rig,
    status: u16,
    body: serde_json::Value,
) -> stagepilot_lights::lightkey::LightkeyReadFixtures {
    *rig.helper.refresh.lock().unwrap() = (status, body);
    rig.link.read_fixtures().await
}

#[tokio::test(flavor = "multi_thread")]
async fn read_fixtures_says_what_was_read_and_when_nothing_changed() {
    let rig = Rig::new(vec![]).await;
    rig.connect().await;
    let ok = refresh_answer(
        &rig,
        200,
        json!({"ok": true, "project": "Show.lightkeyproj", "count": 66, "map": 9, "changed": true}),
    )
    .await;
    assert!(ok.ok);
    assert_eq!(ok.message, "Read 66 fixtures from Show.lightkeyproj");
    assert_eq!(
        (ok.project.as_deref(), ok.count, ok.changed, ok.code),
        (Some("Show.lightkeyproj"), Some(66), Some(true), None)
    );
    let same = refresh_answer(&rig, 200, json!({"ok": true, "project": "Show.lightkeyproj", "count": 66, "map": 9, "changed": false})).await;
    assert!(same.ok);
    assert_eq!(same.message, "No change: 66 fixtures");
    assert_eq!(same.changed, Some(false));
    let one = refresh_answer(
        &rig,
        200,
        json!({"ok": true, "project": "Solo.lightkeyproj", "count": 1, "changed": true}),
    )
    .await;
    assert_eq!(one.message, "Read 1 fixture from Solo.lightkeyproj");
    assert_eq!(*rig.helper.refreshes.lock().unwrap(), 3);
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_successful_read_refreshes_the_helper_numbers_without_waiting_for_the_next_poll() {
    let rig = Rig::new(vec![]).await;
    rig.connect().await;
    wait_for(
        || {
            rig.link
                .status()
                .helper
                .is_some_and(|h| h.fixtures == Some(3))
        },
        3.0,
    )
    .await;
    let mut health = health_0_5_0();
    health["fixture_map"]["count"] = json!(66);
    health["fixture_map"]["map"] = json!(9);
    *rig.helper.health.lock().unwrap() = (200, health);
    let ok = refresh_answer(
        &rig,
        200,
        json!({"ok": true, "project": "Show.lightkeyproj", "count": 66, "map": 9, "changed": true}),
    )
    .await;
    assert!(ok.ok);
    wait_for(
        || {
            rig.link
                .status()
                .helper
                .is_some_and(|h| h.fixtures == Some(66))
        },
        3.0,
    )
    .await;
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn read_fixtures_passes_each_helper_error_through_in_its_own_words() {
    let rig = Rig::new(vec![]).await;
    rig.connect().await;
    let permission = "macOS has not allowed this helper to read your Documents folder. Open System Settings > Privacy & Security > Files and Folders and allow python3, then press Read fixtures again.";
    let cases = [
        (
            409,
            json!({"ok": false, "code": "lightkey_closed", "error": "Lightkey is not running."}),
            "lightkey_closed",
            "Lightkey is not running.",
        ),
        (
            409,
            json!({"ok": false, "code": "no_project", "error": "Lightkey is open with no saved project."}),
            "no_project",
            "Lightkey is open with no saved project.",
        ),
        (
            409,
            json!({"ok": false, "code": "busy", "error": "A read is already running."}),
            "busy",
            "A read is already running.",
        ),
        (
            403,
            json!({"ok": false, "code": "permission_denied", "folder": "Documents", "error": permission}),
            "permission_denied",
            permission,
        ),
        (
            422,
            json!({"ok": false, "code": "unreadable", "error": "The project file could not be read."}),
            "unreadable",
            "The project file could not be read.",
        ),
    ];
    for (status, body, code, text) in cases {
        let answer = refresh_answer(&rig, status, body).await;
        assert!(!answer.ok, "{code}");
        assert_eq!(answer.code.as_deref(), Some(code));
        assert_eq!(answer.message, text);
        assert_eq!(
            (answer.project, answer.count, answer.changed),
            (None, None, None)
        );
    }
    // an answer with no text still says something plain
    let bare = refresh_answer(&rig, 500, json!({})).await;
    assert!(!bare.ok && bare.message.contains("500"), "{bare:?}");
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn read_fixtures_on_an_old_helper_asks_for_the_update() {
    let rig = Rig::new(vec![]).await;
    rig.connect().await;
    for status in [404, 405] {
        let answer = refresh_answer(&rig, status, json!({"error": "not found"})).await;
        assert!(!answer.ok);
        assert_eq!(answer.code.as_deref(), Some("old_helper"));
        assert_eq!(answer.message, "Update the helper to use this.");
    }
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn read_fixtures_with_nobody_listening_or_lighting_off_says_so() {
    let rig = Rig::new(vec![]).await;
    rig.settings.lock().unwrap().3.api_url = "http://127.0.0.1:1".into();
    let gone = rig.link.read_fixtures().await;
    assert!(!gone.ok);
    assert_eq!(gone.code.as_deref(), Some("unreachable"));
    assert_eq!(gone.message, "Cannot reach the Lightkey API.");
    rig.settings.lock().unwrap().0 = false;
    let off = rig.link.read_fixtures().await;
    assert_eq!(off.code.as_deref(), Some("lighting_off"));
    assert_eq!(*rig.helper.refreshes.lock().unwrap(), 0);
}

// ------------------------------------------- warn only, through the plugin: replay never stops

mod notice {
    use super::*;
    use stagepilot_core::config::{
        CaptureStatus, LightingMethod, LightsSettings, OscArgument, OscLightingCue,
        OscMessage as CueMessage, SongLightingCueMap,
    };
    use stagepilot_core::plugin::Plugin;
    use stagepilot_lights::lightkey_link::{ArmedStore, MemoryArmedStore};
    use stagepilot_lights::{LightsOptions, LightsPlugin};
    use stagepilot_osc::{OscBackend, OscOutput, OutputError};

    type Log = Arc<Mutex<Vec<String>>>;
    struct Out(Log);
    impl OscOutput for Out {
        fn closed(&self) -> bool {
            false
        }
        fn send_message(&self, address: &str, _: &[OscArgument]) -> Result<(), OutputError> {
            self.0.lock().unwrap().push(address.into());
            Ok(())
        }
        fn close(&self) {}
    }
    struct Backend(Log);
    impl OscBackend for Backend {
        fn open_output(&self, _: &str, _: u16) -> Result<Arc<dyn OscOutput>, OutputError> {
            Ok(Arc::new(Out(self.0.clone())))
        }
    }

    /// A complete capture of `song-1` made under `captured`, replayed while the helper says the
    /// project is `Show`. Returns the status and what the replay sent.
    async fn replay_captured_under(
        captured: Option<&str>,
    ) -> (stagepilot_lights::lightkey::LightkeyStatus, Vec<String>) {
        let relay = FakeRelay::start(vec![dim("P1", 0.0)]).await;
        let helper = Arc::new(Helper {
            health: Mutex::new((200, health_0_5_0())),
            refresh: Mutex::new((200, json!({}))),
            refreshes: Mutex::new(0),
        });
        let http_port = serve_http(Arc::new(Mutex::new(vec![])), helper).await;
        let row = OscLightingCue {
            id: uuid::Uuid::new_v4(),
            at_seconds: 0.2,
            address: String::new(),
            args: vec![],
            label: String::new(),
            color: None,
            messages: vec![CueMessage {
                address: "/fixture/P1/overrides/dimmer".into(),
                args: vec![OscArgument::Float(0.7)],
            }],
            source: OscCueSource::Captured,
            edited: false,
            fade_seconds: 0.0,
        };
        let mut cue_maps = BTreeMap::new();
        cue_maps.insert(
            "song-1".to_string(),
            SongLightingCueMap {
                song_key: "song-1".into(),
                song_title: "Holy Forever".into(),
                cues: vec![],
                osc_cues: vec![row],
                loudness: None,
                captured_edited: false,
                capture: Some(CaptureStatus {
                    complete: true,
                    coverage_seconds: 100.0,
                    recorded_at: "2026-11-02T00:00:00Z".into(),
                    project: captured.map(str::to_string),
                }),
                effects: vec![],
            },
        );
        let lights = LightsSettings {
            enabled: true,
            method: LightingMethod::Osc,
            osc_host: "127.0.0.1".into(),
            osc_port: 9000,
            cue_maps,
            lightkey: LightkeySettings {
                api_url: format!("http://127.0.0.1:{http_port}"),
                replay: true,
                ..LightkeySettings::default()
            },
            ..LightsSettings::default()
        };
        let bus = EventBus::new();
        let log: Log = Arc::default();
        let mut options = LightsOptions::new(Arc::new(|| {
            Err(stagepilot_midi::MidiError::public("no MIDI in this test"))
        }));
        let backend_log = log.clone();
        options.osc_backend_factory = Arc::new(move || Arc::new(Backend(backend_log.clone())));
        options.lightkey = LinkOptions {
            armed_store: Arc::new(MemoryArmedStore::new()) as Arc<dyn ArmedStore>,
            relay_port: relay.port,
            ..LinkOptions::default()
        };
        let plugin = LightsPlugin::new(bus.clone(), lights, options).unwrap();
        plugin.start().await.unwrap();
        for _ in 0..150 {
            let s = plugin.lightkey_status().await;
            if s.link == Link::Connected && s.project.is_some() {
                break;
            }
            pause(0.02).await;
        }
        let song = song(10);
        bus.publish(new_event(
            EventType::SongStarted,
            "t",
            EventPayload::Song {
                song: song.clone(),
                index: 0,
                position_seconds: None,
                observed_at: None,
            },
        ))
        .await;
        bus.publish(new_event(
            EventType::TimerStarted,
            "t",
            EventPayload::Timer(TimerPayload {
                duration_seconds: 10,
                song: Some(song),
                started_at: Some(Utc::now()),
                running: true,
                ..TimerPayload::default()
            }),
        ))
        .await;
        pause(1.0).await;
        let status = plugin.lightkey_status().await;
        let sent = log.lock().unwrap().clone();
        plugin.stop().await.unwrap();
        (status, sent)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_song_captured_under_another_project_warns_and_still_replays() {
        let (status, sent) = replay_captured_under(Some("Old Project")).await;
        let notice = status.project_mismatch.expect("the notice");
        assert_eq!(
            notice.message,
            "Captured with Old Project. Lightkey has Show open."
        );
        assert_eq!(
            (notice.captured.as_str(), notice.current.as_str()),
            ("Old Project", "Show")
        );
        assert_eq!(status.replay_blocked, None, "replay is not held");
        assert!(
            sent.iter().any(|a| a == "/fixture/P1/overrides/dimmer"),
            "replay kept going: {sent:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_same_project_or_an_old_capture_never_warns() {
        for captured in [Some("Show"), None] {
            let (status, sent) = replay_captured_under(captured).await;
            assert_eq!(status.project_mismatch, None, "{captured:?}");
            assert!(
                sent.iter().any(|a| a == "/fixture/P1/overrides/dimmer"),
                "{sent:?}"
            );
        }
    }
}
