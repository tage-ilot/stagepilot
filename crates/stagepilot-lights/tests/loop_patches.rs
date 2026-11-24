//! Loops with patches: a chase that repeats MOSTLY cleanly is one loop plus small corrections.
//!
//! Part 1 plays hand-made patches through the evaluator and the replay engine: a patch plays in
//! place of the loop for its span, begins and ends on the loop's own value, can be seeked into,
//! is an instant for the scheduler, and the operator can take the fixture over inside it.
//! Part 2 fits synthetic songs (a 16-fixture chase with an extra brightening, a pulse dropped
//! once, a fixture that joins late, a slow drift of the level, a one-off hit between cycles) and
//! checks that each becomes ONE loop with patches, replays within the tolerance, and is far
//! smaller than the plain ramps.

use stagepilot_core::config::{
    EffectMember, EffectPatch, EffectPatchStep, EffectStep, LightingEffect, OscArgument,
    OscCueSource, OscLightingCue, OscMessage,
};
use stagepilot_lights::lightkey_curve::Timeline;
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand};
use stagepilot_lights::lightkey_replay::{is_smooth, simulate, ReplayEngine};
use stagepilot_lights::lightkey_smooth::{
    fit, reconstruct_report, smooth_rows, SmoothCapture, SmoothParams, LEVEL_TOLERANCE,
};
use uuid::Uuid;

const LEAD: f64 = 0.045;
const A: &str = "/fixture/A/overrides/dimmer";
const B: &str = "/fixture/B/overrides/dimmer";

fn no_one(_: f64, _: &str) -> bool {
    false
}

fn num(args: &[FixtureArg]) -> f64 {
    match args.first() {
        Some(FixtureArg::Float(x)) => *x,
        other => panic!("{other:?}"),
    }
}

fn at(t: &Timeline, address: &str, seconds: f64) -> f64 {
    num(&t.playback_at(address, seconds).expect("a value"))
}

fn row(at: f64, address: &str, value: f64) -> OscLightingCue {
    OscLightingCue {
        id: Uuid::new_v4(),
        at_seconds: at,
        address: "/stagepilot/look".into(),
        args: vec![],
        label: String::new(),
        color: None,
        messages: vec![OscMessage {
            address: address.into(),
            args: vec![OscArgument::Float(value)],
        }],
        source: OscCueSource::Captured,
        edited: false,
        fade_seconds: 0.0,
    }
}

/// 0 -> 1 -> 0 over 4 s (A), the same one second later (B); window 0..200 s.
fn triangle_loop() -> LightingEffect {
    LightingEffect {
        id: Uuid::new_v4(),
        label: "loop".into(),
        start_seconds: 0.0,
        end_seconds: 200.0,
        period_seconds: 4.0,
        members: vec![
            EffectMember {
                fixture: "A".into(),
                offset_seconds: 0.0,
            },
            EffectMember {
                fixture: "B".into(),
                offset_seconds: 1.0,
            },
        ],
        cycle: vec![
            EffectStep {
                at_seconds: 0.0,
                property: "dimmer".into(),
                value: vec![1.0],
                fade_seconds: 2.0,
            },
            EffectStep {
                at_seconds: 2.0,
                property: "dimmer".into(),
                value: vec![0.0],
                fade_seconds: 2.0,
            },
        ],
        patches: Vec::new(),
        source: OscCueSource::Captured,
        edited: false,
    }
}

fn step(at: f64, value: f64, fade: f64) -> EffectPatchStep {
    EffectPatchStep {
        at_seconds: at,
        value: vec![value],
        fade_seconds: fade,
    }
}

/// A holds 0.9 from 21.0 s to 24.0 s: the loop is at 0.5 at 21.0 (going up) and at 0 at 24.0.
fn patched() -> LightingEffect {
    let mut e = triangle_loop();
    e.patches.push(EffectPatch {
        fixture: "A".into(),
        property: "dimmer".into(),
        from_seconds: 21.0,
        to_seconds: 24.0,
        steps: vec![step(0.0, 0.9, 0.5), step(2.5, 0.0, 0.5)],
    });
    e
}

// ---------------------------------------------------------------------- the evaluator

