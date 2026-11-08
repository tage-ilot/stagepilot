//! Fitting a recording inside the per-song caps, with a guaranteed end. Pure: no I/O, no clock
//! (the elapsed time it reports is the only thing it reads).
//!
//! A recording is first fitted exactly ([`crate::lightkey_smooth::fit`], tolerance x1). When the
//! result is over the caps ([`Caps`]) it goes down this ladder and stops at the first stage that
//! fits:
//!
//! 1. **Exact** fit, tolerance x1: the normal case.
//! 2. **Looser** tolerance x2..x32. Ramps get fewer; steps (options, text, bools, values more than
//!    0.5 s apart) do not, so this alone cannot save a step-heavy song. The smallest candidate
//!    seen is kept and one with more items than a tighter fit is never taken. It stops when two
//!    doublings in a row gain less than 5 %.
//! 3. **Thinning**: per address, changes closer than T seconds collapse to the last value of each
//!    T-window (T = 0.1, 0.25, 0.5, 1, 2, 4, 8). The final value of every address stays.
//! 4. **Trim** (the guarantee): drops whole units (a step with the ramps that follow it) with the
//!    smallest effect (size of the change x time until the next change), taking from the busiest
//!    addresses first, until the song is inside the caps. A ramp is never left half removed.
//!
//! At most 13 full fits run (1 + 5 + 7). A fit budget ([`FIT_BUDGET_MS`]) sends a slow song
//! straight to the trim, which is linear in the song.

use crate::lightkey_fixtures::{FixtureArg, FixtureCommand};
use crate::lightkey_smooth::{self, is_angle, SmoothCapture, SmoothLook, SmoothParams};
use stagepilot_core::config::{
    LightingEffect, MAX_EFFECTS_PER_SONG, MAX_EFFECT_CYCLE_STEPS_PER_SONG,
    MAX_OSC_MESSAGES_PER_ROW, MAX_OSC_MESSAGES_PER_SONG, MAX_OSC_ROWS_PER_SONG,
};
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::time::Instant;

/// Tolerance multipliers of stage 2.
pub const LOOSEN_SCALES: [f64; 5] = [2.0, 4.0, 8.0, 16.0, 32.0];
/// Thinning windows of stage 3, seconds.
pub const THIN_SECONDS: [f64; 7] = [0.1, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0];
/// No new fit starts after this long; the best candidate so far goes to the trim.
pub const FIT_BUDGET_MS: u128 = 30_000;
/// A song this many times over the caps is not loosened (ramps cannot shrink it that far).
const HOPELESS_LOAD: f64 = 25.0;
/// A looser fit must be at least this much smaller (as a share) to replace a tighter one.
const MIN_GAIN: f64 = 0.95;

/// The per-song caps the fit aims at. `Caps::default()` is the timeline's own limits; tests use
/// tiny ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub rows: usize,
    pub messages: usize,
    pub effects: usize,
    pub cycle_steps: usize,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            rows: MAX_OSC_ROWS_PER_SONG,
            messages: MAX_OSC_MESSAGES_PER_SONG,
            effects: MAX_EFFECTS_PER_SONG,
            cycle_steps: MAX_EFFECT_CYCLE_STEPS_PER_SONG,
        }
    }
}

impl Caps {
    /// The caps of an older build (1,000 rows, 15,000 messages, 200 effects, 20,000 steps).
    pub const OLD: Caps = Caps {
        rows: 1_000,
        messages: 15_000,
        effects: 200,
        cycle_steps: 20_000,
    };

    pub fn fits(&self, size: &Size) -> bool {
        size.rows <= self.rows
            && size.messages <= self.messages
            && size.effects <= self.effects
            && size.cycle_steps <= self.cycle_steps
    }

    /// How far over the caps a size is: 1.0 or less fits.
    pub fn load(&self, size: &Size) -> f64 {
        let ratio = |n: usize, cap: usize| n as f64 / cap.max(1) as f64;
        ratio(size.rows, self.rows)
            .max(ratio(size.messages, self.messages))
            .max(ratio(size.effects, self.effects))
            .max(ratio(size.cycle_steps, self.cycle_steps))
    }

    /// Caps `share` (0..=1) of the way down from `size`, never above these caps: where the
    /// trim aims when the timeline had less room than these caps promised.
    pub fn below(&self, size: &Size, share: f64) -> Caps {
        let s = |n: usize, cap: usize| (((n as f64) * share).floor() as usize).min(cap);
        Caps {
            rows: s(size.rows, self.rows).max(1),
            messages: s(size.messages, self.messages).max(1),
            effects: s(size.effects, self.effects),
            cycle_steps: s(size.cycle_steps, self.cycle_steps),
        }
    }
}

