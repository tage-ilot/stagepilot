//! Behaviour tests for the lights plugin, ported from `backend/tests/test_lights_plugin.py` and the
//! plugin parts of `test_lights_osc.py`. All outputs are in-memory fakes except one real loopback
//! UDP socket test. Mapping: `docs/archive/rust-migration/phase4-test-map.md`.

use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{
    LightingCue, LightingMethod, LightsSettings, OscArgument, OscLightingCue, SongLightingCueMap,
};
use stagepilot_core::events::{new_event, EventPayload, EventType, TimerPayload};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::{ConnectionStatus, PluginStatus, Song};
use stagepilot_lights::{LightsOptions, LightsPlugin};
use stagepilot_midi::fake::{FakeOutputBackend, SentMessage};
use stagepilot_midi::output::MidiOutputBackend;
use stagepilot_osc::fake::FakeOscBackend;
use stagepilot_osc::{OscBackend, OutputError};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

fn song() -> Song {
    Song {
        id: "plan-item-1".into(),
        title: "Holy Forever".into(),
        duration_seconds: Some(300),
        order: 1,
        service_sequence: None,
        is_generic: false,
        source_song_id: Some("song-1".into()),
        description: None,
        formatted_duration: None,
    }
}

fn midi_cue(at: f64, note: u8, velocity: u8, label: &str) -> LightingCue {
    LightingCue {
        id: Uuid::new_v4(),
        at_seconds: at,
        note,
        velocity,
        label: label.into(),
        color: None,
    }
}

fn osc_cue(at: f64, address: &str, args: Vec<OscArgument>, label: &str) -> OscLightingCue {
    OscLightingCue {
        id: Uuid::new_v4(),
        at_seconds: at,
        address: address.into(),
        args,
        label: label.into(),
        color: None,
        messages: Vec::new(),
        source: Default::default(),
        edited: false,
        fade_seconds: 0.0,
    }
}

fn settings(
    method: LightingMethod,
    cues: Vec<LightingCue>,
    osc: Vec<OscLightingCue>,
) -> LightsSettings {
    let mut cue_maps = BTreeMap::new();
    cue_maps.insert(
        "song-1".to_string(),
        SongLightingCueMap {
            song_key: "song-1".into(),
            song_title: "Holy Forever".into(),
            cues,
            osc_cues: osc,
            loudness: None,
            captured_edited: false,
            capture: None,
            effects: Vec::new(),
        },
    );
    LightsSettings {
        enabled: true,
        method,
        output_name: Some("StagePilot to Lightkey".into()),
        channel: 3,
        pulse_ms: 10,
        osc_port: 9000,
        cue_maps,
        ..LightsSettings::default()
    }
}

struct Rig {
    bus: EventBus,
    plugin: LightsPlugin,
    midi: Arc<FakeOutputBackend>,
    osc: Arc<FakeOscBackend>,
}

fn rig(settings: LightsSettings) -> Rig {
    rig_with(
        settings,
        &["StagePilot to Lightkey"],
        Duration::from_secs(2),
    )
}

fn rig_with(settings: LightsSettings, names: &[&str], monitor: Duration) -> Rig {
    let bus = EventBus::new();
    let midi = FakeOutputBackend::new(names);
    let osc = Arc::new(FakeOscBackend::default());
    let mut options = LightsOptions::new({
        let midi = midi.clone();
        Arc::new(move || Ok(midi.clone() as Arc<dyn MidiOutputBackend>))
    });
    options.osc_backend_factory = {
        let osc = osc.clone();
        Arc::new(move || osc.clone() as Arc<dyn OscBackend>)
    };
    options.monitor_interval = monitor;
    let plugin = LightsPlugin::new(bus.clone(), settings, options).unwrap();
    Rig {
        bus,
        plugin,
        midi,
        osc,
    }
}

async fn publish_song_start(bus: &EventBus, duration: u32) {
    bus.publish(new_event(
        EventType::SongStarted,
        "test",
        EventPayload::Song {
            song: song(),
            index: 0,
            position_seconds: None,
            observed_at: None,
        },
    ))
    .await;
    bus.publish(new_event(
        EventType::TimerStarted,
        "propresenter",
        EventPayload::Timer(TimerPayload {
            duration_seconds: duration,
            song: Some(song()),
            ..Default::default()
        }),
    ))
    .await;
}