#[test]
fn a_patch_plays_in_place_of_the_loop_and_the_loop_plays_everywhere_else() {
    let plain = Timeline::new(&[], &[triangle_loop()]);
    let with = Timeline::new(&[], &[patched()]);
    // before and after the patch: the loop
    for t in [3.0, 10.4, 20.9, 24.0, 24.01, 31.7, 150.0] {
        assert_eq!(at(&plain, A, t), at(&with, A, t), "A at {t}");
    }
    // another member and another stretch are never touched
    for t in [21.2, 22.0, 23.5] {
        assert_eq!(at(&plain, B, t), at(&with, B, t), "B at {t}");
    }
    // inside: it starts from the loop's own value and ramps to 0.9 over 0.5 s, then holds, then
    // returns to 0 over the last 0.5 s
    assert!((at(&with, A, 21.0) - at(&plain, A, 21.0)).abs() < 1e-9);
    assert!((at(&with, A, 21.25) - (0.5 + 0.4 * 0.5)).abs() < 1e-9);
    assert!((at(&with, A, 21.5) - 0.9).abs() < 1e-9);
    assert!((at(&with, A, 23.0) - 0.9).abs() < 1e-9);
    assert!((at(&with, A, 23.75) - 0.45).abs() < 1e-9);
    // ... and ends on the loop's value: no jump where the loop plays again
    let before = at(&with, A, 24.0 - 1e-6);
    let after = at(&with, A, 24.0);
    assert!((before - after).abs() < 1e-4, "{before} -> {after}");
}

#[test]
fn state_and_value_agree_and_the_address_stays_driven_by_the_effect_through_a_patch() {
    let with = Timeline::new(&[], &[patched()]);
    for t in [20.0, 21.5, 23.9, 24.5] {
        assert!(with.in_effect(A, t), "in effect at {t}");
        assert!(with.is_moving(A, t));
    }
    let state = with.state_at(22.0);
    let a = num(&state[A]);
    assert!((a - 0.9).abs() < 1e-9);
    assert_eq!(
        num(&with.value_at(A, 22.0).expect("a value")),
        num(&with.playback_at(A, 22.0).expect("a value"))
    );
}

#[test]
fn an_older_loop_without_patches_is_evaluated_as_before() {
    let e = triangle_loop();
    let t = Timeline::new(&[], std::slice::from_ref(&e));
    assert!((at(&t, A, 1.0) - 0.5).abs() < 1e-9);
    assert!((at(&t, A, 3.0) - 0.5).abs() < 1e-9);
    assert!((at(&t, B, 2.0) - 0.5).abs() < 1e-9);
}

/// A loop whose cycle holds an instant step: 0 for 2 s, then 1 at once, then 0 at once at 3 s.
fn blink_loop() -> LightingEffect {
    let mut e = triangle_loop();
    e.members.truncate(1);
    e.cycle = vec![
        EffectStep {
            at_seconds: 0.0,
            property: "dimmer".into(),
            value: vec![0.0],
            fade_seconds: 0.0,
        },
        EffectStep {
            at_seconds: 2.0,
            property: "dimmer".into(),
            value: vec![1.0],
            fade_seconds: 0.0,
        },
    ];
    e
}

#[test]
fn the_edges_and_instant_steps_of_a_patch_are_instants_and_the_loops_own_are_silent_inside() {
    let mut e = blink_loop();
    // from 10.0 to 15.0 the patch holds 0.3 and steps to 0.8 at 12.5 (an instant step)
    e.patches.push(EffectPatch {
        fixture: "A".into(),
        property: "dimmer".into(),
        from_seconds: 10.0,
        to_seconds: 15.0,
        steps: vec![step(0.0, 0.3, 0.0), step(2.5, 0.8, 0.0)],
    });
    let t = Timeline::new(&[], std::slice::from_ref(&e));
    // the loop steps to 1 at 2, 6, 10, 14... and back to 0 at 4, 8, 12, 16 (period 4)
    assert_eq!(at(&t, A, 9.9), 0.0);
    // the patch's start (10.0) is an instant: the step to 1 that the loop would take there is
    // replaced by the patch's own
    assert!(t.instant_step_in(A, 9.99, 10.0));
    assert_eq!(at(&t, A, 10.0), 0.3);
    // the loop's instant at 14.0 (silent inside the patch) is not an instant of the address ...
    assert!(!t.instant_step_in(A, 13.5, 14.5));
    assert_eq!(at(&t, A, 14.2), 0.8);
    // ... the patch's own instant step is
    assert!(t.instant_step_in(A, 12.4, 12.6));
    // the patch's end (15.0) is one too: the loop plays again (it holds 1 since 14.0)
    assert!(t.instant_step_in(A, 14.9, 15.0));
    assert_eq!(at(&t, A, 15.0), 1.0);
    // a scheduler walking the instants hits them in order
    let mut seen = Vec::new();
    let mut cursor = 8.5;
    while let Some(next) = t.next_boundary(cursor) {
        if next > 17.0 {
            break;
        }
        seen.push(next);
        cursor = next;
    }
    assert_eq!(seen, vec![10.0, 12.5, 15.0, 16.0]);
}

