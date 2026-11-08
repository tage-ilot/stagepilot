//! Receive-only action mapping: port of `playback_api/mapping.py`.
//!
//! The mapper owns the saved song order, the skips and the generation counter. It decides what a
//! Playback event means for StagePilot; the plugin executes the decision through the arbitrated
//! dispatcher. Every state read the Python code awaits is synchronous here, so the "re-check the
//! generation after the await" steps collapse into one atomic decision ([`Mapper::plan`]).

use crate::arrangement::plan_index_for;
use crate::normalizer::SetlistId;
use crate::normalizer::{EventType, Heartbeat, PlaybackEvent};
use stagepilot_core::state::ApplicationState;
use std::collections::{BTreeMap, BTreeSet};

/// Queue envelope captured on receipt, never reconstructed on consumption.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub event: PlaybackEvent,
    pub discovery: bool,
    pub generation: u64,
}

/// What an observation asks StagePilot to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Plan {
    /// `stop_timer` action.
    Stop,
    /// Stop the timer, freezing it at this Playback position.
    StopAt(f64),
    /// `restart_current` action.
    Restart,
    /// Start this one-based plan position.
    Position(i64),
    ResumeAt(f64),
    SeekTo(f64),
}

/// What one observation asks of StagePilot: the timer plan plus, only in learned mode, an
/// instruction to put Up next back where it was before a tentative binding moved it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Decision {
    pub plan: Option<Plan>,
    /// `Some(index)` restores Up next to that zero-based plan index (`Some(None)`: past the end).
    pub restore_up_next: Option<Option<usize>>,
}

/// A song heard for at least this many seconds (summed from heartbeat positions) is kept.
pub const COMMIT_SECONDS: f64 = 10.0;
/// Two heartbeats further apart than this are a seek or a gap, not playing time.
const MAX_PLAYING_GAP: f64 = 5.0;
/// Shown when a started song could not be placed in the plan.
pub const NOT_PLACED_NOTE: &str = "Playback song not placed; set Up next or run a scan";

/// How the Playback song -> plan position pairing was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderMode {
    /// A discovered, validated song order.
    Scanned,
    /// Provisional: learned from songs as they play.
    Learned,
    None,
}

impl OrderMode {
    pub fn as_str(self) -> &'static str {
        match self {
            OrderMode::Scanned => "scanned",
            OrderMode::Learned => "learned",
            OrderMode::None => "none",
        }
    }
}

/// One learned pairing of an (opaque) Playback song id with a one-based plan position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LearnedSong {
    pub song_id: i64,
    pub plan_position: i64,
    /// Played for [`COMMIT_SECONDS`]; until then the binding is tentative.
    pub confirmed: bool,
    /// The song's Playback length disagreed with its Planning Center length.
    pub doubtful: bool,
}

#[derive(Debug, Clone, Copy)]
struct Tentative {
    song_id: i64,
    /// Up next (zero-based plan index) before this binding moved it.
    prev_up_next: Option<usize>,
    played: f64,
    last_position: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Mapper {
    pub song_order: Vec<i64>,
    /// Songs left out of the pairing (see `arrangement`); the order itself never changes.
    pub skipped_song_ids: BTreeSet<i64>,
    pub skipped_plan_song_ids: BTreeSet<String>,
    pub captured_version: Option<i64>,
    pub setlist_id: Option<SetlistId>,
    pub stale: bool,
    pub discovery: bool,
    generation: u64,
    heartbeat: Option<Heartbeat>,
    invalidated: bool,
    // ---- learned (provisional) order -------------------------------------------------------
    learn_enabled: bool,
    learned: Vec<LearnedSong>,
    tentative: Option<Tentative>,
    /// `(new song id, previous song id)` from the last auto-advance `song.changed`.
    pending_advance: Option<(i64, i64)>,
    learn_plan_id: Option<String>,
    learn_version: Option<i64>,
    learn_setlist: Option<SetlistId>,
    /// Latest playing heartbeat position per Playback song id (its length at the natural end).
    last_position: BTreeMap<i64, f64>,
    learn_note: Option<String>,
}

impl Default for Mapper {
    fn default() -> Self {
        Self {
            song_order: vec![],
            skipped_song_ids: BTreeSet::new(),
            skipped_plan_song_ids: BTreeSet::new(),
            captured_version: None,
            setlist_id: None,
            stale: true,
            discovery: false,
            generation: 0,
            heartbeat: None,
            invalidated: true,
            learn_enabled: true,
            learned: vec![],
            tentative: None,
            pending_advance: None,
            learn_plan_id: None,
            learn_version: None,
            learn_setlist: None,
            last_position: BTreeMap::new(),
            learn_note: None,
        }
    }
}

impl Mapper {
    pub fn new() -> Self {
        Self::default()
    }

    /// Install only a complete, explicitly discovered order, never learned IDs.
    pub fn install_order(
        &mut self,
        song_order: &[i64],
        captured_version: i64,
        setlist_id: Option<SetlistId>,
    ) -> Result<(), String> {
        let unique: BTreeSet<_> = song_order.iter().collect();
        if song_order.is_empty() || song_order.len() > 200 || unique.len() != song_order.len() {
            return Err("Discovery requires unique integer IDs and an integer version.".into());
        }
        self.compare_learned_with_scan(song_order);
        self.drop_learned();
        self.song_order = song_order.to_vec();
        self.captured_version = Some(captured_version);
        self.setlist_id = setlist_id;
        self.stale = true; // A fresh authoritative heartbeat must validate the result.
        self.heartbeat = None;
        self.generation += 1;
        self.invalidated = false;
        Ok(())
    }

    /// Install the saved skips; a change drops anything queued under the old arrangement.
    pub fn set_skips(&mut self, playback: &[i64], plan: &[String]) {
        let playback: BTreeSet<i64> = playback.iter().copied().collect();
        let plan: BTreeSet<String> = plan.iter().cloned().collect();
        if playback != self.skipped_song_ids || plan != self.skipped_plan_song_ids {
            self.skipped_song_ids = playback;
            self.skipped_plan_song_ids = plan;
            self.generation += 1;
        }
    }

    pub fn set_discovery(&mut self, active: bool) {
        if active != self.discovery {
            self.discovery = active;
            self.generation += 1; // Also suppress pre-discovery queued actions.
        }
    }

    /// A disconnect drops queued work without invalidating the saved order.
    pub fn discard_pending(&mut self) {
        self.generation += 1;
        self.heartbeat = None;
    }

    pub fn revision(&self) -> u64 {
        self.generation
    }

