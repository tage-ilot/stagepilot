//! Smooth capture on synthetic recordings: ramps, steps, gaps, effects, and the evaluator the
//! replay is built on. Real takes are checked by the optional test at the bottom (set
//! `STAGEPILOT_SMOOTH_TAKES` to a takes file; skipped otherwise).

use stagepilot_core::config::{
    EffectMember, EffectStep, LightingEffect, OscArgument, OscCueSource, OscLightingCue, OscMessage,
};
use stagepilot_lights::lightkey_curve::{value_at, Timeline};
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand};
use stagepilot_lights::lightkey_smooth::{
    fit, reconstruct_report, smooth_rows, ReconstructReport, SmoothCapture, SmoothParams,
};
use uuid::Uuid;

type Changes = Vec<(f64, FixtureCommand)>;

fn cmd(fixture: &str, property: &str, v: &[f64]) -> FixtureCommand {
    FixtureCommand::new(
        format!("/fixture/{fixture}/overrides/{property}"),
        v.iter().map(|x| FixtureArg::Float(*x)).collect(),
    )
}

/// Samples of `f` every `dt` seconds from `t0` to `t1` (inclusive).
fn signal(
    fixture: &str,
    property: &str,
    t0: f64,
    t1: f64,
    dt: f64,
    mut f: impl FnMut(f64) -> f64,
) -> Changes {
    let n = ((t1 - t0) / dt).round() as usize;
    (0..=n)
        .map(|i| {
            let t = t0 + i as f64 * dt;
            (t, cmd(fixture, property, &[f(t)]))
        })
        .collect()
}

fn triangle(t: f64, period: f64, low: f64, high: f64) -> f64 {
    let p = (t / period).rem_euclid(1.0);
    low + (high - low) * if p < 0.5 { 2.0 * p } else { 2.0 - 2.0 * p }
}

fn run(changes: &Changes, duration: f64, detect: bool) -> SmoothCapture {
    fit(
        &[],
        changes,
        duration,
        &SmoothParams {
            detect_effects: detect,
            ..SmoothParams::default()
        },
    )
}

fn report(changes: &Changes, duration: f64, capture: &SmoothCapture) -> ReconstructReport {
    reconstruct_report(
        &[],
        changes,
        duration,
        &smooth_rows(&capture.rows),
        &capture.effects,
        1.0,
    )
}

fn max_outside(report: &ReconstructReport) -> f64 {
    report
        .outside_effects
        .iter()
        .map(|c| c.max)
        .fold(0.0, f64::max)
}

fn timeline(capture: &SmoothCapture) -> Timeline {
    Timeline::new(&smooth_rows(&capture.rows), &capture.effects)
}

fn level(t: &Timeline, fixture: &str, property: &str, at: f64) -> f64 {
    match t
        .value_at(&format!("/fixture/{fixture}/overrides/{property}"), at)
        .expect("a value")[0]
    {
        FixtureArg::Float(f) => f,
        ref other => panic!("not a number: {other:?}"),
    }
}

// ----------------------------------------------------------------------------------- ramps

#[test]
fn a_linear_ramp_is_one_row() {
    let changes = signal("A", "dimmer", 1.0, 11.0, 0.025, |t| 0.1 + 0.08 * (t - 1.0));
    let capture = run(&changes, 20.0, false);
    // a step to the start value, then one ramp
    assert_eq!(capture.rows.len(), 2, "{:?}", capture.rows);
    assert_eq!(capture.rows[0].fade_seconds, 0.0);
    assert!((capture.rows[1].fade_seconds - 10.0).abs() < 1e-6);
    assert!(max_outside(&report(&changes, 20.0, &capture)) <= 0.03);
}