#[test]
fn a_patch_edge_inside_the_window_is_never_missed_when_next_boundary_is_asked_from_inside() {
    let t = Timeline::new(&[], &[patched()]);
    // asking from the middle of the patch finds its end
    let next = t.next_boundary(22.0).expect("a boundary");
    assert!(next > 22.0 && next <= 24.0 + 1e-9, "{next}");
}

// ----------------------------------------------------------------------------- the replay

#[test]
fn the_replay_plays_a_patch_from_the_middle_and_all_the_way() {
    let e = patched();
    let rows = vec![row(0.0, A, 0.0)];
    let timeline = Timeline::new(&rows, std::slice::from_ref(&e));
    assert!(is_smooth(&rows, std::slice::from_ref(&e)));
    // seek into the middle of the patch: the first packet is the patch's value there
    let mut engine = ReplayEngine::new(&rows, std::slice::from_ref(&e));
    let packets = simulate(&mut engine, 22.3, 26.0, LEAD, &no_one);
    let mine: Vec<_> = packets.iter().filter(|p| p.message.address == A).collect();
    assert!(!mine.is_empty());
    let first = match mine[0].message.args.first() {
        Some(OscArgument::Float(x)) => *x,
        other => panic!("{other:?}"),
    };
    assert!(
        (first - at(&timeline, A, 22.3 + LEAD)).abs() < 1e-9,
        "{first}"
    );
    assert!((first - 0.9).abs() < 1e-9);
    // every packet equals what the evaluator says at the position it was made for (within the
    // distance a ramp moves in half a tick: 1/0.5 s ramps at 0.9/0.5 per second)
    for p in &mine {
        let v = match p.message.args.first() {
            Some(OscArgument::Float(x)) => *x,
            other => panic!("{other:?}"),
        };
        let want = at(&timeline, A, p.position);
        assert!(
            (v - want).abs() <= 1.8 * 0.05 + 1.0 / 255.0 * 1.5,
            "at {} sent {v}, evaluator {want}",
            p.position
        );
    }
    // and at the end of the patch it has come back to the loop
    let last = mine.last().expect("packets");
    let v = match last.message.args.first() {
        Some(OscArgument::Float(x)) => *x,
        other => panic!("{other:?}"),
    };
    assert!((v - at(&timeline, A, last.position)).abs() < 0.1);
}

#[test]
fn the_operator_can_take_a_fixture_over_inside_a_patch_and_gets_it_back_where_the_song_is() {
    let e = patched();
    let rows = vec![row(0.0, A, 0.0)];
    let timeline = Timeline::new(&rows, std::slice::from_ref(&e));
    let mut engine = ReplayEngine::new(&rows, std::slice::from_ref(&e));
    // the operator has A from 21.5 to 22.5, in the middle of the patch
    let yielded = |now: f64, address: &str| address == A && (21.5..22.5).contains(&now);
    let packets = simulate(&mut engine, 20.0, 26.0, LEAD, &yielded);
    let mine: Vec<_> = packets.iter().filter(|p| p.message.address == A).collect();
    // nothing is sent for A while the operator has it ...
    assert!(
        mine.iter()
            .all(|p| !(21.5 + LEAD + 0.05..22.5 + LEAD - 0.05).contains(&p.position)),
        "sent while yielded"
    );
    // ... and right after, it is back at the song's value (still inside the patch: 0.9)
    let back = mine
        .iter()
        .find(|p| p.position >= 22.5 + LEAD - 0.05)
        .expect("a packet after the take-back");
    let v = match back.message.args.first() {
        Some(OscArgument::Float(x)) => *x,
        other => panic!("{other:?}"),
    };
    assert!((v - at(&timeline, A, back.position)).abs() < 0.05, "{v}");
}

