//! A recorded song is never thrown away for size: the song-1 replica (2026-11-08), adversarial
//! recordings, the monotonic ladder, the recorder bound and the raw recording on disk.
//!
//! Wall-clock assertions are generous (a slow macOS Intel runner takes about five times longer
//! than this machine, and these run unoptimised).

use stagepilot_core::config::{
    SongLightingCueMap, MAX_EFFECTS_PER_SONG, MAX_EFFECT_CYCLE_STEPS_PER_SONG,
    MAX_OSC_MESSAGES_PER_SONG, MAX_OSC_ROWS_PER_SONG,
};
use stagepilot_lights::lightkey_curve::Timeline;
use stagepilot_lights::lightkey_fit::{
    fit_within_caps, fit_within_caps_budgeted, Caps, Fitted, Size, Stage,
};
use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand, FixtureState};
use stagepilot_lights::lightkey_looks::{
    save_capture_with_effects, within_caps, CaptureSave, LookRecorder,
};
use stagepilot_lights::lightkey_recording::{self, RawRecording};
use stagepilot_lights::lightkey_smooth::smooth_rows;
use std::time::Instant;

/// The heavy tests fit hundreds of thousands of changes on every core and the replay tests time
/// ticks: run one at a time, or the clock measures the neighbours.
static HEAVY: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn heavy() -> std::sync::MutexGuard<'static, ()> {
    HEAVY.lock().unwrap_or_else(|p| p.into_inner())
}

/// A small deterministic random source (no `rand` in the workspace).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

type Changes = Vec<(f64, FixtureCommand)>;

fn dim(fixture: &str, value: f64) -> FixtureCommand {
    FixtureCommand::new(
        format!("/fixture/{fixture}/overrides/dimmer"),
        vec![FixtureArg::Float(value)],
    )
}

fn colour(fixture: &str, rgb: [f64; 3]) -> FixtureCommand {
    FixtureCommand::new(
        format!("/fixture/{fixture}/overrides/color"),
        rgb.iter().map(|v| FixtureArg::Float(*v)).collect(),
    )
}

fn option(fixture: &str, value: &str) -> FixtureCommand {
    FixtureCommand::new(
        format!("/fixture/{fixture}/overrides/gobo"),
        vec![FixtureArg::Text(value.to_string())],
    )
}

fn by_time(changes: &mut Changes) {
    changes.sort_by(|a, b| a.0.total_cmp(&b.0));
}

/// The song-1 replica: 336 s, 66 fixtures. 40 fixtures change colour about once and a gobo about
/// twice a second (steps), 16 run a beat chase, 10 have noisy dimmers on a slow swell, and a few
/// fixtures ramp.
fn song_one() -> (Vec<FixtureCommand>, Changes, f64) {
    let duration = 336.0;
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut baseline = Vec::new();
    let mut changes: Changes = Vec::new();
    for n in 0..66 {
        let f = format!("F{n}");
        baseline.push(dim(&f, 0.0));
        baseline.push(colour(&f, [0.0, 0.0, 0.0]));
        baseline.push(option(&f, "Open"));
    }
    // 40 fixtures: colour and gobo steps
    for n in 0..40 {
        let f = format!("F{n}");
        let mut t = rng.next();
        while t < duration {
            changes.push((t, colour(&f, [rng.next(), rng.next(), rng.next()])));
            t += 0.5 + rng.next();
        }
        let mut t = rng.next() * 0.5;
        while t < duration {
            changes.push((
                t,
                option(&f, &format!("Gobo {}", (rng.next() * 12.0) as u32)),
            ));
            t += 0.25 + rng.next() * 0.5;
        }
    }
    // 16 fixtures chase: a pulse every 0.5 s, each fixture 1/16 of a period after the one before
    for n in 40..56 {
        let f = format!("F{n}");
        let phase = f64::from(n - 40) * 0.5 / 16.0;
        let mut cycle = 0.0;
        while cycle + phase < duration {
            let t0 = cycle + phase;
            for k in 0..=5 {
                let rise = t0 + f64::from(k) * 0.01;
                let fall = t0 + 0.2 + f64::from(k) * 0.01;
                if fall < duration {
                    changes.push((rise, dim(&f, f64::from(k) / 5.0)));
                    changes.push((fall, dim(&f, 1.0 - f64::from(k) / 5.0)));
                }
            }
            cycle += 0.5;
        }
    }
    // 10 noisy dimmers on a slow swell
    for n in 56..66 {
        let f = format!("F{n}");
        let mut t = 0.0;
        while t < duration {
            let swell = 0.5 + 0.3 * (t / 7.0 + f64::from(n)).sin();
            changes.push((t, dim(&f, swell + (rng.next() - 0.5) * 0.02)));
            t += 0.04;
        }
    }
    // a few long ramps on fixtures that otherwise only step
    for n in 0..3 {
        let f = format!("F{n}");
        let mut t = 20.0 * f64::from(n + 1);
        let start = t;
        while t < start + 20.0 {
            changes.push((t, dim(&f, (t - start) / 20.0)));
            t += 0.02;
        }
    }
    by_time(&mut changes);
    (baseline, changes, duration)
}

