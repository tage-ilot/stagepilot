//! "Clean up glitches": repair a song that is already saved. Pure: no I/O, no clock.
//!
//! Songs captured before the capture was cleaned before fitting carry brief flashes in their rows:
//! a value goes back to an older value for a frame or two and returns (the replay reproduces them
//! faithfully, so they show as jerks). The raw recording of such a song is gone, so the repair
//! works from what the song plays:
//!
//! 1. the song's rows (never its loops) are played on the 25 ms grid, per continuous address
//!    ([`lightkey_smooth::is_continuous`]). A flash is an excursion of one to [`MAX_FLASH_FRAMES`]
//!    frames that leaves the value (or the straight line the value is on) by more than 5/3 of the
//!    fit tolerance (0.05 of the range) and returns to it, outside the windows of loops. Two or
//!    three frames level with each other are a pulse and stay; so does a step;
//! 2. only the addresses that have a flash (on the grid, or half a frame off it) are repaired:
//!    the frames of a flash are put on the line between the frames around it and the address is
//!    fitted again ([`lightkey_smooth::fit`], without loop detection, at half the tolerance of a
//!    capture). Every other message of every row stays as it was, and so do the loops;
//! 3. an address is only taken from the repair when the song it plays stays within one fit
//!    tolerance of the original away from the flashes. Otherwise it is left as it was.
//!
//! Rows that are `manual` or `edited`, and every address they name, are never changed.

use crate::lightkey_curve::Timeline;
use crate::lightkey_fixtures::{fixture_of, FixtureArg, FixtureCommand};
use crate::lightkey_smooth::{
    fit, is_continuous, smooth_rows, tolerance_for, SmoothParams, CHECK_STEP_SECONDS,
};
use stagepilot_core::config::{OscCueSource, OscLightingCue, SongLightingCueMap};
use std::collections::{BTreeMap, BTreeSet};

/// The longest excursion that is a flash (frames): 75 ms.
const MAX_FLASH_FRAMES: usize = 3;
/// A frame sticks out of both neighbours by more than this many tolerances: a flash.
const AWAY: f64 = 5.0 / 3.0;
/// ...while the neighbours are within this many tolerances of each other.
const BACK: f64 = 2.0 / 3.0;
/// The repair fits the played values this much tighter than a capture is fitted (a share of the
/// tolerance), so that the repaired song stays within one tolerance of the original.
const FIT_SCALE: f64 = 0.5;
/// Grid phases the addresses with flashes are looked for on (seconds): a flash shorter than a
/// frame is on one frame boundary or the other. The flashes are counted on the first only.
const PHASES: [f64; 2] = [0.0, CHECK_STEP_SECONDS / 2.0];

/// What the flashes of a song are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    /// One-frame flashes on the 25 ms grid (an address and a frame count once).
    pub flashes: usize,
    /// Lights (fixtures) that have at least one.
    pub lights: usize,
    /// The flashes of each address: first frame and number of frames, on the 25 ms grid.
    pub found: BTreeMap<String, Vec<(usize, usize)>>,
}

/// The result of [`clean_glitches`].
#[derive(Debug, Clone, PartialEq)]
pub struct Cleaned {
    pub map: SongLightingCueMap,
    /// Flashes before and after (see [`Scan::flashes`]).
    pub flashes_before: usize,
    pub flashes_after: usize,
    pub lights: usize,
    pub rows_before: usize,
    pub rows_after: usize,
    pub messages_before: usize,
    pub messages_after: usize,
    /// Addresses taken from the repair, and addresses left as they were because the repair
    /// would have moved the song by more than one tolerance.
    pub repaired: usize,
    pub kept: usize,
    /// The largest difference between the repaired song and the original at the frames that are
    /// not flashes, as a share of the tolerance of the address (1.0 = one tolerance).
    pub max_deviation: f64,
}

fn numeric(args: &[FixtureArg]) -> Option<Vec<f64>> {
    if args.is_empty() {
        return None;
    }
    args.iter()
        .map(|a| match a {
            FixtureArg::Float(f) if f.is_finite() => Some(*f),
            _ => None,
        })
        .collect()
}

/// The addresses a repair may change: every address of a fixture message, minus those that a
/// `manual` or `edited` row names.
fn protected(map: &SongLightingCueMap) -> BTreeSet<&str> {
    map.osc_cues
        .iter()
        .filter(|r| r.source != OscCueSource::Captured || r.edited)
        .flat_map(|r| r.messages.iter().map(|m| m.address.as_str()))
        .collect()
}

