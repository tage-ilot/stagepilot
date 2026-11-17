//! Production Planning Center plugin lifecycle and plan refresh orchestration: port of
//! `plugins/planning_center/plugin.py` and `plugins/planning_center/preferences.py`.
//!
//! Python runs refreshes as one asyncio task with `pending_reload` / `pending_selection` flags.
//! Here the same state machine lives in one mutex-guarded [`Sched`] record: at most one refresh
//! runs, a request that arrives meanwhile sets a flag, and the running task loops once more.

use crate::client::{PcoApi, PcoClientFactory};
use crate::errors::PcoError;
use crate::models::*;
use crate::plan_cache::{CachedServicePlan, MemoryPlanCacheStore, PlanCacheStore};
use async_trait::async_trait;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use stagepilot_core::bus::{EventBus, Subscription};
use stagepilot_core::events::{new_event, EventPayload, EventType, Integration};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::service_pin::{
    PinKeeper, PlanPicker, ServicePin, UpcomingPlan, UpcomingPlans, NOTICE_PIN_PASSED,
    NOTICE_PIN_VANISHED, PICKER_CACHE_SECONDS, PICKER_DAYS, PICKER_MAX_PLANS,
    SOURCE_PLANNING_CENTER,
};
use stagepilot_core::state::*;
use stagepilot_core::state_service::ActionOutcome;
use stagepilot_core::store::StateStore;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration as StdDuration, Instant};

pub const PLUGIN_NAME: &str = "planning_center";
pub const PLUGIN_VERSION: &str = "0.1.0";
pub const ALL_SERVICE_TYPES_ID: &str = "stagepilot:all-service-types";

pub type TodayProvider = Arc<dyn Fn(Tz) -> NaiveDate + Send + Sync>;

pub fn local_today(timezone: Tz) -> NaiveDate {
    Utc::now().with_timezone(&timezone).date_naive()
}

/// Shared preferred-plan scoring for discovery and explicit duration previews.
pub fn preferred_candidate<'a>(
    settings: &PcoSettings,
    candidates: &'a [PlanCandidate],
) -> Option<&'a PlanCandidate> {
    let title = settings.plan_title_preference.as_deref();
    let time = settings.preferred_service_time.as_deref();
    let scores: Vec<(u32, &PlanCandidate)> = candidates
        .iter()
        .map(|c| {
            let title_hit = title.is_some_and(|t| {
                !t.is_empty() && c.title.to_lowercase().trim() == t.to_lowercase().trim()
            });
            let time_hit = time.is_some_and(|t| {
                !t.is_empty()
                    && c.service_times
                        .iter()
                        .any(|s| s.format("%H:%M").to_string() == t)
            });
            (u32::from(title_hit) + u32::from(time_hit), c)
        })
        .collect();
    let highest = scores.iter().map(|(s, _)| *s).max().unwrap_or(0);
    let matches: Vec<&PlanCandidate> = scores
        .iter()
        .filter(|(s, _)| *s == highest)
        .map(|(_, c)| *c)
        .collect();
    if highest > 0 && matches.len() == 1 {
        Some(matches[0])
    } else {
        None
    }
}

struct Health {
    status: PluginStatus,
    last_error: Option<String>,
    /// Whether the MOST RECENT error means the configured settings are genuinely invalid (an
    /// archived/unavailable service type) versus a transient failure the scheduled refresh retries.
    last_error_is_configuration: bool,
    last_activity_at: Option<DateTime<Utc>>,
}

#[derive(Default)]
struct Sched {
    running: bool,
    pending_reload: bool,
    pending_selection: Option<String>,
}

struct Inner {
    bus: EventBus,
    store: StateStore,
    timezone_name: String,
    timezone: Tz,
    factory: PcoClientFactory,
    today: TodayProvider,
    cache: Arc<dyn PlanCacheStore>,
    cache_warning: Mutex<Option<String>>,
    settings: Mutex<PcoSettings>,
    client: Mutex<Option<Arc<dyn PcoApi>>>,
    subscriptions: Mutex<Vec<Subscription>>,
    sched: Mutex<Sched>,
    idle: tokio::sync::watch::Sender<bool>,
    health: Mutex<Health>,
    stopping: AtomicBool,
    /// The service the operator picked instead of the upcoming one (`service-pin.json`).
    pins: Mutex<Arc<PinKeeper>>,
    /// A one-time, neutral note about a pin that ended by itself; shown with the selection.
    notice: Mutex<Option<String>>,
    /// The picker's list of plans, kept for a minute.
    plan_list: Mutex<Option<CachedPlanList>>,
    plan_list_ttl_ms: AtomicU64,
    /// How often a held pin is checked against the date (the midnight rollover).
    expiry_interval_ms: AtomicU64,
    expiry_watch: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

struct CachedPlanList {
    at: Instant,
    key: String,
    plans: Vec<UpcomingPlan>,
}

#[derive(Clone)]
pub struct PcoPlugin {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

struct LoadContext {
    api_connected: bool,
    configured: Option<Vec<ServiceType>>,
    previous_type_active: bool,
    /// The pinned plan was loaded (the result is that plan, not the upcoming one).
    pinned_loaded: bool,
    /// The pinned plan is not in Planning Center any more (or not in the configured service
    /// types): the pin is dropped and the upcoming plan is loaded instead.
    pin_vanished: bool,
}

/// What `apply_result` tells the state about the service it loads.
struct SelectionHint {
    pinned: bool,
    /// The upcoming service while another one is pinned (when it could be found).
    upcoming: Option<UpcomingService>,
    /// The date of the picked plan: it may lie beyond the upcoming search window.
    pinned_date: Option<NaiveDate>,
}

impl PcoPlugin {
    pub fn new(
        bus: EventBus,
        store: StateStore,
        settings: PcoSettings,
        timezone_name: &str,
        factory: PcoClientFactory,
        today: Option<TodayProvider>,
        cache: Option<Arc<dyn PlanCacheStore>>,
    ) -> Result<Self, PcoError> {
        let timezone = crate::client::parse_timezone(timezone_name)?;
        let (idle, _) = tokio::sync::watch::channel(true);
        Ok(Self {
            inner: Arc::new(Inner {
                bus,
                store,
                timezone_name: timezone_name.to_string(),
                timezone,
                factory,
                today: today.unwrap_or_else(|| Arc::new(local_today)),
                cache: cache.unwrap_or_else(|| Arc::new(MemoryPlanCacheStore::default())),
                cache_warning: Mutex::new(None),
                settings: Mutex::new(settings),
                client: Mutex::new(None),
                subscriptions: Mutex::new(Vec::new()),
                sched: Mutex::new(Sched::default()),
                idle,
                health: Mutex::new(Health {
                    status: PluginStatus::Stopped,
                    last_error: None,
                    last_error_is_configuration: false,
                    last_activity_at: None,
                }),
                stopping: AtomicBool::new(false),
                pins: Mutex::new(PinKeeper::in_memory()),
                notice: Mutex::new(None),
                plan_list: Mutex::new(None),
                plan_list_ttl_ms: AtomicU64::new(PICKER_CACHE_SECONDS * 1000),
                expiry_interval_ms: AtomicU64::new(30_000),
                expiry_watch: Mutex::new(None),
            }),
        })
    }