async fn reposition(bus: &EventBus, kind: EventType, position: f64, running: bool, duration: u32) {
    let remaining = (f64::from(duration) - position).round().max(0.0) as u32;
    bus.publish(new_event(
        kind,
        "test",
        EventPayload::Timer(TimerPayload {
            duration_seconds: remaining,
            song: Some(song()),
            position_seconds: Some(position),
            remaining_seconds: Some(remaining),
            running,
            ..Default::default()
        }),
    ))
    .await;
}

fn msg(kind: &'static str, channel: u8, note: u8, velocity: u8) -> SentMessage {
    SentMessage {
        kind,
        channel,
        note,
        velocity,
    }
}

fn notes(midi: &FakeOutputBackend) -> Vec<u8> {
    midi.messages()
        .iter()
        .filter(|m| m.kind == "note_on")
        .map(|m| m.note)
        .collect()
}

async fn wait_for(what: &str, mut predicate: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !predicate() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn aof(kind: &str, v: serde_json::Value) -> OscArgument {
    serde_json::from_value(serde_json::json!({"type": kind, "value": v})).unwrap()
}

// ---- test_lights_plugin.py ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn timer_start_sends_elapsed_cue_as_note_on_off_pulse() {
    let r = rig(settings(
        LightingMethod::Midi,
        vec![midi_cue(0.0, 72, 110, "Intro")],
        vec![],
    ));
    r.plugin.start().await.unwrap();
    publish_song_start(&r.bus, 300).await;
    wait_for("the pulse", || r.midi.messages().len() == 2).await;
    // The plugin records `last_cue` just after the note-off goes out; wait for it instead of
    // reading the snapshot in that gap (it flaked on a slow runner).
    wait_for("the recorded cue", || {
        futures_block(r.plugin.snapshot(false)).last_cue.is_some()
    })
    .await;
    let snapshot = r.plugin.snapshot(false).await;
    r.plugin.stop().await.unwrap();
    assert_eq!(
        r.midi.messages(),
        [msg("note_on", 3, 72, 110), msg("note_off", 3, 72, 0)]
    );
    assert_eq!(snapshot.connection_status, ConnectionStatus::Connected);
    assert_eq!(snapshot.last_cue.unwrap().label, "Intro");
}

#[tokio::test(flavor = "multi_thread")]
async fn output_monitor_reports_disconnect_and_reconnects() {
    let r = rig_with(
        settings(LightingMethod::Midi, vec![], vec![]),
        &["StagePilot to Lightkey"],
        Duration::from_millis(10),
    );
    r.plugin.start().await.unwrap();
    let first = r.midi.ports()[0].clone();
    r.midi.set_names(&[]);
    wait_for("disconnect", || {
        futures_block(r.plugin.snapshot(false)).connection_status == ConnectionStatus::Disconnected
    })
    .await;
    assert!(first.messages().is_empty());
    r.midi.set_names(&["StagePilot to Lightkey"]);
    wait_for("recovery", || {
        futures_block(r.plugin.snapshot(false)).connection_status == ConnectionStatus::Connected
    })
    .await;
    assert_eq!(r.midi.ports().len(), 2);
    r.plugin.stop().await.unwrap();
}

fn futures_block<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
}

#[tokio::test(flavor = "multi_thread")]
async fn unavailable_configured_output_starts_disconnected() {
    let r = rig_with(
        settings(LightingMethod::Midi, vec![], vec![]),
        &[],
        Duration::from_millis(10),
    );
    r.plugin.start().await.unwrap();
    let snapshot = r.plugin.snapshot(false).await;
    r.plugin.stop().await.unwrap();
    assert_eq!(snapshot.connection_status, ConnectionStatus::Disconnected);
    assert!(snapshot.detail.unwrap().contains("unavailable"));
}

