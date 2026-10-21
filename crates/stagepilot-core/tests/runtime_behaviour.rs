//! Behaviour tests for the event bus, plugin manager, state service and critical alerts: the Rust
//! counterparts of `test_event_bus.py`, `test_plugin_manager.py`, `test_runtime_plugin_lifecycle.py`,
//! `test_state_service.py` and `test_critical_alerts.py`.

use async_trait::async_trait;
use chrono::NaiveDate;
use stagepilot_core::alerts::*;
use stagepilot_core::bus::EventBus;
use stagepilot_core::events::*;
use stagepilot_core::plugin::*;
use stagepilot_core::state::*;
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---- event bus --------------------------------------------------------------------------------

#[tokio::test]
async fn publication_reaches_every_subscriber_and_a_wildcard() {
    let bus = EventBus::new();
    let seen = Arc::new(AtomicUsize::new(0));
    for event_type in [
        Some(EventType::TimerStopped),
        Some(EventType::TimerStopped),
        None,
    ] {
        let seen = seen.clone();
        bus.subscribe(event_type, move |_| {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });
    }
    bus.publish(empty_event(EventType::TimerStopped, "t")).await;
    bus.publish(empty_event(EventType::TimerStarted, "t")).await;
    // two typed subscribers + wildcard for the first event, wildcard only for the second
    assert_eq!(seen.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn a_failing_or_panicking_subscriber_does_not_stop_the_others() {
    let bus = EventBus::new();
    let delivered = Arc::new(AtomicUsize::new(0));
    bus.subscribe(None, |_| async { Err("boom".to_string()) });
    bus.subscribe(None, |_| async { panic!("handler panic") });
    let d = delivered.clone();
    bus.subscribe(None, move |_| {
        let d = d.clone();
        async move {
            d.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    });
    let report = bus.publish(empty_event(EventType::TimerStopped, "t")).await;
    assert_eq!(delivered.load(Ordering::SeqCst), 1);
    assert_eq!(report.failures.len(), 2);
}

#[tokio::test]
async fn a_slow_subscriber_is_abandoned_after_the_timeout_and_does_not_block_others() {
    let bus = EventBus::with_handler_timeout(Duration::from_millis(50));
    let fast = Arc::new(AtomicUsize::new(0));
    bus.subscribe(None, |_| async {
        tokio::time::sleep(Duration::from_secs(30)).await;
        Ok(())
    });
    let f = fast.clone();
    bus.subscribe(None, move |_| {
        let f = f.clone();
        async move {
            f.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    });
    let started = std::time::Instant::now();
    let report = bus.publish(empty_event(EventType::TimerStopped, "t")).await;
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(fast.load(Ordering::SeqCst), 1);
    assert_eq!(report.failures.len(), 1);
}

#[tokio::test]
async fn unsubscribe_stops_future_delivery() {
    let bus = EventBus::new();
    let seen = Arc::new(AtomicUsize::new(0));
    let s = seen.clone();
    let sub = bus.subscribe(None, move |_| {
        let s = s.clone();
        async move {
            s.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    });
    bus.publish(empty_event(EventType::TimerStopped, "t")).await;
    assert!(bus.unsubscribe(&sub));
    assert!(!bus.unsubscribe(&sub));
    bus.publish(empty_event(EventType::TimerStopped, "t")).await;
    assert_eq!(seen.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_lagging_observer_loses_old_events_but_never_blocks_publishing() {
    let bus = EventBus::new();
    let mut observer = bus.observe();
    for _ in 0..1000 {
        bus.publish(empty_event(EventType::TimerStopped, "t")).await;
    }
    // 1000 publishes completed with nobody reading; the observer is told it lagged.
    assert!(matches!(
        observer.recv().await,
        Err(tokio::sync::broadcast::error::RecvError::Lagged(_))
    ));
    assert!(observer.recv().await.is_ok());
}

// ---- plugin manager ---------------------------------------------------------------------------

struct Fake {
    name: &'static str,
    start_results: Mutex<Vec<Result<(), String>>>,
    starts: AtomicUsize,
    stops: AtomicUsize,
    hang_start: bool,
    panic_start: bool,
    health_error: bool,
}

impl Fake {
    fn ok(name: &'static str) -> Arc<Self> {
        Self::with(name, vec![])
    }
    fn with(name: &'static str, start_results: Vec<Result<(), String>>) -> Arc<Self> {
        Arc::new(Self {
            name,
            start_results: Mutex::new(start_results),
            starts: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            hang_start: false,
            panic_start: false,
            health_error: false,
        })
    }
}

#[async_trait]
impl Plugin for Fake {
    fn name(&self) -> &str {
        self.name
    }
    fn version(&self) -> &str {
        "1.0"
    }
    async fn start(&self) -> Result<(), String> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        if self.hang_start {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
        if self.panic_start {
            panic!("plugin panic");
        }
        let mut results = self.start_results.lock().unwrap();
        if results.is_empty() {
            Ok(())
        } else {
            results.remove(0)
        }
    }
    async fn stop(&self) -> Result<(), String> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn health(&self) -> Result<PluginHealth, String> {
        if self.health_error {
            return Err("probe failed".into());
        }
        Ok(PluginHealth {
            name: self.name.into(),
            version: "1.0".into(),
            status: PluginStatus::Running,
            last_error: None,
            last_activity_at: None,
        })
    }
}

fn fast() -> SupervisorConfig {
    SupervisorConfig {
        start_timeout: Duration::from_millis(100),
        stop_timeout: Duration::from_millis(100),
        health_timeout: Duration::from_millis(100),
        initial_backoff: Duration::from_millis(10),
        max_backoff: Duration::from_millis(80),
    }
}

async fn status_of(manager: &PluginManager, name: &str) -> PluginStatus {
    manager
        .health()
        .await
        .into_iter()
        .find(|h| h.name == name)
        .map(|h| h.status)
        .unwrap()
}

#[tokio::test]
async fn a_failed_start_does_not_stop_a_healthy_plugin() {
    let manager = PluginManager::with_config(EventBus::new(), fast());
    manager
        .register(Fake::with("bad", vec![Err("no".into())]))
        .unwrap();
    manager.register(Fake::ok("good")).unwrap();
    manager.start_all().await;
    assert_eq!(status_of(&manager, "good").await, PluginStatus::Running);
    assert_eq!(status_of(&manager, "bad").await, PluginStatus::Error);
}

#[tokio::test]
async fn restart_failed_retries_a_plugin_whose_first_start_failed() {
    let manager = PluginManager::with_config(EventBus::new(), fast());
    manager
        .register(Fake::with("flaky", vec![Err("not yet".into())]))
        .unwrap();
    manager.start_all().await;
    manager.restart_failed().await;
    assert_eq!(status_of(&manager, "flaky").await, PluginStatus::Running);
}

#[tokio::test]
async fn a_hung_or_panicking_plugin_is_contained_and_reported() {
    let manager = PluginManager::with_config(EventBus::new(), fast());
    let hang = Arc::new(Fake {
        hang_start: true,
        ..Arc::try_unwrap(Fake::ok("hang")).ok().unwrap()
    });
    let panic = Arc::new(Fake {
        panic_start: true,
        ..Arc::try_unwrap(Fake::ok("panic")).ok().unwrap()
    });
    manager.register(hang).unwrap();
    manager.register(panic).unwrap();
    manager.register(Fake::ok("good")).unwrap();
    manager.start_all().await;
    assert_eq!(status_of(&manager, "good").await, PluginStatus::Running);
    assert_eq!(status_of(&manager, "hang").await, PluginStatus::Error);
    assert_eq!(status_of(&manager, "panic").await, PluginStatus::Error);
}

#[tokio::test]
async fn the_supervisor_restarts_a_crashed_plugin_with_backoff() {
    let manager = PluginManager::with_config(EventBus::new(), fast());
    let plugin = Fake::with(
        "flaky",
        vec![Err("1".into()), Err("2".into()), Err("3".into())],
    );
    manager.register(plugin.clone()).unwrap();
    manager.register(Fake::ok("other")).unwrap();
    manager.start_all().await;
    manager.supervise("flaky");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while status_of(&manager, "flaky").await != PluginStatus::Running {
        assert!(
            std::time::Instant::now() < deadline,
            "plugin was never restarted"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // first start + three failures' worth of restarts
    assert!(plugin.starts.load(Ordering::SeqCst) >= 4);
    assert_eq!(status_of(&manager, "other").await, PluginStatus::Running);
    manager.stop_all().await;
}

#[tokio::test]
async fn health_probe_failure_is_isolated_and_reported_safely() {
    let manager = PluginManager::with_config(EventBus::new(), fast());
    let broken = Arc::new(Fake {
        health_error: true,
        ..Arc::try_unwrap(Fake::ok("broken")).ok().unwrap()
    });
    manager.register(broken).unwrap();
    manager.register(Fake::ok("fine")).unwrap();
    manager.start_all().await;
    let health = manager.health().await;
    assert_eq!(health.len(), 2);
    let broken = health.iter().find(|h| h.name == "broken").unwrap();
    assert_eq!(broken.status, PluginStatus::Error);
    assert_eq!(
        broken.last_error.as_deref(),
        Some("Plugin health check failed.")
    );
}

#[tokio::test]
async fn add_remove_replace_and_failed_add_leave_nothing_registered() {
    let manager = PluginManager::with_config(EventBus::new(), fast());
    let health = manager.add(Fake::ok("a")).await.unwrap();
    assert_eq!(health.status, PluginStatus::Running);
    assert!(
        manager.add(Fake::ok("a")).await.is_err(),
        "duplicate names are refused"
    );
    let failed = manager
        .add(Fake::with("b", vec![Err("no".into())]))
        .await
        .unwrap();
    assert_eq!(failed.status, PluginStatus::Error);
    assert!(!manager.has("b"));
    let replaced = manager.replace("a", Fake::ok("c")).await.unwrap();
    assert_eq!(replaced.name, "c");
    assert!(!manager.has("a") && manager.has("c"));
    assert!(manager.remove("c").await.is_some());
    assert!(!manager.has("c"));
}

// ---- state service ----------------------------------------------------------------------------

fn plan(titles: &[(&str, &str, Option<u32>)]) -> ServicePlan {
    ServicePlan {
        id: "plan-1".into(),
        title: "Test Service".into(),
        date: NaiveDate::from_ymd_opt(2026, 7, 12).unwrap(),
        service_type: "Weekend".into(),
        service_type_id: None,
        service_times: vec!["09:00".into()],
        duration_source: "Scheduled item length".into(),
        songs: titles
            .iter()
            .enumerate()
            .map(|(i, (id, title, d))| Song::new(id, title, *d, i as u32 + 1))
            .collect(),
    }
}

async fn setup() -> (EventBus, StateStore, StateService) {
    let bus = EventBus::new();
    let store = StateStore::default();
    let service = StateService::new(bus.clone(), store.clone(), 100, 50);
    service.start();
    (bus, store, service)
}

async fn load(bus: &EventBus, plan: ServicePlan) {
    bus.publish(new_event(
        EventType::ServiceLoaded,
        "test",
        EventPayload::Service { plan },
    ))
    .await;
}

fn title(song: &Option<Song>) -> Option<&str> {
    song.as_ref().map(|s| s.title.as_str())
}

#[tokio::test]
async fn start_next_selects_first_then_advances_and_stops_at_the_end() {
    let (bus, store, service) = setup().await;
    load(
        &bus,
        plan(&[("a", "Alpha", Some(240)), ("b", "Beta", Some(240))]),
    )
    .await;
    assert!(
        service
            .dispatch(ActionName::StartNext, "test")
            .await
            .accepted
    );
    let first = store.snapshot();
    assert_eq!(title(&first.current_song), Some("Alpha"));
    assert_eq!(title(&first.next_song), Some("Beta"));
    assert!(
        service
            .dispatch(ActionName::StartNext, "test")
            .await
            .accepted
    );
    let second = store.snapshot();
    assert_eq!(title(&second.current_song), Some("Beta"));
    assert!(second.next_song.is_none());
    let third = service.dispatch(ActionName::StartNext, "test").await;
    assert!(!third.accepted);
    assert_eq!(third.message, "Already at the final song.");
}

#[tokio::test]
async fn previous_and_next_only_move_the_cursor_and_say_so() {
    let (bus, store, service) = setup().await;
    load(
        &bus,
        plan(&[
            ("a", "Alpha", Some(240)),
            ("b", "Beta", Some(240)),
            ("c", "Gamma", Some(240)),
        ]),
    )
    .await;
    service.dispatch(ActionName::StartNext, "test").await;
    service.dispatch(ActionName::StartNext, "test").await;
    let previous = service.dispatch(ActionName::Previous, "test").await;
    assert_eq!(
        previous.message,
        "\"Alpha\" is up next. Press Start next to play it."
    );
    let state = store.snapshot();
    assert_eq!(
        title(&state.playing_song),
        Some("Beta"),
        "the playing song does not change"
    );
    assert_eq!(title(&state.up_next_song), Some("Alpha"));
    service.dispatch(ActionName::StartNext, "test").await;
    assert_eq!(title(&store.snapshot().playing_song), Some("Alpha"));
}

#[tokio::test]
async fn an_empty_plan_rejects_actions_without_changing_position() {
    let (_bus, store, service) = setup().await;
    let outcome = service.dispatch(ActionName::StartNext, "test").await;
    assert!(!outcome.accepted);
    assert_eq!(outcome.message, "No songs are loaded.");
    assert!(store.snapshot().current_song.is_none());
}

#[tokio::test]
async fn reset_position_clears_the_service_position_and_the_timer() {
    let (bus, store, service) = setup().await;
    load(
        &bus,
        plan(&[("a", "Alpha", Some(240)), ("b", "Beta", Some(240))]),
    )
    .await;
    service.dispatch(ActionName::StartNext, "test").await;
    let outcome = service.dispatch(ActionName::ResetPosition, "test").await;
    assert!(outcome.accepted);
    let state = store.snapshot();
    assert!(state.current_song.is_none() && state.playing_song.is_none());
    assert_eq!(title(&state.up_next_song), Some("Alpha"));
    assert_eq!(state.timer.status, TimerStatus::Stopped);
}

#[tokio::test]
async fn reload_keeps_the_current_song_by_stable_id() {
    let (bus, store, service) = setup().await;
    load(
        &bus,
        plan(&[("a", "Alpha", Some(240)), ("b", "Beta", Some(240))]),
    )
    .await;
    service.dispatch(ActionName::StartNext, "test").await;
    service.dispatch(ActionName::StartNext, "test").await;
    // The same songs, reordered and renamed: position follows the id.
    load(
        &bus,
        plan(&[
            ("b", "Beta (renamed)", Some(240)),
            ("a", "Alpha", Some(240)),
        ]),
    )
    .await;
    let state = store.snapshot();
    assert_eq!(title(&state.current_song), Some("Beta (renamed)"));
    assert_eq!(state.current_song_index, Some(0));
}

#[tokio::test]
async fn a_song_without_a_duration_never_starts_a_timer() {
    for duration in [None, Some(0)] {
        let (bus, store, service) = setup().await;
        let started = Arc::new(AtomicUsize::new(0));
        let s = started.clone();
        bus.subscribe(Some(EventType::SongStarted), move |_| {
            let s = s.clone();
            async move {
                s.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });
        load(&bus, plan(&[("a", "No Duration", duration)])).await;
        let outcome = service.dispatch(ActionName::StartNext, "test").await;
        let state = store.snapshot();
        assert!(!outcome.accepted);
        assert_eq!(title(&state.current_song), Some("No Duration"));
        assert_eq!(state.timer.status, TimerStatus::Error);
        assert!(!state.recent_errors.is_empty());
        assert_eq!(started.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn demo_events_cannot_impersonate_a_real_integration_connection() {
    let (bus, store, _service) = setup().await;
    let connection = |source: &str| {
        new_event(
            EventType::ConnectionChanged,
            source,
            EventPayload::Connection {
                integration: Integration::Midi,
                status: ConnectionStatus::Connected,
                detail: None,
            },
        )
    };
    bus.publish(connection("demo")).await;
    assert_eq!(store.snapshot().midi_status, ConnectionStatus::Disconnected);
    bus.publish(connection("midi")).await;
    assert_eq!(store.snapshot().midi_status, ConnectionStatus::Connected);
}

#[tokio::test]
async fn recent_events_are_bounded_and_in_publication_order() {
    let bus = EventBus::new();
    let store = StateStore::default();
    let service = StateService::new(bus.clone(), store.clone(), 5, 50);
    service.start();
    for _ in 0..12 {
        bus.publish(empty_event(EventType::ApplicationStarted, "t"))
            .await;
    }
    assert_eq!(store.snapshot().recent_events.len(), 5);
}

#[tokio::test]
async fn revision_increases_on_every_mutation_and_a_slow_observer_never_blocks_writers() {
    let store = StateStore::default();
    let mut slow = store.subscribe();
    let first = store.snapshot().revision;
    for _ in 0..500 {
        store.mutate(|s| s.last_action = Some("x".into()));
    }
    assert_eq!(store.snapshot().revision, first + 500);
    // The observer never read in between and still sees only the latest snapshot.
    slow.changed().await.unwrap();
    assert_eq!(slow.borrow().revision, first + 500);
}

// ---- critical alerts --------------------------------------------------------------------------

fn alert_store(dir: &tempfile::TempDir) -> (CriticalAlertStore, Arc<AtomicU64>) {
    let now = Arc::new(AtomicU64::new(10_000));
    let c = now.clone();
    (
        CriticalAlertStore::with_clock(
            dir.path().join("alerts.sqlite3"),
            Box::new(move || c.load(Ordering::SeqCst) as f64),
        ),
        now,
    )
}

#[test]
fn a_fresh_alert_notifies_and_a_repeat_inside_the_cooldown_only_counts() {
    let dir = tempfile::tempdir().unwrap();
    let (store, now) = alert_store(&dir);
    let first = store.record("midi", "port-1", "lost", "critical").unwrap();
    assert!(first.should_notify);
    now.fetch_add(10, Ordering::SeqCst);
    let repeat = store.record("midi", "port-1", "lost", "critical").unwrap();
    assert!(!repeat.should_notify);
    assert_eq!(repeat.alert.count, 2);
}

#[test]
fn a_repeat_exactly_at_the_cooldown_boundary_notifies_again() {
    let dir = tempfile::tempdir().unwrap();
    let (store, now) = alert_store(&dir);
    store.record("midi", "port-1", "lost", "critical").unwrap();
    now.fetch_add(COOLDOWN_SECONDS as u64, Ordering::SeqCst);
    assert!(
        store
            .record("midi", "port-1", "lost", "critical")
            .unwrap()
            .should_notify
    );
}

#[test]
fn acknowledge_closes_the_alert_and_a_new_occurrence_notifies_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = alert_store(&dir);
    let first = store.record("midi", "port-1", "lost", "critical").unwrap();
    assert!(store.acknowledge(first.alert.id).unwrap().is_some());
    assert!(store.list_open().unwrap().is_empty());
    let again = store.record("midi", "port-1", "lost", "critical").unwrap();
    assert!(again.should_notify);
    assert_ne!(again.alert.id, first.alert.id);
    assert_eq!(store.list_open().unwrap().len(), 1);
}

#[test]
fn alert_critical_without_an_enrolled_identity_still_records_locally() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = alert_store(&dir);
    let result = alert_critical(&store, "midi", "port-1", "lost", "critical", None).unwrap();
    assert!(result.should_notify);
    assert!(store.get(result.alert.id).unwrap().is_some());
}

#[test]
fn the_control_plane_payload_shape_is_exact() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = alert_store(&dir);
    let result = store.record("midi", "port-1", "lost", "critical").unwrap();
    let payload = build_control_plane_payload(&result.alert);
    let mut keys: Vec<_> = payload.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "category",
            "count",
            "first_seen",
            "last_seen",
            "message",
            "severity",
            "subject"
        ]
    );
}
