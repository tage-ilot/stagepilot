//! Byte-for-byte equality with python-osc for every OSC message the Lightkey replay sends.
//!
//! `tests/fixtures/lightkey_osc_golden.json` is produced by
//! `tools/rust-output/gen_lightkey_osc_golden.py`: for each take and start position, Python's real
//! `replay_plan` output goes through the real `UdpOscOutputBackend` (python-osc) to a loopback
//! socket and the received datagram is recorded as hex. Here the Rust `replay_plan` must produce
//! the same commands in the same order, the Rust encoder must produce the same bytes, and the
//! full chain (event bus -> Lightkey link -> `LightsPlugin` OSC output) must put exactly those
//! datagrams on the wire. No socket other than the in-memory fake is used.

use chrono::Utc;
use serde_json::Value;
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::LightsSettings;
use stagepilot_core::events::{new_event, EventPayload, EventType, TimerPayload};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::Song;
use stagepilot_lights::lightkey::{osc_address, replay_plan, LightkeyTake, Verb};
use stagepilot_lights::lightkey_link::{MemoryTakeStore, TakeStore};
use stagepilot_lights::{LightsOptions, LightsPlugin};
use stagepilot_midi::fake::FakeOutputBackend;
use stagepilot_midi::output::MidiOutputBackend;
use stagepilot_osc::fake::FakeOscBackend;
use stagepilot_osc::{encode_message, OscBackend};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Case {
    name: String,
    take: LightkeyTake,
    position: f64,
    sync_now: usize,
    packets: Vec<Packet>,
}

struct Packet {
    cue: String,
    verb: Verb,
    at_seconds: f64,
    address: String,
    hex: String,
}

fn cases() -> Vec<Case> {
    let text = include_str!("fixtures/lightkey_osc_golden.json");
    let items: Vec<Value> = serde_json::from_str(text).expect("golden json");
    items
        .into_iter()
        .map(|item| {
            let mut take: LightkeyTake =
                serde_json::from_value(item["take"].clone()).expect("take");
            take.normalise_and_validate().expect("valid take");
            Case {
                name: item["name"].as_str().unwrap().to_string(),
                take,
                position: item["position"].as_f64().unwrap(),
                sync_now: item["sync_now"].as_u64().unwrap() as usize,
                packets: item["packets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| Packet {
                        cue: p["cue"].as_str().unwrap().to_string(),
                        verb: match p["verb"].as_str().unwrap() {
                            "activate" => Verb::Activate,
                            _ => Verb::Deactivate,
                        },
                        at_seconds: p["at_seconds"].as_f64().unwrap(),
                        address: p["address"].as_str().unwrap().to_string(),
                        hex: p["hex"].as_str().unwrap().to_string(),
                    })
                    .collect(),
            }
        })
        .collect()
}

#[test]
fn golden_set_is_not_trivially_small() {
    let all = cases();
    assert!(all.len() >= 9);
    assert!(all.iter().map(|c| c.packets.len()).sum::<usize>() >= 100);
    assert!(all.iter().any(|c| c.name == "awkward cue names"));
}

#[test]
fn every_replay_command_matches_python_and_encodes_to_the_python_bytes() {
    for case in cases() {
        let (sync_now, later) = replay_plan(&case.take, case.position);
        assert_eq!(
            sync_now.len(),
            case.sync_now,
            "{} @ {}",
            case.name,
            case.position
        );
        let commands: Vec<_> = sync_now.iter().chain(later.iter()).collect();
        assert_eq!(
            commands.len(),
            case.packets.len(),
            "{} @ {}: command count",
            case.name,
            case.position
        );
        for (command, want) in commands.iter().zip(&case.packets) {
            let label = format!("{} @ {}: {:?}", case.name, case.position, want.cue);
            assert_eq!(command.cue, want.cue, "{label}");
            assert_eq!(command.verb, want.verb, "{label}");
            assert_eq!(command.at_seconds, want.at_seconds, "{label}");
            let address = osc_address(&command.cue, command.verb);
            assert_eq!(address, want.address, "{label}: address");
            let bytes = encode_message(&address, &[]).expect("encodes");
            assert_eq!(hex::encode(bytes), want.hex, "{label}: datagram");
        }
    }
}

async fn wait_for(check: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not reached in time");
}

/// Start playback at `position` through the real plugin and return the datagrams that reached
/// its (in-memory) OSC output.
async fn replayed_datagrams(take: &LightkeyTake, position: f64, expected: usize) -> Vec<Vec<u8>> {
    let bus = EventBus::new();
    let midi = FakeOutputBackend::new(&["Out"]);
    let osc = Arc::new(FakeOscBackend::default());
    let store = Arc::new(MemoryTakeStore::new());
    store.set(std::iter::once((take.song_key.clone(), take.clone())).collect());
    let mut options = LightsOptions::new(Arc::new(move || {
        Ok(midi.clone() as Arc<dyn MidiOutputBackend>)
    }));
    options.osc_backend_factory = {
        let osc = osc.clone();
        Arc::new(move || osc.clone() as Arc<dyn OscBackend>)
    };
    options.lightkey.store = store as Arc<dyn TakeStore>;
    let settings = LightsSettings {
        enabled: true,
        ..LightsSettings::default()
    };
    let plugin = LightsPlugin::new(bus.clone(), settings, options).expect("plugin");
    plugin.start().await.expect("starts");
    let song = Song {
        id: "item".into(),
        title: take.song_title.clone(),
        duration_seconds: Some(take.duration_seconds as u32),
        order: 1,
        service_sequence: None,
        is_generic: false,
        source_song_id: Some(take.song_key.clone()),
        description: None,
        formatted_duration: None,
    };
    bus.publish(new_event(
        EventType::TimerSeeked,
        "test",
        EventPayload::Timer(TimerPayload {
            duration_seconds: take.duration_seconds as u32,
            song: Some(song),
            position_seconds: Some(position),
            running: true,
            started_at: Some(Utc::now()),
            ..TimerPayload::default()
        }),
    ))
    .await;
    let output = || {
        osc.outputs()
            .into_iter()
            .flat_map(|o| o.packets.lock().unwrap().clone())
            .collect::<Vec<_>>()
    };
    wait_for(|| output().len() >= expected).await;
    let datagrams = output();
    plugin.stop().await.expect("stops");
    datagrams
}

#[tokio::test(flavor = "multi_thread")]
async fn the_link_puts_the_python_datagrams_on_the_wire_in_order() {
    // Only cases whose last command is due within 1.5 s of the start position are timed through
    // the whole chain (the test must not wait minutes); the others are covered command by command
    // above.
    let mut ran = 0;
    for case in cases() {
        if case
            .packets
            .iter()
            .any(|p| p.at_seconds - case.position > 1.5)
        {
            continue;
        }
        let got = replayed_datagrams(&case.take, case.position, case.packets.len()).await;
        let want: Vec<String> = case.packets.iter().map(|p| p.hex.clone()).collect();
        let got: Vec<String> = got.iter().map(hex::encode).collect();
        assert_eq!(got, want, "{} @ {}", case.name, case.position);
        ran += 1;
    }
    assert!(ran >= 3, "only {ran} cases went through the whole chain");
}
