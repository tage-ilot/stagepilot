//! Smooth capture: fits a recording of fixture changes with straight-line ramps and repeating
//! effects, so a song is hundreds of timeline items instead of tens of thousands. Pure: no I/O,
//! no clock.
//!
//! # What is fitted
//!
//! Per address (`/fixture/<name>/overrides/<property>`) the recorded changes are a series of
//! samples a few milliseconds apart. They become, in this order:
//!
//! 1. **Effects** (see `crate::lightkey_effects`): a window where a fixture group repeats the
//!    same cycle is stored once, as a loop.
//! 2. **Ramps** for everything else: greedy piecewise-linear fit through the samples with a
//!    largest error of 3 % (levels, colour per channel) or 1.5 degrees (angles), a ramp never
//!    longer than 60 s. Samples more than 0.5 s apart are never joined: the value is held and
//!    steps at the next sample ("hold then step"). Options, text, bools and a changed argument
//!    count never interpolate; they step at the time they change. A segment shorter than 0.1 s
//!    whose change is inside the tolerance is an instant step; a short but larger move keeps
//!    its true (short) fade so the error bound holds.
//! 3. **Rows**: the ramps and steps of one fixture that start within [`GROUP_SLOT`] and last
//!    the same time become ONE row with several messages; rows of different fixtures that are
//!    exactly identical (a chase at zero offset) share one row.
//!
//! # How it is judged
//!
//! [`reconstruct_report`] rebuilds every address at 25 ms from the rows and effects (with the
//! same [`Timeline`] the replay uses) and compares it with the recorded series (linear between
//! samples at most 0.5 s apart, held otherwise).

use crate::lightkey_curve::Timeline;
use crate::lightkey_effects::{self, EffectInfo};
use crate::lightkey_fixtures::{fixture_of, FixtureArg, FixtureCommand};
use stagepilot_core::config::{
    round_millis, LightingEffect, OscCueSource, OscLightingCue, OscMessage,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

/// Largest error of a fitted level or colour channel (fraction of the 0..1 range).
pub const LEVEL_TOLERANCE: f64 = 0.03;
/// Largest error of a fitted angle, degrees.
pub const ANGLE_TOLERANCE: f64 = 1.5;
/// A ramp never runs longer than this, seconds.
pub const MAX_RAMP_SECONDS: f64 = 60.0;
/// Samples further apart than this are not joined by a ramp: hold, then step.
pub const GAP_SECONDS: f64 = 0.5;
/// A fitted segment shorter than this whose change is inside the tolerance is an instant step.
pub const STEP_SECONDS: f64 = 0.1;
/// Ramps of one fixture that start within this many seconds (and last the same time, within
/// the same slack) share a row.
pub const GROUP_SLOT: f64 = 0.02;
/// Resolution of the reconstruction check, seconds.
pub const CHECK_STEP_SECONDS: f64 = 0.025;
const EPSILON: f64 = 1e-9;

fn property_of(address: &str) -> &str {
    address.rsplit('/').next().unwrap_or("")
}

/// Whether the address is an angle (pan, tilt, zoom angle, rotation).
pub fn is_angle(address: &str) -> bool {
    let p = property_of(address).to_lowercase();
    ["pan", "tilt", "angle", "rotat"]
        .iter()
        .any(|k| p.contains(k))
}

/// The class a property is reported under.
pub fn property_class(address: &str, dim: usize) -> &'static str {
    let p = property_of(address).to_lowercase();
    if is_angle(address) {
        "angle"
    } else if p.contains("dimmer") || p.contains("intensity") {
        "dimmer"
    } else if p.contains("color") && dim >= 3 {
        "colour"
    } else {
        "other level"
    }
}

/// Largest error allowed for this address, `scale` times the base tolerance (a recording that
/// would not fit the caps is fitted again at a looser tolerance, see `crate::lightkey_fit`).
pub fn tolerance_for(address: &str, scale: f64) -> f64 {
    scale
        * if is_angle(address) {
            ANGLE_TOLERANCE
        } else {
            LEVEL_TOLERANCE
        }
}

/// Settings of one fit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmoothParams {
    /// Multiplies both tolerances (1.0 normally; larger when a song is too large to save as
    /// recorded, see `crate::lightkey_fit`).
    pub tolerance_scale: f64,
    /// Look for repeating effects (always on in the app; tests turn it off to test ramps).
    pub detect_effects: bool,
}