#[tokio::test(flavor = "multi_thread")]
async fn timer_payload_carries_song_without_relying_on_subscriber_order() {
    let r = rig(settings(
        LightingMethod::Midi,
        vec![midi_cue(0.0, 71, 120, "")],
        vec![],
    ));
    r.plugin.start().await.unwrap();
    r.bus
        .publish(new_event(
            EventType::TimerStarted,
            "propresenter",
            EventPayload::Timer(TimerPayload {
                duration_seconds: 300,
                song: Some(song()),
                ..Default::default()
            }),
        ))
        .await;
    wait_for("the pulse", || r.midi.messages().len() == 2).await;
    r.plugin.stop().await.unwrap();
    assert_eq!(
        r.midi.messages(),
        [msg("note_on", 3, 71, 120), msg("note_off", 3, 71, 0)]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_or_reset_request_cancels_future_lighting_cues() {
    for kind in [
        EventType::TimerStopRequested,
        EventType::TimerResetRequested,
    ] {
        let r = rig(settings(
            LightingMethod::Midi,
            vec![midi_cue(1.0, 73, 127, "")],
            vec![],
        ));
        r.plugin.start().await.unwrap();
        publish_song_start(&r.bus, 300).await;
        r.bus
            .publish(new_event(kind, "test", EventPayload::Empty))
            .await;
        tokio::time::sleep(Duration::from_millis(80)).await;
        r.plugin.stop().await.unwrap();
        assert!(r.midi.messages().is_empty(), "{kind:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_cancels_old_timeline_and_starts_exactly_one_new_timeline() {
    let r = rig(settings(
        LightingMethod::Midi,
        vec![midi_cue(0.0, 74, 100, "")],
        vec![],
    ));
    r.plugin.start().await.unwrap();
    publish_song_start(&r.bus, 300).await;
    wait_for("first pulse", || r.midi.messages().len() == 2).await;
    r.bus
        .publish(new_event(
            EventType::SongRestarted,
            "test",
            EventPayload::Song {
                song: song(),
                index: 0,
                position_seconds: None,
                observed_at: None,
            },
        ))
        .await;
    r.bus
        .publish(new_event(
            EventType::TimerStarted,
            "propresenter",
            EventPayload::Timer(TimerPayload {
                duration_seconds: 300,
                ..Default::default()
            }),
        ))
        .await;
    wait_for("second pulse", || r.midi.messages().len() == 4).await;
    tokio::time::sleep(Duration::from_millis(40)).await;
    r.plugin.stop().await.unwrap();
    let kinds: Vec<_> = r.midi.messages().iter().map(|m| m.kind).collect();
    assert_eq!(kinds, ["note_on", "note_off", "note_on", "note_off"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn cues_after_confirmed_timer_duration_are_not_scheduled() {
    let r = rig(settings(
        LightingMethod::Midi,
        vec![midi_cue(5.0, 75, 127, "")],
        vec![],
    ));
    r.plugin.start().await.unwrap();
    publish_song_start(&r.bus, 4).await;
    tokio::time::sleep(Duration::from_millis(40)).await;
    r.plugin.stop().await.unwrap();
    assert!(r.midi.messages().is_empty());
}

fn cue(at: f64, note: u8) -> LightingCue {
    midi_cue(at, note, 100, &format!("c{note}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn resume_skips_past_cues_and_times_upcoming_from_position() {
    let r = rig(settings(
        LightingMethod::Midi,
        vec![cue(10.0, 1), cue(100.0, 2), cue(200.0, 3)],
        vec![],
    ));
    r.plugin.start().await.unwrap();
    reposition(&r.bus, EventType::TimerResumed, 99.95, true, 300).await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(
        notes(&r.midi).is_empty(),
        "nothing burst, upcoming cue not yet due"
    );
    tokio::time::sleep(Duration::from_millis(250)).await;
    r.plugin.stop().await.unwrap();
    assert_eq!(
        notes(&r.midi),
        [2],
        "past cue 1 skipped; cue 3 is 100 s away"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn seek_forward_skips_cues_between() {
    let r = rig(settings(
        LightingMethod::Midi,
        vec![cue(5.0, 1), cue(60.0, 2), cue(150.0, 3)],
        vec![],
    ));
    r.plugin.start().await.unwrap();
    reposition(&r.bus, EventType::TimerSeeked, 149.95, true, 300).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    r.plugin.stop().await.unwrap();
    assert_eq!(notes(&r.midi), [3]);
}

#[tokio::test(flavor = "multi_thread")]
async fn seek_back_replays_only_upcoming_cues_from_new_offset() {
    let r = rig(settings(
        LightingMethod::Midi,
        vec![cue(10.0, 1), cue(50.0, 2), cue(200.0, 3)],
        vec![],
    ));
    r.plugin.start().await.unwrap();
    reposition(&r.bus, EventType::TimerSeeked, 9.95, true, 300).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    r.plugin.stop().await.unwrap();
    assert_eq!(notes(&r.midi), [1]);
}

#[tokio::test(flavor = "multi_thread")]
async fn seek_while_stopped_sends_nothing_and_cancels_pending() {
    let r = rig(settings(
        LightingMethod::Midi,
        vec![cue(1.0, 1), cue(20.0, 2)],
        vec![],
    ));
    r.plugin.start().await.unwrap();
    publish_song_start(&r.bus, 300).await;
    r.bus
        .publish(new_event(
            EventType::TimerStopRequested,
            "t",
            EventPayload::Timer(TimerPayload::default()),
        ))
        .await;
    reposition(&r.bus, EventType::TimerSeeked, 19.95, false, 300).await;
    tokio::time::sleep(Duration::from_millis(1300)).await;
    r.plugin.stop().await.unwrap();
    assert!(notes(&r.midi).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn seek_to_zero_while_running_replays_full_timeline() {
    let r = rig(settings(
        LightingMethod::Midi,
        vec![cue(0.0, 1), cue(1.0, 2)],
        vec![],
    ));
    r.plugin.start().await.unwrap();
    reposition(&r.bus, EventType::TimerSeeked, 0.0, true, 300).await;
    tokio::time::sleep(Duration::from_millis(1300)).await;
    r.plugin.stop().await.unwrap();
    assert_eq!(notes(&r.midi), [1, 2]);
}

#[tokio::test(flavor = "multi_thread")]
async fn cue_exactly_at_position_boundary_is_sent() {
    let r = rig(settings(
        LightingMethod::Midi,
        vec![cue(99.0, 1), cue(100.0, 2)],
        vec![],
    ));
    r.plugin.start().await.unwrap();
    reposition(&r.bus, EventType::TimerResumed, 100.0, true, 300).await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    r.plugin.stop().await.unwrap();
    assert_eq!(notes(&r.midi), [2]);
}

#[tokio::test(flavor = "multi_thread")]
async fn position_past_end_is_clamped_and_sends_nothing() {
    let r = rig(settings(
        LightingMethod::Midi,
        vec![cue(10.0, 1), cue(299.0, 2)],
        vec![],
    ));
    r.plugin.start().await.unwrap();
    reposition(&r.bus, EventType::TimerSeeked, 400.0, true, 300).await; // remaining clamps to 0
    tokio::time::sleep(Duration::from_millis(120)).await;
    r.plugin.stop().await.unwrap();
    assert!(notes(&r.midi).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_from_stopped_state_is_harmless() {
    let r = rig(settings(LightingMethod::Midi, vec![cue(1.0, 1)], vec![]));
    r.plugin.start().await.unwrap();
    reposition(&r.bus, EventType::TimerResumed, 0.0, true, 300).await;
    for _ in 0..2 {
        r.bus
            .publish(new_event(
                EventType::TimerStopRequested,
                "t",
                EventPayload::Timer(TimerPayload::default()),
            ))
            .await;
    }
    tokio::time::sleep(Duration::from_millis(1300)).await;
    r.plugin.stop().await.unwrap();
    assert!(notes(&r.midi).is_empty());
}

// ---- test_lights_osc.py (plugin level) ----------------------------------------------------------

fn both_methods(method: LightingMethod) -> LightsSettings {
    settings(
        method,
        vec![midi_cue(0.0, 72, 100, "midi")],
        vec![osc_cue(
            0.0,
            "/cue/1",
            vec![aof("float", serde_json::json!(1.0))],
            "osc",
        )],
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn osc_method_sends_one_shot_osc_and_no_midi() {
    let r = rig(both_methods(LightingMethod::Osc));
    r.plugin.start().await.unwrap();
    publish_song_start(&r.bus, 300).await;
    wait_for("the osc cue", || !r.osc.sent().is_empty()).await;
    tokio::time::sleep(Duration::from_millis(40)).await;
    r.plugin.stop().await.unwrap();
    assert_eq!(r.osc.opened(), [("127.0.0.1".to_string(), 9000)]);
    assert_eq!(
        r.osc.sent(),
        [("/cue/1".to_string(), vec![OscArgument::Float(1.0)])]
    );
    assert!(r.midi.messages().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn midi_method_sends_midi_and_never_osc() {
    let r = rig(both_methods(LightingMethod::Midi));
    r.plugin.start().await.unwrap();
    publish_song_start(&r.bus, 300).await;
    wait_for("the pulse", || r.midi.messages().len() == 2).await;
    r.plugin.stop().await.unwrap();
    assert!(r.osc.sent().is_empty());
    assert!(r.osc.opened().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_method_cancels_timeline_and_new_method_waits_for_next_song() {
    let mut s = both_methods(LightingMethod::Osc);
    s.cue_maps.get_mut("song-1").unwrap().osc_cues[0].at_seconds = 1.0;
    let r = rig(s);
    r.plugin.start().await.unwrap();
    publish_song_start(&r.bus, 300).await;
    let switched = both_methods(LightingMethod::Midi);
    r.plugin.reconfigure(switched).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(r.osc.sent().is_empty(), "the OSC timeline was cancelled");
    assert!(r.midi.messages().is_empty(), "MIDI waits for the next song");
    publish_song_start(&r.bus, 300).await;
    wait_for("midi pulse", || r.midi.messages().len() == 2).await;
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_back_restores_kept_cues() {
    let r = rig(both_methods(LightingMethod::Midi));
    r.plugin.start().await.unwrap();
    r.plugin
        .reconfigure(both_methods(LightingMethod::Osc))
        .await;
    r.plugin
        .reconfigure(both_methods(LightingMethod::Midi))
        .await;
    publish_song_start(&r.bus, 300).await;
    wait_for("midi pulse", || r.midi.messages().len() == 2).await;
    r.plugin.stop().await.unwrap();
    assert!(r.osc.sent().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn osc_test_cue_sends_and_reports_sent_not_connected() {
    let r = rig(both_methods(LightingMethod::Osc));
    r.plugin.start().await.unwrap();
    let outcome = r
        .plugin
        .test_osc_cue("/test/go", vec![aof("int", serde_json::json!(7))])
        .await;
    assert!(outcome.accepted);
    assert_eq!(
        outcome.message,
        "Sent /test/go to 127.0.0.1:9000 (UDP, delivery not confirmed)."
    );
    assert_eq!(
        r.osc.sent(),
        [("/test/go".to_string(), vec![OscArgument::Int(7)])]
    );
    let snapshot = r.plugin.snapshot(false).await;
    assert_eq!(snapshot.connection_status, ConnectionStatus::Connected);
    assert_eq!(
        snapshot.detail.as_deref(),
        Some("Sent /test/go to 127.0.0.1:9000.")
    );
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn osc_test_cue_refused_while_midi_is_saved_and_midi_test_refused_for_osc() {
    let r = rig(both_methods(LightingMethod::Midi));
    r.plugin.start().await.unwrap();
    let outcome = r.plugin.test_osc_cue("/x", vec![]).await;
    assert!(!outcome.accepted);
    assert_eq!(
        outcome.message,
        "Lighting is set to MIDI. Switch to OSC and save first."
    );
    r.plugin.stop().await.unwrap();

    let r = rig(both_methods(LightingMethod::Osc));
    r.plugin.start().await.unwrap();
    let outcome = r.plugin.test_cue(60, 127).await;
    assert!(!outcome.accepted);
    assert_eq!(
        outcome.message,
        "Lighting is set to OSC. Switch to MIDI and save first."
    );
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn osc_test_cue_rejects_bad_address() {
    let r = rig(both_methods(LightingMethod::Osc));
    r.plugin.start().await.unwrap();
    for (address, message) in [
        ("cue/1", "An OSC address must start with /."),
        ("/cue 1", "An OSC address cannot contain spaces."),
    ] {
        let outcome = r.plugin.test_osc_cue(address, vec![]).await;
        assert_eq!(outcome.message, message);
        assert!(!outcome.accepted);
    }
    assert!(r.osc.sent().is_empty());
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn osc_send_error_is_surfaced_as_error_status() {
    let r = rig(both_methods(LightingMethod::Osc));
    r.plugin.start().await.unwrap();
    *r.osc.outputs()[0].fail_sends.lock().unwrap() =
        Some("OSC send to 127.0.0.1:9000 failed: Network is unreachable.".into());
    let outcome = r.plugin.test_osc_cue("/x", vec![]).await;
    assert!(!outcome.accepted);
    assert!(outcome.message.contains("failed"), "{}", outcome.message);
    let snapshot = r.plugin.snapshot(false).await;
    assert_eq!(snapshot.connection_status, ConnectionStatus::Error);
    assert_eq!(r.plugin.health().await.unwrap().status, PluginStatus::Error);
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_osc_host_reports_error_on_connect() {
    let r = rig(both_methods(LightingMethod::Osc));
    r.osc.fail_open(OutputError::public(
        "The OSC host \"nope\" could not be found.",
    ));
    r.plugin.start().await.unwrap();
    let snapshot = r.plugin.snapshot(false).await;
    assert_eq!(snapshot.connection_status, ConnectionStatus::Error);
    assert_eq!(
        snapshot.detail.as_deref(),
        Some("The OSC host \"nope\" could not be found.")
    );
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn osc_over_real_udp_from_timeline() {
    use std::net::UdpSocket;
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut s = both_methods(LightingMethod::Osc);
    s.osc_port = receiver.local_addr().unwrap().port();
    let bus = EventBus::new();
    let midi = FakeOutputBackend::new(&[]);
    let options = LightsOptions::new({
        let midi = midi.clone();
        Arc::new(move || Ok(midi.clone() as Arc<dyn MidiOutputBackend>))
    });
    // The default OSC backend is the real UDP one.
    let plugin = LightsPlugin::new(bus.clone(), s, options).unwrap();
    plugin.start().await.unwrap();
    publish_song_start(&bus, 300).await;
    let mut buffer = [0u8; 512];
    let (n, _) = receiver.recv_from(&mut buffer).unwrap();
    plugin.stop().await.unwrap();
    assert_eq!(
        stagepilot_osc::encode_message("/cue/1", &[OscArgument::Float(1.0)]).unwrap(),
        buffer[..n].to_vec()
    );
}

// ---- extras the Python suite lacks -------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn osc_only_setup_does_not_need_a_midi_backend() {
    let bus = EventBus::new();
    let osc = Arc::new(FakeOscBackend::default());
    let mut options = LightsOptions::new(Arc::new(|| {
        Err(stagepilot_midi::MidiError::internal("no ALSA"))
    }));
    options.osc_backend_factory = {
        let osc = osc.clone();
        Arc::new(move || osc.clone() as Arc<dyn OscBackend>)
    };
    let plugin = LightsPlugin::new(bus, both_methods(LightingMethod::Osc), options).unwrap();
    plugin.start().await.unwrap();
    let snapshot = plugin.snapshot(false).await;
    assert_eq!(snapshot.connection_status, ConnectionStatus::Ready);
    assert!(snapshot.outputs.is_empty());
    plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn disabled_lights_stay_disconnected_and_send_nothing() {
    let mut s = both_methods(LightingMethod::Osc);
    s.enabled = false;
    let r = rig(s);
    r.plugin.start().await.unwrap();
    publish_song_start(&r.bus, 300).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let snapshot = r.plugin.snapshot(false).await;
    assert_eq!(snapshot.connection_status, ConnectionStatus::Disconnected);
    assert_eq!(
        snapshot.detail.as_deref(),
        Some("Disconnected. Lighting OSC output is not configured.")
    );
    assert!(r.osc.sent().is_empty() && r.midi.messages().is_empty());
    let outcome = r
        .plugin
        .reconfigure(LightsSettings {
            enabled: false,
            ..both_methods(LightingMethod::Osc)
        })
        .await;
    assert_eq!(outcome.message, "Lighting output disabled.");
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_aborted_pulse_still_sends_its_note_off() {
    let mut s = settings(
        LightingMethod::Midi,
        vec![midi_cue(0.0, 80, 100, "")],
        vec![],
    );
    s.pulse_ms = 500;
    let r = rig(s);
    r.plugin.start().await.unwrap();
    publish_song_start(&r.bus, 300).await;
    wait_for("note on", || r.midi.messages().len() == 1).await;
    r.bus
        .publish(new_event(
            EventType::TimerStopRequested,
            "t",
            EventPayload::Empty,
        ))
        .await;
    wait_for("note off after cancel", || r.midi.messages().len() == 2).await;
    assert_eq!(r.midi.messages()[1], msg("note_off", 3, 80, 0));
    r.plugin.stop().await.unwrap();
}

// ---- m5 / m12: honest status and read-only port list ---------------------------------------------

/// A fake Lightkey helper that can be switched up and down.
struct SwitchableHelper(Arc<std::sync::atomic::AtomicBool>);

#[async_trait::async_trait]
impl stagepilot_lights::lightkey_link::LightkeyHttp for SwitchableHelper {
    async fn get_json(
        &self,
        url: &str,
        _query: &[(&str, String)],
    ) -> Result<serde_json::Value, stagepilot_lights::lightkey_link::HttpFailure> {
        use stagepilot_lights::lightkey_link::HttpFailure;
        if !self.0.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(HttpFailure::Other("the helper is down".into()));
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
            * 1000.0;
        if url.ends_with("/time") {
            Ok(serde_json::json!({ "t": now_ms }))
        } else if url.ends_with("/log") {
            Ok(serde_json::json!({
                "events": [],
                "state": { "online": true, "active": [], "seq": 1, "server_time": now_ms },
            }))
        } else {
            Err(HttpFailure::Status(404))
        }
    }
}

fn rig_with_helper(settings: LightsSettings, helper_up: Arc<std::sync::atomic::AtomicBool>) -> Rig {
    rig_with_helper_and_hold(settings, helper_up, None)
}

/// `rig_with_helper` with its own hold time for the Test cue card text (`None` = the default).
fn rig_with_helper_and_hold(
    settings: LightsSettings,
    helper_up: Arc<std::sync::atomic::AtomicBool>,
    test_cue_hold: Option<Duration>,
) -> Rig {
    let bus = EventBus::new();
    let midi = FakeOutputBackend::new(&["StagePilot to Lightkey"]);
    let osc = Arc::new(FakeOscBackend::default());
    let mut options = LightsOptions::new({
        let midi = midi.clone();
        Arc::new(move || Ok(midi.clone() as Arc<dyn MidiOutputBackend>))
    });
    options.osc_backend_factory = {
        let osc = osc.clone();
        Arc::new(move || osc.clone() as Arc<dyn OscBackend>)
    };
    if let Some(hold) = test_cue_hold {
        options.test_cue_hold = hold;
    }
    options.lightkey.http_factory = Arc::new(move || {
        Arc::new(SwitchableHelper(helper_up.clone()))
            as Arc<dyn stagepilot_lights::lightkey_link::LightkeyHttp>
    });
    let plugin = LightsPlugin::new(bus.clone(), settings, options).unwrap();
    Rig {
        bus,
        plugin,
        midi,
        osc,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn osc_is_only_ready_while_the_replay_address_is_there_but_capture_is_not() {
    let up = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let r = rig_with_helper(
        settings(
            LightingMethod::Osc,
            vec![],
            vec![osc_cue(0.0, "/live/go", vec![], "Go")],
        ),
        up,
    );
    r.plugin.start().await.unwrap();
    let before = r.plugin.snapshot(false).await;
    assert_eq!(before.connection_status, ConnectionStatus::Ready);
    assert_eq!(before.last_cue_at, None);
    assert!(before
        .detail
        .unwrap()
        .contains("Capture is not connected yet"));
    // A cue going out proves nothing about capture: the card stays Ready.
    publish_song_start(&r.bus, 300).await;
    wait_for("the cue", || !r.osc.sent().is_empty()).await;
    // The fake records the send inside the plugin's blocking send; the plugin notes the cue time
    // a moment after it returns. Wait for that note instead of assuming it is already there.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let after = loop {
        let snapshot = r.plugin.snapshot(false).await;
        if snapshot.last_cue_at.is_some() || tokio::time::Instant::now() >= deadline {
            break snapshot;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    assert!(after.last_cue_at.is_some());
    assert_eq!(after.connection_status, ConnectionStatus::Ready);
    r.plugin.stop().await.unwrap();
}

const SENT_TEXT: &str = "Sent /test/go to 127.0.0.1:9000.";

#[tokio::test(flavor = "multi_thread")]
async fn a_link_change_right_after_the_test_cue_does_not_redraw_the_card_until_the_hold_is_over() {
    use std::sync::atomic::Ordering;
    let hold = Duration::from_secs(2);
    let up = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let r = rig_with_helper_and_hold(
        settings(LightingMethod::Osc, vec![], vec![]),
        up.clone(),
        Some(hold),
    );
    r.plugin.start().await.unwrap();
    let before_cue = tokio::time::Instant::now();
    assert!(r.plugin.test_osc_cue("/test/go", vec![]).await.accepted);
    // The helper comes up now: the link changes, which wants the card to say that capture is up.
    up.store(true, Ordering::SeqCst);
    let deadline = before_cue + Duration::from_secs(30);
    let final_detail = loop {
        let snapshot = r.plugin.snapshot(false).await;
        let detail = snapshot.detail.clone().unwrap_or_default();
        let read_at = tokio::time::Instant::now();
        if detail != SENT_TEXT {
            // The card was redrawn at some time before this read; the hold started after
            // `before_cue`, so this read cannot have come earlier than the end of the hold.
            assert!(
                read_at >= before_cue + hold,
                "the card was redrawn inside the hold: {detail}"
            );
            break detail;
        }
        assert_eq!(snapshot.connection_status, ConnectionStatus::Connected);
        assert!(read_at < deadline, "the card never followed the link");
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    assert!(
        final_detail.contains("Capture and replay connected"),
        "after the hold the card follows the link: {final_detail}"
    );
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_link_change_inside_the_hold_is_not_lost_when_the_hold_ends() {
    use std::sync::atomic::Ordering;
    // The helper is up, then goes away right after the test cue: the card must end on Ready.
    let up = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = rig_with_helper_and_hold(
        settings(LightingMethod::Osc, vec![], vec![]),
        up.clone(),
        Some(Duration::from_millis(300)),
    );
    r.plugin.start().await.unwrap();
    wait_for("connected", || {
        futures_block(r.plugin.snapshot(false)).connection_status == ConnectionStatus::Connected
    })
    .await;
    assert!(r.plugin.test_osc_cue("/test/go", vec![]).await.accepted);
    up.store(false, Ordering::SeqCst);
    wait_for("back to ready after the hold", || {
        futures_block(r.plugin.snapshot(false)).connection_status == ConnectionStatus::Ready
    })
    .await;
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn with_no_hold_the_card_follows_the_link_at_once() {
    use std::sync::atomic::Ordering;
    let up = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let r = rig_with_helper_and_hold(
        settings(LightingMethod::Osc, vec![], vec![]),
        up.clone(),
        Some(Duration::ZERO),
    );
    r.plugin.start().await.unwrap();
    assert!(r.plugin.test_osc_cue("/test/go", vec![]).await.accepted);
    up.store(true, Ordering::SeqCst);
    wait_for("the card follows the link", || {
        futures_block(r.plugin.snapshot(false))
            .detail
            .is_some_and(|d| d.contains("Capture and replay connected"))
    })
    .await;
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn osc_is_connected_when_capture_and_replay_are_both_up_and_follows_the_helper() {
    use std::sync::atomic::Ordering;
    let up = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = rig_with_helper(
        settings(
            LightingMethod::Osc,
            vec![],
            vec![osc_cue(0.0, "/live/go", vec![], "Go")],
        ),
        up.clone(),
    );
    r.plugin.start().await.unwrap();
    // Nothing has been sent, yet capture and replay are both available.
    wait_for("connected", || {
        futures_block(r.plugin.snapshot(false)).connection_status == ConnectionStatus::Connected
    })
    .await;
    let connected = r.plugin.snapshot(false).await;
    assert_eq!(connected.last_cue_at, None);
    assert!(connected
        .detail
        .unwrap()
        .contains("Capture and replay connected"));
    // The helper goes away: only the replay address is left.
    up.store(false, Ordering::SeqCst);
    wait_for("back to ready", || {
        futures_block(r.plugin.snapshot(false)).connection_status == ConnectionStatus::Ready
    })
    .await;
    // And back.
    up.store(true, Ordering::SeqCst);
    wait_for("connected again", || {
        futures_block(r.plugin.snapshot(false)).connection_status == ConnectionStatus::Connected
    })
    .await;
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn midi_ports_are_listed_read_only_while_the_method_is_osc() {
    let r = rig(both_methods(LightingMethod::Osc));
    r.plugin.start().await.unwrap();
    let snapshot = r.plugin.snapshot(false).await;
    assert_eq!(snapshot.method, LightingMethod::Osc);
    let names: Vec<&str> = snapshot.outputs.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["StagePilot to Lightkey"]);
    assert!(snapshot.outputs.iter().all(|o| !o.connected));
    assert_eq!(snapshot.connection_status, ConnectionStatus::Ready);
    assert!(r.midi.ports().is_empty(), "listing must not open a port");
    r.plugin.stop().await.unwrap();
}