fn song_end(map: &SongLightingCueMap) -> f64 {
    let rows = map
        .osc_cues
        .iter()
        .map(|r| r.at_seconds + r.fade_seconds.max(0.0));
    let effects = map.effects.iter().map(|e| e.end_seconds);
    rows.chain(effects)
        .filter(|t| t.is_finite())
        .fold(0.0, f64::max)
}

fn frames(end: f64) -> usize {
    (end / CHECK_STEP_SECONDS).ceil() as usize + 2
}

/// The frames of `address` as numbers; `None` before its first row (or not numeric).
fn grid(tl: &Timeline, address: &str, n: usize, phase: f64) -> Vec<Option<Vec<f64>>> {
    (0..n)
        .map(|k| {
            tl.playback_at(address, k as f64 * CHECK_STEP_SECONDS + phase)
                .and_then(|v| numeric(&v))
        })
        .collect()
}

/// The flashes of one address: `(first frame, frames)` of every excursion of up to
/// [`MAX_FLASH_FRAMES`] frames that leaves the value (or the ramp it is on) and returns to it.
/// `hidden[k]`: a loop plays the address at frame `k` (never a flash).
fn flashes(values: &[Option<Vec<f64>>], hidden: &[bool], tol: f64) -> Vec<(usize, usize)> {
    let (away, back) = (AWAY * tol, BACK * tol);
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut taken = vec![false; values.len()];
    for width in 1..=MAX_FLASH_FRAMES {
        for k in 2..values.len().saturating_sub(width + 1) {
            let end = k + width; // the frame the value is back at
            if (k - 2..=end + 1).any(|j| hidden[j]) || (k..end).any(|j| taken[j]) {
                continue;
            }
            let all: Option<Vec<&Vec<f64>>> =
                (k - 2..=end + 1).map(|j| values[j].as_ref()).collect();
            let Some(all) = all else { continue };
            let dim = all[0].len();
            if all.iter().any(|v| v.len() != dim) {
                continue;
            }
            let (o1, a, b, o2) = (all[0], all[1], all[all.len() - 2], all[all.len() - 1]);
            let sticks_out = (0..dim).any(|c| {
                let slope = (b[c] - a[c]) / (width + 1) as f64;
                // before and after, the value is on one line (a level, or a ramp) that the excursion
                // leaves and comes back to
                let before = (o1[c] - (a[c] - slope)).abs() <= back;
                let after = (o2[c] - (b[c] + slope)).abs() <= back;
                let level = (b[c] - a[c]).abs() <= back;
                // how far each frame of the excursion is from the line between the two sides
                let d: Vec<f64> = (1..=width)
                    .map(|i| all[1 + i][c] - (a[c] + slope * i as f64))
                    .collect();
                let above = d.iter().all(|v| *v > back);
                let below = d.iter().all(|v| *v < -back);
                let peak = d.iter().fold(0.0f64, |m, v| m.max(v.abs()));
                let (top, bottom) = d
                    .iter()
                    .fold((f64::MIN, f64::MAX), |(t, b), v| (t.max(*v), b.min(*v)));
                // frames level with each other (two or three) are a pulse, not a flash
                let flat = width >= 2 && top - bottom <= back;
                // a clear excursion back to the same level is also a flash where the value ends a
                // ramp or starts one: one side then is not on the line
                let steady = (before && after) || (level && (before || after) && peak > 2.0 * away);
                steady && (above || below) && peak > away && !flat
            });
            if sticks_out {
                (k..end).for_each(|j| taken[j] = true);
                out.push((k, width));
            }
        }
    }
    out.sort_unstable();
    out
}

/// The addresses of the song that a repair may look at, with their first value's size.
fn candidates<'a>(map: &'a SongLightingCueMap, tl: &'a Timeline) -> Vec<&'a str> {
    let skip = protected(map);
    tl.addresses()
        .into_iter()
        .filter(|a| fixture_of(a).is_some() && !skip.contains(a))
        .collect()
}

/// Count the one-frame flashes a song plays (loops excluded: they are not changed).
pub fn scan(map: &SongLightingCueMap) -> Scan {
    scan_with(map, &Timeline::new(&map.osc_cues, &map.effects), 0.0).0
}