impl Default for SmoothParams {
    fn default() -> Self {
        Self {
            tolerance_scale: 1.0,
            detect_effects: true,
        }
    }
}

/// One timeline row before it becomes an [`OscLightingCue`].
#[derive(Debug, Clone, PartialEq)]
pub struct SmoothLook {
    pub at_seconds: f64,
    pub fade_seconds: f64,
    pub messages: Vec<FixtureCommand>,
}

/// Why a recorded series is (partly) plain ramps.
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesNote {
    pub address: String,
    pub samples: usize,
    /// Rows (ramps + steps) this address needed in the saved rows.
    pub items: usize,
    pub reason: String,
}

/// The result of fitting a recording.
#[derive(Debug, Clone, Default)]
pub struct SmoothCapture {
    pub rows: Vec<SmoothLook>,
    pub effects: Vec<LightingEffect>,
    pub effect_infos: Vec<EffectInfo>,
    pub notes: Vec<SeriesNote>,
    /// Raw samples per address that were read (for the report).
    pub raw_samples: usize,
    /// Messages in the rows (a row carries several).
    pub ramps: usize,
    pub steps: usize,
}

// ------------------------------------------------------------------------- recorded series

/// All samples of one address, after the start state and the end are settled.
#[derive(Debug, Clone)]
pub struct AddrData {
    pub address: String,
    pub items: Vec<(f64, Vec<FixtureArg>)>,
}

/// Samples of one address as numbers (every sample numeric, one argument count).
#[derive(Debug, Clone)]
pub struct Series {
    pub address: String,
    pub dim: usize,
    pub ts: Vec<f64>,
    /// `ts.len() * dim` values, sample by sample.
    pub vs: Vec<f64>,
}

impl Series {
    pub fn from_data(data: &AddrData) -> Option<Series> {
        let dim = data.items.first()?.1.len();
        if dim == 0 {
            return None;
        }
        let mut ts = Vec::with_capacity(data.items.len());
        let mut vs = Vec::with_capacity(data.items.len() * dim);
        for (t, v) in &data.items {
            if v.len() != dim {
                return None;
            }
            ts.push(*t);
            for a in v {
                match a {
                    FixtureArg::Float(f) if f.is_finite() => vs.push(*f),
                    _ => return None,
                }
            }
        }
        Some(Series {
            address: data.address.clone(),
            dim,
            ts,
            vs,
        })
    }

    pub fn len(&self) -> usize {
        self.ts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ts.is_empty()
    }

    pub fn sample(&self, i: usize) -> &[f64] {
        &self.vs[i * self.dim..(i + 1) * self.dim]
    }

    /// The recorded value at `t`: linear between samples at most [`GAP_SECONDS`] apart, held
    /// otherwise, the first/last value outside the recording.
    pub fn at(&self, t: f64, out: &mut [f64]) {
        let n = self.ts.len();
        let i = self.ts.partition_point(|&x| x <= t);
        if i == 0 {
            out.copy_from_slice(self.sample(0));
        } else if i >= n {
            out.copy_from_slice(self.sample(n - 1));
        } else {
            let (t0, t1) = (self.ts[i - 1], self.ts[i]);
            let (a, b) = (self.sample(i - 1), self.sample(i));
            if t1 - t0 <= GAP_SECONDS && t1 > t0 {
                let f = (t - t0) / (t1 - t0);
                for k in 0..self.dim {
                    out[k] = a[k] + (b[k] - a[k]) * f;
                }
            } else {
                out.copy_from_slice(a);
            }
        }
    }

    /// The series on a regular grid `0, dt, 2dt...` (`n` points), `n * dim` values.
    pub fn grid(&self, dt: f64, n: usize) -> Vec<f64> {
        let mut out = vec![0.0; n * self.dim];
        let mut buf = vec![0.0; self.dim];
        for i in 0..n {
            self.at(i as f64 * dt, &mut buf);
            out[i * self.dim..(i + 1) * self.dim].copy_from_slice(&buf);
        }
        out
    }
}

