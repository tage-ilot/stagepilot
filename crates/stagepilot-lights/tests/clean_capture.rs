//! The capture is cleaned before it is fitted (`lightkey_smooth::clean_changes`, used by
//! `lightkey_fit::fit_within_caps`): a chase recorded with arrival-time smear and flash-backs
//! becomes one loop instead of tens of thousands of rows, and the loop starts at its first
//! matching cycle. Everything here is synthetic; no real rig data is in the repository.

use stagepilot_lights::lightkey_curve::Timeline;
use stagepilot_lights::lightkey_fit::{fit_within_caps, Caps, Size};
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand};
use stagepilot_lights::lightkey_smooth::{
    clean_changes, fit, is_continuous, smooth_rows, SmoothParams,
};

type Changes = Vec<(f64, FixtureCommand)>;

const PERIOD: f64 = 4.0;
const FIXTURES: usize = 16;
/// Phase rank of fixture P1..P16 (a scrambled order, like a real chase patch).
const RANK: [usize; FIXTURES] = [0, 13, 1, 11, 6, 12, 7, 2, 3, 4, 10, 15, 5, 14, 9, 8];

/// Small deterministic generator (no dependency).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as f64) / f64::from(1u32 << 31)
    }
}

/// One pulse of 4.0 s: rise 0.4 s, high 1.2 s, fall 1.0 s, dark 1.4 s.
fn pulse(phase: f64) -> f64 {
    let p = phase.rem_euclid(PERIOD);
    if p < 0.4 {
        p / 0.4
    } else if p < 1.6 {
        1.0
    } else if p < 2.6 {
        1.0 - (p - 1.6)
    } else {
        0.0
    }
}

fn address(n: usize) -> String {
    format!("/fixture/P{}/overrides/color", n + 1)
}

fn truth(n: usize, t: f64, start: f64) -> f64 {
    if t < start {
        0.0
    } else {
        pulse(t - start - RANK[n] as f64 * 0.2)
    }
}

/// The recording of the chase as the rig delivers it: 40 Hz frames, 8-bit values, a frame now
/// and then held back (delivered 0-5 ms before the next) or bunched (delivered 0-5 ms after the
/// previous one): arrival smear, so over a third of the events arrive within 0-5 ms of the
/// previous one of their address, and about as many frames are early as late. Plus bursts where an older
/// stream is flushed just before a live frame, so that older values flash back for up to 20 ms.
fn noisy_chase(seconds: f64, start: f64, noisy: bool) -> Changes {
    let mut rng = Rng(0x5eed_c0de);
    let ms = |t: f64| (t * 1000.0).round() / 1000.0;
    let mut out: Changes = Vec::new();
    for n in 0..FIXTURES {
        let a = address(n);
        let put = |out: &mut Changes, at: f64, v: f64| {
            out.push((
                ms(at),
                FixtureCommand::new(a.clone(), vec![FixtureArg::Float(v); 3]),
            ));
        };
        // the start state: dark
        put(&mut out, 0.0, 0.0);
        let frames = (seconds / 0.025).round() as usize;
        let value =
            |k: usize| (truth(n, start + 0.0125 + k as f64 * 0.025, start) * 255.0).round() / 255.0;
        let (mut held, mut last) = (false, start);
        for k in 0..=frames {
            let nominal = start + 0.0125 + k as f64 * 0.025;
            let r = rng.next();
            let at = if !noisy {
                nominal
            } else if r < 0.1 && !held {
                // held back: delivered 0-5 ms before the next frame
                held = true;
                nominal + 0.025 - rng.next() * 0.005
            } else if r < 0.2 {
                // bunched: delivered 0-5 ms after the previous event
                held = false;
                last + rng.next() * 0.005
            } else {
                held = false;
                nominal + rng.next() * 0.002
            }
            .max(last);
            last = at;
            put(&mut out, at, value(k));
            if noisy && k >= 12 && rng.next() < 0.1 {
                // an older stream flushed just before the live frame: 3-6 stale values (the
                // frames six back) 1-2 ms apart, which flash for up to 12 ms
                let m = 3 + (rng.next() * 4.0) as usize;
                let mut t = at;
                for j in 0..m {
                    t -= 0.001 + rng.next() * 0.001;
                    put(&mut out, t, value(k - 6 - j % 3));
                }
            }
        }
        // the show ends: dark
        put(&mut out, start + seconds + 0.1, 0.0);
    }
    out.sort_by(|x, y| x.0.total_cmp(&y.0));
    out
}

/// Share of 25 ms samples in `[from, to)` where the rebuilt value is within `limit` of the truth.
fn share_within(timeline: &Timeline, start: f64, from: f64, to: f64, limit: f64) -> (f64, usize) {
    let (mut good, mut total) = (0usize, 0usize);
    let mut t = from;
    while t < to {
        for n in 0..FIXTURES {
            total += 1;
            let want = truth(n, t, start);
            let have = timeline
                .value_at(&address(n), t)
                .and_then(|v| match v.first() {
                    Some(FixtureArg::Float(f)) => Some(*f),
                    _ => None,
                });
            if have.is_some_and(|h| (h - want).abs() <= limit) {
                good += 1;
            }
        }
        t += 0.025;
    }
    (good as f64 / total.max(1) as f64, total)
}

