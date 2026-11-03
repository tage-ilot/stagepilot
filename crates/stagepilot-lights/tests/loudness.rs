//! Level maths, the recorder on the song clock, and how recordings average.
use stagepilot_core::config::SongLoudness;
use stagepilot_lights::loudness::*;

fn sine(amplitude: f32, n: usize, rate: u32) -> Vec<f32> {
    (0..n)
        .map(|i| amplitude * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin())
        .collect()
}

fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} vs {b}");
}

#[test]
fn level_of_synthetic_signals() {
    // A full-scale sine: peak 0 dBFS, RMS -3.01 dBFS.
    let full = window_level(&sine(1.0, 48_000, 48_000));
    near(full.peak_db, 0.0, 0.01);
    near(full.rms_db, -3.01, 0.05);
    // 0.1 amplitude square-ish DC block: RMS equals the level, -20 dBFS.
    let tenth = window_level(&vec![0.1f32; 4800]);
    near(tenth.rms_db, -20.0, 0.01);
    near(tenth.peak_db, -20.0, 0.01);
    // Clipping: a flat 1.0 is exactly 0 dBFS.
    let clip = window_level(&vec![1.0f32; 4800]);
    near(clip.rms_db, 0.0, 1e-9);
    // Silence is -inf and below the floor.
    let silence = window_level(&vec![0.0f32; 4800]);
    assert!(silence.rms_db.is_infinite() && silence.rms_db < 0.0);
    // A -60 dBFS DC block.
    near(window_level(&vec![0.001f32; 4800]).rms_db, -60.0, 0.01);
}

#[test]
fn byte_scale_is_minus_60_to_0() {
    assert_eq!(level_byte(f64::NEG_INFINITY), 1);
    assert_eq!(level_byte(-60.0), 1);
    assert_eq!(level_byte(-90.0), 1);
    assert_eq!(level_byte(0.0), 255);
    assert_eq!(level_byte(6.0), 255);
    assert_eq!(level_byte(-30.0), 128);
    assert_eq!(byte_db(0), None);
    near(byte_db(255).unwrap(), 0.0, 1e-9);
    near(byte_db(1).unwrap(), -60.0, 1e-9);
    near(byte_db(level_byte(-20.0)).unwrap(), -20.0, 0.2);
}

#[test]
fn downmix_averages_channels() {
    assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
    assert_eq!(downmix(&[0.25, 0.5], 1), vec![0.25, 0.5]);
}

const RATE: u32 = 1000;

/// Feeds `seconds` of `level` ending at wall time `end`, in 100 ms blocks like a device would.
fn feed(rec: &mut LoudnessRecorder, from_wall: f64, seconds: f64, level: f32) {
    let blocks = (seconds * 10.0).round() as usize;
    for b in 1..=blocks {
        rec.push(from_wall + b as f64 * 0.1, RATE, &vec![level; 100]);
    }
}

#[test]
fn windows_line_up_with_the_song_clock() {
    // Song time 0 happened at wall 100.0. Loud (0.1 = -20 dBFS) for 1 s, then quiet (0.01 = -40).
    let mut rec = LoudnessRecorder::new(100.0);
    feed(&mut rec, 100.0, 1.0, 0.1);
    feed(&mut rec, 101.0, 1.0, 0.01);
    let loud = rec.finish(2.0).unwrap();
    assert_eq!(loud.values.len(), 8);
    assert_eq!(loud.step_seconds, 0.25);
    assert_eq!(loud.takes, 1);
    for v in &loud.values[..4] {
        near(byte_db(*v).unwrap(), -20.0, 0.3);
    }
    for v in &loud.values[4..] {
        near(byte_db(*v).unwrap(), -40.0, 0.3);
    }
}

#[test]
fn audio_before_the_song_started_is_ignored() {
    let mut rec = LoudnessRecorder::new(100.0);
    feed(&mut rec, 99.0, 1.0, 0.5); // before song time 0
    feed(&mut rec, 100.0, 0.5, 0.1);
    let l = rec.finish(1.0).unwrap();
    assert_eq!(l.values.len(), 4);
    near(byte_db(l.values[0]).unwrap(), -20.0, 0.3);
    assert_eq!(l.values[2], 0);
    assert_eq!(l.values[3], 0);
}

#[test]
fn a_pause_leaves_a_gap_and_resume_moves_the_clock() {
    let mut rec = LoudnessRecorder::new(100.0);
    feed(&mut rec, 100.0, 1.0, 0.1); // song 0..1
    rec.set_running(false);
    feed(&mut rec, 101.0, 2.0, 0.5); // paused: nothing collected
                                     // Resume at song position 1.0, wall 103.0: song time 0 is now wall 102.0.
    rec.set_started_at(102.0);
    rec.set_running(true);
    feed(&mut rec, 103.0, 1.0, 0.1); // song 1..2
    let l = rec.finish(3.0).unwrap();
    assert!(l.values[..8].iter().all(|v| *v != 0), "{:?}", l.values);
    assert!(l.values[8..].iter().all(|v| *v == 0), "{:?}", l.values);
    // The paused 0.5 never shows (it would be -6 dBFS).
    assert!(l
        .values
        .iter()
        .all(|v| byte_db(*v).is_none_or(|d| d < -15.0)));
}