fn run(baseline: &[FixtureCommand], changes: &Changes, duration: f64, caps: &Caps) -> Fitted {
    run_budgeted(baseline, changes, duration, caps, u128::MAX)
}

fn run_budgeted(
    baseline: &[FixtureCommand],
    changes: &Changes,
    duration: f64,
    caps: &Caps,
    budget_ms: u128,
) -> Fitted {
    let started = Instant::now();
    let fitted = fit_within_caps_budgeted(baseline, changes, duration, caps, budget_ms);
    println!(
        "  {} raw changes -> {:?} scale x{} thin {} s trimmed {}: {:?} in {} ms ({} fits, wall {:?})",
        changes.len(),
        fitted.stage,
        fitted.scale,
        fitted.thin_seconds,
        fitted.trimmed,
        Size::of_capture(&fitted.capture),
        fitted.fit_ms,
        fitted.fits,
        started.elapsed()
    );
    for a in &fitted.attempts {
        println!(
            "    fit {:?} x{} thin {} -> {:?} in {} ms",
            a.stage, a.scale, a.thin_seconds, a.size, a.ms
        );
    }
    fitted
}

/// The capture goes through the real save step (the caps the timeline enforces).
fn saves(fitted: &Fitted) -> SongLightingCueMap {
    let rows = smooth_rows(&fitted.capture.rows);
    let status = stagepilot_core::config::CaptureStatus {
        complete: true,
        coverage_seconds: 336.0,
        recorded_at: "2026-11-08T00:00:00Z".into(),
        project: None,
        simplified: None,
    };
    match save_capture_with_effects(
        None,
        "song",
        "Song",
        rows,
        fitted.capture.effects.clone(),
        status,
    ) {
        CaptureSave::Saved(map) => {
            assert!(within_caps(&map));
            map
        }
        other => panic!("not saved: {}", matches!(other, CaptureSave::TooLarge)),
    }
}

/// The last recorded value of every address in `only` is what the saved timeline ends on.
fn final_values_intact(map: &SongLightingCueMap, changes: &Changes, duration: f64, only: &str) {
    let timeline = Timeline::new(&map.osc_cues, &map.effects);
    let mut last: std::collections::HashMap<&str, &FixtureCommand> = Default::default();
    for (_, c) in changes {
        if c.a.contains(only) {
            last.insert(&c.a, c);
        }
    }
    assert!(!last.is_empty());
    for (address, command) in last {
        let have = timeline.value_at(address, duration + 1.0);
        let ok = match (&have, command.v.first()) {
            (Some(have), Some(FixtureArg::Float(want))) => {
                matches!(have.first(), Some(FixtureArg::Float(h)) if (h - want).abs() < 0.07)
            }
            (Some(have), Some(_)) => have == &command.v,
            _ => false,
        };
        assert!(ok, "{address}: have {have:?}, want {:?}", command.v);
    }
}