// --------------------------------------------------------------------------- fitting songs

type Changes = Vec<(f64, FixtureCommand)>;

const FIXTURES: usize = 16;
const PERIOD: f64 = 4.0;
const T0: f64 = 0.0;
const T1: f64 = 125.0;
const BASE_PEAK: f64 = 0.6;

fn name(i: usize) -> String {
    format!("P{}", i + 1)
}

fn cmd(fixture: &str, v: f64) -> FixtureCommand {
    FixtureCommand::new(
        format!("/fixture/{fixture}/overrides/dimmer"),
        vec![FixtureArg::Float(v)],
    )
}

/// A triangle pulse 0.5 s wide of height `peak` centred on `centre`.
fn pulse(t: f64, centre: f64, peak: f64) -> f64 {
    (peak * (1.0 - (t - centre).abs() / 0.25)).max(0.0)
}

/// The chase: fixture `i` pulses once per period, `i * period / 16` after fixture 0. `shape`
/// gives (fixture, cycle number, time) the pulse height and may add to the value.
fn chase(shape: impl Fn(usize, usize, f64, f64) -> f64) -> Changes {
    let mut changes = Changes::new();
    for i in 0..FIXTURES {
        let n = ((T1 - T0) / 0.025).round() as usize;
        for k in 0..=n {
            let t = T0 + k as f64 * 0.025;
            let phase = (t - T0 - i as f64 * PERIOD / FIXTURES as f64).rem_euclid(PERIOD);
            let cycle = ((t - T0 - i as f64 * PERIOD / FIXTURES as f64) / PERIOD)
                .floor()
                .max(0.0) as usize;
            let base = pulse(phase, 0.5, BASE_PEAK) + pulse(phase - PERIOD, 0.5, BASE_PEAK);
            let v = shape(i, cycle, t, base);
            changes.push((t, cmd(&name(i), v.clamp(0.0, 1.0))));
        }
    }
    changes
}

fn run(changes: &Changes, detect: bool) -> SmoothCapture {
    fit(
        &[],
        changes,
        T1 + 1.0,
        &SmoothParams {
            detect_effects: detect,
            ..SmoothParams::default()
        },
    )
}

struct Outcome {
    capture: SmoothCapture,
    /// Rows' messages + cycle steps + patch steps, and the messages of the plain fit.
    items: usize,
    plain: usize,
}

fn fitted(changes: &Changes) -> Outcome {
    let capture = run(changes, true);
    let plain = run(changes, false)
        .rows
        .iter()
        .map(|r| r.messages.len())
        .sum();
    let patch_steps: usize = capture
        .effects
        .iter()
        .flat_map(|e| &e.patches)
        .map(|p| p.steps.len())
        .sum();
    let items = capture.rows.iter().map(|r| r.messages.len()).sum::<usize>()
        + capture.effects.iter().map(|e| e.cycle.len()).sum::<usize>()
        + patch_steps;
    Outcome {
        capture,
        items,
        plain,
    }
}

fn patch_count(capture: &SmoothCapture) -> usize {
    capture.effects.iter().map(|e| e.patches.len()).sum()
}

fn members(capture: &SmoothCapture) -> usize {
    capture.effects.iter().map(|e| e.members.len()).sum()
}

/// The fitted song, rebuilt, against the recording: the share of points further than twice the
/// tolerance (at most 2 % is the acceptance) and the largest error.
fn rebuilt(changes: &Changes, capture: &SmoothCapture) -> (f64, f64) {
    let report = reconstruct_report(
        &[],
        changes,
        T1 + 1.0,
        &smooth_rows(&capture.rows),
        &capture.effects,
        1.0,
    );
    let overall = &report.overall[0];
    (overall.over_double, overall.max)
}

fn level(capture: &SmoothCapture, fixture: usize, at_seconds: f64) -> f64 {
    let t = Timeline::new(&smooth_rows(&capture.rows), &capture.effects);
    num(&t
        .value_at(
            &format!("/fixture/{}/overrides/dimmer", name(fixture)),
            at_seconds,
        )
        .expect("a value"))
}

