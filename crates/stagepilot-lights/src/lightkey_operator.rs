//! Operator override per fixture: which DMX changes are the operator's and which are the
//! replay's own echo. Pure: every method takes the time, nothing reads a clock.
//!
//! The replay sends `/fixture/<name>/overrides/<property>` and the same change comes back on the
//! `lightkey-api` relay a few tens of milliseconds later. A change on a fixture the replay holds
//! that matches no send of the last [`ECHO_WINDOW`] is the operator's. A match is the same fixture
//! and property with the same value within the property's tolerance; wheel and option properties
//! (and non-numeric values) match with any value, because wheel slots are approximate. Only
//! properties the replay has itself sent to a fixture are judged: a fixture that Lightkey moves
//! on its own (a running cue, an effect, a fade) never counts as the operator.

use crate::lightkey_fixtures::{fixture_of, FixtureArg, FixtureCommand};
use stagepilot_core::config::OscArgument;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::Duration;
use tokio::time::Instant;

/// A relay change this soon after a replay send of the same fixture property is that send coming
/// back, never the operator.
pub const ECHO_WINDOW: Duration = Duration::from_millis(200);
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

/// Fixture names compare case-insensitively and percent-decoded, because held names come from the
/// replay's OSC addresses and yielded names from the relay's.
pub fn normalise_fixture(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).trim().to_lowercase()
}

/// A property whose value is a slot or an option: approximate, so any value in the window matches.
fn is_wheel_like(address: &str) -> bool {
    let a = address.to_lowercase();
    ["wheel", "gobo", "slot", "option", "prism", "macro"]
        .iter()
        .any(|k| a.contains(k))
}

fn tolerance(address: &str) -> f64 {
    if address.to_lowercase().ends_with("angle") {
        1.0
    } else {
        0.02
    }
}

#[derive(Debug, Clone, Copy)]
struct Sent {
    at: Instant,
    /// The value, when the send had exactly one numeric argument.
    value: Option<f64>,
}

fn osc_number(args: &[OscArgument]) -> Option<f64> {
    match args {
        [OscArgument::Float(x)] => Some(*x),
        _ => None,
    }
}

fn fixture_number(args: &[FixtureArg]) -> Option<f64> {
    match args {
        [FixtureArg::Float(x)] => Some(*x),
        _ => None,
    }
}

#[derive(Debug, Default)]
pub struct OperatorTracker {
    /// Lower-cased address -> the replay's sends inside the echo window.
    recent: HashMap<String, Vec<Sent>>,
    /// Lower-cased addresses the replay has sent, per normalised fixture name (O-02).
    sent_properties: HashMap<String, HashSet<String>>,
    /// Fixture -> time of the operator's latest change on it.
    yielded: BTreeMap<String, Instant>,
    ignore_until: Option<Instant>,
}