/// The samples of every address in the order they first appear: the baseline start value of
/// an address that only moves later, changes at or before 0 as the start state at 0.0, and the
/// last change after `duration` folded onto `duration` (earlier ones after it are dropped).
pub fn prepare(
    baseline: &[FixtureCommand],
    changes: &[(f64, FixtureCommand)],
    duration: f64,
) -> Vec<AddrData> {
    let mut order: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut base: HashMap<&str, &FixtureCommand> = HashMap::new();
    for c in baseline {
        if seen.insert(&c.a) {
            order.push(&c.a);
        }
        base.insert(&c.a, c);
    }
    let mut series: HashMap<&str, Vec<(f64, &FixtureCommand)>> = HashMap::new();
    for (at, c) in changes {
        if seen.insert(&c.a) {
            order.push(&c.a);
        }
        series.entry(&c.a).or_default().push((*at, c));
    }
    let mut out = Vec::new();
    for address in order {
        let Some(mut list) = series.remove(address) else {
            continue;
        };
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut start: Option<&FixtureCommand> = None;
        let mut later: Vec<(f64, &FixtureCommand)> = Vec::new();
        let mut after_end: Option<&FixtureCommand> = None;
        for (at, c) in list {
            if at <= 0.0 {
                start = Some(c);
            } else if duration <= 0.0 || at <= duration {
                later.push((at, c));
            } else {
                after_end = Some(c);
            }
        }
        if let Some(c) = after_end {
            later.push((duration, c));
        }
        let changed_at_start = start.is_some();
        if later.is_empty() && !changed_at_start {
            continue;
        }
        let start = start.or_else(|| base.get(address).copied());
        let mut items: Vec<(f64, Vec<FixtureArg>)> = Vec::new();
        if let Some(s) = start {
            items.push((0.0, s.v.clone()));
        }
        for (at, c) in later {
            match items.last_mut() {
                Some(last) if (last.0 - at).abs() < EPSILON => last.1 = c.v.clone(),
                _ => items.push((at, c.v.clone())),
            }
        }
        out.push(AddrData {
            address: address.to_string(),
            items,
        });
    }
    out
}

// ------------------------------------------------------------------------------- the ramp fit

fn max_diff(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max)
}

/// Greedy piecewise-linear fit through sample points: segments `(i, j)` (sample indexes) that
/// start where the last ended. Every sample strictly inside a segment lies within `tol` of the
/// straight line between the segment's end samples (largest error over the channels), and a
/// segment is never longer than `max_ramp` seconds. A segment of two samples always fits.
pub fn polyline(
    ts: &[f64],
    vs: &[f64],
    dim: usize,
    tol: f64,
    max_ramp: f64,
) -> Vec<(usize, usize)> {
    let n = ts.len();
    let mut segments = Vec::new();
    let mut i = 0;
    let mut line = vec![0.0; dim];
    while i + 1 < n {
        let mut j = i + 1;
        while j + 1 < n && ts[j + 1] - ts[i] <= max_ramp {
            let (t0, t1) = (ts[i], ts[j + 1]);
            let (v0, v1) = (
                &vs[i * dim..(i + 1) * dim],
                &vs[(j + 1) * dim..(j + 2) * dim],
            );
            let mut ok = true;
            for k in i + 1..=j {
                let f = if t1 > t0 {
                    (ts[k] - t0) / (t1 - t0)
                } else {
                    1.0
                };
                for c in 0..dim {
                    line[c] = v0[c] + (v1[c] - v0[c]) * f;
                }
                if max_diff(&vs[k * dim..(k + 1) * dim], &line) > tol {
                    ok = false;
                    break;
                }
            }
            if !ok {
                break;
            }
            j += 1;
        }
        segments.push((i, j));
        i = j;
    }
    segments
}

fn round4(x: f64) -> f64 {
    let r = (x * 10_000.0).round_ties_even() / 10_000.0;
    if !r.is_finite() {
        // 1e305 * 10,000 is infinity: keep the value, a row must never hold a non-finite number
        x
    } else if r == 0.0 {
        0.0
    } else {
        r
    }
}

fn rounded(v: &[f64]) -> Vec<FixtureArg> {
    v.iter().map(|x| FixtureArg::Float(round4(*x))).collect()
}

/// One ramp or step of one address, before grouping.
#[derive(Debug, Clone, PartialEq)]
struct Item {
    address: String,
    at: f64,
    fade: f64,
    value: Vec<FixtureArg>,
}

/// One sample of the fit input: time, numbers or not, and whether the run must break before it
/// (the first sample after an effect window).
struct Sample<'a> {
    t: f64,
    v: &'a [FixtureArg],
    numeric: Option<Vec<f64>>,
    cut_before: bool,
}

