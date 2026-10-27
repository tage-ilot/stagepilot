//! The demo Services plugin and the placeholder Playback input.
//!
//! `DemoPlugin` is a port of `plugins/demo/plugin.py`. `PlaybackStub` only reproduces what the
//! Python Playback input publishes at start-up while nothing is selected (a disconnected
//! `connection.changed`), so the state snapshot and `/health` match. Lights, MIDI and
//! ProPresenter are real plugins (`outputs.rs`).

use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use stagepilot_core::bus::{EventBus, Subscription};
use stagepilot_core::events::{new_event, EventPayload, EventType, Integration, TimerPayload};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::{
    ConnectionStatus, PluginHealth, PluginStatus, ServiceLoadStatus, ServicePlan, Song,
};
use std::sync::Mutex;

pub const PLUGIN_VERSION: &str = "0.1.0";
pub const PLAYBACK_DISCONNECTED_REASON: &str =
    "Disconnected. Choose Scan Network to find Playback.";

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Today's date in UTC-agnostic local terms: Python uses `date.today()` (process local date).
pub fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

pub fn demo_service_plan(date: NaiveDate) -> ServicePlan {
    let song = |id: &str, title: &str, seconds: u32, order: u32| {
        Song::new(id, title, Some(seconds), order)
    };
    ServicePlan {
        id: "demo-service-plan".into(),
        title: "Sunday Worship \u{2014} Demo".into(),
        date,
        service_type: "Weekend Services".into(),
        service_type_id: None,
        service_times: vec!["09:00".into(), "11:00".into()],
        duration_source: "Demo scheduled item length".into(),
        songs: vec![
            song("demo-battle-belongs", "Battle Belongs", 281, 1),
            song("demo-holy-forever", "Holy Forever", 336, 2),
            song("demo-gratitude", "Gratitude", 352, 3),
            song("demo-firm-foundation", "Firm Foundation", 377, 4),
        ],
    }
}

struct Status {
    status: PluginStatus,
    last_activity_at: Option<chrono::DateTime<Utc>>,
}

pub struct DemoPlugin {
    bus: EventBus,
    state: Mutex<Status>,
    subscriptions: Mutex<Vec<Subscription>>,
    timer_subscriptions: Mutex<Vec<Subscription>>,
    simulate_propresenter: Mutex<bool>,
}

impl DemoPlugin {
    pub fn new(bus: EventBus) -> Self {
        Self::with_simulation(bus, true)
    }

    /// `simulate_propresenter = false` leaves the countdown to a real ProPresenter plugin.
    pub fn with_simulation(bus: EventBus, simulate_propresenter: bool) -> Self {
        Self {
            bus,
            state: Mutex::new(Status {
                status: PluginStatus::Stopped,
                last_activity_at: None,
            }),
            subscriptions: Mutex::new(Vec::new()),
            timer_subscriptions: Mutex::new(Vec::new()),
            simulate_propresenter: Mutex::new(simulate_propresenter),
        }
    }

    fn touch(&self, status: Option<PluginStatus>) {
        let mut s = lock(&self.state);
        if let Some(status) = status {
            s.status = status;
        }
        s.last_activity_at = Some(Utc::now());
    }

    fn subscribe_timer(&self) {
        let mut subs = Vec::new();
        for kind in [EventType::SongStarted, EventType::SongRestarted] {
            let bus = self.bus.clone();
            subs.push(self.bus.subscribe(Some(kind), move |event| {
                let bus = bus.clone();
                async move {
                    if let EventPayload::Song { song, .. } = &event.payload {
                        if let Some(duration) = song.duration_seconds.filter(|d| *d > 0) {
                            bus.publish(new_event(
                                EventType::TimerStarted,
                                "demo",
                                EventPayload::Timer(TimerPayload {
                                    duration_seconds: duration,
                                    song: Some(song.clone()),
                                    ..Default::default()
                                }),
                            ))
                            .await;
                        }
                    }
                    Ok(())
                }
            }));
        }
        let bus = self.bus.clone();
        subs.push(
            self.bus
                .subscribe(Some(EventType::TimerStopRequested), move |_event| {
                    let bus = bus.clone();
                    async move {
                        bus.publish(timer_stopped()).await;
                        Ok(())
                    }
                }),
        );
        lock(&self.timer_subscriptions).extend(subs);
    }

    fn unsubscribe_timer(&self) {
        for sub in lock(&self.timer_subscriptions).drain(..) {
            self.bus.unsubscribe(&sub);
        }
    }