#[test]
fn an_s_curve_fade_is_a_few_ramps_within_tolerance() {
    let changes = signal("A", "dimmer", 2.0, 8.0, 0.025, |t| {
        let x = ((t - 2.0) / 6.0).clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    });
    let capture = run(&changes, 20.0, false);
    assert!(capture.rows.len() <= 8, "{} rows", capture.rows.len());
    assert!(capture.rows.len() >= 3);
    assert!(max_outside(&report(&changes, 20.0, &capture)) <= 0.03 + 1e-9);
}

#[test]
fn a_step_is_an_instant_row_and_levels_hold_on_both_sides() {
    let mut changes = signal("A", "dimmer", 1.0, 2.0, 0.025, |_| 0.2);
    changes.extend(signal("A", "dimmer", 2.025, 3.0, 0.025, |_| 0.9));
    let capture = run(&changes, 5.0, false);
    let t = timeline(&capture);
    assert!((level(&t, "A", "dimmer", 1.5) - 0.2).abs() < 1e-6);
    assert!((level(&t, "A", "dimmer", 3.5) - 0.9).abs() < 1e-6);
    assert!(capture.rows.len() <= 3, "{:?}", capture.rows);
    assert!(max_outside(&report(&changes, 5.0, &capture)) <= 0.03 + 1e-9);
}

#[test]
fn a_gap_over_half_a_second_is_hold_then_step_never_a_ramp() {
    let mut changes = signal("A", "dimmer", 1.0, 2.0, 0.025, |_| 0.2);
    changes.extend(signal("A", "dimmer", 150.0, 151.0, 0.025, |_| 0.8));
    let capture = run(&changes, 160.0, false);
    // no ramp across the 148 s of silence: every row is an instant step
    assert!(
        capture.rows.iter().all(|r| r.fade_seconds == 0.0),
        "{:?}",
        capture.rows
    );
    let t = timeline(&capture);
    assert!(
        (level(&t, "A", "dimmer", 80.0) - 0.2).abs() < 1e-6,
        "held, not interpolated"
    );
    assert!((level(&t, "A", "dimmer", 150.0) - 0.8).abs() < 1e-6);
}

#[test]
fn a_ramp_is_never_longer_than_sixty_seconds() {
    let changes = signal("A", "dimmer", 0.0, 130.0, 0.1, |t| t / 130.0);
    let capture = run(&changes, 140.0, false);
    assert!(capture.rows.iter().all(|r| r.fade_seconds <= 60.0 + 1e-9));
    assert!(capture.rows.len() >= 3);
    assert!(max_outside(&report(&changes, 140.0, &capture)) <= 0.03 + 1e-9);
}

#[test]
fn options_and_text_step_and_never_ramp() {
    let mut changes: Changes = vec![
        (
            1.0,
            FixtureCommand::new("/fixture/A/overrides/gobo", vec![FixtureArg::Int(1)]),
        ),
        (
            2.0,
            FixtureCommand::new("/fixture/A/overrides/gobo", vec![FixtureArg::Int(3)]),
        ),
        (
            2.1,
            FixtureCommand::new(
                "/fixture/A/overrides/prismType",
                vec![FixtureArg::Text("six".into())],
            ),
        ),
    ];
    changes.extend(signal("A", "dimmer", 1.0, 2.0, 0.025, |t| t - 1.0));
    let capture = run(&changes, 5.0, false);
    let rows = smooth_rows(&capture.rows);
    let t = Timeline::new(&rows, &[]);
    assert_eq!(
        t.value_at("/fixture/A/overrides/gobo", 1.5),
        Some(vec![FixtureArg::Int(1)])
    );
    assert_eq!(
        t.value_at("/fixture/A/overrides/gobo", 2.5),
        Some(vec![FixtureArg::Int(3)])
    );
    assert_eq!(
        t.value_at("/fixture/A/overrides/prismType", 3.0),
        Some(vec![FixtureArg::Text("six".into())])
    );
    for row in &capture.rows {
        for m in &row.messages {
            if m.a.ends_with("gobo") || m.a.ends_with("prismType") {
                assert_eq!(row.fade_seconds, 0.0, "{m:?}");
            }
        }
    }
}

