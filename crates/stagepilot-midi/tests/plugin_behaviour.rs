//! Behaviour tests for the Playback MIDI input plugin, ported from `backend/tests/test_midi_plugin.py`
//! (and the plugin-level parts of `test_midi_selection_api.py`). All hardware is the in-memory
//! fake. The mapping to the Python tests is in `docs/rust-migration/phase4-test-map.md`.

use async_trait::async_trait;
use stagepilot_core::api::MidiCueName;
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{MidiSettings, MidiVelocityMappings};
use stagepilot_core::events::{ActionName, EventPayload, EventType, StagePilotEvent};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::{ConnectionStatus, PluginStatus};
use stagepilot_core::state_service::ActionOutcome;
use stagepilot_midi::fake::FakeMidiBackend;
use stagepilot_midi::models::{input_id, MidiMessageDisposition};
use stagepilot_midi::plugin::{ActionDispatcher, MidiPlaybackPlugin, MidiPluginOptions};
use stagepilot_midi::{MidiBackend, MidiBackendFactory, MidiError};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[derive(Default)]
struct Recording {
    calls: Mutex<Vec<(ActionName, String)>>,
    positions: Mutex<Vec<(i64, String)>>,
}

impl Recording {
    fn calls(&self) -> Vec<(ActionName, String)> {
        lock(&self.calls).clone()
    }
    fn positions(&self) -> Vec<(i64, String)> {
        lock(&self.positions).clone()
    }
}

#[async_trait]
impl ActionDispatcher for Recording {
    async fn dispatch(&self, action: ActionName, source: &str) -> ActionOutcome {
        lock(&self.calls).push((action, source.to_string()));
        ActionOutcome::ok(format!("Accepted {}.", action.as_str()))
    }
    async fn dispatch_song_position(&self, position: i64, source: &str) -> ActionOutcome {
        lock(&self.positions).push((position, source.to_string()));
        ActionOutcome::ok(format!("Accepted song {position}."))
    }
}

struct Harness {
    plugin: MidiPlaybackPlugin,
    backend: Arc<FakeMidiBackend>,
    factory_calls: Arc<AtomicUsize>,
    dispatcher: Arc<Recording>,
    clock: Arc<Mutex<f64>>,
    events: Arc<Mutex<Vec<StagePilotEvent>>>,
}

impl Harness {
    fn advance(&self, seconds: f64) {
        *lock(&self.clock) += seconds;
    }
    fn connection_events(&self) -> Vec<(ConnectionStatus, Option<String>)> {
        lock(&self.events)
            .iter()
            .filter_map(|e| match &e.payload {
                EventPayload::Connection {
                    integration: stagepilot_core::events::Integration::Midi,
                    status,
                    detail,
                } if e.event_type == EventType::ConnectionChanged => {
                    Some((*status, detail.clone()))
                }
                _ => None,
            })
            .collect()
    }
    #[allow(clippy::type_complexity)]
    fn note_events(&self) -> Vec<(u8, u8, u8, ActionName, Option<u8>, bool, bool)> {
        lock(&self.events)
            .iter()
            .filter_map(|e| match &e.payload {
                EventPayload::MidiNote {
                    channel,
                    note,
                    velocity,
                    action,
                    song_position,
                    connection_id,
                    simulated,
                } if e.event_type == EventType::MidiNoteReceived => Some((
                    *channel,
                    *note,
                    *velocity,
                    *action,
                    *song_position,
                    *simulated,
                    connection_id.is_some(),
                )),
                _ => None,
            })
            .collect()
    }
}

fn settings(input: Option<&str>) -> MidiSettings {
    MidiSettings {
        enabled: true,
        input_name: input.map(String::from),
        channel: 1,
        debounce_ms: 250,
        ..MidiSettings::default()
    }
}

async fn harness(names: &[&str], settings: MidiSettings) -> Harness {
    let bus = EventBus::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    {
        let events = events.clone();
        bus.subscribe(None, move |event| {
            let events = events.clone();
            async move {
                lock(&events).push((*event).clone());
                Ok(())
            }
        });
    }
    let backend = FakeMidiBackend::new(names);
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let factory: MidiBackendFactory = {
        let backend = backend.clone();
        let calls = factory_calls.clone();
        Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(backend.clone() as Arc<dyn MidiBackend>)
        })
    };
    let dispatcher = Arc::new(Recording::default());
    let clock = Arc::new(Mutex::new(10.0));
    let mut options = MidiPluginOptions::new(factory);
    {
        let clock = clock.clone();
        options.clock = Arc::new(move || *lock(&clock));
    }
    options.reconnect_delays = vec![Duration::from_millis(5)];
    options.monitor_interval = Duration::from_millis(5);
    let plugin = MidiPlaybackPlugin::new(bus, settings, dispatcher.clone(), options).unwrap();
    Harness {
        plugin,
        backend,
        factory_calls,
        dispatcher,
        clock,
        events,
    }
}

