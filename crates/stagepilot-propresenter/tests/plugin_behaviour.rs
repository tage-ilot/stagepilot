//! Plugin behaviour with a fake client, ported from `test_propresenter_plugin.py`,
//! `test_propresenter_recovery.py` and the plugin parts of `test_propresenter_api.py`.

use async_trait::async_trait;
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::ProPresenterSettings;
use stagepilot_core::events::{new_event, EventPayload, EventType, StagePilotEvent, TimerPayload};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::{ConnectionStatus, PluginStatus, Song};
use stagepilot_propresenter::client::ProPresenterClient;
use stagepilot_propresenter::{
    ClientFactory, Countdown, Identifier, Look, PpError, ProPresenterPlugin, Timer,
};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn timer(name: &str, uuid: &str, index: u32, countdown: bool) -> Timer {
    Timer {
        id: Identifier {
            uuid: uuid.into(),
            name: name.into(),
            index,
        },
        allows_overrun: false,
        countdown: countdown.then_some(Countdown { duration: 60 }),
        count_down_to_time: None,
        elapsed: None,
        state: Some("stopped".into()),
        time: None,
    }
}

fn look(uuid: &str, name: &str, index: u32) -> Look {
    Look {
        id: Identifier {
            uuid: uuid.into(),
            name: name.into(),
            index,
        },
    }
}

#[derive(Default)]
struct Fake {
    timers: Mutex<Vec<Timer>>,
    looks: Mutex<Vec<Look>>,
    current: Mutex<Option<Look>>,
    triggered: Mutex<Vec<String>>,
    calls: Mutex<Vec<(String, String)>>,
    closed: Mutex<bool>,
    stale_current_reads: AtomicUsize,
    reads_after_trigger: AtomicUsize,
    /// Scripted list_timers results consumed first (recovery tests).
    list_results: Mutex<VecDeque<Result<Vec<Timer>, PpError>>>,
    list_calls: AtomicUsize,
    /// `stop_timer` fails with a 404 for this uuid (recreated-timer test).
    stop_fails_for: Mutex<Option<String>>,
    set_duration_error: Mutex<Option<PpError>>,
    /// Ids `/v1/look/current` reports after each trigger, in turn (it differs every time).
    live_ids: Mutex<VecDeque<String>>,
    /// What `/v1/look/current` reports after a trigger, whatever was triggered.
    after_trigger: Mutex<Option<Look>>,
    /// `/v1/timers/current` answer and whether it fails.
    current_timers: Mutex<Vec<Timer>>,
    current_timers_fail: AtomicBool,
}

impl Fake {
    fn new() -> Arc<Self> {
        let fake = Arc::new(Self::default());
        *lock(&fake.timers) = vec![timer("Song Countdown", "timer-uuid", 0, true)];
        *lock(&fake.looks) = vec![look("look-worship", "Worship", 0)];
        fake
    }
    fn calls(&self) -> Vec<(String, String)> {
        lock(&self.calls).clone()
    }
    fn rec(&self, kind: &str, arg: impl ToString) {
        lock(&self.calls).push((kind.to_string(), arg.to_string()));
    }
}