/// The time fixture `i` peaks in cycle `c` of the clean chase.
fn peak_time(i: usize, c: usize) -> f64 {
    T0 + i as f64 * PERIOD / FIXTURES as f64 + c as f64 * PERIOD + 0.5
}

/// `smaller`: the song must come to at most `1 / smaller` of the plain messages.
fn assert_one_loop_with_patches(
    o: &Outcome,
    changes: &Changes,
    min_patches: usize,
    smaller: usize,
) {
    let capture = &o.capture;
    assert_eq!(
        capture.effects.len(),
        1,
        "{:?}",
        capture
            .effect_infos
            .iter()
            .map(|i| (i.period_seconds, i.members.len(), i.patches))
            .collect::<Vec<_>>()
    );
    assert_eq!(members(capture), FIXTURES);
    assert!(
        patch_count(capture) >= min_patches,
        "{} patches",
        patch_count(capture)
    );
    let (over_double, max) = rebuilt(changes, capture);
    assert!(
        over_double <= 0.02,
        "{over_double} over twice the tolerance"
    );
    assert!(
        max <= 6.0 * LEVEL_TOLERANCE,
        "largest error {max} ({} tolerances)",
        max / LEVEL_TOLERANCE
    );
    assert!(
        o.items * smaller <= o.plain,
        "{} items against {} plain messages",
        o.items,
        o.plain
    );
}

#[test]
fn a_clean_chase_is_still_one_loop_with_no_patches() {
    let changes = chase(|_, _, _, base| base);
    let o = fitted(&changes);
    assert_eq!(o.capture.effects.len(), 1);
    assert_eq!(members(&o.capture), FIXTURES);
    assert_eq!(patch_count(&o.capture), 0);
    assert!(o.capture.effects[0].cycle.len() <= 6);
    let (over_double, _) = rebuilt(&changes, &o.capture);
    assert!(over_double <= 0.02);
}

#[test]
fn a_chase_with_an_extra_brightening_in_one_cycle_is_one_loop_and_a_few_patches() {
    // every fixture pulses to full in its 10th cycle instead of 0.6
    let changes = chase(|_, cycle, t, base| {
        if cycle == 10 {
            base / BASE_PEAK
        } else {
            let _ = t;
            base
        }
    });
    let o = fitted(&changes);
    assert_one_loop_with_patches(&o, &changes, 1, 3);
    // the brighter cycle plays at full where the chase peaked
    for i in [0usize, 7, 15] {
        let v = level(&o.capture, i, peak_time(i, 10));
        assert!(v > 0.9, "fixture {i}: {v}");
        let normal = level(&o.capture, i, peak_time(i, 9));
        assert!(
            (normal - BASE_PEAK).abs() < 3.0 * LEVEL_TOLERANCE,
            "{normal}"
        );
    }
}

#[test]
fn a_chase_where_every_fifth_cycle_is_brighter_is_one_loop_with_a_patch_for_each() {
    // 6 of 30 cycles: more than the 2 % a clean loop may miss, so it used to stay plain ramps
    let changes = chase(|_, cycle, _, base| {
        if cycle % 5 == 4 {
            base / BASE_PEAK
        } else {
            base
        }
    });
    let o = fitted(&changes);
    assert_one_loop_with_patches(&o, &changes, 6, 2);
}

#[test]
fn a_pulse_dropped_once_is_a_patch_that_plays_nothing() {
    let changes = chase(|i, cycle, _, base| {
        if (i == 4 && cycle == 7) || (i == 11 && cycle == 20) {
            0.0
        } else {
            base
        }
    });
    let o = fitted(&changes);
    assert_one_loop_with_patches(&o, &changes, 2, 3);
    assert!(level(&o.capture, 4, peak_time(4, 7)) < 2.0 * LEVEL_TOLERANCE);
    assert!(level(&o.capture, 11, peak_time(11, 20)) < 2.0 * LEVEL_TOLERANCE);
    // the neighbours of the dropped pulses still play
    assert!((level(&o.capture, 4, peak_time(4, 8)) - BASE_PEAK).abs() < 3.0 * LEVEL_TOLERANCE);
}

