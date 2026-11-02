//! Operator override per fixture: which DMX changes are the operator's and which are the
//! replay's own echo. Pure: every method takes the time, nothing reads a clock.
//!
//! The replay sends `/fixture/<name>/overrides/<property>` and the same change comes back on the
//! `lightkey-api` relay a few tens of milliseconds later. A change on a fixture the replay holds
//! that matches no send of the last [`ECHO_WINDOW`] (same fixture and property, any value, because
//! wheel slots are approximate) is the operator's.

use crate::lightkey_fixtures::{fixture_of, FixtureCommand};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;
use tokio::time::Instant;

/// A relay change this soon after a replay send of the same fixture property is that send coming
/// back, never the operator.
pub const ECHO_WINDOW: Duration = Duration::from_millis(500);
/// After the relay stream was (re)started its state is unknown: changes are not judged for this
/// long, so a reconnect can never make every fixture look operator-driven.
pub const STREAM_HOLDOFF: Duration = Duration::from_secs(3);
/// Remembered sends beyond this are pruned (older than the echo window first).
const MAX_RECENT: usize = 4096;

/// What one batch of relay changes did to the fixtures.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    /// Held fixtures the operator just took: removed from the held set, replay stops sending.
    pub newly_yielded: Vec<String>,
    /// Already yielded fixtures the operator touched again: their quiet time restarted.
    pub restarted: Vec<String>,
}

#[derive(Debug, Default)]
pub struct OperatorTracker {
    /// Lower-cased address -> when replay last sent it.
    recent: HashMap<String, Instant>,
    /// Fixture -> time of the operator's latest change on it.
    yielded: BTreeMap<String, Instant>,
    ignore_until: Option<Instant>,
}

impl OperatorTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replay sent `address` now.
    pub fn note_send(&mut self, address: &str, now: Instant) {
        if self.recent.len() >= MAX_RECENT {
            self.recent
                .retain(|_, at| now.saturating_duration_since(*at) <= ECHO_WINDOW);
        }
        self.recent.insert(address.to_lowercase(), now);
    }

    fn is_echo(&self, address: &str, now: Instant) -> bool {
        self.recent
            .get(&address.to_lowercase())
            .is_some_and(|at| now.saturating_duration_since(*at) <= ECHO_WINDOW)
    }

    /// The relay stream (re)started: do not judge changes for [`STREAM_HOLDOFF`].
    pub fn stream_reset(&mut self, now: Instant) {
        self.ignore_until = Some(now + STREAM_HOLDOFF);
    }

    /// Judge a batch of relay changes seen at `now`. `held` loses the fixtures the operator took.
    /// Keyframe-detected differences are never operator input.
    pub fn observe(
        &mut self,
        held: &mut BTreeSet<String>,
        commands: &[FixtureCommand],
        keyframe: bool,
        now: Instant,
    ) -> Verdict {
        let mut verdict = Verdict::default();
        if keyframe || self.ignore_until.is_some_and(|until| now < until) {
            return verdict;
        }
        for command in commands {
            let Some(fixture) = fixture_of(&command.a) else {
                continue;
            };
            if self.is_echo(&command.a, now) {
                continue;
            }
            if let Some(at) = self.yielded.get_mut(fixture) {
                *at = now;
                if !verdict.restarted.iter().any(|f| f == fixture) {
                    verdict.restarted.push(fixture.to_string());
                }
            } else if held.remove(fixture) {
                self.yielded.insert(fixture.to_string(), now);
                verdict.newly_yielded.push(fixture.to_string());
            }
        }
        verdict
    }

    pub fn is_yielded(&self, fixture: &str) -> bool {
        self.yielded.contains_key(fixture)
    }

    pub fn yielded_count(&self) -> usize {
        self.yielded.len()
    }

    /// When the fixture's quiet time ends, if the operator has it.
    pub fn deadline(&self, fixture: &str, quiet: Duration) -> Option<Instant> {
        self.yielded.get(fixture).map(|at| *at + quiet)
    }

    /// Yielded fixtures with no operator change for `quiet`.
    pub fn due(&self, quiet: Duration, now: Instant) -> Vec<String> {
        self.yielded
            .iter()
            .filter(|(_, at)| now.saturating_duration_since(**at) >= quiet)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Replay has the fixture again.
    pub fn take_back(&mut self, fixture: &str) -> bool {
        self.yielded.remove(fixture).is_some()
    }

    /// Forget every yielded fixture except those in `keep`.
    pub fn retain_yielded(&mut self, keep: &BTreeSet<String>) {
        self.yielded.retain(|name, _| keep.contains(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lightkey_fixtures::FixtureArg;

    fn cmd(address: &str, x: f64) -> FixtureCommand {
        FixtureCommand::new(address, vec![FixtureArg::Float(x)])
    }
    const DIM: &str = "/fixture/A/overrides/dimmer";

    fn held(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn echo_inside_the_window_is_ignored_and_outside_is_not() {
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["A"]);
        t.note_send(DIM, t0);
        let v = t.observe(
            &mut h,
            &[cmd(DIM, 0.5)],
            false,
            t0 + Duration::from_millis(45),
        );
        assert_eq!(v, Verdict::default());
        let v = t.observe(
            &mut h,
            &[cmd(DIM, 0.5)],
            false,
            t0 + Duration::from_millis(600),
        );
        assert_eq!(v.newly_yielded, vec!["A".to_string()]);
        assert!(h.is_empty());
    }

    #[test]
    fn unheld_fixtures_and_keyframes_are_ignored() {
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["B"]);
        assert_eq!(
            t.observe(&mut h, &[cmd(DIM, 1.0)], false, t0),
            Verdict::default()
        );
        assert_eq!(
            t.observe(&mut h, &[cmd("/fixture/B/overrides/dimmer", 1.0)], true, t0),
            Verdict::default()
        );
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn holdoff_after_a_stream_reset() {
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["A"]);
        t.stream_reset(t0);
        assert_eq!(
            t.observe(&mut h, &[cmd(DIM, 1.0)], false, t0 + Duration::from_secs(1)),
            Verdict::default()
        );
        assert_eq!(
            t.observe(&mut h, &[cmd(DIM, 1.0)], false, t0 + Duration::from_secs(4))
                .newly_yielded
                .len(),
            1
        );
    }

    #[test]
    fn due_and_restart() {
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["A"]);
        t.observe(&mut h, &[cmd(DIM, 1.0)], false, t0);
        let quiet = Duration::from_secs(30);
        assert!(t.due(quiet, t0 + Duration::from_secs(29)).is_empty());
        let v = t.observe(
            &mut h,
            &[cmd(DIM, 0.2)],
            false,
            t0 + Duration::from_secs(20),
        );
        assert_eq!(v.restarted, vec!["A".to_string()]);
        assert!(t.due(quiet, t0 + Duration::from_secs(40)).is_empty());
        assert_eq!(
            t.due(quiet, t0 + Duration::from_secs(50)),
            vec!["A".to_string()]
        );
        assert!(t.take_back("A"));
        assert_eq!(t.yielded_count(), 0);
    }
}