impl OperatorTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replay sent `address` with `args` now.
    pub fn note_send(&mut self, address: &str, args: &[OscArgument], now: Instant) {
        if self.recent.len() >= MAX_RECENT {
            self.recent.retain(|_, sends| {
                sends.retain(|s| now.saturating_duration_since(s.at) <= ECHO_WINDOW);
                !sends.is_empty()
            });
        }
        let key = normalise_fixture(address);
        if let Some(fixture) = fixture_of(address) {
            self.sent_properties
                .entry(normalise_fixture(fixture))
                .or_default()
                .insert(key.clone());
        }
        let sends = self.recent.entry(key).or_default();
        sends.retain(|s| now.saturating_duration_since(s.at) <= ECHO_WINDOW);
        sends.push(Sent {
            at: now,
            value: osc_number(args),
        });
    }

    /// The replay no longer holds this fixture: forget which properties it had sent.
    pub fn forget_sent(&mut self, fixture: &str) {
        self.sent_properties.remove(&normalise_fixture(fixture));
    }

    fn is_echo(&self, command: &FixtureCommand, now: Instant) -> bool {
        let Some(sends) = self.recent.get(&normalise_fixture(&command.a)) else {
            return false;
        };
        let seen = fixture_number(&command.v);
        sends
            .iter()
            .filter(|s| now.saturating_duration_since(s.at) <= ECHO_WINDOW)
            .any(|s| match (s.value, seen) {
                (Some(sent), Some(seen)) if !is_wheel_like(&command.a) => {
                    (sent - seen).abs() <= tolerance(&command.a)
                }
                _ => true,
            })
    }

    /// The tracked yielded name equal to `fixture` (case and %-encoding aside).
    fn yielded_key(&self, fixture: &str) -> Option<String> {
        let n = normalise_fixture(fixture);
        self.yielded
            .keys()
            .find(|k| normalise_fixture(k) == n)
            .cloned()
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
            let judged = self
                .sent_properties
                .get(&normalise_fixture(fixture))
                .is_some_and(|set| set.contains(&normalise_fixture(&command.a)));
            if !judged || self.is_echo(command, now) {
                continue;
            }
            if let Some(key) = self.yielded_key(fixture) {
                self.yielded.insert(key.clone(), now);
                if !verdict.restarted.iter().any(|f| *f == key) {
                    verdict.restarted.push(key);
                }
            } else {
                let n = normalise_fixture(fixture);
                let name = held.iter().find(|h| normalise_fixture(h) == n).cloned();
                if let Some(name) = name {
                    held.remove(&name);
                    self.yielded.insert(name.clone(), now);
                    verdict.newly_yielded.push(name);
                }
            }
        }
        verdict
    }

    pub fn is_yielded(&self, fixture: &str) -> bool {
        self.yielded_key(fixture).is_some()
    }

    pub fn yielded_count(&self) -> usize {
        self.yielded.len()
    }

    /// When the fixture's quiet time ends, if the operator has it.
    pub fn deadline(&self, fixture: &str, quiet: Duration) -> Option<Instant> {
        self.yielded
            .get(&self.yielded_key(fixture)?)
            .map(|at| *at + quiet)
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
        self.yielded_key(fixture)
            .is_some_and(|key| self.yielded.remove(&key).is_some())
    }

    /// Forget every yielded fixture except those in `keep`.
    pub fn retain_yielded(&mut self, keep: &BTreeSet<String>) {
        let keep: HashSet<String> = keep.iter().map(|k| normalise_fixture(k)).collect();
        self.yielded
            .retain(|name, _| keep.contains(&normalise_fixture(name)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(address: &str, x: f64) -> FixtureCommand {
        FixtureCommand::new(address, vec![FixtureArg::Float(x)])
    }
    fn f(x: f64) -> Vec<OscArgument> {
        vec![OscArgument::Float(x)]
    }
    const DIM: &str = "/fixture/A/overrides/dimmer";

    fn held(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }
    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn echo_inside_the_window_is_ignored_and_outside_is_not() {
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["A"]);
        t.note_send(DIM, &f(0.5), t0);
        let v = t.observe(&mut h, &[cmd(DIM, 0.5)], false, t0 + ms(45));
        assert_eq!(v, Verdict::default());
        let v = t.observe(&mut h, &[cmd(DIM, 0.5)], false, t0 + ms(600));
        assert_eq!(v.newly_yielded, vec!["A".to_string()]);
        assert!(h.is_empty());
    }

    #[test]
    fn operator_change_on_a_moving_property_within_the_old_window_is_not_masked() {
        // O-01: replay sent 0.2; the operator pulls the fader to 0.9 300 ms later.
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["A"]);
        t.note_send(DIM, &f(0.2), t0);
        let v = t.observe(&mut h, &[cmd(DIM, 0.9)], false, t0 + ms(100));
        assert_eq!(
            v.newly_yielded,
            vec!["A".to_string()],
            "different value is the operator's"
        );
    }

    #[test]
    fn an_echo_of_an_earlier_send_still_matches_after_a_newer_one() {
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["A"]);
        t.note_send(DIM, &f(0.2), t0);
        t.note_send(DIM, &f(0.4), t0 + ms(20));
        assert_eq!(
            t.observe(&mut h, &[cmd(DIM, 0.2)], false, t0 + ms(50)),
            Verdict::default()
        );
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn wheel_properties_match_any_value_in_the_window() {
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["A"]);
        let gobo = "/fixture/A/overrides/goboWheel";
        t.note_send(gobo, &f(3.0), t0);
        assert_eq!(
            t.observe(&mut h, &[cmd(gobo, 3.4)], false, t0 + ms(60)),
            Verdict::default()
        );
    }

    #[test]
    fn unrelated_property_change_does_not_yield_a_held_fixture() {
        // O-02: replay only ever sent the dimmer; Lightkey's own chase moves the pan.
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["A"]);
        t.note_send(DIM, &f(0.5), t0);
        let v = t.observe(
            &mut h,
            &[cmd("/fixture/A/overrides/panAngle", 40.0)],
            false,
            t0 + ms(2000),
        );
        assert_eq!(v, Verdict::default());
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn fixture_names_match_ignoring_case_and_percent_encoding() {
        // O-05
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["Moving Head 1"]);
        t.note_send("/fixture/Moving Head 1/overrides/dimmer", &f(0.5), t0);
        let v = t.observe(
            &mut h,
            &[cmd("/fixture/moving%20head%201/overrides/dimmer", 0.9)],
            false,
            t0 + ms(1000),
        );
        assert_eq!(v.newly_yielded, vec!["Moving Head 1".to_string()]);
        assert!(t.is_yielded("moving head 1"));
        assert_eq!(t.yielded_count(), 1);
    }

    #[test]
    fn unheld_fixtures_and_keyframes_are_ignored() {
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["B"]);
        t.note_send("/fixture/B/overrides/dimmer", &f(0.0), t0);
        assert_eq!(
            t.observe(&mut h, &[cmd(DIM, 1.0)], false, t0 + ms(1000)),
            Verdict::default()
        );
        assert_eq!(
            t.observe(
                &mut h,
                &[cmd("/fixture/B/overrides/dimmer", 1.0)],
                true,
                t0 + ms(1000)
            ),
            Verdict::default()
        );
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn holdoff_after_a_stream_reset() {
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["A"]);
        t.note_send(DIM, &f(0.0), t0);
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
        t.note_send(DIM, &f(0.0), t0 - Duration::from_secs(5));
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
