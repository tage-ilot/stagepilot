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
use std::collections::BTreeSet;

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
        if let Some(hb) = heartbeat {
            self.heartbeat = Some(hb);
            if hb.setlist_version != self.captured_version || !self.song_order.contains(&hb.song_id)
            {
                self.invalidate();
            } else if !self.song_order.is_empty() && !self.invalidated {
                // Only a newly installed order can recover from stale observations.
                // Once invalidated, matching later heartbeats cannot silently repair it.
                self.stale = false;
            }
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
        self.stale = true;
        self.invalidated = true;
        self.generation += 1;
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
        let Some(song_id) = event.song_id else {
            return false;
        };
        if self.stale || self.heartbeat.is_none() || !self.song_order.contains(&song_id) {
            return false;
        }
        state.current_song.is_some() && state.current_song_index == self.plan_index(song_id, state)
    }

    /// Decide what `observation` means right now (Python `PlaybackMapper.dispatch`).
    pub fn plan(&self, observation: &Observation, state: &ApplicationState) -> Option<Plan> {
        let event = &observation.event;
        if observation.discovery || self.discovery || observation.generation != self.generation {
            return None;
        }
        match event.kind {
            EventType::SongPaused => {
                // Stop works even while the saved order is stale; the exact frozen position is
                // used only when this is provably the current song.
                match event.position {
                    Some(position) if self.current_song_is(event, state) => {
                        Some(Plan::StopAt(position))
                    }
                    _ => Some(Plan::Stop),
                }
            }
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
