//! The fixture capture against a fake `lightkey-api` relay: a real UDP socket that speaks the relay
//! protocol (`lightkey-api` docs/relay.md), with its clock a fixed offset ahead of ours, so the
//! clock-ping conversion is exercised too. The HTTP side is the same fake agent the cue-edge tests
//! use. Fixture OSC goes to an in-memory recorder; nothing is sent anywhere.

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
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
        };
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
                let t2 = wall() + OFFSET;
                let Ok((_, OscPacket::Message(m))) = rosc::decoder::decode_udp(&buf[..n]) else {
                    continue;
                };
                match m.addr.as_str() {
                    "/relay/ping" => {
                        log.lock().unwrap().push("ping".into());
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
                            let hello = OscMessage {
                                addr: "/relay/hello".into(),
                                args: vec![
                                    OscType::Int(1),
                                    OscType::Int(777),
                                    OscType::String("Show".into()),
                                    OscType::Int(3),
                                    OscType::Int(1),
                                ],
                            };
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

async fn serve_http(active: Active) -> u16 {
    async fn time() -> Response {
        axum::Json(json!({"t": (wall() + OFFSET) * 1000.0})).into_response()
    }
    async fn log(
        State(active): State<Active>,
        Query(_q): Query<BTreeMap<String, String>>,
    ) -> Response {
        axum::Json(json!({
            "events": [],
            "state": {
                "online": true,
                "active": *active.lock().unwrap(),
                "seq": 1,
                "server_time": (wall() + OFFSET) * 1000.0,
            },
        }))
        .into_response()
    }
    let app = Router::new()
        .route("/time", get(time))
        .route("/log", get(log))
        .with_state(active);
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
    attempts: Arc<Mutex<usize>>,
    settings: Arc<Mutex<LinkSettings>>,
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
        let http_port = serve_http(active).await;
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

#[tokio::test(flavor = "multi_thread")]
async fn subscribes_measures_the_clock_and_reports_live_fixtures() {
    let rig = Rig::new(vec![dim("P1", 0.0), color("P1", 0.0, 0.0, 0.0)]).await;
    rig.connect().await;
    let status = rig.link.status().fixtures.unwrap();
    assert_eq!(status.link, FixtureLink::Live);
    assert_eq!(status.fixtures, 3);
    assert_eq!(status.lost_packets, 0);
    wait_for(|| rig.relay.asked("ping") >= 12, 3.0).await;
    assert!(rig.relay.asked("subscribe") >= 1);
    rig.link.stop().await;
    wait_for(|| rig.relay.asked("unsubscribe") >= 1, 2.0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_relay_that_never_answers_leaves_the_strip_as_before() {
    let rig = Rig::new(vec![]).await;
    // point the link at a UDP port nobody listens on
    drop(rig.relay.socket.clone());
    let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let port = silent.local_addr().unwrap().port();
    let link = {
        let settings = rig.settings.clone();
        let provider: stagepilot_lights::lightkey_link::SettingsProvider =
            Arc::new(move || settings.lock().unwrap().clone());
        LightkeyLink::new(
            rig.bus.clone(),
            provider,
            LinkOptions {
                relay_port: port,
                ..LinkOptions::default()
            },
        )
    };
    link.start().await;
    wait_for(|| link.status().link == Link::Connected, 3.0).await;
    pause(0.4).await;
    assert!(link.status().fixtures.is_none()); // nothing to show: cue recording is untouched
    link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn records_fixture_changes_at_song_time_with_the_clock_offset_removed() {
    let rig = Rig::new(vec![
        dim("P1", 0.0),
        color("P1", 0.0, 0.0, 0.0),
        dim("P2", 0.5),
    ])
    .await;
    rig.connect().await;
    assert_eq!(
        rig.link.set_recording(true).await,
        "Recording starts with the next song."
    );
    rig.song_start(&song(2)).await;
    assert_eq!(
        rig.link.status().message.as_deref(),
        Some("Recording Holy Forever.")
    );
    assert!(rig.link.status().fixtures.unwrap().recording);
    pause(0.5).await;
    rig.relay.change(vec![dim("P1", 1.0)], 0.0, 0).await;
    pause(0.6).await;
    rig.relay
        .change(vec![color("P1", 1.0, 0.0, 0.0)], 0.0, 0)
        .await;
    pause(1.2).await;
    rig.song_stop(&song(2), 2.0).await;
    wait_for(|| !rig.saved.lock().unwrap().is_empty(), 3.0).await;
    let saved = rig.saved.lock().unwrap().remove(0);
    assert_eq!(saved.song_key, "song-1");
    assert_eq!(saved.song_title, "Holy Forever");
    assert!(saved
        .rows
        .iter()
        .all(|row| row.source == OscCueSource::Captured && row.color.is_none()));
    let dimmer = |row: &stagepilot_core::config::OscLightingCue| {
        row.messages
            .iter()
            .any(|m| m.address == "/fixture/P1/overrides/dimmer")
    };
    // the starting state at 0.0, then the dimmer at about 0.5 s and the colour at about 1 s on
    // the 0.5 s grid; the Lightkey clock is 5 s ahead and song times still come out right
    let times: Vec<f64> = saved.rows.iter().map(|r| r.at_seconds).collect();
    assert_eq!(times.len(), 3, "{times:?}");
    assert_eq!(times[0], 0.0);
    assert!((times[1] - 0.5).abs() <= 0.2, "{times:?}");
    assert!((times[2] - 1.0).abs() <= 0.3, "{times:?}");
    assert!(dimmer(&saved.rows[1]));
    assert_eq!(saved.rows[2].messages.len(), 1);
    assert_eq!(
        saved.rows[2].messages[0].address,
        "/fixture/P1/overrides/color"
    );
    // P2 never moved: it stays with Lightkey's cues
    assert!(saved
        .rows
        .iter()
        .all(|r| r.messages.iter().all(|m| !m.address.contains("/P2/"))));
    assert_eq!(saved.rows[0].label, "Look - 1 fixture");
    assert_eq!(
        rig.link.status().message.as_deref(),
        Some("Saved 3 looks to Holy Forever's timeline.")
    );
    assert_eq!(rig.link.status().captured.unwrap().looks, 3);
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_late_pause_does_not_end_or_save_the_recording() {
    // R-01: pause at 70 % of the song, resume in place, finish: one save covering the whole song.
    let rig = Rig::new(vec![dim("P1", 0.0)]).await;
    rig.connect().await;
    rig.link.set_recording(true).await;
    rig.song_start(&song(10)).await;
    pause(0.3).await;
    rig.relay.change(vec![dim("P1", 0.4)], 0.0, 0).await;
    // The operator pauses at 7 s: a stop request with the position, then the timer's own event.
    for kind in [EventType::TimerStopRequested, EventType::TimerStopped] {
        rig.bus
            .publish(new_event(
                kind,
                "t",
                EventPayload::Timer(TimerPayload {
                    duration_seconds: 3,
                    song: Some(song(10)),
                    position_seconds: (kind == EventType::TimerStopRequested).then_some(7.0),
                    remaining_seconds: Some(3),
                    ..TimerPayload::default()
                }),
            ))
            .await;
    }
    pause(0.6).await;
    assert!(
        rig.saved.lock().unwrap().is_empty(),
        "a pause is not the end of the song"
    );
    // Resume in place, change the light, and let the song end.
    rig.bus
        .publish(new_event(
            EventType::TimerResumed,
            "t",
            EventPayload::Timer(TimerPayload {
                duration_seconds: 3,
                song: Some(song(10)),
                started_at: Some(Utc::now()),
                position_seconds: Some(7.0),
                remaining_seconds: Some(3),
                running: true,
                ..TimerPayload::default()
            }),
        ))
        .await;
    pause(0.3).await;
    rig.relay.change(vec![dim("P1", 0.9)], 0.0, 0).await;
    pause(0.3).await;
    rig.song_stop(&song(10), 10.0).await;
    wait_for(|| !rig.saved.lock().unwrap().is_empty(), 3.0).await;
    assert_eq!(rig.saved.lock().unwrap().len(), 1, "exactly one save");
    let saved = rig.saved.lock().unwrap().remove(0);
    let values: Vec<f64> = saved
        .rows
        .iter()
        .flat_map(|r| r.messages.iter())
        .filter_map(|m| match m.args.first() {
            Some(stagepilot_core::config::OscArgument::Float(x)) => Some(*x),
            _ => None,
        })
        .collect();
    let has = |x: f64| values.iter().any(|v| (v - x).abs() < 0.001);
    assert!(has(0.4) && has(0.9), "{values:?}");
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_packet_is_repaired_by_asking_for_a_keyframe() {
    let rig = Rig::new(vec![dim("P1", 0.0)]).await;
    rig.connect().await;
    rig.relay.change(vec![dim("P1", 0.5)], 0.0, 0).await;
    rig.relay.change(vec![dim("P1", 1.0)], 0.0, 3).await; // three packets never arrive
    wait_for(
        || rig.link.status().fixtures.unwrap().lost_packets == 1,
        2.0,
    )
    .await;
    wait_for(|| rig.relay.asked("keyframe") >= 1, 2.0).await;
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_recording_stores_fixture_data_only() {
    let rig = Rig::new(vec![dim("P1", 0.0)]).await;
    rig.connect().await;
    rig.link.set_recording(true).await;
    rig.song_start(&song(2)).await;
    pause(0.4).await;
    rig.relay.change(vec![dim("P1", 1.0)], 0.0, 0).await;
    pause(1.8).await;
    rig.song_stop(&song(2), 2.0).await;
    wait_for(|| !rig.saved.lock().unwrap().is_empty(), 3.0).await;
    let saved = rig.saved.lock().unwrap().remove(0);
    let json = format!("{:?}", saved.rows);
    assert!(!json.contains("Warm"), "cue names were stored: {json}");
    assert!(saved.rows.iter().all(|r| r
        .messages
        .iter()
        .all(|m| m.address.starts_with("/fixture/"))));
    rig.link.stop().await;
}

/// A recording that runs on past the song's scheduled length (events after the end) used to
/// pile into one oversize event and lose the whole capture on the rig.
#[tokio::test(flavor = "multi_thread")]
async fn a_recording_past_the_song_end_with_a_huge_change_still_saves() {
    let rig = Rig::new(vec![dim("P1", 0.0)]).await;
    rig.connect().await;
    rig.link.set_recording(true).await;
    rig.song_start(&song(2)).await;
    pause(0.3).await;
    rig.relay.change(vec![dim("P1", 1.0)], 0.0, 0).await;
    pause(1.9).await; // past the 2 s the song was scheduled for
                      // 2500 distinct addresses change after the end
    for batch in 0..50 {
        let late: Vec<FixtureCommand> = (0..50)
            .map(|n| dim(&format!("L{batch}x{n}"), 0.7))
            .collect();
        rig.relay.change(late, 0.0, 0).await;
    }
    pause(0.3).await;
    rig.song_stop(&song(2), 2.2).await;
    wait_for(|| !rig.saved.lock().unwrap().is_empty(), 3.0).await;
    let saved = rig.saved.lock().unwrap().remove(0);
    assert!(saved.rows.iter().any(|r| r
        .messages
        .iter()
        .any(|m| m.address == "/fixture/P1/overrides/dimmer")));
    assert!(saved.rows.iter().all(|r| r.at_seconds <= 2.0));
    // R-02 (late changes folded into the final row) is covered exactly by the `thin` and
    // `LookRecorder` unit tests; over UDP the relay may lose some or all of the burst, so this
    // test only checks that the capture survives it.
    rig.link.stop().await;
}

/// A song too large for the caps is fitted again at twice the tolerance, not dropped, and the
/// status message says so. (The Detail setting no longer changes the result.)
#[tokio::test(flavor = "multi_thread")]
async fn a_song_too_large_for_the_caps_is_fitted_at_twice_the_tolerance_and_says_so() {
    use stagepilot_core::config::LightkeyDetail;
    let rig = Rig::with_row_limit(vec![dim("P1", 0.0)], 5).await;
    rig.settings.lock().unwrap().3.detail = LightkeyDetail::Fine; // ignored
    rig.connect().await;
    rig.link.set_recording(true).await;
    rig.song_start(&song(2)).await;
    pause(2.0).await;
    // a 5 % zigzag in 60 ms steps (more rows than the cap at 3 %, one ramp at 6 %), delivered with the song time it happened at
    for k in 0..30u32 {
        let at = 1.9 - f64::from(k) * 0.06;
        rig.relay
            .change(vec![dim("P1", 0.1 + f64::from(k % 2) * 0.05)], at, 0)
            .await;
    }
    pause(0.2).await;
    rig.song_stop(&song(2), 2.0).await;
    wait_for(|| !rig.saved.lock().unwrap().is_empty(), 3.0).await;
    let saved = rig.saved.lock().unwrap().remove(0);
    assert!(saved.rows.len() <= 5, "{} rows", saved.rows.len());
    assert!(
        *rig.attempts.lock().unwrap() >= 2,
        "the first try was refused"
    );
    let message = rig.link.status().message.unwrap_or_default();
    assert!(message.contains("fitted loosely"), "message: {message}");
    assert!(message.starts_with("Saved "), "message: {message}");
    rig.link.stop().await;
}