#[test]
fn colour_is_fitted_per_channel_and_properties_of_one_fixture_share_a_row() {
    let n = 200;
    let mut changes: Changes = Vec::new();
    for i in 0..=n {
        let t = 1.0 + i as f64 * 0.025;
        let f = i as f64 / n as f64;
        changes.push((t, cmd("A", "color", &[f, 1.0 - f, 0.5])));
        changes.push((t, cmd("A", "dimmer", &[0.2 + 0.6 * f])));
    }
    let capture = run(&changes, 10.0, false);
    // step row (colour + dimmer), one ramp row (colour + dimmer)
    assert_eq!(capture.rows.len(), 2, "{:?}", capture.rows);
    assert_eq!(capture.rows[1].messages.len(), 2);
    assert!(max_outside(&report(&changes, 10.0, &capture)) <= 0.03 + 1e-9);
}

#[test]
fn identical_rows_of_different_fixtures_share_one_row() {
    let mut changes = Changes::new();
    for fixture in ["P1", "P2", "P3"] {
        changes.extend(signal(fixture, "dimmer", 1.0, 3.0, 0.025, |t| {
            0.1 + 0.3 * (t - 1.0)
        }));
    }
    let capture = run(&changes, 10.0, false);
    assert_eq!(capture.rows.len(), 2, "{:?}", capture.rows);
    assert_eq!(capture.rows[1].messages.len(), 3);
}

#[test]
fn the_start_state_and_a_late_mover_get_a_row_at_zero() {
    let baseline = vec![cmd("A", "dimmer", &[0.4])];
    let changes: Changes = signal("A", "dimmer", 3.0, 4.0, 0.025, |t| 0.4 + 0.5 * (t - 3.0));
    let capture = fit(
        &baseline,
        &changes,
        10.0,
        &SmoothParams {
            detect_effects: false,
            ..Default::default()
        },
    );
    let t = timeline(&capture);
    assert!((level(&t, "A", "dimmer", 1.0) - 0.4).abs() < 1e-6);
    assert!((level(&t, "A", "dimmer", 4.5) - 0.9).abs() < 1e-3);
}

// ---------------------------------------------------------------------------------- effects

fn sweep(period: f64, fixtures: &[(&str, f64)], t0: f64, t1: f64) -> Changes {
    let mut changes = Changes::new();
    for (fixture, offset) in fixtures {
        changes.extend(signal(fixture, "panAngle", t0, t1, 0.025, |t| {
            triangle(t - offset, period, -60.0, 60.0)
        }));
    }
    changes
}

#[test]
fn a_triangle_sweep_on_three_fixtures_with_phase_offsets_is_one_effect() {
    let changes = sweep(7.95, &[("Z1", 0.0), ("Z2", 1.0), ("Z3", 2.0)], 1.0, 120.0);
    let capture = run(&changes, 125.0, true);
    assert_eq!(capture.effects.len(), 1, "{:?}", capture.effect_infos);
    let e = &capture.effects[0];
    assert!(
        (e.period_seconds - 7.95).abs() < 0.02,
        "period {}",
        e.period_seconds
    );
    assert_eq!(e.members.len(), 3);
    let mut offsets: Vec<f64> = e.members.iter().map(|m| m.offset_seconds).collect();
    offsets.sort_by(f64::total_cmp);
    let spread: Vec<f64> = offsets.windows(2).map(|w| w[1] - w[0]).collect();
    // the three fixtures are 1 s apart (the loop wraps, so the gaps sum to the period)
    assert!(
        spread
            .iter()
            .all(|d| (d - 1.0).abs() < 0.06 || (d - (7.95 - 2.0)).abs() < 0.06),
        "{offsets:?}"
    );
    assert!(e.cycle.len() <= 6, "{} steps", e.cycle.len());
    assert!(capture.rows.len() <= 12, "{} rows", capture.rows.len());
    let r = report(&changes, 125.0, &capture);
    assert!(max_outside(&r) <= 1.5 + 1e-9);
    // inside the effect 95 % within 2x tolerance
    let inside = &r.inside_effects[0];
    assert!(inside.over_double <= 0.05, "{inside:?}");
    let overall = &r.overall[0];
    assert!(overall.p99 <= 2.0 * 1.5, "{overall:?}");
}

