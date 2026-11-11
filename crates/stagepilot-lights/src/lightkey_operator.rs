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
//!
//! Three more echoes cover the first look of a song, a seek and a take-back (F-H2):
//! - **Baseline.** The first value sent to a property has no earlier sent value. The last value
//!   the relay reported for it (the fixture's state before our send) stands in for it, so the
//!   path rule below works from the first send on.
//! - **Stale frame.** The relay can still report the old position for a moment after the first
//!   send of a different value. A change equal to that baseline within [`FIRST_SEND_WINDOW`]
//!   of the first send is the echo, not the operator.
//! - **Glide.** A moving head that has to travel more than [`GLIDE_MIN_MOVE`] of its range
//!   needs up to a second and settles with a small overshoot. For [`GLIDE_WINDOW`] after such
//!   a send, a value inside the stretch from the old value to the new one, widened by the
//!   overshoot margin, is the echo.

use crate::lightkey_fixtures::{fixture_of, FixtureArg, FixtureCommand};
use stagepilot_core::config::OscArgument;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::Duration;
use tokio::time::Instant;

/// A relay change this soon after a replay send of the same fixture property is that send coming
/// back, never the operator.
pub const ECHO_WINDOW: Duration = Duration::from_millis(200);
/// A change equal to the value a property had before the replay's first send of a different value
/// is that old position being reported late, never the operator, for this long after the send.
pub const FIRST_SEND_WINDOW: Duration = Duration::from_millis(500);
/// A send that moves a head by more than [`GLIDE_MIN_MOVE`] of the property's range is echoed
/// by values on the way (and the small overshoot) for this long.
pub const GLIDE_WINDOW: Duration = Duration::from_millis(1500);
/// Share of the range a send must move a position property to start a glide window.
pub const GLIDE_MIN_MOVE: f64 = 0.05;
/// After the relay stream was (re)started its state is unknown: changes are not judged for this
/// long, so a reconnect can never make every fixture look operator-driven.
pub const STREAM_HOLDOFF: Duration = Duration::from_secs(3);
/// Remembered sends beyond this are pruned (older than the glide window first).
const MAX_RECENT: usize = 4096;
/// Observed relay values beyond this are forgotten (a rig has about 2,000).
const MAX_OBSERVED: usize = 50_000;

/// Which echo rules are on. `current()` is the shipped behaviour; `rc10()` is the behaviour
/// before F-H2 (path rule only), kept so the offline checks can show what the new rules change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EchoRules {
    /// R1/R2: the relay's last value is the previous value of a first send, and a change equal
    /// to it shortly after that send is the echo.
    pub baseline: bool,
    /// R3: the long glide window with the overshoot margin for position properties.
    pub glide: bool,
    /// Two values of an angle are the same within the DMX resolution: an 8-bit pan moves in
    /// steps of 2.47 degrees, so a sent value is reported a step away, more than the 1 degree
    /// the earlier rule allowed.
    pub resolution: bool,
}

impl EchoRules {
    pub const fn current() -> Self {
        Self {
            baseline: true,
            glide: true,
            resolution: true,
        }
    }

    pub const fn rc10() -> Self {
        Self {
            baseline: false,
            glide: false,
            resolution: false,
        }
    }
}

impl Default for EchoRules {
    fn default() -> Self {
        Self::current()
    }
}

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

/// How far apart two values of a property can be and still be the same value. Angles: 1 degree,
/// and with `resolution` at least half a percent of the travel (an 8-bit pan of 630 degrees
/// moves in steps of 2.47 degrees, a tilt of 270 in steps of 1.06). Everything else 2 %.
fn tolerance(address: &str, resolution: bool) -> f64 {
    if address.to_lowercase().ends_with("angle") {
        match position_scale(address) {
            Some((range, _)) if resolution => 1.0_f64.max(RESOLUTION_SHARE * range),
            _ => 1.0,
        }
    } else {
        0.02
    }
}

/// Coarsest DMX step of an 8-bit channel as a share of its travel, rounded up.
const RESOLUTION_SHARE: f64 = 0.005;