/// Fit one address. `cuts` are windows an effect drives: the samples inside are dropped, the
/// ramp ends at the window start and the rows restart (with a step) at its end.
type EmitFn<'a> = dyn FnMut(&mut Vec<Item>, f64, f64, Vec<FixtureArg>) + 'a;

fn fit_address(
    data: &AddrData,
    series: Option<&Series>,
    tol: f64,
    cuts: &[(f64, f64)],
) -> Vec<Item> {
    let mut samples: Vec<Sample> = Vec::new();
    let mut synthetic: Vec<(f64, Vec<FixtureArg>)> = Vec::new();
    let numeric_of = |v: &[FixtureArg]| -> Option<Vec<f64>> {
        if v.is_empty() {
            return None;
        }
        v.iter()
            .map(|a| match a {
                FixtureArg::Float(f) if f.is_finite() => Some(*f),
                _ => None,
            })
            .collect()
    };
    if let (false, Some(series)) = (cuts.is_empty(), series) {
        let mut buf = vec![0.0; series.dim];
        // synthetic edge samples first, so they can be borrowed below
        for &(s, e) in cuts {
            for edge in [s, e] {
                series.at(edge, &mut buf);
                synthetic.push((edge, rounded(&buf)));
            }
        }
        let inside = |t: f64| cuts.iter().any(|&(s, e)| t > s && t < e);
        let mut all: Vec<(f64, &Vec<FixtureArg>, bool)> = Vec::new();
        for (t, v) in &data.items {
            if !inside(*t)
                && !cuts
                    .iter()
                    .any(|&(s, e)| (*t - s).abs() < EPSILON || (*t - e).abs() < EPSILON)
            {
                all.push((*t, v, false));
            }
        }
        for (k, &(s, e)) in cuts.iter().enumerate() {
            all.push((s, &synthetic[2 * k].1, false));
            all.push((e, &synthetic[2 * k + 1].1, true));
        }
        all.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (t, v, cut) in all {
            samples.push(Sample {
                t,
                v,
                numeric: numeric_of(v),
                cut_before: cut,
            });
        }
    } else {
        for (t, v) in &data.items {
            samples.push(Sample {
                t: *t,
                v,
                numeric: numeric_of(v),
                cut_before: false,
            });
        }
    }

    let mut items: Vec<Item> = Vec::new();
    let mut last: Option<Vec<FixtureArg>> = None;
    let mut emit = |items: &mut Vec<Item>, at: f64, fade: f64, value: Vec<FixtureArg>| {
        // A row that does not change the value (rounded) is not needed.
        if fade <= 0.0 && last.as_ref() == Some(&value) {
            return;
        }
        last = Some(value.clone());
        items.push(Item {
            address: data.address.clone(),
            at: round_millis(at),
            fade: round_millis(fade),
            value,
        });
    };
    let mut run: Vec<usize> = Vec::new();
    let flush = |run: &mut Vec<usize>, items: &mut Vec<Item>, emit: &mut EmitFn| {
        if run.is_empty() {
            return;
        }
        let first = &samples[run[0]];
        let dim = first.numeric.as_ref().map_or(0, Vec::len);
        if dim == 0 {
            // not numeric: steps at the time of each change
            for &k in run.iter() {
                let s = &samples[k];
                emit(items, s.t, 0.0, s.v.to_vec());
            }
            run.clear();
            return;
        }
        let ts: Vec<f64> = run.iter().map(|&k| samples[k].t).collect();
        let mut vs: Vec<f64> = Vec::with_capacity(run.len() * dim);
        for &k in run.iter() {
            vs.extend(samples[k].numeric.as_ref().expect("numeric run"));
        }
        emit(items, ts[0], 0.0, rounded(&vs[0..dim]));
        for (i, j) in polyline(&ts, &vs, dim, tol, MAX_RAMP_SECONDS) {
            let to = rounded(&vs[j * dim..(j + 1) * dim]);
            let change = max_diff(&vs[i * dim..(i + 1) * dim], &vs[j * dim..(j + 1) * dim]);
            let dt = ts[j] - ts[i];
            if dt < STEP_SECONDS && change <= tol {
                emit(items, ts[i], 0.0, to);
            } else {
                // flat segments are skipped by the next ramp starting from the held value
                let from = rounded(&vs[i * dim..(i + 1) * dim]);
                if from == to {
                    continue;
                }
                emit(items, ts[i], dt, to);
            }
        }
        run.clear();
    };

    for k in 0..samples.len() {
        let s = &samples[k];
        let continues = run.last().is_some_and(|&p| {
            let prev = &samples[p];
            !s.cut_before
                && s.t - prev.t <= GAP_SECONDS
                && matches!((&prev.numeric, &s.numeric), (Some(a), Some(b)) if a.len() == b.len())
        });
        if !continues {
            flush(&mut run, &mut items, &mut emit);
        }
        run.push(k);
    }
    flush(&mut run, &mut items, &mut emit);
    items
}