#[test]
fn a_pause_in_the_middle_leaves_zero_slots() {
    let mut rec = LoudnessRecorder::new(0.0);
    feed(&mut rec, 0.0, 1.0, 0.1);
    rec.set_running(false);
    rec.set_running(true);
    feed(&mut rec, 2.0, 1.0, 0.1); // song 2..3, nothing for 1..2
    let l = rec.finish(3.0).unwrap();
    assert!(l.values[4..8].iter().all(|v| *v == 0));
    assert!(l.values[8..].iter().all(|v| *v != 0));
}

#[test]
fn nothing_heard_gives_nothing() {
    assert!(LoudnessRecorder::new(0.0).finish(10.0).is_none());
}

#[test]
fn cannot_exceed_the_value_cap() {
    let mut rec = LoudnessRecorder::new(0.0);
    feed(&mut rec, 0.0, 1.0, 0.1);
    let l = rec.finish(100_000.0).unwrap();
    assert_eq!(l.values.len(), 4000);
}

fn lo(values: &[u8], takes: u32) -> SongLoudness {
    SongLoudness {
        step_seconds: 0.25,
        values: values.to_vec(),
        takes,
    }
}

#[test]
fn averaging_is_in_power_and_weighted_by_takes() {
    // -20 dB and -30 dB, equal weights: mean power (0.01 + 0.001) / 2 = 0.0055 -> -22.6 dB.
    let a = level_byte(-20.0);
    let b = level_byte(-30.0);
    let m = merge_loudness(Some(&lo(&[a], 1)), lo(&[b], 1));
    assert_eq!(m.takes, 2);
    near(byte_db(m.values[0]).unwrap(), -22.6, 0.3);
    // Three takes of -20 against one of -30: weighted 3:1 -> (3*0.01 + 0.001)/4 = 0.00775 -> -21.1 dB.
    let m = merge_loudness(Some(&lo(&[a], 3)), lo(&[b], 1));
    assert_eq!(m.takes, 4);
    near(byte_db(m.values[0]).unwrap(), -21.1, 0.3);
}

#[test]
fn takes_cap_at_eight_and_the_envelope_still_adapts() {
    let loud = level_byte(-10.0);
    let quiet = level_byte(-50.0);
    let mut m = lo(&[loud], 8);
    m = merge_loudness(Some(&m), lo(&[quiet], 1));
    assert_eq!(m.takes, 8);
    // Weight 8:1 keeps it near loud but not equal.
    let d = byte_db(m.values[0]).unwrap();
    assert!(d < -10.0 && d > -20.0, "{d}");
    let again = merge_loudness(Some(&m), lo(&[quiet], 1));
    assert!(byte_db(again.values[0]).unwrap() < d);
}

#[test]
fn slots_missing_in_one_recording_keep_the_other() {
    let a = level_byte(-20.0);
    let b = level_byte(-30.0);
    let m = merge_loudness(Some(&lo(&[a, 0, a], 2)), lo(&[0, b, b, b], 1));
    assert_eq!(m.values.len(), 4);
    assert_eq!(m.values[0], a);
    assert_eq!(m.values[1], b);
    assert!(m.values[2] != a && m.values[2] != b);
    assert_eq!(m.values[3], b);
    assert_eq!(m.takes, 3);
}

#[test]
fn first_recording_and_step_change_replace() {
    let n = lo(&[10], 1);
    assert_eq!(merge_loudness(None, n.clone()), n);
    let other = SongLoudness {
        step_seconds: 0.5,
        values: vec![200],
        takes: 4,
    };
    assert_eq!(merge_loudness(Some(&other), n.clone()), n);
}

#[test]
fn peak_slot_finds_the_loudest() {
    let l = lo(&[0, 5, 9, 9, 3], 1);
    assert_eq!(peak_slot(&l), Some((0.5, 9)));
    assert_eq!(peak_slot(&lo(&[0, 0], 1)), None);
}

#[test]
fn meter_reads_since_last_and_repeats() {
    let meter = LevelMeter::default();
    assert!(meter.read().is_none());
    meter.feed(&vec![0.1; 100]);
    near(meter.read().unwrap().rms_db, -20.0, 0.01);
    near(meter.read().unwrap().rms_db, -20.0, 0.01); // nothing new: same reading
    meter.clear();
    assert!(meter.read().is_none());
}

#[test]
fn merge_enforces_the_caps_on_a_first_recording() {
    let big = SongLoudness {
        step_seconds: 0.25,
        values: vec![9; 5000],
        takes: 50,
    };
    let m = merge_loudness(None, big);
    assert_eq!((m.values.len(), m.takes), (4000, 8));
}
