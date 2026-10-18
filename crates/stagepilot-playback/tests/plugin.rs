//! Behaviour tests for the Playback plugin against the Rust fake Playback and the real core
//! (event bus, state store, state service, settings service). Each test names the rule in
//! `plugins/playback_api/plugin.py` it mirrors.

use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{IntegrationModes, MidiSource, PlaybackApiSettings};
use stagepilot_core::events::{new_event, EventPayload, EventType, StagePilotEvent};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::settings::{PersistentSettings, RuntimeSettings, SettingsService};
use stagepilot_core::state::{
    ConnectionStatus, PluginStatus, ServiceLoadStatus, ServicePlan, Song,
};
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
use stagepilot_playback::find::{
    DiscoverySource, LanCheck, ScanCandidate, ScanHooks, ScanOutcome, ScanReport,
};
use stagepilot_playback::plugin::{
    plan_key, DiscoveryState, PlaybackError, PlaybackPlugin, PluginOptions, ScanState, Scanner,
    NOT_FOUND_REASON,
};
use stagepilot_playback::testkit::FakePlayback;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Rig {
    bus: EventBus,
    store: StateStore,
    settings: Arc<SettingsService>,
    plugin: PlaybackPlugin,
    events: Arc<Mutex<Vec<StagePilotEvent>>>,
}

#[derive(Clone, Default)]
struct FakeScanner(Arc<Mutex<Option<ScanReport>>>);

impl Scanner for FakeScanner {
    fn scan(
        &self,
        _host: Option<String>,
        _port: u16,
        _hooks: ScanHooks,
    ) -> Pin<Box<dyn Future<Output = ScanReport> + Send>> {
        let report = self.0.lock().unwrap().clone().expect("scan report set");
        Box::pin(async move { report })
    }
}

fn report(outcome: ScanOutcome, candidates: Vec<ScanCandidate>) -> ScanReport {
    ScanReport {
        candidates,
        outcome,
        error_class: None,
        message: None,
        typed_host: None,
        networks: vec![],
        interfaces: vec![],
        hosts_total: 0,
        hosts_probed: 0,
        error_counts: BTreeMap::new(),
        sample_errors: vec![],
        loopback_ok: None,
        lan_check: LanCheck::Skipped,
        duration: Duration::ZERO,
        phases: vec![],
        classification: None,
        classification_inputs: serde_json::json!({}),
    }
}

fn plan() -> ServicePlan {
    ServicePlan {
        id: "plan-1".into(),
        title: "Sunday".into(),
        date: chrono::NaiveDate::from_ymd_opt(2026, 10, 18).unwrap(),
        service_type: "Weekend".into(),
        service_type_id: Some("st-1".into()),
        service_times: vec![],
        duration_source: "test".into(),
        songs: vec![
            Song::new("a", "Alpha", Some(200), 1),
            Song::new("b", "Beta", Some(210), 2),
            Song::new("c", "Gamma", Some(220), 3),
        ],
    }
}

fn settings_for(
    midi_source: MidiSource,
    host: Option<&str>,
    port: u16,
) -> (Arc<SettingsService>, RuntimeSettings) {
    let persistent = PersistentSettings {
        integration_modes: IntegrationModes {
            midi_source,
            ..IntegrationModes::default()
        },
        playback_api: PlaybackApiSettings {
            host: host.map(String::from),
            port,
            auto_scan: false,
            ..PlaybackApiSettings::default()
        },
        ..PersistentSettings::default()
    };
    let runtime = RuntimeSettings::from_persistent(&persistent);
    let service = SettingsService::ephemeral(runtime.clone());
    service.save(persistent).unwrap();
    (Arc::new(service), runtime)
}