    /// Validate status BEFORE queuing the accompanying event batch.
    pub fn observe(
        &mut self,
        heartbeat: Option<Heartbeat>,
        events: &[PlaybackEvent],
        setlist_id: Option<&SetlistId>,
    ) -> Vec<Observation> {
        if let (Some(mine), Some(theirs)) = (&self.setlist_id, setlist_id) {
            if mine != theirs {
                self.invalidate();
            }
        }
        if let Some(theirs) = setlist_id {
            if self
                .learn_setlist
                .as_ref()
                .is_some_and(|mine| mine != theirs)
            {
                self.reset_learned();
            }
            self.learn_setlist = Some(theirs.clone());
        }
        let was_stale = self.stale;
        if let Some(hb) = heartbeat {
            self.heartbeat = Some(hb);
            self.learn_from_heartbeat(&hb);
            if hb.setlist_version != self.captured_version || !self.song_order.contains(&hb.song_id)
            {
                self.invalidate();
            } else if !self.song_order.is_empty() && !self.invalidated {
                // Only a newly installed order can recover from stale observations.
                // Once invalidated, matching later heartbeats cannot silently repair it.
                self.stale = false;
            }
        }
        if was_stale && !self.stale {
            self.drop_learned(); // A validated scanned order replaces anything learned.
        }
        for event in events {
            if event
                .song_id
                .is_some_and(|id| !self.song_order.contains(&id))
            {
                self.invalidate();
            }
        }
        events
            .iter()
            .map(|event| Observation {
                event: event.clone(),
                discovery: self.discovery,
                generation: self.generation,
            })
            .collect()
    }

    fn invalidate(&mut self) {
        // With learning on, an order that is already invalid stays invalid without bumping the
        // generation again: learned mode has foreign song ids on every heartbeat and must not
        // discard queued observations each time. With learning off this is exactly as before.
        let quiet = self.learn_enabled && self.stale && self.invalidated;
        self.stale = true;
        self.invalidated = true;
        if !quiet {
            self.generation += 1;
        }
    }

    // ---- learned order: state --------------------------------------------------------------

    /// Learning is on and there is no valid scanned order to defer to.
    fn learning_active(&self) -> bool {
        self.learn_enabled && (self.stale || self.song_order.is_empty())
    }

    /// The `learn_order` setting. Turning it off clears everything learned.
    pub fn set_learning(&mut self, enabled: bool) {
        if self.learn_enabled && !enabled {
            self.reset_learned();
        }
        self.learn_enabled = enabled;
    }

    pub fn learning_enabled(&self) -> bool {
        self.learn_enabled
    }

    fn drop_learned(&mut self) {
        self.learned.clear();
        self.tentative = None;
        self.pending_advance = None;
        self.learn_note = None;
    }

    /// Clear all learned bindings and drop anything queued under them.
    fn reset_learned(&mut self) {
        let had = !self.learned.is_empty() || self.tentative.is_some();
        self.drop_learned();
        if had {
            self.generation += 1;
        }
    }

    pub fn order_mode(&self) -> OrderMode {
        if !self.song_order.is_empty() && !self.stale {
            OrderMode::Scanned
        } else if self.learn_enabled && !self.learned.is_empty() {
            OrderMode::Learned
        } else {
            OrderMode::None
        }
    }

    /// The learned pairings, in plan order.
    pub fn learned_songs(&self) -> Vec<LearnedSong> {
        let mut list = self.learned.clone();
        list.sort_by_key(|l| l.plan_position);
        list
    }

    /// Plain-language state of learned mode for the operator (`None` unless learning).
    pub fn learned_note(&self, plan_songs: usize) -> Option<String> {
        if !self.learning_active() {
            return None;
        }
        let mut note = format!(
            "Learning the order as songs play ({} of {} placed). Scan for exact order and lengths.",
            self.learned.len(),
            plan_songs
        );
        if self.learned.iter().any(|l| l.doubtful) {
            note.push_str(" Check order; run a scan.");
        }
        if let Some(why) = &self.learn_note {
            note.push(' ');
            note.push_str(why);
            note.push('.');
        }
        Some(note)
    }

    fn position_of(&self, song_id: i64) -> Option<i64> {
        self.learned
            .iter()
            .find(|l| l.song_id == song_id)
            .map(|l| l.plan_position)
    }

    fn holder_of(&self, position: i64) -> Option<i64> {
        self.learned
            .iter()
            .find(|l| l.plan_position == position)
            .map(|l| l.song_id)
    }

    /// Count (never name) how a scan disagrees with what was learned.
    fn compare_learned_with_scan(&self, scanned: &[i64]) {
        if self.learned.is_empty() {
            return;
        }
        let (mut agree, mut disagree, mut absent) = (0u32, 0u32, 0u32);
        for l in &self.learned {
            match scanned.iter().position(|id| *id == l.song_id) {
                Some(i) if i as i64 + 1 == l.plan_position => agree += 1,
                Some(_) => disagree += 1,
                None => absent += 1,
            }
        }
        tracing::info!(
            learned = self.learned.len() as u32,
            agree,
            disagree,
            absent,
            "playback_learned_vs_scanned"
        );
    }

    /// Heartbeat bookkeeping: playing time for the 10 s commit and each song's last position.
    fn learn_from_heartbeat(&mut self, hb: &Heartbeat) {
        if let Some(version) = hb.setlist_version {
            if self.learn_version.is_some_and(|old| old != version) {
                self.reset_learned();
            }
            self.learn_version = Some(version);
        }
        if hb.playing {
            self.last_position.insert(hb.song_id, hb.position);
        }
        let Some(t) = self.tentative.as_mut() else {
            return;
        };
        if t.song_id != hb.song_id || !hb.playing {
            t.last_position = None;
            return;
        }
        if let Some(prev) = t.last_position {
            let step = hb.position - prev;
            if step > 0.0 && step <= MAX_PLAYING_GAP {
                t.played += step;
            }
        }
        t.last_position = Some(hb.position);
        if t.played >= COMMIT_SECONDS {
            let id = t.song_id;
            self.tentative = None;
            if let Some(l) = self.learned.iter_mut().find(|l| l.song_id == id) {
                l.confirmed = true;
            }
        }
    }

    /// Release a binding that never played long enough; returns where Up next must go back to.
    fn abort_tentative(&mut self) -> Option<Option<usize>> {
        let t = self.tentative.take()?;
        self.learned.retain(|l| l.song_id != t.song_id);
        Some(t.prev_up_next)
    }

    /// A plan with a different id means different songs: bindings no longer apply.
    fn sync_plan(&mut self, state: &ApplicationState) {
        let id = state.plan.as_ref().map(|p| p.id.clone());
        if self.learn_plan_id != id {
            if self.learn_plan_id.is_some() {
                self.reset_learned();
            }
            self.learn_plan_id = id;
        }
    }