fn circular(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(PERIOD);
    d.min(PERIOD - d)
}

#[test]
fn a_noisy_chase_becomes_one_loop_and_the_lead_in_stays_rows() {
    let (start, seconds, duration) = (3.5, 350.0, 360.0);
    let raw = noisy_chase(seconds, start, true);
    assert!(raw.len() > 100_000, "a real-sized recording: {}", raw.len());
    // the capture is as smeared as the real one: many events within 5 ms of the previous
    let times: Vec<f64> = raw
        .iter()
        .filter(|(_, c)| c.a == address(0))
        .map(|(t, _)| *t)
        .collect();
    let bunched = times.windows(2).filter(|w| w[1] - w[0] < 0.005).count();
    assert!(
        bunched as f64 / times.len() as f64 > 0.3,
        "{bunched} of {} events bunched",
        times.len()
    );

    // before: the raw recording does not become a loop and is far over 1,000 rows
    let before = fit(&[], &raw, duration, &SmoothParams::default());
    let before_size = Size::of_capture(&before);
    assert!(before.effects.is_empty(), "raw noise defeats the loop");
    assert!(before_size.rows > 1_000, "{} rows", before_size.rows);

    // after: one loop of 16 members and a handful of rows
    let fitted = fit_within_caps(&[], &raw, duration, &Caps::default());
    let size = Size::of_capture(&fitted.capture);
    assert_eq!(fitted.capture.effects.len(), 1, "{size:?}");
    let effect = &fitted.capture.effects[0];
    assert_eq!(effect.members.len(), FIXTURES);
    assert!((effect.period_seconds - PERIOD).abs() < 0.005);
    assert!(size.rows <= 60, "{} rows", size.rows);
    assert_eq!(fitted.raw_changes, raw.len(), "the raw count is reported");

    // the loop starts where the chase does, not at 0.0; the lead-in before it stays rows
    assert!(
        effect.start_seconds >= start - 0.05 && effect.start_seconds < start + PERIOD,
        "loop starts at {}",
        effect.start_seconds
    );
    assert!(fitted
        .capture
        .rows
        .iter()
        .any(|r| r.at_seconds < effect.start_seconds && r.at_seconds >= start));

    // members keep their phase (compared with the first member, the truth is 0.2 s steps)
    let phase = |fixture: &str| {
        effect
            .members
            .iter()
            .find(|m| m.fixture == fixture)
            .map(|m| m.offset_seconds)
            .expect("member")
    };
    for (n, rank) in RANK.iter().enumerate().skip(1) {
        let truth_gap = (*rank as f64 - RANK[0] as f64) * 0.2;
        let got_gap = phase(&format!("P{}", n + 1)) - phase("P1");
        assert!(
            circular(got_gap, truth_gap) <= 0.02,
            "P{}: offset gap {got_gap} against {truth_gap}",
            n + 1
        );
    }

    // the timeline rebuilt from rows and loop is the truth, the first 10 s included
    let timeline = Timeline::new(&smooth_rows(&fitted.capture.rows), &fitted.capture.effects);
    let (all, _) = share_within(&timeline, start, 0.0, start + seconds, 0.02);
    let (first, _) = share_within(&timeline, start, 0.0, 10.0, 0.02);
    eprintln!(
        "raw {} changes; before: {} rows, {} loops; after: {} rows, {} messages, {} loop(s), \
         {} members, loop {:.3}-{:.3} s; within 0.02: {:.4} (first 10 s {:.4})",
        raw.len(),
        before_size.rows,
        before.effects.len(),
        size.rows,
        size.messages,
        fitted.capture.effects.len(),
        effect.members.len(),
        effect.start_seconds,
        effect.end_seconds,
        all,
        first
    );
    assert!(all >= 0.99, "{:.4} of samples within 0.02", all);
    assert!(first >= 0.99, "{:.4} of the first 10 s within 0.02", first);
}

#[test]
fn a_clean_chase_also_starts_its_loop_at_the_first_matching_cycle() {
    // no noise: only the lead-in is in the way (dark for 3.5 s, then the chase)
    let (start, seconds) = (3.5, 60.0);
    let raw = noisy_chase(seconds, start, false);
    let capture = fit(&[], &raw, 70.0, &SmoothParams::default());
    assert_eq!(capture.effects.len(), 1);
    let effect = &capture.effects[0];
    assert!(
        effect.start_seconds >= start - 0.05 && effect.start_seconds < start + PERIOD,
        "loop starts at {}",
        effect.start_seconds
    );
    let timeline = Timeline::new(&smooth_rows(&capture.rows), &capture.effects);
    let (share, _) = share_within(&timeline, start, 0.0, start + seconds, 0.02);
    assert!(share >= 0.99, "{share:.4}");
}

