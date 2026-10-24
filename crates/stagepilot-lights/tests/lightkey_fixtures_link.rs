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
use stagepilot_core::config::{LightkeySettings, OscArgument};
use stagepilot_core::events::{new_event, EventPayload, EventType, TimerPayload};
use stagepilot_core::state::Song;
use stagepilot_lights::lightkey::{FixtureLink, LightkeyLink as Link};
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand};
use stagepilot_lights::lightkey_link::{
    LightkeyLink, LinkOptions, LinkSettings, MemoryTakeStore, SendCommand, TakeStore,
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

type Sent = Arc<Mutex<Vec<(f64, String, Vec<OscArgument>)>>>;

struct Rig {
    relay: FakeRelay,
    bus: EventBus,
    sent: Sent,
    store: Arc<MemoryTakeStore>,
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
            },
        )));
        let bus = EventBus::new();
        let sent: Sent = Arc::default();
        let t0 = Instant::now();
        let (sent_in, t0_in) = (sent.clone(), t0);
        let send_command: SendCommand = Arc::new(move |address: String, args: Vec<OscArgument>| {
            let sent = sent_in.clone();
            Box::pin(async move {
                sent.lock()
                    .unwrap()
                    .push((t0_in.elapsed().as_secs_f64(), address, args));
                Ok(())
            })
        });
        let provider_settings = settings.clone();
        let provider: stagepilot_lights::lightkey_link::SettingsProvider =
            Arc::new(move || provider_settings.lock().unwrap().clone());
        let store = Arc::new(MemoryTakeStore::new());
        let link = LightkeyLink::new(
            bus.clone(),
            provider,
            Arc::new(|_address: String| Box::pin(async { Ok(()) })),
            LinkOptions {
                store: store.clone() as Arc<dyn TakeStore>,
                send_command: Some(send_command),
                relay_port: relay.port,
                ..LinkOptions::default()
            },
        );
        Self {
            relay,
            bus,
            sent,
            store,
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

    fn addresses(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|(_, a, _)| a.clone())
            .collect()
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
            Arc::new(|_a: String| Box::pin(async { Ok(()) })),
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
        Some("Recording \"Holy Forever\".")
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
    wait_for(|| rig.link.has_take("song-1"), 3.0).await;
    let take = rig.store.takes().remove("song-1").unwrap();
    let track = take.fixtures.expect("fixture track");
    assert_eq!(track.events.len(), 2);
    // the Lightkey clock is 5 s ahead: song times still come out right
    assert!(
        (track.events[0].at_seconds - 0.5).abs() < 0.25,
        "{:?}",
        track.events[0]
    );
    assert!(
        (track.events[1].at_seconds - 1.1).abs() < 0.25,
        "{:?}",
        track.events[1]
    );
    assert_eq!(track.events[1].osc, vec![color("P1", 1.0, 0.0, 0.0)]);
    // P2 never moved: it stays with the cues
    assert_eq!(track.fixtures(), vec!["P1"]);
    assert!(track.initial.iter().all(|c| c.fixture() == Some("P1")));
    assert!(rig
        .link
        .status()
        .message
        .unwrap()
        .contains("2 fixture changes"));
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

fn stored_take() -> stagepilot_lights::lightkey::LightkeyTake {
    use stagepilot_lights::lightkey::LightkeyTake;
    use stagepilot_lights::lightkey_fixtures::{FixtureEvent, FixtureTrack};
    let mut take = LightkeyTake::new(
        "song-1".into(),
        "Holy Forever".into(),
        Utc::now(),
        2.0,
        vec![],
        vec![],
    )
    .unwrap();
    take.fixtures = Some(FixtureTrack {
        initial: vec![dim("P1", 0.0), color("P1", 0.0, 0.0, 0.0)],
        events: vec![
            FixtureEvent {
                at_seconds: 0.4,
                osc: vec![dim("P1", 1.0)],
            },
            FixtureEvent {
                at_seconds: 0.8,
                osc: vec![color("P1", 1.0, 0.0, 0.5), dim("MH1", 0.7)],
            },
        ],
        lead_seconds: 0.045,
    });
    take
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_sends_the_start_state_then_each_change_early_then_clears_every_fixture() {
    let rig = Rig::new(vec![]).await;
    rig.store.set(
        [("song-1".to_string(), stored_take())]
            .into_iter()
            .collect(),
    );
    rig.connect().await;
    rig.song_start(&song(2)).await;
    wait_for(
        || {
            rig.addresses()
                .iter()
                .any(|a| a.ends_with("/MH1/overrides/dimmer"))
        },
        3.0,
    )
    .await;
    let sent = rig.sent.lock().unwrap().clone();
    // the state at the start, in the recorded order, as typed arguments
    assert_eq!(sent[0].1, "/fixture/P1/overrides/dimmer");
    assert_eq!(sent[0].2, vec![OscArgument::Float(0.0)]);
    assert_eq!(sent[1].1, "/fixture/P1/overrides/color");
    assert_eq!(sent[1].2.len(), 3);
    let at = |address: &str| sent.iter().find(|s| s.1 == address).map(|s| s.0).unwrap();
    let dimmer_at = sent
        .iter()
        .filter(|s| s.1 == "/fixture/P1/overrides/dimmer")
        .nth(1)
        .unwrap()
        .0;
    let started = sent[0].0;
    assert!(
        (dimmer_at - started - (0.4 - 0.045)).abs() < 0.12,
        "{}",
        dimmer_at - started
    );
    assert!(at("/fixture/MH1/overrides/dimmer") - started > 0.7);
    // song ends: both fixtures are given back, twice over
    wait_for(
        || {
            rig.addresses()
                .iter()
                .filter(|a| a.ends_with("/overrides/clear"))
                .count()
                >= 4
        },
        4.0,
    )
    .await;
    let clears: Vec<String> = rig
        .addresses()
        .into_iter()
        .filter(|a| a.ends_with("/overrides/clear"))
        .collect();
    assert!(clears.contains(&"/fixture/P1/overrides/clear".to_string()));
    assert!(clears.contains(&"/fixture/MH1/overrides/clear".to_string()));
    assert!(!clears.iter().any(|a| a.contains('*')));
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_the_song_gives_the_fixtures_back_at_once() {
    let rig = Rig::new(vec![]).await;
    rig.store.set(
        [("song-1".to_string(), stored_take())]
            .into_iter()
            .collect(),
    );
    rig.connect().await;
    rig.song_start(&song(10)).await;
    wait_for(|| rig.addresses().len() >= 2, 3.0).await;
    rig.song_stop(&song(10), 0.5).await;
    wait_for(
        || {
            rig.addresses()
                .iter()
                .any(|a| a == "/fixture/P1/overrides/clear")
        },
        2.0,
    )
    .await;
    let before = rig.sent.lock().unwrap().len();
    pause(1.2).await; // the 0.8 s change must not fire after the stop
    let after: Vec<String> = rig.addresses()[before..].to_vec();
    assert!(
        !after.iter().any(|a| a.ends_with("/MH1/overrides/dimmer")),
        "{after:?}"
    );
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_switched_off_or_recording_armed_sends_no_fixture_commands() {
    let rig = Rig::new(vec![]).await;
    rig.store.set(
        [("song-1".to_string(), stored_take())]
            .into_iter()
            .collect(),
    );
    rig.connect().await;
    rig.settings.lock().unwrap().3.replay = false;
    rig.song_start(&song(2)).await;
    pause(0.6).await;
    assert!(rig.sent.lock().unwrap().is_empty());
    rig.settings.lock().unwrap().3.replay = true;
    rig.link.set_recording(true).await;
    rig.song_start(&song(2)).await;
    pause(0.6).await;
    assert!(rig.sent.lock().unwrap().is_empty());
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_replay_off_during_a_song_releases_the_fixtures() {
    let rig = Rig::new(vec![]).await;
    rig.store.set(
        [("song-1".to_string(), stored_take())]
            .into_iter()
            .collect(),
    );
    rig.connect().await;
    rig.song_start(&song(10)).await;
    wait_for(|| rig.addresses().len() >= 2, 3.0).await;
    rig.settings.lock().unwrap().3.replay = false;
    rig.link.refresh_replay().await;
    wait_for(
        || {
            rig.addresses()
                .iter()
                .any(|a| a == "/fixture/P1/overrides/clear")
        },
        2.0,
    )
    .await;
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_old_take_without_fixtures_still_loads_and_replays_cues_only() {
    let text = r#"{"takes":[{"song_key":"song-1","song_title":"Holy Forever","recorded_at":"2026-10-20T20:00:00Z","duration_seconds":10.0,"initial":["Warm"],"edges":[{"at_seconds":0.5,"added":["Stars"],"removed":[]}]}]}"#;
    let file: serde_json::Value = serde_json::from_str(text).unwrap();
    let take: stagepilot_lights::lightkey::LightkeyTake =
        serde_json::from_value(file["takes"][0].clone()).unwrap();
    assert!(take.fixtures.is_none());
    let again = serde_json::to_string(&take).unwrap();
    assert!(!again.contains("fixtures")); // nothing new written for a take that has none
    let summary = stagepilot_lights::lightkey::summarize(&take);
    assert_eq!(summary.fixture_count, None);
}

// ------------------------------------------------------------------ paced bursts (m4)

fn big_take(count: usize) -> stagepilot_lights::lightkey::LightkeyTake {
    use stagepilot_lights::lightkey::LightkeyTake;
    use stagepilot_lights::lightkey_fixtures::FixtureTrack;
    let mut take = LightkeyTake::new(
        "song-1".into(),
        "Holy Forever".into(),
        Utc::now(),
        10.0,
        vec![],
        vec![],
    )
    .unwrap();
    take.fixtures = Some(FixtureTrack {
        initial: (0..count).map(|n| dim(&format!("F{n:02}"), 0.5)).collect(),
        events: vec![],
        lead_seconds: 0.045,
    });
    take
}

#[tokio::test(flavor = "multi_thread")]
async fn the_replay_start_burst_is_paced_to_200_packets_a_second_in_order() {
    let rig = Rig::new(vec![]).await;
    rig.store
        .set([("song-1".to_string(), big_take(60))].into_iter().collect());
    rig.connect().await;
    rig.song_start(&song(10)).await;
    wait_for(|| rig.sent.lock().unwrap().len() >= 60, 5.0).await;
    let sent = rig.sent.lock().unwrap().clone();
    let expected: Vec<String> = (0..60)
        .map(|n| format!("/fixture/F{n:02}/overrides/dimmer"))
        .collect();
    let got: Vec<String> = sent.iter().take(60).map(|s| s.1.clone()).collect();
    assert_eq!(got, expected); // order kept
    let span = sent[59].0 - sent[0].0;
    // 60 packets at <= 200/s take at least 295 ms (allow timer slack)
    assert!(span >= 0.27, "60 packets went out in {span:.4} s");
    rig.link.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_mid_burst_cancels_the_rest_of_the_burst() {
    let rig = Rig::new(vec![]).await;
    rig.store
        .set([("song-1".to_string(), big_take(60))].into_iter().collect());
    rig.connect().await;
    rig.song_start(&song(10)).await;
    wait_for(|| rig.sent.lock().unwrap().len() >= 8, 5.0).await;
    rig.song_stop(&song(10), 0.1).await;
    pause(0.8).await;
    let sets = rig
        .addresses()
        .iter()
        .filter(|a| a.ends_with("/overrides/dimmer"))
        .count();
    assert!(sets < 60, "{sets} of 60 commands were sent after the stop");
    rig.link.stop().await;
}