    /// Call when the loaded plan may have changed (outside an event).
    pub fn note_plan(&mut self, plan_id: Option<&str>) {
        let id = plan_id.map(String::from);
        if self.learn_plan_id != id {
            if self.learn_plan_id.is_some() {
                self.reset_learned();
            }
            self.learn_plan_id = id;
        }
    }

    /// Flag a bound song whose Playback length is far from its Planning Center length.
    fn check_doubt(&mut self, song_id: i64, state: &ApplicationState) {
        let Some(position) = self.position_of(song_id) else {
            return;
        };
        let (Some(length), Some(plan)) = (self.last_position.get(&song_id).copied(), &state.plan)
        else {
            return;
        };
        let Some(pco) = plan
            .songs
            .get((position - 1) as usize)
            .and_then(|s| s.duration_seconds)
            .filter(|d| *d > 0)
        else {
            return;
        };
        let pco = f64::from(pco);
        if (length - pco).abs() > f64::max(20.0, 0.15 * pco) {
            if let Some(l) = self.learned.iter_mut().find(|l| l.song_id == song_id) {
                l.doubtful = true;
            }
        }
    }

    fn up_next_index(state: &ApplicationState) -> Option<usize> {
        let up = state.up_next_song.as_ref()?;
        state
            .plan
            .as_ref()?
            .songs
            .iter()
            .position(|s| s.id == up.id)
    }

    /// Learned-mode decision for one observation; the only place bindings are made.
    fn plan_learned(&mut self, observation: &Observation, state: &ApplicationState) -> Decision {
        let event = &observation.event;
        self.sync_plan(state);
        match event.kind {
            EventType::SongChanged if event.reason.as_deref() == Some("auto-advance") => {
                if let (Some(new), Some(prev)) = (event.song_id, event.previous_song_id) {
                    self.check_doubt(prev, state);
                    self.pending_advance = Some((new, prev));
                }
                Decision::default()
            }
            EventType::SongEnded => {
                if let Some(id) = event.song_id {
                    self.check_doubt(id, state);
                }
                Decision::default()
            }
            EventType::SongStarted => self.learned_start(event, state),
            EventType::SongPaused | EventType::SongStopped => {
                self.pending_advance = None;
                let restore = self.abort_tentative();
                Decision {
                    plan: self.plan_transport(event, state),
                    restore_up_next: restore,
                }
            }
            EventType::TransportReverted
                if event.reason.as_deref() == Some("song.started")
                    && !event.playing.unwrap_or(false) =>
            {
                let restore = self.abort_tentative();
                Decision {
                    plan: Some(Plan::Stop),
                    restore_up_next: restore,
                }
            }
            _ => Decision {
                plan: self.plan_transport(event, state),
                restore_up_next: None,
            },
        }
    }

    fn learned_start(&mut self, event: &PlaybackEvent, state: &ApplicationState) -> Decision {
        let mut decision = Decision::default();
        let Some(x) = event.song_id else {
            return decision;
        };
        let advance_from = self
            .pending_advance
            .take()
            .filter(|(new, _)| *new == x)
            .map(|(_, prev)| prev);
        let len = state.plan.as_ref().map_or(0, |p| p.songs.len()) as i64;
        if self.heartbeat.is_none() || len == 0 {
            return decision;
        }
        let mut up_next = Self::up_next_index(state);
        if self.tentative.is_some_and(|t| t.song_id != x) {
            decision.restore_up_next = self.abort_tentative();
            if let Some(restored) = decision.restore_up_next {
                up_next = restored;
            }
        }
        let start = |k: i64| {
            if state.current_song.is_some() && state.current_song_index == Some((k - 1) as usize) {
                Plan::Restart
            } else {
                Plan::Position(k)
            }
        };
        // Single-step evidence: the previous song was bound at k, so this one is k + 1.
        let evidence = advance_from
            .and_then(|prev| self.position_of(prev))
            .map(|k| k + 1);
        let mut bound = self.position_of(x);
        if bound.is_some_and(|k| k > len) {
            self.learned.retain(|l| l.song_id != x); // The plan shrank under this binding.
            bound = None;
        }
        // 1. Already placed (unless fresh evidence says otherwise: that is a conflict).
        if let Some(k) = bound {
            if evidence.is_none_or(|e| e == k) {
                decision.plan = Some(start(k));
                self.learn_note = None;
                return decision;
            }
            self.reset_learned();
        }
        // 2. Auto-advance from a placed song.
        let evidence = if bound.is_some() { None } else { evidence };
        let mut target = None;
        if let Some(e) = evidence.filter(|e| *e <= len) {
            match self.holder_of(e) {
                None => target = Some(e),
                Some(other) if other != x => self.reset_learned(), // Two ids, one position.
                Some(_) => {}
            }
        }
        // 3. Otherwise the plan's Up next (what Start next would play).
        if target.is_none() {
            target = up_next
                .map(|u| u as i64 + 1)
                .filter(|p| *p >= 1 && *p <= len && self.holder_of(*p).is_none());
        }
        // 4. Otherwise monitor only, and say why.
        let Some(position) = target else {
            self.learn_note = Some(NOT_PLACED_NOTE.into());
            return decision;
        };
        self.learned.push(LearnedSong {
            song_id: x,
            plan_position: position,
            confirmed: false,
            doubtful: false,
        });
        self.tentative = Some(Tentative {
            song_id: x,
            prev_up_next: up_next,
            played: 0.0,
            last_position: None,
        });
        self.learn_note = None;
        decision.plan = Some(Plan::Position(position));
        decision
    }

    /// Pause / stop / resume / seek / snapshot: shared by scanned and learned mode.
    fn plan_transport(&self, event: &PlaybackEvent, state: &ApplicationState) -> Option<Plan> {
        match event.kind {
            EventType::SongPaused => match event.position {
                Some(position) if self.current_song_is(event, state) => {
                    Some(Plan::StopAt(position))
                }
                _ => Some(Plan::Stop),
            },
            EventType::SongStopped => Some(Plan::Stop),
            EventType::TransportReverted
                if event.reason.as_deref() == Some("song.started")
                    && !event.playing.unwrap_or(false) =>
            {
                Some(Plan::Stop)
            }
            EventType::SongResumed | EventType::Seek | EventType::StateSnapshot => {
                self.reapply(event, state)
            }
            _ => None,
        }
    }