/// What a fit comes to, in the units the caps count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    /// Timeline rows after a row over the per-row message cap is split.
    pub rows: usize,
    pub messages: usize,
    pub effects: usize,
    pub cycle_steps: usize,
}

impl Size {
    pub fn of(rows: &[SmoothLook], effects: &[LightingEffect]) -> Size {
        Size {
            rows: rows
                .iter()
                .map(|r| r.messages.len().div_ceil(MAX_OSC_MESSAGES_PER_ROW))
                .sum(),
            messages: rows.iter().map(|r| r.messages.len()).sum(),
            effects: effects.len(),
            cycle_steps: effects.iter().map(|e| e.cycle.len()).sum(),
        }
    }

    pub fn of_capture(capture: &SmoothCapture) -> Size {
        Size::of(&capture.rows, &capture.effects)
    }

    /// Timeline items: messages and loop steps ("changes" in the operator's text).
    pub fn items(&self) -> usize {
        self.messages + self.cycle_steps
    }
}

/// How far down the ladder a song went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Exact,
    Loosened,
    Thinned,
    Trimmed,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Exact => "exact",
            Stage::Loosened => "loosened",
            Stage::Thinned => "thinned",
            Stage::Trimmed => "trimmed",
        }
    }
}

/// One full fit, for the log and the offline tool.
#[derive(Debug, Clone)]
pub struct AttemptReport {
    pub stage: Stage,
    pub scale: f64,
    pub thin_seconds: f64,
    pub size: Size,
    pub ms: u128,
}

/// The outcome: a capture inside the caps and how it got there.
#[derive(Debug, Clone)]
pub struct Fitted {
    pub capture: SmoothCapture,
    pub stage: Stage,
    pub scale: f64,
    pub thin_seconds: f64,
    /// Timeline items (messages and loop steps) the trim dropped.
    pub trimmed: usize,
    /// Changes the recording held.
    pub raw_changes: usize,
    /// Items the exact fit needed, and items the saved capture has.
    pub exact_items: usize,
    pub kept_items: usize,
    /// Full fits that ran.
    pub fits: usize,
    pub fit_ms: u128,
    pub attempts: Vec<AttemptReport>,
}

struct Candidate {
    capture: SmoothCapture,
    size: Size,
    scale: f64,
    thin: f64,
}

#[allow(clippy::too_many_arguments)]
fn attempt(
    baseline: &[FixtureCommand],
    changes: &[(f64, FixtureCommand)],
    duration: f64,
    scale: f64,
    thin: f64,
    stage: Stage,
    attempts: &mut Vec<AttemptReport>,
) -> Candidate {
    let started = Instant::now();
    let params = SmoothParams {
        tolerance_scale: scale,
        ..SmoothParams::default()
    };
    let capture = if thin > 0.0 {
        lightkey_smooth::fit(baseline, &thin_changes(changes, thin), duration, &params)
    } else {
        lightkey_smooth::fit(baseline, changes, duration, &params)
    };
    let size = Size::of_capture(&capture);
    attempts.push(AttemptReport {
        stage,
        scale,
        thin_seconds: thin,
        size,
        ms: started.elapsed().as_millis(),
    });
    Candidate {
        capture,
        size,
        scale,
        thin,
    }
}

/// Fit `changes` inside `caps`. Always returns a capture inside the caps (an empty one only when
/// the caps are zero); `exact_items == 0` means nothing was recorded.
pub fn fit_within_caps(
    baseline: &[FixtureCommand],
    changes: &[(f64, FixtureCommand)],
    duration: f64,
    caps: &Caps,
) -> Fitted {
    fit_within_caps_budgeted(baseline, changes, duration, caps, FIT_BUDGET_MS)
}

