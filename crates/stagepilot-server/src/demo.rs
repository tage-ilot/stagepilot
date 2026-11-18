//! The demo Services plugin and the placeholder Playback input.
//!
//! `DemoPlugin` is a port of `plugins/demo/plugin.py`. `PlaybackStub` only reproduces what the
//! Python Playback input publishes at start-up while nothing is selected (a disconnected
//! `connection.changed`), so the state snapshot and `/health` match. Lights, MIDI and
//! ProPresenter are real plugins (`outputs.rs`).

use async_trait::async_trait;
use chrono::{Datelike, Duration, NaiveDate, Utc, Weekday};
use stagepilot_core::bus::{EventBus, Subscription};
use stagepilot_core::events::{new_event, EventPayload, EventType, Integration, TimerPayload};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::service_pin::{
    PinKeeper, PlanPicker, ServicePin, UpcomingPlan, UpcomingPlans, NOTICE_PIN_PASSED,
    NOTICE_PIN_VANISHED, SOURCE_DEMO,
};
use stagepilot_core::state::{
    ConnectionStatus, PluginHealth, PluginStatus, ServiceLoadStatus, ServicePlan, ServiceSelection,
    Song, UpcomingService,
};
use stagepilot_core::store::StateStore;
use std::sync::{Arc, Mutex};

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

/// How many weekly Sunday plans follow the demo's plan for today.
pub const DEMO_SUNDAYS: usize = 6;
const DEMO_SERVICE_TYPE_ID: &str = "demo-weekend";
const DEMO_SERVICE_TYPE: &str = "Weekend Services";

/// (title, songs as (id, title, seconds)) of the weekly demo plans, in order.
#[allow(clippy::type_complexity)]
const WEEKLY: [(&str, [(&str, &str, u32); 4]); DEMO_SUNDAYS] = [
    (
        "Communion Sunday",
        [
            ("demo-way-maker", "Way Maker", 392),
            ("demo-goodness-of-god", "Goodness of God", 301),
            ("demo-lamb-of-god", "Lamb of God", 268),
            ("demo-good-good-father", "Good Good Father", 318),
        ],
    ),
    (
        "Family Sunday",
        [
            ("demo-this-is-amazing-grace", "This Is Amazing Grace", 276),
            ("demo-reckless-love", "Reckless Love", 352),
            ("demo-living-hope", "Living Hope", 331),
            ("demo-build-my-life", "Build My Life", 345),
        ],
    ),
    (
        "Worship Night",
        [
            ("demo-what-a-beautiful-name", "What a Beautiful Name", 322),
            ("demo-raise-a-hallelujah", "Raise a Hallelujah", 305),
            ("demo-graves-into-gardens", "Graves Into Gardens", 371),
            ("demo-jireh", "Jireh", 395),
        ],
    ),
    (
        "Baptism Sunday",
        [
            ("demo-oceans", "Oceans", 481),
            ("demo-no-longer-slaves", "No Longer Slaves", 407),
            ("demo-glorious-day", "Glorious Day", 238),
            ("demo-cornerstone", "Cornerstone", 292),
        ],
    ),
    (
        "Missions Sunday",
        [
            ("demo-great-are-you-lord", "Great Are You Lord", 355),
            ("demo-here-i-am-to-worship", "Here I Am to Worship", 266),
            ("demo-let-it-rain", "Let It Rain", 312),
            ("demo-open-the-eyes", "Open the Eyes of My Heart", 284),
        ],
    ),
    (
        "Celebration Sunday",
        [
            ("demo-this-is-our-god", "This Is Our God", 289),
            ("demo-who-you-say-i-am", "Who You Say I Am", 296),
            ("demo-king-of-kings", "King of Kings", 343),
            ("demo-hills-and-valleys", "Hills and Valleys", 361),
        ],
    ),
];

/// The demo's plans from `today`: the plan for today (what the demo always loaded) and then
/// the next `DEMO_SUNDAYS` Sundays, each with its own title and songs.
pub fn demo_plans(today: NaiveDate) -> Vec<ServicePlan> {
    let mut plans = vec![demo_service_plan(today)];
    let mut date = today;
    for (title, songs) in WEEKLY {
        date += Duration::days(1);
        while date.weekday() != Weekday::Sun {
            date += Duration::days(1);
        }
        plans.push(ServicePlan {
            id: format!("demo-plan-{date}"),
            title: format!("{title} \u{2014} Demo"),
            date,
            service_type: DEMO_SERVICE_TYPE.into(),
            service_type_id: None,
            service_times: vec!["09:00".into(), "11:00".into()],
            duration_source: "Demo scheduled item length".into(),
            songs: songs
                .iter()
                .enumerate()
                .map(|(i, (id, name, seconds))| Song::new(id, name, Some(*seconds), i as u32 + 1))
                .collect(),
        });
    }
    plans
}

struct Status {
    status: PluginStatus,
    last_activity_at: Option<chrono::DateTime<Utc>>,
}