/// The scan at one grid phase, and the addresses that have a flash on it.
fn scan_with(map: &SongLightingCueMap, tl: &Timeline, phase: f64) -> (Scan, BTreeSet<String>) {
    let n = frames(song_end(map));
    let mut out = Scan::default();
    let mut fixtures: BTreeSet<&str> = BTreeSet::new();
    let mut addresses = BTreeSet::new();
    for address in candidates(map, tl) {
        let values = grid(tl, address, n, phase);
        let Some(dim) = values.iter().flatten().next().map(Vec::len) else {
            continue;
        };
        if !is_continuous(address, dim) {
            continue;
        }
        let hidden: Vec<bool> = (0..n)
            .map(|k| tl.in_effect(address, k as f64 * CHECK_STEP_SECONDS + phase))
            .collect();
        let found = find_and_clean(&values, &hidden, tolerance_for(address, 1.0)).0;
        if !found.is_empty() {
            out.flashes += found.len();
            out.found.insert(address.to_string(), found);
            if let Some(f) = fixture_of(address) {
                fixtures.insert(f);
            }
            addresses.insert(address.to_string());
        }
    }
    out.lights = fixtures.len();
    (out, addresses)
}

fn messages_of(rows: &[OscLightingCue]) -> usize {
    rows.iter().map(|r| r.messages.len()).sum()
}

/// The frames of one address (`None`: no value yet, or not a number).
type Frames = Vec<Option<Vec<f64>>>;

/// The flashes of an address and its frames with every flash taken out: the frames of a flash lie
/// on the line between the frames around it. Every other frame stays exactly as it was. A flash
/// next to another one is found in a second look (at most three).
fn find_and_clean(
    values: &[Option<Vec<f64>>],
    hidden: &[bool],
    tol: f64,
) -> (Vec<(usize, usize)>, Frames) {
    let mut out = values.to_vec();
    let mut all = Vec::new();
    for _ in 0..3 {
        let found = flashes(&out, hidden, tol);
        if found.is_empty() {
            break;
        }
        let from = out.clone();
        for &(k, width) in &found {
            let (Some(a), Some(b)) = (&from[k - 1], &from[k + width]) else {
                continue;
            };
            for (i, frame) in out[k..k + width].iter_mut().enumerate() {
                let f = (i + 1) as f64 / (width + 1) as f64;
                if let Some(x) = frame.as_mut() {
                    for (c, v) in x.iter_mut().enumerate() {
                        *v = a[c] + (b[c] - a[c]) * f;
                    }
                }
            }
        }
        all.extend(found);
    }
    all.sort_unstable();
    all.dedup();
    (all, out)
}

/// The samples the fit gets for one address: each frame that differs from the one before it, and
/// the frame before that (the value it held), so a hold stays a hold and a step stays a step.
fn samples(address: &str, frames: &[Option<Vec<f64>>]) -> Vec<(f64, FixtureCommand)> {
    let command = |k: usize| {
        let v = frames[k].as_ref().expect("a frame with a value");
        (
            k as f64 * CHECK_STEP_SECONDS,
            FixtureCommand::new(address, v.iter().map(|f| FixtureArg::Float(*f)).collect()),
        )
    };
    let mut out: Vec<usize> = Vec::new();
    let mut push = |k: usize| {
        if out.last() != Some(&k) {
            out.push(k);
        }
    };
    for k in 0..frames.len() {
        let Some(now) = &frames[k] else { continue };
        let before = k.checked_sub(1).and_then(|p| frames[p].as_ref());
        if before != Some(now) {
            if before.is_some() {
                push(k - 1);
            }
            push(k);
        }
    }
    if let Some(last) = (0..frames.len()).rev().find(|&k| frames[k].is_some()) {
        push(last);
    }
    out.into_iter().map(command).collect()
}