#[async_trait]
impl ProPresenterClient for Fake {
    async fn close(&self) {
        *lock(&self.closed) = true;
    }
    async fn list_timers(&self) -> Result<Vec<Timer>, PpError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        self.rec("list", "");
        if let Some(result) = lock(&self.list_results).pop_front() {
            return result;
        }
        Ok(lock(&self.timers).clone())
    }
    async fn find_timer(&self, name: &str) -> Result<Timer, PpError> {
        lock(&self.timers)
            .iter()
            .find(|t| t.id.name == name)
            .cloned()
            .ok_or_else(|| PpError::TimerNotFound(name.into()))
    }
    async fn get_timer(&self, _id: &str) -> Result<Timer, PpError> {
        Ok(lock(&self.timers)[0].clone())
    }
    async fn list_looks(&self) -> Result<Vec<Look>, PpError> {
        Ok(lock(&self.looks).clone())
    }
    async fn current_look(&self) -> Result<Look, PpError> {
        if !lock(&self.triggered).is_empty()
            && self.reads_after_trigger.load(Ordering::SeqCst)
                < self.stale_current_reads.load(Ordering::SeqCst)
        {
            self.reads_after_trigger.fetch_add(1, Ordering::SeqCst);
            return Ok(look("old-live-look", "Previous", 99));
        }
        Ok(lock(&self.current)
            .clone()
            .unwrap_or_else(|| lock(&self.looks)[0].clone()))
    }
    async fn trigger_look(&self, look_id: &str) -> Result<(), PpError> {
        lock(&self.triggered).push(look_id.to_string());
        let selected = lock(&self.looks)
            .iter()
            .find(|l| l.id.uuid == look_id)
            .cloned();
        if let Some(forced) = lock(&self.after_trigger).clone() {
            *lock(&self.current) = Some(forced);
            return Ok(());
        }
        if let Some(selected) = selected {
            let uuid = lock(&self.live_ids)
                .pop_front()
                .unwrap_or_else(|| "unique-live-look".to_string());
            *lock(&self.current) = Some(look(&uuid, &selected.id.name, selected.id.index));
        }
        Ok(())
    }
    async fn current_timers(&self) -> Result<Vec<Timer>, PpError> {
        if self.current_timers_fail.load(Ordering::SeqCst) {
            return Err(PpError::Connection("timers/current is down".into()));
        }
        Ok(lock(&self.current_timers).clone())
    }
    async fn stop_timer(&self, id: &str) -> Result<(), PpError> {
        self.rec("stop", id);
        if lock(&self.stop_fails_for).as_deref() == Some(id) {
            return Err(PpError::Response("ProPresenter returned HTTP 404.".into()));
        }
        Ok(())
    }
    async fn set_timer_duration(&self, t: &Timer, seconds: i64) -> Result<Timer, PpError> {
        if let Some(error) = lock(&self.set_duration_error).clone() {
            return Err(error);
        }
        self.rec("set", seconds);
        self.rec("reset", &t.id.uuid);
        let mut updated = t.clone();
        updated.countdown = Some(Countdown {
            duration: seconds as u32,
        });
        lock(&self.timers)[0] = updated.clone();
        Ok(updated)
    }
    async fn reset_timer(&self, id: &str) -> Result<(), PpError> {
        self.rec("reset", id);
        Ok(())
    }
    async fn start_timer(&self, id: &str) -> Result<(), PpError> {
        self.rec("start", id);
        Ok(())
    }
}

struct Rig {
    bus: EventBus,
    plugin: ProPresenterPlugin,
    events: Arc<Mutex<Vec<StagePilotEvent>>>,
}

fn rig_with(fakes: Vec<Arc<Fake>>, settings: ProPresenterSettings) -> Rig {
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
    let queue = Arc::new(Mutex::new(VecDeque::from(fakes)));
    let factory: ClientFactory = Arc::new(move |_settings| {
        let fake = lock(&queue)
            .pop_front()
            .expect("a fake client per factory call");
        fake as Arc<dyn ProPresenterClient>
    });
    let plugin = ProPresenterPlugin::new(bus.clone(), settings, factory);
    Rig {
        bus,
        plugin,
        events,
    }
}

fn enabled() -> ProPresenterSettings {
    ProPresenterSettings {
        enabled: true,
        health_check_interval_seconds: 300.0,
        ..ProPresenterSettings::default()
    }
}

fn song(title: &str, duration: Option<u32>) -> Song {
    Song::new("s", title, duration, 1)
}

async fn publish_song(bus: &EventBus, kind: EventType, song: Song) {
    bus.publish(new_event(
        kind,
        "test",
        EventPayload::Song { song, index: 0 },
    ))
    .await;
}

async fn publish_timer(bus: &EventBus, kind: EventType, remaining: u32, running: bool) {
    bus.publish(new_event(
        kind,
        "test",
        EventPayload::Timer(TimerPayload {
            duration_seconds: remaining,
            song: Some(song("T", Some(281))),
            position_seconds: Some(f64::from(281 - remaining)),
            remaining_seconds: Some(remaining),
            running,
            ..Default::default()
        }),
    ))
    .await;
}