// ---------------------------------------------------------------------------------- grouping

fn split_address(address: &str) -> (&str, &str) {
    let fixture = fixture_of(address).unwrap_or("");
    (fixture, property_of(address))
}

/// Group items into rows: per fixture (items starting within [`GROUP_SLOT`] with the same
/// fade), then rows of different fixtures that are exactly identical share one row.
fn group_rows(mut items: Vec<Item>, order: &HashMap<String, usize>) -> Vec<SmoothLook> {
    items.sort_by(|a, b| {
        a.at.total_cmp(&b.at)
            .then_with(|| a.fade.total_cmp(&b.fade))
            .then_with(|| order.get(&a.address).cmp(&order.get(&b.address)))
    });
    struct Group {
        fixture: String,
        at: f64,
        fade: f64,
        items: Vec<Item>,
    }
    let mut groups: Vec<Group> = Vec::new();
    // open groups per fixture, newest last
    let mut open: HashMap<String, Vec<usize>> = HashMap::new();
    for item in items {
        let (fixture, _) = split_address(&item.address);
        let fixture = fixture.to_string();
        let list = open.entry(fixture.clone()).or_default();
        // groups that ended more than the slot ago can no longer take items
        list.retain(|&g| item.at - groups[g].at <= GROUP_SLOT + EPSILON);
        let found = list.iter().copied().find(|&g| {
            (groups[g].fade - item.fade).abs() <= GROUP_SLOT + EPSILON
                && !groups[g].items.iter().any(|i| i.address == item.address)
        });
        match found {
            Some(g) => groups[g].items.push(item),
            None => {
                groups.push(Group {
                    fixture: fixture.clone(),
                    at: item.at,
                    fade: item.fade,
                    items: vec![item],
                });
                list.push(groups.len() - 1);
            }
        }
    }
    // exact merge across fixtures
    type Key = (i64, i64, Vec<(String, Vec<i64>)>);
    let mut rows: Vec<SmoothLook> = Vec::new();
    let mut index: HashMap<Key, usize> = HashMap::new();
    for g in groups {
        let mut sig: Vec<(String, Vec<i64>)> = g
            .items
            .iter()
            .map(|i| {
                (
                    property_of(&i.address).to_string(),
                    i.value
                        .iter()
                        .map(|a| match a {
                            FixtureArg::Float(f) => (f * 10_000.0).round() as i64,
                            _ => i64::MIN,
                        })
                        .chain(std::iter::once(
                            // non-numeric values never merge unless equal
                            i.value
                                .iter()
                                .map(|a| match a {
                                    FixtureArg::Float(_) => 0,
                                    other => {
                                        use std::hash::{Hash, Hasher};
                                        let mut h =
                                            std::collections::hash_map::DefaultHasher::new();
                                        format!("{other:?}").hash(&mut h);
                                        h.finish() as i64
                                    }
                                })
                                .fold(0, i64::wrapping_add),
                        ))
                        .collect(),
                )
            })
            .collect();
        sig.sort();
        let key: Key = (
            (g.at * 1000.0).round() as i64,
            (g.fade * 1000.0).round() as i64,
            sig,
        );
        let _ = &g.fixture;
        let commands: Vec<FixtureCommand> = g
            .items
            .into_iter()
            .map(|i| FixtureCommand::new(i.address, i.value))
            .collect();
        match index.get(&key) {
            Some(&r) => rows[r].messages.extend(commands),
            None => {
                index.insert(key, rows.len());
                rows.push(SmoothLook {
                    at_seconds: g.at,
                    fade_seconds: g.fade,
                    messages: commands,
                });
            }
        }
    }
    rows.sort_by(|a, b| {
        a.at_seconds
            .total_cmp(&b.at_seconds)
            .then_with(|| a.fade_seconds.total_cmp(&b.fade_seconds))
    });
    rows
}

// ------------------------------------------------------------------------------- the pipeline