#[test]
fn a_fixture_that_joins_late_stays_in_the_loop_with_a_patch_for_the_time_it_was_dark() {
    let changes = chase(|i, _, t, base| if i == 15 && t < 60.0 { 0.0 } else { base });
    let o = fitted(&changes);
    assert_one_loop_with_patches(&o, &changes, 1, 3);
    // dark while it was late, playing after
    assert!(level(&o.capture, 15, peak_time(15, 5)) < 2.0 * LEVEL_TOLERANCE);
    assert!((level(&o.capture, 15, peak_time(15, 20)) - BASE_PEAK).abs() < 3.0 * LEVEL_TOLERANCE);
}

#[test]
fn a_slow_drift_of_the_level_is_followed_by_loops_and_patches_not_given_up() {
    // the pulses grow from 0.5 to 0.7 over the song: the loop plays the middle, patches follow
    // the drift away from it (or a loop of its own takes each end); never plain ramps
    let changes = chase(|_, _, t, base| {
        let grow = 0.5 + 0.2 * (t - T0) / (T1 - T0);
        base / BASE_PEAK * grow
    });
    let o = fitted(&changes);
    let capture = &o.capture;
    assert!(!capture.effects.is_empty() && capture.effects.len() <= 3);
    assert!(capture.effects.iter().all(|e| e.members.len() == FIXTURES));
    assert!(
        patch_count(capture) >= 1,
        "{} patches",
        patch_count(capture)
    );
    let (over_double, max) = rebuilt(&changes, capture);
    assert!(
        over_double <= 0.02,
        "{over_double} over twice the tolerance"
    );
    assert!(max <= 6.0 * LEVEL_TOLERANCE, "largest error {max}");
    assert!(
        o.items * 2 <= o.plain,
        "{} items against {}",
        o.items,
        o.plain
    );
}

#[test]
fn a_one_off_hit_between_cycles_is_a_patch() {
    // fixture 2 flashes to 0.8 for half a second at 50.4 s, where its loop is dark
    let changes = chase(|i, _, t, base| {
        if i == 2 && (50.4..50.9).contains(&t) {
            base.max(0.8)
        } else {
            base
        }
    });
    let o = fitted(&changes);
    assert_one_loop_with_patches(&o, &changes, 1, 3);
    assert!(level(&o.capture, 2, 50.65) > 0.7);
}

#[test]
fn patches_never_jump_where_they_start_or_end() {
    let changes = chase(|_, cycle, _, base| {
        if cycle % 5 == 4 {
            base / BASE_PEAK
        } else {
            base
        }
    });
    let o = fitted(&changes);
    let effect = &o.capture.effects[0];
    assert!(!effect.patches.is_empty());
    let t = Timeline::new(&[], std::slice::from_ref(effect));
    for p in &effect.patches {
        let address = format!("/fixture/{}/overrides/{}", p.fixture, p.property);
        let (before, start) = (
            num(&t
                .value_at(&address, p.from_seconds - 1e-4)
                .expect("a value")),
            num(&t.value_at(&address, p.from_seconds).expect("a value")),
        );
        assert!(
            (before - start).abs() <= LEVEL_TOLERANCE,
            "{address}: {before} -> {start} entering at {}",
            p.from_seconds
        );
        let (inside, after) = (
            num(&t.value_at(&address, p.to_seconds - 1e-4).expect("a value")),
            num(&t.value_at(&address, p.to_seconds).expect("a value")),
        );
        assert!(
            (inside - after).abs() <= LEVEL_TOLERANCE,
            "{address}: {inside} -> {after} leaving at {}",
            p.to_seconds
        );
    }
}

#[test]
fn a_recording_that_does_not_repeat_gets_no_loop_and_no_patches() {
    // a pseudo-random walk per fixture: nothing repeats
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut changes = Changes::new();
    for i in 0..4 {
        let mut v = 0.5f64;
        for k in 0..=2000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            v = (v + ((seed % 1000) as f64 / 1000.0 - 0.5) * 0.08).clamp(0.0, 1.0);
            changes.push((1.0 + k as f64 * 0.05, cmd(&name(i), v)));
        }
    }
    let capture = run(&changes, true);
    assert!(capture.effects.is_empty(), "{:?}", capture.effect_infos);
}