    /// Where the pin lives (the server gives the plugin its `service-pin.json`).
    pub fn set_pin_keeper(&self, keeper: Arc<PinKeeper>) {
        *lock(&self.inner.pins) = keeper;
    }

    /// Tests: how long a plan list is kept (zero keeps nothing).
    pub fn set_plan_list_ttl(&self, ttl: StdDuration) {
        self.inner
            .plan_list_ttl_ms
            .store(ttl.as_millis() as u64, Ordering::SeqCst);
    }

    /// Tests: how often a held pin is checked against the date.
    pub fn set_expiry_check_interval(&self, interval: StdDuration) {
        self.inner
            .expiry_interval_ms
            .store(interval.as_millis().max(1) as u64, Ordering::SeqCst);
    }

    /// Replace settings before the plugin manager starts this plugin.
    pub fn prepare_start(&self, settings: PcoSettings) -> Result<(), PcoError> {
        let health = lock(&self.inner.health);
        if health.status != PluginStatus::Stopped || lock(&self.inner.client).is_some() {
            return Err(PcoError::Generic(
                "Planning Center startup settings can only be prepared while stopped.".into(),
            ));
        }
        drop(health);
        *lock(&self.inner.settings) = settings;
        Ok(())
    }

    /// Refresh the saved service after the desktop startup reconciliation pass.
    pub async fn refresh_now(&self) {
        let running = lock(&self.inner.health).status == PluginStatus::Running;
        if !running || lock(&self.inner.client).is_none() {
            return;
        }
        self.inner.request_regular_refresh(true).await;
    }

    /// Is the plugin already running exactly these settings (credentials included) and
    /// connected to Planning Center? Then applying them again would only flap the connection and
    /// reload the plan.
    pub fn is_running_with(&self, settings: &PcoSettings) -> bool {
        let inner = &self.inner;
        !inner.stopping.load(Ordering::SeqCst)
            && lock(&inner.health).status == PluginStatus::Running
            && lock(&inner.client).is_some()
            && *lock(&inner.settings) == *settings
            && inner.store.snapshot().planning_center_status == ConnectionStatus::Connected
    }

    /// Apply restart-exempt settings changes to the already-running plugin: replace the settings
    /// and client in place, then re-run validation and a full plan refresh (what `start` does), so
    /// the connection status follows the new settings without a restart.
    pub async fn reconfigure(&self, settings: PcoSettings) -> ActionOutcome {
        let inner = &self.inner;
        if inner.stopping.load(Ordering::SeqCst)
            || lock(&inner.health).status == PluginStatus::Stopped
        {
            return ActionOutcome::rejected(
                "Restart StagePilot to apply the updated Planning Center settings.",
            );
        }
        let previous_settings = lock(&inner.settings).clone();
        let previous_client = lock(&inner.client).clone();
        *lock(&inner.settings) = settings.clone();
        if let Err(error) = inner.validate_configuration() {
            *lock(&inner.settings) = previous_settings;
            let detail = inner.with_cache_warning(&error.to_string());
            inner.record_error(&detail, &error, true);
            inner
                .publish_connection(ConnectionStatus::Error, &detail)
                .await;
            return ActionOutcome::rejected(detail);
        }
        match (inner.factory)(&settings) {
            Ok(client) => *lock(&inner.client) = Some(client),
            Err(error) => {
                *lock(&inner.settings) = previous_settings;
                *lock(&inner.client) = previous_client;
                let detail = "Planning Center settings could not be applied to the running plugin.";
                inner.record_error(detail, &error, true);
                inner
                    .publish_connection(ConnectionStatus::Error, detail)
                    .await;
                return ActionOutcome::rejected(detail);
            }
        }
        *lock(&inner.plan_list) = None;
        inner.request_regular_refresh(true).await;
        let new_client = lock(&inner.client).clone();
        if let (Some(previous), Some(current)) = (previous_client, new_client) {
            if !Arc::ptr_eq(&previous, &current) {
                previous.close().await;
            }
        }
        let (status, is_configuration, last_error) = {
            let h = lock(&inner.health);
            (
                h.status,
                h.last_error_is_configuration,
                h.last_error.clone(),
            )
        };
        if status == PluginStatus::Error && is_configuration {
            return ActionOutcome::rejected(last_error.unwrap_or_else(|| {
                "Planning Center settings were applied but the connection failed.".to_string()
            }));
        }
        // A transient failure does NOT reject the settings save: the scheduled refresh retries on
        // its own and the dashboard already shows the live connection error. Rejecting here would
        // make the frontend reload and repeat the same failure in a loop (2026-10-03 bug).
        ActionOutcome::ok("Planning Center settings applied to the running service source.")
    }
}

impl Inner {
    fn today(&self) -> NaiveDate {
        (self.today)(self.timezone)
    }

    fn settings(&self) -> PcoSettings {
        lock(&self.settings).clone()
    }

    fn keeper(&self) -> Arc<PinKeeper> {
        lock(&self.pins).clone()
    }

