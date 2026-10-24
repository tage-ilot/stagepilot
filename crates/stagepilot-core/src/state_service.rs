//! Event-driven service plan navigation and state projection: port of
//! `services/state_service.py`. Message strings and event order match the Python service.

use crate::bus::{EventBus, Subscription};
use crate::events::{
    empty_event, new_event, ActionName, EventPayload, EventType, StagePilotEvent, TimerPayload,
};
use crate::state::*;
use crate::store::StateStore;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutcome {
    pub accepted: bool,
    pub message: String,
}

impl ActionOutcome {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            accepted: true,
            message: message.into(),
        }
    }
    pub fn rejected(message: impl Into<String>) -> Self {
        Self {
            accepted: false,
            message: message.into(),
        }
    }
}

/// Leave the playing song as it is (Python `_KEEP`).
#[derive(Clone, Copy)]
enum Playing {
    Keep,
    Set(Option<usize>),
}

struct Inner {
    bus: EventBus,
    store: StateStore,
    recent_event_limit: usize,
    recent_error_limit: usize,
    subscriptions: Mutex<Vec<Subscription>>,
    pending_start_index: Mutex<usize>,
    action_lock: tokio::sync::Mutex<()>,
    action_results: Mutex<HashMap<Uuid, ActionOutcome>>,
    active: AtomicBool,
}

#[derive(Clone)]
pub struct StateService {
    inner: Arc<Inner>,
}

fn keep_last<T>(items: &mut Vec<T>, limit: usize) {
    if items.len() > limit {
        let excess = items.len() - limit;
        items.drain(..excess);
    }
}