async fn rig_with(
    midi_source: MidiSource,
    host: Option<&str>,
    port: u16,
    scanner: Option<Arc<dyn Scanner>>,
) -> Rig {
    let (settings, runtime) = settings_for(midi_source, host, port);
    let bus = EventBus::new();
    let store = StateStore::default();
    let service = StateService::new(
        bus.clone(),
        store.clone(),
        runtime.recent_event_limit,
        runtime.recent_error_limit,
    );
    service.start();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    bus.add_tap(move |e| sink.lock().unwrap().push(e.clone()));
    let mut options = PluginOptions {
        step_timeout: Duration::from_secs(2),
        ..PluginOptions::default()
    };
    if let Some(scanner) = scanner {
        options.scanner = scanner;
    }
    let plugin = PlaybackPlugin::new(
        bus.clone(),
        store.clone(),
        service.clone(),
        settings.clone(),
        options,
    );
    Rig {
        bus,
        store,
        settings,
        plugin,
        events,
    }
}

async fn rig(fake: &FakePlayback) -> Rig {
    rig_with(MidiSource::PlaybackApi, Some("127.0.0.1"), fake.port, None).await
}

impl Rig {
    async fn load_plan(&self) {
        let plan = plan();
        self.bus
            .publish(new_event(
                EventType::ServiceLoaded,
                "test",
                EventPayload::Service { plan },
            ))
            .await;
        self.bus
            .publish(new_event(
                EventType::ServiceLoadChanged,
                "test",
                EventPayload::ServiceLoad {
                    status: ServiceLoadStatus::Loaded,
                    target_date: None,
                    candidates: vec![],
                    skipped_items: vec![],
                    message: None,
                    is_stale: false,
                },
            ))
            .await;
    }

    fn connection_details(&self) -> Vec<(ConnectionStatus, String)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.event_type == EventType::ConnectionChanged)
            .filter_map(|e| match &e.payload {
                EventPayload::Connection { status, detail, .. } => {
                    Some((*status, detail.clone().unwrap_or_default()))
                }
                _ => None,
            })
            .collect()
    }
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..1500 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for: {what}");
}

fn songs(fake: &FakePlayback) {
    fake.auto_beat(Duration::from_millis(100));
    let mut s = fake.state.lock().unwrap();
    s.songs = vec![101, 102, 103];
    s.song = 102; // start in the middle so both walk directions are exercised
}

// ---- lifecycle ----------------------------------------------------------------------------