pub struct DemoPlugin {
    bus: EventBus,
    /// The picked service (shared with the Planning Center source, which ignores a demo pin).
    pins: Arc<PinKeeper>,
    /// A one-time note about a pin that ended by itself.
    notice: Arc<Mutex<Option<String>>>,
    /// Where the "which service is this" answer goes (none in plain unit tests).
    store: Option<StateStore>,
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
            pins: PinKeeper::in_memory(),
            notice: Arc::new(Mutex::new(None)),
            store: None,
            state: Mutex::new(Status {
                status: PluginStatus::Stopped,
                last_activity_at: None,
            }),
            subscriptions: Mutex::new(Vec::new()),
            timer_subscriptions: Mutex::new(Vec::new()),
            simulate_propresenter: Mutex::new(simulate_propresenter),
        }
    }

    /// Keep the picked service in `pins` and report the selection into `store`.
    pub fn with_picker(mut self, pins: Arc<PinKeeper>, store: StateStore) -> Self {
        self.pins = pins;
        self.store = Some(store);
        self
    }

    fn loader(&self) -> Loader {
        Loader {
            bus: self.bus.clone(),
            pins: self.pins.clone(),
            notice: self.notice.clone(),
            store: self.store.clone(),
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

/// Loads the demo plan the picker chose (or today's plan) the way every other source does:
/// the selection first, then `service.loaded` and the load state.
#[derive(Clone)]
struct Loader {
    bus: EventBus,
    pins: Arc<PinKeeper>,
    notice: Arc<Mutex<Option<String>>>,
    store: Option<StateStore>,
}

impl Loader {
    /// The pin that still counts today; one whose date passed, or whose plan is not offered,
    /// is dropped with a one-time notice.
    fn plan_to_load(&self, today: NaiveDate) -> (ServicePlan, bool, ServicePlan) {
        let plans = demo_plans(today);
        let upcoming = plans[0].clone();
        if let Some(pin) = self.pins.get(SOURCE_DEMO) {
            let message = if pin.is_expired(today) {
                Some(NOTICE_PIN_PASSED)
            } else if plans.iter().any(|p| p.id == pin.plan_id) {
                None
            } else {
                Some(NOTICE_PIN_VANISHED)
            };
            match message {
                None => {
                    if let Some(plan) = plans.iter().find(|p| p.id == pin.plan_id) {
                        return (plan.clone(), true, upcoming);
                    }
                }
                Some(text) => {
                    let _ = self.pins.clear();
                    *lock(&self.notice) = Some(text.to_string());
                }
            }
        }
        (upcoming.clone(), false, upcoming)
    }

    async fn load(&self) {
        let (plan, pinned, upcoming) = self.plan_to_load(today());
        let notice = lock(&self.notice).clone();
        if let Some(store) = &self.store {
            // Said only when something is to be said (as the Planning Center source does): a
            // state without `selection` is the upcoming service, unpicked.
            let selection = (pinned || notice.is_some()).then(|| ServiceSelection {
                pinned,
                plan_id: Some(plan.id.clone()),
                date: Some(plan.date),
                notice,
            });
            let upcoming = pinned.then(|| UpcomingService {
                plan_id: upcoming.id.clone(),
                date: upcoming.date,
                title: upcoming.title.clone(),
            });
            store.mutate(move |s| {
                s.selection = selection;
                s.upcoming = upcoming;
            });
        }
        publish_plan(&self.bus, plan).await;
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
        let loader = self.loader();
        // A reload, and the picker's choice (or "back to the upcoming service"), load the plan
        // the pin says; the route has already saved the pin.
        for kind in [
            EventType::ServiceReloadRequested,
            EventType::ServicePlanSelectionRequested,
        ] {
            let loader = loader.clone();
            let subscription = self.bus.subscribe(Some(kind), move |_event| {
                let loader = loader.clone();
                async move {
                    loader.load().await;
                    Ok(())
                }
            });
            lock(&self.subscriptions).push(subscription);
        }
        let simulate = *lock(&self.simulate_propresenter);
        if simulate {
            self.subscribe_timer();
        }
        loader.load().await;
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

    fn plan_picker(&self) -> Option<&dyn PlanPicker> {
        Some(self)
    }
}

fn demo_listing(plan: &ServicePlan) -> UpcomingPlan {
    UpcomingPlan {
        id: plan.id.clone(),
        date: plan.date,
        title: plan.title.clone(),
        service_type_id: DEMO_SERVICE_TYPE_ID.into(),
        service_type_name: DEMO_SERVICE_TYPE.into(),
    }
}

#[async_trait]
impl PlanPicker for DemoPlugin {
    async fn upcoming_plans(&self) -> Result<UpcomingPlans, String> {
        let today = today();
        let pin = self.pins.get(SOURCE_DEMO).filter(|p| !p.is_expired(today));
        let plans = demo_plans(today);
        Ok(UpcomingPlans {
            upcoming_id: plans.first().map(|p| p.id.clone()),
            plans: plans.iter().map(demo_listing).collect(),
            pinned_id: pin.as_ref().map(|p| p.plan_id.clone()),
            pinned_date: pin.map(|p| p.date),
        })
    }

    fn today(&self) -> NaiveDate {
        today()
    }

    fn pin(&self) -> Option<ServicePin> {
        self.pins.get(SOURCE_DEMO)
    }

    fn set_pin(&self, plan: &UpcomingPlan) -> Result<(), String> {
        *lock(&self.notice) = None;
        self.pins.set(ServicePin::new(
            SOURCE_DEMO,
            &plan.id,
            plan.date,
            Some(plan.service_type_id.clone()),
            Some(plan.title.clone()),
        ))
    }

    fn clear_pin(&self) -> Result<(), String> {
        *lock(&self.notice) = None;
        self.pins.clear()
    }

    fn restore_pin(&self, previous: Option<ServicePin>) -> Result<(), String> {
        self.pins.restore(previous)
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