    /// The pin that still counts today. A pin whose date has passed (in the configured time
    /// zone) is removed here, once, with a one-time notice.
    fn active_pin(&self) -> Option<ServicePin> {
        let keeper = self.keeper();
        let pin = keeper.get(SOURCE_PLANNING_CENTER)?;
        if !pin.is_expired(self.today()) {
            return Some(pin);
        }
        if let Err(error) = keeper.clear() {
            tracing::warn!(reason = %error, "service_pin_expired_not_cleared");
        }
        *lock(&self.notice) = Some(NOTICE_PIN_PASSED.to_string());
        None
    }

    /// Tell the state which service is loaded and whether the operator picked it. `hint` is
    /// the result of a load that is about to be projected; without one the loaded plan is read
    /// from the state.
    fn publish_selection(&self, hint: Option<SelectionHint>, plan: Option<&ServicePlan>) {
        let notice = lock(&self.notice).clone();
        let pin = self.keeper().get(SOURCE_PLANNING_CENTER);
        let today = self.today();
        self.store.mutate(|s| {
            let plan = plan.or(s.plan.as_ref()).cloned();
            // A pick that is (now) the upcoming service is simply the upcoming service.
            let is_upcoming =
                |p: &ServicePlan, u: Option<&UpcomingService>| u.is_some_and(|u| u.plan_id == p.id);
            let pinned = match (&hint, &plan, &pin) {
                (Some(h), Some(p), _) => h.pinned && !is_upcoming(p, h.upcoming.as_ref()),
                (None, Some(p), Some(k)) => {
                    k.plan_id == p.id
                        && !k.is_expired(today)
                        && !is_upcoming(p, s.upcoming.as_ref())
                }
                _ => false,
            };
            let upcoming = if pinned {
                hint.as_ref()
                    .and_then(|h| h.upcoming.clone())
                    .or_else(|| s.upcoming.clone().filter(|u| u.date >= today))
            } else if s.service_load.status == ServiceLoadStatus::Loaded || hint.is_some() {
                plan.as_ref().map(|p| UpcomingService {
                    plan_id: p.id.clone(),
                    date: p.date,
                    title: p.title.clone(),
                })
            } else {
                None
            };
            s.selection = Some(ServiceSelection {
                pinned,
                plan_id: plan.as_ref().map(|p| p.id.clone()),
                date: plan.as_ref().map(|p| p.date),
                notice,
            });
            s.upcoming = upcoming;
        });
    }

    /// The upcoming service as plain discovery finds it (no pin), without keeping its songs.
    async fn discover_upcoming(
        &self,
        search_date: NaiveDate,
        previous_plan_service_type_id: Option<&str>,
    ) -> Option<UpcomingService> {
        let mut ctx = LoadContext {
            api_connected: false,
            configured: None,
            previous_type_active: true,
            pinned_loaded: false,
            pin_vanished: false,
        };
        match self
            .load_result(
                search_date,
                None,
                previous_plan_service_type_id,
                &mut ctx,
                None,
            )
            .await
        {
            Ok(PlanDiscoveryResult::Loaded { candidate, .. }) => Some(UpcomingService {
                plan_id: candidate.id,
                date: candidate.target_date,
                title: candidate.title,
            }),
            _ => None,
        }
    }

    /// The plans the picker lists (cached for a minute): one plans call per service type.
    async fn picker_plans(&self) -> Result<Vec<UpcomingPlan>, PcoError> {
        let today = self.today();
        let key = format!(
            "{}|{today}",
            self.settings().service_type_id.unwrap_or_default()
        );
        let ttl = StdDuration::from_millis(self.plan_list_ttl_ms.load(Ordering::SeqCst));
        if let Some(cached) = lock(&self.plan_list).as_ref() {
            if cached.key == key && cached.at.elapsed() < ttl {
                return Ok(cached.plans.clone());
            }
        }
        let client = self.client()?;
        let service_types = client.list_service_types().await?;
        let configured = self.configured_service_types(&service_types)?;
        let mut found = client
            .list_upcoming_plans(&configured, today, PICKER_DAYS, &self.timezone_name)
            .await?;
        found.sort_by(|a, b| {
            (a.target_date, a.service_times.first(), a.id.as_str()).cmp(&(
                b.target_date,
                b.service_times.first(),
                b.id.as_str(),
            ))
        });
        found.truncate(PICKER_MAX_PLANS);
        let plans: Vec<UpcomingPlan> = found
            .into_iter()
            .map(|c| UpcomingPlan {
                id: c.id,
                date: c.target_date,
                title: c.title,
                service_type_id: c.service_type_id,
                service_type_name: c.service_type_name,
            })
            .collect();
        *lock(&self.plan_list) = Some(CachedPlanList {
            at: Instant::now(),
            key,
            plans: plans.clone(),
        });
        Ok(plans)
    }

    /// The midnight rollover: a held pin whose date has passed is dropped by a refresh, which
    /// then loads the upcoming plan.
    async fn check_pin_expiry(self: &Arc<Self>) {
        let expired = self
            .keeper()
            .get(SOURCE_PLANNING_CENTER)
            .is_some_and(|p| p.is_expired(self.today()));
        if expired && !self.stopping.load(Ordering::SeqCst) {
            self.request_regular_refresh(false).await;
        }
    }

    fn with_cache_warning(&self, detail: &str) -> String {
        match lock(&self.cache_warning).as_deref() {
            Some(warning) => format!("{detail} {warning}"),
            None => detail.to_string(),
        }
    }

    fn validate_configuration(&self) -> Result<(), PcoError> {
        let settings = self.settings();
        if !settings.is_configured() {
            if let Some(issue) = &settings.credential_issue {
                return Err(PcoError::configuration(issue));
            }
            return Err(PcoError::configuration(
                "Planning Center credentials are not configured.",
            ));
        }
        if settings.service_type_id.is_none() {
            return Err(PcoError::configuration(
                "A Planning Center service type must be configured.",
            ));
        }
        Ok(())
    }

    fn record_error(&self, detail: &str, error: &PcoError, is_configuration: bool) {
        let mut health = lock(&self.health);
        health.status = PluginStatus::Error;
        health.last_error = Some(detail.to_string());
        health.last_error_is_configuration = is_configuration;
        health.last_activity_at = Some(Utc::now());
        // Never include messages, args, response bodies or service type names.
        tracing::warn!(
            exception_type = error.type_name(),
            http_status = error.http_status(),
            error_type = "safe_planning_center_error",
            "planning_center_load_failed"
        );
    }