/// Repair the flashes of a saved song (see the module documentation). The song is returned as it
/// is, with the same counts, when it has none. Loops are never changed.
pub fn clean_glitches(map: &SongLightingCueMap) -> Cleaned {
    let full = Timeline::new(&map.osc_cues, &map.effects);
    let (before, mut wanted) = scan_with(map, &full, PHASES[0]);
    let mut out = Cleaned {
        map: map.clone(),
        flashes_before: before.flashes,
        flashes_after: before.flashes,
        lights: before.lights,
        rows_before: map.osc_cues.len(),
        rows_after: map.osc_cues.len(),
        messages_before: messages_of(&map.osc_cues),
        messages_after: messages_of(&map.osc_cues),
        repaired: 0,
        kept: 0,
        max_deviation: 0.0,
    };
    // the rows alone: what a loop hides is not repaired, and not played back
    let rows = Timeline::new(&map.osc_cues, &[]);
    // the addresses that have a flash on either grid phase (a flash shorter than a frame is
    // on one or the other)
    wanted.extend(scan_with(map, &full, PHASES[1]).1);
    if wanted.is_empty() {
        return out;
    }
    let end = song_end(map);
    let n = frames(end);
    let duration = (n - 1) as f64 * CHECK_STEP_SECONDS;
    let mut cleaned: Vec<(f64, FixtureCommand)> = Vec::new();
    for address in &wanted {
        let tol = tolerance_for(address, 1.0);
        let frames = grid(&rows, address, n, 0.0);
        cleaned.extend(samples(
            address,
            &find_and_clean(&frames, &vec![false; n], tol).1,
        ));
    }
    let params = SmoothParams {
        tolerance_scale: FIT_SCALE,
        detect_effects: false,
    };
    let fitted = fit(&[], &cleaned, duration, &params);
    let fitted_rows = smooth_rows(&fitted.rows);
    // the fitted rows of each address, one message per row (to play one address alone)
    let mut new_rows: BTreeMap<String, Vec<OscLightingCue>> = BTreeMap::new();
    for row in &fitted_rows {
        for message in &row.messages {
            let mut single = row.clone();
            single.messages = vec![message.clone()];
            new_rows
                .entry(message.address.clone())
                .or_default()
                .push(single);
        }
    }
    // an address is taken from the repair only when the song it plays stays near the original
    let mut take: BTreeSet<&str> = BTreeSet::new();
    for address in &wanted {
        let tol = tolerance_for(address, 1.0);
        let candidate = Timeline::new(new_rows.get(address).map_or(&[][..], Vec::as_slice), &[]);
        let original = grid(&rows, address, n, 0.0);
        let hidden = vec![false; n];
        let flash_set: BTreeSet<usize> = find_and_clean(&original, &hidden, tol)
            .0
            .into_iter()
            .flat_map(|(k, w)| k..k + w)
            .collect();
        let repaired = grid(&candidate, address, n, 0.0);
        let mut worst = 0.0f64;
        for k in 0..n {
            if flash_set.contains(&k) {
                continue;
            }
            worst = worst.max(match (&original[k], &repaired[k]) {
                (None, None) => 0.0,
                (Some(a), Some(b)) if a.len() == b.len() => a
                    .iter()
                    .zip(b)
                    .map(|(x, y)| (x - y).abs() / tol)
                    .fold(0.0, f64::max),
                _ => f64::INFINITY,
            });
        }
        if worst <= 1.0 && new_rows.contains_key(address) {
            take.insert(address);
            out.max_deviation = out.max_deviation.max(worst);
        } else {
            out.kept += 1;
        }
    }
    out.repaired = take.len();
    if take.is_empty() {
        return out;
    }
    // the rows: every message of a repaired address leaves its row; a row left empty goes
    let mut rows_out: Vec<OscLightingCue> = Vec::with_capacity(map.osc_cues.len());
    for row in &map.osc_cues {
        if !row
            .messages
            .iter()
            .any(|m| take.contains(m.address.as_str()))
        {
            rows_out.push(row.clone());
            continue;
        }
        let mut row = row.clone();
        row.messages.retain(|m| !take.contains(m.address.as_str()));
        if row.messages.is_empty() {
            continue;
        }
        relabel(&mut row);
        rows_out.push(row);
    }
    for row in fitted_rows {
        let mut row = row;
        row.messages.retain(|m| take.contains(m.address.as_str()));
        if !row.messages.is_empty() {
            relabel(&mut row);
            rows_out.push(row);
        }
    }
    rows_out.sort_by(|a, b| {
        a.at_seconds
            .total_cmp(&b.at_seconds)
            .then_with(|| a.fade_seconds.total_cmp(&b.fade_seconds))
    });
    out.map.osc_cues = rows_out;
    out.rows_after = out.map.osc_cues.len();
    out.messages_after = messages_of(&out.map.osc_cues);
    out.flashes_after = scan(&out.map).flashes;
    out
}