#[test]
fn song_one_saves_at_full_fidelity_under_the_new_caps() {
    let _one_at_a_time = heavy();
    let (baseline, changes, duration) = song_one();
    // `BIG_SONGS_DUMP=/some/folder cargo test ...` keeps the replica as a raw recording for
    // `cargo run --example refit_recording`
    if let Ok(folder) = std::env::var("BIG_SONGS_DUMP") {
        let path = lightkey_recording::save(
            std::path::Path::new(&folder),
            &RawRecording::new("song-one-replica", duration, &baseline, &changes),
            chrono::Utc::now(),
        )
        .unwrap();
        println!("raw recording: {}", path.display());
    }
    let fitted = run(&baseline, &changes, duration, &Caps::default());
    assert_eq!(
        fitted.stage,
        Stage::Exact,
        "the new caps hold song 1 as recorded"
    );
    assert_eq!(fitted.fits, 1);
    assert!(
        fitted.capture.rows.len() > 10_000,
        "the replica must be as busy as song 1: {} rows",
        fitted.capture.rows.len()
    );
    let map = saves(&fitted);
    final_values_intact(&map, &changes, duration, "gobo");
    final_values_intact(&map, &changes, duration, "color");
    assert!(fitted.fit_ms < 120_000, "fit took {} ms", fitted.fit_ms);
}

#[test]
fn song_one_with_the_old_caps_still_saves_through_the_fallback_stages() {
    let _one_at_a_time = heavy();
    let (baseline, changes, duration) = song_one();
    let fitted = run(&baseline, &changes, duration, &Caps::OLD);
    assert!(Caps::OLD.fits(&Size::of_capture(&fitted.capture)));
    assert_ne!(fitted.stage, Stage::Exact);
    assert!(fitted.fits <= 13, "{} fits", fitted.fits);
    assert!(fitted.kept_items < fitted.exact_items);
    let map = saves(&fitted);
    assert!(map.osc_cues.len() <= Caps::OLD.rows);
    // the song's last gobo and colour of every fixture is still what the timeline ends on
    final_values_intact(&map, &changes, duration, "gobo");
    final_values_intact(&map, &changes, duration, "color");
    assert!(fitted.fit_ms < 300_000, "fit took {} ms", fitted.fit_ms);
}

fn ends_inside(
    name: &str,
    baseline: &[FixtureCommand],
    changes: &Changes,
    duration: f64,
) -> Fitted {
    println!("{name}");
    let caps = Caps::default();
    // the real budget: a slow song goes to the trim instead of fitting again and again
    let fitted = run_budgeted(
        baseline,
        changes,
        duration,
        &caps,
        stagepilot_lights::lightkey_fit::FIT_BUDGET_MS,
    );
    assert!(
        caps.fits(&Size::of_capture(&fitted.capture)),
        "{name}: over the caps"
    );
    assert!(fitted.fits <= 13);
    assert!(fitted.fit_ms < 300_000, "{name}: {} ms", fitted.fit_ms);
    assert!(
        fitted.capture.rows.len() <= MAX_OSC_ROWS_PER_SONG
            && fitted
                .capture
                .rows
                .iter()
                .map(|r| r.messages.len())
                .sum::<usize>()
                <= MAX_OSC_MESSAGES_PER_SONG
            && fitted.capture.effects.len() <= MAX_EFFECTS_PER_SONG
            && fitted
                .capture
                .effects
                .iter()
                .map(|e| e.cycle.len())
                .sum::<usize>()
                <= MAX_EFFECT_CYCLE_STEPS_PER_SONG
    );
    assert!(fitted.kept_items > 0, "{name}: an empty capture");
    saves(&fitted);
    fitted
}

#[test]
fn options_flipping_every_50_ms_for_ten_minutes_on_66_fixtures() {
    let _one_at_a_time = heavy();
    let mut changes: Changes = Vec::new();
    let mut baseline = Vec::new();
    for n in 0..66 {
        let f = format!("F{n}");
        baseline.push(option(&f, "A"));
        // every fixture flips at its own phase so no two rows are alike
        let phase = f64::from(n) * 0.00071;
        for k in 0..12_000 {
            changes.push((
                f64::from(k) * 0.05 + phase + 0.001,
                option(&f, if k % 2 == 0 { "B" } else { "A" }),
            ));
        }
    }
    by_time(&mut changes);
    let fitted = ends_inside("flips", &baseline, &changes, 600.0);
    assert!(matches!(fitted.stage, Stage::Thinned | Stage::Trimmed));
}