/// Fit a recording: ramps and steps for everything, effects where a loop repeats. See the
/// module documentation. `changes` need not be sorted.
pub fn fit(
    baseline: &[FixtureCommand],
    changes: &[(f64, FixtureCommand)],
    duration: f64,
    params: &SmoothParams,
) -> SmoothCapture {
    let data = prepare(baseline, changes, duration);
    let order: HashMap<String, usize> = data
        .iter()
        .enumerate()
        .map(|(i, d)| (d.address.clone(), i))
        .collect();
    let series: Vec<Option<Series>> = data.iter().map(Series::from_data).collect();
    let end = data
        .iter()
        .filter_map(|d| d.items.last().map(|i| i.0))
        .fold(0.0, f64::max);

    let mut capture = SmoothCapture {
        raw_samples: data.iter().map(|d| d.items.len()).sum(),
        ..Default::default()
    };
    let mut cuts: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
    let mut reasons: HashMap<String, String> = HashMap::new();
    if params.detect_effects {
        let numeric: Vec<&Series> = series.iter().flatten().collect();
        let found = lightkey_effects::detect(&numeric, end, params.tolerance_scale);
        capture.effects = found.effects;
        capture.effect_infos = found.infos;
        cuts = found.taken;
        reasons = found.reasons;
    }

    let mut items: Vec<Item> = Vec::new();
    let mut per_address: HashMap<&str, usize> = HashMap::new();
    for (d, s) in data.iter().zip(&series) {
        let tol = tolerance_for(&d.address, params.tolerance_scale);
        let mut windows = cuts.get(&d.address).cloned().unwrap_or_default();
        windows.sort_by(|a, b| a.0.total_cmp(&b.0));
        let fitted = fit_address(d, s.as_ref(), tol, &windows);
        per_address.insert(&d.address, fitted.len());
        items.extend(fitted);
    }
    for (d, s) in data.iter().zip(&series) {
        let items_n = per_address.get(d.address.as_str()).copied().unwrap_or(0);
        let reason = match (&s, reasons.get(&d.address)) {
            (None, _) => {
                "not a plain number (options, text or a changing argument count): steps".to_string()
            }
            (Some(_), Some(r)) => r.clone(),
            (Some(_), None) => "ramps".to_string(),
        };
        capture.notes.push(SeriesNote {
            address: d.address.clone(),
            samples: d.items.len(),
            items: items_n,
            reason,
        });
    }
    let rows = group_rows(items, &order);
    capture.ramps = rows
        .iter()
        .filter(|r| r.fade_seconds > 0.0)
        .map(|r| r.messages.len())
        .sum();
    capture.steps = rows
        .iter()
        .filter(|r| r.fade_seconds <= 0.0)
        .map(|r| r.messages.len())
        .sum();
    capture.rows = rows;
    capture
}

/// The timeline rows of a fit: `source = captured`, no colour, label "Look - N fixtures", times
/// and fades to the millisecond. A row over the per-row message cap is split into several rows
/// at the same time.
pub fn smooth_rows(rows: &[SmoothLook]) -> Vec<OscLightingCue> {
    use stagepilot_core::config::MAX_OSC_MESSAGES_PER_ROW;
    let mut out = Vec::new();
    for look in rows {
        for chunk in look.messages.chunks(MAX_OSC_MESSAGES_PER_ROW) {
            if chunk.is_empty() {
                continue;
            }
            let n = chunk
                .iter()
                .filter_map(FixtureCommand::fixture)
                .collect::<HashSet<_>>()
                .len();
            out.push(OscLightingCue {
                id: Uuid::new_v4(),
                at_seconds: round_millis(look.at_seconds),
                address: crate::lightkey_looks::LOOK_ROW_ADDRESS.to_string(),
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
                fade_seconds: round_millis(look.fade_seconds),
            });
        }
    }
    out
}

// ------------------------------------------------------------------ the reconstruction check

/// Error statistics of one property class.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClassError {
    pub class: String,
    /// Grid points compared.
    pub points: usize,
    pub max: f64,
    pub p99: f64,
    /// Share of points whose error is above the tolerance of the class.
    pub over_tolerance: f64,
    /// Share of points whose error is above twice the tolerance.
    pub over_double: f64,
    pub tolerance: f64,
}