#[tokio::test]
async fn selected_playback_connects_and_publishes_the_python_sentence() {
    // plugin.py `connection` + `_publish_connection`: one plain sentence, owned by the plugin.
    let fake = FakePlayback::start().await;
    let r = rig(&fake).await;
    r.plugin.start().await.unwrap();
    until("connected", || r.plugin.connection().connected).await;
    until("event", || {
        r.connection_details().iter().any(|(s, d)| {
            *s == ConnectionStatus::Connected && d == "Connected to Playback on this computer."
        })
    })
    .await;
    let first = &r.connection_details()[0];
    assert_eq!(
        first.0,
        ConnectionStatus::Disconnected,
        "starts disconnected"
    );
    assert_eq!(first.1, "Disconnected. Looking for Playback…");
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn stop_disconnects_publishes_and_reports_stopped() {
    // plugin.py `stop` -> `_stop_selected`: ownership released, a last disconnected event.
    let fake = FakePlayback::start().await;
    let r = rig(&fake).await;
    r.plugin.start().await.unwrap();
    until("connected", || r.plugin.connection().connected).await;
    assert_eq!(
        r.plugin.health().await.unwrap().status,
        PluginStatus::Running
    );
    r.plugin.stop().await.unwrap();
    assert!(!r.plugin.connection().connected);
    assert_eq!(
        r.plugin.health().await.unwrap().status,
        PluginStatus::Stopped
    );
    let last = r.connection_details().pop().unwrap();
    assert_eq!(last.0, ConnectionStatus::Disconnected);
    // Stopping twice is harmless.
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn unselected_playback_never_connects_and_says_how_to_start() {
    // plugin.py `_start_selected`: no client unless Playback is the selected input and enabled.
    let fake = FakePlayback::start().await;
    let r = rig_with(MidiSource::Simulated, Some("127.0.0.1"), fake.port, None).await;
    r.plugin.start().await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(*fake.connections.lock().unwrap(), 0, "never dialled");
    assert_eq!(
        r.plugin.connection_reason(),
        "Disconnected. Choose Scan Network to find Playback."
    );
    assert_eq!(
        r.plugin.health().await.unwrap().status,
        PluginStatus::Running
    );
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn unreachable_playback_reports_how_to_fix_it_and_health_error() {
    // plugin.py `connection` (last_error branch) and `health` (supervisor, not connectivity).
    let fake = FakePlayback::start().await;
    let port = fake.port;
    fake.go_offline().await;
    let r = rig_with(MidiSource::PlaybackApi, Some("127.0.0.1"), port, None).await;
    r.plugin.start().await.unwrap();
    until("unreachable sentence", || {
        r.plugin
            .connection_reason()
            .starts_with("Disconnected. Looking for Playback… If it stays offline")
    })
    .await;
    let health = r.plugin.health().await.unwrap();
    assert_eq!(
        health.status,
        PluginStatus::Running,
        "health is the supervisor's"
    );
    assert_eq!(
        health.last_error.as_deref(),
        Some("Playback unavailable; remote connections may be off")
    );
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn reconnects_after_the_connection_drops() {
    // client.py reconnect + plugin `_observe`: ownership returns, connection event re-published.
    let fake = FakePlayback::start().await;
    let r = rig(&fake).await;
    r.plugin.start().await.unwrap();
    until("connected", || r.plugin.connection().connected).await;
    fake.drop_connection();
    until("dropped", || *fake.connections.lock().unwrap() >= 1).await;
    until("reconnected", || {
        *fake.connections.lock().unwrap() >= 2 && r.plugin.connection().connected
    })
    .await;
    let kinds: Vec<_> = r.connection_details().iter().map(|d| d.0).collect();
    assert!(kinds.contains(&ConnectionStatus::Connected));
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn registers_and_starts_under_the_plugin_manager() {
    // The task asked for a supervised plugin in the core manager.
    let fake = FakePlayback::start().await;
    let r = rig(&fake).await;
    let manager = stagepilot_core::plugin::PluginManager::new(r.bus.clone());
    manager.register(Arc::new(r.plugin.clone())).unwrap();
    manager.start_all().await;
    until("connected", || r.plugin.connection().connected).await;
    let health = manager.health().await;
    assert_eq!(health.len(), 1);
    assert_eq!(
        (health[0].name.as_str(), health[0].status),
        ("playback_api", PluginStatus::Running)
    );
    manager.stop_all().await;
    assert_eq!(
        r.plugin.health().await.unwrap().status,
        PluginStatus::Stopped
    );
}

// ---- discovery ----------------------------------------------------------------------------

#[tokio::test]
async fn discovery_without_a_connection_is_a_conflict() {
    // plugin.py `_check_discovery`.
    let fake = FakePlayback::start().await;
    let r = rig_with(MidiSource::Simulated, Some("127.0.0.1"), fake.port, None).await;
    r.plugin.start().await.unwrap();
    assert_eq!(
        r.plugin.discover_song_order().await,
        Err(PlaybackError::Conflict(
            "Playback is disconnected. Enable remote connections and connect first.".into()
        ))
    );
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn discovery_while_playing_is_a_conflict() {
    let fake = FakePlayback::start().await;
    songs(&fake);
    fake.state.lock().unwrap().playing = true;
    let r = rig(&fake).await;
    r.plugin.start().await.unwrap();
    until("connected", || r.plugin.connection().connected).await;
    assert_eq!(
        r.plugin.discover_song_order().await,
        Err(PlaybackError::Conflict(
            "Stop Playback before discovering song order.".into()
        ))
    );
    assert_eq!(r.plugin.discovery_snapshot().state, DiscoveryState::Idle);
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn discovery_saves_order_lengths_and_restores_the_selection() {
    // plugin.py `discover_song_order`: order, version, setlist id, lengths and the plan key are
    // saved; the original selection is restored; the mapper becomes usable.
    let fake = FakePlayback::start().await;
    songs(&fake);
    let r = rig(&fake).await;
    r.load_plan().await;
    r.plugin.start().await.unwrap();
    until("connected", || r.plugin.connection().connected).await;
    r.plugin.discover_song_order().await.unwrap();
    let snap = r.plugin.discovery_snapshot();
    assert_eq!(snap.state, DiscoveryState::Done, "{:?}", snap.error);
    assert_eq!(snap.total, 3);
    let saved = r.settings.snapshot().playback_api;
    assert_eq!(saved.song_order, vec![101, 102, 103]);
    assert_eq!(saved.captured_version, Some(1));
    assert_eq!(saved.song_lengths.len(), 3);
    assert!(saved.captured_at.is_some() && saved.lengths_measured_at.is_some());
    assert!(saved.discovery_duration_seconds.is_some());
    assert_eq!(
        saved.scan_plan_key.as_deref(),
        Some(
            format!(
                "[null, {}]",
                serde_json::to_string(&plan_key(&plan())).unwrap()
            )
            .as_str()
        )
    );
    assert_eq!(fake.state.lock().unwrap().song, 102, "selection restored");
    assert_eq!(
        fake.state.lock().unwrap().position,
        0.0,
        "back at the start"
    );
    // A fresh authoritative heartbeat validates the new order.
    until("order valid", || !r.plugin.mapper_stale()).await;
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn discovery_aborts_and_saves_nothing_when_playback_starts_playing() {
    // plugin.py `_step` -> `_check_discovery`: any playing heartbeat aborts with no result.
    let fake = FakePlayback::start().await;
    songs(&fake);
    let r = rig(&fake).await;
    r.plugin.start().await.unwrap();
    until("connected", || r.plugin.connection().connected).await;
    let state = fake.state.clone();
    let flip = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        state.lock().unwrap().playing = true;
    });
    r.plugin.discover_song_order().await.unwrap();
    flip.await.unwrap();
    let snap = r.plugin.discovery_snapshot();
    assert_eq!(snap.state, DiscoveryState::Failed);
    assert!(snap
        .error
        .unwrap()
        .ends_with("No result saved; check Playback selection."));
    assert!(r.settings.snapshot().playback_api.song_order.is_empty());
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn cancelled_discovery_reports_cancelled_and_saves_nothing() {
    // plugin.py `except asyncio.CancelledError`.
    let fake = FakePlayback::start().await;
    songs(&fake);
    let r = rig(&fake).await;
    r.plugin.start().await.unwrap();
    until("connected", || r.plugin.connection().connected).await;
    let plugin = r.plugin.clone();
    let task = tokio::spawn(async move { plugin.discover_song_order().await });
    until("running", || {
        r.plugin.discovery_snapshot().state == DiscoveryState::Running
    })
    .await;
    r.plugin.cancel_discovery();
    task.await.unwrap().unwrap();
    let snap = r.plugin.discovery_snapshot();
    assert_eq!(snap.state, DiscoveryState::Failed);
    assert_eq!(
        snap.error.as_deref(),
        Some("Song order discovery cancelled; no result saved. Check Playback selection.")
    );
    assert!(r.settings.snapshot().playback_api.song_order.is_empty());
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn settings_and_arrangement_are_refused_while_discovery_runs() {
    // plugin.py `save_settings` / `save_arrangement` / `reconfigure` guard.
    let fake = FakePlayback::start().await;
    songs(&fake);
    let r = rig(&fake).await;
    r.plugin.start().await.unwrap();
    until("connected", || r.plugin.connection().connected).await;
    let plugin = r.plugin.clone();
    let task = tokio::spawn(async move { plugin.discover_song_order().await });
    until("running", || {
        r.plugin.discovery_snapshot().state == DiscoveryState::Running
    })
    .await;
    let msg = "Wait for song discovery or Planning Center changes to finish.";
    assert_eq!(
        r.plugin.save_settings(r.settings.snapshot()).await,
        Err(PlaybackError::Conflict(msg.into()))
    );
    assert_eq!(
        r.plugin.save_arrangement(vec![], vec![]).await,
        Err(PlaybackError::Conflict(msg.into()))
    );
    assert_eq!(
        r.plugin.reconfigure().await,
        Err(PlaybackError::Conflict(
            "Wait for Discover Song Order before changing inputs.".into()
        ))
    );
    assert_eq!(
        r.plugin.find(None).await.unwrap_err(),
        PlaybackError::Conflict("Wait for Discover Song Order before scanning.".into())
    );
    r.plugin.cancel_discovery();
    task.await.unwrap().unwrap();
    r.plugin.stop().await.unwrap();
}

// ---- mapping through the plugin -------------------------------------------------------------

#[tokio::test]
async fn song_started_drives_the_timer_and_stop_stops_it() {
    // mapping.py `dispatch`: song.started -> start the mapped plan song; song.stopped -> stop.
    let fake = FakePlayback::start().await;
    songs(&fake);
    let r = rig(&fake).await;
    r.load_plan().await;
    r.plugin.start().await.unwrap();
    until("connected", || r.plugin.connection().connected).await;
    r.plugin.discover_song_order().await.unwrap();
    until("order valid", || !r.plugin.mapper_stale()).await;

    // The operator selects song 102 in Playback (arms the start), then presses play.
    fake.send_raw(r#"{"setlistSelectSong":{"setlistSongID":102}}"#);
    // Wait until the plugin has seen the selection (a poll, not a guessed delay).
    until("selection observed", || {
        r.plugin
            .events()
            .iter()
            .any(|e| e.event.kind == stagepilot_playback::normalizer::EventType::SongSelected)
    })
    .await;
    {
        let mut s = fake.state.lock().unwrap();
        s.playing = true;
        s.position = 0.5;
    }
    fake.beat();
    until("song started", || {
        r.store.snapshot().playing_song_index == Some(1)
    })
    .await;
    let state = r.store.snapshot();
    assert_eq!(
        state.playing_song_index,
        Some(1),
        "plan song 2 (Beta) is playing"
    );

    // Playback stops.
    {
        let mut s = fake.state.lock().unwrap();
        s.playing = false;
    }
    fake.beat();
    until("stop handled", || {
        r.store.snapshot().last_action.as_deref() == Some("stop_timer")
    })
    .await;
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_stale_order_never_starts_a_song_but_stop_still_works() {
    // mapping.py: stale -> no start; stop works even while stale.
    let fake = FakePlayback::start().await;
    songs(&fake);
    let r = rig(&fake).await;
    r.load_plan().await;
    r.plugin.start().await.unwrap();
    until("connected", || r.plugin.connection().connected).await;
    // No discovery: the mapper has no order and is stale.
    assert!(r.plugin.mapper_stale());
    {
        let mut s = fake.state.lock().unwrap();
        s.playing = true;
    }
    fake.beat();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(r.store.snapshot().playing_song_index, None);
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn midi_connected_but_playback_selected_keeps_playback_in_charge() {
    // arbiter: MIDI may own only when the user chose MIDI; API mode never falls back to it.
    let fake = FakePlayback::start().await;
    let r = rig(&fake).await;
    r.plugin.start().await.unwrap();
    until("connected", || r.plugin.connection().connected).await;
    r.plugin.set_midi_connected(true).await;
    assert_eq!(
        r.plugin.midi_guard().as_deref(),
        Some("ignored: MIDI is not the selected connection method")
    );
    fake.drop_connection();
    until("playback lost", || {
        !r.plugin.connection().playback_api.connected
    })
    .await;
    assert!(
        !r.plugin.connection().connected,
        "MIDI never takes over while Playback is the selected input"
    );
    r.plugin.stop().await.unwrap();
}

// ---- settings -------------------------------------------------------------------------------

#[tokio::test]
async fn save_settings_refuses_a_hand_edited_song_order() {
    // plugin.py `save_settings`.
    let fake = FakePlayback::start().await;
    let r = rig(&fake).await;
    let mut edited = r.settings.snapshot();
    edited.playback_api.song_order = vec![1, 2, 3];
    assert_eq!(
        r.plugin.save_settings(edited).await,
        Err(PlaybackError::Invalid(
            "Song order can only be changed by Discover Song Order.".into()
        ))
    );
}

#[tokio::test]
async fn save_settings_reconfigures_the_connection() {
    // plugin.py `save_settings` -> `_reconfigure`: pointing at another port reconnects there.
    let first = FakePlayback::start().await;
    let second = FakePlayback::start().await;
    second.state.lock().unwrap().song = 202;
    let r = rig(&first).await;
    r.plugin.start().await.unwrap();
    until("connected", || r.plugin.connection().connected).await;
    let mut edited = r.settings.snapshot();
    edited.playback_api.port = second.port;
    r.plugin.save_settings(edited).await.unwrap();
    until("second connection", || {
        *second.connections.lock().unwrap() >= 1
    })
    .await;
    until("reconnected", || {
        r.plugin.connection().connected
            && r.plugin.status().heartbeat.map(|h| h.song_id) == Some(202)
    })
    .await;
    assert_eq!(r.plugin.config().port, second.port);
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn save_settings_validates_the_connection_options() {
    let fake = FakePlayback::start().await;
    let r = rig(&fake).await;
    let mut edited = r.settings.snapshot();
    edited.playback_api.host = Some("bad host".into());
    assert!(matches!(
        r.plugin.save_settings(edited).await,
        Err(PlaybackError::Invalid(_))
    ));
}

#[tokio::test]
async fn arrangement_rules_match_python() {
    // plugin.py `save_arrangement`.
    let fake = FakePlayback::start().await;
    songs(&fake);
    let r = rig(&fake).await;
    r.plugin.start().await.unwrap();
    until("connected", || r.plugin.connection().connected).await;
    assert_eq!(
        r.plugin.save_arrangement(vec![], vec![]).await,
        Err(PlaybackError::Invalid(
            "Scan songs first, then choose which songs to skip.".into()
        ))
    );
    r.load_plan().await;
    r.plugin.discover_song_order().await.unwrap();
    assert_eq!(
        r.plugin.save_arrangement(vec![], vec!["zzz".into()]).await,
        Err(PlaybackError::Invalid(
            "A skipped song is not in the loaded service plan.".into()
        ))
    );
    r.plugin
        .save_arrangement(vec![103], vec!["c".into()])
        .await
        .unwrap();
    let saved = r.settings.snapshot().playback_api;
    assert_eq!(saved.skipped_song_ids, vec![103]);
    assert_eq!(saved.skipped_plan_song_ids, vec!["c".to_string()]);
    assert_eq!(saved.song_order, vec![101, 102, 103], "order untouched");
    assert_eq!(saved.lengths_applied_key, None);
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn plan_change_and_applied_keys_follow_python() {
    // plugin.py `plan_changed`, `mark_lengths_applied`, `lengths_applied`.
    let fake = FakePlayback::start().await;
    songs(&fake);
    let r = rig(&fake).await;
    r.load_plan().await;
    r.plugin.start().await.unwrap();
    until("connected", || r.plugin.connection().connected).await;
    assert!(
        !r.plugin.plan_changed(),
        "nothing scanned yet is not a change"
    );
    r.plugin.discover_song_order().await.unwrap();
    assert!(!r.plugin.plan_changed());
    assert!(!r.plugin.lengths_applied());
    r.plugin.mark_lengths_applied("plan-1").unwrap();
    assert!(r.plugin.lengths_applied());
    // A different plan makes the scan stale.
    let mut other = plan();
    other.songs.pop();
    r.bus
        .publish(new_event(
            EventType::ServiceLoaded,
            "test",
            EventPayload::Service { plan: other },
        ))
        .await;
    assert!(r.plugin.plan_changed());
    r.plugin.stop().await.unwrap();
}

// ---- scanning -------------------------------------------------------------------------------

#[tokio::test]
async fn one_scan_hit_selects_playback_and_saves_the_address() {
    // plugin.py `_scan` + `_adopt`: exactly one candidate turns Playback on and remembers it.
    let fake = FakePlayback::start().await;
    let scanner = FakeScanner::default();
    *scanner.0.lock().unwrap() = Some(report(
        ScanOutcome::Found,
        vec![ScanCandidate {
            name: "Studio".into(),
            host: "127.0.0.1".into(),
            source: DiscoverySource::Loopback,
        }],
    ));
    let r = rig_with(
        MidiSource::Simulated,
        None,
        fake.port,
        Some(Arc::new(scanner)),
    )
    .await;
    r.plugin.start().await.unwrap();
    assert_eq!(r.plugin.scan_snapshot().state, ScanState::Idle);
    r.plugin.find(None).await.unwrap();
    assert_eq!(r.plugin.connection_reason(), "Looking for Playback…");
    until("scan found", || {
        r.plugin.scan_snapshot().state == ScanState::Found
    })
    .await;
    let saved = r.settings.snapshot();
    assert_eq!(saved.integration_modes.midi_source, MidiSource::PlaybackApi);
    assert!(saved.playback_api.enabled);
    assert_eq!(saved.playback_api.host.as_deref(), Some("127.0.0.1"));
    until("connected", || r.plugin.connection().connected).await;
    assert_eq!(r.plugin.scan_snapshot().candidates.len(), 1);
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn several_scan_hits_wait_for_the_operator() {
    let scanner = FakeScanner::default();
    let hit = |host: &str| ScanCandidate {
        name: host.into(),
        host: host.into(),
        source: DiscoverySource::Lan,
    };
    *scanner.0.lock().unwrap() = Some(report(
        ScanOutcome::Found,
        vec![hit("10.0.0.2"), hit("10.0.0.3")],
    ));
    let r = rig_with(MidiSource::Simulated, None, 8080, Some(Arc::new(scanner))).await;
    r.plugin.start().await.unwrap();
    r.plugin.find(None).await.unwrap();
    until("found", || {
        r.plugin.scan_snapshot().state == ScanState::Found
    })
    .await;
    assert_eq!(r.plugin.scan_snapshot().candidates.len(), 2);
    assert_eq!(
        r.settings.snapshot().integration_modes.midi_source,
        MidiSource::Simulated,
        "nothing was selected for the operator"
    );
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn an_empty_scan_says_where_to_look() {
    let scanner = FakeScanner::default();
    *scanner.0.lock().unwrap() = Some(report(ScanOutcome::NotFound, vec![]));
    let r = rig_with(MidiSource::Simulated, None, 8080, Some(Arc::new(scanner))).await;
    r.plugin.start().await.unwrap();
    r.plugin.find(None).await.unwrap();
    until("not found", || {
        r.plugin.scan_snapshot().state == ScanState::NotFound
    })
    .await;
    assert_eq!(
        r.plugin.scan_snapshot().reason.as_deref(),
        Some(NOT_FOUND_REASON)
    );
    assert_eq!(r.plugin.connection_reason(), NOT_FOUND_REASON);
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_scan_error_class_gets_its_one_next_step() {
    let scanner = FakeScanner::default();
    let mut rep = report(ScanOutcome::Error, vec![]);
    rep.error_class = Some(stagepilot_playback::find::ErrorClass::Refused);
    *scanner.0.lock().unwrap() = Some(rep);
    let r = rig_with(MidiSource::Simulated, None, 8080, Some(Arc::new(scanner))).await;
    r.plugin.start().await.unwrap();
    r.plugin.find(Some("10.0.0.9")).await.unwrap();
    until("not found", || {
        r.plugin.scan_snapshot().state == ScanState::NotFound
    })
    .await;
    let snap = r.plugin.scan_snapshot();
    assert!(snap.typed);
    assert_eq!(
        snap.reason.as_deref(),
        Some("That computer answered, but Playback isn't accepting connections. In Playback, turn on Remote Connections.")
    );
    assert_eq!(snap.error_class.as_deref(), Some("refused"));
    r.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn find_rejects_an_invalid_typed_host_and_cancel_resets_the_scan() {
    let r = rig_with(MidiSource::Simulated, None, 8080, None).await;
    assert!(matches!(
        r.plugin.find(Some("bad host")).await,
        Err(PlaybackError::Invalid(_))
    ));
    // A blank host is "no host" (Python `host.strip() or None`).
    let scanner = FakeScanner::default();
    *scanner.0.lock().unwrap() = Some(report(ScanOutcome::NotFound, vec![]));
    let r = rig_with(MidiSource::Simulated, None, 8080, Some(Arc::new(scanner))).await;
    r.plugin.start().await.unwrap();
    r.plugin.find(Some("   ")).await.unwrap();
    assert!(!r.plugin.scan_snapshot().typed);
    r.plugin.cancel_scan().await;
    let snap = r.plugin.scan_snapshot();
    assert!(matches!(snap.state, ScanState::Idle | ScanState::NotFound));
    r.plugin.stop().await.unwrap();
}