/// "Look - N fixtures" says how many fixtures the row names: say it again after messages left.
fn relabel(row: &mut OscLightingCue) {
    if !(row.label.starts_with("Look - ")
        && (row.label.ends_with(" fixtures") || row.label.ends_with(" fixture")))
    {
        return;
    }
    let n = row
        .messages
        .iter()
        .filter_map(|m| fixture_of(&m.address))
        .collect::<BTreeSet<_>>()
        .len();
    row.label = format!("Look - {n} fixture{}", if n == 1 { "" } else { "s" });
}

#[cfg(test)]
mod tests {
    use super::*;
    use stagepilot_core::config::{
        EffectMember, EffectStep, LightingEffect, OscArgument, OscMessage,
    };
    use uuid::Uuid;

    const DT: f64 = CHECK_STEP_SECONDS;

    fn row(
        at: f64,
        address: &str,
        value: Vec<f64>,
        source: OscCueSource,
        edited: bool,
    ) -> OscLightingCue {
        OscLightingCue {
            id: Uuid::new_v4(),
            at_seconds: at,
            address: "/stagepilot/look".into(),
            args: Vec::new(),
            label: "Look - 1 fixture".into(),
            color: None,
            messages: vec![OscMessage {
                address: address.into(),
                args: value
                    .into_iter()
                    .map(|v| OscArgument::Float(v as f32 as f64))
                    .collect(),
            }],
            source,
            edited,
            fade_seconds: 0.0,
        }
    }

    /// A song of instant rows, one per 25 ms frame whose value differs from the one before: the
    /// frames of `address` are `value(k)`.
    fn rows_of(address: &str, frames: usize, value: impl Fn(usize) -> f64) -> Vec<OscLightingCue> {
        let mut out = Vec::new();
        let mut last = f64::NAN;
        for k in 0..frames {
            let v = (value(k) * 10_000.0).round() / 10_000.0;
            if v != last {
                out.push(row(
                    k as f64 * DT,
                    address,
                    vec![v],
                    OscCueSource::Captured,
                    false,
                ));
                last = v;
            }
        }
        out
    }

    fn song(rows: Vec<OscLightingCue>, effects: Vec<LightingEffect>) -> SongLightingCueMap {
        SongLightingCueMap {
            song_key: "s".into(),
            song_title: "Synthetic".into(),
            cues: Vec::new(),
            osc_cues: rows,
            loudness: None,
            captured_edited: false,
            capture: None,
            effects,
        }
    }

    fn played(map: &SongLightingCueMap, address: &str, at: f64) -> f64 {
        let tl = Timeline::new(&map.osc_cues, &map.effects);
        match tl.playback_at(address, at).expect("a value")[0] {
            FixtureArg::Float(f) => f,
            _ => panic!("not a number"),
        }
    }

    const DIM: &str = "/fixture/F1/overrides/dimmer";
    const FRAMES: usize = 400;

    /// A slow ramp up and down with the given frames overwritten.
    fn ramp_with(overwrite: &[(usize, f64)]) -> SongLightingCueMap {
        let base = |k: usize| 0.2 + 0.6 * (1.0 - ((k as f64 - 200.0) / 200.0).abs());
        song(
            rows_of(DIM, FRAMES, |k| {
                overwrite
                    .iter()
                    .find(|(f, _)| *f == k)
                    .map_or(base(k), |(_, v)| *v)
            }),
            Vec::new(),
        )
    }