#[test]
fn white_noise_on_200_addresses() {
    let _one_at_a_time = heavy();
    let mut rng = Rng(7);
    let mut changes: Changes = Vec::new();
    for n in 0..200 {
        let f = format!("N{n}");
        let mut t = rng.next() * 0.02;
        while t < 30.0 {
            changes.push((t, dim(&f, rng.next())));
            t += 0.02;
        }
    }
    by_time(&mut changes);
    ends_inside("noise", &[], &changes, 30.0);
}

#[test]
fn one_address_at_one_kilohertz() {
    let _one_at_a_time = heavy();
    let mut rng = Rng(11);
    let changes: Changes = (1..60_000)
        .map(|k| (f64::from(k) * 0.001, dim("Solo", rng.next())))
        .collect();
    ends_inside("1 kHz", &[], &changes, 60.0);
}

#[test]
fn a_twenty_minute_song() {
    let _one_at_a_time = heavy();
    let mut rng = Rng(23);
    let mut changes: Changes = Vec::new();
    for n in 0..20 {
        let f = format!("L{n}");
        let mut t = rng.next() * 0.1;
        while t < 1200.0 {
            changes.push((t, dim(&f, rng.next())));
            changes.push((
                t + 0.003,
                option(&f, &format!("v{}", (rng.next() * 9.0) as u32)),
            ));
            t += 0.5;
        }
    }
    by_time(&mut changes);
    let fitted = ends_inside("20 minutes", &[], &changes, 1200.0);
    // however it got there (the fit budget may send it straight to the trim), a song a few times
    // over the caps keeps most of the caps' worth, not a token few
    assert!(
        fitted.capture.rows.len() > 10_000,
        "kept only {} rows",
        fitted.capture.rows.len()
    );
}

#[test]
fn nan_infinity_and_huge_values_cannot_stop_a_save() {
    // through the recorder: a value that cannot be sent is dropped, the rest is kept
    let mut recorder = LookRecorder::new(&FixtureState::new());
    for (k, v) in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        1e300,
        -1e300,
        0.5,
    ]
    .into_iter()
    .enumerate()
    {
        recorder.observe(1.0 + k as f64, &[dim("A", v)]);
    }
    assert_eq!(recorder.len(), 3, "NaN and the infinities are not recorded");
    let (baseline, changes) = recorder.raw();
    let fitted = ends_inside("huge", &baseline, &changes, 10.0);
    for row in &fitted.capture.rows {
        for m in &row.messages {
            assert!(m
                .v
                .iter()
                .all(|a| !matches!(a, FixtureArg::Float(f) if !f.is_finite())));
        }
    }
    // straight into the fit (a recording from an older build): no panic, still a capture
    let raw: Changes = vec![
        (1.0, dim("A", f64::NAN)),
        (2.0, dim("A", f64::INFINITY)),
        (3.0, dim("A", 1e308)),
        (4.0, dim("A", -1e308)),
    ];
    let fitted = fit_within_caps(&[], &raw, 10.0, &Caps::default());
    assert!(Caps::default().fits(&Size::of_capture(&fitted.capture)));
}

#[test]
fn one_change_only_and_no_change_at_all() {
    let fitted = ends_inside("one change", &[], &vec![(1.0, dim("A", 0.5))], 10.0);
    assert_eq!(fitted.stage, Stage::Exact);
    assert_eq!(fitted.capture.rows.len(), 1);
    let none = fit_within_caps(&[], &[], 10.0, &Caps::default());
    assert_eq!(
        none.exact_items, 0,
        "nothing recorded: the link says so and saves nothing"
    );
}

