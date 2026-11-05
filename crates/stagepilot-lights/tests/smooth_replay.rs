//! Smooth replay: the engine that evaluates a song's rows and effects at the song position and
//! sends only what changed, on a simulated clock (no sockets, no sleeping), and the same engine
//! inside the lights plugin on tokio's paused clock (seek, pause, operator take-over).

use stagepilot_core::config::{
    EffectMember, EffectStep, LightingEffect, OscArgument, OscCueSource, OscLightingCue, OscMessage,
};
use stagepilot_lights::lightkey_curve::Timeline;
use stagepilot_lights::lightkey_fixtures::FixtureArg;
use stagepilot_lights::lightkey_replay::{
    is_smooth, simulate, ReplayEngine, SentPacket, FAST_TICK_SECONDS, MAX_PACKETS_PER_SECOND,
    SLOW_TICK_SECONDS,
};
use uuid::Uuid;

const LEAD: f64 = 0.045;
const DIM: &str = "/fixture/A/overrides/dimmer";
const STEP: f64 = 1.0 / 255.0;

fn f(x: f64) -> Vec<OscArgument> {
    vec![OscArgument::Float(x)]
}

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

fn num(m: &OscMessage) -> f64 {
    match m.args.first() {
        Some(OscArgument::Float(x)) => *x,
        other => panic!("{other:?}"),
    }
}

fn value(t: &Timeline, address: &str, at: f64) -> f64 {
    match t.playback_at(address, at).expect("a value").first() {
        Some(FixtureArg::Float(x)) => *x,
        other => panic!("{other:?}"),
    }
}

fn of<'a>(packets: &'a [SentPacket], address: &str) -> Vec<&'a SentPacket> {
    packets
        .iter()
        .filter(|p| p.message.address == address)
        .collect()
}

/// Most packets in any one second of slot times.
fn busiest_second(packets: &[SentPacket]) -> usize {
    let mut best = 0;
    let mut lo = 0;
    for hi in 0..packets.len() {
        while packets[hi].at - packets[lo].at >= 1.0 {
            lo += 1;
        }
        best = best.max(hi - lo + 1);
    }
    best
}

fn no_one(_: f64, _: &str) -> bool {
    false
}

