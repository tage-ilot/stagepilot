//! Offline check for a false yield: the replay judging its own fast sweep to be the operator.
//! No sockets, no sleeping: the engine replays a plan on a simulated clock, every packet it sends
//! goes to the [`OperatorTracker`] as a send at its wall time, and the DMX changes a rig recorded
//! go to it as relay events. Used by the `false_yield_report` example and the regression test.

use crate::lightkey_fixtures::{fixture_of, FixtureCommand};
use crate::lightkey_operator::{normalise_fixture, EchoRules, OperatorTracker, ECHO_WINDOW};
use crate::lightkey_replay::{simulate, ReplayEngine};
use crate::plugin::LOOK_LEAD_SECONDS;
use stagepilot_core::config::{LightingEffect, OscArgument, OscLightingCue};
use std::collections::BTreeSet;
use std::time::Duration;
use tokio::time::Instant;

/// When the test played, in epoch milliseconds, and where the song was.
#[derive(Debug, Clone)]
pub struct Timeline {
    /// Wall time of song position 0.
    pub start_ms: f64,
    /// Song position of the first pause.
    pub pause1_s: f64,
    /// Song position jumped to.
    pub jump_to_s: f64,
    /// Wall time of the resume after the jump.
    pub resume_ms: f64,
    /// Wall time of the second pause.
    pub pause2_ms: f64,
    /// Relay events in the first seconds of each pass are not judged: the start of a pass
    /// (connect, first look, re-sync, release of unused fixtures) is paced by things this
    /// harness does not model.
    pub settle_s: f64,
    /// Pass 2 is judged only this long after the resume. After a real yield the replay would
    /// have carried on differently (the 30 s take-back); an open-loop run cannot model that.
    pub pass2_judge_s: f64,
    /// Delay of a relay event after the DMX time stamp (OSC out, Lightkey, sACN, helper, relay).
    pub relay_lag_ms: f64,
}

impl Timeline {
    /// The 2026-11-08 "Elohim" test (UTC 20:11:51.946 start, 37.5 s pause, jump to 182.7 s,
    /// resume 20:12:33.2, pause 20:12:56.4).
    pub fn elohim() -> Self {
        Self {
            start_ms: 1_794_168_711_946.0,
            pause1_s: 37.5,
            jump_to_s: 182.7,
            resume_ms: 1_794_168_753_200.0,
            pause2_ms: 1_794_168_776_400.0,
            settle_s: 2.0,
            pass2_judge_s: 15.0,
            relay_lag_ms: 100.0,
        }
    }
}

/// A packet the engine sent, at its wall time.
#[derive(Debug, Clone)]
pub struct Sent {
    pub wall_ms: f64,
    pub address: String,
    pub args: Vec<OscArgument>,
}

/// The first event that flagged a fixture.
#[derive(Debug, Clone)]
pub struct Flag {
    pub fixture: String,
    /// Wall time, seconds after the song start.
    pub wall_s: f64,
    pub second_pass: bool,
    pub command: FixtureCommand,
    /// The replay's latest sends on that address before the event: (ms before, arguments).
    pub sends: Vec<(f64, Vec<OscArgument>)>,
}

#[derive(Debug, Default)]
pub struct Report {
    /// Fixture packets of pass 1 and pass 2 (the plan values; they do not depend on the detector).
    pub pass_packets: [usize; 2],
    pub sent: Vec<Sent>,
    pub relay_events: usize,
    pub held: BTreeSet<String>,
    pub flags: Vec<Flag>,
    /// Fixtures the operator already had that the operator touched again: (fixture, wall
    /// seconds after the harness's start time).
    pub restarts: Vec<(String, f64)>,
}

impl Report {
    pub fn flagged(&self) -> BTreeSet<String> {
        self.flags.iter().map(|f| f.fixture.clone()).collect()
    }
}