#[test]
fn a_looser_tolerance_never_gives_more_items_than_a_tighter_one() {
    // loops (a chase), ramps and steps: the case where loop detection used to disappear at x8
    let (baseline, changes, _) = song_one();
    // keep it quick: the chase and the swells, 120 s
    let changes: Changes = changes
        .into_iter()
        .filter(|(t, c)| *t < 120.0 && (c.a.contains("dimmer")))
        .collect();
    let mut previous: Option<(f64, usize)> = None;
    let mut with_loops = 0;
    for scale in [1.0, 2.0, 4.0, 8.0, 16.0, 32.0] {
        let capture = stagepilot_lights::lightkey_smooth::fit(
            &baseline,
            &changes,
            120.0,
            &stagepilot_lights::lightkey_smooth::SmoothParams {
                tolerance_scale: scale,
                ..Default::default()
            },
        );
        let size = Size::of_capture(&capture);
        println!(
            "x{scale}: {} rows, {} messages, {} loops ({} steps)",
            size.rows, size.messages, size.effects, size.cycle_steps
        );
        with_loops += usize::from(size.effects > 0);
        if let Some((before_scale, before)) = previous {
            assert!(
                size.items() + size.rows <= before,
                "x{scale} has {} items, x{before_scale} had {before}",
                size.items() + size.rows
            );
        }
        previous = Some((scale, size.items() + size.rows));
    }
    assert!(
        with_loops >= 3,
        "the chase must be found as loops at several tolerances"
    );
}

#[test]
fn the_recorder_holds_a_bounded_number_of_changes_and_keeps_the_final_values() {
    // 20 minutes of 200 addresses changing every 20 ms is 12 million changes; the same shape at
    // a smaller limit: 200 addresses, every 20 ms, 60 s, limit 50,000
    let mut recorder = LookRecorder::with_limit(&FixtureState::new(), 50_000);
    let mut rng = Rng(3);
    let mut last = vec![0.0; 200];
    let mut observed = 0usize;
    for k in 0..3000u32 {
        let t = f64::from(k) * 0.02 + 0.001;
        for (n, slot) in last.iter_mut().enumerate() {
            let v = rng.next();
            *slot = v;
            recorder.observe(t, &[dim(&format!("W{n}"), v)]);
            observed += 1;
        }
        assert!(recorder.len() <= 50_000, "held {} at {t}", recorder.len());
    }
    assert_eq!(observed, 600_000);
    assert!(recorder.thinned_times() > 0 && recorder.thin_seconds() > 0.0);
    let (_, changes) = recorder.raw();
    // the newest value of every address survived the thinning
    for (n, want) in last.iter().enumerate() {
        let address = format!("/fixture/W{n}/overrides/dimmer");
        let newest = changes.iter().rev().find(|c| c.1.a == address).unwrap();
        assert_eq!(newest.1.v, vec![FixtureArg::Float(*want)]);
    }
    // and the thinned recording still fits
    let fitted = fit_within_caps(&[], &changes, 60.0, &Caps::default());
    assert!(Caps::default().fits(&Size::of_capture(&fitted.capture)));
}

#[test]
fn the_redate_mark_survives_thinning() {
    let mut recorder = LookRecorder::with_limit(&FixtureState::new(), 100);
    for k in 0..40 {
        recorder.observe(f64::from(k) * 0.001, &[dim("A", f64::from(k) / 100.0)]);
    }
    recorder.set_mark();
    for k in 0..300 {
        recorder.observe(10.0 + f64::from(k) * 0.5, &[dim("B", f64::from(k) / 400.0)]);
    }
    assert!(recorder.thinned_times() > 0);
    let moved = recorder.shift_since_mark(-1.0);
    assert!(moved > 0 && moved <= recorder.len());
    let (_, changes) = recorder.raw();
    // the early A changes did not move, the B ones did
    assert!(changes
        .iter()
        .filter(|c| c.1.a.contains("/A/"))
        .all(|c| c.0 < 1.0));
    assert!(changes
        .iter()
        .filter(|c| c.1.a.contains("/B/"))
        .all(|c| c.0 >= 9.0));
}