    async fn publish_connection(&self, status: ConnectionStatus, detail: &str) {
        self.bus
            .publish(new_event(
                EventType::ConnectionChanged,
                PLUGIN_NAME,
                EventPayload::Connection {
                    integration: Integration::PlanningCenter,
                    status,
                    detail: Some(detail.to_string()),
                },
            ))
            .await;
    }

    #[allow(clippy::too_many_arguments)]
    async fn publish_load_state(
        &self,
        status: ServiceLoadStatus,
        target_date: NaiveDate,
        candidates: Vec<ServicePlanCandidate>,
        skipped_items: Vec<SkippedServiceItem>,
        message: Option<String>,
        is_stale: bool,
    ) {
        self.bus
            .publish(new_event(
                EventType::ServiceLoadChanged,
                PLUGIN_NAME,
                EventPayload::ServiceLoad {
                    status,
                    target_date: Some(target_date),
                    candidates,
                    skipped_items,
                    message,
                    is_stale,
                },
            ))
            .await;
    }

    fn client(&self) -> Result<Arc<dyn PcoApi>, PcoError> {
        lock(&self.client)
            .clone()
            .ok_or_else(|| PcoError::configuration("Planning Center is not initialized."))
    }

    async fn close_client(&self) {
        let client = lock(&self.client).take();
        if let Some(client) = client {
            client.close().await;
        }
    }

    // ---- scheduling -------------------------------------------------------------------------------

    /// Run (or queue) a regular refresh; `wait` joins the refresh that is running or just started.
    async fn request_regular_refresh(self: &Arc<Self>, wait: bool) {
        if self.stopping.load(Ordering::SeqCst) {
            return;
        }
        {
            let mut sched = lock(&self.sched);
            if sched.running {
                sched.pending_reload = true;
            } else {
                sched.running = true;
                self.idle.send_replace(false);
                let this = self.clone();
                tokio::spawn(async move { this.run_scheduled_refresh(None).await });
            }
        }
        if wait {
            self.wait_idle().await;
        }
    }

    async fn wait_idle(&self) {
        let mut rx = self.idle.subscribe();
        let _ = rx.wait_for(|idle| *idle).await;
    }

    async fn run_scheduled_refresh(self: Arc<Self>, initial_selection: Option<String>) {
        let mut selected = initial_selection;
        loop {
            if self.stopping.load(Ordering::SeqCst) {
                break;
            }
            self.refresh_once(selected.take()).await;
            let mut sched = lock(&self.sched);
            if let Some(next) = sched.pending_selection.take() {
                sched.pending_reload = false;
                selected = Some(next);
                continue;
            }
            if sched.pending_reload {
                sched.pending_reload = false;
                continue;
            }
            break;
        }
        let mut sched = lock(&self.sched);
        sched.running = false;
        sched.pending_reload = false;
        sched.pending_selection = None;
        self.idle.send_replace(true);
    }

    async fn on_reload_requested(self: &Arc<Self>) {
        if self.stopping.load(Ordering::SeqCst) {
            return;
        }
        let mut sched = lock(&self.sched);
        if sched.running {
            sched.pending_reload = true;
            return;
        }
        sched.running = true;
        self.idle.send_replace(false);
        let this = self.clone();
        tokio::spawn(async move { this.run_scheduled_refresh(None).await });
    }

    async fn on_plan_selection_requested(self: &Arc<Self>, plan_id: &str) {
        if self.stopping.load(Ordering::SeqCst) {
            return;
        }
        let snapshot = self.store.snapshot();
        // The operator's pick (the pin the route just saved) or "back to the upcoming service"
        // (the pin was cleared; the id is the upcoming plan's, or empty): load as a refresh
        // does, which reads the pin.
        let pinned_pick = self
            .keeper()
            .get(SOURCE_PLANNING_CENTER)
            .is_some_and(|p| p.plan_id == plan_id);
        let back_to_upcoming = plan_id.is_empty()
            || (self.keeper().get(SOURCE_PLANNING_CENTER).is_none()
                && snapshot
                    .upcoming
                    .as_ref()
                    .is_some_and(|u| u.plan_id == plan_id));
        if pinned_pick || back_to_upcoming {
            self.request_regular_refresh(true).await;
            return;
        }
        let today = self.today();
        let target_date = snapshot.service_load.target_date;
        let candidate_ids: Vec<&str> = snapshot
            .service_load
            .candidates
            .iter()
            .filter(|c| Some(c.target_date) == target_date)
            .map(|c| c.id.as_str())
            .collect();
        if snapshot.service_load.status != ServiceLoadStatus::Ambiguous
            || target_date.is_none()
            || target_date.is_some_and(|d| d < today)
        {
            if snapshot
                .service_load
                .candidates
                .iter()
                .any(|c| c.id == plan_id)
            {
                self.request_regular_refresh(true).await;
            }
            return;
        }
        if !candidate_ids.contains(&plan_id) {
            return;
        }
        {
            let mut sched = lock(&self.sched);
            if sched.running {
                sched.pending_selection = Some(plan_id.to_string());
            } else {
                sched.running = true;
                self.idle.send_replace(false);
                let this = self.clone();
                let id = plan_id.to_string();
                tokio::spawn(async move { this.run_scheduled_refresh(Some(id)).await });
            }
        }
        self.wait_idle().await;
    }

    // ---- the refresh itself ----------------------------------------------------------------------------

    fn actionable_plan_date(state: &ApplicationState, search_date: NaiveDate) -> Option<NaiveDate> {
        state
            .plan
            .as_ref()
            .filter(|p| p.date >= search_date)
            .map(|p| p.date)
    }