/// [`fit_within_caps`] with its own time budget: no new fit starts once `budget_ms` has passed
/// (tests that need the whole ladder pass `u128::MAX`).
pub fn fit_within_caps_budgeted(
    baseline: &[FixtureCommand],
    changes: &[(f64, FixtureCommand)],
    duration: f64,
    caps: &Caps,
    budget_ms: u128,
) -> Fitted {
    let started = Instant::now();
    let over_budget = || started.elapsed().as_millis() > budget_ms;
    let mut attempts = Vec::new();
    let mut best = attempt(
        baseline,
        changes,
        duration,
        1.0,
        0.0,
        Stage::Exact,
        &mut attempts,
    );
    let exact_items = best.size.items();
    let done = |best: Candidate, trimmed: usize, attempts: Vec<AttemptReport>, started: Instant| {
        let stage = if trimmed > 0 {
            Stage::Trimmed
        } else if best.thin > 0.0 {
            Stage::Thinned
        } else if best.scale > 1.0 {
            Stage::Loosened
        } else {
            Stage::Exact
        };
        let fits = attempts.len();
        Fitted {
            kept_items: Size::of_capture(&best.capture).items(),
            capture: best.capture,
            stage,
            scale: best.scale,
            thin_seconds: best.thin,
            trimmed,
            raw_changes: changes.len(),
            exact_items,
            fits,
            fit_ms: started.elapsed().as_millis(),
            attempts,
        }
    };
    if caps.fits(&best.size) {
        return done(best, 0, attempts, started);
    }

    // stage 2: a looser tolerance
    if caps.load(&best.size) <= HOPELESS_LOAD {
        let mut stale = 0;
        for scale in LOOSEN_SCALES {
            if over_budget() {
                break;
            }
            let candidate = attempt(
                baseline,
                changes,
                duration,
                scale,
                0.0,
                Stage::Loosened,
                &mut attempts,
            );
            let (now, before) = (caps.load(&candidate.size), caps.load(&best.size));
            if now < before && (now <= 1.0 || now < before * MIN_GAIN) {
                best = candidate;
                stale = 0;
            } else {
                stale += 1;
            }
            if caps.fits(&best.size) {
                return done(best, 0, attempts, started);
            }
            if stale >= 2 {
                break;
            }
        }
    }

    // stage 3: merge changes that are close in time
    for thin in THIN_SECONDS {
        if over_budget() {
            break;
        }
        let candidate = attempt(
            baseline,
            changes,
            duration,
            best.scale,
            thin,
            Stage::Thinned,
            &mut attempts,
        );
        if caps.load(&candidate.size) < caps.load(&best.size) {
            best = candidate;
        }
        if caps.fits(&best.size) {
            return done(best, 0, attempts, started);
        }
    }

    // stage 4: the guarantee
    let (capture, trimmed) = trim_to_caps(&best.capture, caps);
    best.capture = capture;
    done(best, trimmed.max(1), attempts, started)
}

/// Per address, changes closer together than `seconds` collapse to the last value of each
/// `seconds`-window (windows start at 0; changes at or before 0 are the start state and form
/// their own window). The final value of every address stays. The result keeps the order in which
/// each address first appears.
pub fn thin_changes(changes: &[(f64, FixtureCommand)], seconds: f64) -> Vec<(f64, FixtureCommand)> {
    if !(seconds.is_finite() && seconds > 0.0) {
        return changes.to_vec();
    }
    let mut kept: Vec<usize> = Vec::new();
    let mut slot: HashMap<&str, (i64, usize)> = HashMap::new();
    for (i, (at, command)) in changes.iter().enumerate() {
        let window = if *at <= 0.0 {
            -1
        } else {
            (*at / seconds).floor() as i64
        };
        match slot.get_mut(command.a.as_str()) {
            Some((w, position)) if *w == window => kept[*position] = i,
            Some(entry) => {
                kept.push(i);
                *entry = (window, kept.len() - 1);
            }
            None => {
                kept.push(i);
                slot.insert(&command.a, (window, kept.len() - 1));
            }
        }
    }
    kept.into_iter().map(|i| changes[i].clone()).collect()
}

// ------------------------------------------------------------------------------------- trim

/// One step of an address and the ramps that follow it until the next step: dropped as a whole,
/// so a ramp is never left half removed (nor starting from a value that is no longer there).
struct Unit {
    items: Vec<(usize, usize)>,
    importance: f64,
}

fn distance(address: &str, a: &[FixtureArg], b: &[FixtureArg]) -> f64 {
    if a.len() != b.len() {
        return 1.0;
    }
    let range = if is_angle(address) { 180.0 } else { 1.0 };
    a.iter()
        .zip(b)
        .map(|(x, y)| match (x, y) {
            (FixtureArg::Float(x), FixtureArg::Float(y)) => ((x - y).abs() / range).min(1.0e6),
            _ => f64::from(u8::from(x != y)),
        })
        .fold(0.0, f64::max)
}