    /// Decide with learning: bindings are made here, so it needs `&mut self`. Outside learned
    /// mode this is exactly [`Mapper::plan`].
    pub fn decide(&mut self, observation: &Observation, state: &ApplicationState) -> Decision {
        if observation.discovery || self.discovery || observation.generation != self.generation {
            return Decision::default();
        }
        if self.learning_active() {
            return self.plan_learned(observation, state);
        }
        Decision {
            plan: self.plan(observation, state),
            restore_up_next: None,
        }
    }

    /// The plan song this Playback song drives, honouring the saved skips.
    fn plan_index(&self, song_id: i64, state: &ApplicationState) -> Option<usize> {
        let ids: Option<Vec<&str>> = state
            .plan
            .as_ref()
            .map(|p| p.songs.iter().map(|s| s.id.as_str()).collect());
        let skipped_plan: Vec<&str> = self
            .skipped_plan_song_ids
            .iter()
            .map(String::as_str)
            .collect();
        let skipped: Vec<i64> = self.skipped_song_ids.iter().copied().collect();
        plan_index_for(
            song_id,
            ids.as_deref(),
            &self.song_order,
            &skipped_plan,
            &skipped,
        )
    }

    /// True only for a valid order whose mapped song is StagePilot's current song.
    fn current_song_is(&self, event: &PlaybackEvent, state: &ApplicationState) -> bool {
        event
            .song_id
            .is_some_and(|song_id| self.song_is_current(song_id, state))
    }

    /// Does this Playback song drive StagePilot's current song (valid order, mapped, current)?
    pub fn song_is_current(&self, song_id: i64, state: &ApplicationState) -> bool {
        if self.learning_active() {
            return self.heartbeat.is_some()
                && self.position_of(song_id).is_some_and(|k| {
                    state.current_song.is_some()
                        && state.current_song_index == Some((k - 1) as usize)
                });
        }
        if self.stale || self.heartbeat.is_none() || !self.song_order.contains(&song_id) {
            return false;
        }
        state.current_song.is_some() && state.current_song_index == self.plan_index(song_id, state)
    }

    /// A position re-sync (the heartbeat disagreed with the timer for a while) wrapped so it
    /// goes through the same checks as a real seek and is dropped if the order changes.
    pub fn resync_observation(&self, event: PlaybackEvent) -> Observation {
        Observation {
            event,
            discovery: self.discovery,
            generation: self.generation,
        }
    }

    /// Decide what `observation` means right now (Python `PlaybackMapper.dispatch`).
    pub fn plan(&self, observation: &Observation, state: &ApplicationState) -> Option<Plan> {
        let event = &observation.event;
        if observation.discovery || self.discovery || observation.generation != self.generation {
            return None;
        }
        match event.kind {
            // Stop works even while the saved order is stale; the exact frozen position is used
            // only when this is provably the current song.
            EventType::SongPaused
            | EventType::SongStopped
            | EventType::TransportReverted
            | EventType::SongResumed
            | EventType::Seek
            | EventType::StateSnapshot => self.plan_transport(event, state),
            EventType::SongStarted => {
                if self.stale || self.heartbeat.is_none() {
                    return None;
                }
                let song_id = event.song_id?;
                if !self.song_order.contains(&song_id) {
                    return None;
                }
                // Skipped, unpaired, or the plan no longer fits: leave the timer alone.
                let index = self.plan_index(song_id, state)?;
                let position = index as i64 + 1;
                if state.current_song.is_some() && state.current_song_index == Some(index) {
                    Some(Plan::Restart)
                } else {
                    Some(Plan::Position(position))
                }
            }
            _ => None,
        }
    }