/// Full travel of a moving-head position property, and the least overshoot margin, in the
/// property's own unit. `None` for everything that is not a position (dimmer, colour, wheels).
/// Pan and tilt are the usual 540 and 270 degrees; the zoom angle is a beam angle of a few
/// tens of degrees; focus and a plain zoom are 0..1.
fn position_scale(address: &str) -> Option<(f64, f64)> {
    let a = address.to_lowercase();
    let property = a.rsplit('/').next().unwrap_or(&a);
    match property {
        "panangle" => Some((540.0, 2.0)),
        "tiltangle" => Some((270.0, 2.0)),
        "zoomangle" => Some((90.0, 2.0)),
        p if p.ends_with("angle") => Some((360.0, 2.0)),
        "focus" | "zoom" => Some((1.0, 0.0)),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct Sent {
    at: Instant,
    /// The value, when the send had exactly one numeric argument.
    value: Option<f64>,
    /// The value the replay had sent to this address before this one (however long ago), when
    /// that was a single number too. A DMX value between the two is on the way Lightkey took
    /// from the old value to the new one. For the first send it is the relay's last value.
    from: Option<f64>,
    /// Nothing had been sent to this address before (replay start, seek, take-back): the first
    /// look goes out as a burst and comes back late.
    first: bool,
    /// The relay's last value when this was the first send (nothing sent before) of a value
    /// that differs from it: the old position, which may still be reported for a moment.
    baseline: Option<f64>,
    /// The stretch a glide covers (old value to new, widened by the overshoot margin), when
    /// this send moved a position property by more than [`GLIDE_MIN_MOVE`] of its range.
    glide: Option<(f64, f64)>,
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
    /// Lower-cased address -> the last single number the replay sent there.
    last_value: HashMap<String, f64>,
    /// Lower-cased address -> the last single number the relay reported there (the fixture's
    /// state before the replay's first send, and the old position of a glide).
    observed: HashMap<String, f64>,
    /// Fixture -> time of the operator's latest change on it.
    yielded: BTreeMap<String, Instant>,
    ignore_until: Option<Instant>,
    rules: EchoRules,
}

impl OperatorTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// A tracker with other echo rules (the offline checks compare with [`EchoRules::rc10`]).
    pub fn with_rules(rules: EchoRules) -> Self {
        Self {
            rules,
            ..Self::default()
        }
    }

    /// The relay's value for `address`, learned from outside the change stream (the relay's
    /// state at the first send): kept only where no change has been seen for it.
    pub fn seed_observed(&mut self, address: &str, value: f64) {
        if self.observed.len() < MAX_OBSERVED {
            self.observed
                .entry(normalise_fixture(address))
                .or_insert(value);
        }
    }

    /// True when a first send to `address` would have no baseline: nothing sent to it and no
    /// relay value known.
    pub fn needs_baseline(&self, address: &str) -> bool {
        let key = normalise_fixture(address);
        self.rules.baseline
            && !self.last_value.contains_key(&key)
            && !self.observed.contains_key(&key)
    }

    /// Replay sent `address` with `args` now.
    pub fn note_send(&mut self, address: &str, args: &[OscArgument], now: Instant) {
        if self.recent.len() >= MAX_RECENT {
            self.recent.retain(|_, sends| {
                sends.retain(|s| now.saturating_duration_since(s.at) <= GLIDE_WINDOW);
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
        let value = osc_number(args);
        let earlier = match value {
            Some(v) => self.last_value.insert(key.clone(), v),
            None => self.last_value.remove(&key),
        };
        // The first value sent to a property (replay start, seek, take-back) has no earlier
        // sent value: the relay's last value is where the fixture was.
        let first = earlier.is_none() && self.rules.baseline;
        let seen_before = if first {
            value.and_then(|_| self.observed.get(&key).copied())
        } else {
            None
        };
        let from = earlier.or(seen_before);
        let baseline = match (value, seen_before) {
            (Some(v), Some(b)) if (v - b).abs() > tolerance(address, self.rules.resolution) => {
                Some(b)
            }
            _ => None,
        };
        let glide = match (self.rules.glide, value, from, position_scale(address)) {
            (true, Some(v), Some(f), Some((range, least)))
                if (v - f).abs() > GLIDE_MIN_MOVE * range =>
            {
                let travel = (v - f).abs();
                let margin = least.max(0.03 * range).max(0.1 * travel);
                Some((v.min(f) - margin, v.max(f) + margin))
            }
            _ => None,
        };
        let sends = self.recent.entry(key).or_default();
        sends.retain(|s| now.saturating_duration_since(s.at) <= GLIDE_WINDOW);
        sends.push(Sent {
            at: now,
            value,
            from,
            first,
            baseline,
            glide,
        });
    }

    /// The replay no longer holds this fixture: forget which properties it had sent.
    pub fn forget_sent(&mut self, fixture: &str) {
        if let Some(properties) = self.sent_properties.remove(&normalise_fixture(fixture)) {
            for property in properties {
                self.last_value.remove(&property);
            }
        }
    }

    fn is_echo(&self, command: &FixtureCommand, now: Instant) -> bool {
        let Some(sends) = self.recent.get(&normalise_fixture(&command.a)) else {
            return false;
        };
        let seen = fixture_number(&command.v);
        let wheel = is_wheel_like(&command.a);
        let tol = tolerance(&command.a, self.rules.resolution);
        // A head is slower than a dimmer: it reports a position it was sent up to half a second
        // later (it travels, or the plan sends nothing for a while on a slow ramp, and a fast
        // ramp is 2 degrees per frame).
        let slow = self.rules.glide && position_scale(&command.a).is_some();
        sends.iter().any(|s| {
            let age = now.saturating_duration_since(s.at);
            // The first look of a song is a burst of up to 150 values: Lightkey works through
            // it for a while, so its echoes come later than those of a steady stream.
            let recent = age
                <= if slow || s.first {
                    FIRST_SEND_WINDOW
                } else {
                    ECHO_WINDOW
                };
            match (s.value, seen) {
                (Some(sent), Some(seen)) if !wheel => {
                    // The value itself, or any value on the way from the one sent before it
                    // (Lightkey passes through those when a fast move is smoothed, and a
                    // sweep faster than the 50 ms tick shows values the replay never sent).
                    (recent
                        && ((sent - seen).abs() <= tol
                            || s.from.is_some_and(|from| {
                                seen >= from.min(sent) - tol && seen <= from.max(sent) + tol
                            })))
                        // The old position, reported late after the first send.
                        || (age <= FIRST_SEND_WINDOW
                            && s.baseline.is_some_and(|b| (seen - b).abs() <= tol))
                        // A long travel of a head, and its overshoot.
                        || (age <= GLIDE_WINDOW
                            && s.glide.is_some_and(|(lo, hi)| seen >= lo && seen <= hi))
                }
                _ => recent,
            }
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

    /// What the relay says now is where the fixture is: the baseline of the next first send.
    /// Learned from every change, judged or not, whatever the replay is doing.
    pub fn learn(&mut self, commands: &[FixtureCommand]) {
        for command in commands {
            if let Some(x) = fixture_number(&command.v) {
                if self.observed.len() >= MAX_OBSERVED {
                    self.observed.clear();
                }
                self.observed.insert(normalise_fixture(&command.a), x);
            }
        }
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
        self.learn(commands);
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
                if !verdict.restarted.contains(&key) {
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

    /// Replay has the fixture again. Its next send is a first send: what it last sent is not
    /// where the operator left it, so the relay's last value becomes the baseline.
    pub fn take_back(&mut self, fixture: &str) -> bool {
        let taken = self
            .yielded_key(fixture)
            .is_some_and(|key| self.yielded.remove(&key).is_some());
        if taken {
            self.forget_last_values(fixture);
        }
        taken
    }

    /// The fixture's next send of each property is a first send.
    fn forget_last_values(&mut self, fixture: &str) {
        if let Some(properties) = self.sent_properties.get(&normalise_fixture(fixture)) {
            for property in properties {
                self.last_value.remove(property);
            }
        }
    }

    /// A replay (re)starts, resumes or seeks: every send is a first send again.
    pub fn restart_sends(&mut self) {
        self.last_value.clear();
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

    const PAN: &str = "/fixture/MH3/overrides/panAngle";

    #[test]
    fn a_fast_sweep_of_135_degrees_in_0_15_s_on_a_held_fixture_does_not_yield() {
        // The replay holds the head at 112 degrees and sweeps it to -23 in three sends 50 ms
        // apart; Lightkey (and the 40 Hz DMX frames) pass through values it never sent.
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["MH3"]);
        t.note_send(PAN, &f(112.4), t0);
        t.note_send(PAN, &f(70.0), t0 + ms(1000));
        t.note_send(PAN, &f(26.0), t0 + ms(1050));
        t.note_send(PAN, &f(-23.5), t0 + ms(1100));
        for (after, seen) in [
            (1030, 90.0),
            (1055, 70.4),
            (1075, 38.3),
            (1100, 11.1),
            (1125, -6.2),
            (1150, -23.5),
        ] {
            let v = t.observe(&mut h, &[cmd(PAN, seen)], false, t0 + ms(after));
            assert_eq!(v, Verdict::default(), "{seen} at +{after} ms");
        }
        assert_eq!(h.len(), 1, "still held");
    }

    #[test]
    fn a_value_outside_the_path_of_the_sweep_is_still_the_operator() {
        // Same sweep; the operator turns the pan to 160, which is not between 112.4 and 70.
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["MH3"]);
        t.note_send(PAN, &f(112.4), t0);
        t.note_send(PAN, &f(70.0), t0 + ms(1000));
        let v = t.observe(&mut h, &[cmd(PAN, 160.0)], false, t0 + ms(1040));
        assert_eq!(v.newly_yielded, vec!["MH3".to_string()]);
        assert!(h.is_empty());
    }

    #[test]
    fn a_real_operator_move_during_a_replay_yields_the_fixture() {
        // The head is held at 40 degrees, the replay keeps sending the same value every 50 ms;
        // the operator moves it to 100 and nothing the replay sent is near that.
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["MH3"]);
        for i in 0..20 {
            t.note_send(PAN, &f(40.0), t0 + ms(i * 50));
        }
        let v = t.observe(&mut h, &[cmd(PAN, 100.0)], false, t0 + ms(990));
        assert_eq!(v.newly_yielded, vec!["MH3".to_string()]);
        assert!(t.is_yielded("MH3"));
    }

    #[test]
    fn the_path_rule_does_not_reach_back_past_the_echo_window() {
        // A step from 0.1 to 0.9: 0.5 is on the path for 200 ms, the operator's afterwards.
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        let mut h = held(&["A"]);
        t.note_send(DIM, &f(0.1), t0);
        t.note_send(DIM, &f(0.9), t0 + ms(1000));
        assert_eq!(
            t.observe(&mut h, &[cmd(DIM, 0.5)], false, t0 + ms(1100)),
            Verdict::default()
        );
        let v = t.observe(&mut h, &[cmd(DIM, 0.5)], false, t0 + ms(1400));
        assert_eq!(v.newly_yielded, vec!["A".to_string()]);
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

    // ---- F-H2: the first look of a song, a seek and a take-back ---------------------------

    const TILT: &str = "/fixture/MH1/overrides/tiltAngle";
    const Z1_PAN: &str = "/fixture/Z1/overrides/panAngle";

    fn tracker_for(rules: EchoRules, state: &[(&str, f64)]) -> OperatorTracker {
        let mut t = OperatorTracker::with_rules(rules);
        t.learn(
            &state
                .iter()
                .map(|(a, x)| cmd(a, *x))
                .collect::<Vec<FixtureCommand>>(),
        );
        t
    }

    #[test]
    fn a_stale_frame_of_the_old_position_after_the_first_send_is_an_echo() {
        // Elohim from 234 s: the head stood at tilt 90.529, the first look sends 75.706, and
        // the relay still reports 90.529 once, 250 ms later.
        let t0 = Instant::now();
        let mut t = tracker_for(EchoRules::current(), &[(TILT, 90.5294)]);
        let mut h = held(&["MH1"]);
        t.note_send(TILT, &f(75.7059), t0);
        assert_eq!(
            t.observe(&mut h, &[cmd(TILT, 90.5294)], false, t0 + ms(250)),
            Verdict::default()
        );
        assert_eq!(
            t.observe(&mut h, &[cmd(TILT, 75.7059)], false, t0 + ms(300)),
            Verdict::default()
        );
        assert_eq!(h.len(), 1);
        // (14.8 degrees is 5.5 % of the tilt range, so the glide window covers it for 1.5 s.)
        let v = t.observe(&mut h, &[cmd(TILT, 90.5294)], false, t0 + ms(1600));
        assert_eq!(v.newly_yielded, vec!["MH1".to_string()]);
    }

    #[test]
    fn the_stale_frame_of_a_small_move_is_an_echo_for_half_a_second_only() {
        // Ever Be from 175 s, Z2: tilt 90.0 reported once after a send of 68.0 ... as a small
        // move the baseline rule alone: 500 ms.
        let t0 = Instant::now();
        let tilt = "/fixture/Z2/overrides/tiltAngle";
        let mut t = tracker_for(EchoRules::current(), &[(tilt, 70.0)]);
        let mut h = held(&["Z2"]);
        t.note_send(tilt, &f(68.0), t0);
        assert_eq!(
            t.observe(&mut h, &[cmd(tilt, 70.0)], false, t0 + ms(450)),
            Verdict::default()
        );
        let v = t.observe(&mut h, &[cmd(tilt, 70.0)], false, t0 + ms(550));
        assert_eq!(v.newly_yielded, vec!["Z2".to_string()]);
    }

    #[test]
    fn the_stale_frame_is_the_operator_without_the_new_rules() {
        let t0 = Instant::now();
        let mut t = tracker_for(EchoRules::rc10(), &[(TILT, 90.5294)]);
        let mut h = held(&["MH1"]);
        t.note_send(TILT, &f(75.7059), t0);
        let v = t.observe(&mut h, &[cmd(TILT, 90.5294)], false, t0 + ms(250));
        assert_eq!(v.newly_yielded, vec!["MH1".to_string()]);
    }

    #[test]
    fn a_value_that_is_not_the_old_position_after_the_first_send_is_still_the_operator() {
        let t0 = Instant::now();
        let mut t = tracker_for(EchoRules::current(), &[(TILT, 90.5294)]);
        let mut h = held(&["MH1"]);
        t.note_send(TILT, &f(75.7059), t0);
        // 120 is outside the stretch 75.7..90.5 (even widened) and is not the baseline
        let v = t.observe(&mut h, &[cmd(TILT, 120.0)], false, t0 + ms(300));
        assert_eq!(v.newly_yielded, vec!["MH1".to_string()]);
    }

    #[test]
    fn nothing_known_about_the_fixture_gives_no_baseline() {
        let t0 = Instant::now();
        let mut t = OperatorTracker::new();
        assert!(t.needs_baseline(TILT));
        t.seed_observed(TILT, 90.0);
        assert!(!t.needs_baseline(TILT));
        t.seed_observed(TILT, 12.0); // a value from the change stream is not overwritten
        let mut h = held(&["MH1"]);
        t.note_send(TILT, &f(75.0), t0);
        assert_eq!(
            t.observe(&mut h, &[cmd(TILT, 90.0)], false, t0 + ms(300)),
            Verdict::default()
        );
        // a baseline equal to the value sent is no old position at all
        let mut t = tracker_for(EchoRules::current(), &[(TILT, 75.0)]);
        let mut h = held(&["MH1"]);
        t.note_send(TILT, &f(75.0), t0);
        let v = t.observe(&mut h, &[cmd(TILT, 60.0)], false, t0 + ms(300));
        assert_eq!(v.newly_yielded, vec!["MH1".to_string()]);
    }

    #[test]
    fn a_long_head_move_is_echoed_by_the_glide_and_its_overshoot_for_one_and_a_half_seconds() {
        // The head stood at pan -250 and is sent to -170: 80 degrees, 15 % of 540.
        let t0 = Instant::now();
        let mut t = tracker_for(EchoRules::current(), &[(PAN, -250.0)]);
        let mut h = held(&["MH3"]);
        t.note_send(PAN, &f(-170.0), t0);
        for (after, seen) in [
            (300, -230.0),
            (900, -171.7),  // arrived
            (1000, -174.2), // settles back and forth
            (1100, -165.0), // overshoot, inside the margin (16.2 degrees)
            (1450, -200.0),
        ] {
            let v = t.observe(&mut h, &[cmd(PAN, seen)], false, t0 + ms(after));
            assert_eq!(v, Verdict::default(), "{seen} at +{after} ms");
        }
        // beyond the overshoot margin
        let v = t.observe(&mut h, &[cmd(PAN, -140.0)], false, t0 + ms(1000));
        assert_eq!(v.newly_yielded, vec!["MH3".to_string()]);
    }

    #[test]
    fn after_one_and_a_half_seconds_the_normal_rule_applies_again() {
        let t0 = Instant::now();
        let mut t = tracker_for(EchoRules::current(), &[(PAN, -250.0)]);
        let mut h = held(&["MH3"]);
        t.note_send(PAN, &f(-170.0), t0);
        let v = t.observe(&mut h, &[cmd(PAN, -200.0)], false, t0 + ms(1600));
        assert_eq!(v.newly_yielded, vec!["MH3".to_string()]);
    }

    #[test]
    fn the_glide_window_restarts_with_each_new_send() {
        let t0 = Instant::now();
        let mut t = tracker_for(EchoRules::current(), &[(PAN, -250.0)]);
        let mut h = held(&["MH3"]);
        t.note_send(PAN, &f(-170.0), t0);
        t.note_send(PAN, &f(-60.0), t0 + ms(1200)); // 110 degrees on
                                                    // 1.3 s after the second send, 2.5 s after the first: -100 is on the way to -60
        assert_eq!(
            t.observe(&mut h, &[cmd(PAN, -100.0)], false, t0 + ms(2500)),
            Verdict::default()
        );
        // ... and 1.6 s after it the rule is the normal one
        let v = t.observe(&mut h, &[cmd(PAN, -100.0)], false, t0 + ms(2900));
        assert_eq!(v.newly_yielded, vec!["MH3".to_string()]);
    }

    #[test]
    fn a_small_move_gets_no_glide_window() {
        // 10 degrees is 1.9 % of the pan range: the path rule only, and for a head 500 ms.
        let t0 = Instant::now();
        let mut t = tracker_for(EchoRules::current(), &[(PAN, -250.0)]);
        let mut h = held(&["MH3"]);
        t.note_send(PAN, &f(-240.0), t0);
        assert_eq!(
            t.observe(&mut h, &[cmd(PAN, -245.0)], false, t0 + ms(400)),
            Verdict::default()
        );
        let v = t.observe(&mut h, &[cmd(PAN, -245.0)], false, t0 + ms(800));
        assert_eq!(v.newly_yielded, vec!["MH3".to_string()]);
    }

    #[test]
    fn the_glide_needs_a_position_property() {
        // A dimmer that jumps 0 to 1 is not a head: 0.5 is the operator's after 200 ms.
        let t0 = Instant::now();
        let mut t = tracker_for(EchoRules::current(), &[(DIM, 0.0)]);
        let mut h = held(&["A"]);
        t.note_send(DIM, &f(1.0), t0);
        let v = t.observe(&mut h, &[cmd(DIM, 0.5)], false, t0 + ms(700));
        assert_eq!(v.newly_yielded, vec!["A".to_string()]);
    }

    #[test]
    fn focus_and_zoom_are_positions_too() {
        let t0 = Instant::now();
        let focus = "/fixture/MH1/overrides/focus";
        let mut t = tracker_for(EchoRules::current(), &[(focus, 0.0)]);
        let mut h = held(&["MH1"]);
        t.note_send(focus, &f(1.0), t0);
        assert_eq!(
            t.observe(&mut h, &[cmd(focus, 0.6)], false, t0 + ms(900)),
            Verdict::default()
        );
    }

    #[test]
    fn an_8_bit_pan_step_is_the_same_value() {
        // The rig's pan moves in steps of 2.47 degrees: a sent -155.83 is reported as -154.41.
        let t0 = Instant::now();
        let mut now = tracker_for(EchoRules::current(), &[]);
        let mut old = tracker_for(EchoRules::rc10(), &[]);
        let mut h = held(&["MH4"]);
        let mut h2 = held(&["MH4"]);
        for t in [&mut now, &mut old] {
            t.note_send(PAN.replace("MH3", "MH4").as_str(), &f(-159.35), t0);
            t.note_send(
                PAN.replace("MH3", "MH4").as_str(),
                &f(-155.83),
                t0 + ms(300),
            );
        }
        let pan4 = PAN.replace("MH3", "MH4");
        assert_eq!(
            now.observe(&mut h, &[cmd(&pan4, -154.41)], false, t0 + ms(330)),
            Verdict::default()
        );
        let v = old.observe(&mut h2, &[cmd(&pan4, -154.41)], false, t0 + ms(330));
        assert_eq!(v.newly_yielded, vec!["MH4".to_string()]);
    }

    #[test]
    fn a_take_back_makes_the_next_send_a_first_send_from_where_the_operator_left_it() {
        let t0 = Instant::now();
        let mut t = tracker_for(EchoRules::current(), &[]);
        let mut h = held(&["MH1"]);
        t.note_send(TILT, &f(40.0), t0);
        let v = t.observe(&mut h, &[cmd(TILT, 100.0)], false, t0 + ms(900));
        assert_eq!(v.newly_yielded, vec!["MH1".to_string()]);
        assert!(t.take_back("MH1"));
        // the replay sends 40 again: the old last value (40) is forgotten, the relay says 100
        let t1 = t0 + Duration::from_secs(31);
        h.insert("MH1".into());
        t.note_send(TILT, &f(40.0), t1);
        // 100 reported late is the operator's old position, not a new operator
        assert_eq!(
            t.observe(&mut h, &[cmd(TILT, 100.0)], false, t1 + ms(300)),
            Verdict::default()
        );
        // and so is a value on the way (60 degrees of travel: a glide)
        assert_eq!(
            t.observe(&mut h, &[cmd(TILT, 70.0)], false, t1 + ms(900)),
            Verdict::default()
        );
    }

    #[test]
    fn a_restart_of_the_replay_forgets_the_last_sent_values() {
        let t0 = Instant::now();
        let mut t = tracker_for(EchoRules::current(), &[(TILT, 10.0)]);
        let mut h = held(&["MH1"]);
        t.note_send(TILT, &f(40.0), t0);
        t.restart_sends();
        t.learn(&[cmd(TILT, 95.0)]);
        t.note_send(TILT, &f(40.0), t0 + Duration::from_secs(60));
        assert_eq!(
            t.observe(
                &mut h,
                &[cmd(TILT, 95.0)],
                false,
                t0 + Duration::from_secs(60) + ms(200)
            ),
            Verdict::default()
        );
    }

    #[test]
    fn events_off_the_path_after_the_window_are_the_operator_exactly_as_before() {
        // R6: the head is sent to 40 and then keeps getting 40; an event at 100 is the operator's
        // 1.6 s after the long move and 2 s into the quiet stretch, as in rc.10.
        for rules in [EchoRules::current(), EchoRules::rc10()] {
            let t0 = Instant::now();
            let mut t = tracker_for(rules, &[(PAN, -250.0)]);
            let mut h = held(&["MH3"]);
            t.note_send(PAN, &f(-170.0), t0);
            for i in 1..40 {
                t.note_send(PAN, &f(-170.0), t0 + ms(i * 50));
            }
            let v = t.observe(&mut h, &[cmd(PAN, -100.0)], false, t0 + ms(2000));
            assert_eq!(v.newly_yielded, vec!["MH3".to_string()], "{rules:?}");
        }
    }

    #[test]
    fn a_hand_set_pan_of_minus_20_then_plus_20_yields_at_once_and_is_handed_back_after_30_s() {
        // Rig test 2026-11-08: Z1.panAngle -20.0 at +191.7 s and +19.99 at +192.9 s; the plan holds
        // Z1 at -89.996 for the whole song.
        let t0 = Instant::now();
        let z1 = Z1_PAN;
        let mut t = tracker_for(EchoRules::current(), &[(z1, -89.996)]);
        let mut h = held(&["Z1"]);
        t.note_send(z1, &f(-89.996), t0);
        for i in 1..=3800 {
            t.note_send(z1, &f(-89.996), t0 + ms(i * 50));
        }
        let first = t0 + ms(191_700);
        let v = t.observe(&mut h, &[cmd(z1, -20.0)], false, first);
        assert_eq!(
            v.newly_yielded,
            vec!["Z1".to_string()],
            "at the first event"
        );
        assert!(h.is_empty());
        let second = t0 + ms(192_900);
        let v = t.observe(&mut h, &[cmd(z1, 19.99)], false, second);
        assert_eq!(v.restarted, vec!["Z1".to_string()]);
        let quiet = Duration::from_secs(30);
        assert!(t
            .due(quiet, second + Duration::from_millis(29_900))
            .is_empty());
        assert_eq!(
            t.due(quiet, second + quiet),
            vec!["Z1".to_string()],
            "handed back 30 s after the last change"
        );
        assert!(t.take_back("Z1"));
        assert!(!t.is_yielded("Z1"));
    }
}