/// Replay `rows`/`effects` through both passes of `timeline` and judge `relay` (wall time in epoch
/// ms, command) against the sends.
pub fn run(
    rows: &[OscLightingCue],
    effects: &[LightingEffect],
    relay: &[(f64, FixtureCommand)],
    timeline: &Timeline,
) -> Report {
    let t = timeline;
    let pass2_len = (t.pause2_ms - t.resume_ms) / 1000.0;
    let mut report = Report::default();
    for (n, (from, to, wall0, song0)) in [
        (0.0, t.pause1_s, t.start_ms, 0.0),
        (
            t.jump_to_s,
            t.jump_to_s + pass2_len,
            t.resume_ms,
            t.jump_to_s,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut engine = ReplayEngine::new(rows, effects);
        for p in simulate(&mut engine, from, to, LOOK_LEAD_SECONDS, &|_, _| false) {
            if p.message.address.starts_with("/fixture/") {
                report.pass_packets[n] += 1;
                report.sent.push(Sent {
                    wall_ms: wall0 + (p.at - song0) * 1000.0,
                    address: p.message.address,
                    args: p.message.args,
                });
            }
        }
    }

    let settle = t.settle_s * 1000.0;
    let mut events: Vec<(f64, &FixtureCommand)> = Vec::new();
    for (ms, cmd) in relay {
        let inside = (*ms >= t.start_ms + settle && *ms <= t.start_ms + t.pause1_s * 1000.0)
            || (*ms >= t.resume_ms + settle && *ms <= t.resume_ms + t.pass2_judge_s * 1000.0);
        if inside {
            events.push((ms + t.relay_lag_ms, cmd));
        }
    }
    let base = Instant::now();
    let at = |ms: f64| base + Duration::from_secs_f64(((ms - t.start_ms) / 1000.0 + 10.0).max(0.0));
    judge(
        &mut report,
        events,
        OperatorTracker::new(),
        t.start_ms,
        t.resume_ms,
        &at,
    );
    report
}

/// Feed the sends in `report` and the relay `events` (wall time in ms, already delayed by the
/// relay lag) to `tracker` in time order and note every fixture it yields.
fn judge(
    report: &mut Report,
    events: Vec<(f64, &FixtureCommand)>,
    mut tracker: OperatorTracker,
    start_ms: f64,
    resume_ms: f64,
    at: &dyn Fn(f64) -> Instant,
) {
    enum Item<'a> {
        Send(&'a Sent),
        Relay(&'a FixtureCommand),
    }
    let sent = std::mem::take(&mut report.sent);
    let mut items: Vec<(f64, Item)> = sent.iter().map(|s| (s.wall_ms, Item::Send(s))).collect();
    items.extend(events.into_iter().map(|(ms, cmd)| (ms, Item::Relay(cmd))));
    // A send at the same millisecond goes first.
    items.sort_by(|x, y| {
        x.0.partial_cmp(&y.0)
            .unwrap()
            .then_with(|| matches!(y.1, Item::Send(_)).cmp(&matches!(x.1, Item::Send(_))))
    });

    let mut held: BTreeSet<String> = BTreeSet::new();
    let mut flags = Vec::new();
    let mut restarts = Vec::new();
    for (ms, item) in &items {
        match item {
            Item::Send(s) => {
                if let Some(f) = fixture_of(&s.address) {
                    held.insert(f.to_string());
                    report.held.insert(f.to_string());
                }
                tracker.note_send(&s.address, &s.args, at(*ms));
            }
            Item::Relay(cmd) => {
                report.relay_events += 1;
                let verdict = tracker.observe(&mut held, std::slice::from_ref(cmd), false, at(*ms));
                for fixture in verdict.restarted {
                    restarts.push((fixture, (ms - start_ms) / 1000.0));
                }
                for fixture in verdict.newly_yielded {
                    let norm = normalise_fixture(&cmd.a);
                    let sends = sent
                        .iter()
                        .rev()
                        .filter(|s| normalise_fixture(&s.address) == norm && s.wall_ms <= *ms)
                        .take(4)
                        .map(|s| (s.wall_ms - ms, s.args.clone()))
                        .collect();
                    flags.push(Flag {
                        fixture,
                        wall_s: (ms - start_ms) / 1000.0,
                        second_pass: *ms >= resume_ms,
                        command: (*cmd).clone(),
                        sends,
                    });
                }
            }
        }
    }
    report.flags = flags;
    report.restarts = restarts;
    report.sent = sent;
}

/// One start of a replay, for songs whose ring covers only the seconds around the start: the
/// engine begins at song position `from_s` (a play, a seek, or the take-back of a fixture)
/// and every relay event in the ring is judged against what it sent.
#[derive(Debug, Clone)]
pub struct StartCase {
    /// Song position the engine starts at.
    pub from_s: f64,
    /// How long it runs, song seconds.
    pub length_s: f64,
    /// Wall time (epoch ms) of the engine's first tick.
    pub engine_wall_ms: f64,
    /// Delay of a relay event after its DMX time stamp.
    pub relay_lag_ms: f64,
    /// The relay's state before the first send: (address, value) the ring does not show.
    pub baseline: Vec<(String, f64)>,
    /// Which echo rules the tracker uses.
    pub rules: EchoRules,
}

/// Replay `rows`/`effects` from `case.from_s` and judge every event of `relay` (wall time in
/// epoch ms, DMX time stamp) against the sends. Events before the first send of a property only
/// teach the tracker where the fixture is.
pub fn run_start(
    rows: &[OscLightingCue],
    effects: &[LightingEffect],
    relay: &[(f64, FixtureCommand)],
    case: &StartCase,
) -> Report {
    let mut report = Report::default();
    let mut engine = ReplayEngine::new(rows, effects);
    let to = case.from_s + case.length_s;
    for p in simulate(&mut engine, case.from_s, to, LOOK_LEAD_SECONDS, &|_, _| {
        false
    }) {
        if p.message.address.starts_with("/fixture/") {
            report.pass_packets[0] += 1;
            report.sent.push(Sent {
                wall_ms: case.engine_wall_ms + (p.at - case.from_s) * 1000.0,
                address: p.message.address,
                args: p.message.args,
            });
        }
    }
    let events: Vec<(f64, &FixtureCommand)> = relay
        .iter()
        .map(|(ms, cmd)| (ms + case.relay_lag_ms, cmd))
        .collect();
    let base = Instant::now();
    let origin = case.engine_wall_ms;
    let at = |ms: f64| base + Duration::from_secs_f64(((ms - origin) / 1000.0 + 10.0).max(0.0));
    let mut tracker = OperatorTracker::with_rules(case.rules);
    for (address, value) in &case.baseline {
        tracker.seed_observed(address, *value);
    }
    judge(&mut report, events, tracker, origin, f64::INFINITY, &at);
    report
}

/// The echo window the report refers to, for printing.
pub fn echo_window_ms() -> u128 {
    ECHO_WINDOW.as_millis()
}
