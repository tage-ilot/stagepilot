//! Recorded fixture changes into timeline "look" rows.
//!
//! A capture is fitted with ramps and effects by `crate::lightkey_smooth` and
//! `crate::lightkey_effects` ([`LookRecorder::smooth`]). The thin-and-merge functions
//! ([`thin`], [`merge`], [`simplify`], [`simplify_song`]) are the older way of making rows and
//! still tidy up captures made before ramps existed: they only ever touch instant rows
//! (`fade_seconds` 0) of a song with no effects. Pure: no I/O, no clock.

use crate::lightkey_fixtures::{is_replayable, FixtureArg, FixtureCommand, FixtureState};
use crate::lightkey_smooth::{self, SmoothCapture, SmoothParams};
use stagepilot_core::config::{
    round_tenths, CaptureStatus, LightingEffect, OscCueSource, OscLightingCue, OscMessage,
    SongLightingCueMap, MAX_EFFECTS_PER_SONG, MAX_EFFECT_CYCLE_STEPS_PER_SONG,
    MAX_OSC_MESSAGES_PER_ROW, MAX_OSC_MESSAGES_PER_SONG, MAX_OSC_ROWS_PER_SONG,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use uuid::Uuid;

/// One row of the timeline: everything that changes in one slot of the Detail grid.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub at_seconds: f64,
    pub messages: Vec<FixtureCommand>,
}

/// A level (dimmer, colour channel...) jump past this between two raw changes is kept at once.
const LEVEL_JUMP: f64 = 0.25;
/// An angle (pan, tilt...) jump past this many degrees is kept at once.
const ANGLE_JUMP: f64 = 25.0;
/// Merge tolerance for levels (2 % of the 0..1 range) and for angles (1 degree).
const LEVEL_TOLERANCE: f64 = 0.02;
const ANGLE_TOLERANCE: f64 = 1.0;
const EPSILON: f64 = 1e-9;

/// The last path segment of an address, lower case (`/fixture/MH1/overrides/panAngle` -> `panangle`).
fn property(address: &str) -> String {
    address.rsplit('/').next().unwrap_or("").to_lowercase()
}

fn is_angle(address: &str) -> bool {
    let p = property(address);
    ["pan", "tilt", "angle", "rotat"]
        .iter()
        .any(|k| p.contains(k))
}

fn is_dimmer(address: &str) -> bool {
    let p = property(address);
    p.contains("dimmer") || p.contains("intensity")
}

/// How far two numeric values of this address may differ and still count as the same look.
pub fn tolerance_for(address: &str) -> f64 {
    if is_angle(address) {
        ANGLE_TOLERANCE
    } else {
        LEVEL_TOLERANCE
    }
}