    fn configured_service_types(
        &self,
        service_types: &[ServiceType],
    ) -> Result<Vec<ServiceType>, PcoError> {
        let configured_id = self.settings().service_type_id;
        if configured_id.as_deref() == Some(ALL_SERVICE_TYPES_ID) {
            let active: Vec<ServiceType> = service_types
                .iter()
                .filter(|s| !s.archived)
                .cloned()
                .collect();
            if active.is_empty() {
                return Err(PcoError::configuration(
                    "No active Planning Center service types are available.",
                ));
            }
            return Ok(active);
        }
        let service_type = service_types
            .iter()
            .find(|s| Some(&s.id) == configured_id.as_ref())
            .ok_or_else(|| {
                PcoError::configuration(
                    "The configured Planning Center service type is unavailable.",
                )
            })?;
        if service_type.archived {
            return Err(PcoError::configuration(
                "The configured Planning Center service type is archived.",
            ));
        }
        Ok(vec![service_type.clone()])
    }

    async fn load_result(
        &self,
        search_date: NaiveDate,
        selected_plan_id: Option<&str>,
        previous_plan_service_type_id: Option<&str>,
        ctx: &mut LoadContext,
        pin: Option<&ServicePin>,
    ) -> Result<PlanDiscoveryResult, PcoError> {
        let client = self.client()?;
        let service_types = client.list_service_types().await?;
        ctx.api_connected = true;
        let configured = self.configured_service_types(&service_types)?;
        ctx.configured = Some(configured.clone());
        let settings = self.settings();
        ctx.previous_type_active = previous_plan_service_type_id.is_none()
            || settings.service_type_id.as_deref() != Some(ALL_SERVICE_TYPES_ID)
            || configured
                .iter()
                .any(|s| Some(s.id.as_str()) == previous_plan_service_type_id);
        if let (Some(pin), None) = (pin, selected_plan_id) {
            // The operator picked this service: load exactly that plan, on its own date, the
            // way the upcoming plan is loaded. A plan that is gone (or no longer in the
            // configured service types) falls through to the normal upcoming search.
            let pinned_type = pin
                .service_type_id
                .as_deref()
                .and_then(|id| configured.iter().find(|s| s.id == id));
            if let Some(service_type) = pinned_type {
                match client
                    .load_plan_for_date(
                        service_type,
                        pin.date,
                        &self.timezone_name,
                        Some(&pin.plan_id),
                        0,
                    )
                    .await
                {
                    Ok(result @ PlanDiscoveryResult::Loaded { .. }) => {
                        ctx.pinned_loaded = true;
                        return Ok(result);
                    }
                    Ok(_) | Err(PcoError::PlanSelection(_)) => {}
                    Err(error) => return Err(error),
                }
            }
            ctx.pin_vanished = true;
        }
        let lookahead = i64::from(settings.upcoming_lookahead_days);
        let mut result = if settings.service_type_id.as_deref() == Some(ALL_SERVICE_TYPES_ID) {
            client
                .load_plan_for_service_types(
                    &configured,
                    search_date,
                    &self.timezone_name,
                    selected_plan_id,
                    lookahead,
                )
                .await?
        } else {
            client
                .load_plan_for_date(
                    &configured[0],
                    search_date,
                    &self.timezone_name,
                    selected_plan_id,
                    lookahead,
                )
                .await?
        };
        if let (PlanDiscoveryResult::Ambiguous { candidates, .. }, None) =
            (&result, selected_plan_id)
        {
            if let Some(preferred) = preferred_candidate(&settings, candidates) {
                let preferred_id = preferred.id.clone();
                let candidates = candidates.clone();
                result = client
                    .resolve_selected_plan(&candidates, &configured, search_date, &preferred_id)
                    .await?;
            }
        }
        Ok(result)
    }