async fn wait_until(what: &str, mut predicate: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !predicate() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

async fn wait_for_health(plugin: &MidiPlaybackPlugin, status: PluginStatus) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if plugin.health().await.unwrap().status == status {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for health {status:?}"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

async fn running(names: &[&str], settings: MidiSettings) -> Harness {
    let h = harness(names, settings).await;
    h.plugin.start().await.unwrap();
    wait_until("the port to open", || !h.backend.ports().is_empty()).await;
    // A note sent before the connection is active is (correctly) dropped as stale.
    wait_for_health(&h.plugin, PluginStatus::Running).await;
    h
}

#[tokio::test(flavor = "multi_thread")]
async fn start_connects_exact_input_reports_health_and_safe_discovery() {
    let h = running(&["Keyboard", "Playback"], settings(Some("Playback"))).await;
    let snapshot = h.plugin.input_snapshot(true).await;
    let playback = snapshot
        .inputs
        .iter()
        .find(|i| i.name == "Playback")
        .unwrap();

    assert_eq!(h.factory_calls.load(Ordering::SeqCst), 1);
    assert_eq!(h.backend.open_calls(), vec!["Playback"]);
    assert!(snapshot.enabled);
    assert_eq!((snapshot.channel, snapshot.note), (1, 112));
    assert_eq!(snapshot.configured_input_name.as_deref(), Some("Playback"));
    assert_eq!(playback.id, input_id("Playback"));
    assert_eq!(playback.id.len(), 64);
    assert!(playback.selected && playback.connected && !playback.ambiguous);
    let names: Vec<_> = snapshot.inputs.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["Keyboard", "Playback"]);
    let statuses: Vec<_> = h.connection_events().iter().map(|(s, _)| *s).collect();
    assert_eq!(
        statuses[statuses.len() - 2..],
        [ConnectionStatus::Connecting, ConnectionStatus::Connected]
    );

    h.plugin.start().await.unwrap(); // starting twice changes nothing
    assert_eq!(h.factory_calls.load(Ordering::SeqCst), 1);
    assert_eq!(h.backend.open_calls(), vec!["Playback"]);
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn input_id_is_the_sha256_of_the_name() {
    // Python: hashlib.sha256(b"Playback").hexdigest()
    assert_eq!(
        input_id("Playback"),
        "2db1db07062e4a013e4662a3eab15891bd2dcaffe689d97cf99a605bf6fe5b67"
    );
    assert_eq!(input_id("").len(), 64);
}

#[tokio::test(flavor = "multi_thread")]
async fn dispatches_all_six_configured_notes_in_fifo_order() {
    let h = running(&["Playback"], settings(Some("Playback"))).await;
    let port = h.backend.ports()[0].clone();
    for velocity in 100..106 {
        port.note_on(1, 112, velocity);
    }
    wait_until("six dispatches", || h.dispatcher.calls().len() == 6).await;
    let actions: Vec<_> = h.dispatcher.calls().into_iter().collect();
    assert_eq!(
        actions,
        [
            (ActionName::StartNext, "midi_playback".to_string()),
            (ActionName::RestartCurrent, "midi_playback".to_string()),
            (ActionName::Previous, "midi_playback".to_string()),
            (ActionName::Next, "midi_playback".to_string()),
            (ActionName::ReloadPlan, "midi_playback".to_string()),
            (ActionName::StopTimer, "midi_playback".to_string()),
        ]
    );
    let notes = h.note_events();
    assert_eq!(
        notes.iter().map(|n| n.3).collect::<Vec<_>>(),
        actions.iter().map(|c| c.0).collect::<Vec<_>>()
    );
    assert!(notes.iter().all(|n| !n.5), "none simulated");
    assert!(
        notes.iter().all(|n| n.6),
        "every real note carries a connection id"
    );
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn velocities_one_through_99_dispatch_exact_song_positions() {
    let h = running(&["Playback"], settings(Some("Playback"))).await;
    let port = h.backend.ports()[0].clone();
    port.note_on(1, 112, 1);
    port.note_on(1, 112, 7);
    port.note_on(1, 112, 99);
    wait_until("three positions", || h.dispatcher.positions().len() == 3).await;
    assert_eq!(
        h.dispatcher.positions(),
        [
            (1, "midi_playback".to_string()),
            (7, "midi_playback".to_string()),
            (99, "midi_playback".to_string())
        ]
    );
    assert!(h.dispatcher.calls().is_empty());
    assert_eq!(
        h.note_events().iter().map(|n| n.4).collect::<Vec<_>>(),
        [Some(1), Some(7), Some(99)]
    );
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn filters_other_channels_and_unmapped_notes_before_dispatch() {
    let h = running(&["Playback"], settings(Some("Playback"))).await;
    let port = h.backend.ports()[0].clone();
    port.note_on(2, 112, 100);
    port.note_on(1, 100, 127);
    port.note_on(1, 112, 103);
    wait_until("one dispatch", || h.dispatcher.calls().len() == 1).await;
    assert_eq!(
        h.dispatcher.calls(),
        [(ActionName::Next, "midi_playback".to_string())]
    );
    let notes: Vec<_> = h.note_events().iter().map(|n| (n.0, n.1, n.2)).collect();
    assert_eq!(notes, [(1, 112, 103)]);
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn reconfigure_changes_the_live_note_and_channel_filter() {
    let h = running(&["Playback"], settings(Some("Playback"))).await;
    let port = h.backend.ports()[0].clone();
    let outcome = h
        .plugin
        .reconfigure(MidiSettings {
            enabled: true,
            input_name: Some("Playback".into()),
            channel: 4,
            note: 124,
            debounce_ms: 0,
            ..MidiSettings::default()
        })
        .await;
    let snapshot = h.plugin.input_snapshot(false).await;
    port.note_on(4, 112, 100);
    port.note_on(1, 124, 100);
    port.note_on(4, 124, 100);
    wait_until("one dispatch", || h.dispatcher.calls().len() == 1).await;
    wait_until("three monitor rows", || {
        futures_executor_block(h.plugin.recent_messages()).len() == 3
    })
    .await;
    let messages = h.plugin.recent_messages().await;
    assert!(outcome.accepted);
    assert!(
        outcome.message.contains("channel 4, note E8 (124)"),
        "{}",
        outcome.message
    );
    assert_eq!((snapshot.channel, snapshot.note), (4, 124));
    assert_eq!(
        h.dispatcher.calls(),
        [(ActionName::StartNext, "midi_playback".to_string())]
    );
    // newest first
    let dispositions: Vec<_> = messages.iter().map(|m| m.disposition).collect();
    assert_eq!(
        dispositions,
        [
            MidiMessageDisposition::Dispatched,
            MidiMessageDisposition::WrongChannel,
            MidiMessageDisposition::Unmapped
        ]
    );
    h.plugin.stop().await.unwrap();
}

/// Poll an async accessor from a sync predicate (the accessors never block).
fn futures_executor_block<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
}

#[tokio::test(flavor = "multi_thread")]
async fn monitor_records_received_notes_and_why_they_were_ignored() {
    let h = running(&["Playback"], settings(Some("Playback"))).await;
    let port = h.backend.ports()[0].clone();
    port.note_on(2, 112, 90);
    port.note_on(1, 16, 100);
    port.note_on(1, 112, 100);
    port.note_off(1, 112);
    wait_until("one dispatch", || h.dispatcher.calls().len() == 1).await;
    wait_until("four monitor rows", || {
        futures_executor_block(h.plugin.recent_messages()).len() == 4
    })
    .await;
    let messages = h.plugin.recent_messages().await;
    let dispositions: Vec<_> = messages.iter().map(|m| m.disposition).collect();
    assert_eq!(
        dispositions,
        [
            MidiMessageDisposition::NoteRelease,
            MidiMessageDisposition::Dispatched,
            MidiMessageDisposition::Unmapped,
            MidiMessageDisposition::WrongChannel
        ]
    );
    let names: Vec<_> = messages
        .iter()
        .map(|m| (m.note, m.note_name.as_str()))
        .collect();
    assert_eq!(names, [(112, "E7"), (112, "E7"), (16, "E-1"), (112, "E7")]);
    assert_eq!(messages[1].action, Some(ActionName::StartNext));
    assert_eq!(
        messages[2].detail,
        "Ignored: StagePilot listens for MIDI note E7 (112)."
    );
    assert_eq!(messages[3].channel, 2);
    assert_eq!(messages[3].velocity, 90);
    assert_eq!(messages[3].input_name.as_deref(), Some("Playback"));
    h.plugin.stop().await.unwrap();
}

#[test]
fn note_names_follow_playback_octave_numbering() {
    for (note, name) in [
        (0, "C-2"),
        (16, "E-1"),
        (100, "E6"),
        (112, "E7"),
        (124, "E8"),
        (127, "G8"),
    ] {
        assert_eq!(MidiPlaybackPlugin::note_name(note), name);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn note_release_latching_and_monotonic_debounce_prevent_duplicates() {
    let h = running(&["Playback"], settings(Some("Playback"))).await;
    let port = h.backend.ports()[0].clone();
    port.note_on(1, 112, 100);
    port.note_on(1, 112, 100);
    port.note_off(1, 112);
    port.note_on(1, 112, 100);
    port.note_on(1, 112, 102);
    wait_until("two dispatches", || h.dispatcher.calls().len() == 2).await;
    assert_eq!(
        h.dispatcher.calls(),
        [
            (ActionName::StartNext, "midi_playback".to_string()),
            (ActionName::Previous, "midi_playback".to_string())
        ]
    );
    h.advance(0.251);
    port.note_on(1, 112, 100);
    wait_until("third dispatch", || h.dispatcher.calls().len() == 3).await;
    assert_eq!(h.dispatcher.calls()[2].0, ActionName::StartNext);
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn simulation_uses_the_mapping_event_dispatch_and_duplicate_pipeline() {
    let h = harness(&[], settings(None)).await;
    h.plugin.start().await.unwrap();
    let first = h.plugin.simulate_cue(MidiCueName::ReloadPlan).await;
    let duplicate = h.plugin.simulate_cue(MidiCueName::ReloadPlan).await;
    h.advance(0.251);
    let after = h.plugin.simulate_cue(MidiCueName::ReloadPlan).await;
    assert!(first.accepted);
    assert_eq!(
        duplicate,
        ActionOutcome::rejected("Duplicate MIDI cue ignored.")
    );
    assert!(after.accepted);
    assert_eq!(
        h.dispatcher.calls(),
        [
            (ActionName::ReloadPlan, "midi.simulation".to_string()),
            (ActionName::ReloadPlan, "midi.simulation".to_string())
        ]
    );
    let notes = h.note_events();
    assert_eq!(notes.len(), 3);
    assert!(notes.iter().all(|n| n.5), "all simulated");
    assert!(notes.iter().all(|n| !n.6), "no connection id");
    assert!(notes.iter().all(|n| n.1 == 112 && n.2 == 104));
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn simulation_rejects_an_unmapped_cue_without_publishing_or_dispatching() {
    let mut s = settings(None);
    s.mappings = MidiVelocityMappings {
        stop_timer: None,
        ..MidiVelocityMappings::default()
    };
    let h = harness(&[], s).await;
    h.plugin.start().await.unwrap();
    let outcome = h.plugin.simulate_cue(MidiCueName::StopTimer).await;
    assert_eq!(
        outcome,
        ActionOutcome::rejected("The MIDI cue \"stop_timer\" is not mapped.")
    );
    assert!(h.dispatcher.calls().is_empty());
    assert!(h.note_events().is_empty());
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_callback_is_rejected_after_reconnect() {
    let h = running(&["Playback"], settings(Some("Playback"))).await;
    let old_port = h.backend.ports()[0].clone();
    old_port.mark_closed();
    wait_until("a second port", || h.backend.ports().len() == 2).await;
    wait_for_health(&h.plugin, PluginStatus::Running).await;
    let new_port = h.backend.ports()[1].clone();
    old_port.note_on(1, 112, 100);
    new_port.note_on(1, 112, 101);
    new_port.note_on(1, 112, 102);
    wait_until("two dispatches", || h.dispatcher.calls().len() == 2).await;
    assert_eq!(
        h.dispatcher.calls(),
        [
            (ActionName::RestartCurrent, "midi_playback".to_string()),
            (ActionName::Previous, "midi_playback".to_string())
        ]
    );
    assert_eq!(
        h.note_events().iter().map(|n| n.2).collect::<Vec<_>>(),
        [101, 102]
    );
    assert_eq!(old_port.close_calls(), 1);
    h.plugin.stop().await.unwrap();
}

/// The connection-activation rule: a note that arrives before the connection is active is dropped
/// (see the comment in `test_midi_plugin.py`: "A note sent before the connection is active is
/// (correctly) dropped as stale"). The fake delivers one from inside `open_input`.
#[tokio::test(flavor = "multi_thread")]
async fn a_note_before_the_connection_is_active_is_dropped() {
    let h = harness(&["Playback"], settings(Some("Playback"))).await;
    *lock(&h.backend.emit_on_open) = Some(stagepilot_midi::MidiMessage {
        kind: stagepilot_midi::NoteType::NoteOn,
        channel: 1,
        note: 112,
        velocity: 100,
    });
    h.plugin.start().await.unwrap();
    wait_for_health(&h.plugin, PluginStatus::Running).await;
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(
        h.dispatcher.calls().is_empty(),
        "the early note must not dispatch"
    );
    assert!(h.note_events().is_empty());
    assert!(h.plugin.recent_messages().await.is_empty());
    // After activation the same note dispatches.
    h.backend.ports()[0].note_on(1, 112, 100);
    wait_until("dispatch after activation", || {
        h.dispatcher.calls().len() == 1
    })
    .await;
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_input_degrades_health_then_reconnects_when_it_appears() {
    let h = harness(&[], settings(Some("Playback"))).await;
    h.plugin.start().await.unwrap();
    wait_until("a disconnected event", || {
        h.connection_events()
            .iter()
            .any(|(s, _)| *s == ConnectionStatus::Disconnected)
    })
    .await;
    assert_eq!(
        h.plugin.health().await.unwrap().status,
        PluginStatus::Starting
    );
    assert!(h.backend.ports().is_empty());

    h.backend.add_name("Playback");
    wait_until("the port", || h.backend.ports().len() == 1).await;
    wait_for_health(&h.plugin, PluginStatus::Running).await;
    h.backend.ports()[0].note_on(1, 112, 105);
    wait_until("dispatch", || h.dispatcher.calls().len() == 1).await;
    assert_eq!(
        h.dispatcher.calls(),
        [(ActionName::StopTimer, "midi_playback".to_string())]
    );
    let statuses: Vec<_> = h.connection_events().iter().map(|(s, _)| *s).collect();
    assert_eq!(
        statuses[statuses.len() - 2..],
        [ConnectionStatus::Connecting, ConnectionStatus::Connected]
    );
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn ambiguous_discovery_is_safe_and_reports_error_health() {
    let h = harness(
        &["Zulu", "Playback", "alpha", "Playback"],
        settings(Some("Playback")),
    )
    .await;
    h.plugin.start().await.unwrap();
    wait_for_health(&h.plugin, PluginStatus::Error).await;
    let snapshot = h.plugin.input_snapshot(true).await;
    let names: Vec<_> = snapshot.inputs.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["alpha", "Playback", "Zulu"]);
    assert!(
        snapshot.inputs[1].ambiguous
            && snapshot.inputs[1].selected
            && !snapshot.inputs[1].connected
    );
    assert!(h.backend.open_calls().is_empty());
    let last = h.connection_events().pop().unwrap();
    assert_eq!(last.0, ConnectionStatus::Error);
    assert!(last.1.unwrap().contains("Multiple MIDI inputs"));
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_is_idempotent_closes_port_and_rejects_late_work() {
    let h = running(&["Playback"], settings(Some("Playback"))).await;
    let port = h.backend.ports()[0].clone();
    h.plugin.stop().await.unwrap();
    h.plugin.stop().await.unwrap();
    port.note_on(1, 112, 100);
    let outcome = h.plugin.simulate_cue(MidiCueName::StartNext).await;
    assert_eq!(port.close_calls(), 1);
    assert!(h.dispatcher.calls().is_empty());
    assert_eq!(
        outcome,
        ActionOutcome::rejected("The MIDI Playback plugin is not running.")
    );
    assert_eq!(
        h.plugin.health().await.unwrap().status,
        PluginStatus::Stopped
    );
    let last = h.connection_events().pop().unwrap();
    assert_eq!(last.0, ConnectionStatus::Disconnected);
    assert_eq!(last.1.as_deref(), Some("MIDI Playback input stopped."));
}

// ---- session selection (plugin-level parts of test_midi_selection_api.py) ----------------------

#[tokio::test(flavor = "multi_thread")]
async fn selecting_by_opaque_id_switches_and_old_callbacks_are_rejected() {
    let h = running(&["Playback", "Other"], settings(Some("Playback"))).await;
    let old = h.backend.ports()[0].clone();
    let other_id = input_id("Other");
    let outcome = h.plugin.select_input(Some(&other_id)).await;
    assert_eq!(
        outcome,
        ActionOutcome::ok("Selected \"Other\" for this session.")
    );
    wait_until("the second port", || h.backend.ports().len() == 2).await;
    wait_for_health(&h.plugin, PluginStatus::Running).await;
    let snapshot = h.plugin.input_snapshot(false).await;
    assert_eq!(snapshot.selected_input_name.as_deref(), Some("Other"));
    // The configured name is untouched; only the session changed.
    assert_eq!(snapshot.configured_input_name.as_deref(), Some("Playback"));
    old.note_on(1, 112, 100);
    h.backend.ports()[1].note_on(1, 112, 101);
    wait_until("one dispatch", || h.dispatcher.calls().len() == 1).await;
    assert_eq!(h.dispatcher.calls()[0].0, ActionName::RestartCurrent);
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn null_selection_disconnects_hardware_but_keeps_simulation_available() {
    let h = running(&["Playback"], settings(Some("Playback"))).await;
    let port = h.backend.ports()[0].clone();
    let outcome = h.plugin.select_input(None).await;
    assert_eq!(
        outcome,
        ActionOutcome::ok("MIDI input disconnected for this session.")
    );
    wait_until("port closed", || port.close_calls() == 1).await;
    let again = h.plugin.select_input(None).await;
    assert_eq!(
        again,
        ActionOutcome::ok("MIDI input is already disconnected.")
    );
    assert!(h.plugin.simulate_cue(MidiCueName::Next).await.accepted);
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_selection_is_refused_and_keeps_the_previous_choice() {
    let h = running(&["Playback"], settings(Some("Playback"))).await;
    let outcome = h.plugin.select_input(Some(&input_id("Gone"))).await;
    assert_eq!(
        outcome,
        ActionOutcome::rejected("The selected MIDI input is no longer available.")
    );
    assert_eq!(
        h.plugin
            .input_snapshot(false)
            .await
            .selected_input_name
            .as_deref(),
        Some("Playback")
    );
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn ambiguous_duplicate_name_is_rejected_without_changing_selection() {
    let h = running(&["Playback", "Twin", "Twin"], settings(Some("Playback"))).await;
    let outcome = h.plugin.select_input(Some(&input_id("Twin"))).await;
    assert_eq!(
        outcome,
        ActionOutcome::rejected("The selected MIDI input name is ambiguous.")
    );
    assert_eq!(
        h.plugin
            .input_snapshot(false)
            .await
            .selected_input_name
            .as_deref(),
        Some("Playback")
    );
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn session_selection_resets_to_configuration_when_the_plugin_restarts() {
    let h = running(&["Playback", "Other"], settings(Some("Playback"))).await;
    h.plugin.select_input(Some(&input_id("Other"))).await;
    wait_until("switched", || h.backend.ports().len() == 2).await;
    h.plugin.stop().await.unwrap();
    h.plugin.start().await.unwrap();
    wait_for_health(&h.plugin, PluginStatus::Running).await;
    assert_eq!(
        h.plugin
            .input_snapshot(false)
            .await
            .selected_input_name
            .as_deref(),
        Some("Playback")
    );
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn select_input_refuses_when_not_running() {
    let h = harness(&["Playback"], settings(Some("Playback"))).await;
    assert_eq!(
        h.plugin.select_input(None).await,
        ActionOutcome::rejected("The MIDI Playback plugin is not running.")
    );
}

// ---- extra coverage the Python suite lacks ------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_backend_that_cannot_start_is_an_error_not_a_panic() {
    let bus = EventBus::new();
    let factory: MidiBackendFactory = Arc::new(|| Err(MidiError::internal("no ALSA")));
    let plugin = MidiPlaybackPlugin::new(
        bus,
        settings(Some("Playback")),
        Arc::new(Recording::default()),
        MidiPluginOptions::new(factory),
    )
    .unwrap();
    let error = plugin.start().await.unwrap_err();
    assert_eq!(error, "The MIDI backend could not be initialized.");
    assert_eq!(plugin.health().await.unwrap().status, PluginStatus::Error);
}

#[tokio::test(flavor = "multi_thread")]
async fn queue_overflow_drops_the_new_message_and_reports_an_error_once() {
    struct Gate(tokio::sync::Notify, AtomicUsize);
    #[async_trait]
    impl ActionDispatcher for Gate {
        async fn dispatch(&self, _a: ActionName, _s: &str) -> ActionOutcome {
            self.1.fetch_add(1, Ordering::SeqCst);
            self.0.notified().await;
            ActionOutcome::ok("ok")
        }
        async fn dispatch_song_position(&self, _p: i64, _s: &str) -> ActionOutcome {
            ActionOutcome::ok("ok")
        }
    }
    let bus = EventBus::new();
    let backend = FakeMidiBackend::new(&["Playback"]);
    let factory: MidiBackendFactory = {
        let backend = backend.clone();
        Arc::new(move || Ok(backend.clone() as Arc<dyn MidiBackend>))
    };
    let gate = Arc::new(Gate(tokio::sync::Notify::new(), AtomicUsize::new(0)));
    let mut options = MidiPluginOptions::new(factory);
    options.queue_capacity = 2;
    options.monitor_interval = Duration::from_millis(5);
    let mut s = settings(Some("Playback"));
    s.debounce_ms = 0;
    let plugin = MidiPlaybackPlugin::new(bus, s, gate.clone(), options).unwrap();
    plugin.start().await.unwrap();
    wait_until("port", || !backend.ports().is_empty()).await;
    wait_for_health(&plugin, PluginStatus::Running).await;
    let port = backend.ports()[0].clone();
    port.note_on(1, 112, 100);
    wait_until("consumer blocked in dispatch", || {
        gate.1.load(Ordering::SeqCst) == 1
    })
    .await;
    for velocity in 101..106 {
        port.note_on(1, 112, velocity);
    }
    wait_until("queue full recorded", || {
        futures_executor_block(plugin.recent_messages())
            .iter()
            .any(|m| m.disposition == MidiMessageDisposition::QueueFull)
    })
    .await;
    wait_for_health(&plugin, PluginStatus::Error).await;
    gate.0.notify_waiters();
    gate.0.notify_one();
    for _ in 0..4 {
        gate.0.notify_one();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn disabled_settings_are_refused_by_reconfigure_and_stopped_plugin_rejects_it() {
    let h = harness(&["Playback"], settings(Some("Playback"))).await;
    assert_eq!(
        h.plugin.reconfigure(settings(Some("Playback"))).await,
        ActionOutcome::rejected("The MIDI Playback plugin is not running.")
    );
    h.plugin.start().await.unwrap();
    let mut off = settings(Some("Playback"));
    off.enabled = false;
    assert_eq!(
        h.plugin.reconfigure(off).await,
        ActionOutcome::rejected("MIDI is turned off in the saved settings.")
    );
    h.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn ownership_hook_rejects_a_cue_when_the_revision_moved_before_it_ran() {
    let h = running(&["Playback"], settings(Some("Playback"))).await;
    let revision = Arc::new(AtomicUsize::new(0));
    {
        let revision = revision.clone();
        h.plugin.set_ownership_revision(Some(Arc::new(move || {
            revision.load(Ordering::SeqCst) as i64
        })));
    }
    h.plugin.set_action_guard(Some(Arc::new(|| {
        Some("ignored: Playback API active".to_string())
    })));
    // The revision is captured when the message is queued; make it move afterwards by bumping it
    // from the guard's first call site: queue one message with revision 0, then bump.
    let port = h.backend.ports()[0].clone();
    revision.store(1, Ordering::SeqCst);
    port.note_on(1, 112, 100); // captured at 1, still 1 when processed: reaches the action guard
    wait_until("recorded", || {
        !futures_executor_block(h.plugin.recent_messages()).is_empty()
    })
    .await;
    let m = h.plugin.recent_messages().await;
    assert_eq!(m[0].disposition, MidiMessageDisposition::ActionRejected);
    assert_eq!(m[0].detail, "ignored: Playback API active");
    assert!(h.dispatcher.calls().is_empty());
    h.plugin.stop().await.unwrap();
}