fn dimmer(a: &str, at: f64, v: f64) -> (f64, FixtureCommand) {
    (
        at,
        FixtureCommand::new(
            format!("/fixture/{a}/overrides/dimmer"),
            vec![FixtureArg::Float(v)],
        ),
    )
}

fn values_of(changes: &Changes, a: &str) -> Vec<(f64, f64)> {
    changes
        .iter()
        .filter(|(_, c)| c.a.ends_with(a))
        .map(|(t, c)| match c.v[0] {
            FixtureArg::Float(f) => (*t, f),
            _ => panic!("not a float"),
        })
        .collect()
}

#[test]
fn a_step_a_two_frame_pulse_and_the_final_value_survive() {
    let mut changes: Changes = Vec::new();
    // frames of 25 ms; a step at 5.0 s, a 50 ms pulse at 8.0 s, a one-frame flash at 9.0 s
    for k in 0..=480 {
        let t = 2.0 + k as f64 * 0.025;
        let v = if t >= 5.0 - 1e-9 { 0.8 } else { 0.1 };
        let v = if (8.0 - 1e-9..8.05 - 1e-9).contains(&t) {
            1.0
        } else {
            v
        };
        let v = if (9.0 - 1e-9..9.025 - 1e-9).contains(&t) {
            0.0
        } else {
            v
        };
        changes.push(dimmer("A", (t * 1000.0).round() / 1000.0, v));
    }
    let cleaned = clean_changes(&changes, 20.0);
    let series = values_of(&cleaned, "dimmer");
    assert!(series.len() < changes.len() / 10, "{} kept", series.len());
    // the step: still there, at its own time
    assert!(series
        .iter()
        .any(|(t, v)| (*t - 5.0).abs() < 0.026 && (*v - 0.8).abs() < 1e-9));
    // the two-frame pulse survives, the one-frame flash goes
    assert!(series
        .iter()
        .any(|(t, v)| (*t - 8.0).abs() < 0.06 && (*v - 1.0).abs() < 1e-9));
    assert!(!series
        .iter()
        .any(|(t, v)| (*t - 9.0).abs() < 0.06 && *v < 0.5));
    // the first and the final value stay as recorded
    assert_eq!(series.first(), Some(&(2.0, 0.1)));
    assert_eq!(series.last(), Some(&(14.0, 0.8)));
}

#[test]
fn the_final_value_stays_even_when_it_is_a_flash() {
    let changes: Changes = vec![
        dimmer("A", 1.0, 0.2),
        dimmer("A", 1.025, 0.2),
        dimmer("A", 1.05, 0.2),
        dimmer("A", 1.075, 0.9),
    ];
    let cleaned = clean_changes(&changes, 10.0);
    assert_eq!(cleaned.last(), Some(&dimmer("A", 1.075, 0.9)));
}

#[test]
fn discrete_properties_are_never_touched() {
    assert!(is_continuous("/fixture/P1/overrides/color", 3));
    assert!(is_continuous("/fixture/MH1/overrides/panAngle", 1));
    assert!(is_continuous("/fixture/MH1/overrides/zoom", 1));
    assert!(!is_continuous("/fixture/MH1/overrides/gobo", 1));
    assert!(!is_continuous(
        "/fixture/MH1/overrides/goboRotationSpeed",
        1
    ));
    assert!(!is_continuous("/fixture/MH1/overrides/shutter", 1));
    assert!(!is_continuous("/fixture/MH1/overrides/prismType", 1));
    assert!(!is_continuous("/fixture/MH1/overrides/macro", 1));
    assert!(!is_continuous("/fixture/MH1/overrides/color2", 1));

    let mut changes: Changes = Vec::new();
    let mut rng = Rng(7);
    for k in 0..400 {
        let t = 1.0 + k as f64 * 0.004;
        let flip = rng.next() < 0.5;
        for (property, value) in [
            ("gobo", FixtureArg::Float(if flip { 3.0 } else { 4.0 })),
            ("shutter", FixtureArg::Float(if flip { 0.0 } else { 1.0 })),
            ("color2", FixtureArg::Int(i64::from(flip))),
            ("lit", FixtureArg::Bool(flip)),
            ("name", FixtureArg::Text(format!("slot {}", u8::from(flip)))),
        ] {
            changes.push((
                (t * 1000.0).round() / 1000.0,
                FixtureCommand::new(format!("/fixture/D/overrides/{property}"), vec![value]),
            ));
        }
        // a continuous address with a changing argument count is left alone as well
        changes.push((
            (t * 1000.0).round() / 1000.0,
            FixtureCommand::new(
                "/fixture/D/overrides/dimmer",
                vec![FixtureArg::Float(0.5); 1 + usize::from(flip)],
            ),
        ));
    }
    assert_eq!(clean_changes(&changes, 10.0), changes);
}

#[test]
fn the_input_recording_is_not_changed() {
    let raw = noisy_chase(12.0, 1.0, true);
    let copy = raw.clone();
    let cleaned = clean_changes(&raw, 20.0);
    assert_eq!(raw, copy);
    assert!(cleaned.len() < raw.len());
}