/// A change that must be kept even inside the Detail interval: a dimmer to or from 0 or full,
/// a colour/level jump past 0.25, an angle jump past 25 degrees, or any change that is not a
/// plain number (options, text, a different number of arguments).
fn is_big_jump(address: &str, before: Option<&[FixtureArg]>, after: &[FixtureArg]) -> bool {
    let Some(before) = before else {
        return true;
    };
    if before.len() != after.len() {
        return true;
    }
    for (a, b) in before.iter().zip(after) {
        match (a, b) {
            (FixtureArg::Float(a), FixtureArg::Float(b)) => {
                let d = (a - b).abs();
                if d <= EPSILON {
                    continue;
                }
                if is_angle(address) {
                    if d > ANGLE_JUMP {
                        return true;
                    }
                } else {
                    if d > LEVEL_JUMP {
                        return true;
                    }
                    let edge = |x: f64| x.abs() <= EPSILON || (x - 1.0).abs() <= EPSILON;
                    if is_dimmer(address) && (edge(*a) || edge(*b)) {
                        return true;
                    }
                }
            }
            _ => {
                if a != b {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether two values are the same look: equal, or numbers within the address's tolerance.
fn within_tolerance(address: &str, a: &[FixtureArg], b: &[FixtureArg]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let tolerance = tolerance_for(address);
    a.iter().zip(b).all(|(x, y)| match (x, y) {
        (FixtureArg::Float(x), FixtureArg::Float(y)) => (x - y).abs() <= tolerance + EPSILON,
        _ => x == y,
    })
}

/// Thin recorded changes into rows on the Detail grid (`interval` seconds).
///
/// Per address a change is kept when at least `interval` has passed since the last kept one, when
/// it is a big jump, or when it ends a run (the next change of that address is `interval` or more
/// away, or there is none). Kept changes are placed on the nearest multiple of `interval`; one
/// row holds everything that landed in its slot (the last value of an address wins). Changes at
/// or before t=0 form the starting state at 0.0, and `baseline` supplies the start value of an
/// address that only moves later. The last change of an address after `duration` is folded into
/// the final row at `duration`; earlier ones after it are dropped. `changes` need not be
/// sorted.
pub fn thin(
    baseline: &[FixtureCommand],
    changes: &[(f64, FixtureCommand)],
    duration: f64,
    interval: f64,
) -> Vec<Look> {
    let interval = interval.max(0.01);
    let mut order: HashMap<&str, usize> = HashMap::new();
    for command in baseline {
        let next = order.len();
        order.entry(&command.a).or_insert(next);
    }
    let mut series: HashMap<&str, Vec<(f64, &FixtureCommand)>> = HashMap::new();
    for (at, command) in changes {
        let next = order.len();
        order.entry(&command.a).or_insert(next);
        series.entry(&command.a).or_default().push((*at, command));
    }
    let baseline: HashMap<&str, &FixtureCommand> =
        baseline.iter().map(|c| (c.a.as_str(), c)).collect();

    let mut rows: BTreeMap<i64, HashMap<&str, &FixtureCommand>> = BTreeMap::new();
    for (address, mut list) in series {
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut start: Option<&FixtureCommand> = None;
        let mut changed_at_start = false;
        let mut after_end: Option<&FixtureCommand> = None;
        let mut later: Vec<(f64, &FixtureCommand)> = Vec::new();
        for (at, command) in list {
            if at <= 0.0 {
                start = Some(command);
                changed_at_start = true;
            } else if duration <= 0.0 || at <= duration {
                later.push((at, command));
            } else {
                after_end = Some(command);
            }
        }
        // A song that overruns keeps its last look (a fade-out, say): the final value after the
        // scheduled end lands in the final row at `duration` instead of being dropped (R-02).
        if let Some(command) = after_end {
            later.push((duration, command));
        }
        if later.is_empty() && !changed_at_start {
            continue;
        }
        let start = start.or_else(|| baseline.get(address).copied());
        if let Some(start) = start {
            rows.entry(0).or_default().insert(address, start);
        }
        let mut last_kept_at = 0.0;
        let mut previous: Option<&[FixtureArg]> = start.map(|c| c.v.as_slice());
        for (i, (at, command)) in later.iter().enumerate() {
            let run_ends = later
                .get(i + 1)
                .is_none_or(|(next, _)| next - at >= interval - EPSILON);
            let keep = at - last_kept_at >= interval - EPSILON
                || run_ends
                || is_big_jump(address, previous, &command.v);
            previous = Some(command.v.as_slice());
            if keep {
                let slot = (at / interval).round() as i64;
                rows.entry(slot).or_default().insert(address, command);
                last_kept_at = *at;
            }
        }
    }
    rows.into_iter()
        .map(|(slot, row)| {
            let mut messages: Vec<&FixtureCommand> = row.into_values().collect();
            messages.sort_by_key(|m| order.get(m.a.as_str()).copied().unwrap_or(usize::MAX));
            Look {
                at_seconds: slot as f64 * interval,
                messages: messages.into_iter().cloned().collect(),
            }
        })
        .collect()
}

/// Merge rows so the timeline has few, smooth items:
/// one message per address per row (the last wins), a message equal to the address's previous
/// kept value is dropped, rows left empty are dropped, and a row that only repeats addresses of
/// the row before it with values within tolerance (levels 2 %, angles 1 degree) of the last kept
/// value folds into that earlier row. Comparing with the last KEPT value keeps the replayed value
/// within the tolerance of the recorded one at every time.
pub fn merge(looks: Vec<Look>) -> Vec<Look> {
    let mut kept: HashMap<String, Vec<FixtureArg>> = HashMap::new();
    let mut out: Vec<Look> = Vec::new();
    let mut previous_addresses: HashSet<String> = HashSet::new();
    for look in looks {
        let mut messages: Vec<FixtureCommand> = Vec::new();
        for message in look.messages {
            match messages.iter_mut().find(|m| m.a == message.a) {
                Some(existing) => existing.v = message.v,
                None => messages.push(message),
            }
        }
        messages.retain(|m| kept.get(&m.a) != Some(&m.v));
        if messages.is_empty() {
            continue;
        }
        let folds = !out.is_empty()
            && messages.iter().all(|m| {
                previous_addresses.contains(&m.a)
                    && kept
                        .get(&m.a)
                        .is_some_and(|value| within_tolerance(&m.a, value, &m.v))
            });
        if folds {
            continue;
        }
        for message in &messages {
            kept.insert(message.a.clone(), message.v.clone());
        }
        previous_addresses = messages.iter().map(|m| m.a.clone()).collect();
        out.push(Look {
            at_seconds: look.at_seconds,
            messages,
        });
    }
    out
}

/// Thin then merge: the rows a recording becomes.
pub fn simplify(
    baseline: &[FixtureCommand],
    changes: &[(f64, FixtureCommand)],
    duration: f64,
    interval: f64,
) -> Vec<Look> {
    merge(thin(baseline, changes, duration, interval))
}

/// Value of every address after all rows at or before `at` (test and report helper).
pub fn state_at(looks: &[Look], at: f64) -> HashMap<String, Vec<FixtureArg>> {
    let mut state = HashMap::new();
    for look in looks.iter().filter(|l| l.at_seconds <= at + EPSILON) {
        for m in &look.messages {
            state.insert(m.a.clone(), m.v.clone());
        }
    }
    state
}

/// Most changes one recording holds at once (about 170 bytes each, so roughly 170 MB). When a
/// song reaches it the recorder thins what it holds (per address, changes closer than a window
/// collapse to the last value) and goes on with that window, doubling it again if it fills
/// again; it never stops recording and never grows without bound. A 20-minute song with 200
/// addresses changing every 20 ms (12 million changes) ends as about a million.
pub const MAX_RECORDED_CHANGES: usize = 1_000_000;
/// The first thinning window while recording, seconds.
const FIRST_THIN_SECONDS: f64 = 0.02;
/// The recorder thins until it holds at most this share of its limit.
const THIN_TARGET: f64 = 0.6;

/// One recorded stretch of the song: it starts with a snapshot of every fixture property and
/// holds the changes seen until it ends (a link drop, a pause, a seek, the end of the pass).
#[derive(Debug, Clone)]
struct Piece {
    start: f64,
    /// `None` while the stretch is still open.
    end: Option<f64>,
    snapshot: Vec<FixtureCommand>,
}

#[derive(Debug, Clone)]
struct Change {
    at: f64,
    command: FixtureCommand,
    /// Index into `pieces`.
    piece: usize,
}

/// The state of every fixture when a pass began recording, and the song position it is from.
pub type StartState = (f64, Vec<FixtureCommand>);

/// What a finished pass recorded (see [`LookRecorder::raw_with_start`]).
#[derive(Debug, Clone)]
pub struct RawPass {
    pub baseline: Vec<FixtureCommand>,
    pub changes: Vec<(f64, FixtureCommand)>,
    pub start: Option<StartState>,
}

/// A piece with its changes, while the latest recording of each stretch is worked out.
struct Work {
    start: f64,
    end: Option<f64>,
    snapshot: Vec<FixtureCommand>,
    changes: Vec<(f64, FixtureCommand)>,
}

impl Work {
    /// What is left of this piece outside the later recording `lo..hi`: the part before it and the
    /// part after it. The part after starts with a snapshot of the state at `hi` (what this piece
    /// knew then), so nothing stale is carried across the re-recorded stretch.
    fn clip(self, lo: f64, hi: f64) -> Vec<Work> {
        let end = self.end.unwrap_or(f64::INFINITY);
        if !(self.start < hi && end > lo) {
            return vec![self];
        }
        let mut out = Vec::new();
        if self.start < lo {
            out.push(Work {
                start: self.start,
                end: Some(lo),
                snapshot: self.snapshot.clone(),
                changes: self.changes.iter().filter(|c| c.0 <= lo).cloned().collect(),
            });
        }
        if end > hi {
            let mut state = FixtureState::new();
            state.apply_all(&self.snapshot);
            for (at, command) in &self.changes {
                if *at <= hi {
                    state.apply(command);
                }
            }
            out.push(Work {
                start: hi,
                end: self.end,
                snapshot: state.commands(),
                changes: self.changes.into_iter().filter(|c| c.0 > hi).collect(),
            });
        }
        out
    }
}

/// Collects the fixture changes of one song as they arrive, then turns them into rows.
///
/// The song is recorded in stretches (see [`begin_stretch`](Self::begin_stretch)); where two
/// stretches cover the same song time the later recording wins, and a stretch starts with a
/// snapshot of every property, so a gap never leaves a stale value behind it.
#[derive(Debug, Clone)]
pub struct LookRecorder {
    baseline: FixtureState,
    /// State inside the open stretch (to drop changes that change nothing).
    current: FixtureState,
    pieces: Vec<Piece>,
    open: Option<usize>,
    /// In the order recorded.
    observed: Vec<Change>,
    /// Most changes held (see [`MAX_RECORDED_CHANGES`]).
    max_changes: usize,
    /// Changes of one address closer than this collapse while recording (0: none do yet).
    thin: f64,
    /// Per address, the newest kept change: (window, index into `observed`, piece). Only used
    /// while `thin` is set.
    slots: HashMap<String, (i64, usize, usize)>,
    /// Where the changes dated with the current clock estimate begin (see [`Self::set_mark`]).
    mark: usize,
    thinned_times: u32,
}

fn window_of(at: f64, seconds: f64) -> i64 {
    if at <= 0.0 {
        -1
    } else {
        (at / seconds).floor() as i64
    }
}

impl LookRecorder {
    /// `baseline` is the fixture state when the song started; the first stretch opens at 0.
    pub fn new(baseline: &FixtureState) -> Self {
        Self::with_limit(baseline, MAX_RECORDED_CHANGES)
    }

    /// A recorder that holds at most `max_changes` (thinning when it fills; tests use small ones).
    pub fn with_limit(baseline: &FixtureState, max_changes: usize) -> Self {
        Self {
            max_changes: max_changes.max(16),
            thin: 0.0,
            slots: HashMap::new(),
            mark: 0,
            thinned_times: 0,
            baseline: baseline.clone(),
            current: baseline.clone(),
            pieces: vec![Piece {
                start: 0.0,
                end: None,
                snapshot: baseline.commands(),
            }],
            open: Some(0),
            observed: Vec::new(),
        }
    }

    /// How many changes are held.
    pub fn len(&self) -> usize {
        self.observed.len()
    }

    /// The window (seconds) changes of one address collapse in while recording; 0 until the
    /// recorder first had to thin.
    pub fn thin_seconds(&self) -> f64 {
        self.thin
    }

    /// How many times the recorder thinned what it held.
    pub fn thinned_times(&self) -> u32 {
        self.thinned_times
    }

    /// Remember where the changes dated with the current clock estimate begin.
    pub fn set_mark(&mut self) {
        self.mark = self.observed.len();
    }

    /// [`Self::shift_from`] the mark. Returns how many moved.
    pub fn shift_since_mark(&mut self, shift: f64) -> usize {
        self.shift_from(self.mark, shift)
    }

    /// Collapse what is held to the last value per address per window, widening the window until
    /// at most [`THIN_TARGET`] of the limit is held.
    fn thin_held(&mut self) {
        let target = (self.max_changes as f64 * THIN_TARGET) as usize;
        let mut thin = if self.thin > 0.0 {
            self.thin * 2.0
        } else {
            FIRST_THIN_SECONDS
        };
        loop {
            let mut slots: HashMap<String, (i64, usize, usize)> = HashMap::new();
            let mut kept: Vec<Change> = Vec::with_capacity(self.observed.len() / 2);
            let mut mark = None;
            for (i, change) in std::mem::take(&mut self.observed).into_iter().enumerate() {
                if i == self.mark {
                    mark = Some(kept.len());
                }
                let window = window_of(change.at, thin);
                match slots.get_mut(&change.command.a) {
                    Some((w, position, piece)) if *w == window && *piece == change.piece => {
                        kept[*position] = change;
                    }
                    Some(entry) => {
                        *entry = (window, kept.len(), change.piece);
                        kept.push(change);
                    }
                    None => {
                        slots.insert(change.command.a.clone(), (window, kept.len(), change.piece));
                        kept.push(change);
                    }
                }
            }
            self.mark = mark.unwrap_or(kept.len());
            self.observed = kept;
            self.slots = slots;
            self.thin = thin;
            if self.observed.len() <= target || thin >= 3600.0 {
                break;
            }
            thin *= 2.0;
        }
        self.thinned_times += 1;
        tracing::warn!(
            held = self.observed.len(),
            thin_seconds = self.thin,
            "lightkey_recording_thinned"
        );
    }

    pub fn is_empty(&self) -> bool {
        self.observed.is_empty()
    }

    /// Whether a stretch is open (changes are being kept).
    pub fn is_recording(&self) -> bool {
        self.open.is_some()
    }

    /// A recorded stretch starts at song `position` with `state`, the value of every property at
    /// that moment. Any stretch still open ends there.
    pub fn begin_stretch(&mut self, position: f64, state: &FixtureState) {
        self.end_stretch(position);
        self.pieces.push(Piece {
            start: position,
            end: None,
            snapshot: state.commands(),
        });
        self.open = Some(self.pieces.len() - 1);
        self.current = state.clone();
    }

    /// The open stretch ends at `position` (the link dropped, the song paused or was moved, the
    /// pass ended). Changes after this are ignored until the next stretch begins.
    pub fn end_stretch(&mut self, position: f64) {
        let Some(index) = self.open.take() else {
            return;
        };
        let start = self.pieces[index].start;
        let end = position.max(start);
        // A stretch with no length and no changes would only cut older data for nothing.
        if end - start < 1e-9
            && index + 1 == self.pieces.len()
            && !self.observed.iter().any(|c| c.piece == index)
        {
            self.pieces.pop();
        } else {
            self.pieces[index].end = Some(end);
        }
    }

    /// Record the commands of one change `position` seconds into the song. Commands that do not
    /// change anything, or cannot be replayed as a fixture override, are dropped, and so is
    /// everything while no stretch is open. Returns how many were kept.
    pub fn observe(&mut self, position: f64, commands: &[FixtureCommand]) -> usize {
        let Some(piece) = self.open else {
            return 0;
        };
        let mut kept = 0;
        for command in commands.iter().filter(|c| is_replayable(c)) {
            if !self.current.apply(command) {
                continue;
            }
            kept += 1;
            let window = window_of(position, self.thin.max(f64::MIN_POSITIVE));
            if self.thin > 0.0 {
                // once thinning, a change in the window of the address's newest kept one
                // replaces it (the last value of the window wins)
                if let Some(&(w, i, p)) = self.slots.get(&command.a) {
                    if w == window && p == piece && i < self.observed.len() {
                        self.observed[i].at = position;
                        self.observed[i].command = command.clone();
                        continue;
                    }
                }
            }
            if self.observed.len() >= self.max_changes {
                self.thin_held();
                // the thinned recorder may now take this change into an existing slot
                let window = window_of(position, self.thin);
                if let Some(&(w, i, p)) = self.slots.get(&command.a) {
                    if w == window && p == piece && i < self.observed.len() {
                        self.observed[i].at = position;
                        self.observed[i].command = command.clone();
                        continue;
                    }
                }
            }
            self.observed.push(Change {
                at: position,
                command: command.clone(),
                piece,
            });
            if self.thin > 0.0 {
                self.slots.insert(
                    command.a.clone(),
                    (
                        window_of(position, self.thin),
                        self.observed.len() - 1,
                        piece,
                    ),
                );
            }
        }
        kept
    }

    /// Move the changes from index `mark` on by `shift` seconds (they were dated with a clock
    /// offset that turned out wrong). Returns how many moved.
    pub fn shift_from(&mut self, mark: usize, shift: f64) -> usize {
        if mark >= self.observed.len() || !shift.is_finite() {
            return 0;
        }
        for change in &mut self.observed[mark..] {
            change.at += shift;
        }
        self.observed.len() - mark
    }

    /// The changes to fit, in time order: per stretch the snapshot (where it differs from what
    /// went before) and its changes, with the earlier recording of any re-recorded stretch cut
    /// out.
    fn flattened(&self) -> Vec<(f64, FixtureCommand)> {
        self.flattened_with_start().0
    }

    fn flattened_with_start(&self) -> (Vec<(f64, FixtureCommand)>, Option<StartState>) {
        let works = self.works();
        let start = works
            .iter()
            .find(|w| !w.snapshot.is_empty())
            .map(|w| (w.start, w.snapshot.clone()));
        let mut state = self.baseline.clone();
        let mut out = Vec::new();
        for work in works {
            for command in &work.snapshot {
                if state.apply(command) {
                    out.push((work.start, command.clone()));
                }
            }
            for (at, command) in work.changes {
                if state.apply(&command) {
                    out.push((at, command));
                }
            }
        }
        (out, start)
    }

    /// The stretches of the pass in song order, the latest recording of each part winning.
    fn works(&self) -> Vec<Work> {
        let mut by_piece: Vec<Vec<(f64, FixtureCommand)>> = vec![Vec::new(); self.pieces.len()];
        for change in &self.observed {
            let piece = &self.pieces[change.piece];
            let mut at = change.at.max(piece.start);
            if let Some(end) = piece.end {
                at = at.min(end);
            }
            by_piece[change.piece].push((at, change.command.clone()));
        }
        let mut works: Vec<Work> = Vec::new();
        for (piece, mut changes) in self.pieces.iter().zip(by_piece) {
            changes.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (lo, hi) = (piece.start, piece.end.unwrap_or(f64::INFINITY));
            let mut next = Vec::new();
            for old in works {
                next.extend(old.clip(lo, hi));
            }
            next.push(Work {
                start: piece.start,
                end: piece.end,
                snapshot: piece.snapshot.clone(),
                changes,
            });
            works = next;
        }
        works.sort_by(|a, b| a.start.total_cmp(&b.start));
        works
    }

    /// The ramps, steps and effects these changes make (see `crate::lightkey_smooth`). Changes
    /// after `duration` fold onto it as in [`thin`]; nothing is dropped.
    pub fn smooth(&self, duration: f64, params: &SmoothParams) -> SmoothCapture {
        let baseline = self.baseline.commands();
        lightkey_smooth::fit(&baseline, &self.flattened(), duration, params)
    }

    /// The raw baseline and changes (the report compares a fit against them).
    pub fn raw(&self) -> (Vec<FixtureCommand>, Vec<(f64, FixtureCommand)>) {
        (self.baseline.commands(), self.flattened())
    }

    /// [`Self::raw`] and the starting look: the state of every fixture when the pass began
    /// recording, with the song position it was taken at (the first recorded stretch's snapshot:
    /// 0 when the stream was live at the song's start, later when recording began late). `None`
    /// when the relay's state was never read. See `crate::lightkey_start`.
    pub fn raw_with_start(&self) -> RawPass {
        let (changes, start) = self.flattened_with_start();
        RawPass {
            baseline: self.baseline.commands(),
            changes,
            start,
        }
    }

    /// The rows these changes make at this Detail `interval` (see [`thin`] and [`merge`]).
    /// The last change of an address after `duration` is folded into the final row; it is never
    /// fatal.
    pub fn looks(&self, duration: f64, interval: f64) -> Vec<Look> {
        let baseline = self.baseline.commands();
        let observed = self.flattened();
        let looks = simplify(&baseline, &observed, duration, interval);
        let late = observed
            .iter()
            .filter(|(at, _)| duration > 0.0 && *at > duration)
            .count();
        if late > 0 {
            tracing::warn!(
                late,
                duration,
                "lightkey_fixture_changes_after_the_end_folded"
            );
        }
        looks
    }
}

/// Distinct fixtures a row drives.
fn fixture_count(messages: &[FixtureCommand]) -> usize {
    messages
        .iter()
        .filter_map(FixtureCommand::fixture)
        .collect::<BTreeSet<_>>()
        .len()
}

/// The `address` a look row carries. A look sends its `messages`; an older StagePilot that does
/// not know them would send this address instead, with no arguments, and no rig listens to it
/// (M-02: a downgrade never sends a fixture address without its value).
pub const LOOK_ROW_ADDRESS: &str = "/stagepilot/look";

/// The timeline rows for looks: `source = captured`, no colour, label "Look - N fixtures", times
/// to 0.1 s. A row over the per-row message cap is split into several rows at the same time.
pub fn look_rows(looks: &[Look]) -> Vec<OscLightingCue> {
    let mut rows = Vec::new();
    for look in looks {
        for chunk in look.messages.chunks(MAX_OSC_MESSAGES_PER_ROW) {
            if chunk.is_empty() {
                continue;
            };
            let n = fixture_count(chunk);
            rows.push(OscLightingCue {
                id: Uuid::new_v4(),
                at_seconds: round_tenths(look.at_seconds),
                address: LOOK_ROW_ADDRESS.to_string(),
                args: Vec::new(),
                label: format!("Look - {n} fixture{}", if n == 1 { "" } else { "s" }),
                color: None,
                messages: chunk
                    .iter()
                    .map(|m| OscMessage {
                        address: m.a.clone(),
                        args: m.osc_args(),
                    })
                    .collect(),
                source: OscCueSource::Captured,
                edited: false,
                fade_seconds: 0.0,
            });
        }
    }
    rows
}

/// The song's cue map with its captured rows replaced by `rows`. Manual rows and MIDI cues stay.
/// `None` when the result would pass the per-song caps (the caller retries with a looser fit).
pub fn replace_captured(
    existing: Option<&SongLightingCueMap>,
    song_key: &str,
    song_title: &str,
    rows: Vec<OscLightingCue>,
) -> Option<SongLightingCueMap> {
    replace_captured_with_effects(existing, song_key, song_title, rows, Vec::new())
}

/// [`replace_captured`] that also replaces the song's captured effects by `effects`. Manual
/// (hand-made) effects stay.
pub fn replace_captured_with_effects(
    existing: Option<&SongLightingCueMap>,
    song_key: &str,
    song_title: &str,
    rows: Vec<OscLightingCue>,
    effects: Vec<LightingEffect>,
) -> Option<SongLightingCueMap> {
    let mut map = existing.cloned().unwrap_or_else(|| SongLightingCueMap {
        song_key: song_key.to_string(),
        song_title: song_title.to_string(),
        cues: Vec::new(),
        osc_cues: Vec::new(),
        loudness: None,
        captured_edited: false,
        capture: None,
        effects: Vec::new(),
    });
    map.osc_cues
        .retain(|row| row.source != OscCueSource::Captured);
    map.effects
        .retain(|effect| effect.source != OscCueSource::Captured);
    map.captured_edited = false;
    map.osc_cues.extend(rows);
    map.effects.extend(effects);
    within_caps(&map).then_some(map)
}

/// What saving a capture pass did to a song's cue map.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum CaptureSave {
    /// The song's whole cue map after the pass.
    Saved(SongLightingCueMap),
    /// The stored capture is at least as good: nothing changes. A complete capture is never
    /// replaced by a partial; a partial is replaced only by one that covers at least as much.
    Kept,
    TooLarge,
}

/// Save a capture pass (`status`) into the song's cue map. A complete pass always replaces the
/// captured rows (manual rows stay). A partial pass replaces captured rows only when they are
/// not a complete capture and cover no more of the song. Captured rows with no status (an older
/// file) count as an incomplete capture that covers nothing.
pub fn save_capture(
    existing: Option<&SongLightingCueMap>,
    song_key: &str,
    song_title: &str,
    rows: Vec<OscLightingCue>,
    status: CaptureStatus,
) -> CaptureSave {
    save_capture_with_effects(existing, song_key, song_title, rows, Vec::new(), status)
}

/// [`save_capture`] for a pass that also found effects.
pub fn save_capture_with_effects(
    existing: Option<&SongLightingCueMap>,
    song_key: &str,
    song_title: &str,
    rows: Vec<OscLightingCue>,
    effects: Vec<LightingEffect>,
    status: CaptureStatus,
) -> CaptureSave {
    if let Some(old) = existing.filter(|map| map.has_captured_rows()) {
        let stored = old.capture.as_ref();
        let keep = if status.complete {
            false
        } else if stored.is_some_and(|c| c.complete) {
            true
        } else {
            stored.map_or(0.0, |c| c.coverage_seconds) > status.coverage_seconds
        };
        if keep {
            return CaptureSave::Kept;
        }
    }
    match replace_captured_with_effects(existing, song_key, song_title, rows, effects) {
        Some(mut map) => {
            map.capture = Some(status);
            CaptureSave::Saved(map)
        }
        None => CaptureSave::TooLarge,
    }
}

/// What simplifying a song's captured rows does.
#[derive(Debug, Clone, PartialEq)]
pub struct Simplified {
    /// The song's whole cue map after the change.
    pub map: SongLightingCueMap,
    pub rows_before: usize,
    pub rows_after: usize,
    pub messages_before: usize,
    pub messages_after: usize,
}

fn fixture_arg(arg: &stagepilot_core::config::OscArgument) -> FixtureArg {
    use stagepilot_core::config::OscArgument;
    match arg {
        OscArgument::Int(v) => FixtureArg::Int(*v),
        OscArgument::Float(v) => FixtureArg::Float(*v),
        OscArgument::String(v) => FixtureArg::Text(v.clone()),
        OscArgument::Bool(v) => FixtureArg::Bool(*v),
    }
}

/// A captured row that can be thinned again: it has messages and every one is a replayable
/// fixture override.
fn simplifiable(row: &OscLightingCue) -> bool {
    row.source == OscCueSource::Captured
        && row.fade_seconds == 0.0
        && !row.messages.is_empty()
        && row.messages.iter().all(|m| {
            is_replayable(&FixtureCommand::new(
                m.address.clone(),
                m.args.iter().map(fixture_arg).collect(),
            ))
        })
}

/// "Simplify this song": run the recording's own thin + merge step over the song's captured
/// rows at `interval` seconds (the Detail grid). Manual rows, MIDI cues and captured rows that
/// are not fixture overrides stay exactly as they are. The result is not checked against the
/// per-song caps here; `None` means the song has no captured rows to simplify.
pub fn simplify_song(map: &SongLightingCueMap, interval: f64) -> Option<Simplified> {
    if is_smooth(map) {
        // Ramps start from the value the row before them left; thinning instant rows around a
        // ramp, or dropping the rows an effect hands over to, would move it. A smooth song is
        // already fitted: report it unchanged, effects and fades included.
        let rows = map
            .osc_cues
            .iter()
            .filter(|r| r.source == OscCueSource::Captured);
        let (n, m) = rows.fold((0, 0), |(n, m), r| (n + 1, m + r.messages.len()));
        if n == 0 && map.effects.is_empty() {
            return None;
        }
        return Some(Simplified {
            map: map.clone(),
            rows_before: n,
            rows_after: n,
            messages_before: m,
            messages_after: m,
        });
    }
    let mut rows: Vec<&OscLightingCue> = map.osc_cues.iter().filter(|r| simplifiable(r)).collect();
    if rows.is_empty() {
        return None;
    }
    rows.sort_by(|a, b| a.at_seconds.total_cmp(&b.at_seconds));
    let changes: Vec<(f64, FixtureCommand)> = rows
        .iter()
        .flat_map(|row| {
            row.messages.iter().map(|m| {
                (
                    row.at_seconds,
                    FixtureCommand::new(
                        m.address.clone(),
                        m.args.iter().map(fixture_arg).collect(),
                    ),
                )
            })
        })
        .collect();
    let duration = rows.last().map_or(0.0, |r| r.at_seconds) + interval;
    let fresh = look_rows(&simplify(&[], &changes, duration, interval));
    let rows_before = rows.len();
    let messages_before: usize = rows.iter().map(|r| r.messages.len()).sum();
    let rows_after = fresh.len();
    let messages_after: usize = fresh.iter().map(|r| r.messages.len()).sum();
    let mut out = map.clone();
    out.osc_cues.retain(|row| !simplifiable(row));
    out.osc_cues.extend(fresh);
    out.osc_cues
        .sort_by(|a, b| a.at_seconds.total_cmp(&b.at_seconds));
    out.captured_edited = map.captured_edited;
    Some(Simplified {
        map: out,
        rows_before,
        rows_after,
        messages_before,
        messages_after,
    })
}

/// Whether the song carries ramps (a row with a fade) or effects: [`simplify_song`] leaves such
/// a song alone.
pub fn is_smooth(map: &SongLightingCueMap) -> bool {
    !map.effects.is_empty() || map.osc_cues.iter().any(|r| r.fade_seconds > 0.0)
}

/// Whether a cue map is within the per-song row, message, effect and cycle-step caps.
pub fn within_caps(map: &SongLightingCueMap) -> bool {
    let messages: usize = map.osc_cues.iter().map(|r| r.messages.len().max(1)).sum();
    let steps: usize = map.effects.iter().map(|e| e.cycle.len()).sum();
    map.osc_cues.len() <= MAX_OSC_ROWS_PER_SONG
        && messages <= MAX_OSC_MESSAGES_PER_SONG
        && map.effects.len() <= MAX_EFFECTS_PER_SONG
        && steps <= MAX_EFFECT_CYCLE_STEPS_PER_SONG
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn f(x: f64) -> Vec<FixtureArg> {
        vec![FixtureArg::Float(x)]
    }
    fn dim(x: f64) -> FixtureCommand {
        FixtureCommand::new("/fixture/MH1/overrides/dimmer", f(x))
    }
    fn col(r: f64, g: f64, b: f64) -> FixtureCommand {
        FixtureCommand::new(
            "/fixture/MH1/overrides/color",
            vec![
                FixtureArg::Float(r),
                FixtureArg::Float(g),
                FixtureArg::Float(b),
            ],
        )
    }
    fn pan(x: f64) -> FixtureCommand {
        FixtureCommand::new("/fixture/MH1/overrides/panAngle", f(x))
    }
    fn gobo(x: i64) -> FixtureCommand {
        FixtureCommand::new("/fixture/MH1/overrides/gobo", vec![FixtureArg::Int(x)])
    }
    fn times(looks: &[Look]) -> Vec<f64> {
        looks.iter().map(|l| l.at_seconds).collect()
    }
    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }
    /// A slow dimmer ramp: a change every 0.1 s from t=0.1, 0.01 apart (no jumps).
    fn ramp(from: f64, count: usize) -> Vec<(f64, FixtureCommand)> {
        (0..count)
            .map(|i| (0.1 * (i as f64 + 1.0), dim(from + 0.01 * (i as f64 + 1.0))))
            .collect()
    }

    #[test]
    fn keeps_a_change_only_when_the_interval_has_passed() {
        let looks = thin(&[], &ramp(0.2, 30), 60.0, 0.5);
        let at: Vec<f64> = times(&looks);
        // 30 changes over 3 s thin to about one per 0.5 s (plus the run's last value).
        assert!(looks.len() <= 8, "{at:?}");
        assert!(looks.len() >= 5, "{at:?}");
        for pair in at.windows(2) {
            assert!(pair[1] - pair[0] >= 0.5 - 1e-9, "{at:?}");
        }
    }

    #[test]
    fn always_keeps_the_last_value_of_a_run() {
        let looks = thin(&[], &ramp(0.2, 13), 60.0, 0.5);
        let last = looks.last().unwrap();
        assert_eq!(last.messages[0].v, f(0.2 + 0.13));
        assert_eq!(
            state_at(&looks, 59.0)["/fixture/MH1/overrides/dimmer"],
            f(0.33)
        );
    }

    /// Values kept for one address out of a slow ramp that has one jump in the middle.
    fn kept_values(
        make: impl Fn(f64) -> FixtureCommand,
        jump_from: f64,
        jump_to: f64,
    ) -> Vec<Vec<FixtureArg>> {
        let mut changes = vec![
            (1.0, make(jump_from)),
            (1.1, make(jump_from + 0.01)),
            (1.2, make(jump_to)),
        ];
        for i in 0..10 {
            changes.push((
                1.3 + 0.1 * f64::from(i),
                make(jump_to + 0.005 * f64::from(i + 1)),
            ));
        }
        thin(&[], &changes, 60.0, 0.5)
            .into_iter()
            .flat_map(|l| l.messages)
            .map(|m| m.v)
            .collect()
    }

    #[test]
    fn keeps_big_jumps_inside_the_interval() {
        // dimmer to 0 in the middle of a ramp
        assert!(kept_values(dim, 0.5, 0.0).contains(&f(0.0)));
        // dimmer to full
        assert!(kept_values(dim, 0.5, 1.0).contains(&f(1.0)));
        // an angle jump past 25 degrees
        assert!(kept_values(pan, 10.0, 60.0).contains(&f(60.0)));
        // a level jump past 0.25 (a colour channel)
        assert!(kept_values(|x| col(x, 0.0, 0.0), 0.1, 0.6).contains(&col(0.6, 0.0, 0.0).v));
        // a small step is not a jump: a 0.2 move is thinned away
        assert!(!kept_values(dim, 0.4, 0.6).contains(&f(0.6)));
        // an option change is always kept
        let options = thin(
            &[],
            &[
                (1.0, gobo(1)),
                (1.1, gobo(2)),
                (1.2, gobo(3)),
                (1.3, gobo(4)),
            ],
            60.0,
            1.0,
        );
        assert_eq!(
            state_at(&options, 60.0)["/fixture/MH1/overrides/gobo"],
            vec![FixtureArg::Int(4)]
        );
    }

    #[test]
    fn rows_sit_on_the_detail_grid_and_group_a_slot() {
        let changes = vec![
            (1.02, dim(0.9)),
            (1.07, pan(40.0)),
            (1.1, col(0.9, 0.2, 0.2)),
            (2.6, dim(0.2)),
        ];
        let looks = thin(&[], &changes, 60.0, 0.5);
        for t in times(&looks) {
            assert!(near((t / 0.5).round() * 0.5, t), "{t}");
        }
        // 1.02, 1.07 and 1.1 all land in the slot at 1.0: one row, three messages
        let row = looks.iter().find(|l| near(l.at_seconds, 1.0)).unwrap();
        assert_eq!(row.messages.len(), 3);
        // idle slots make no row
        assert_eq!(looks.len(), 2, "{looks:?}");
    }

    #[test]
    fn starting_state_becomes_a_row_at_zero() {
        let baseline = vec![dim(0.1), col(0.0, 0.0, 0.0), pan(5.0)];
        let changes = vec![
            (-0.5, col(0.3, 0.3, 0.3)),
            (0.0, col(0.4, 0.4, 0.4)),
            (3.0, dim(0.9)),
        ];
        let looks = thin(&baseline, &changes, 60.0, 0.5);
        assert!(near(looks[0].at_seconds, 0.0));
        let start = &looks[0].messages;
        // colour changed at t<=0 (last one wins); dimmer moves later so its baseline is the start
        // value; panAngle never moves so it is left alone
        assert!(start
            .iter()
            .any(|m| m.a.ends_with("color") && m.v == col(0.4, 0.4, 0.4).v));
        assert!(start
            .iter()
            .any(|m| m.a.ends_with("dimmer") && m.v == f(0.1)));
        assert!(!start.iter().any(|m| m.a.ends_with("panAngle")));
    }

    #[test]
    fn the_last_change_after_the_end_is_folded_into_the_final_row_at_the_duration() {
        // R-02: an overrunning song keeps its final look (a fade-out), not dropped.
        let changes = vec![(10.0, dim(0.5)), (100.0, dim(0.9)), (101.0, dim(0.1))];
        let looks = thin(&[], &changes, 60.0, 0.5);
        assert_eq!(looks.len(), 2);
        assert_eq!(looks[0].messages[0].v, f(0.5));
        assert_eq!(looks[1].at_seconds, 60.0);
        assert_eq!(looks[1].messages[0].v, f(0.1), "the last value wins");
    }

    #[test]
    fn merge_keeps_the_last_value_per_address_in_a_row() {
        let looks = vec![Look {
            at_seconds: 1.0,
            messages: vec![dim(0.2), dim(0.8)],
        }];
        let merged = merge(looks);
        assert_eq!(merged[0].messages.len(), 1);
        assert_eq!(merged[0].messages[0].v, f(0.8));
    }

    #[test]
    fn merge_drops_a_value_equal_to_the_previous_one_and_empty_rows() {
        let looks = vec![
            Look {
                at_seconds: 1.0,
                messages: vec![dim(0.5), pan(10.0)],
            },
            Look {
                at_seconds: 2.0,
                messages: vec![dim(0.5)],
            },
            Look {
                at_seconds: 3.0,
                messages: vec![dim(0.5), pan(50.0)],
            },
        ];
        let merged = merge(looks);
        assert_eq!(merged.len(), 2, "{merged:?}");
        assert_eq!(merged[1].messages.len(), 1);
        assert_eq!(merged[1].messages[0].a, pan(0.0).a);
    }

    #[test]
    fn merge_folds_a_row_that_only_repeats_the_previous_addresses_within_tolerance() {
        let looks = vec![
            Look {
                at_seconds: 1.0,
                messages: vec![dim(0.50), pan(10.0)],
            },
            Look {
                at_seconds: 1.5,
                messages: vec![dim(0.51), pan(10.5)],
            },
            Look {
                at_seconds: 2.0,
                messages: vec![dim(0.80), pan(10.5)],
            },
        ];
        let merged = merge(looks);
        assert_eq!(times(&merged), vec![1.0, 2.0], "{merged:?}");
    }

    #[test]
    fn merge_keeps_a_row_with_a_new_address_or_a_value_past_the_tolerance() {
        let new_address = vec![
            Look {
                at_seconds: 1.0,
                messages: vec![dim(0.50)],
            },
            Look {
                at_seconds: 1.5,
                messages: vec![dim(0.51), pan(10.0)],
            },
        ];
        assert_eq!(merge(new_address).len(), 2);
        let too_far = vec![
            Look {
                at_seconds: 1.0,
                messages: vec![pan(10.0)],
            },
            Look {
                at_seconds: 1.5,
                messages: vec![pan(11.5)],
            },
        ];
        assert_eq!(merge(too_far).len(), 2);
        let option_change = vec![
            Look {
                at_seconds: 1.0,
                messages: vec![gobo(1)],
            },
            Look {
                at_seconds: 1.5,
                messages: vec![gobo(2)],
            },
        ];
        assert_eq!(merge(option_change).len(), 2);
    }

    #[test]
    fn merge_does_not_drift_beyond_the_tolerance_over_a_slow_ramp() {
        // 0.5% per row: each step is within 2% of the last KEPT value, so rows fold until the
        // gap to the kept value passes 2%, never letting the replayed value drift further.
        let looks: Vec<Look> = (0..40)
            .map(|i| Look {
                at_seconds: 1.0 + 0.5 * f64::from(i),
                messages: vec![dim(0.005 * f64::from(i))],
            })
            .collect();
        let merged = merge(looks.clone());
        assert!(merged.len() < looks.len() / 2, "{}", merged.len());
        assert_within_tolerance(&looks, &merged);
    }

    pub(super) fn assert_within_tolerance(before: &[Look], after: &[Look]) {
        for look in before {
            let want = state_at(before, look.at_seconds);
            let got = state_at(after, look.at_seconds);
            for (address, value) in &want {
                let have = got
                    .get(address)
                    .unwrap_or_else(|| panic!("{address} lost at {}", look.at_seconds));
                let tol = tolerance_for(address);
                for (a, b) in value.iter().zip(have) {
                    match (a, b) {
                        (FixtureArg::Float(a), FixtureArg::Float(b)) => {
                            assert!(
                                (a - b).abs() <= tol + 1e-9,
                                "{address} at {}: {a} vs {b}",
                                look.at_seconds
                            )
                        }
                        _ => assert_eq!(a, b, "{address} at {}", look.at_seconds),
                    }
                }
            }
        }
    }

    #[test]
    fn merging_never_changes_a_final_value_beyond_the_tolerance() {
        // Deterministic pseudo-random walks over a few addresses.
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut level = 0.5;
        let mut angle = 90.0;
        let mut looks = Vec::new();
        for i in 0..400 {
            let mut messages = Vec::new();
            if next() < 0.6 {
                level = (level + (next() - 0.5) * 0.05).clamp(0.0, 1.0);
                messages.push(dim(level));
            }
            if next() < 0.6 {
                angle += (next() - 0.5) * 2.0;
                messages.push(pan(angle));
            }
            if next() < 0.1 {
                messages.push(gobo((next() * 3.0) as i64));
            }
            if !messages.is_empty() {
                looks.push(Look {
                    at_seconds: 0.5 * f64::from(i),
                    messages,
                });
            }
        }
        let merged = merge(looks.clone());
        assert!(merged.len() < looks.len());
        assert_within_tolerance(&looks, &merged);
    }

    fn at(name: &str, x: f64) -> FixtureCommand {
        FixtureCommand::new(format!("/fixture/{name}/overrides/dimmer"), f(x))
    }

    #[test]
    fn recorder_drops_unchanged_and_unreplayable_commands() {
        let mut base = FixtureState::new();
        base.apply_all(&[at("P1", 0.0), at("P2", 0.5)]);
        let mut r = LookRecorder::new(&base);
        assert_eq!(r.observe(1.5, &[at("P1", 1.0), at("P2", 0.5)]), 1);
        let bad = FixtureCommand::new("/live/x/cue/y/activate", vec![]);
        let nan = FixtureCommand::new(
            "/fixture/P1/overrides/pan",
            vec![FixtureArg::Float(f64::NAN)],
        );
        assert_eq!(r.observe(2.0, &[bad, nan]), 0);
        assert_eq!(r.len(), 1);
        let looks = r.looks(10.0, 0.5);
        // P1 moves, so its start value is the first row; P2 never moved and is left alone
        assert_eq!(looks.len(), 2);
        assert!(looks
            .iter()
            .all(|l| l.messages.iter().all(|m| m.a.contains("/P1/"))));
    }

    /// 2100 addresses changing after the scheduled end used to pile into one event (clamped to
    /// the duration), trip a 2048-command limit and lose the whole capture.
    #[test]
    fn a_recording_that_runs_past_its_scheduled_end_keeps_the_capture() {
        let mut r = LookRecorder::new(&FixtureState::new());
        r.observe(1.0, &[at("P1", 1.0)]);
        r.observe(2.0, &[at("P1", 0.5)]);
        for n in 0..2100 {
            r.observe(11.0 + f64::from(n) * 0.01, &[at(&format!("X{n}"), 1.0)]);
        }
        let looks = r.looks(10.0, 0.5);
        assert!(!looks.is_empty());
        assert!(looks.iter().all(|l| l.at_seconds <= 10.0));
        let names: HashSet<&str> = looks
            .iter()
            .flat_map(|l| l.messages.iter())
            .map(|m| m.a.as_str())
            .collect();
        // P1, plus the final value of each of the 2100 fixtures that moved after the end,
        // folded into the row at the duration.
        assert_eq!(names.len(), 2101);
        assert_eq!(
            state_at(&looks, 10.0)["/fixture/P1/overrides/dimmer"],
            f(0.5)
        );
    }

    #[test]
    fn one_oversize_change_is_split_into_rows_not_rejected() {
        let mut r = LookRecorder::new(&FixtureState::new());
        let many: Vec<FixtureCommand> = (0..5000).map(|n| at(&format!("F{n}"), 1.0)).collect();
        r.observe(3.0, &many);
        let rows = look_rows(&r.looks(10.0, 0.5));
        assert!(rows
            .iter()
            .all(|row| row.messages.len() <= MAX_OSC_MESSAGES_PER_ROW));
        assert_eq!(
            rows.iter().map(|row| row.messages.len()).sum::<usize>(),
            5000
        );
        assert!(rows
            .iter()
            .all(|row| row.source == OscCueSource::Captured && row.color.is_none()));
    }

    #[test]
    fn rows_are_labelled_with_their_fixture_count() {
        let looks = vec![Look {
            at_seconds: 1.04,
            messages: vec![at("A", 1.0), pan(3.0), at("B", 0.1)],
        }];
        let rows = look_rows(&looks);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Look - 3 fixtures");
        assert_eq!(rows[0].at_seconds, 1.0);
        let one = look_rows(&[Look {
            at_seconds: 0.0,
            messages: vec![dim(0.5)],
        }]);
        assert_eq!(one[0].label, "Look - 1 fixture");
    }

    #[test]
    fn replace_captured_keeps_manual_rows_and_midi_cues() {
        let manual = OscLightingCue {
            id: Uuid::new_v4(),
            at_seconds: 4.0,
            address: "/live/1/cue/1/activate".into(),
            args: vec![],
            label: "Strobe".into(),
            color: None,
            messages: vec![],
            source: OscCueSource::Manual,
            edited: false,
            fade_seconds: 0.0,
        };
        let old_look = look_rows(&[Look {
            at_seconds: 1.0,
            messages: vec![dim(0.5)],
        }]);
        let existing = SongLightingCueMap {
            song_key: "s".into(),
            song_title: "Song".into(),
            cues: vec![],
            osc_cues: std::iter::once(manual.clone()).chain(old_look).collect(),
            loudness: None,
            captured_edited: false,
            capture: None,
            effects: Vec::new(),
        };
        let fresh = look_rows(&[Look {
            at_seconds: 2.0,
            messages: vec![dim(0.9)],
        }]);
        let map = replace_captured(Some(&existing), "s", "Song", fresh.clone()).unwrap();
        assert_eq!(map.osc_cues.len(), 2);
        assert!(map.osc_cues.contains(&manual));
        assert!(map.osc_cues.contains(&fresh[0]));
        // a second save replaces the first save's rows again, not the manual one
        let again = replace_captured(Some(&map), "s", "Song", fresh).unwrap();
        assert_eq!(again.osc_cues.len(), 2);
    }

    #[test]
    fn replace_captured_refuses_a_song_past_the_caps() {
        let looks: Vec<Look> = (0..=MAX_OSC_ROWS_PER_SONG)
            .map(|i| Look {
                at_seconds: i as f64,
                messages: vec![dim(0.5)],
            })
            .collect();
        assert!(replace_captured(None, "s", "Song", look_rows(&looks)).is_none());
        assert!(replace_captured(None, "s", "Song", look_rows(&looks[..10])).is_some());
    }

    #[test]
    fn shift_from_moves_only_the_later_changes_and_keeps_time_order() {
        let mut base = FixtureState::new();
        base.apply_all(&[at("P1", 0.0), at("P2", 0.0)]);
        let mut r = LookRecorder::new(&base);
        r.observe(10.0, &[at("P1", 0.5)]);
        r.observe(20.0, &[at("P2", 0.5)]);
        r.observe(21.0, &[at("P1", 1.0)]);
        assert_eq!(r.shift_from(1, 6.5), 2);
        let observed = r.raw().1;
        let times: Vec<f64> = observed.iter().map(|(t, _)| *t).collect();
        assert_eq!(times, vec![10.0, 26.5, 27.5]);
        // moving earlier past an unmoved change keeps the list sorted
        assert_eq!(r.shift_from(1, -20.0), 2);
        let times: Vec<f64> = r.raw().1.iter().map(|(t, _)| *t).collect();
        assert_eq!(times, vec![6.5, 7.5, 10.0]);
        assert_eq!(r.shift_from(9, 1.0), 0);
    }

    fn state_of(commands: &[FixtureCommand]) -> FixtureState {
        let mut state = FixtureState::new();
        state.apply_all(commands);
        state
    }

    fn times_values(r: &LookRecorder) -> Vec<(f64, FixtureCommand)> {
        r.raw().1
    }

    #[test]
    fn a_stretch_recorded_again_replaces_the_earlier_recording_of_it() {
        let base = state_of(&[at("P1", 0.0)]);
        let mut r = LookRecorder::new(&base);
        r.observe(10.0, &[at("P1", 0.5)]);
        r.observe(110.0, &[at("P1", 0.7)]);
        r.end_stretch(120.0);
        // seek back to 100 s: the stretch 100..120 is recorded again (the light was at 0.5)
        r.begin_stretch(100.0, &state_of(&[at("P1", 0.5)]));
        r.observe(150.0, &[at("P1", 0.8)]);
        r.end_stretch(200.0);
        let got = times_values(&r);
        assert_eq!(got, vec![(10.0, at("P1", 0.5)), (150.0, at("P1", 0.8))]);
    }

    #[test]
    fn a_short_re_recording_inside_a_long_one_keeps_the_state_after_it() {
        let base = state_of(&[at("P1", 0.0)]);
        let mut r = LookRecorder::new(&base);
        r.observe(10.0, &[at("P1", 0.5)]);
        r.observe(120.0, &[at("P1", 0.6)]);
        r.observe(250.0, &[at("P1", 0.9)]);
        r.end_stretch(300.0);
        // 100..150 is recorded again, and the light was at 0.2 then
        r.begin_stretch(100.0, &state_of(&[at("P1", 0.2)]));
        r.end_stretch(150.0);
        let got = times_values(&r);
        // before: 10 s -> 0.5; the later take starts at 100 with 0.2; what the long take knew at
        // 150 (0.6) starts its remaining part; then 250 -> 0.9
        assert_eq!(
            got,
            vec![
                (10.0, at("P1", 0.5)),
                (100.0, at("P1", 0.2)),
                (150.0, at("P1", 0.6)),
                (250.0, at("P1", 0.9)),
            ]
        );
    }

    #[test]
    fn a_gap_starts_the_next_stretch_with_the_value_the_light_has_by_then() {
        let base = state_of(&[at("P1", 0.0), at("P2", 0.0)]);
        let mut r = LookRecorder::new(&base);
        r.observe(10.0, &[at("P1", 0.5)]);
        r.end_stretch(20.0); // the link drops; both lights move meanwhile
        assert_eq!(r.observe(25.0, &[at("P2", 0.4)]), 0, "nothing in a gap");
        assert!(!r.is_recording());
        r.begin_stretch(40.0, &state_of(&[at("P1", 0.8), at("P2", 0.4)]));
        r.observe(45.0, &[at("P2", 0.6)]);
        let got = times_values(&r);
        assert_eq!(
            got,
            vec![
                (10.0, at("P1", 0.5)),
                (40.0, at("P1", 0.8)),
                (40.0, at("P2", 0.4)),
                (45.0, at("P2", 0.6)),
            ]
        );
    }
}