/// What rebuilding the recording from rows and effects gives, per property class.
#[derive(Debug, Clone, Default)]
pub struct ReconstructReport {
    /// Points not driven by an effect.
    pub outside_effects: Vec<ClassError>,
    /// Points driven by an effect.
    pub inside_effects: Vec<ClassError>,
    /// All points.
    pub overall: Vec<ClassError>,
    /// Non-numeric addresses: share of the time the rebuilt value differs from the recorded.
    pub option_mismatch: f64,
    pub option_points: usize,
}

fn summarise(class: &str, tolerance: f64, mut errors: Vec<f32>) -> Option<ClassError> {
    if errors.is_empty() {
        return None;
    }
    errors.sort_by(f32::total_cmp);
    let n = errors.len();
    let p99 = f64::from(errors[((n as f64 * 0.99).ceil() as usize).clamp(1, n) - 1]);
    let over = |limit: f64| {
        errors
            .iter()
            .filter(|e| f64::from(**e) > limit + 1e-9)
            .count() as f64
            / n as f64
    };
    Some(ClassError {
        class: class.to_string(),
        points: n,
        max: f64::from(errors[n - 1]),
        p99,
        over_tolerance: over(tolerance),
        over_double: over(2.0 * tolerance),
        tolerance,
    })
}

/// Rebuild every address at [`CHECK_STEP_SECONDS`] from `rows` and `effects` and compare with
/// the recorded samples of `baseline` + `changes` (the same input as [`fit`]).
pub fn reconstruct_report(
    baseline: &[FixtureCommand],
    changes: &[(f64, FixtureCommand)],
    duration: f64,
    rows: &[OscLightingCue],
    effects: &[LightingEffect],
    tolerance_scale: f64,
) -> ReconstructReport {
    let data = prepare(baseline, changes, duration);
    let timeline = Timeline::new(rows, effects);
    type Buckets = BTreeMap<&'static str, Vec<f32>>;
    let (mut outside, mut inside, mut overall): (Buckets, Buckets, Buckets) =
        (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
    let mut report = ReconstructReport::default();
    let mut option_bad = 0usize;
    let mut tols: BTreeMap<&'static str, f64> = BTreeMap::new();
    for d in &data {
        let Some(series) = Series::from_data(d) else {
            // options and text: held value must match exactly
            let mut k = 0usize;
            let end = d.items.last().map_or(0.0, |i| i.0);
            let mut t = d.items.first().map_or(0.0, |i| i.0);
            while t <= end + 1e-9 {
                while k + 1 < d.items.len() && d.items[k + 1].0 <= t + 1e-9 {
                    k += 1;
                }
                let want = &d.items[k].1;
                let have = timeline.value_at(&d.address, t);
                report.option_points += 1;
                if have.as_ref() != Some(want) {
                    option_bad += 1;
                }
                t += CHECK_STEP_SECONDS;
            }
            continue;
        };
        let class = property_class(&d.address, series.dim);
        let tol = tolerance_for(&d.address, tolerance_scale);
        tols.insert(class, tol);
        let mut buf = vec![0.0; series.dim];
        let first = series.ts[0];
        let last = *series.ts.last().expect("non-empty");
        let mut t = first;
        while t <= last + 1e-9 {
            series.at(t, &mut buf);
            if let Some(have) = timeline.value_at(&d.address, t) {
                let err = have
                    .iter()
                    .zip(&buf)
                    .map(|(h, w)| match h {
                        FixtureArg::Float(h) => (h - w).abs(),
                        _ => f64::INFINITY,
                    })
                    .fold(0.0, f64::max);
                let e = err as f32;
                overall.entry(class).or_default().push(e);
                if timeline.in_effect(&d.address, t) {
                    inside.entry(class).or_default().push(e);
                } else {
                    outside.entry(class).or_default().push(e);
                }
            } else {
                overall.entry(class).or_default().push(f32::INFINITY);
                outside.entry(class).or_default().push(f32::INFINITY);
            }
            t += CHECK_STEP_SECONDS;
        }
    }
    report.option_mismatch = if report.option_points > 0 {
        option_bad as f64 / report.option_points as f64
    } else {
        0.0
    };
    let fill = |buckets: Buckets| -> Vec<ClassError> {
        buckets
            .into_iter()
            .filter_map(|(class, errors)| summarise(class, tols[class], errors))
            .collect()
    };
    report.outside_effects = fill(outside);
    report.inside_effects = fill(inside);
    report.overall = fill(overall);
    report
}