    async fn refresh_once(self: &Arc<Self>, selected_plan_id: Option<String>) {
        let search_date = self.today();
        let pin = self.active_pin();
        let previous_state = self.store.snapshot();
        let previous_plan_date = Self::actionable_plan_date(&previous_state, search_date);
        let has_actionable_plan = previous_plan_date.is_some();
        let previous_candidate_date = previous_state.service_load.target_date;
        let previous_candidates: Vec<ServicePlanCandidate> = match previous_candidate_date {
            Some(date) if date >= search_date => previous_state
                .service_load
                .candidates
                .iter()
                .filter(|c| c.target_date == date)
                .cloned()
                .collect(),
            _ => vec![],
        };
        let previous_skipped = if has_actionable_plan {
            previous_state.service_load.skipped_items.clone()
        } else {
            vec![]
        };
        let previous_plan_service_type_id = if has_actionable_plan {
            previous_state
                .plan
                .as_ref()
                .and_then(|p| p.service_type_id.clone())
        } else {
            None
        };
        let retained_target_date = previous_plan_date
            .or(if previous_candidates.len() >= 2 {
                previous_candidate_date
            } else {
                None
            })
            .unwrap_or(search_date);
        self.publish_load_state(
            ServiceLoadStatus::Loading,
            retained_target_date,
            vec![],
            previous_skipped.clone(),
            Some("Looking for the current or next upcoming Planning Center plan.".into()),
            has_actionable_plan,
        )
        .await;
        self.publish_connection(
            ConnectionStatus::Connecting,
            "Loading Planning Center Services.",
        )
        .await;

        let mut ctx = LoadContext {
            api_connected: false,
            configured: None,
            previous_type_active: true,
            pinned_loaded: false,
            pin_vanished: false,
        };
        let keep_candidates = selected_plan_id.is_some() && previous_candidates.len() >= 2;
        let outcome = self
            .load_result(
                search_date,
                selected_plan_id.as_deref(),
                previous_plan_service_type_id.as_deref(),
                &mut ctx,
                pin.as_ref(),
            )
            .await;
        let result = match outcome {
            Ok(result) => result,
            Err(error @ PcoError::PlanSelection(_)) => {
                let detail = error.to_string();
                self.record_error(&detail, &error, true);
                self.publish_connection(ConnectionStatus::Connected, &detail)
                    .await;
                let ambiguous = previous_candidates.len() >= 2;
                self.publish_load_state(
                    if ambiguous {
                        ServiceLoadStatus::Ambiguous
                    } else {
                        ServiceLoadStatus::Error
                    },
                    retained_target_date,
                    previous_candidates,
                    previous_skipped,
                    Some(detail),
                    has_actionable_plan,
                )
                .await;
                return;
            }
            Err(error) if error.is_planning_center_error() => {
                // A settings/OAuth change can queue a replacement refresh while the previous
                // request is still in flight: do not publish that superseded failure.
                if lock(&self.sched).pending_reload {
                    return;
                }
                let detail = self.with_cache_warning(&error.to_string());
                // Only a genuinely invalid configuration may make reconfigure() reject a save;
                // timeouts, rate limits and transport errors are transient and retried.
                let is_configuration = matches!(error, PcoError::Configuration(_));
                self.record_error(&detail, &error, is_configuration);
                let status = if ctx.api_connected && is_configuration {
                    ConnectionStatus::Connected
                } else {
                    ConnectionStatus::Error
                };
                self.publish_connection(status, &detail).await;
                self.publish_load_state(
                    if keep_candidates {
                        ServiceLoadStatus::Ambiguous
                    } else {
                        ServiceLoadStatus::Error
                    },
                    retained_target_date,
                    if keep_candidates {
                        previous_candidates
                    } else {
                        vec![]
                    },
                    previous_skipped,
                    Some(detail),
                    has_actionable_plan,
                )
                .await;
                return;
            }
            Err(error) => {
                let detail =
                    self.with_cache_warning("Planning Center plan loading failed unexpectedly.");
                tracing::error!(
                    exception_type = error.type_name(),
                    "planning_center_unexpected_load_failure"
                );
                self.record_error(&detail, &error, true);
                self.publish_connection(ConnectionStatus::Error, &detail)
                    .await;
                self.publish_load_state(
                    if keep_candidates {
                        ServiceLoadStatus::Ambiguous
                    } else {
                        ServiceLoadStatus::Error
                    },
                    retained_target_date,
                    if keep_candidates {
                        previous_candidates
                    } else {
                        vec![]
                    },
                    previous_skipped,
                    Some(detail),
                    has_actionable_plan,
                )
                .await;
                return;
            }
        };

        {
            let mut health = lock(&self.health);
            health.status = PluginStatus::Running;
            health.last_error = None;
            health.last_activity_at = Some(Utc::now());
        }
        self.publish_connection(
            ConnectionStatus::Connected,
            "Planning Center Services is connected.",
        )
        .await;
        if ctx.pin_vanished {
            if let Err(error) = self.keeper().clear() {
                tracing::warn!(reason = %error, "service_pin_vanished_not_cleared");
            }
            *lock(&self.notice) = Some(NOTICE_PIN_VANISHED.to_string());
        }
        let hint = if ctx.pinned_loaded {
            // Another service is loaded on purpose: still say which one is the upcoming one.
            let upcoming = self
                .discover_upcoming(search_date, previous_plan_service_type_id.as_deref())
                .await;
            SelectionHint {
                pinned: true,
                upcoming,
                pinned_date: pin.as_ref().map(|p| p.date),
            }
        } else {
            SelectionHint {
                pinned: false,
                upcoming: None,
                pinned_date: None,
            }
        };
        if let Err(error) = self
            .apply_result(
                result,
                search_date,
                previous_plan_date,
                previous_skipped.clone(),
                ctx.previous_type_active,
                hint,
            )
            .await
        {
            let detail = "Planning Center plan projection failed unexpectedly.";
            tracing::error!(
                exception_type = error.type_name(),
                "planning_center_unexpected_projection_failure"
            );
            self.record_error(detail, &error, false);
            let current = self.store.snapshot();
            let current_plan_date = Self::actionable_plan_date(&current, search_date);
            self.publish_connection(ConnectionStatus::Error, detail)
                .await;
            self.publish_load_state(
                if keep_candidates {
                    ServiceLoadStatus::Ambiguous
                } else {
                    ServiceLoadStatus::Error
                },
                current_plan_date.unwrap_or(retained_target_date),
                if keep_candidates {
                    previous_candidates
                } else {
                    vec![]
                },
                previous_skipped,
                Some(detail.to_string()),
                current_plan_date.is_some(),
            )
            .await;
        } else {
            // Not found / ambiguous outcomes and a pin that ended carry no plan of their own;
            // say so, with the one-time notice if there is one.
            self.publish_selection(None, None);
        }
    }