fn effect_importance(effect: &LightingEffect) -> f64 {
    let mut spread = 0.0f64;
    for k in 0..effect
        .cycle
        .iter()
        .map(|s| s.value.len())
        .max()
        .unwrap_or(0)
    {
        let values: Vec<f64> = effect
            .cycle
            .iter()
            .filter_map(|s| s.value.get(k))
            .copied()
            .collect();
        let (lo, hi) = values
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(*v), hi.max(*v))
            });
        if hi >= lo {
            spread = spread.max(hi - lo);
        }
    }
    (effect.end_seconds - effect.start_seconds).max(0.0)
        * spread.max(1e-6)
        * effect.members.len().max(1) as f64
}

/// Bring `capture` inside `caps` by dropping the least important parts. Returns the smaller
/// capture and how many timeline items (messages and loop steps) went.
pub fn trim_to_caps(capture: &SmoothCapture, caps: &Caps) -> (SmoothCapture, usize) {
    let mut out = capture.clone();
    let before = Size::of_capture(&out).items();
    if caps.fits(&Size::of_capture(&out)) {
        return (out, 0);
    }

    // effects: the least important go first
    if out.effects.len() > caps.effects
        || out.effects.iter().map(|e| e.cycle.len()).sum::<usize>() > caps.cycle_steps
    {
        let mut order: Vec<usize> = (0..out.effects.len()).collect();
        order.sort_by(|a, b| {
            effect_importance(&out.effects[*a]).total_cmp(&effect_importance(&out.effects[*b]))
        });
        let mut dropped: HashSet<usize> = HashSet::new();
        let mut effects = out.effects.len();
        let mut steps: usize = out.effects.iter().map(|e| e.cycle.len()).sum();
        for i in order {
            if effects <= caps.effects && steps <= caps.cycle_steps {
                break;
            }
            effects -= 1;
            steps -= out.effects[i].cycle.len();
            dropped.insert(i);
        }
        let mut index = 0;
        out.effects.retain(|_| {
            let keep = !dropped.contains(&index);
            index += 1;
            keep
        });
        out.effect_infos.clear();
    }

    trim_rows(&mut out, caps);
    out.ramps = out
        .rows
        .iter()
        .filter(|r| r.fade_seconds > 0.0)
        .map(|r| r.messages.len())
        .sum();
    out.steps = out
        .rows
        .iter()
        .filter(|r| r.fade_seconds <= 0.0)
        .map(|r| r.messages.len())
        .sum();
    let after = Size::of_capture(&out).items();
    (out, before.saturating_sub(after))
}