    /// Start or stop simulating the timer output while running, as a fresh start would.
    pub async fn set_simulate_propresenter(&self, simulate: bool) {
        {
            let mut current = lock(&self.simulate_propresenter);
            if *current == simulate {
                return;
            }
            *current = simulate;
        }
        if lock(&self.state).status != PluginStatus::Running {
            return;
        }
        if simulate {
            self.subscribe_timer();
            self.bus.publish(timer_stopped()).await;
        } else {
            self.unsubscribe_timer();
        }
    }
}

async fn publish_plan(bus: &EventBus, plan: ServicePlan) {
    let date = plan.date;
    bus.publish(new_event(
        EventType::ServiceLoaded,
        "demo",
        EventPayload::Service { plan },
    ))
    .await;
    bus.publish(new_event(
        EventType::ServiceLoadChanged,
        "demo",
        EventPayload::ServiceLoad {
            status: ServiceLoadStatus::Loaded,
            target_date: Some(date),
            candidates: vec![],
            skipped_items: vec![],
            message: Some("Demo service loaded.".into()),
            is_stale: false,
        },
    ))
    .await;
}

fn timer_stopped() -> stagepilot_core::events::StagePilotEvent {
    new_event(
        EventType::TimerStopped,
        "demo",
        EventPayload::Timer(TimerPayload {
            duration_seconds: 0,
            ..Default::default()
        }),
    )
}

#[async_trait]
impl Plugin for DemoPlugin {
    fn name(&self) -> &str {
        "demo"
    }
    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    async fn start(&self) -> Result<(), String> {
        self.touch(Some(PluginStatus::Starting));
        let bus = self.bus.clone();
        let reload = self
            .bus
            .subscribe(Some(EventType::ServiceReloadRequested), {
                let bus = bus.clone();
                move |_event| {
                    let bus = bus.clone();
                    async move {
                        publish_plan(&bus, demo_service_plan(today())).await;
                        Ok(())
                    }
                }
            });
        lock(&self.subscriptions).push(reload);
        let simulate = *lock(&self.simulate_propresenter);
        if simulate {
            self.subscribe_timer();
        }
        publish_plan(&self.bus, demo_service_plan(today())).await;
        if simulate {
            self.bus.publish(timer_stopped()).await;
        }
        self.touch(Some(PluginStatus::Running));
        Ok(())
    }

    async fn stop(&self) -> Result<(), String> {
        self.touch(Some(PluginStatus::Stopping));
        self.unsubscribe_timer();
        for sub in lock(&self.subscriptions).drain(..) {
            self.bus.unsubscribe(&sub);
        }
        self.touch(Some(PluginStatus::Stopped));
        Ok(())
    }

    async fn health(&self) -> Result<PluginHealth, String> {
        let s = lock(&self.state);
        Ok(PluginHealth {
            name: "demo".into(),
            version: PLUGIN_VERSION.into(),
            status: s.status,
            last_error: None,
            last_activity_at: s.last_activity_at,
        })
    }
}

/// Playback input while nothing is selected: publishes the disconnected MIDI connection once.
pub struct PlaybackStub {
    bus: EventBus,
    state: Mutex<Status>,
}

impl PlaybackStub {
    pub fn new(bus: EventBus) -> Self {
        Self::new_for(bus)
    }

    fn new_for(bus: EventBus) -> Self {
        Self {
            bus,
            state: Mutex::new(Status {
                status: PluginStatus::Stopped,
                last_activity_at: None,
            }),
        }
    }
}

#[async_trait]
impl Plugin for PlaybackStub {
    fn name(&self) -> &str {
        "playback_api"
    }
    fn version(&self) -> &str {
        PLUGIN_VERSION
    }
    async fn start(&self) -> Result<(), String> {
        self.bus
            .publish(new_event(
                EventType::ConnectionChanged,
                "playback_connection",
                EventPayload::Connection {
                    integration: Integration::Midi,
                    status: ConnectionStatus::Disconnected,
                    detail: Some(PLAYBACK_DISCONNECTED_REASON.into()),
                },
            ))
            .await;
        let mut s = lock(&self.state);
        s.status = PluginStatus::Running;
        s.last_activity_at = Some(Utc::now());
        Ok(())
    }
    async fn stop(&self) -> Result<(), String> {
        lock(&self.state).status = PluginStatus::Stopped;
        Ok(())
    }
    async fn health(&self) -> Result<PluginHealth, String> {
        let s = lock(&self.state);
        Ok(PluginHealth {
            name: "playback_api".into(),
            version: PLUGIN_VERSION.into(),
            status: s.status,
            last_error: None,
            last_activity_at: s.last_activity_at,
        })
    }
}