    async fn apply_result(
        &self,
        result: PlanDiscoveryResult,
        search_date: NaiveDate,
        previous_plan_date: Option<NaiveDate>,
        previous_skipped: Vec<SkippedServiceItem>,
        previous_plan_type_still_active: bool,
        hint: SelectionHint,
    ) -> Result<(), PcoError> {
        let lookahead = i64::from(self.settings().upcoming_lookahead_days);
        let search_end = search_date + Duration::days(lookahead);
        // A pinned plan may be dated beyond the upcoming search window.
        let loaded_end = hint.pinned_date.map_or(search_end, |d| d.max(search_end));
        match result {
            PlanDiscoveryResult::NotFound { target_date, .. } => {
                if target_date != search_date {
                    return Err(PcoError::Unexpected(
                        "Planning Center returned an invalid search anchor.".into(),
                    ));
                }
                let retained = previous_plan_date.is_some() && previous_plan_type_still_active;
                let message = if retained {
                    format!(
                        "No current or upcoming Planning Center plan was found through {search_end}; keeping the previously loaded plan stale."
                    )
                } else if previous_plan_date.is_some() && !previous_plan_type_still_active {
                    "The previously loaded Planning Center service's type has been archived and no replacement was found; the service is no longer available.".to_string()
                } else {
                    format!(
                        "No Planning Center service plan was found from {target_date} through {search_end}."
                    )
                };
                self.publish_load_state(
                    ServiceLoadStatus::NotFound,
                    if retained {
                        previous_plan_date.unwrap_or(target_date)
                    } else {
                        target_date
                    },
                    vec![],
                    if retained { previous_skipped } else { vec![] },
                    Some(message),
                    retained,
                )
                .await;
                Ok(())
            }
            PlanDiscoveryResult::Ambiguous {
                target_date,
                candidates,
                ..
            } => {
                if !(search_date <= target_date && target_date <= search_end) {
                    return Err(PcoError::Unexpected(
                        "Planning Center returned candidates outside the search window.".into(),
                    ));
                }
                let retained = previous_plan_date == Some(target_date);
                self.publish_load_state(
                    ServiceLoadStatus::Ambiguous,
                    target_date,
                    candidates.iter().map(candidate_view).collect(),
                    if retained { previous_skipped } else { vec![] },
                    Some(format!(
                        "Multiple Planning Center plans match {target_date}. Select one to continue."
                    )),
                    retained,
                )
                .await;
                Ok(())
            }
            PlanDiscoveryResult::Loaded {
                candidate,
                plan,
                skipped_items,
            } => {
                let loaded_date = plan.date;
                if !(search_date <= loaded_date && loaded_date <= loaded_end)
                    || candidate.target_date != loaded_date
                    || candidate.id != plan.id
                {
                    return Err(PcoError::Unexpected(
                        "Planning Center returned an invalid loaded plan.".into(),
                    ));
                }
                if previous_plan_date.is_some_and(|d| d != loaded_date) {
                    self.publish_load_state(
                        ServiceLoadStatus::Loading,
                        loaded_date,
                        vec![],
                        vec![],
                        Some(format!(
                            "Loading the Planning Center plan for {loaded_date}."
                        )),
                        false,
                    )
                    .await;
                }
                // Which service this is, before the plan itself arrives, so nothing shows the
                // new plan with the old "picked / upcoming" answer.
                self.publish_selection(Some(hint), Some(&plan));
                let before = self.store.snapshot();
                let report = self
                    .bus
                    .publish(new_event(
                        EventType::ServiceLoaded,
                        PLUGIN_NAME,
                        EventPayload::Service { plan: plan.clone() },
                    ))
                    .await;
                let projected = self.store.snapshot();
                if !report.failures.is_empty()
                    || projected.revision <= before.revision
                    || projected.plan.as_ref() != Some(&plan)
                {
                    return Err(PcoError::Unexpected(
                        "Planning Center plan was not projected into application state.".into(),
                    ));
                }
                let message = self.cache_fresh_plan(
                    &plan,
                    projected
                        .last_successful_plan_reload_at
                        .unwrap_or_else(Utc::now),
                );
                self.publish_load_state(
                    ServiceLoadStatus::Loaded,
                    loaded_date,
                    vec![],
                    skipped_items
                        .iter()
                        .map(|i| SkippedServiceItem {
                            item_id: i.item_id.clone(),
                            title: i.title.clone(),
                            description: i.description.clone(),
                            item_type: i.item_type.clone(),
                            sequence: i.sequence,
                            duration_seconds: i.duration_seconds,
                            reason: i.reason.as_str().to_string(),
                        })
                        .collect(),
                    Some(message),
                    false,
                )
                .await;
                Ok(())
            }
        }
    }

    fn cache_fresh_plan(&self, plan: &ServicePlan, refreshed_at: DateTime<Utc>) -> String {
        let saved = self.cache.save(&CachedServicePlan {
            schema_version: 1,
            plan: plan.clone(),
            last_successful_refresh: refreshed_at,
        });
        *lock(&self.cache_warning) = saved.err().map(|e| e.0);
        let message = format!(
            "Loaded \"{}\" for {} from Planning Center.",
            plan.title, plan.date
        );
        self.with_cache_warning(&message)
    }

    async fn restore_cached_plan(&self) {
        let cached = match self.cache.load() {
            Ok(Some(cached)) => cached,
            Ok(None) => return,
            Err(error) => {
                *lock(&self.cache_warning) = Some(error.0);
                return;
            }
        };
        let today = self.today();
        if cached.plan.date < today {
            *lock(&self.cache_warning) = Some(format!(
                "The cached service \"{}\" expired on {} and was not loaded.",
                cached.plan.title, cached.plan.date
            ));
            return;
        }
        let configured_type = self.settings().service_type_id;
        if configured_type.as_deref() != Some(ALL_SERVICE_TYPES_ID)
            && cached.plan.service_type_id != configured_type
        {
            *lock(&self.cache_warning) = Some(
                "The cached service belongs to a different Planning Center service type and was not loaded."
                    .into(),
            );
            return;
        }
        let report = self
            .bus
            .publish(new_event(
                EventType::ServiceLoaded,
                PLUGIN_NAME,
                EventPayload::Service {
                    plan: cached.plan.clone(),
                },
            ))
            .await;
        let state = self.store.snapshot();
        if !report.failures.is_empty() || state.plan.as_ref() != Some(&cached.plan) {
            *lock(&self.cache_warning) =
                Some("The last-known-good service could not be restored.".into());
            return;
        }
        let refreshed = cached.last_successful_refresh;
        self.store
            .mutate(|s| s.last_successful_plan_reload_at = Some(refreshed));
        self.publish_selection(None, Some(&cached.plan));
        self.publish_load_state(
            ServiceLoadStatus::Loading,
            cached.plan.date,
            vec![],
            vec![],
            Some(format!(
                "Loaded cached service \"{}\" from {}; refreshing Planning Center.",
                cached.plan.title,
                refreshed.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false)
            )),
            true,
        )
        .await;
        *lock(&self.cache_warning) = Some(
            "The displayed service was loaded from the last-known-good cache and may be stale."
                .into(),
        );
    }