#[test]
fn the_raw_recording_is_written_pruned_and_refits_to_the_same_counts() {
    let (baseline, changes, _) = song_one();
    let duration = 60.0;
    // a 60 s cut keeps this quick
    let changes: Changes = changes.into_iter().filter(|(t, _)| *t < 60.0).collect();
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("lightkey-recordings");
    let recording = RawRecording::new("song-1", duration, &baseline, &changes);
    let path = lightkey_recording::save(&folder, &recording, chrono::Utc::now()).unwrap();
    assert!(path.starts_with(&folder));
    let loaded = lightkey_recording::load(&path).unwrap();
    let (b2, c2) = loaded.into_parts();
    assert_eq!(b2.len(), baseline.len());
    assert_eq!(c2.len(), changes.len());
    let caps = Caps::OLD;
    let first = fit_within_caps(&baseline, &changes, 60.0, &caps);
    let again = fit_within_caps(&b2, &c2, 60.0, &caps);
    assert_eq!(
        Size::of_capture(&first.capture),
        Size::of_capture(&again.capture)
    );
    assert_eq!(first.stage, again.stage);
    assert_eq!(first.scale, again.scale);
    assert_eq!(first.thin_seconds, again.thin_seconds);
    assert_eq!(first.trimmed, again.trimmed);
    // pruned: only the newest five of eight stay
    for i in 1..8 {
        let p = lightkey_recording::save(
            &folder,
            &RawRecording::new("song-1", 1.0, &[], &changes[..3]),
            chrono::Utc::now() + chrono::Duration::seconds(i),
        )
        .unwrap();
        let file = std::fs::File::options().write(true).open(&p).unwrap();
        file.set_modified(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(2_000_000 + i as u64),
        )
        .unwrap();
    }
    let names = std::fs::read_dir(&folder).unwrap().count();
    assert_eq!(names, lightkey_recording::KEEP_FILES);
}

#[test]
fn an_unwritable_folder_does_not_stop_the_raw_recording_from_being_reported() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("a-file");
    std::fs::write(&blocker, b"x").unwrap();
    let recording = RawRecording::new("s", 1.0, &[], &[(0.5, dim("A", 1.0))]);
    let error = lightkey_recording::save(
        &blocker.join("lightkey-recordings"),
        &recording,
        chrono::Utc::now(),
    )
    .unwrap_err();
    assert!(error.contains("cannot"), "{error}");
}

// ------------------------------------------------------------------ replay at the caps

/// A song at the per-song caps as the replay sees it: 50,000 rows with 12 messages each over 330
/// addresses (66 fixtures x 5 properties), a third of them ramps, and 1,000 effects of 250 steps.
fn song_at_the_caps() -> (
    Vec<stagepilot_core::config::OscLightingCue>,
    Vec<stagepilot_core::config::LightingEffect>,
) {
    use stagepilot_core::config::*;
    use uuid::Uuid;
    let props = ["dimmer", "pan", "tilt", "zoom", "iris"];
    let per_row = MAX_OSC_MESSAGES_PER_SONG / MAX_OSC_ROWS_PER_SONG;
    let rows = (0..MAX_OSC_ROWS_PER_SONG)
        .map(|r| OscLightingCue {
            id: Uuid::new_v4(),
            at_seconds: r as f64 * 0.024,
            address: "/stagepilot/look".into(),
            args: vec![],
            label: format!("Look - {per_row} fixtures"),
            color: None,
            messages: (0..per_row)
                .map(|i| {
                    // each row touches 12 different addresses; a given address recurs every
                    // 27.5 rows on average
                    let fixture = (r * per_row + i) % 66;
                    let prop = props[((r * per_row + i) / 66) % props.len()];
                    OscMessage {
                        address: format!("/fixture/Moving Head {fixture}/overrides/{prop}"),
                        args: vec![OscArgument::Float(
                            ((r * 31 + i * 17) % 1000) as f64 / 1000.0,
                        )],
                    }
                })
                .collect(),
            source: OscCueSource::Captured,
            edited: false,
            fade_seconds: if r % 3 == 0 { 0.3 } else { 0.0 },
        })
        .collect();
    let steps_each = MAX_EFFECT_CYCLE_STEPS_PER_SONG / MAX_EFFECTS_PER_SONG;
    let effects = (0..MAX_EFFECTS_PER_SONG)
        .map(|e| LightingEffect {
            id: Uuid::new_v4(),
            label: format!("Loop {e}"),
            // sixteen back-to-back windows per fixture, each in the song's first 1,200 s
            start_seconds: (e / 66) as f64 * 75.0,
            end_seconds: (e / 66) as f64 * 75.0 + 70.0,
            period_seconds: 5.0,
            members: vec![EffectMember {
                fixture: format!("Moving Head {}", e % 66),
                offset_seconds: 0.0,
            }],
            cycle: (0..steps_each)
                .map(|s| EffectStep {
                    at_seconds: s as f64 * 5.0 / steps_each as f64,
                    property: "color".into(),
                    value: vec![(s % 10) as f64 / 10.0, 0.5, 0.25],
                    fade_seconds: 0.01,
                })
                .collect(),
            source: OscCueSource::Captured,
            edited: false,
        })
        .collect();
    (rows, effects)
}