/// 0 -> 1 -> 0 over `period`, starting from the last step's value (0).
fn loop_effect(period: f64, start: f64, end: f64, members: &[(&str, f64)]) -> LightingEffect {
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

// ------------------------------------------------------------------------------- what is smooth

#[test]
fn only_a_ramp_or_an_effect_makes_a_song_smooth() {
    let old = vec![row(1.0, 0.0, DIM, f(0.5)), row(2.0, 0.0, DIM, f(0.9))];
    assert!(
        !is_smooth(&old, &[]),
        "rows without a fade are the old path"
    );
    let ramp = vec![row(1.0, 2.0, DIM, f(0.5))];
    assert!(is_smooth(&ramp, &[]));
    let effect = loop_effect(4.0, 0.0, 100.0, &[("A", 0.0)]);
    assert!(is_smooth(&old, &[effect]));
}

// ------------------------------------------------------------------------------------ ramps

#[test]
fn a_sixty_second_ramp_sends_one_dmx_step_at_a_time_and_ends_exactly() {
    let rows = vec![row(0.0, 0.0, DIM, f(0.0)), row(10.0, 60.0, DIM, f(1.0))];
    let timeline = Timeline::new(&rows, &[]);
    let mut engine = ReplayEngine::new(&rows, &[]);
    let packets = simulate(&mut engine, 0.0, 80.0, LEAD, &no_one);
    // 255 DMX steps over the ramp, plus the start value; nothing like 20 Hz x 60 s = 1200
    assert!(
        (235..=300).contains(&packets.len()),
        "{} packets",
        packets.len()
    );
    for p in &packets {
        let want = value(&timeline, DIM, p.position);
        assert!(
            (num(&p.message) - want).abs() <= STEP * 1.5 + 1e-9,
            "at {}: sent {} want {want}",
            p.position,
            num(&p.message)
        );
    }
    let last = packets.last().unwrap();
    assert_eq!(num(&last.message), 1.0, "the end of the ramp is exact");
    assert!(
        (last.at - (70.0 - LEAD)).abs() < 0.1,
        "the last packet is at the end of the ramp, not later: {}",
        last.at
    );
    assert!(busiest_second(&packets) <= 20);
}

#[test]
fn nothing_moving_means_no_tick_and_no_packets() {
    let rows = vec![row(0.0, 0.0, DIM, f(0.3)), row(100.0, 0.0, DIM, f(0.6))];
    let mut engine = ReplayEngine::new(&rows, &[]);
    let first = engine.tick(0.0, LEAD, &|_| false);
    assert_eq!(first.messages.len(), 1);
    let wake = first.wake_in.expect("wakes for the step at 100 s");
    assert!((wake - (100.0 - LEAD)).abs() < 0.01, "{wake}");
    let second = engine.tick(wake, 100.0 - 0.0001, &|_| false);
    // the wake lands just before the row: the step is not early by more than the lead
    let third = engine.tick(wake + 0.01, 100.0 + 0.01, &|_| false);
    assert_eq!(second.messages.len() + third.messages.len(), 1);
    let idle = engine.tick(wake + 1.0, 101.0, &|_| false);
    assert!(idle.messages.is_empty());
    assert_eq!(idle.wake_in, None, "idle engine asks for no tick");
}

#[test]
fn a_seek_just_evaluates_at_the_new_position() {
    let rows = vec![row(0.0, 0.0, DIM, f(0.0)), row(10.0, 60.0, DIM, f(1.0))];
    let timeline = Timeline::new(&rows, &[]);
    for p in [0.0, 9.9, 35.0, 69.9, 120.0] {
        let mut engine = ReplayEngine::new(&rows, &[]);
        let packets = simulate(&mut engine, p, p + 1.0, LEAD, &no_one);
        let first = &packets[0];
        assert!(
            (num(&first.message) - value(&timeline, DIM, p + LEAD)).abs() < 1e-9,
            "start at {p}: {}",
            num(&first.message)
        );
    }
}

#[test]
fn an_address_before_its_first_row_gets_nothing() {
    let rows = vec![row(20.0, 0.0, DIM, f(0.5))];
    let mut engine = ReplayEngine::new(&rows, &[]);
    let packets = simulate(&mut engine, 0.0, 10.0, LEAD, &no_one);
    assert!(packets.is_empty());
    assert_eq!(engine.fixtures_without_value(5.0), vec!["A".to_string()]);
    assert!(engine.fixtures_without_value(25.0).is_empty());
}

#[test]
fn a_fixture_the_operator_has_is_not_driven_and_comes_back_at_the_current_value() {
    let rows = vec![row(0.0, 0.0, DIM, f(0.0)), row(0.0, 60.0, DIM, f(1.0))];
    let timeline = Timeline::new(&rows, &[]);
    let mut engine = ReplayEngine::new(&rows, &[]);
    // the operator has A between 10 s and 40 s
    let packets = simulate(&mut engine, 0.0, 60.0, LEAD, &|now, a| {
        a.contains("/A/") && (10.0..40.0).contains(&now)
    });
    let during: Vec<_> = packets
        .iter()
        .filter(|p| p.at >= 10.0 && p.at < 40.0)
        .collect();
    assert!(during.is_empty(), "{} packets while yielded", during.len());
    let back = packets.iter().find(|p| p.at >= 40.0).expect("taken back");
    let want = value(&timeline, DIM, back.position);
    assert!(
        (num(&back.message) - want).abs() <= STEP * 1.5,
        "the value jumps to the interpolated one for that moment: {} vs {want}",
        num(&back.message)
    );
    assert!(back.position > 40.0 && num(&back.message) > 0.6);
}

// ---------------------------------------------------------------------------------- effects

#[test]
fn an_effect_plays_from_its_cycle_with_member_offsets_and_never_drifts() {
    let effect = loop_effect(7.95, 5.0, 3000.0, &[("A", 0.0), ("B", 1.0), ("C", 2.0)]);
    let rows = vec![row(0.0, 0.0, DIM, f(0.0))];
    let timeline = Timeline::new(&rows, std::slice::from_ref(&effect));
    // period arithmetic is exact: 250 periods later is the same value
    for t in [12.3, 40.0, 100.1] {
        let a = value(&timeline, DIM, t);
        let b = value(&timeline, DIM, t + 7.95 * 240.0);
        assert!((a - b).abs() < 1e-9, "{a} vs {b}");
    }
    let mut engine = ReplayEngine::new(&rows, std::slice::from_ref(&effect));
    let packets = simulate(&mut engine, 1700.0, 1720.0, LEAD, &no_one);
    for member in ["A", "B", "C"] {
        let address = format!("/fixture/{member}/overrides/dimmer");
        let mine = of(&packets, &address);
        assert!(!mine.is_empty());
        for p in &mine {
            let want = value(&timeline, &address, p.position);
            assert!(
                // sent as it will be half a tick later: the slope (0.5 per second) times 62 ms
                (num(&p.message) - want).abs() <= 0.5 * SLOW_TICK_SECONDS / 2.0 + STEP * 1.5 + 1e-9,
                "{member} at {}",
                p.position
            );
        }
        // every address stays below the fast rate
        assert!(busiest_second(&mine.iter().map(|p| (*p).clone()).collect::<Vec<_>>()) <= 21);
    }
    // the three members play the same shape 1 s apart
    let a = value(&timeline, "/fixture/A/overrides/dimmer", 1710.0);
    let b = value(&timeline, "/fixture/B/overrides/dimmer", 1711.0);
    assert!((a - b).abs() < 1e-9);
}

#[test]
fn too_many_effect_addresses_lower_the_effect_rate_and_never_delay_a_step() {
    let members: Vec<String> = (0..150).map(|i| format!("F{i}")).collect();
    let refs: Vec<(&str, f64)> = members
        .iter()
        .enumerate()
        .map(|(i, m)| (m.as_str(), i as f64 * 0.01))
        .collect();
    let effect = loop_effect(4.0, 0.0, 1000.0, &refs);
    let step_address = "/fixture/S/overrides/dimmer";
    let rows = vec![
        row(0.0, 0.0, step_address, f(0.0)),
        row(103.0, 0.0, step_address, f(1.0)),
        row(105.5, 0.0, step_address, f(0.2)),
    ];
    let mut engine = ReplayEngine::new(&rows, std::slice::from_ref(&effect));
    let packets = simulate(&mut engine, 100.0, 110.0, LEAD, &no_one);
    assert!(
        busiest_second(&packets) as f64 <= MAX_PACKETS_PER_SECOND + 1.0,
        "{} in the busiest second",
        busiest_second(&packets)
    );
    assert!(
        engine.effect_interval() > FAST_TICK_SECONDS + 1e-9
            && engine.effect_interval() <= SLOW_TICK_SECONDS + 1e-9,
        "effect interval {}",
        engine.effect_interval()
    );
    for (row_at, want) in [(103.0, 1.0), (105.5, 0.2)] {
        let step = of(&packets, step_address)
            .into_iter()
            .find(|p| (num(&p.message) - want).abs() < 1e-9 && p.position > 100.5)
            .expect("the step was sent");
        let due = row_at - LEAD;
        assert!(
            step.at >= due - 0.02 && step.at <= due + 0.06,
            "step due at {due} went out at {}",
            step.at
        );
    }
    // the busy effect still moves: every member got packets
    for m in &members {
        assert!(!of(&packets, &format!("/fixture/{m}/overrides/dimmer")).is_empty());
    }
}

#[test]
fn an_effect_meets_the_rows_around_it_without_a_jump_beyond_the_tolerance() {
    // rows hold 0.0 before and after; the effect starts and ends at phase 0 (value 0.0)
    let effect = loop_effect(4.0, 10.0, 50.0, &[("A", 0.0)]);
    let rows = vec![row(0.0, 0.0, DIM, f(0.0))];
    let timeline = Timeline::new(&rows, std::slice::from_ref(&effect));
    let mut engine = ReplayEngine::new(&rows, std::slice::from_ref(&effect));
    let packets = simulate(&mut engine, 0.0, 70.0, LEAD, &no_one);
    let mine = of(&packets, DIM);
    for seam in [10.0, 50.0] {
        let near: Vec<_> = mine
            .iter()
            .filter(|p| (p.position - seam).abs() < 0.3)
            .collect();
        assert!(near.len() >= 2, "packets near {seam}");
        for p in near {
            let want = value(&timeline, DIM, p.position);
            assert!((num(&p.message) - want).abs() <= 0.5 * SLOW_TICK_SECONDS / 2.0 + STEP * 1.5);
        }
    }
    // consecutive sent values never differ by more than one tick of the steepest slope + a step
    let steepest = 2.0 / 4.0 * SLOW_TICK_SECONDS;
    for pair in mine.windows(2) {
        let d = (num(&pair[1].message) - num(&pair[0].message)).abs();
        assert!(
            d <= steepest + 3.0 * STEP,
            "jump {d} at {}",
            pair[1].position
        );
    }
}

// ------------------------------------------------- a captured effect meets its rows (S1 fit)

#[test]
fn a_fitted_effect_hands_over_to_the_rows_around_it_within_the_tolerance() {
    use stagepilot_lights::lightkey_fixtures::FixtureCommand;
    use stagepilot_lights::lightkey_smooth::{fit, smooth_rows, SmoothParams};
    fn triangle(t: f64, period: f64) -> f64 {
        let p = (t / period).rem_euclid(1.0);
        -60.0 + 120.0 * if p < 0.5 { 2.0 * p } else { 2.0 - 2.0 * p }
    }
    // a ramp up to the first peak at 4 s, a sweep from 4 s to 124 s, then a slow ramp back to 0
    let fixtures = [("Z1", 0.0), ("Z2", 1.0), ("Z3", 2.0)];
    let mut changes: Vec<(f64, FixtureCommand)> = Vec::new();
    for (name, offset) in fixtures {
        let mut t = 0.0;
        while t <= 150.0 + 1e-9 {
            let v = if t < 124.0 {
                triangle(t - offset, 7.95)
            } else {
                // hold the last swept value, then ease to 0 over 20 s
                let last = triangle(124.0 - offset, 7.95);
                let f = ((t - 130.0) / 20.0).clamp(0.0, 1.0);
                last * (1.0 - f)
            };
            changes.push((
                t,
                FixtureCommand::new(
                    format!("/fixture/{name}/overrides/panAngle"),
                    vec![FixtureArg::Float(v)],
                ),
            ));
            t += 0.025;
        }
    }
    let capture = fit(&[], &changes, 150.0, &SmoothParams::default());
    assert!(!capture.effects.is_empty(), "{:?}", capture.effect_infos);
    let seams: Vec<f64> = capture
        .effects
        .iter()
        .flat_map(|e| [e.start_seconds, e.end_seconds])
        .filter(|s| *s > 1.0)
        .collect();
    assert!(!seams.is_empty());
    let rows = smooth_rows(&capture.rows);
    let mut engine = ReplayEngine::new(&rows, &capture.effects);
    let packets = simulate(&mut engine, 0.0, 150.0, LEAD, &no_one);
    let timeline = Timeline::new(&rows, &capture.effects);
    let tolerance = 1.5;
    for (name, _) in fixtures {
        let address = format!("/fixture/{name}/overrides/panAngle");
        let mine = of(&packets, &address);
        let want = |t: f64| value(&timeline, &address, t);
        // 30 degrees a second on the sweep: a tick of up to 125 ms held, centred, is 1.9 degrees
        let allowed = 2.0 * tolerance + 30.0 * SLOW_TICK_SECONDS / 2.0;
        for seam in seams.iter().copied() {
            let near: Vec<_> = mine
                .iter()
                .filter(|p| (p.position - seam).abs() < 1.0)
                .collect();
            assert!(!near.is_empty(), "{name}: packets near the seam at {seam}");
            for p in near {
                assert!(
                    (num(&p.message) - want(p.position)).abs() <= allowed,
                    "{name} at {}: sent {} raw {}",
                    p.position,
                    num(&p.message),
                    want(p.position)
                );
            }
            // the value the rows give just before and the effect just after differ by less
            // than the tolerance
            let before = value(&timeline, &address, seam - 1e-6);
            let after = value(&timeline, &address, seam + 1e-6);
            assert!(
                (after - before).abs() <= 2.0 * tolerance + 0.01,
                "{name}: seam {seam} jumps {before} -> {after}"
            );
        }
    }
}