impl StateService {
    pub fn new(
        bus: EventBus,
        store: StateStore,
        recent_event_limit: usize,
        recent_error_limit: usize,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                bus,
                store,
                recent_event_limit,
                recent_error_limit,
                subscriptions: Mutex::new(Vec::new()),
                pending_start_index: Mutex::new(0),
                action_lock: tokio::sync::Mutex::new(()),
                action_results: Mutex::new(HashMap::new()),
                active: AtomicBool::new(false),
            }),
        }
    }

    fn pending(&self) -> usize {
        *self
            .inner
            .pending_start_index
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    fn set_pending(&self, value: usize) {
        *self
            .inner
            .pending_start_index
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = value;
    }

    pub fn start(&self) {
        type Method = fn(
            StateService,
            Arc<StagePilotEvent>,
        ) -> futures_util::future::BoxFuture<'static, Result<(), String>>;
        macro_rules! h {
            ($m:ident) => {{
                let f: Method = |svc, event| Box::pin(async move { svc.$m(&event).await });
                f
            }};
        }
        let handlers: Vec<(Option<EventType>, Method)> = vec![
            (Some(EventType::ActionRequested), h!(handle_action)),
            (
                Some(EventType::ServiceLoadChanged),
                h!(handle_service_load_changed),
            ),
            (Some(EventType::ServiceLoaded), h!(handle_service_loaded)),
            (Some(EventType::TimerStarted), h!(handle_timer_started)),
            (Some(EventType::TimerStopped), h!(handle_timer_stopped)),
            (Some(EventType::TimerFailed), h!(handle_timer_failed)),
            (
                Some(EventType::ConnectionChanged),
                h!(handle_connection_changed),
            ),
            (
                Some(EventType::PluginStatusChanged),
                h!(handle_plugin_status),
            ),
            (Some(EventType::PluginFailed), h!(handle_plugin_status)),
            (
                Some(EventType::ApplicationStarted),
                h!(handle_application_lifecycle),
            ),
            (
                Some(EventType::ApplicationStopping),
                h!(handle_application_lifecycle),
            ),
        ];
        // Recent events are recorded synchronously, in publication order, by a bus tap (the
        // Python wildcard subscriber ran as a separate task and could reorder nested events).
        self.inner.active.store(true, Ordering::SeqCst);
        let svc = self.clone();
        self.inner.bus.add_tap(move |event| {
            if svc.inner.active.load(Ordering::SeqCst) {
                svc.record_event(event);
            }
        });
        let mut subs = self
            .inner
            .subscriptions
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        for (event_type, method) in handlers {
            let svc = self.clone();
            subs.push(
                self.inner
                    .bus
                    .subscribe(event_type, move |event| method(svc.clone(), event)),
            );
        }
    }

    pub fn stop(&self) {
        self.inner.active.store(false, Ordering::SeqCst);
        let mut subs = self
            .inner
            .subscriptions
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        for sub in subs.drain(..) {
            self.inner.bus.unsubscribe(&sub);
        }
    }

    /// Forget the loaded service (Services source switched live).
    pub async fn clear_service(&self) {
        let _guard = self.inner.action_lock.lock().await;
        self.set_pending(0);
        self.inner.store.mutate(|s| {
            s.plan = None;
            s.current_song = None;
            s.next_song = None;
            s.current_song_index = None;
            s.playing_song = None;
            s.playing_song_index = None;
            s.up_next_song = None;
            s.service_load = ServiceLoadState::default();
            s.timer = TimerState::default();
            s.last_successful_plan_reload_at = None;
            s.last_action = None;
        });
    }

    /// Forget the countdown (Presentation output switched live).
    pub fn clear_timer(&self) {
        self.inner.store.mutate(|s| s.timer = TimerState::default());
    }

    pub async fn dispatch(&self, action: ActionName, source: &str) -> ActionOutcome {
        let request_id = Uuid::new_v4();
        let report = self
            .inner
            .bus
            .publish(new_event(
                EventType::ActionRequested,
                source,
                EventPayload::Action { action, request_id },
            ))
            .await;
        let outcome = self
            .inner
            .action_results
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&request_id);
        if let Some(outcome) = outcome {
            return outcome;
        }
        if !report.failures.is_empty() {
            return ActionOutcome::rejected("The action handler failed; see recent errors.");
        }
        ActionOutcome::rejected("No service is available to handle this action.")
    }

    /// Start an exact one-based song position without changing manual-control semantics.
    pub async fn dispatch_song_position(&self, position: i64, source: &str) -> ActionOutcome {
        let _guard = self.inner.action_lock.lock().await;
        self.start_song_position(position, source).await
    }

    pub async fn resume_timer_at(&self, position_seconds: f64, source: &str) -> ActionOutcome {
        let _guard = self.inner.action_lock.lock().await;
        self.apply_position(position_seconds, source, true).await
    }

    pub async fn seek_timer(&self, position_seconds: f64, source: &str) -> ActionOutcome {
        let _guard = self.inner.action_lock.lock().await;
        self.apply_position(position_seconds, source, false).await
    }

    /// Stop the timer, freezing remaining time from an explicit position.
    pub async fn stop_timer_at(&self, position_seconds: f64, source: &str) -> ActionOutcome {
        let _guard = self.inner.action_lock.lock().await;
        let position = match clean_position(position_seconds) {
            Ok(p) => p,
            Err(message) => return ActionOutcome::rejected(message),
        };
        let state = self.inner.store.snapshot();
        let song = match state.current_song {
            Some(song) if song.duration_seconds.unwrap_or(0) > 0 => song,
            _ => return self.stop_timer().await,
        };
        let remaining = remaining(song.duration_seconds.unwrap_or(0), position);
        self.inner.store.mutate(|s| {
            s.timer = TimerState {
                status: TimerStatus::Stopped,
                duration_seconds: s.timer.duration_seconds,
                position_seconds: Some(position),
                remaining_seconds: Some(remaining),
                ..Default::default()
            };
            s.last_action = Some(ActionName::StopTimer.as_str().into());
        });
        self.inner
            .bus
            .publish(new_event(
                EventType::TimerStopRequested,
                source,
                EventPayload::Timer(TimerPayload {
                    duration_seconds: remaining,
                    song: Some(song),
                    position_seconds: Some(position),
                    remaining_seconds: Some(remaining),
                    ..Default::default()
                }),
            ))
            .await;
        ActionOutcome::ok("Timer stop requested.")
    }

    async fn apply_position(
        &self,
        position_seconds: f64,
        source: &str,
        resume: bool,
    ) -> ActionOutcome {
        let position = match clean_position(position_seconds) {
            Ok(p) => p,
            Err(message) => return ActionOutcome::rejected(message),
        };
        let state = self.inner.store.snapshot();
        let Some(song) = state.current_song.clone() else {
            return ActionOutcome::rejected("No current song.");
        };
        let duration = song.duration_seconds.unwrap_or(0);
        if duration == 0 {
            return ActionOutcome::rejected(format!(
                "Cannot time \"{}\": scheduled duration is missing or zero.",
                song.title
            ));
        }
        if state.timer.status == TimerStatus::Error {
            return ActionOutcome::rejected("The timer is in an error state.");
        }
        let was_running = state.timer.status == TimerStatus::Running;
        let remaining = remaining(duration, position);
        let running = remaining > 0 && (resume || was_running);
        let now = Utc::now();
        self.inner.store.mutate(|s| {
            s.timer = if running {
                TimerState {
                    status: TimerStatus::Running,
                    duration_seconds: Some(remaining),
                    started_at: Some(now),
                    position_seconds: Some(position),
                    ..Default::default()
                }
            } else {
                TimerState {
                    status: TimerStatus::Stopped,
                    duration_seconds: s.timer.duration_seconds,
                    position_seconds: Some(position),
                    remaining_seconds: Some(remaining),
                    ..Default::default()
                }
            };
        });
        let payload = TimerPayload {
            duration_seconds: remaining,
            song: Some(song.clone()),
            started_at: running.then_some(now),
            position_seconds: Some(position),
            remaining_seconds: Some(remaining),
            running,
            ..Default::default()
        };
        let event_type = if resume {
            EventType::TimerResumed
        } else {
            EventType::TimerSeeked
        };
        self.inner
            .bus
            .publish(new_event(
                event_type,
                source,
                EventPayload::Timer(payload.clone()),
            ))
            .await;
        if resume && !running {
            // Nothing left to count: leave every consumer stopped.
            self.inner
                .bus
                .publish(new_event(
                    EventType::TimerStopRequested,
                    source,
                    EventPayload::Timer(payload),
                ))
                .await;
        }
        let verb = if resume { "Resumed" } else { "Seeked" };
        ActionOutcome::ok(format!("{verb} \"{}\" at {position:.1}s.", song.title))
    }

    async fn handle_action(&self, event: &StagePilotEvent) -> Result<(), String> {
        let EventPayload::Action { action, request_id } = &event.payload else {
            return Ok(());
        };
        let _guard = self.inner.action_lock.lock().await;
        let outcome = if event.source == "planning_center_lengths"
            && *action == ActionName::ReloadPlan
            && self.inner.store.snapshot().timer.status == TimerStatus::Running
        {
            ActionOutcome::rejected("Reload the plan when the timer is idle.")
        } else {
            match action {
                ActionName::StartNext => self.start_next().await,
                ActionName::RestartCurrent => self.restart_current().await,
                ActionName::Previous => self.previous().await,
                ActionName::Next => self.next().await,
                ActionName::StopTimer => self.stop_timer().await,
                ActionName::ReloadPlan => self.reload_plan().await,
                ActionName::ResetPosition => self.reset_position().await,
            }
        };
        self.inner
            .action_results
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(*request_id, outcome);
        Ok(())
    }

    async fn start_next(&self) -> ActionOutcome {
        let state = self.inner.store.snapshot();
        let songs = state
            .plan
            .as_ref()
            .map(|p| p.songs.clone())
            .unwrap_or_default();
        if songs.is_empty() {
            return ActionOutcome::rejected("No songs are loaded.");
        }
        let target = self.pending();
        if target >= songs.len() {
            return ActionOutcome::rejected("Already at the final song.");
        }
        let song = songs[target].clone();
        self.select_song(target, ActionName::StartNext).await;
        if song.duration_seconds.unwrap_or(0) == 0 {
            self.set_pending(target);
            self.set_labels(Playing::Keep);
            let message = format!(
                "Cannot start \"{}\": scheduled duration is missing or zero.",
                song.title
            );
            self.set_timer_error(&message, None);
            return ActionOutcome::rejected(message);
        }
        self.set_pending(target + 1);
        self.set_labels(Playing::Set(Some(target)));
        self.inner
            .bus
            .publish(new_event(
                EventType::SongStarted,
                "state_service",
                EventPayload::Song {
                    song: song.clone(),
                    index: target,
                },
            ))
            .await;
        ActionOutcome::ok(format!("Started \"{}\".", song.title))
    }

    async fn start_song_position(&self, position: i64, source: &str) -> ActionOutcome {
        let state = self.inner.store.snapshot();
        let songs = state
            .plan
            .as_ref()
            .map(|p| p.songs.clone())
            .unwrap_or_default();
        if songs.is_empty() {
            return ActionOutcome::rejected("No songs are loaded.");
        }
        if position < 1 || position as usize > songs.len() {
            return ActionOutcome::rejected(format!(
                "Song velocity {position} does not match a loaded song (1-{}).",
                songs.len()
            ));
        }
        let target = position as usize - 1;
        let song = songs[target].clone();
        self.select_song(target, ActionName::StartNext).await;
        if song.duration_seconds.unwrap_or(0) == 0 {
            self.set_pending(target);
            self.set_labels(Playing::Keep);
            let message = format!(
                "Cannot start \"{}\": scheduled duration is missing or zero.",
                song.title
            );
            self.set_timer_error(&message, None);
            return ActionOutcome::rejected(message);
        }
        self.set_pending(target + 1);
        self.set_labels(Playing::Set(Some(target)));
        self.inner
            .bus
            .publish(new_event(
                EventType::SongStarted,
                source,
                EventPayload::Song {
                    song: song.clone(),
                    index: target,
                },
            ))
            .await;
        ActionOutcome::ok(format!("Started song {position}, \"{}\".", song.title))
    }

    async fn restart_current(&self) -> ActionOutcome {
        let state = self.inner.store.snapshot();
        let (Some(song), Some(index)) = (state.current_song, state.current_song_index) else {
            return ActionOutcome::rejected("No current song to restart.");
        };
        if song.duration_seconds.unwrap_or(0) == 0 {
            let message = format!(
                "Cannot restart \"{}\": duration is missing or zero.",
                song.title
            );
            self.set_timer_error(&message, None);
            return ActionOutcome::rejected(message);
        }
        self.set_last_action(ActionName::RestartCurrent);
        self.set_labels(Playing::Set(Some(index)));
        self.inner
            .bus
            .publish(new_event(
                EventType::SongRestarted,
                "state_service",
                EventPayload::Song {
                    song: song.clone(),
                    index,
                },
            ))
            .await;
        ActionOutcome::ok(format!("Restarted \"{}\".", song.title))
    }

    fn up_next_message(title: &str) -> String {
        format!("\"{title}\" is up next. Press Start next to play it.")
    }

    async fn previous(&self) -> ActionOutcome {
        let state = self.inner.store.snapshot();
        let Some(plan) = state.plan.as_ref().filter(|p| !p.songs.is_empty()) else {
            return ActionOutcome::rejected("No songs are loaded.");
        };
        let Some(current) = state.current_song_index.filter(|i| *i != 0) else {
            return ActionOutcome::rejected("Already at the first song.");
        };
        let target = current - 1;
        self.set_pending(target);
        self.select_song(target, ActionName::Previous).await;
        self.set_labels(Playing::Keep); // the playing song is untouched; only Up next moves
        ActionOutcome::ok(Self::up_next_message(&plan.songs[target].title))
    }

    async fn next(&self) -> ActionOutcome {
        let state = self.inner.store.snapshot();
        let Some(plan) = state.plan.as_ref().filter(|p| !p.songs.is_empty()) else {
            return ActionOutcome::rejected("No songs are loaded.");
        };
        let target = state.current_song_index.map_or(0, |i| i + 1);
        if target >= plan.songs.len() {
            return ActionOutcome::rejected("Already at the final song.");
        }
        self.set_pending(target);
        self.select_song(target, ActionName::Next).await;
        self.set_labels(Playing::Keep);
        ActionOutcome::ok(Self::up_next_message(&plan.songs[target].title))
    }

    async fn stop_timer(&self) -> ActionOutcome {
        self.set_last_action(ActionName::StopTimer);
        self.inner
            .bus
            .publish(empty_event(EventType::TimerStopRequested, "state_service"))
            .await;
        ActionOutcome::ok("Timer stop requested.")
    }

    async fn reload_plan(&self) -> ActionOutcome {
        self.set_last_action(ActionName::ReloadPlan);
        self.inner
            .bus
            .publish(empty_event(
                EventType::ServiceReloadRequested,
                "state_service",
            ))
            .await;
        ActionOutcome::ok("Service plan reload requested.")
    }

    async fn reset_position(&self) -> ActionOutcome {
        self.set_pending(0);
        self.inner
            .bus
            .publish(empty_event(EventType::TimerResetRequested, "state_service"))
            .await;
        self.inner.store.mutate(|s| {
            s.current_song = None;
            s.current_song_index = None;
            s.next_song = s.plan.as_ref().and_then(|p| p.songs.first().cloned());
            s.timer = TimerState {
                status: TimerStatus::Stopped,
                ..Default::default()
            };
            s.last_action = Some(ActionName::ResetPosition.as_str().into());
        });
        self.set_labels(Playing::Set(None));
        ActionOutcome::ok("Service position and timer reset.")
    }

    /// Derive the on-screen labels: the song really playing and the song Start next will play.
    fn set_labels(&self, playing: Playing) {
        let pending = self.pending();
        self.inner.store.mutate(|s| {
            let songs: Vec<Song> = s.plan.as_ref().map(|p| p.songs.clone()).unwrap_or_default();
            let mut index = match playing {
                Playing::Keep => s.playing_song_index,
                Playing::Set(i) => i,
            };
            if index.is_some_and(|i| i >= songs.len()) {
                index = None;
            }
            s.playing_song_index = index;
            s.playing_song = index.map(|i| songs[i].clone());
            s.up_next_song = songs.get(pending).cloned();
        });
    }

    async fn select_song(&self, index: usize, action: ActionName) {
        let snapshot = self.inner.store.mutate(|s| {
            let Some(plan) = s.plan.as_ref() else {
                return;
            };
            let Some(song) = plan.songs.get(index).cloned() else {
                return;
            };
            let next = plan.songs.get(index + 1).cloned();
            if s.timer.status == TimerStatus::Stopped && Some(index) != s.current_song_index {
                // A frozen remaining time belongs to the previous song.
                s.timer = TimerState {
                    status: TimerStatus::Stopped,
                    ..Default::default()
                };
            }
            s.current_song_index = Some(index);
            s.current_song = Some(song);
            s.next_song = next;
            s.last_action = Some(action.as_str().into());
        });
        if let Some(song) = snapshot.current_song {
            self.inner
                .bus
                .publish(new_event(
                    EventType::SongSelected,
                    "state_service",
                    EventPayload::Song { song, index },
                ))
                .await;
        }
    }

    async fn handle_service_loaded(&self, event: &StagePilotEvent) -> Result<(), String> {
        let EventPayload::Service { plan: incoming } = &event.payload else {
            return Ok(());
        };
        let previous = self.inner.store.snapshot();
        let preserved = safe_match(
            previous.plan.as_ref(),
            previous.current_song_index,
            incoming,
        );
        let playing = safe_match(
            previous.plan.as_ref(),
            previous.playing_song_index,
            incoming,
        );
        // Start next keeps playing the same song across a reload: follow the pending target by
        // id (never by position), falling back to "after the cursor" when it cannot be found.
        let pending_now = self.pending();
        let old_len = previous.plan.as_ref().map_or(0, |p| p.songs.len());
        let pending_match = if pending_now < old_len {
            safe_match(previous.plan.as_ref(), Some(pending_now), incoming)
        } else {
            None // past the final song: nothing to follow
        };
        match (preserved, pending_match) {
            (None, _) => self.set_pending(0),
            (Some(_), Some(p)) => self.set_pending(p),
            (Some(i), None) => self.set_pending(i + 1),
        }
        let incoming = incoming.clone();
        self.inner.store.mutate(|s| {
            s.last_successful_plan_reload_at = Some(Utc::now());
            match preserved {
                None => {
                    s.current_song = None;
                    s.current_song_index = None;
                    s.next_song = incoming.songs.first().cloned();
                }
                Some(i) => {
                    s.current_song_index = Some(i);
                    s.current_song = incoming.songs.get(i).cloned();
                    s.next_song = incoming.songs.get(i + 1).cloned();
                }
            }
            s.plan = Some(incoming);
        });
        self.set_labels(Playing::Set(playing));
        Ok(())
    }

    async fn handle_service_load_changed(&self, event: &StagePilotEvent) -> Result<(), String> {
        let EventPayload::ServiceLoad {
            status,
            target_date,
            candidates,
            skipped_items,
            message,
            is_stale,
        } = &event.payload
        else {
            return Ok(());
        };
        let before = self.inner.store.snapshot();
        let date_rollover = match (&before.plan, target_date) {
            (Some(plan), Some(target)) => plan.date != *target,
            _ => false,
        };
        if date_rollover && before.timer.status == TimerStatus::Running {
            self.inner
                .bus
                .publish(empty_event(EventType::TimerStopRequested, "state_service"))
                .await;
        }
        let load = ServiceLoadState {
            status: *status,
            target_date: *target_date,
            candidates: candidates.clone(),
            skipped_items: skipped_items.clone(),
            message: message.clone(),
            is_stale: *is_stale,
            last_attempt_at: Some(event.timestamp),
        };
        let invalidated_type = *status == ServiceLoadStatus::NotFound && !*is_stale;
        let mut reset_pending = false;
        self.inner.store.mutate(|s| {
            let plan_invalid = s.plan.is_some()
                && (matches!((&s.plan, target_date), (Some(p), Some(t)) if p.date != *t)
                    || invalidated_type);
            if plan_invalid {
                s.plan = None;
                s.current_song = None;
                s.next_song = None;
                s.current_song_index = None;
                s.playing_song = None;
                s.playing_song_index = None;
                s.up_next_song = None;
                s.timer = TimerState {
                    status: TimerStatus::Stopped,
                    ..Default::default()
                };
                reset_pending = true;
            }
            s.service_load = load;
        });
        if reset_pending {
            self.set_pending(0);
        }
        Ok(())
    }

    async fn handle_timer_started(&self, event: &StagePilotEvent) -> Result<(), String> {
        let EventPayload::Timer(payload) = &event.payload else {
            return Ok(());
        };
        self.inner.store.mutate(|s| {
            s.timer = TimerState {
                status: TimerStatus::Running,
                duration_seconds: Some(payload.duration_seconds),
                started_at: Some(payload.started_at.unwrap_or(event.timestamp)),
                position_seconds: Some(payload.position_seconds.unwrap_or(0.0)),
                ..Default::default()
            };
        });
        Ok(())
    }

    async fn handle_timer_stopped(&self, event: &StagePilotEvent) -> Result<(), String> {
        let payload = match &event.payload {
            EventPayload::Timer(p) => Some(p),
            _ => None,
        };
        self.inner.store.mutate(|s| {
            let previous = s.timer.clone();
            let mut position = payload.and_then(|p| p.position_seconds);
            let mut remaining = payload.and_then(|p| p.remaining_seconds);
            if position.is_none() && remaining.is_none() {
                if previous.status == TimerStatus::Running && previous.started_at.is_some() {
                    // Freeze where the countdown stood when it was stopped.
                    let started: DateTime<Utc> = previous.started_at.unwrap_or(event.timestamp);
                    let elapsed =
                        ((event.timestamp - started).num_milliseconds() as f64 / 1000.0).max(0.0);
                    let total = previous.duration_seconds.unwrap_or(0);
                    remaining = Some(((total as f64 - elapsed).round().max(0.0)) as u32);
                    position =
                        Some(previous.position_seconds.unwrap_or(0.0) + elapsed.min(total as f64));
                } else if previous.status == TimerStatus::Stopped {
                    // Already frozen (stop_timer_at, or a repeated stop): keep it.
                    position = previous.position_seconds;
                    remaining = previous.remaining_seconds;
                }
            }
            s.timer = TimerState {
                status: TimerStatus::Stopped,
                duration_seconds: previous.duration_seconds,
                position_seconds: position,
                remaining_seconds: remaining,
                ..Default::default()
            };
        });
        Ok(())
    }

    async fn handle_timer_failed(&self, event: &StagePilotEvent) -> Result<(), String> {
        let EventPayload::Timer(payload) = &event.payload else {
            return Ok(());
        };
        let message = payload
            .message
            .clone()
            .unwrap_or_else(|| "Timer operation failed.".to_string());
        self.set_timer_error(&message, Some(event.id));
        Ok(())
    }

    fn set_timer_error(&self, message: &str, event_id: Option<Uuid>) {
        let limit = self.inner.recent_error_limit;
        self.inner.store.mutate(|s| {
            s.timer = TimerState {
                status: TimerStatus::Error,
                last_error: Some(message.to_string()),
                ..Default::default()
            };
            s.recent_errors.push(ErrorSummary {
                timestamp: Utc::now(),
                component: "timer".into(),
                message: message.to_string(),
                event_id,
            });
            keep_last(&mut s.recent_errors, limit);
        });
    }

    async fn handle_connection_changed(&self, event: &StagePilotEvent) -> Result<(), String> {
        let EventPayload::Connection {
            integration,
            status,
            detail,
        } = &event.payload
        else {
            return Ok(());
        };
        // Simulation can exercise domain behaviour, but it is never evidence of a verified
        // external connection.
        if event.source == "demo" {
            return Ok(());
        }
        let limit = self.inner.recent_error_limit;
        self.inner.store.mutate(|s| {
            match integration {
                crate::events::Integration::PlanningCenter => s.planning_center_status = *status,
                crate::events::Integration::Midi => s.midi_status = *status,
                crate::events::Integration::Propresenter => s.propresenter_status = *status,
                crate::events::Integration::Lights => s.lights_status = *status,
            }
            if *status == ConnectionStatus::Error {
                if let Some(detail) = detail.as_ref().filter(|d| !d.is_empty()) {
                    s.recent_errors.push(ErrorSummary {
                        timestamp: Utc::now(),
                        component: integration.as_str().into(),
                        message: detail.clone(),
                        event_id: Some(event.id),
                    });
                    keep_last(&mut s.recent_errors, limit);
                }
            }
        });
        Ok(())
    }

    async fn handle_plugin_status(&self, event: &StagePilotEvent) -> Result<(), String> {
        let EventPayload::Plugin {
            name,
            version,
            status,
            error,
        } = &event.payload
        else {
            return Ok(());
        };
        let limit = self.inner.recent_error_limit;
        self.inner.store.mutate(|s| {
            s.plugins.insert(
                name.clone(),
                PluginHealth {
                    name: name.clone(),
                    version: version.clone(),
                    status: *status,
                    last_error: error.clone(),
                    last_activity_at: Some(event.timestamp),
                },
            );
            if event.event_type == EventType::PluginFailed {
                // A plugin that reports its own connection error right before the manager
                // reports the failed start would otherwise be listed twice (m16).
                let repeats_last = |s: &ApplicationState, error: &str| {
                    s.recent_errors
                        .last()
                        .is_some_and(|last| last.component == *name && last.message == error)
                };
                if let Some(error) = error
                    .as_ref()
                    .filter(|e| !e.is_empty() && !repeats_last(s, e))
                {
                    s.recent_errors.push(ErrorSummary {
                        timestamp: Utc::now(),
                        component: name.clone(),
                        message: error.clone(),
                        event_id: Some(event.id),
                    });
                    keep_last(&mut s.recent_errors, limit);
                }
            }
        });
        Ok(())
    }

    async fn handle_application_lifecycle(&self, event: &StagePilotEvent) -> Result<(), String> {
        let status = if event.event_type == EventType::ApplicationStarted {
            ApplicationStatus::Running
        } else {
            ApplicationStatus::Stopping
        };
        self.inner.store.mutate(|s| s.application_status = status);
        Ok(())
    }

    fn set_last_action(&self, action: ActionName) {
        self.inner
            .store
            .mutate(|s| s.last_action = Some(action.as_str().into()));
    }

    fn record_event(&self, event: &StagePilotEvent) {
        let limit = self.inner.recent_event_limit;
        self.inner.store.mutate(|s| {
            s.recent_events.push(EventSummary {
                id: event.id,
                event_type: event.event_type.as_str(),
                timestamp: event.timestamp,
                source: event.source.clone(),
            });
            keep_last(&mut s.recent_events, limit);
        });
    }
}