fn call(kind: &str, arg: impl ToString) -> (String, String) {
    (kind.to_string(), arg.to_string())
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

fn events_of(rig: &Rig, kind: EventType) -> Vec<StagePilotEvent> {
    lock(&rig.events)
        .iter()
        .filter(|e| e.event_type == kind)
        .cloned()
        .collect()
}

// ---- test_propresenter_plugin.py ---------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn song_start_drives_stop_set_reset_start_then_publishes_timer_started() {
    let fake = Fake::new();
    let r = rig_with(vec![fake.clone()], enabled());
    r.plugin.start().await.unwrap();
    assert_eq!(
        r.plugin.snapshot(false).await.connection_status,
        ConnectionStatus::Connected
    );
    assert_eq!(fake.calls(), [call("list", "")]);

    publish_song(
        &r.bus,
        EventType::SongStarted,
        song("Battle Belongs", Some(281)),
    )
    .await;
    assert_eq!(
        fake.calls(),
        [
            call("list", ""),
            call("stop", "timer-uuid"),
            call("set", 281),
            call("reset", "timer-uuid"),
            call("start", "timer-uuid"),
        ]
    );
    let started = events_of(&r, EventType::TimerStarted);
    assert_eq!(started.len(), 1);
    let EventPayload::Timer(payload) = &started[0].payload else {
        panic!("timer payload")
    };
    assert_eq!(payload.duration_seconds, 281);
    assert!(payload.started_at.is_some());
    assert_eq!(started[0].source, "propresenter");

    publish_song(
        &r.bus,
        EventType::SongRestarted,
        song("Battle Belongs", Some(281)),
    )
    .await;
    let calls = fake.calls();
    assert_eq!(
        calls[calls.len() - 4..],
        [
            call("stop", "timer-uuid"),
            call("set", 281),
            call("reset", "timer-uuid"),
            call("start", "timer-uuid")
        ]
    );

    r.bus
        .publish(new_event(
            EventType::TimerStopRequested,
            "test",
            EventPayload::Empty,
        ))
        .await;
    assert_eq!(fake.calls().last().unwrap(), &call("stop", "timer-uuid"));
    assert_eq!(events_of(&r, EventType::TimerStopped).len(), 1);

    r.bus
        .publish(new_event(
            EventType::TimerResetRequested,
            "test",
            EventPayload::Empty,
        ))
        .await;
    let calls = fake.calls();
    assert_eq!(
        calls[calls.len() - 4..],
        [
            call("stop", "timer-uuid"),
            call("set", 0),
            call("reset", "timer-uuid"),
            call("reset", "timer-uuid")
        ]
    );
    r.plugin.stop().await.unwrap();
    assert!(*lock(&fake.closed));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_song_without_a_positive_duration_publishes_timer_failed() {
    let fake = Fake::new();
    let r = rig_with(vec![fake.clone()], enabled());
    r.plugin.start().await.unwrap();
    publish_song(&r.bus, EventType::SongStarted, song("Mystery", None)).await;
    let failed = events_of(&r, EventType::TimerFailed);
    assert_eq!(failed.len(), 1);
    let EventPayload::Timer(p) = &failed[0].payload else {
        panic!()
    };
    assert_eq!(
        p.message.as_deref(),
        Some("Cannot start countdown for \"Mystery\": the song has no positive duration.")
    );
    assert_eq!(fake.calls(), [call("list", "")]);
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_holds_then_resume_and_seek_apply_remaining() {
    let fake = Fake::new();
    let r = rig_with(vec![fake.clone()], enabled());
    r.plugin.start().await.unwrap();
    lock(&fake.calls).clear();
    publish_timer(&r.bus, EventType::TimerStopRequested, 100, false).await;
    assert_eq!(fake.calls(), [call("stop", "timer-uuid")]); // hold: no reset, no duration change
    lock(&fake.calls).clear();
    publish_timer(&r.bus, EventType::TimerResumed, 100, true).await;
    assert_eq!(
        fake.calls(),
        [
            call("stop", "timer-uuid"),
            call("set", 100),
            call("reset", "timer-uuid"),
            call("start", "timer-uuid")
        ]
    );
    lock(&fake.calls).clear();
    publish_timer(&r.bus, EventType::TimerSeeked, 40, true).await;
    assert!(fake.calls().contains(&call("set", 40)));
    assert_eq!(fake.calls().last().unwrap(), &call("start", "timer-uuid"));
    lock(&fake.calls).clear();
    publish_timer(&r.bus, EventType::TimerSeeked, 0, false).await; // seek while stopped / clamp 0
    assert!(fake.calls().contains(&call("set", 0)));
    assert!(!fake.calls().iter().any(|c| c.0 == "start"));
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn seek_while_stopped_updates_held_value_without_running() {
    let fake = Fake::new();
    let r = rig_with(vec![fake.clone()], enabled());
    r.plugin.start().await.unwrap();
    lock(&fake.calls).clear();
    publish_timer(&r.bus, EventType::TimerSeeked, 55, false).await;
    assert!(fake.calls().contains(&call("set", 55)));
    assert!(!fake.calls().iter().any(|c| c.0 == "start"));
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn reposition_failure_is_logged_not_raised() {
    let fake = Fake::new();
    let r = rig_with(vec![fake.clone()], enabled());
    r.plugin.start().await.unwrap();
    // Let the supervisor's first probe finish so it cannot record success over the error.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    *lock(&fake.set_duration_error) = Some(PpError::Value("down".into()));
    publish_timer(&r.bus, EventType::TimerResumed, 10, true).await;
    // The publish returned (no panic, no subscriber failure) and the plugin records the error.
    // The failure is recorded (status Error) and the supervisor is woken to recover; with a
    // healthy fake it may already have flipped back to Running, so only "no panic, still alive,
    // never Stopped" is stable to assert.
    let health = r.plugin.health().await.unwrap();
    assert!(matches!(
        health.status,
        PluginStatus::Error | PluginStatus::Running
    ));
    r.plugin.stop().await.unwrap();
}

// ---- test_propresenter_recovery.py -------------------------------------------------------------

fn recovering(results: Vec<Result<Vec<Timer>, PpError>>) -> Arc<Fake> {
    let fake = Fake::new();
    *lock(&fake.list_results) = results.into();
    fake
}

fn quick() -> ProPresenterSettings {
    ProPresenterSettings {
        enabled: true,
        reconnect_initial_seconds: 0.01,
        reconnect_max_seconds: 0.02,
        health_check_interval_seconds: 300.0,
        ..ProPresenterSettings::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn plugin_reconnects_when_propresenter_appears_after_startup() {
    let fake = recovering(vec![
        Err(PpError::Connection("offline".into())),
        Ok(vec![timer("Song Countdown", "timer-uuid", 0, true)]),
    ]);
    let r = rig_with(vec![fake.clone()], quick());
    r.plugin.start().await.unwrap();
    wait_for("reconnect", || {
        events_of(&r, EventType::ConnectionChanged).iter().any(|e| {
            matches!(
                &e.payload,
                EventPayload::Connection {
                    status: ConnectionStatus::Connected,
                    ..
                }
            )
        })
    })
    .await;
    let snapshot = r.plugin.snapshot(false).await;
    assert!(snapshot.timer_found);
    assert_eq!(snapshot.connection_status, ConnectionStatus::Connected);
    assert!(fake.list_calls.load(Ordering::SeqCst) >= 2);
    r.plugin.stop().await.unwrap();
    assert!(*lock(&fake.closed));
}

#[tokio::test(flavor = "multi_thread")]
async fn unreachable_propresenter_is_disconnected_until_poll_recovers() {
    let fake = recovering(vec![
        Err(PpError::Connection("ProPresenter is not open.".into())),
        Err(PpError::Connection("ProPresenter is not open.".into())),
        Ok(vec![timer("Song Countdown", "timer-uuid", 0, true)]),
    ]);
    let mut settings = quick();
    settings.reconnect_initial_seconds = 0.03;
    settings.reconnect_max_seconds = 0.03;
    let r = rig_with(vec![fake], settings);
    r.plugin.start().await.unwrap();
    assert_eq!(
        r.plugin.snapshot(false).await.connection_status,
        ConnectionStatus::Disconnected
    );
    wait_for("recovery", || {
        futures_block(r.plugin.snapshot(false)).connection_status == ConnectionStatus::Connected
    })
    .await;
    r.plugin.stop().await.unwrap();
}

fn futures_block<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
}

#[tokio::test(flavor = "multi_thread")]
async fn propresenter_api_response_failure_is_error() {
    let fake = recovering(vec![Err(PpError::Response(
        "ProPresenter returned invalid data.".into(),
    ))]);
    let mut settings = quick();
    settings.reconnect_initial_seconds = 1.0;
    settings.reconnect_max_seconds = 1.0;
    let r = rig_with(vec![fake], settings);
    r.plugin.start().await.unwrap();
    let snapshot = r.plugin.snapshot(false).await;
    assert_eq!(snapshot.connection_status, ConnectionStatus::Error);
    assert_eq!(
        snapshot.detail.as_deref(),
        Some("ProPresenter returned invalid data.")
    );
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_request_rediscovers_a_recreated_timer() {
    let fake = Fake::new();
    *lock(&fake.stop_fails_for) = Some("old-timer".into());
    *lock(&fake.list_results) = VecDeque::from(vec![
        Ok(vec![timer("Song Countdown", "old-timer", 0, true)]),
        Ok(vec![timer("Song Countdown", "new-timer", 0, true)]),
    ]);
    let r = rig_with(vec![fake.clone()], enabled());
    r.plugin.start().await.unwrap();
    r.bus
        .publish(new_event(
            EventType::TimerStopRequested,
            "test",
            EventPayload::Empty,
        ))
        .await;
    let stops: Vec<_> = fake
        .calls()
        .into_iter()
        .filter(|c| c.0 == "stop")
        .map(|c| c.1)
        .collect();
    assert_eq!(stops, ["old-timer", "new-timer"]);
    assert_eq!(fake.list_calls.load(Ordering::SeqCst), 2);
    r.plugin.stop().await.unwrap();
}

// ---- test_propresenter_api.py (plugin level) ---------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn status_and_timer_refresh_are_exposed() {
    let fake = Fake::new();
    *lock(&fake.timers) = vec![
        timer("Song Countdown", "timer-uuid", 0, true),
        timer("Message Timer", "other", 1, false),
    ];
    let r = rig_with(vec![fake], enabled());
    r.plugin.start().await.unwrap();
    let payload = r.plugin.snapshot(false).await;
    assert!(payload.enabled);
    assert_eq!(payload.connection_status, ConnectionStatus::Connected);
    assert!(payload.timer_found);
    assert_eq!(payload.selected_timer_id.as_deref(), Some("timer-uuid"));
    assert_eq!(
        payload
            .timers
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        ["Song Countdown", "Message Timer"]
    );
    assert!(r.plugin.refresh_timers().await.timer_found);
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn session_settings_recreate_client_and_report_missing_timer() {
    let first = Fake::new();
    let second = Fake::new();
    *lock(&second.timers) = vec![timer("Different Timer", "timer-uuid", 0, true)];
    let r = rig_with(vec![first.clone(), second.clone()], enabled());
    r.plugin.start().await.unwrap();
    let mut settings = enabled();
    settings.host = "192.168.4.40".into();
    settings.port = 1026;
    settings.timer_name = "Missing Timer".into();
    settings.look_id = Some("look-worship".into());
    settings.request_timeout_seconds = 4.0;
    let snapshot = r.plugin.reconfigure(settings).await;
    assert_eq!(snapshot.connection_status, ConnectionStatus::Connected);
    assert!(!snapshot.timer_found);
    assert_eq!(snapshot.host, "192.168.4.40");
    assert_eq!(snapshot.port, 1026);
    assert!(snapshot
        .detail
        .unwrap()
        .to_lowercase()
        .contains("not found"));
    assert!(*lock(&first.closed));
    let snapshot = r.plugin.apply_look("look-worship").await.unwrap();
    assert_eq!(*lock(&second.triggered), ["look-worship"]);
    assert_eq!(snapshot.current_look_id.as_deref(), Some("look-worship"));
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn saved_look_tolerates_delayed_current_look_confirmation() {
    let first = Fake::new();
    let second = Fake::new();
    second.stale_current_reads.store(2, Ordering::SeqCst);
    let r = rig_with(vec![first, second.clone()], enabled());
    r.plugin.start().await.unwrap();
    r.plugin.reconfigure(enabled()).await;
    r.plugin.apply_look("look-worship").await.unwrap();
    assert_eq!(*lock(&second.triggered), ["look-worship"]);
    assert_eq!(second.reads_after_trigger.load(Ordering::SeqCst), 2);
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_look_reports_an_unknown_look() {
    let r = rig_with(vec![Fake::new()], enabled());
    r.plugin.start().await.unwrap();
    let error = r.plugin.apply_look("nope").await.unwrap_err();
    assert_eq!(
        error,
        PpError::LookNotFound("ProPresenter Look \"nope\" was not found.".into())
    );
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_timer_reports_connected_with_the_api_connected_but_message() {
    let fake = Fake::new();
    *lock(&fake.timers) = vec![];
    let r = rig_with(vec![fake], enabled());
    r.plugin.start().await.unwrap();
    let snapshot = r.plugin.snapshot(false).await;
    assert_eq!(snapshot.connection_status, ConnectionStatus::Connected);
    assert_eq!(
        snapshot.detail.as_deref(),
        Some("API connected, but timer \"Song Countdown\" was not found.")
    );
    assert!(!snapshot.timer_found);
    assert_eq!(r.plugin.health().await.unwrap().status, PluginStatus::Error);
    r.plugin.stop().await.unwrap();
}

// ---- m3: look confirmation ------------------------------------------------------------------------

fn worship_and_sermon(fake: &Fake) {
    *lock(&fake.looks) = vec![
        look("E94606BB-0000-4000-8000-000000000001", "Worship", 0),
        look("AD203048-0000-4000-8000-000000000002", "Sermon", 1),
    ];
    *lock(&fake.current) = Some(look("ZZZZ", "Sermon", 1));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_look_is_confirmed_although_current_look_reports_a_new_uuid_every_time() {
    // Captured on the rig: /v1/looks keeps E94606BB..., /v1/look/current answers 36EDD0F0, CB8B7044,
    // 15616A88 for the same Look "Worship" at index 0.
    let fake = Fake::new();
    worship_and_sermon(&fake);
    *lock(&fake.live_ids) = VecDeque::from([
        "36EDD0F0-0000-4000-8000-0000000000AA".to_string(),
        "CB8B7044-0000-4000-8000-0000000000BB".to_string(),
        "15616A88-0000-4000-8000-0000000000CC".to_string(),
    ]);
    let r = rig_with(vec![fake.clone()], enabled());
    r.plugin.start().await.unwrap();
    for _ in 0..3 {
        r.plugin
            .apply_look("E94606BB-0000-4000-8000-000000000001")
            .await
            .unwrap();
    }
    assert_eq!(lock(&fake.triggered).len(), 3);
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_look_with_the_same_index_but_another_name_and_uuid_is_not_a_confirmation() {
    let fake = Fake::new();
    worship_and_sermon(&fake);
    *lock(&fake.after_trigger) = Some(look("OTHER-UUID", "Prayer", 0));
    let r = rig_with(vec![fake], enabled());
    r.plugin.start().await.unwrap();
    let error = r
        .plugin
        .apply_look("E94606BB-0000-4000-8000-000000000001")
        .await
        .unwrap_err();
    assert!(matches!(error, PpError::LookNotFound(_)), "{error:?}");
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_look_with_the_same_name_at_another_index_is_not_a_confirmation() {
    let fake = Fake::new();
    worship_and_sermon(&fake);
    *lock(&fake.after_trigger) = Some(look("OTHER-UUID", "Worship", 5));
    let r = rig_with(vec![fake], enabled());
    r.plugin.start().await.unwrap();
    let error = r
        .plugin
        .apply_look("E94606BB-0000-4000-8000-000000000001")
        .await
        .unwrap_err();
    assert!(matches!(error, PpError::LookNotFound(_)), "{error:?}");
    r.plugin.stop().await.unwrap();
}

// ---- m2: timer state --------------------------------------------------------------------------------

fn with_state(mut t: Timer, state: Option<&str>) -> Timer {
    t.state = state.map(str::to_string);
    t
}

#[tokio::test(flavor = "multi_thread")]
async fn timer_state_comes_from_timers_current() {
    let fake = Fake::new();
    *lock(&fake.timers) = vec![
        with_state(timer("Song Countdown", "timer-uuid", 0, true), None),
        with_state(timer("Message Timer", "other", 1, false), None),
    ];
    *lock(&fake.current_timers) = vec![with_state(
        timer("Song Countdown", "timer-uuid", 0, true),
        Some("overrunning"),
    )];
    let r = rig_with(vec![fake], enabled());
    r.plugin.start().await.unwrap();
    let snapshot = r.plugin.snapshot(true).await;
    let states: Vec<Option<&str>> = snapshot.timers.iter().map(|t| t.state.as_deref()).collect();
    assert_eq!(states, [Some("overrunning"), None]);
    r.plugin.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_timers_current_call_does_not_break_the_probe() {
    let fake = Fake::new();
    *lock(&fake.timers) = vec![with_state(timer("Song Countdown", "timer-uuid", 0, true), None)];
    fake.current_timers_fail.store(true, Ordering::SeqCst);
    let r = rig_with(vec![fake], enabled());
    r.plugin.start().await.unwrap();
    let snapshot = r.plugin.snapshot(true).await;
    assert_eq!(snapshot.connection_status, ConnectionStatus::Connected);
    assert!(snapshot.timer_found);
    assert_eq!(snapshot.timers[0].state, None);
    r.plugin.stop().await.unwrap();
}
