//! Connection-only ownership and bounded cross-source action deduplication:
//! mirrors `playback_api/arbiter.py` exactly (same thresholds: 5 s heartbeat freshness,
//! 3 s stability before failback, 1.5 s gap that resets stability, 2 s duplicate window).
//!
//! The clock is injected (`Clock`), so tests drive time by hand.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

/// Seconds since the first call, from a monotonic clock (Python `time.monotonic`).
pub fn monotonic_clock() -> Clock {
    let start = Instant::now();
    Arc::new(move || start.elapsed().as_secs_f64())
}

/// A hand-driven clock for tests.
#[derive(Clone, Default)]
pub struct ManualClock(Arc<Mutex<f64>>);

impl ManualClock {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set(&self, v: f64) {
        *self.0.lock().unwrap() = v;
    }
    pub fn advance(&self, by: f64) {
        *self.0.lock().unwrap() += by;
    }
    pub fn clock(&self) -> Clock {
        let c = self.0.clone();
        Arc::new(move || *c.lock().unwrap())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    PlaybackApi,
    Midi,
    None,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::PlaybackApi => "playback_api",
            Source::Midi => "midi",
            Source::None => "none",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStatus {
    pub connected: bool,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackConnection {
    pub active_source: Source,
    pub connected: bool,
    pub reason: &'static str,
    pub playback_api: SourceStatus,
    pub midi: SourceStatus,
}

/// Action identity for deduplication: ("start", Some(position)), ("stop_timer", None), ...
pub type ActionKey = (&'static str, Option<i64>);

pub struct PlaybackSourceArbiter {
    clock: Clock,
    pub websocket: bool,
    pub heartbeat_at: Option<f64>,
    pub stable_since: Option<f64>,
    pub midi_connected: bool,
    /// MIDI may own only when the user chose MIDI; API mode never falls back to it.
    pub midi_allowed: bool,
    pub owner: Source,
    pub revision: u64,
    recent: HashMap<ActionKey, (Source, f64)>,
}

impl PlaybackSourceArbiter {
    pub fn new(clock: Clock) -> Self {
        PlaybackSourceArbiter {
            clock,
            websocket: false,
            heartbeat_at: None,
            stable_since: None,
            midi_connected: false,
            midi_allowed: true,
            owner: Source::None,
            revision: 0,
            recent: HashMap::new(),
        }
    }

    pub fn now(&self) -> f64 {
        (self.clock)()
    }

    pub fn observe_api(&mut self, connected: bool, fresh: bool) {
        let now = self.now();
        if !connected || self.heartbeat_at.is_some_and(|h| now - h >= 5.0) {
            self.stable_since = None;
            self.heartbeat_at = None;
        }
        self.websocket = connected;
        if connected && fresh {
            if self.stable_since.is_none() || self.heartbeat_at.is_some_and(|h| now - h > 1.5) {
                self.stable_since = Some(now);
            }
            self.heartbeat_at = Some(now);
        }
        self.snapshot();
    }

    pub fn snapshot(&mut self) -> PlaybackConnection {
        let now = self.now();
        let api = self.websocket && self.heartbeat_at.is_some_and(|h| now - h < 5.0);
        if !api {
            self.stable_since = None;
        }
        let mut owner = if self.midi_connected && self.midi_allowed {
            Source::Midi
        } else {
            Source::None
        };
        if api
            && (self.owner == Source::PlaybackApi
                || !self.midi_connected
                || (self.stable_since.is_some_and(|s| now - s >= 3.0)
                    && self.heartbeat_at.is_some_and(|h| now - h <= 1.5)))
        {
            owner = Source::PlaybackApi;
        }
        if owner != self.owner {
            self.owner = owner;
            self.revision += 1;
        }
        PlaybackConnection {
            active_source: owner,
            connected: owner != Source::None,
            reason: match owner {
                Source::PlaybackApi => "Connected to Playback.",
                Source::Midi => "Connected through MIDI.",
                Source::None => "Disconnected.",
            },
            playback_api: SourceStatus {
                connected: api,
                reason: if api {
                    "Playback is connected."
                } else {
                    "Playback is disconnected."
                },
            },
            midi: SourceStatus {
                connected: self.midi_connected,
                reason: if self.midi_connected {
                    "MIDI is connected."
                } else {
                    "MIDI is disconnected."
                },
            },
        }
    }

    pub fn allows(&mut self, source: Source) -> bool {
        self.snapshot().active_source == source
    }

    /// True when a *different* source did the same thing within the last 2 s.
    pub fn duplicate(&mut self, source: Source, key: ActionKey) -> bool {
        let now = self.now();
        self.recent.retain(|_, v| now - v.1 < 2.0);
        self.recent.get(&key).is_some_and(|p| p.0 != source)
    }

    pub fn remember(&mut self, source: Source, key: ActionKey) {
        let now = self.now();
        self.recent.insert(key, (source, now));
    }

    pub fn forget(&mut self, key: &ActionKey) {
        self.recent.remove(key);
    }

    /// True when any other source recently started a song (any "start" key).
    pub fn other_source_started(&self, origin: Source) -> bool {
        self.recent
            .iter()
            .any(|(k, (s, _))| *s != origin && k.0 == "start")
    }
}

// ---------------------------------------------------------------------------------------------
// ArbitratedDispatcher
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionName {
    StartNext,
    RestartCurrent,
    Previous,
    Next,
    StopTimer,
    ReloadPlan,
    ResetPosition,
}

impl ActionName {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionName::StartNext => "start_next",
            ActionName::RestartCurrent => "restart_current",
            ActionName::Previous => "previous",
            ActionName::Next => "next",
            ActionName::StopTimer => "stop_timer",
            ActionName::ReloadPlan => "reload_plan",
            ActionName::ResetPosition => "reset_position",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutcome {
    pub accepted: bool,
    pub message: String,
}

impl ActionOutcome {
    pub fn new(accepted: bool, message: impl Into<String>) -> Self {
        ActionOutcome {
            accepted,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimerCall {
    Resume(f64),
    Seek(f64),
    Stop(f64),
}

/// The downstream StagePilot dispatcher (the Python `ActionDispatcher` + `TimerPositionDispatcher`).
pub trait ActionDispatcher: Send + Sync {
    fn dispatch(
        &self,
        action: ActionName,
        source: &str,
    ) -> impl std::future::Future<Output = ActionOutcome> + Send;
    fn dispatch_song_position(
        &self,
        position: i64,
        source: &str,
    ) -> impl std::future::Future<Output = ActionOutcome> + Send;
    fn timer(
        &self,
        call: TimerCall,
        source: &str,
    ) -> impl std::future::Future<Output = ActionOutcome> + Send;
}

/// What the dispatcher needs from the state store: the current song index.
pub trait StateView: Send + Sync {
    fn current_song_index(&self) -> impl std::future::Future<Output = Option<i64>> + Send;
}

pub struct ArbitratedDispatcher<D: ActionDispatcher, S: StateView> {
    pub arbiter: Arc<Mutex<PlaybackSourceArbiter>>,
    dispatcher: D,
    state: S,
    pub suppressed: Box<dyn Fn() -> bool + Send + Sync>,
    pub api_mapping_revision: Box<dyn Fn() -> i64 + Send + Sync>,
}

impl<D: ActionDispatcher, S: StateView> ArbitratedDispatcher<D, S> {
    pub fn new(arbiter: Arc<Mutex<PlaybackSourceArbiter>>, dispatcher: D, state: S) -> Self {
        ArbitratedDispatcher {
            arbiter,
            dispatcher,
            state,
            suppressed: Box::new(|| false),
            api_mapping_revision: Box::new(|| 0),
        }
    }

    fn arb<R>(&self, f: impl FnOnce(&mut PlaybackSourceArbiter) -> R) -> R {
        f(&mut self.arbiter.lock().unwrap())
    }

    pub async fn dispatch(&self, action: ActionName, source: &str) -> ActionOutcome {
        let revision = self.arb(|a| {
            a.snapshot();
            a.revision
        });
        let mapping_revision = (self.api_mapping_revision)();
        let index = self.state.current_song_index().await;
        let changed = self.arb(|a| {
            a.snapshot();
            revision != a.revision
        });
        if changed {
            return ActionOutcome::new(false, "ignored: Playback source ownership changed");
        }
        if source == "playback_api"
            && matches!(action, ActionName::StartNext | ActionName::RestartCurrent)
            && mapping_revision != (self.api_mapping_revision)()
        {
            return ActionOutcome::new(false, "ignored: Playback song order changed");
        }
        let mut position = index.map(|i| i + 1);
        if action == ActionName::StartNext {
            position = Some(index.map_or(1, |i| i + 2));
        }
        let key: ActionKey = if matches!(action, ActionName::StartNext | ActionName::RestartCurrent)
        {
            ("start", position)
        } else {
            (action.as_str(), None)
        };
        self.run(source, key, Step::Action(action)).await
    }

    pub async fn dispatch_song_position(&self, position: i64, source: &str) -> ActionOutcome {
        self.run(source, ("start", Some(position)), Step::Position(position))
            .await
    }

    pub async fn resume_timer_at(&self, seconds: f64, source: &str) -> ActionOutcome {
        self.run(
            source,
            ("resume", None),
            Step::Timer(TimerCall::Resume(seconds)),
        )
        .await
    }

    pub async fn seek_timer(&self, seconds: f64, source: &str) -> ActionOutcome {
        self.run(
            source,
            ("seek", None),
            Step::Timer(TimerCall::Seek(seconds)),
        )
        .await
    }

    pub async fn stop_timer_at(&self, seconds: f64, source: &str) -> ActionOutcome {
        self.run(
            source,
            ("stop_timer", None),
            Step::Timer(TimerCall::Stop(seconds)),
        )
        .await
    }

    async fn forward(&self, step: &Step, source: &str) -> ActionOutcome {
        match step {
            Step::Action(a) => self.dispatcher.dispatch(*a, source).await,
            Step::Position(p) => self.dispatcher.dispatch_song_position(*p, source).await,
            Step::Timer(t) => self.dispatcher.timer(*t, source).await,
        }
    }

    async fn run(&self, source: &str, key: ActionKey, step: Step) -> ActionOutcome {
        if (self.suppressed)() {
            return ActionOutcome::new(false, "ignored: song order discovery active");
        }
        if source == "midi.simulation" && !self.arb(|a| a.allows(Source::PlaybackApi)) {
            return self.forward(&step, source).await;
        }
        let origin = if source == "playback_api" {
            Source::PlaybackApi
        } else {
            Source::Midi
        };
        if origin == Source::Midi && !self.arb(|a| a.midi_allowed) {
            return ActionOutcome::new(
                false,
                "ignored: MIDI is not the selected connection method",
            );
        }
        if !self.arb(|a| a.allows(origin)) {
            return ActionOutcome::new(
                false,
                if origin == Source::Midi {
                    "ignored: Playback API active"
                } else {
                    "ignored: MIDI active"
                },
            );
        }
        // Reserve before awaiting: simultaneous queues cannot dispatch the same action.
        let blocked = self.arb(|a| {
            if a.duplicate(origin, key)
                || (matches!(step, Step::Action(ActionName::StartNext))
                    && a.other_source_started(origin))
            {
                return true;
            }
            a.remember(origin, key);
            false
        });
        if blocked {
            return ActionOutcome::new(false, "ignored: duplicate cross-source Playback action");
        }
        let outcome = self.forward(&step, source).await;
        if !outcome.accepted {
            self.arb(|a| a.forget(&key));
        }
        outcome
    }
}

enum Step {
    Action(ActionName),
    Position(i64),
    Timer(TimerCall),
}
