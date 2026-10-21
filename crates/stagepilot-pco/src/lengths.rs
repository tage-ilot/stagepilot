//! Optional plan-item-only writes, write-ahead undo, and idle-only reload verification: port of
//! `services/planning_center_lengths.py`.
//!
//! The Playback routes (owned by the Playback port) call this service; this crate has no Playback
//! dependency. Everything the Python service reads from the Playback controller is a hook:
//! `is_playing`, `write_allowed` and `plan_current`.

use crate::client::{extract_songs, PcoApi, PcoClientFactory};
use crate::errors::PcoError;
use crate::models::{PcoSettings, ServiceType};
use crate::plugin::preferred_candidate;
use chrono::Utc;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use stagepilot_core::events::ActionName;
use stagepilot_core::settings::{PlanningCenterLengthUndo, SettingsService};
use stagepilot_core::state::{ConnectionStatus, ServicePlan, TimerStatus};
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const DENIED_MESSAGE: &str = "Planning Center did not allow this change. The account or token used by StagePilot can't edit this plan.";
const CHANGED: &str = "Someone changed this item; their edit was left alone.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Updated,
    Restored,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LengthItemResult {
    pub item_id: String,
    pub title: String,
    #[serde(default)]
    pub old_length: Option<i64>,
    #[serde(default)]
    pub new_length: Option<i64>,
    pub status: ItemStatus,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Update,
    Restore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Idle,
    Running,
    Done,
    Partial,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReloadOutcome {
    NotNeeded,
    Verified,
    Deferred,
    Failed,
    DifferentPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LengthUpdateResult {
    pub operation: Operation,
    pub status: RunStatus,
    pub message: String,
    pub items: Vec<LengthItemResult>,
    pub reload: ReloadOutcome,
}

impl Default for LengthUpdateResult {
    fn default() -> Self {
        Self {
            operation: Operation::Update,
            status: RunStatus::Idle,
            message: String::new(),
            items: vec![],
            reload: ReloadOutcome::NotNeeded,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LengthPreview {
    pub token: String,
    pub category: String,
    pub plan_title: String,
    pub plan_date: String,
    pub items: Vec<LengthItemResult>,
    #[serde(default)]
    pub message: String,
}

/// A failure the Playback routes translate (Python raises `ValueError`, `PlanningCenter*Error`).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LengthError {
    /// `ValueError`: a user-correctable precondition (HTTP 409 in the routes).
    #[error("{0}")]
    Value(String),
    #[error(transparent)]
    Pco(#[from] PcoError),
}

struct PreviewState {
    preview: LengthPreview,
    candidates: Vec<PlanningCenterLengthUndo>,
    signature: String,
    created: Instant,
}

/// `m:ss`, or `unknown`.
pub fn duration(value: Option<i64>) -> String {
    match value {
        None => "unknown".to_string(),
        Some(v) => format!("{}:{:02}", v.div_euclid(60), v.rem_euclid(60)),
    }
}

/// Identity of the plan the scanned lengths were measured for (not its durations).
pub fn plan_key(plan: &ServicePlan) -> String {
    serde_json::to_string(&serde_json::json!([
        plan.service_type_id,
        plan.id,
        plan.songs.iter().map(|s| s.id.clone()).collect::<Vec<_>>()
    ]))
    .unwrap_or_default()
}

pub type PlanCurrent = Arc<dyn Fn() -> BoxFuture<'static, bool> + Send + Sync>;

pub struct LengthService {
    settings: Arc<SettingsService>,
    state: StateStore,
    dispatcher: StateService,
    factory: PcoClientFactory,
    effective: Arc<dyn Fn() -> PcoSettings + Send + Sync>,
    reload_timeout: Duration,
    pub is_playing: Arc<dyn Fn() -> bool + Send + Sync>,
    pub write_allowed: Mutex<Arc<dyn Fn() -> bool + Send + Sync>>,
    pub plan_current: PlanCurrent,
    pub on_applied: Arc<dyn Fn(&str) + Send + Sync>,
    result: Mutex<LengthUpdateResult>,
    lock: tokio::sync::Mutex<()>,
    preview: Mutex<Option<PreviewState>>,
    target_label: Mutex<String>,
}

fn lk<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl LengthService {
    pub fn new(
        settings: Arc<SettingsService>,
        state: StateStore,
        dispatcher: StateService,
        factory: PcoClientFactory,
        effective: Arc<dyn Fn() -> PcoSettings + Send + Sync>,
    ) -> Self {
        let is_playing: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(|| false);
        let playing = is_playing.clone();
        Self {
            settings,
            state,
            dispatcher,
            factory,
            effective,
            reload_timeout: Duration::from_secs(10),
            is_playing,
            write_allowed: Mutex::new(Arc::new(move || !playing())),
            plan_current: Arc::new(|| Box::pin(async { true })),
            on_applied: Arc::new(|_| {}),
            result: Mutex::new(LengthUpdateResult::default()),
            lock: tokio::sync::Mutex::new(()),
            preview: Mutex::new(None),
            target_label: Mutex::new(String::new()),
        }
    }

    pub fn with_reload_timeout(mut self, timeout: Duration) -> Self {
        self.reload_timeout = timeout;
        self
    }

    pub fn result(&self) -> LengthUpdateResult {
        lk(&self.result).clone()
    }

    pub fn undo_available(&self) -> bool {
        !self
            .settings
            .snapshot()
            .planning_center_length_undo
            .is_empty()
    }

    pub fn set_write_allowed(&self, hook: Arc<dyn Fn() -> bool + Send + Sync>) {
        *lk(&self.write_allowed) = hook;
    }

    fn write_allowed_now(&self) -> bool {
        let hook = lk(&self.write_allowed).clone();
        hook()
    }

    fn client(&self) -> Result<Arc<dyn PcoApi>, PcoError> {
        (self.factory)(&(self.effective)())
    }

    fn persist(&self, records: Vec<PlanningCenterLengthUndo>) -> Result<(), String> {
        let mut snapshot = self.settings.snapshot();
        snapshot.planning_center_length_undo = records;
        self.settings.save(snapshot).map_err(|e| e.0)
    }

    pub async fn categories(&self) -> Result<Vec<ServiceType>, PcoError> {
        let client = self.client()?;
        let result = client.list_service_types().await;
        client.close().await;
        Ok(result?.into_iter().filter(|c| !c.archived).collect())
    }

    pub async fn preview(
        &self,
        category_id: &str,
        lengths: &[Option<f64>],
        signature: &str,
    ) -> Result<LengthPreview, LengthError> {
        let _guard = self.lock.lock().await;
        *lk(&self.preview) = None;
        let runtime = self.settings.effective_runtime_settings();
        let pco = (self.effective)();
        let timezone = crate::client::parse_timezone(&runtime.timezone)?;
        let today = crate::plugin::local_today(timezone);
        let client = self.client()?;
        let outcome = async {
            let categories: Vec<ServiceType> = client
                .list_service_types()
                .await?
                .into_iter()
                .filter(|c| !c.archived)
                .collect();
            let category = categories
                .iter()
                .find(|c| c.id == category_id)
                .cloned()
                .ok_or_else(|| {
                    LengthError::Value("Choose an available Planning Center category.".into())
                })?;
            let mut result = client
                .load_plan_for_date(
                    &category,
                    today,
                    &runtime.timezone,
                    None,
                    i64::from(pco.upcoming_lookahead_days),
                )
                .await?;
            if let crate::models::PlanDiscoveryResult::Ambiguous { candidates, .. } = &result {
                if let Some(chosen) = preferred_candidate(&pco, candidates) {
                    let id = chosen.id.clone();
                    let candidates = candidates.clone();
                    result = client
                        .resolve_selected_plan(
                            &candidates,
                            std::slice::from_ref(&category),
                            today,
                            &id,
                        )
                        .await?;
                }
            }
            match result {
                crate::models::PlanDiscoveryResult::Loaded { plan, .. } => Ok(plan),
                crate::models::PlanDiscoveryResult::Ambiguous { .. } => Err(LengthError::Value(
                    "More than one upcoming plan matches. Set a preferred service time first."
                        .into(),
                )),
                crate::models::PlanDiscoveryResult::NotFound { .. } => Err(LengthError::Value(
                    "No upcoming plan in this category.".into(),
                )),
            }
        }
        .await;
        client.close().await;
        self.preview_plan(&outcome?, lengths, signature)
    }

    pub fn preview_plan(
        &self,
        plan: &ServicePlan,
        lengths: &[Option<f64>],
        signature: &str,
    ) -> Result<LengthPreview, LengthError> {
        let service_type_id = plan
            .service_type_id
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| LengthError::Value("The plan has no service type.".into()))?;
        let undo = self.settings.snapshot().planning_center_length_undo;
        let mut rows: Vec<LengthItemResult> = Vec::new();
        let mut candidates: Vec<PlanningCenterLengthUndo> = Vec::new();
        for (index, song) in plan.songs.iter().enumerate() {
            let measured = lengths.get(index).copied().flatten();
            let target = measured.map(|m| (m + 0.5).floor() as i64);
            let duration_seconds = song.duration_seconds.map(i64::from);
            let reason = if target.is_none() {
                Some("No measured Playback length.")
            } else if duration_seconds.is_none() {
                Some("The plan length is unknown.")
            } else if target == duration_seconds {
                Some("Already the same.")
            } else if undo.iter().any(|r| {
                (
                    r.service_type_id.as_str(),
                    r.plan_id.as_str(),
                    r.item_id.as_str(),
                ) == (service_type_id.as_str(), plan.id.as_str(), song.id.as_str())
                    && duration_seconds != Some(r.new_length)
                    && duration_seconds != r.pending_from_length
            }) {
                Some(CHANGED)
            } else {
                None
            };
            rows.push(LengthItemResult {
                item_id: song.id.clone(),
                title: song.title.clone(),
                old_length: duration_seconds,
                new_length: target,
                status: if reason.is_some() {
                    ItemStatus::Skipped
                } else {
                    ItemStatus::Updated
                },
                reason: reason.map(String::from),
            });
            if reason.is_none() {
                if let (Some(target), Some(old)) = (target, duration_seconds) {
                    candidates.push(PlanningCenterLengthUndo {
                        service_type_id: service_type_id.clone(),
                        plan_id: plan.id.clone(),
                        item_id: song.id.clone(),
                        title: song.title.clone(),
                        old_length: old,
                        new_length: target,
                        written_at: Utc::now(),
                        pending_from_length: None,
                    });
                }
            }
        }
        let message = if candidates.is_empty() {
            if rows
                .iter()
                .all(|r| r.reason.as_deref() == Some("Already the same."))
            {
                "Planning Center already matches Playback to the second."
            } else {
                "No song times can be changed."
            }
        } else {
            ""
        };
        let preview = LengthPreview {
            token: uuid::Uuid::new_v4().simple().to_string(),
            category: plan.service_type.clone(),
            plan_title: plan.title.clone(),
            plan_date: plan.date.to_string(),
            items: rows,
            message: message.to_string(),
        };
        *lk(&self.preview) = Some(PreviewState {
            preview: preview.clone(),
            candidates,
            signature: signature.to_string(),
            created: Instant::now(),
        });
        *lk(&self.target_label) = format!("{} {}", plan.title, plan.date);
        Ok(preview)
    }

    pub async fn confirm(&self, token: &str, signature: &str) -> Result<(), LengthError> {
        let _guard = self.lock.lock().await;
        let stale =
            || LengthError::Value("The scan or Playback changed. Preview the plan again.".into());
        let (preview, candidates, saved_signature, created) = match lk(&self.preview).as_ref() {
            Some(p) => (
                p.preview.clone(),
                p.candidates.clone(),
                p.signature.clone(),
                p.created,
            ),
            None => {
                return Err(LengthError::Value(
                    "Preview the plan again before updating.".into(),
                ))
            }
        };
        let current = (self.plan_current)().await;
        if token != preview.token
            || !current
            || signature != saved_signature
            || created.elapsed() > Duration::from_secs(300)
            || !self.write_allowed_now()
        {
            *lk(&self.preview) = None;
            return Err(stale());
        }
        *lk(&self.preview) = None;
        *lk(&self.result) = LengthUpdateResult {
            status: RunStatus::Running,
            items: preview
                .items
                .iter()
                .filter(|r| r.status == ItemStatus::Skipped)
                .cloned()
                .collect(),
            ..Default::default()
        };
        self.apply(&candidates, false).await;
        let result = self.result();
        if !candidates.is_empty()
            && result.status == RunStatus::Done
            && !result.items.iter().any(|i| i.status == ItemStatus::Failed)
        {
            // Never turn a finished write into an error.
            (self.on_applied)(&candidates[0].plan_id);
        }
        Ok(())
    }

    pub async fn restore(&self) {
        let _guard = self.lock.lock().await;
        *lk(&self.result) = LengthUpdateResult {
            operation: Operation::Restore,
            status: RunStatus::Running,
            ..Default::default()
        };
        let records = self.settings.snapshot().planning_center_length_undo;
        self.apply(&records, true).await;
    }

    fn push_item(&self, item: LengthItemResult) -> usize {
        let mut result = lk(&self.result);
        result.items.push(item);
        result.items.len() - 1
    }

    fn update_item(&self, index: usize, update: impl FnOnce(&mut LengthItemResult)) {
        update(&mut lk(&self.result).items[index]);
    }

    #[allow(clippy::too_many_lines)]
    async fn apply(&self, candidates: &[PlanningCenterLengthUndo], restore: bool) {
        let mut changed: Vec<PlanningCenterLengthUndo> = Vec::new();
        let mut completed: Vec<PlanningCenterLengthUndo> = Vec::new();
        let mut halted: Option<String> = None;
        let client = if candidates.is_empty() {
            None
        } else {
            match self.client() {
                Ok(c) => Some(c),
                Err(_) => {
                    halted = Some(
                        "Planning Center times could not be changed. Song order and lengths are saved."
                            .into(),
                    );
                    for r in candidates {
                        self.push_item(LengthItemResult {
                            item_id: r.item_id.clone(),
                            title: r.title.clone(),
                            old_length: None,
                            new_length: None,
                            status: ItemStatus::Failed,
                            reason: halted.clone(),
                        });
                    }
                    None
                }
            }
        };
        if let Some(client) = &client {
            for record in candidates {
                let expected = if restore {
                    record.new_length
                } else {
                    record.old_length
                };
                let target = if restore {
                    record.old_length
                } else {
                    record.new_length
                };
                let index = self.push_item(LengthItemResult {
                    item_id: record.item_id.clone(),
                    title: record.title.clone(),
                    old_length: Some(expected),
                    new_length: Some(target),
                    status: ItemStatus::Failed,
                    reason: None,
                });
                if let Some(reason) = &halted {
                    self.update_item(index, |i| i.reason = Some(reason.clone()));
                    continue;
                }
                let step = self
                    .apply_one(client.as_ref(), record, restore, index)
                    .await;
                match step {
                    Step::Changed(done) => {
                        changed.push(record.clone());
                        if restore && done {
                            completed.push(record.clone());
                        }
                    }
                    Step::Completed => completed.push(record.clone()),
                    Step::Skipped => {}
                    Step::Halted(reason) => halted = Some(reason),
                    Step::HaltedChanged(reason) => {
                        changed.push(record.clone());
                        halted = Some(reason);
                    }
                }
            }
            client.close().await;
        }
        if !changed.is_empty() {
            self.reload(&changed, restore).await;
        }
        if restore && !completed.is_empty() {
            let remaining: Vec<PlanningCenterLengthUndo> = self
                .settings
                .snapshot()
                .planning_center_length_undo
                .into_iter()
                .filter(|r| !completed.contains(r))
                .collect();
            if self.persist(remaining).is_err() {
                halted = Some("Restored times could not be cleared from the undo record.".into());
            }
        }
        let any_failed = lk(&self.result)
            .items
            .iter()
            .any(|i| i.status == ItemStatus::Failed);
        let failed = halted.is_some() || any_failed;
        lk(&self.result).status = if failed && !changed.is_empty() {
            RunStatus::Partial
        } else if failed {
            RunStatus::Failed
        } else {
            RunStatus::Done
        };
        self.summarize(halted.as_deref());
    }

    async fn apply_one(
        &self,
        client: &dyn PcoApi,
        record: &PlanningCenterLengthUndo,
        restore: bool,
        index: usize,
    ) -> Step {
        let mut expected = if restore {
            record.new_length
        } else {
            record.old_length
        };
        let target = if restore {
            record.old_length
        } else {
            record.new_length
        };
        let previous_records = self.settings.snapshot().planning_center_length_undo;
        let mut attempted = false;
        let mut accepted = false;
        let mut undo = record.clone();
        let skip = |this: &Self| {
            this.update_item(index, |i| {
                i.status = ItemStatus::Skipped;
                i.reason = Some(CHANGED.to_string());
            });
        };
        let outcome: Result<Step, PcoError> = async {
            let state = self.state.snapshot();
            if state.planning_center_status != ConnectionStatus::Connected {
                return Err(PcoError::configuration("Connect Planning Center first."));
            }
            if (self.is_playing)() {
                return Err(PcoError::configuration(
                    "Playback started; stop it before changing plan times.",
                ));
            }
            if !restore && !(self.write_allowed_now() && (self.plan_current)().await) {
                return Err(PcoError::configuration(
                    "The scan or Playback changed; preview again.",
                ));
            }
            let current = client
                .get_plan_item(&record.service_type_id, &record.plan_id, &record.item_id)
                .await?;
            let (songs, _) = extract_songs(std::slice::from_ref(&current));
            if restore
                && record.pending_from_length.is_some()
                && current.attributes.length == record.pending_from_length
            {
                // A later uncertain PATCH never committed: restore the previous StagePilot value,
                // not an unrelated remote edit.
                expected = record.pending_from_length.unwrap_or(expected);
                self.update_item(index, |i| i.old_length = Some(expected));
            }
            if songs.is_empty() || current.attributes.length != Some(expected) {
                skip(self);
                return Ok(if restore {
                    Step::Completed
                } else {
                    Step::Skipped
                });
            }
            if (self.is_playing)() {
                return Err(PcoError::configuration(
                    "Playback started; stop it before changing plan times.",
                ));
            }
            if !restore {
                // Record before PATCH; a crash/timeout can leave an uncertain write.
                let prior = previous_records
                    .iter()
                    .find(|r| {
                        (&r.service_type_id, &r.plan_id, &r.item_id)
                            == (&record.service_type_id, &record.plan_id, &record.item_id)
                    })
                    .cloned();
                if let Some(prior) = &prior {
                    if expected != prior.new_length && Some(expected) != prior.pending_from_length {
                        skip(self);
                        return Ok(Step::Skipped);
                    }
                }
                // Coalesce a sequence of explicit updates into one restore point.
                undo = PlanningCenterLengthUndo {
                    old_length: prior.as_ref().map_or(expected, |p| p.old_length),
                    pending_from_length: Some(expected),
                    ..record.clone()
                };
                let mut records: Vec<PlanningCenterLengthUndo> = previous_records
                    .iter()
                    .filter(|r| Some(*r) != prior.as_ref())
                    .cloned()
                    .collect();
                records.push(undo.clone());
                self.persist(records)
                    .map_err(|_| PcoError::Generic("persist".into()))?;
            }
            attempted = true;
            let playing = self.is_playing.clone();
            let allowed = lk(&self.write_allowed).clone();
            let response = client
                .update_plan_item_length(
                    &record.service_type_id,
                    &record.plan_id,
                    &record.item_id,
                    target,
                    Some(expected),
                    &move || !playing() && (restore || allowed()),
                )
                .await?;
            accepted = true;
            tracing::info!(
                item_id = %record.item_id,
                old_length = expected,
                new_length = target,
                "planning_center_item_length_written"
            );
            self.update_item(index, |i| {
                i.status = if restore {
                    ItemStatus::Restored
                } else {
                    ItemStatus::Updated
                };
            });
            if response.attributes.length != Some(target) {
                return Err(PcoError::Unexpected("Unconfirmed write".into()));
            }
            let verified = client
                .get_plan_item(&record.service_type_id, &record.plan_id, &record.item_id)
                .await?;
            if verified.attributes.length != Some(target) {
                return Err(PcoError::Unexpected("Unconfirmed write".into()));
            }
            if !restore {
                let cleared: Vec<PlanningCenterLengthUndo> = self
                    .settings
                    .snapshot()
                    .planning_center_length_undo
                    .into_iter()
                    .map(|r| {
                        if r == undo {
                            PlanningCenterLengthUndo {
                                pending_from_length: None,
                                ..r
                            }
                        } else {
                            r
                        }
                    })
                    .collect();
                self.persist(cleared)
                    .map_err(|_| PcoError::Generic("persist".into()))?;
            }
            Ok(Step::Changed(true))
        }
        .await;
        match outcome {
            Ok(step) => step,
            Err(PcoError::LengthConflict(_)) => {
                skip(self);
                if restore {
                    Step::Completed
                } else {
                    let _ = self.persist(previous_records);
                    Step::Skipped
                }
            }
            Err(error) => {
                let halted = match &error {
                    PcoError::Permission(_) => DENIED_MESSAGE.to_string(),
                    PcoError::RateLimit { .. } => {
                        "Planning Center is rate limiting changes. Stopped after one Retry-After retry."
                            .to_string()
                    }
                    PcoError::Configuration(message) => message.clone(),
                    PcoError::Api { status } => format!(
                        "Planning Center returned HTTP {status}; this change was not confirmed."
                    ),
                    _ => "Planning Center times could not be changed or confirmed. Check the plan before trying again; song order and lengths are saved.".to_string(),
                };
                self.update_item(index, |i| i.reason = Some(halted.clone()));
                // A definite HTTP refusal did not write. Uncertain writes retain undo.
                if !restore
                    && attempted
                    && !accepted
                    && matches!(error, PcoError::Permission(_) | PcoError::RateLimit { .. })
                {
                    let _ = self.persist(previous_records);
                }
                // `accepted` writes that fail verification still count as changed.
                if accepted {
                    return Step::HaltedChanged(halted);
                }
                Step::Halted(halted)
            }
        }
    }

    async fn reload(&self, changed: &[PlanningCenterLengthUndo], restore: bool) {
        let state = self.state.snapshot();
        let loaded: Vec<&PlanningCenterLengthUndo> = changed
            .iter()
            .filter(|r| {
                state.plan.as_ref().is_some_and(|p| {
                    r.plan_id == p.id && Some(&r.service_type_id) == p.service_type_id.as_ref()
                })
            })
            .collect();
        if loaded.is_empty() {
            lk(&self.result).reload = ReloadOutcome::DifferentPlan;
            return;
        }
        if state.timer.status == TimerStatus::Running {
            lk(&self.result).reload = ReloadOutcome::Deferred;
            return;
        }
        let previous_reload = state.last_successful_plan_reload_at;
        let outcome = self
            .dispatcher
            .dispatch(ActionName::ReloadPlan, "planning_center_lengths")
            .await;
        if !outcome.accepted {
            lk(&self.result).reload = ReloadOutcome::Deferred;
            return;
        }
        let waited = tokio::time::timeout(self.reload_timeout, async {
            loop {
                let state = self.state.snapshot();
                if state.last_successful_plan_reload_at != previous_reload {
                    let verified = state.plan.as_ref().is_some_and(|plan| {
                        loaded.iter().all(|r| {
                            plan.id == r.plan_id
                                && plan.service_type_id.as_ref() == Some(&r.service_type_id)
                                && plan
                                    .songs
                                    .iter()
                                    .find(|s| s.id == r.item_id)
                                    .and_then(|s| s.duration_seconds)
                                    .map(i64::from)
                                    == Some(if restore { r.old_length } else { r.new_length })
                        })
                    });
                    return verified;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        lk(&self.result).reload = match waited {
            Ok(true) => ReloadOutcome::Verified,
            _ => ReloadOutcome::Failed,
        };
    }

    fn summarize(&self, halted: Option<&str>) {
        let mut result = lk(&self.result);
        let changed: Vec<&LengthItemResult> = result
            .items
            .iter()
            .filter(|i| matches!(i.status, ItemStatus::Updated | ItemStatus::Restored))
            .collect();
        let unchanged: Vec<&LengthItemResult> = result
            .items
            .iter()
            .filter(|i| i.status == ItemStatus::Skipped)
            .collect();
        let verb = if result.operation == Operation::Restore {
            "Restored"
        } else {
            "Updated"
        };
        let mut parts: Vec<String> = Vec::new();
        if !changed.is_empty() {
            parts.push(format!(
                "{verb} {} songs in Planning Center: {}.",
                changed.len(),
                changed
                    .iter()
                    .map(|i| format!(
                        "{} {} -> {}",
                        i.title,
                        duration(i.old_length),
                        duration(i.new_length)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        } else if halted.is_none() {
            parts.push("No Planning Center times needed changing.".to_string());
        }
        let within: Vec<&&LengthItemResult> = unchanged
            .iter()
            .filter(|i| i.reason.as_deref() == Some("Already the same."))
            .collect();
        if !within.is_empty() {
            parts.push(format!(
                "Left {} songs as they were (already the same).",
                within.len()
            ));
        }
        for item in &result.items {
            if let Some(reason) = &item.reason {
                let is_within = item.status == ItemStatus::Skipped && reason == "Already the same.";
                if !is_within {
                    parts.push(format!("{}: {reason}", item.title));
                }
            }
        }
        if let Some(halted) = halted {
            if !result.items.iter().any(|i| i.status == ItemStatus::Failed) {
                parts.push(halted.to_string());
            }
        }
        match result.reload {
            ReloadOutcome::Verified => {
                parts.push("StagePilot reloaded the plan and verified the song times.".into())
            }
            ReloadOutcome::Deferred => parts.push(
                "Times changed in Planning Center. The timer is running; reload the plan when it is idle."
                    .into(),
            ),
            ReloadOutcome::DifferentPlan => parts.push(format!(
                "Updated {} in Planning Center. StagePilot is showing a different plan, so nothing changed here.",
                lk(&self.target_label)
            )),
            ReloadOutcome::Failed => parts.push(
                "Times changed in Planning Center, but StagePilot has not reloaded and verified them yet."
                    .into(),
            ),
            ReloadOutcome::NotNeeded => {}
        }
        result.message = parts.join(" ");
    }
}

enum Step {
    /// The write landed (restore: whether the undo record can be cleared).
    Changed(bool),
    /// Restore only: nothing to write and the record may be dropped.
    Completed,
    Skipped,
    Halted(String),
    /// A write was accepted by Planning Center but not confirmed: counts as changed and halts.
    HaltedChanged(String),
}