    fn worst_difference(a: &SongLightingCueMap, b: &SongLightingCueMap, skip: &[usize]) -> f64 {
        (0..FRAMES)
            .filter(|k| !skip.contains(k))
            .map(|k| (played(a, DIM, k as f64 * DT) - played(b, DIM, k as f64 * DT)).abs())
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_one_frame_flash_is_found_and_removed() {
        let map = ramp_with(&[(120, 0.9)]);
        assert_eq!(scan(&map).flashes, 1);
        let c = clean_glitches(&map);
        assert_eq!((c.flashes_before, c.flashes_after), (1, 0));
        assert_eq!(c.lights, 1);
        assert_eq!(scan(&c.map).flashes, 0);
        // the flash frame is back on the ramp, every other frame within one tolerance
        let clean = ramp_with(&[]);
        assert!((played(&c.map, DIM, 120.0 * DT) - played(&clean, DIM, 120.0 * DT)).abs() < 0.03);
        assert!(worst_difference(&c.map, &map, &[120]) <= tolerance_for(DIM, 1.0));
    }

    #[test]
    fn a_flash_back_to_an_older_value_in_a_hold_is_removed() {
        // 0.5 held, one frame at 0.1, back to 0.5
        let map = song(
            rows_of(DIM, FRAMES, |k| {
                if k == 100 {
                    0.1
                } else if k < 250 {
                    0.5
                } else {
                    0.9
                }
            }),
            Vec::new(),
        );
        let c = clean_glitches(&map);
        assert_eq!((c.flashes_before, c.flashes_after), (1, 0));
        assert!((played(&c.map, DIM, 100.0 * DT) - 0.5).abs() < 0.03);
        assert!((played(&c.map, DIM, 300.0 * DT) - 0.9).abs() < 0.03);
    }

    #[test]
    fn a_flash_on_a_ramp_is_removed_and_a_two_frame_dip_too() {
        let map = ramp_with(&[(60, 0.8), (150, 0.2), (151, 0.5)]);
        assert_eq!(scan(&map).flashes, 2);
        let c = clean_glitches(&map);
        assert_eq!(c.flashes_after, 0);
        assert!(worst_difference(&c.map, &map, &[60, 150, 151]) <= tolerance_for(DIM, 1.0));
    }

    #[test]
    fn a_pulse_of_two_frames_and_a_step_survive() {
        // a level pulse of 2 frames (50 ms) at 0.9, and a step 0.2 -> 0.7 at frame 250
        let map = song(
            rows_of(DIM, FRAMES, |k| {
                if k == 100 || k == 101 {
                    0.9
                } else if k < 250 {
                    0.2
                } else {
                    0.7
                }
            }),
            Vec::new(),
        );
        assert_eq!(scan(&map).flashes, 0);
        let c = clean_glitches(&map);
        assert_eq!(
            c.map, map,
            "nothing to repair: the song is returned as it was"
        );
        assert_eq!(
            (c.repaired, c.rows_before, c.rows_after),
            (0, c.rows_before, c.rows_before)
        );
    }

    #[test]
    fn a_pulse_of_two_frames_next_to_a_flash_still_survives() {
        let map = song(
            rows_of(DIM, FRAMES, |k| match k {
                100 | 101 => 0.9,
                200 => 0.8,
                k if k < 250 => 0.2,
                _ => 0.7,
            }),
            Vec::new(),
        );
        let c = clean_glitches(&map);
        assert_eq!((c.flashes_before, c.flashes_after), (1, 0));
        assert!(played(&c.map, DIM, 100.0 * DT) > 0.85);
        assert!(played(&c.map, DIM, 101.0 * DT) > 0.85);
        assert!(played(&c.map, DIM, 200.0 * DT) < 0.25);
        assert!(played(&c.map, DIM, 300.0 * DT) > 0.65);
    }

    #[test]
    fn loops_are_never_changed() {
        let effect = LightingEffect {
            id: Uuid::new_v4(),
            label: "pulse".into(),
            start_seconds: 2.0,
            end_seconds: 9.0,
            period_seconds: 1.0,
            members: vec![EffectMember {
                fixture: "L1".into(),
                offset_seconds: 0.0,
            }],
            cycle: vec![
                EffectStep {
                    at_seconds: 0.0,
                    property: "dimmer".into(),
                    value: vec![0.1],
                    fade_seconds: 0.5,
                },
                EffectStep {
                    at_seconds: 0.5,
                    property: "dimmer".into(),
                    value: vec![0.9],
                    fade_seconds: 0.5,
                },
            ],
            source: OscCueSource::Captured,
            edited: false,
        };
        let mut map = ramp_with(&[(120, 0.9)]);
        map.effects = vec![effect];
        let c = clean_glitches(&map);
        assert_eq!(c.map.effects, map.effects);
        assert_eq!(c.flashes_after, 0);
    }

    #[test]
    fn manual_and_edited_rows_are_never_changed() {
        let mut rows = ramp_with(&[(120, 0.9)]).osc_cues;
        // the same flash on two other addresses, one in a manual row, one in an edited captured row
        for (address, source, edited) in [
            ("/fixture/F2/overrides/dimmer", OscCueSource::Manual, false),
            ("/fixture/F3/overrides/dimmer", OscCueSource::Captured, true),
        ] {
            rows.push(row(1.0, address, vec![0.4], source, edited));
            rows.push(row(1.0 + DT, address, vec![0.9], source, edited));
            rows.push(row(1.0 + 2.0 * DT, address, vec![0.4], source, edited));
        }
        let map = song(rows, Vec::new());
        assert_eq!(
            scan(&map).flashes,
            1,
            "only the captured, unedited address counts"
        );
        let c = clean_glitches(&map);
        for r in map
            .osc_cues
            .iter()
            .filter(|r| r.source == OscCueSource::Manual || r.edited)
        {
            assert!(
                c.map.osc_cues.contains(r),
                "row at {} was changed",
                r.at_seconds
            );
        }
        assert_eq!(c.flashes_after, 0);
    }

    #[test]
    fn rows_that_do_not_hold_flashes_stay_as_they_were() {
        let mut map = ramp_with(&[(120, 0.9)]);
        // an option (text) and a gobo-like float: not continuous, never touched
        map.osc_cues.push(row(
            1.0,
            "/fixture/F1/overrides/gobo",
            vec![0.1],
            OscCueSource::Captured,
            false,
        ));
        map.osc_cues.push(row(
            1.0 + DT,
            "/fixture/F1/overrides/gobo",
            vec![0.9],
            OscCueSource::Captured,
            false,
        ));
        map.osc_cues.push(row(
            1.0 + 2.0 * DT,
            "/fixture/F1/overrides/gobo",
            vec![0.1],
            OscCueSource::Captured,
            false,
        ));
        let c = clean_glitches(&map);
        let gobo = |m: &SongLightingCueMap| -> Vec<OscLightingCue> {
            m.osc_cues
                .iter()
                .filter(|r| r.messages[0].address.ends_with("gobo"))
                .cloned()
                .collect()
        };
        assert_eq!(gobo(&c.map), gobo(&map));
        assert_eq!(c.flashes_before, 1);
    }

    #[test]
    fn a_colour_flash_on_one_channel_is_removed() {
        let colour = "/fixture/F1/overrides/color";
        let rows = (0..FRAMES)
            .map(|k| {
                let v = if k == 80 {
                    vec![0.2, 0.9, 0.2]
                } else {
                    vec![0.2, 0.3, 0.2]
                };
                row(k as f64 * DT, colour, v, OscCueSource::Captured, false)
            })
            .collect();
        let map = song(rows, Vec::new());
        assert_eq!(scan(&map).flashes, 1);
        let c = clean_glitches(&map);
        assert_eq!(c.flashes_after, 0);
        let tl = Timeline::new(&c.map.osc_cues, &[]);
        let v = tl.playback_at(colour, 80.0 * DT).expect("a colour");
        assert!(matches!(v[1], FixtureArg::Float(g) if (g - 0.3).abs() < 0.03));
    }

    #[test]
    fn a_song_with_nothing_to_repair_comes_back_the_same() {
        let map = ramp_with(&[]);
        let c = clean_glitches(&map);
        assert_eq!(c.map, map);
        assert_eq!((c.flashes_before, c.flashes_after, c.repaired), (0, 0, 0));
    }

    #[test]
    fn the_repair_is_stable_and_repeating_it_finds_nothing() {
        let c = clean_glitches(&ramp_with(&[(60, 0.8), (150, 0.2), (151, 0.5), (300, 0.9)]));
        let again = clean_glitches(&c.map);
        assert_eq!(again.flashes_before, 0);
        assert_eq!(again.map, c.map);
    }

    #[test]
    fn a_flash_shorter_than_a_frame_is_removed_too() {
        // 0.5 held; 0.9 for 10 ms that falls between two frames of the 25 ms grid, then 0.5 again
        let mut rows = vec![row(0.0, DIM, vec![0.5], OscCueSource::Captured, false)];
        rows.push(row(1.0105, DIM, vec![0.9], OscCueSource::Captured, false));
        rows.push(row(1.0205, DIM, vec![0.5], OscCueSource::Captured, false));
        rows.push(row(5.0, DIM, vec![0.8], OscCueSource::Captured, false));
        let map = song(rows, Vec::new());
        assert_eq!(scan(&map).flashes, 0, "no frame of the grid sees it");
        assert!(
            (played(&map, DIM, 1.0125) - 0.9).abs() < 1e-6,
            "but a replay can"
        );
        let c = clean_glitches(&map);
        assert!((played(&c.map, DIM, 1.0125) - 0.5).abs() < 0.03);
        assert!((played(&c.map, DIM, 6.0) - 0.8).abs() < 0.03);
    }
}