    /// Resume / seek / reconnect alignment; monitor-only unless mapped and valid.
    fn reapply(&self, event: &PlaybackEvent, state: &ApplicationState) -> Option<Plan> {
        let position = event.position?;
        if event.kind == EventType::StateSnapshot && !event.playing.unwrap_or(false) {
            return None; // Reconnected while not playing: nothing to align.
        }
        if !self.current_song_is(event, state) {
            return None;
        }
        Some(if event.kind == EventType::Seek {
            Plan::SeekTo(position)
        } else {
            Plan::ResumeAt(position)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stagepilot_core::state::{ServicePlan, Song};

    fn hb(song: i64, version: i64) -> Heartbeat {
        Heartbeat {
            song_id: song,
            position: 0.0,
            playing: false,
            pad: false,
            setlist_version: Some(version),
        }
    }

    fn ev(kind: EventType, song: Option<i64>) -> PlaybackEvent {
        let mut e: PlaybackEvent = serde_json::from_value(serde_json::json!({
            "type": serde_json::to_value(kind).unwrap(), "timestamp": 0.0
        }))
        .unwrap();
        e.song_id = song;
        e
    }

    fn state(current: Option<usize>) -> ApplicationState {
        let songs: Vec<Song> = (0..3)
            .map(|i| Song::new(&format!("s{i}"), "t", Some(100), i + 1))
            .collect();
        ApplicationState {
            plan: Some(ServicePlan {
                id: "p".into(),
                title: "t".into(),
                date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                service_type: "s".into(),
                service_type_id: None,
                service_times: vec![],
                duration_source: "d".into(),
                songs: songs.clone(),
            }),
            current_song: current.map(|i| songs[i].clone()),
            current_song_index: current,
            ..ApplicationState::default()
        }
    }

    fn ready() -> Mapper {
        let mut m = Mapper::new();
        m.install_order(&[10, 20, 30], 5, None).unwrap();
        m.observe(Some(hb(10, 5)), &[], None);
        m
    }

    fn plan_of(m: &mut Mapper, e: PlaybackEvent, s: &ApplicationState) -> Option<Plan> {
        let o = m.observe(None, &[e], None).remove(0);
        m.plan(&o, s)
    }

    #[test]
    fn install_requires_a_unique_bounded_order() {
        let mut m = Mapper::new();
        assert!(m.install_order(&[], 1, None).is_err());
        assert!(m.install_order(&[1, 1], 1, None).is_err());
        assert!(m
            .install_order(&(0..201).collect::<Vec<_>>(), 1, None)
            .is_err());
        assert!(m.install_order(&[1, 2], 1, None).is_ok());
        assert!(m.stale, "a fresh authoritative heartbeat must validate it");
    }

    #[test]
    fn a_matching_heartbeat_validates_but_never_repairs_an_invalidated_order() {
        let mut m = ready();
        assert!(!m.stale);
        m.observe(Some(hb(10, 6)), &[], None); // version changed
        assert!(m.stale);
        m.observe(Some(hb(10, 5)), &[], None); // matching again does not repair it
        assert!(m.stale);
        m.install_order(&[10, 20, 30], 5, None).unwrap();
        m.observe(Some(hb(10, 5)), &[], None);
        assert!(!m.stale, "only a newly installed order recovers");
    }

    #[test]
    fn unknown_song_event_or_other_setlist_invalidates() {
        let mut m = ready();
        m.observe(None, &[ev(EventType::SongSelected, Some(99))], None);
        assert!(m.stale);
        let mut m = Mapper::new();
        m.install_order(&[10], 5, Some(SetlistId::Int(1))).unwrap();
        m.observe(Some(hb(10, 5)), &[], Some(&SetlistId::Int(1)));
        assert!(!m.stale);
        m.observe(None, &[], Some(&SetlistId::Int(2)));
        assert!(m.stale);
    }

    #[test]
    fn song_started_maps_to_a_position_or_a_restart() {
        let mut m = ready();
        assert_eq!(
            plan_of(&mut m, ev(EventType::SongStarted, Some(20)), &state(None)),
            Some(Plan::Position(2))
        );
        assert_eq!(
            plan_of(
                &mut m,
                ev(EventType::SongStarted, Some(20)),
                &state(Some(1))
            ),
            Some(Plan::Restart)
        );
    }

    #[test]
    fn stopped_and_reverted_stop_even_when_stale() {
        let mut m = Mapper::new(); // stale, no order
        let s = state(None);
        assert_eq!(
            plan_of(&mut m, ev(EventType::SongStopped, Some(1)), &s),
            Some(Plan::Stop)
        );
        let mut reverted = ev(EventType::TransportReverted, Some(1));
        reverted.reason = Some("song.started".into());
        reverted.playing = Some(false);
        assert_eq!(plan_of(&mut m, reverted.clone(), &s), Some(Plan::Stop));
        reverted.playing = Some(true);
        assert_eq!(plan_of(&mut m, reverted, &s), None);
        assert_eq!(
            plan_of(&mut m, ev(EventType::SongStarted, Some(1)), &s),
            None,
            "stale never starts"
        );
    }

    #[test]
    fn paused_freezes_at_the_position_only_for_the_current_song() {
        let mut m = ready();
        let mut paused = ev(EventType::SongPaused, Some(20));
        paused.position = Some(12.5);
        assert_eq!(
            plan_of(&mut m, paused.clone(), &state(Some(1))),
            Some(Plan::StopAt(12.5))
        );
        assert_eq!(plan_of(&mut m, paused, &state(Some(0))), Some(Plan::Stop));
    }

    #[test]
    fn resume_seek_and_snapshot_realign_only_a_valid_current_song() {
        let mut m = ready();
        let s = state(Some(1));
        let mut resumed = ev(EventType::SongResumed, Some(20));
        resumed.position = Some(30.0);
        assert_eq!(
            plan_of(&mut m, resumed.clone(), &s),
            Some(Plan::ResumeAt(30.0))
        );
        let mut seek = ev(EventType::Seek, Some(20));
        seek.position = Some(40.0);
        assert_eq!(plan_of(&mut m, seek, &s), Some(Plan::SeekTo(40.0)));
        let mut snap = ev(EventType::StateSnapshot, Some(20));
        snap.position = Some(5.0);
        snap.playing = Some(false);
        assert_eq!(
            plan_of(&mut m, snap.clone(), &s),
            None,
            "reconnected while stopped"
        );
        snap.playing = Some(true);
        assert_eq!(plan_of(&mut m, snap, &s), Some(Plan::ResumeAt(5.0)));
        assert_eq!(
            plan_of(&mut m, resumed, &state(Some(2))),
            None,
            "not the current song"
        );
    }

    #[test]
    fn queued_observations_die_with_their_generation_and_during_discovery() {
        let mut m = ready();
        let o = m
            .observe(None, &[ev(EventType::SongStopped, Some(10))], None)
            .remove(0);
        m.discard_pending(); // disconnect
        assert_eq!(m.plan(&o, &state(None)), None);
        let o = m
            .observe(None, &[ev(EventType::SongStopped, Some(10))], None)
            .remove(0);
        m.set_discovery(true);
        assert_eq!(m.plan(&o, &state(None)), None);
    }

    #[test]
    fn skips_remap_the_plan_and_a_skipped_song_does_not_start_anything() {
        let mut m = ready();
        m.set_skips(&[10], &[]);
        m.observe(Some(hb(20, 5)), &[], None);
        // 10 is skipped so 20 pairs with plan song 1 (index 0).
        assert_eq!(
            plan_of(&mut m, ev(EventType::SongStarted, Some(20)), &state(None)),
            Some(Plan::Position(1))
        );
        assert_eq!(
            plan_of(&mut m, ev(EventType::SongStarted, Some(10)), &state(None)),
            None
        );
    }
}

#[cfg(test)]
mod learned_tests {
    use super::*;
    use stagepilot_core::state::{ServicePlan, Song};

    const VERSION: i64 = 5;

    fn plan_state(
        plan_id: &str,
        current: Option<usize>,
        up_next: Option<usize>,
        pco_seconds: u32,
    ) -> ApplicationState {
        let songs: Vec<Song> = (0..4)
            .map(|i| Song::new(&format!("s{i}"), "t", Some(pco_seconds), i + 1))
            .collect();
        ApplicationState {
            plan: Some(ServicePlan {
                id: plan_id.into(),
                title: "t".into(),
                date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                service_type: "s".into(),
                service_type_id: None,
                service_times: vec![],
                duration_source: "d".into(),
                songs: songs.clone(),
            }),
            current_song: current.map(|i| songs[i].clone()),
            current_song_index: current,
            up_next_song: up_next.map(|i| songs[i].clone()),
            ..ApplicationState::default()
        }
    }

    fn state(current: Option<usize>, up_next: Option<usize>) -> ApplicationState {
        plan_state("plan", current, up_next, 200)
    }

    fn hb(song: i64, position: f64, playing: bool) -> Heartbeat {
        Heartbeat {
            song_id: song,
            position,
            playing,
            pad: false,
            setlist_version: Some(VERSION),
        }
    }

    fn ev(kind: EventType, song: i64) -> PlaybackEvent {
        let mut e: PlaybackEvent = serde_json::from_value(serde_json::json!({
            "type": serde_json::to_value(kind).unwrap(), "timestamp": 0.0
        }))
        .unwrap();
        e.song_id = Some(song);
        e
    }

    fn started(song: i64) -> PlaybackEvent {
        ev(EventType::SongStarted, song)
    }

    fn advance(from: i64, to: i64) -> PlaybackEvent {
        let mut e = ev(EventType::SongChanged, to);
        e.reason = Some("auto-advance".into());
        e.previous_song_id = Some(from);
        e
    }

    /// One heartbeat plus events, each observation decided in order against `state`.
    fn drive(
        m: &mut Mapper,
        heartbeat: Heartbeat,
        events: &[PlaybackEvent],
        state: &ApplicationState,
    ) -> Vec<Decision> {
        let observed = m.observe(Some(heartbeat), events, None);
        observed.iter().map(|o| m.decide(o, state)).collect()
    }

    /// Start `song` and let it play `seconds` of heartbeats, one per second.
    fn play(m: &mut Mapper, song: i64, seconds: u32, state: &ApplicationState) -> Decision {
        let d = drive(m, hb(song, 0.0, true), &[started(song)], state).remove(0);
        for t in 1..=seconds {
            m.observe(Some(hb(song, f64::from(t), true)), &[], None);
        }
        d
    }

    fn pos(plan: Plan) -> Decision {
        Decision {
            plan: Some(plan),
            restore_up_next: None,
        }
    }

    fn ids(m: &Mapper) -> Vec<(i64, i64, bool)> {
        m.learned_songs()
            .iter()
            .map(|l| (l.song_id, l.plan_position, l.confirmed))
            .collect()
    }

    #[test]
    fn first_song_binds_to_the_up_next_position() {
        let mut m = Mapper::new();
        assert_eq!(m.order_mode(), OrderMode::None);
        let d = play(&mut m, 501, 0, &state(None, Some(0)));
        assert_eq!(d, pos(Plan::Position(1)));
        assert_eq!(ids(&m), [(501, 1, false)], "tentative until it has played");
        assert_eq!(m.order_mode(), OrderMode::Learned);
    }

    #[test]
    fn up_next_other_than_the_first_song_is_the_anchor() {
        let mut m = Mapper::new();
        let d = play(&mut m, 501, 0, &state(None, Some(2)));
        assert_eq!(d, pos(Plan::Position(3)));
    }

    #[test]
    fn a_bound_song_restarts_when_current_and_repositions_otherwise() {
        let mut m = Mapper::new();
        play(&mut m, 501, 11, &state(None, Some(0)));
        assert_eq!(ids(&m), [(501, 1, true)]);
        let again = drive(
            &mut m,
            hb(501, 0.0, true),
            &[started(501)],
            &state(Some(0), Some(1)),
        );
        assert_eq!(again, [pos(Plan::Restart)]);
        let elsewhere = drive(
            &mut m,
            hb(501, 0.0, true),
            &[started(501)],
            &state(Some(2), Some(3)),
        );
        assert_eq!(elsewhere, [pos(Plan::Position(1))]);
    }

    #[test]
    fn auto_advance_steps_one_position_from_the_bound_song() {
        let mut m = Mapper::new();
        play(&mut m, 501, 11, &state(None, Some(0)));
        // Up next deliberately points elsewhere: the single-step evidence must win.
        let s = state(Some(0), Some(3));
        let out = drive(
            &mut m,
            hb(502, 0.0, true),
            &[advance(501, 502), started(502)],
            &s,
        );
        assert_eq!(out, [Decision::default(), pos(Plan::Position(2))]);
        assert_eq!(ids(&m), [(501, 1, true), (502, 2, false)]);
    }

    #[test]
    fn auto_advance_past_the_end_of_the_plan_falls_back_to_up_next() {
        let mut m = Mapper::new();
        play(&mut m, 501, 11, &state(None, Some(3)));
        assert_eq!(ids(&m), [(501, 4, true)]);
        let s = state(Some(3), Some(0));
        let out = drive(
            &mut m,
            hb(502, 0.0, true),
            &[advance(501, 502), started(502)],
            &s,
        );
        assert_eq!(
            out[1],
            pos(Plan::Position(1)),
            "position 5 is outside a 4-song plan"
        );
    }

    #[test]
    fn an_occupied_up_next_position_means_no_binding_and_says_why() {
        let mut m = Mapper::new();
        play(&mut m, 501, 11, &state(None, Some(0)));
        let out = drive(
            &mut m,
            hb(777, 0.0, true),
            &[started(777)],
            &state(Some(0), Some(0)),
        );
        assert_eq!(out, [Decision::default()]);
        assert_eq!(ids(&m), [(501, 1, true)]);
        assert!(m
            .learned_note(4)
            .unwrap()
            .contains("Playback song not placed; set Up next or run a scan"));
    }

    #[test]
    fn no_up_next_and_no_evidence_means_monitor_only() {
        let mut m = Mapper::new();
        let out = drive(
            &mut m,
            hb(501, 0.0, true),
            &[started(501)],
            &state(None, None),
        );
        assert_eq!(out, [Decision::default()]);
        assert!(m.learned_songs().is_empty());
        assert_eq!(m.order_mode(), OrderMode::None);
        // No plan at all is the same.
        let out = drive(
            &mut m,
            hb(501, 0.0, true),
            &[started(501)],
            &ApplicationState::default(),
        );
        assert_eq!(out, [Decision::default()]);
    }

    #[test]
    fn two_ids_for_one_position_is_a_conflict_that_resets_everything() {
        let mut m = Mapper::new();
        play(&mut m, 501, 11, &state(None, Some(0)));
        let s = state(Some(0), Some(1));
        drive(
            &mut m,
            hb(502, 0.0, true),
            &[advance(501, 502), started(502)],
            &s,
        );
        for t in 1..=11 {
            m.observe(Some(hb(502, f64::from(t), true)), &[], None);
        }
        let before = m.revision();
        // 503 "follows" 501 (position 2) but 502 already holds position 2.
        let s = state(Some(1), Some(3));
        let out = drive(
            &mut m,
            hb(503, 0.0, true),
            &[advance(501, 503), started(503)],
            &s,
        );
        assert!(m.revision() > before, "a reset bumps the generation");
        assert_eq!(out[1], pos(Plan::Position(4)), "re-anchored on Up next");
        assert_eq!(ids(&m), [(503, 4, false)], "501 and 502 were forgotten");
    }

    #[test]
    fn one_id_at_two_positions_is_a_conflict_that_resets_everything() {
        let mut m = Mapper::new();
        play(&mut m, 501, 11, &state(None, Some(0)));
        play(&mut m, 503, 11, &state(Some(0), Some(2)));
        assert_eq!(ids(&m), [(501, 1, true), (503, 3, true)]);
        // 503 is said to follow 501 (so position 2) but it is bound at 3.
        let s = state(Some(0), Some(1));
        let out = drive(
            &mut m,
            hb(503, 0.0, true),
            &[advance(501, 503), started(503)],
            &s,
        );
        assert_eq!(out[1], pos(Plan::Position(2)));
        assert_eq!(ids(&m), [(503, 2, false)]);
    }

    #[test]
    fn a_song_is_kept_after_ten_seconds_of_heartbeat_time() {
        let mut m = Mapper::new();
        play(&mut m, 501, 0, &state(None, Some(0)));
        for t in 1..=10 {
            m.observe(Some(hb(501, f64::from(t), true)), &[], None);
        }
        assert_eq!(ids(&m), [(501, 1, false)], "9 s counted: still tentative");
        m.observe(Some(hb(501, 11.0, true)), &[], None);
        assert_eq!(ids(&m), [(501, 1, true)], "10 s counted: kept");
    }

    #[test]
    fn a_seek_does_not_count_as_playing_time() {
        let mut m = Mapper::new();
        play(&mut m, 501, 0, &state(None, Some(0)));
        m.observe(Some(hb(501, 1.0, true)), &[], None);
        m.observe(Some(hb(501, 120.0, true)), &[], None); // jumped, not played
        m.observe(Some(hb(501, 121.0, true)), &[], None);
        assert_eq!(ids(&m), [(501, 1, false)]);
    }

    #[test]
    fn stopping_before_ten_seconds_releases_the_slot_and_restores_up_next() {
        for kind in [EventType::SongStopped, EventType::SongPaused] {
            let mut m = Mapper::new();
            play(&mut m, 501, 3, &state(None, Some(1)));
            assert_eq!(ids(&m), [(501, 2, false)]);
            let s = state(Some(1), Some(2));
            let out = drive(&mut m, hb(501, 3.0, false), &[ev(kind, 501)], &s);
            assert_eq!(
                out,
                [Decision {
                    plan: Some(Plan::Stop),
                    restore_up_next: Some(Some(1)),
                }],
                "{kind:?}"
            );
            assert!(m.learned_songs().is_empty());
        }
    }

    #[test]
    fn switching_songs_before_ten_seconds_releases_and_rebinds_from_the_restored_up_next() {
        let mut m = Mapper::new();
        play(&mut m, 501, 2, &state(None, Some(0))); // a blip on the wrong song
        let s = state(Some(0), Some(1));
        let out = drive(&mut m, hb(502, 0.0, true), &[started(502)], &s);
        assert_eq!(
            out,
            [Decision {
                plan: Some(Plan::Position(1)),
                restore_up_next: Some(Some(0)),
            }]
        );
        assert_eq!(ids(&m), [(502, 1, false)]);
    }

    #[test]
    fn a_blip_never_changes_what_a_later_song_gets() {
        let mut m = Mapper::new();
        play(&mut m, 501, 1, &state(None, Some(0)));
        drive(
            &mut m,
            hb(501, 1.0, false),
            &[ev(EventType::SongStopped, 501)],
            &state(Some(0), Some(1)),
        );
        // Up next was restored to 0, so the real first song takes position 1.
        let d = play(&mut m, 502, 11, &state(Some(0), Some(0)));
        assert_eq!(d, pos(Plan::Position(1)));
        assert_eq!(ids(&m), [(502, 1, true)]);
    }

    #[test]
    fn stop_or_pause_after_the_commit_keeps_the_binding() {
        for kind in [EventType::SongStopped, EventType::SongPaused] {
            let mut m = Mapper::new();
            play(&mut m, 501, 11, &state(None, Some(0)));
            let out = drive(
                &mut m,
                hb(501, 11.0, false),
                &[ev(kind, 501)],
                &state(Some(0), Some(1)),
            );
            assert_eq!(out[0].restore_up_next, None);
            assert_eq!(ids(&m), [(501, 1, true)]);
        }
    }

    #[test]
    fn a_reverted_start_before_ten_seconds_releases_the_slot() {
        let mut m = Mapper::new();
        play(&mut m, 501, 1, &state(None, Some(0)));
        let mut reverted = ev(EventType::TransportReverted, 501);
        reverted.reason = Some("song.started".into());
        reverted.playing = Some(false);
        let out = drive(
            &mut m,
            hb(501, 0.0, false),
            &[reverted],
            &state(Some(0), Some(1)),
        );
        assert_eq!(
            out,
            [Decision {
                plan: Some(Plan::Stop),
                restore_up_next: Some(Some(0)),
            }]
        );
    }

    fn with_two_songs() -> Mapper {
        let mut m = Mapper::new();
        play(&mut m, 501, 11, &state(None, Some(0)));
        drive(
            &mut m,
            hb(502, 0.0, true),
            &[advance(501, 502), started(502)],
            &state(Some(0), Some(1)),
        );
        for t in 1..=11 {
            m.observe(Some(hb(502, f64::from(t), true)), &[], None);
        }
        assert_eq!(ids(&m), [(501, 1, true), (502, 2, true)]);
        m
    }

    #[test]
    fn a_new_playback_setlist_forgets_everything() {
        let mut m = with_two_songs();
        m.observe(None, &[], Some(&SetlistId::Int(1)));
        assert_eq!(ids(&m).len(), 2, "the first id seen is just remembered");
        m.observe(None, &[], Some(&SetlistId::Int(2)));
        assert!(m.learned_songs().is_empty());
    }

    #[test]
    fn a_new_setlist_version_forgets_everything() {
        let mut m = with_two_songs();
        let mut newer = hb(502, 12.0, true);
        newer.setlist_version = Some(VERSION + 1);
        m.observe(Some(newer), &[], None);
        assert!(m.learned_songs().is_empty());
    }

    #[test]
    fn a_different_plan_forgets_everything_but_a_reload_of_the_same_plan_does_not() {
        let mut m = with_two_songs();
        m.note_plan(Some("plan")); // same id: reloaded, not replaced
        assert_eq!(ids(&m).len(), 2);
        let before = m.revision();
        m.note_plan(Some("other"));
        assert!(m.learned_songs().is_empty());
        assert!(m.revision() > before);
        // The same through an event.
        let mut m = with_two_songs();
        let s = plan_state("other", None, Some(0), 200);
        let out = drive(&mut m, hb(502, 0.0, true), &[started(502)], &s);
        assert_eq!(
            out,
            [pos(Plan::Position(1))],
            "502 is re-placed on the new plan"
        );
        assert_eq!(ids(&m), [(502, 1, false)]);
    }

    #[test]
    fn turning_learning_off_forgets_everything_and_stops_binding() {
        let mut m = with_two_songs();
        m.set_learning(false);
        assert!(m.learned_songs().is_empty());
        assert_eq!(m.order_mode(), OrderMode::None);
        let out = drive(
            &mut m,
            hb(501, 0.0, true),
            &[started(501)],
            &state(None, Some(0)),
        );
        assert_eq!(
            out,
            [Decision::default()],
            "stale and not learning: nothing starts"
        );
        assert!(m.learned_note(4).is_none());
        m.set_learning(true);
        let out = drive(
            &mut m,
            hb(501, 0.0, true),
            &[started(501)],
            &state(None, Some(0)),
        );
        assert_eq!(out, [pos(Plan::Position(1))]);
    }

    #[test]
    fn an_unknown_id_on_a_non_start_event_does_not_disturb_learned_mode() {
        let mut m = with_two_songs();
        let before = m.revision();
        m.observe(
            Some(hb(999, 0.0, false)),
            &[ev(EventType::SongSelected, 999), ev(EventType::Seek, 999)],
            None,
        );
        assert_eq!(ids(&m).len(), 2);
        assert_eq!(m.revision(), before, "queued observations stay valid");
        assert_eq!(m.order_mode(), OrderMode::Learned);
        let o = m
            .observe(None, &[ev(EventType::SongPaused, 999)], None)
            .remove(0);
        assert_eq!(
            m.decide(&o, &state(Some(1), Some(2))).plan,
            Some(Plan::Stop)
        );
    }

    #[test]
    fn transport_uses_a_learned_binding_for_the_current_song() {
        let mut m = with_two_songs();
        let current = state(Some(1), Some(2)); // plan song 2 is playing = 502
        let mut paused = ev(EventType::SongPaused, 502);
        paused.position = Some(42.5);
        let mut resumed = ev(EventType::SongResumed, 502);
        resumed.position = Some(43.0);
        let mut seek = ev(EventType::Seek, 502);
        seek.position = Some(90.0);
        let mut snap = ev(EventType::StateSnapshot, 502);
        snap.position = Some(5.0);
        snap.playing = Some(true);
        let out = drive(
            &mut m,
            hb(502, 42.5, true),
            &[paused.clone(), resumed, seek, snap],
            &current,
        );
        let plans: Vec<_> = out.iter().map(|d| d.plan).collect();
        assert_eq!(
            plans,
            [
                Some(Plan::StopAt(42.5)),
                Some(Plan::ResumeAt(43.0)),
                Some(Plan::SeekTo(90.0)),
                Some(Plan::ResumeAt(5.0)),
            ]
        );
        // Another song is current: only a plain stop, nothing realigned.
        let other = state(Some(0), Some(2));
        let out = drive(&mut m, hb(502, 42.5, true), &[paused], &other);
        assert_eq!(out[0].plan, Some(Plan::Stop));
    }

    #[test]
    fn transport_without_a_heartbeat_after_a_disconnect_is_not_trusted() {
        let mut m = with_two_songs();
        m.discard_pending();
        let mut paused = ev(EventType::SongPaused, 502);
        paused.position = Some(42.5);
        let o = m.observe(None, &[paused], None).remove(0);
        assert_eq!(
            m.decide(&o, &state(Some(1), Some(2))).plan,
            Some(Plan::Stop)
        );
    }

    #[test]
    fn a_length_far_from_the_plan_length_marks_the_binding_doubtful() {
        // Plan length 200 s: tolerance is max(20 s, 15% = 30 s) = 30 s.
        for (length, doubtful) in [(229.0, false), (231.0, true), (170.0, false), (160.0, true)] {
            let mut m = Mapper::new();
            play(&mut m, 501, 11, &state(None, Some(0)));
            m.observe(Some(hb(501, length, true)), &[], None);
            let s = state(Some(0), Some(1));
            drive(
                &mut m,
                hb(502, 0.0, true),
                &[advance(501, 502), started(502)],
                &s,
            );
            let flags: Vec<bool> = m.learned_songs().iter().map(|l| l.doubtful).collect();
            assert_eq!(flags[0], doubtful, "length {length}");
            assert_eq!(m.learned_note(4).unwrap().contains("Check order"), doubtful);
        }
        // A short song: the 20 s floor applies (plan 60 s, 15% would be 9 s).
        let mut m = Mapper::new();
        let s = plan_state("plan", None, Some(0), 60);
        play(&mut m, 501, 11, &s);
        m.observe(Some(hb(501, 78.0, true)), &[], None);
        drive(
            &mut m,
            hb(501, 78.0, true),
            &[ev(EventType::SongEnded, 501)],
            &s,
        );
        assert!(
            !m.learned_songs()[0].doubtful,
            "18 s is within the 20 s floor"
        );
        m.observe(Some(hb(501, 85.0, true)), &[], None);
        drive(
            &mut m,
            hb(501, 85.0, true),
            &[ev(EventType::SongEnded, 501)],
            &s,
        );
        assert!(m.learned_songs()[0].doubtful, "25 s is over the 20 s floor");
    }

    #[test]
    fn a_valid_scanned_order_is_never_overridden() {
        let mut m = Mapper::new();
        m.install_order(&[900, 17, 42], VERSION, None).unwrap();
        m.observe(Some(hb(900, 0.0, false)), &[], None);
        assert_eq!(m.order_mode(), OrderMode::Scanned);
        let s = state(None, Some(0));
        // 17 is scanned position 2 even though Up next says 1.
        let out = drive(&mut m, hb(17, 0.0, true), &[started(17)], &s);
        assert_eq!(out, [pos(Plan::Position(2))]);
        assert!(m.learned_songs().is_empty());
        assert_eq!(m.order_mode(), OrderMode::Scanned);
        assert!(m.learned_note(4).is_none());
    }

    #[test]
    fn installing_a_scan_replaces_what_was_learned() {
        let mut m = with_two_songs();
        m.install_order(&[502, 501, 42], VERSION, None).unwrap();
        assert!(m.learned_songs().is_empty());
        m.observe(Some(hb(502, 0.0, false)), &[], None);
        assert_eq!(m.order_mode(), OrderMode::Scanned);
        // The scan disagrees with what was learned (501 was 1, is now 2): the scan wins.
        let out = drive(
            &mut m,
            hb(501, 0.0, true),
            &[started(501)],
            &state(None, Some(0)),
        );
        assert_eq!(out, [pos(Plan::Position(2))]);
    }

    #[test]
    fn a_stale_scanned_order_defers_to_learning_until_a_heartbeat_validates_it() {
        let mut m = Mapper::new();
        m.install_order(&[900, 17, 42], VERSION, None).unwrap(); // stale until validated
        let out = drive(
            &mut m,
            hb(555, 0.0, true),
            &[started(555)],
            &state(None, Some(0)),
        );
        assert_eq!(
            out,
            [pos(Plan::Position(1))],
            "foreign id: the order is invalid, so learn"
        );
        assert_eq!(m.order_mode(), OrderMode::Learned);
    }
}