    async fn handle_start_failure(&self, detail: &str) {
        {
            let mut health = lock(&self.health);
            health.status = PluginStatus::Error;
            health.last_error = Some(detail.to_string());
            health.last_activity_at = Some(Utc::now());
        }
        let state = self.store.snapshot();
        let target = self.today();
        let plan_date = Self::actionable_plan_date(&state, target);
        self.publish_connection(ConnectionStatus::Error, detail)
            .await;
        self.publish_load_state(
            ServiceLoadStatus::Error,
            plan_date.unwrap_or(target),
            vec![],
            if plan_date.is_some() {
                state.service_load.skipped_items.clone()
            } else {
                vec![]
            },
            Some(detail.to_string()),
            plan_date.is_some(),
        )
        .await;
        for subscription in lock(&self.subscriptions).drain(..) {
            self.bus.unsubscribe(&subscription);
        }
        self.close_client().await;
    }
}

fn candidate_view(candidate: &PlanCandidate) -> ServicePlanCandidate {
    ServicePlanCandidate {
        id: candidate.id.clone(),
        title: candidate.title.clone(),
        service_type_id: candidate.service_type_id.clone(),
        service_type_name: candidate.service_type_name.clone(),
        target_date: candidate.target_date,
        service_times: candidate
            .service_times
            .iter()
            .map(|t| t.format("%H:%M").to_string())
            .collect(),
    }
}

#[async_trait]
impl Plugin for PcoPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    async fn start(&self) -> Result<(), String> {
        let inner = &self.inner;
        lock(&inner.health).status = PluginStatus::Starting;
        inner.stopping.store(false, Ordering::SeqCst);
        inner.restore_cached_plan().await;
        let prepared = inner.validate_configuration().and_then(|_| {
            let settings = inner.settings();
            (inner.factory)(&settings)
        });
        match prepared {
            Ok(client) => *lock(&inner.client) = Some(client),
            Err(error) => {
                let detail = inner.with_cache_warning(&error.to_string());
                inner.handle_start_failure(&detail).await;
                return Err(error.to_string());
            }
        }
        let mut subscriptions = Vec::new();
        {
            let this = inner.clone();
            subscriptions.push(inner.bus.subscribe(
                Some(EventType::ServiceReloadRequested),
                move |_event| {
                    let this = this.clone();
                    async move {
                        this.on_reload_requested().await;
                        Ok(())
                    }
                },
            ));
            let this = inner.clone();
            subscriptions.push(inner.bus.subscribe(
                Some(EventType::ServicePlanSelectionRequested),
                move |event| {
                    let this = this.clone();
                    async move {
                        if let EventPayload::ServicePlanSelection { plan_id } = &event.payload {
                            this.on_plan_selection_requested(plan_id).await;
                        }
                        Ok(())
                    }
                },
            ));
        }
        lock(&inner.subscriptions).extend(subscriptions);
        lock(&inner.health).status = PluginStatus::Running;
        {
            // The midnight rollover: nothing else reloads the plan by itself, so a held pin
            // is checked against the date now and then.
            let this = inner.clone();
            let handle = tokio::spawn(async move {
                loop {
                    let wait = this.expiry_interval_ms.load(Ordering::SeqCst);
                    tokio::time::sleep(StdDuration::from_millis(wait)).await;
                    if this.stopping.load(Ordering::SeqCst) {
                        break;
                    }
                    this.check_pin_expiry().await;
                }
            });
            if let Some(previous) = lock(&inner.expiry_watch).replace(handle) {
                previous.abort();
            }
        }
        inner.request_regular_refresh(true).await;
        Ok(())
    }

    async fn stop(&self) -> Result<(), String> {
        let inner = &self.inner;
        if lock(&inner.health).status == PluginStatus::Stopped && lock(&inner.client).is_none() {
            return Ok(());
        }
        lock(&inner.health).status = PluginStatus::Stopping;
        inner.stopping.store(true, Ordering::SeqCst);
        if let Some(watch) = lock(&inner.expiry_watch).take() {
            watch.abort();
        }
        for subscription in lock(&inner.subscriptions).drain(..) {
            inner.bus.unsubscribe(&subscription);
        }
        // An in-flight refresh sees `stopping` at its next loop turn; wait for it to finish.
        inner.wait_idle().await;
        {
            let mut sched = lock(&inner.sched);
            sched.pending_reload = false;
            sched.pending_selection = None;
        }
        inner.close_client().await;
        inner
            .publish_connection(
                ConnectionStatus::Disconnected,
                "Planning Center plugin stopped.",
            )
            .await;
        let mut health = lock(&inner.health);
        health.status = PluginStatus::Stopped;
        health.last_activity_at = Some(Utc::now());
        Ok(())
    }

    async fn health(&self) -> Result<PluginHealth, String> {
        let health = lock(&self.inner.health);
        Ok(PluginHealth {
            name: PLUGIN_NAME.to_string(),
            version: PLUGIN_VERSION.to_string(),
            status: health.status,
            last_error: health.last_error.clone(),
            last_activity_at: health.last_activity_at,
        })
    }

    fn plan_picker(&self) -> Option<&dyn PlanPicker> {
        Some(self)
    }
}

#[async_trait]
impl PlanPicker for PcoPlugin {
    async fn upcoming_plans(&self) -> Result<UpcomingPlans, String> {
        let inner = &self.inner;
        // Drops a pin whose date has passed before it is reported.
        let pin = inner.active_pin();
        let plans = inner.picker_plans().await.map_err(|error| {
            let detail = error.to_string();
            tracing::warn!(
                exception_type = error.type_name(),
                http_status = error.http_status(),
                "planning_center_plan_list_failed"
            );
            if detail.is_empty() {
                "Planning Center could not list the upcoming services.".to_string()
            } else {
                detail
            }
        })?;
        let known = inner
            .store
            .snapshot()
            .upcoming
            .map(|u| u.plan_id)
            .filter(|id| plans.iter().any(|p| &p.id == id));
        let upcoming_id = known.or_else(|| {
            // Not known from the loaded plan: the only plan on the nearest date, if there is
            // exactly one, within the days the upcoming search looks.
            let lookahead = i64::from(inner.settings().upcoming_lookahead_days);
            let first = plans.first()?;
            if first.date > inner.today() + Duration::days(lookahead) {
                return None;
            }
            let same_day = plans.iter().filter(|p| p.date == first.date).count();
            (same_day == 1).then(|| first.id.clone())
        });
        Ok(UpcomingPlans {
            plans,
            upcoming_id,
            pinned_id: pin.as_ref().map(|p| p.plan_id.clone()),
            pinned_date: pin.map(|p| p.date),
        })
    }

    fn today(&self) -> NaiveDate {
        self.inner.today()
    }

    fn pin(&self) -> Option<ServicePin> {
        self.inner.keeper().get(SOURCE_PLANNING_CENTER)
    }

    fn set_pin(&self, plan: &UpcomingPlan) -> Result<(), String> {
        *lock(&self.inner.notice) = None;
        self.inner.keeper().set(ServicePin::new(
            SOURCE_PLANNING_CENTER,
            &plan.id,
            plan.date,
            Some(plan.service_type_id.clone()),
            Some(plan.title.clone()),
        ))
    }

    fn clear_pin(&self) -> Result<(), String> {
        *lock(&self.inner.notice) = None;
        self.inner.keeper().clear()
    }

    fn restore_pin(&self, previous: Option<ServicePin>) -> Result<(), String> {
        self.inner.keeper().restore(previous)
    }
}