#[test]
fn a_noisy_sine_is_an_effect_within_the_tolerance() {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        ((seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * 0.02
    };
    let changes = signal("A", "dimmer", 0.0, 100.0, 0.05, |t| {
        0.5 + 0.4 * (std::f64::consts::TAU * t / 5.0).sin() + noise()
    });
    let capture = run(&changes, 100.0, true);
    assert_eq!(capture.effects.len(), 1, "{:?}", capture.effect_infos);
    assert!((capture.effects[0].period_seconds - 5.0).abs() < 0.02);
    let r = report(&changes, 100.0, &capture);
    assert!(
        r.inside_effects[0].over_double <= 0.05,
        "{:?}",
        r.inside_effects
    );
}

#[test]
fn a_slow_fade_is_not_an_effect() {
    let changes = signal("A", "dimmer", 0.0, 200.0, 0.1, |t| 0.5 * t / 200.0);
    let capture = run(&changes, 200.0, true);
    assert!(capture.effects.is_empty(), "{:?}", capture.effect_infos);
    // and a slow back-and-forth with only 1-2 cycles in the song is not one either
    let slow = signal("A", "dimmer", 0.0, 60.0, 0.05, |t| {
        triangle(t, 40.0, 0.0, 1.0)
    });
    assert!(run(&slow, 60.0, true).effects.is_empty());
}

#[test]
fn a_constant_level_is_not_an_effect() {
    let changes = signal("A", "dimmer", 0.0, 100.0, 0.05, |_| 0.5);
    assert!(run(&changes, 100.0, true).effects.is_empty());
}

#[test]
fn an_effect_that_starts_and_stops_mid_song_covers_only_its_window() {
    let mut changes = signal("A", "panAngle", 0.0, 20.0, 0.025, |_| 10.0);
    changes.extend(signal("A", "panAngle", 20.025, 80.0, 0.025, |t| {
        triangle(t, 6.0, -50.0, 50.0)
    }));
    changes.extend(signal("A", "panAngle", 80.025, 100.0, 0.025, |_| -20.0));
    let capture = run(&changes, 100.0, true);
    assert_eq!(capture.effects.len(), 1, "{:?}", capture.effect_infos);
    let e = &capture.effects[0];
    assert!(
        e.start_seconds >= 19.0 && e.start_seconds <= 28.0,
        "start {}",
        e.start_seconds
    );
    assert!(
        e.end_seconds >= 74.0 && e.end_seconds <= 82.0,
        "end {}",
        e.end_seconds
    );
    let r = report(&changes, 100.0, &capture);
    assert!(max_outside(&r) <= 1.5 + 1e-9, "{:?}", r.outside_effects);
    let t = timeline(&capture);
    assert!((level(&t, "A", "panAngle", 10.0) - 10.0).abs() < 1.5);
    assert!((level(&t, "A", "panAngle", 90.0) + 20.0).abs() < 1.5);
}

#[test]
fn an_interrupted_effect_is_found_in_both_halves() {
    let mut changes = signal("A", "panAngle", 0.0, 60.0, 0.025, |t| {
        triangle(t, 5.0, -40.0, 40.0)
    });
    changes.extend(signal("A", "panAngle", 60.025, 75.0, 0.025, |_| 0.0));
    changes.extend(signal("A", "panAngle", 75.025, 140.0, 0.025, |t| {
        triangle(t, 5.0, -40.0, 40.0)
    }));
    let capture = run(&changes, 140.0, true);
    assert!(capture.effects.len() >= 2, "{:?}", capture.effect_infos);
    let r = report(&changes, 140.0, &capture);
    assert!(
        max_outside(&r) <= 1.5 + 1e-9,
        "{:?} {:?}",
        r.outside_effects,
        capture.effect_infos
    );
    // inside an effect up to 2 % of the moments may be off (the acceptance is 98 %)
    assert!(r.overall[0].over_double <= 0.02 + 1e-9, "{:?}", r.overall);
}

#[test]
fn an_effect_with_a_drifting_period_falls_back_to_ramps_within_tolerance() {
    // the period grows 6 % every cycle: no fixed loop matches 90 % of a 3-cycle window
    let mut phase = 0.0f64;
    let mut changes = Changes::new();
    let mut t = 0.0;
    while t <= 120.0 {
        let period = 4.0 + 0.4 * (t / 4.0);
        phase += 0.025 / period;
        changes.push((
            t,
            cmd("A", "panAngle", &[triangle(phase, 1.0, -50.0, 50.0)]),
        ));
        t += 0.025;
    }
    let capture = run(&changes, 120.0, true);
    let r = report(&changes, 120.0, &capture);
    // whatever was or was not detected, the reconstruction must hold
    assert!(max_outside(&r) <= 1.5 + 1e-9, "{:?}", r.outside_effects);
    assert!(r.overall[0].p99 <= 3.0, "{:?}", r.overall);
}

#[test]
fn turning_effect_detection_off_gives_plain_ramps_for_the_same_signal() {
    let changes = sweep(7.95, &[("Z1", 0.0)], 1.0, 60.0);
    let capture = run(&changes, 60.0, false);
    assert!(capture.effects.is_empty());
    assert!(capture.rows.len() > 10);
    assert!(max_outside(&report(&changes, 60.0, &capture)) <= 1.5 + 1e-9);
}

// --------------------------------------------------------------------------- the evaluator

fn row(at: f64, fade: f64, address: &str, args: Vec<OscArgument>) -> OscLightingCue {
    OscLightingCue {
        id: Uuid::new_v4(),
        at_seconds: at,
        address: "/stagepilot/look".into(),
        args: vec![],
        label: String::new(),
        color: None,
        messages: vec![OscMessage {
            address: address.into(),
            args,
        }],
        source: OscCueSource::Captured,
        edited: false,
        fade_seconds: fade,
    }
}

fn f(x: f64) -> Vec<OscArgument> {
    vec![OscArgument::Float(x)]
}

fn num(v: Option<Vec<FixtureArg>>) -> f64 {
    match v.expect("a value").first() {
        Some(FixtureArg::Float(x)) => *x,
        other => panic!("{other:?}"),
    }
}

const DIM: &str = "/fixture/A/overrides/dimmer";

#[test]
fn value_at_ramps_from_the_value_before_the_row() {
    let rows = vec![row(1.0, 0.0, DIM, f(0.2)), row(2.0, 4.0, DIM, f(1.0))];
    assert_eq!(value_at(&rows, &[], DIM, 0.5), None);
    assert!((num(value_at(&rows, &[], DIM, 1.5)) - 0.2).abs() < 1e-9);
    assert!((num(value_at(&rows, &[], DIM, 4.0)) - 0.6).abs() < 1e-9);
    assert!((num(value_at(&rows, &[], DIM, 6.0)) - 1.0).abs() < 1e-9);
    assert!((num(value_at(&rows, &[], DIM, 600.0)) - 1.0).abs() < 1e-9);
}

#[test]
fn a_row_that_starts_inside_a_running_ramp_continues_from_where_it_is() {
    let rows = vec![
        row(0.0, 0.0, DIM, f(0.0)),
        row(1.0, 10.0, DIM, f(1.0)),
        row(6.0, 0.0, DIM, f(0.0)),
    ];
    assert!((num(value_at(&rows, &[], DIM, 5.999)) - 0.5).abs() < 1e-3);
    assert!((num(value_at(&rows, &[], DIM, 6.0))).abs() < 1e-9);
    let rows = vec![
        row(0.0, 0.0, DIM, f(0.0)),
        row(1.0, 10.0, DIM, f(1.0)),
        row(6.0, 2.0, DIM, f(0.0)),
    ];
    assert!((num(value_at(&rows, &[], DIM, 6.0)) - 0.5).abs() < 1e-9);
    assert!((num(value_at(&rows, &[], DIM, 7.0)) - 0.25).abs() < 1e-9);
}

#[test]
fn an_instant_row_applies_before_a_ramp_at_the_same_time_whatever_the_list_order() {
    let rows = vec![row(1.0, 2.0, DIM, f(1.0)), row(1.0, 0.0, DIM, f(0.0))];
    assert!((num(value_at(&rows, &[], DIM, 1.0))).abs() < 1e-9);
    assert!((num(value_at(&rows, &[], DIM, 2.0)) - 0.5).abs() < 1e-9);
}

#[test]
fn options_step_even_with_a_fade_and_colours_ramp_per_channel() {
    let colour = "/fixture/A/overrides/color";
    let rows = vec![
        row(
            0.0,
            0.0,
            colour,
            vec![
                OscArgument::Float(0.0),
                OscArgument::Float(1.0),
                OscArgument::Float(0.5),
            ],
        ),
        row(
            1.0,
            2.0,
            colour,
            vec![
                OscArgument::Float(1.0),
                OscArgument::Float(0.0),
                OscArgument::Float(0.5),
            ],
        ),
        row(
            0.0,
            0.0,
            "/fixture/A/overrides/gobo",
            vec![OscArgument::Int(1)],
        ),
        row(
            1.0,
            2.0,
            "/fixture/A/overrides/gobo",
            vec![OscArgument::Int(2)],
        ),
    ];
    let t = Timeline::new(&rows, &[]);
    let mid = t.value_at(colour, 2.0).unwrap();
    assert_eq!(
        mid,
        vec![
            FixtureArg::Float(0.5),
            FixtureArg::Float(0.5),
            FixtureArg::Float(0.5)
        ]
    );
    assert_eq!(
        t.value_at("/fixture/A/overrides/gobo", 1.0),
        Some(vec![FixtureArg::Int(2)])
    );
}

fn loop_effect(period: f64, start: f64, end: f64, members: &[(&str, f64)]) -> LightingEffect {
    // 0 -> 1 over the first half, 1 -> 0 over the second
    LightingEffect {
        id: Uuid::new_v4(),
        label: "loop".into(),
        start_seconds: start,
        end_seconds: end,
        period_seconds: period,
        members: members
            .iter()
            .map(|(f, o)| EffectMember {
                fixture: (*f).into(),
                offset_seconds: *o,
            })
            .collect(),
        cycle: vec![
            EffectStep {
                at_seconds: 0.0,
                property: "dimmer".into(),
                value: vec![1.0],
                fade_seconds: period / 2.0,
            },
            EffectStep {
                at_seconds: period / 2.0,
                property: "dimmer".into(),
                value: vec![0.0],
                fade_seconds: period / 2.0,
            },
        ],
        source: OscCueSource::Captured,
        edited: false,
    }
}

#[test]
fn an_effect_loops_with_exact_period_arithmetic_and_member_offsets() {
    // the cycle starts from the last step's value (0), rises to 1 at period/2, falls back by period
    let effect = loop_effect(4.0, 10.0, 1010.0, &[("A", 0.0), ("B", 1.0)]);
    let rows = vec![row(0.0, 0.0, DIM, f(0.7))];
    let t = Timeline::new(&rows, std::slice::from_ref(&effect));
    let a = |at: f64| num(t.value_at("/fixture/A/overrides/dimmer", at));
    let b = |at: f64| num(t.value_at("/fixture/B/overrides/dimmer", at));
    assert!(
        (a(5.0) - 0.7).abs() < 1e-9,
        "before the window the rows apply"
    );
    assert!((a(10.0) - 0.0).abs() < 1e-9);
    assert!((a(11.0) - 0.5).abs() < 1e-9);
    assert!((a(12.0) - 1.0).abs() < 1e-9);
    assert!(
        (a(14.0) - 0.0).abs() < 1e-9,
        "one period later it is the same"
    );
    // 1000 periods later: no drift
    assert!((a(11.0 + 4.0 * 249.0) - 0.5).abs() < 1e-9);
    // B plays the same cycle 1 s later
    assert!((b(11.0) - 0.0).abs() < 1e-9);
    assert!((b(12.0) - 0.5).abs() < 1e-9);
    // a member that is not listed is untouched
    assert!(t.value_at("/fixture/C/overrides/dimmer", 12.0).is_none());
    // after the window the rows apply again
    assert!((a(1010.0) - 0.7).abs() < 1e-9);
    assert!(t.in_effect("/fixture/A/overrides/dimmer", 500.0));
    assert!(!t.in_effect("/fixture/A/overrides/dimmer", 1010.0));
}

#[test]
fn seeking_is_pure_the_same_position_gives_the_same_values() {
    let capture = run(
        &sweep(7.95, &[("Z1", 0.0), ("Z2", 1.0)], 1.0, 100.0),
        105.0,
        true,
    );
    let t = timeline(&capture);
    let first = t.state_at(63.37);
    let _ = t.state_at(5.0);
    let _ = t.state_at(90.0);
    assert_eq!(t.state_at(63.37), first);
}

// ----------------------------------------------------------------------- optional: real takes

#[test]
fn real_takes_reconstruct_within_the_acceptance() {
    let Ok(path) = std::env::var("STAGEPILOT_SMOOTH_TAKES") else {
        eprintln!("STAGEPILOT_SMOOTH_TAKES is unset: skipped");
        return;
    };
    let text = std::fs::read_to_string(path).expect("takes file");
    let doc: serde_json::Value = serde_json::from_str(&text).expect("json");
    for take in doc["takes"].as_array().expect("takes") {
        let duration = take["duration_seconds"].as_f64().unwrap_or(0.0);
        let command = |v: &serde_json::Value| -> Option<FixtureCommand> {
            let a = v["a"].as_str()?.to_string();
            let args = v["v"]
                .as_array()?
                .iter()
                .map(|x| match x {
                    serde_json::Value::String(s) => Some(FixtureArg::Text(s.clone())),
                    serde_json::Value::Number(n) if n.is_i64() => n.as_i64().map(FixtureArg::Int),
                    other => other.as_f64().map(FixtureArg::Float),
                })
                .collect::<Option<Vec<_>>>()?;
            Some(FixtureCommand::new(a, args))
        };
        let baseline: Vec<FixtureCommand> = take["fixtures"]["initial"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(command)
            .collect();
        let mut changes = Changes::new();
        for event in take["fixtures"]["events"].as_array().expect("events") {
            let at = event["at_seconds"].as_f64().unwrap_or(0.0);
            for m in event["osc"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(command)
            {
                changes.push((at, m));
            }
        }
        let capture = fit(&baseline, &changes, duration, &SmoothParams::default());
        let rows = smooth_rows(&capture.rows);
        let r = reconstruct_report(&baseline, &changes, duration, &rows, &capture.effects, 1.0);
        for c in &r.outside_effects {
            assert!(c.max <= c.tolerance * 1.001, "outside effects {c:?}");
        }
        for c in &r.overall {
            assert!(c.p99 <= 2.0 * c.tolerance, "overall {c:?}");
        }
        let items = rows.len() + capture.effects.len();
        eprintln!(
            "{}: {} rows + {} effects = {items} items",
            take["song_title"],
            rows.len(),
            capture.effects.len()
        );
    }
}