#[test]
fn the_replay_engine_builds_and_ticks_inside_budget_at_the_caps() {
    let _one_at_a_time = heavy();
    use stagepilot_lights::lightkey_replay::ReplayEngine;
    let (rows, effects) = song_at_the_caps();
    let started = Instant::now();
    let mut engine = ReplayEngine::new(&rows, &effects);
    let build = started.elapsed();
    // 20 minutes at the ordinary 50 ms tick; the first 2,000 ticks and the last 2,000 are timed
    // separately so a cost that grows with the song position would show
    let mut time = |from: f64| {
        let started = Instant::now();
        let mut sent = 0usize;
        for k in 0..2_000 {
            let p = from + f64::from(k) * 0.05;
            sent += engine.tick(p, p, &|_| false).messages.len();
        }
        (started.elapsed(), sent)
    };
    let (early, sent_early) = time(0.0);
    let (late, sent_late) = time(1_000.0);
    println!(
        "replay at the caps: build {build:?}; per tick {:?} early ({sent_early} sent), {:?} late ({sent_late} sent)",
        early / 2_000,
        late / 2_000
    );
    // the same song once nothing is left to play: only the scan of every address remains
    let started = Instant::now();
    for k in 0..2_000 {
        let p = 1_500.0 + f64::from(k) * 0.05;
        engine.tick(p, p, &|_| false);
    }
    let idle = started.elapsed() / 2_000;
    println!("replay at the caps: idle tick {idle:?}");
    assert!(
        idle < std::time::Duration::from_millis(5),
        "idle tick {idle:?}"
    );
    assert!(build.as_secs() < 60, "build took {build:?}");
    // Every one of the 330 addresses is moving in this song, so a tick sends about 25 messages:
    // 2 to 3 ms on an optimised build. Generous for a slow runner; flat from start to end.
    assert!(
        early / 2_000 < std::time::Duration::from_millis(20),
        "tick {:?}",
        early / 2_000
    );
    assert!(
        late / 2_000 < std::time::Duration::from_millis(20),
        "tick {:?}",
        late / 2_000
    );
}

#[test]
fn the_song_one_replica_replays_with_a_cheap_tick() {
    let _one_at_a_time = heavy();
    use stagepilot_lights::lightkey_replay::ReplayEngine;
    let (baseline, changes, duration) = song_one();
    let fitted =
        fit_within_caps_budgeted(&baseline, &changes, duration, &Caps::default(), u128::MAX);
    let rows = smooth_rows(&fitted.capture.rows);
    let started = Instant::now();
    let mut engine = ReplayEngine::new(&rows, &fitted.capture.effects);
    let build = started.elapsed();
    let started = Instant::now();
    let mut sent = 0usize;
    let ticks = (duration / 0.05) as u32;
    for k in 0..ticks {
        let p = f64::from(k) * 0.05;
        sent += engine.tick(p, p, &|_| false).messages.len();
    }
    let tick = started.elapsed() / ticks;
    println!(
        "song-1 replica replay ({} rows): build {build:?}, {tick:?} a tick over {ticks} ticks, {sent} messages sent",
        rows.len()
    );
    assert!(tick < std::time::Duration::from_millis(20), "tick {tick:?}");
}