fn clean_position(position_seconds: f64) -> Result<f64, String> {
    if !position_seconds.is_finite() {
        return Err("Position must be a finite number of seconds.".to_string());
    }
    Ok(position_seconds.max(0.0))
}

/// Python `max(0, round(duration - position))`; `round` there is banker's rounding.
fn remaining(duration: u32, position: f64) -> u32 {
    let value = duration as f64 - position;
    if value <= 0.0 {
        return 0;
    }
    round_half_even(value) as u32
}

fn round_half_even(value: f64) -> f64 {
    let floor = value.floor();
    let diff = value - floor;
    if (diff - 0.5).abs() < f64::EPSILON {
        if floor % 2.0 == 0.0 {
            floor
        } else {
            floor + 1.0
        }
    } else {
        value.round()
    }
}

/// Find the song at `previous_index` of the previous plan in the incoming plan: by unique id,
/// else by unique title.
fn safe_match(
    previous_plan: Option<&ServicePlan>,
    previous_index: Option<usize>,
    incoming: &ServicePlan,
) -> Option<usize> {
    let (plan, index) = (previous_plan?, previous_index?);
    let previous_song = plan.songs.get(index)?;
    let id_matches: Vec<usize> = incoming
        .songs
        .iter()
        .enumerate()
        .filter(|(_, s)| s.id == previous_song.id)
        .map(|(i, _)| i)
        .collect();
    if id_matches.len() == 1 {
        return Some(id_matches[0]);
    }
    let old_title_count = plan
        .songs
        .iter()
        .filter(|s| s.title == previous_song.title)
        .count();
    let title_matches: Vec<usize> = incoming
        .songs
        .iter()
        .enumerate()
        .filter(|(_, s)| s.title == previous_song.title)
        .map(|(i, _)| i)
        .collect();
    if old_title_count == 1 && title_matches.len() == 1 {
        return Some(title_matches[0]);
    }
    None
}