fn trim_rows(out: &mut SmoothCapture, caps: &Caps) {
    let size = Size::of(&out.rows, &out.effects);
    if size.rows <= caps.rows && size.messages <= caps.messages {
        return;
    }
    // per address, the items in time order: (at, fade, row, message)
    let mut by_address: HashMap<&str, Vec<(f64, f64, usize, usize)>> = HashMap::new();
    let mut song_end = 0.0f64;
    for (r, row) in out.rows.iter().enumerate() {
        song_end = song_end.max(row.at_seconds + row.fade_seconds);
        for (m, message) in row.messages.iter().enumerate() {
            by_address.entry(message.a.as_str()).or_default().push((
                row.at_seconds,
                row.fade_seconds,
                r,
                m,
            ));
        }
    }
    let mut addresses: Vec<(&str, Vec<Unit>)> = Vec::new();
    for (address, mut items) in by_address {
        items.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let value = |item: &(f64, f64, usize, usize)| &out.rows[item.2].messages[item.3].v;
        // units: a step starts one, ramps join the unit before them
        let mut starts: Vec<usize> = Vec::new();
        for (i, item) in items.iter().enumerate() {
            if i == 0 || item.1 <= 0.0 {
                starts.push(i);
            }
        }
        let mut units: Vec<Unit> = Vec::with_capacity(starts.len());
        for (u, &first) in starts.iter().enumerate() {
            let last = starts.get(u + 1).copied().unwrap_or(items.len());
            let slice = &items[first..last];
            let before_value = if u == 0 {
                None
            } else {
                Some(value(&items[first - 1]))
            };
            let size = match before_value {
                None => 1.0,
                Some(prev) => slice
                    .iter()
                    .map(|item| distance(address, prev, value(item)))
                    .fold(0.0, f64::max),
            };
            let next_at = starts.get(u + 1).map_or(song_end, |&n| items[n].0);
            let dt = (next_at - items[first].0).max(1e-3);
            units.push(Unit {
                items: slice.iter().map(|item| (item.2, item.3)).collect(),
                importance: size * dt,
            });
        }
        addresses.push((address, units));
    }
    // the busiest address first; a stable order keeps the result the same from run to run
    addresses.sort_by(|a, b| a.0.cmp(b.0));

    let mut removed: HashSet<(usize, usize)> = HashSet::new();
    // per address: unit indexes by importance, least first, last unit (the final value) excluded
    let mut order: Vec<Vec<usize>> = Vec::with_capacity(addresses.len());
    let mut alive: Vec<usize> = Vec::with_capacity(addresses.len());
    for (_, units) in &addresses {
        let mut idx: Vec<usize> = (0..units.len().saturating_sub(1)).collect();
        idx.sort_by(|a, b| units[*a].importance.total_cmp(&units[*b].importance));
        order.push(idx);
        alive.push(units.len());
    }
    let mut cursor = vec![0usize; addresses.len()];
    let mut heap: BinaryHeap<(usize, std::cmp::Reverse<usize>)> = BinaryHeap::new();
    for (a, count) in alive.iter().enumerate() {
        if !order[a].is_empty() {
            heap.push((*count, std::cmp::Reverse(a)));
        }
    }

    let current = |rows: &[SmoothLook], removed: &HashSet<(usize, usize)>| -> Size {
        let mut size = Size::default();
        for (r, row) in rows.iter().enumerate() {
            let n = (0..row.messages.len())
                .filter(|m| !removed.contains(&(r, *m)))
                .count();
            size.rows += n.div_ceil(MAX_OSC_MESSAGES_PER_ROW);
            size.messages += n;
        }
        size
    };
    let fits = |s: &Size| s.rows <= caps.rows && s.messages <= caps.messages;
    let mut now = current(&out.rows, &removed);
    let mut batch = 1usize;
    while !fits(&now) {
        let Some(&(_, std::cmp::Reverse(_))) = heap.peek() else {
            break;
        };
        // how many units to take this round: enough for the excess, doubling when rows are
        // slow to empty
        let avg = (now.messages as f64 / alive.iter().sum::<usize>().max(1) as f64).max(1.0);
        let by_messages = (now.messages.saturating_sub(caps.messages) as f64 / avg).ceil() as usize;
        let by_rows = if now.rows > caps.rows {
            ((1.0 - caps.rows as f64 / now.rows as f64) * 1.1 * alive.iter().sum::<usize>() as f64)
                .ceil() as usize
        } else {
            0
        };
        let take = by_messages.max(by_rows).max(batch).max(1);
        let mut taken = 0;
        while taken < take {
            let Some((count, std::cmp::Reverse(a))) = heap.pop() else {
                break;
            };
            if count != alive[a] {
                continue;
            }
            let unit = order[a][cursor[a]];
            cursor[a] += 1;
            for item in &addresses[a].1[unit].items {
                removed.insert(*item);
            }
            alive[a] -= 1;
            taken += 1;
            if cursor[a] < order[a].len() {
                heap.push((alive[a], std::cmp::Reverse(a)));
            }
        }
        batch = (batch * 2).min(usize::MAX / 4);
        now = current(&out.rows, &removed);
    }
    if !fits(&now) {
        // Every address is down to its final value and the song is still over the caps: only
        // then do final values go, the least important first.
        let mut lasts: Vec<(f64, usize)> = addresses
            .iter()
            .enumerate()
            .filter_map(|(a, (_, units))| units.last().map(|u| (u.importance, a)))
            .collect();
        lasts.sort_by(|x, y| x.0.total_cmp(&y.0));
        for chunk in lasts.chunks(256) {
            for &(_, a) in chunk {
                if let Some(unit) = addresses[a].1.last() {
                    for item in &unit.items {
                        removed.insert(*item);
                    }
                }
            }
            now = current(&out.rows, &removed);
            if fits(&now) {
                break;
            }
        }
    }
    for (r, row) in out.rows.iter_mut().enumerate() {
        let mut m = 0;
        row.messages.retain(|_| {
            let keep = !removed.contains(&(r, m));
            m += 1;
            keep
        });
    }
    out.rows.retain(|row| !row.messages.is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dim(fixture: &str, value: f64) -> FixtureCommand {
        FixtureCommand::new(
            format!("/fixture/{fixture}/overrides/dimmer"),
            vec![FixtureArg::Float(value)],
        )
    }

    fn option(fixture: &str, value: &str) -> FixtureCommand {
        FixtureCommand::new(
            format!("/fixture/{fixture}/overrides/gobo"),
            vec![FixtureArg::Text(value.to_string())],
        )
    }

    #[test]
    fn thinning_keeps_the_last_value_of_each_window_and_the_final_value() {
        let changes: Vec<(f64, FixtureCommand)> = (0..100)
            .map(|i| (0.01 + f64::from(i) * 0.01, dim("A", f64::from(i) / 100.0)))
            .collect();
        let thin = thin_changes(&changes, 0.25);
        assert!(thin.len() <= 5, "{}", thin.len());
        assert_eq!(thin.last().unwrap().1, changes.last().unwrap().1);
        assert_eq!(thin_changes(&changes, 0.0).len(), changes.len());
    }

    #[test]
    fn a_small_song_is_exact() {
        let changes = vec![(1.0, dim("A", 0.5)), (2.0, dim("A", 1.0))];
        let fitted = fit_within_caps(&[], &changes, 10.0, &Caps::default());
        assert_eq!(fitted.stage, Stage::Exact);
        assert_eq!(fitted.fits, 1);
        assert_eq!(fitted.trimmed, 0);
    }

    #[test]
    fn steps_are_thinned_or_trimmed_into_tiny_caps_and_the_final_value_stays() {
        // 40 s of an option flipping every 50 ms on 3 fixtures
        let mut changes = Vec::new();
        for i in 0..800 {
            for f in ["A", "B", "C"] {
                changes.push((
                    f64::from(i) * 0.05 + 0.01,
                    option(f, &format!("v{}", i % 7)),
                ));
            }
        }
        let caps = Caps {
            rows: 20,
            messages: 60,
            effects: 5,
            cycle_steps: 50,
        };
        let fitted = fit_within_caps(&[], &changes, 40.0, &caps);
        assert!(caps.fits(&Size::of_capture(&fitted.capture)));
        assert!(fitted.fits <= 13);
        assert!(matches!(fitted.stage, Stage::Thinned | Stage::Trimmed));
        // every address ends on its recorded final value
        for f in ["A", "B", "C"] {
            let want = &changes.iter().rev().find(|c| c.1.a.contains(f)).unwrap().1;
            let last = fitted
                .capture
                .rows
                .iter()
                .flat_map(|r| r.messages.iter().map(move |m| (r.at_seconds, m)))
                .filter(|(_, m)| m.a == want.a)
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap();
            assert_eq!(last.1.v, want.v);
        }
    }

    #[test]
    fn trim_drops_whole_ramps_never_half() {
        // two addresses: A is busy (many steps), B has one ramp chain and its final value
        let mut rows = Vec::new();
        for i in 0..50 {
            rows.push(SmoothLook {
                at_seconds: f64::from(i),
                fade_seconds: 0.0,
                messages: vec![dim("A", f64::from(i % 2))],
            });
        }
        rows.push(SmoothLook {
            at_seconds: 10.0,
            fade_seconds: 0.0,
            messages: vec![dim("B", 0.0)],
        });
        rows.push(SmoothLook {
            at_seconds: 10.0,
            fade_seconds: 2.0,
            messages: vec![dim("B", 0.5)],
        });
        rows.push(SmoothLook {
            at_seconds: 12.0,
            fade_seconds: 2.0,
            messages: vec![dim("B", 1.0)],
        });
        let capture = SmoothCapture {
            rows,
            ..SmoothCapture::default()
        };
        let caps = Caps {
            rows: 30,
            messages: 30,
            effects: 0,
            cycle_steps: 0,
        };
        let (trimmed, removed) = trim_to_caps(&capture, &caps);
        assert!(removed > 0);
        assert!(caps.fits(&Size::of_capture(&trimmed)));
        // B's ramp chain is whole or gone: the ramps are present together with their step
        let b: Vec<_> = trimmed
            .rows
            .iter()
            .flat_map(|r| r.messages.iter().map(move |m| (r.fade_seconds, m)))
            .filter(|(_, m)| m.a.contains("/B/"))
            .collect();
        let ramps = b.iter().filter(|(f, _)| *f > 0.0).count();
        assert!(ramps == 0 || ramps == 2, "ramps={ramps}");
        // A's last change is kept
        assert!(trimmed
            .rows
            .iter()
            .any(|r| r.at_seconds == 49.0 && r.messages[0].a.contains("/A/")));
    }
}
