//! Effect detection: finds the loops a recording repeats and stores each as ONE item. Pure: no
//! I/O, no clock.
//!
//! # Method
//!
//! 1. Every numeric address is resampled to a 50 ms grid. For each candidate lag `T` of 1 to
//!    30 s the grid is compared with itself `T` earlier; a point *matches* when the two values
//!    are within twice the tolerance and the value moved by at least four tolerances during
//!    that period (so a flat stretch never counts). The longest window in which at least 90 %
//!    of the points match (and which holds at least three cycles) is the loop of that lag; the
//!    smallest lag whose window is within 5 % of the best one wins (a loop of `T` also repeats
//!    after `2T`). The period is then refined to the millisecond on the raw samples, comparing
//!    many cycles apart so a tiny period error shows. Up to three separate windows are taken
//!    per address, so an effect that stops and starts again is found twice.
//! 2. Candidates of one fixture with the same period (within 1 %) and overlapping windows are
//!    joined (pan and tilt of one mover); fixtures with the same properties, the same period
//!    and overlapping windows are joined into a cluster, each fixture's phase offset found by
//!    sliding its grid against the seed's, then refined on the raw samples.
//! 3. The cycle is the per-phase median of all members and all cycles, fitted with ramps
//!    (the same fit as the rows, at half the tolerance, closed into a loop).
//! 4. Acceptance: the effect is rebuilt with [`Timeline`], the same evaluator the replay
//!    uses, and compared with the raw samples of every member over the window at 25 ms. A
//!    member stays when at least 98 % of its points are within twice the tolerance; members
//!    that fail leave and are tried as effects of their own. An effect must also be worth it:
//!    its cycle steps must be at most 60 % of the ramp segments the same window needs as plain
//!    rows.
//!
//! False-positive guards: a slow monotone fade does not match itself `T` earlier (it moved too
//! far in a period, or too little to count as moving); a flat stretch never matches; a window
//! shorter than three cycles is never an effect; two overlapping loops on one address do not
//! match any lag at 90 % and stay ramps.

use crate::lightkey_curve::Timeline;
use crate::lightkey_fixtures::{fixture_of, FixtureArg};
use crate::lightkey_smooth::{polyline, property_class, tolerance_for, Series, MAX_RAMP_SECONDS};
use stagepilot_core::config::{
    EffectMember, EffectStep, LightingEffect, OscCueSource, MAX_EFFECTS_PER_SONG,
    MAX_EFFECT_CYCLE_STEPS, MAX_EFFECT_MEMBERS,
};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

const DT: f64 = 0.05;
const MIN_PERIOD: f64 = 1.0;
const MAX_PERIOD: f64 = 30.0;
/// An address needs this many samples before a loop is looked for.
const MIN_SAMPLES: usize = 30;
const WINDOWS_PER_ADDRESS: usize = 3;
const TEMPLATE_STEP: f64 = 0.025;
const ACCEPT_SHARE: f64 = 0.98;
const MAX_STEP_SHARE: f64 = 0.6;
const MIN_CYCLES: f64 = 3.0;
/// Loops of different addresses whose periods differ by less than this share a cluster; the
/// period is then refined on the cluster as a whole.
const PERIOD_SLACK: f64 = 0.02;

/// One accepted effect, for the report.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectInfo {
    pub label: String,
    pub period_seconds: f64,
    pub start_seconds: f64,
    pub end_seconds: f64,
    /// Fixture and phase offset.
    pub members: Vec<(String, f64)>,
    pub properties: Vec<String>,
    pub cycle_steps: usize,
    /// Plain ramp segments the same windows would have needed.
    pub replaces_segments: usize,
    /// Worst member: share of the window within twice the tolerance.
    pub worst_share: f64,
}

/// What detection found.
#[derive(Debug, Clone, Default)]
pub struct Detected {
    pub effects: Vec<LightingEffect>,
    pub infos: Vec<EffectInfo>,
    /// Per address: the windows an effect drives (the rows skip them).
    pub taken: HashMap<String, Vec<(f64, f64)>>,
    /// Per address: why it stays ramps (or partly).
    pub reasons: HashMap<String, String>,
}

// ----------------------------------------------------------------- range queries (sparse table)

struct Extent {
    dim: usize,
    n: usize,
    mins: Vec<Vec<f64>>,
    maxs: Vec<Vec<f64>>,
}

impl Extent {
    fn new(x: &[f64], dim: usize, n: usize) -> Self {
        let mut mins = vec![x.to_vec()];
        let mut maxs = vec![x.to_vec()];
        let mut span = 1;
        while span * 2 <= n {
            let (pm, px) = (mins.last().expect("level"), maxs.last().expect("level"));
            let len = n - span * 2 + 1;
            let mut nm = vec![0.0; len * dim];
            let mut nx = vec![0.0; len * dim];
            for i in 0..len {
                for c in 0..dim {
                    nm[i * dim + c] = pm[i * dim + c].min(pm[(i + span) * dim + c]);
                    nx[i * dim + c] = px[i * dim + c].max(px[(i + span) * dim + c]);
                }
            }
            mins.push(nm);
            maxs.push(nx);
            span *= 2;
        }
        Self { dim, n, mins, maxs }
    }

    /// Largest (max - min) over channels in `l..=r`.
    fn range(&self, l: usize, r: usize) -> f64 {
        debug_assert!(r < self.n);
        let len = r - l + 1;
        let level = usize::BITS as usize - 1 - len.leading_zeros() as usize;
        let (a, b) = (l, r + 1 - (1 << level));
        let mut best = 0.0f64;
        for c in 0..self.dim {
            let lo = self.mins[level][a * self.dim + c].min(self.mins[level][b * self.dim + c]);
            let hi = self.maxs[level][a * self.dim + c].max(self.maxs[level][b * self.dim + c]);
            best = best.max(hi - lo);
        }
        best
    }
}

/// A loop found on one address.
#[derive(Debug, Clone)]
struct Candidate {
    series: usize,
    period: f64,
    start: f64,
    end: f64,
}

struct AddressSearch {
    candidates: Vec<Candidate>,
    reason: Option<String>,
}

fn grid_len(end: f64) -> usize {
    (end / DT).floor() as usize + 1
}

/// The longest window (as grid indexes `first..=last` of matching points) for lag `k`, where
/// at least 90 % match.
fn longest_window(score: &[i32]) -> Option<(usize, usize)> {
    // prefix sums; interval i..j has sum P[j] - P[i]
    let mut prefix = Vec::with_capacity(score.len() + 1);
    prefix.push(0i64);
    for s in score {
        prefix.push(prefix.last().expect("seed") + i64::from(*s));
    }
    // left candidates: strictly new minima, so prefix decreases along the list
    let mut left: Vec<usize> = Vec::new();
    for (i, p) in prefix.iter().enumerate() {
        if left.last().is_none_or(|&l| *p < prefix[l]) {
            left.push(i);
        }
    }
    let mut best: Option<(usize, usize)> = None;
    for j in (1..prefix.len()).rev() {
        let pos = left.partition_point(|&l| prefix[l] > prefix[j]);
        if let Some(&i) = left.get(pos) {
            if i < j && best.is_none_or(|(bi, bj)| j - i > bj - bi) {
                best = Some((i, j));
            }
        }
    }
    best
}

/// Search one address for loops.
fn search_address(series: &Series, index: usize, end: f64, scale: f64) -> AddressSearch {
    let tol = tolerance_for(&series.address, scale);
    let n = grid_len(end);
    let dim = series.dim;
    if series.len() < MIN_SAMPLES {
        return AddressSearch {
            candidates: vec![],
            reason: Some(format!("only {} changes: plain ramps", series.len())),
        };
    }
    let x = series.grid(DT, n);
    let ext = Extent::new(&x, dim, n);
    if ext.range(0, n - 1) < 6.0 * tol {
        return AddressSearch {
            candidates: vec![],
            reason: Some("moves less than 6 tolerances: plain ramps".into()),
        };
    }
    let kmin = (MIN_PERIOD / DT).round() as usize;
    let kmax = ((MAX_PERIOD / DT).round() as usize).min(n / 3);
    let mut masked = vec![false; n];
    let mut candidates = Vec::new();
    let mut best_share = 0.0f64;
    for _ in 0..WINDOWS_PER_ADDRESS {
        // (k, total points of the window, first matching index, last matching index)
        let mut found: Vec<(usize, usize, usize, usize)> = Vec::new();
        for k in kmin..=kmax {
            let mut score = vec![-9i32; n - k];
            for i in k..n {
                if masked[i] {
                    continue;
                }
                let mut diff = 0.0f64;
                for c in 0..dim {
                    diff = diff.max((x[i * dim + c] - x[(i - k) * dim + c]).abs());
                }
                if diff <= 2.0 * tol && ext.range(i - k, i) >= 4.0 * tol {
                    score[i - k] = 1;
                }
            }
            if let Some((a, b)) = longest_window(&score) {
                // score index s is grid index s + k; matched points a..b (b exclusive)
                let first = a + k;
                let last = b + k - 1;
                let span = last + 1 - first;
                if span >= 2 * k {
                    found.push((k, span + k, first, last));
                }
            }
        }
        let Some(&(_, best_total, _, _)) = found.iter().max_by_key(|f| f.1) else {
            break;
        };
        best_share = best_share.max(best_total as f64 / n as f64);
        // the best window's lag, then its fundamental: a loop of T also repeats after 2T, 3T...
        // and the best window sits somewhere on a plateau of near-equal lags
        let near = |f: &&(usize, usize, usize, usize)| f.1 as f64 >= 0.95 * best_total as f64;
        let Some(&(mut k, _, mut first, mut last)) = found.iter().filter(near).max_by_key(|f| f.1)
        else {
            break;
        };
        loop {
            let divisor = (2..=8usize).find_map(|m| {
                let target = k / m;
                found
                    .iter()
                    .filter(near)
                    .filter(|f| f.0 >= kmin && f.0 + 2 >= target && f.0 <= target + 2 && f.0 < k)
                    .max_by_key(|f| f.1)
                    .copied()
            });
            match divisor {
                Some((k2, _, f2, l2)) => {
                    k = k2;
                    first = f2;
                    last = l2;
                }
                None => break,
            }
        }
        let w_start = first - k;
        let w_end = last;
        // block these points for the next round
        for m in masked.iter_mut().take(w_end + 1).skip(w_start) {
            *m = true;
        }
        if ext.range(w_start, w_end) < 6.0 * tol {
            continue;
        }
        let (t0, t1) = (w_start as f64 * DT, w_end as f64 * DT);
        let period = refine_period(series, t0, t1, k as f64 * DT);
        if (t1 - t0) < MIN_CYCLES * period {
            continue;
        }
        candidates.push(Candidate {
            series: index,
            period,
            start: t0,
            end: t1,
        });
    }
    let reason = if candidates.is_empty() {
        Some(format!(
            "no loop of 3 or more cycles found (best repeating window covers {:.0} % of the recording): plain ramps",
            best_share * 100.0
        ))
    } else {
        None
    };
    AddressSearch { candidates, reason }
}

/// The period to the millisecond: compare the series with itself `L` cycles earlier for `L`
/// as large as the window and the 50 ms search allow, so a 1 ms error in the period shows as
/// `L` ms of shift.
fn refine_period(series: &Series, t0: f64, t1: f64, coarse: f64) -> f64 {
    let cycles = ((t1 - t0) / coarse).floor() as usize;
    if cycles < 3 {
        return coarse;
    }
    let lag_cycles = (cycles - 1).clamp(1, 4);
    let from = t0 + lag_cycles as f64 * coarse;
    let mut a = vec![0.0; series.dim];
    let mut b = vec![0.0; series.dim];
    let mut best = (f64::INFINITY, coarse);
    let mut candidate = coarse - 0.075;
    while candidate <= coarse + 0.075 + 1e-9 {
        let lag = lag_cycles as f64 * candidate;
        let mut total = 0.0;
        let mut count = 0usize;
        let mut t = from;
        while t <= t1 {
            series.at(t, &mut a);
            series.at(t - lag, &mut b);
            total += a
                .iter()
                .zip(&b)
                .map(|(x, y)| (x - y).abs())
                .fold(0.0, f64::max);
            count += 1;
            t += DT;
        }
        let mean = total / count.max(1) as f64;
        if mean < best.0 - 1e-12 {
            best = (mean, candidate);
        }
        candidate += 0.0005;
    }
    (best.1 * 1000.0).round() / 1000.0
}

// ------------------------------------------------------------------------------- clustering

/// The loops of one fixture with the same period and window: its properties.
#[derive(Debug, Clone)]
struct FixtureLoop {
    fixture: String,
    period: f64,
    start: f64,
    end: f64,
    /// (property, series index)
    properties: Vec<(String, usize)>,
}

fn overlap(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.1.min(b.1) - a.0.max(b.0)).max(0.0)
}

fn split(address: &str) -> Option<(String, String)> {
    let fixture = fixture_of(address)?;
    let property = address.rsplit('/').next()?;
    Some((fixture.to_string(), property.to_string()))
}

fn join_fixture_loops(candidates: &[Candidate], series: &[&Series]) -> Vec<FixtureLoop> {
    let mut order: Vec<&Candidate> = candidates.iter().collect();
    order.sort_by(|a, b| (b.end - b.start).total_cmp(&(a.end - a.start)));
    let mut loops: Vec<FixtureLoop> = Vec::new();
    for c in order {
        let Some((fixture, property)) = split(&series[c.series].address) else {
            continue;
        };
        let target = loops.iter_mut().find(|l| {
            l.fixture == fixture
                && (l.period - c.period).abs() <= PERIOD_SLACK * l.period
                && !l.properties.iter().any(|(p, _)| *p == property)
                && overlap((l.start, l.end), (c.start, c.end)) >= MIN_CYCLES * l.period
        });
        match target {
            Some(l) => {
                l.start = l.start.max(c.start);
                l.end = l.end.min(c.end);
                l.properties.push((property, c.series));
            }
            None => loops.push(FixtureLoop {
                fixture,
                period: c.period,
                start: c.start,
                end: c.end,
                properties: vec![(property, c.series)],
            }),
        }
    }
    loops
}

fn property_set(l: &FixtureLoop) -> BTreeSet<&str> {
    l.properties.iter().map(|(p, _)| p.as_str()).collect()
}

/// The phase offset of `member` against `seed` (`member(t) = seed(t - offset)`), or `None`
/// when no offset makes them match.
fn phase_offset(
    seed: &FixtureLoop,
    member: &FixtureLoop,
    series: &[&Series],
    scale: f64,
) -> Option<f64> {
    let (w0, w1) = (seed.start.max(member.start), seed.end.min(member.end));
    let period = seed.period;
    if w1 - w0 < 2.0 * period {
        return None;
    }
    let mut pairs: Vec<(&Series, &Series, f64)> = Vec::new();
    for (p, si) in &seed.properties {
        let (_, mi) = member.properties.iter().find(|(q, _)| q == p)?;
        pairs.push((
            series[*si],
            series[*mi],
            tolerance_for(&series[*si].address, scale),
        ));
    }
    let lag_max = (period / DT).round() as usize;
    let i0 = (w0 / DT).ceil() as usize + lag_max;
    let i1 = (w1 / DT).floor() as usize;
    if i1 <= i0 {
        return None;
    }
    let grids: Vec<(Vec<f64>, Vec<f64>)> = pairs
        .iter()
        .map(|(s, m, _)| {
            let n = i1 + 1;
            (s.grid(DT, n), m.grid(DT, n))
        })
        .collect();
    let mut best = (0.0f64, f64::INFINITY, 0usize);
    for lag in 0..lag_max {
        let mut hit = 0usize;
        let mut total = 0usize;
        let mut sum = 0.0;
        for (gi, (sg, mg)) in grids.iter().enumerate() {
            let dim = pairs[gi].0.dim;
            let tol = pairs[gi].2;
            for i in i0..=i1 {
                let mut d = 0.0f64;
                for c in 0..dim {
                    d = d.max((mg[i * dim + c] - sg[(i - lag) * dim + c]).abs());
                }
                total += 1;
                sum += d / tol;
                if d <= 2.0 * tol {
                    hit += 1;
                }
            }
        }
        let cov = hit as f64 / total as f64;
        let mean = sum / total as f64;
        if cov > best.0 + 1e-9 || ((cov - best.0).abs() <= 1e-9 && mean < best.1) {
            best = (cov, mean, lag);
        }
    }
    if best.0 < 0.9 {
        return None;
    }
    // refine on the raw samples, +-50 ms around the coarse lag, 2 ms steps
    let coarse = best.2 as f64 * DT;
    let mut refined = (f64::INFINITY, coarse);
    let mut a = [0.0; 8];
    let mut b = [0.0; 8];
    let mut o = coarse - 0.05;
    while o <= coarse + 0.05 + 1e-9 {
        let mut sum = 0.0;
        let mut count = 0usize;
        let mut t = w0 + period + 0.1;
        while t <= w1 {
            for (s, m, tol) in &pairs {
                let (ab, bb) = (&mut a[..s.dim], &mut b[..s.dim]);
                m.at(t, ab);
                s.at(t - o, bb);
                sum += ab
                    .iter()
                    .zip(bb.iter())
                    .map(|(x, y)| (x - y).abs())
                    .fold(0.0, f64::max)
                    / tol;
                count += 1;
            }
            t += DT;
        }
        let mean = sum / count.max(1) as f64;
        if mean < refined.0 {
            refined = (mean, o);
        }
        o += 0.002;
    }
    Some(refined.1.rem_euclid(period))
}

// -------------------------------------------------------------------------- building a cycle

struct Cluster<'a> {
    loops: Vec<(&'a FixtureLoop, f64)>,
}

/// The effect for these members over `[start, end)`, or `None` when the cycle cannot be built.
fn build_effect(
    cluster: &Cluster,
    series: &[&Series],
    start: f64,
    end: f64,
    period: f64,
    scale: f64,
) -> Option<(LightingEffect, usize)> {
    let seed = cluster.loops[0].0;
    let steps_n = ((period / TEMPLATE_STEP).round() as usize).max(8);
    let mut cycle: Vec<EffectStep> = Vec::new();
    for (property, _) in &seed.properties {
        let tol = {
            let (_, si) = seed.properties.iter().find(|(p, _)| p == property)?;
            tolerance_for(&series[*si].address, scale)
        };
        let dim = {
            let (_, si) = seed.properties.iter().find(|(p, _)| p == property)?;
            series[*si].dim
        };
        let mut taus = Vec::with_capacity(steps_n + 1);
        let mut values: Vec<f64> = Vec::with_capacity((steps_n + 1) * dim);
        let mut buf = vec![0.0; dim];
        for k in 0..steps_n {
            let tau = period * k as f64 / steps_n as f64;
            let mut samples: Vec<Vec<f64>> = vec![Vec::new(); dim];
            for (l, offset) in &cluster.loops {
                let (_, si) = l.properties.iter().find(|(p, _)| p == property)?;
                let s = series[*si];
                let c_min = ((-(offset + tau)) / period).ceil() as i64;
                let c_max = ((end - start - offset - tau) / period).floor() as i64;
                for c in c_min..=c_max {
                    let t = start + offset + c as f64 * period + tau;
                    if t < start - 1e-9 || t > end + 1e-9 {
                        continue;
                    }
                    s.at(t, &mut buf);
                    for (ch, v) in buf.iter().enumerate() {
                        samples[ch].push(*v);
                    }
                }
            }
            taus.push(tau);
            for ch in samples.iter_mut() {
                if ch.is_empty() {
                    return None;
                }
                ch.sort_by(f64::total_cmp);
                values.push(ch[ch.len() / 2]);
            }
        }
        // close the loop: the cycle ends where it began
        taus.push(period);
        let first: Vec<f64> = values[0..dim].to_vec();
        values.extend(first);
        for (i, j) in polyline(&taus, &values, dim, 0.5 * tol, MAX_RAMP_SECONDS) {
            let from = &values[i * dim..(i + 1) * dim];
            let to = &values[j * dim..(j + 1) * dim];
            if from.iter().zip(to).all(|(a, b)| (a - b).abs() < 1e-9) {
                continue;
            }
            cycle.push(EffectStep {
                at_seconds: round3(taus[i]),
                property: property.clone(),
                value: to.iter().map(|v| round4(*v)).collect(),
                fade_seconds: round3(taus[j] - taus[i]),
            });
        }
    }
    if cycle.is_empty() || cycle.len() > MAX_EFFECT_CYCLE_STEPS {
        return None;
    }
    cycle.sort_by(|a, b| {
        a.at_seconds
            .total_cmp(&b.at_seconds)
            .then_with(|| a.property.cmp(&b.property))
    });
    let steps = cycle.len();
    let n = cluster.loops.len();
    let effect = LightingEffect {
        id: Uuid::new_v4(),
        label: format!(
            "Loop - {n} fixture{}, {period} s",
            if n == 1 { "" } else { "s" }
        ),
        start_seconds: round3(start),
        end_seconds: round3(end),
        period_seconds: round3(period),
        members: cluster
            .loops
            .iter()
            .map(|(l, o)| EffectMember {
                fixture: l.fixture.clone(),
                offset_seconds: round3(*o),
            })
            .collect(),
        cycle,
        source: OscCueSource::Captured,
        edited: false,
    };
    Some((effect, steps))
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn round4(x: f64) -> f64 {
    let r = (x * 10_000.0).round() / 10_000.0;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

/// How scattered the members' samples are around the cycle at this period: for each phase the
/// median absolute deviation over all members and cycles, in tolerances, averaged. The right
/// period lines the cycles up and gives the smallest value.
fn dispersion(
    cluster: &Cluster,
    series: &[&Series],
    start: f64,
    end: f64,
    period: f64,
    scale: f64,
) -> f64 {
    let seed = cluster.loops[0].0;
    let steps_n = ((period / 0.05).round() as usize).max(8);
    // a handful of members is enough to find the period
    let members: Vec<&(&FixtureLoop, f64)> = cluster.loops.iter().take(4).collect();
    let mut total = 0.0;
    let mut count = 0usize;
    for (property, _) in &seed.properties {
        for k in 0..steps_n {
            let tau = period * k as f64 / steps_n as f64;
            let mut samples: Vec<f64> = Vec::new();
            let mut tol = 1.0;
            for (l, offset) in &members {
                let Some((_, si)) = l.properties.iter().find(|(p, _)| p == property) else {
                    continue;
                };
                let s = series[*si];
                tol = tolerance_for(&s.address, scale);
                let mut buf = vec![0.0; s.dim];
                let c_min = ((-(offset + tau)) / period).ceil() as i64;
                let c_max = ((end - start - offset - tau) / period).floor() as i64;
                for c in c_min..=c_max {
                    let t = start + offset + c as f64 * period + tau;
                    s.at(t, &mut buf);
                    samples.push(buf[0]);
                }
            }
            if samples.len() < 3 {
                continue;
            }
            samples.sort_by(f64::total_cmp);
            let median = samples[samples.len() / 2];
            let mut dev: Vec<f64> = samples.iter().map(|v| (v - median).abs()).collect();
            dev.sort_by(f64::total_cmp);
            total += dev[dev.len() / 2] / tol;
            count += 1;
        }
    }
    if count == 0 {
        f64::INFINITY
    } else {
        total / count as f64
    }
}

/// The period (to the millisecond) that lines the members' cycles up best, searched around
/// `period`.
fn refine_cluster_period(
    cluster: &Cluster,
    series: &[&Series],
    start: f64,
    end: f64,
    period: f64,
    scale: f64,
) -> f64 {
    let mut best = (
        dispersion(cluster, series, start, end, period, scale),
        period,
    );
    for j in -30..=30 {
        let candidate = period + j as f64 * 0.002;
        if j == 0 || candidate < MIN_PERIOD {
            continue;
        }
        let d = dispersion(cluster, series, start, end, candidate, scale);
        if d < best.0 - 1e-9 {
            best = (d, candidate);
        }
    }
    (best.1 * 1000.0).round() / 1000.0
}

/// How well an effect reproduces the recording.
struct Eval {
    /// Per member: the share of the window where every property is within twice the tolerance.
    member_share: Vec<f64>,
    /// Per [`CHECK_STEP`] from the effect's start: the share of members that are within it.
    good: Vec<f64>,
}

const CHECK_STEP: f64 = 0.025;
/// A moment counts as reproduced when this share of the members is within twice the tolerance.
const GOOD_MOMENT: f64 = 0.8;

/// Compare the effect with the recorded samples of every member over its window, using the
/// replay's evaluator.
fn evaluate(effect: &LightingEffect, cluster: &Cluster, series: &[&Series], scale: f64) -> Eval {
    let timeline = Timeline::new(&[], std::slice::from_ref(effect));
    let (start, end) = (effect.start_seconds, effect.end_seconds);
    let steps = (((end - start) / CHECK_STEP).floor() as usize).max(1);
    let mut good = vec![0.0; steps];
    let mut member_share = Vec::new();
    for (l, _) in &cluster.loops {
        let mut ok_count = 0usize;
        let mut bufs: Vec<(String, &Series, f64, Vec<f64>)> = l
            .properties
            .iter()
            .map(|(property, si)| {
                let s = series[*si];
                (
                    format!("/fixture/{}/overrides/{}", l.fixture, property),
                    s,
                    tolerance_for(&s.address, scale),
                    vec![0.0; s.dim],
                )
            })
            .collect();
        for (i, g) in good.iter_mut().enumerate() {
            let t = start + i as f64 * CHECK_STEP;
            let mut ok = true;
            for (address, s, tol, buf) in bufs.iter_mut() {
                s.at(t, buf);
                let within = timeline.value_at(address, t).is_some_and(|v| {
                    v.iter().zip(buf.iter()).all(|(a, b)| match a {
                        FixtureArg::Float(a) => (a - b).abs() <= 2.0 * *tol,
                        _ => false,
                    })
                });
                ok &= within;
            }
            if ok {
                ok_count += 1;
                *g += 1.0;
            }
        }
        member_share.push(ok_count as f64 / steps as f64);
    }
    let n = cluster.loops.len().max(1) as f64;
    for g in good.iter_mut() {
        *g /= n;
    }
    Eval { member_share, good }
}

/// The longest stretch of an evaluation where at least 95 % of the moments are reproduced,
/// as `(start, end)` seconds, when it holds at least three cycles.
fn trim(eval: &Eval, start: f64, period: f64) -> Option<(f64, f64)> {
    let score: Vec<i32> = eval
        .good
        .iter()
        .map(|g| if *g >= GOOD_MOMENT { 1 } else { -49 })
        .collect();
    let (a, b) = longest_window(&score)?;
    let (t0, t1) = (start + a as f64 * CHECK_STEP, start + b as f64 * CHECK_STEP);
    (t1 - t0 >= MIN_CYCLES * period).then_some((t0, t1))
}

/// Ramp segments the members need as plain rows inside `[start, end]`.
fn plain_segments(
    cluster: &Cluster,
    series: &[&Series],
    start: f64,
    end: f64,
    scale: f64,
) -> usize {
    let mut total = 0usize;
    for (l, _) in &cluster.loops {
        for (_, si) in &l.properties {
            let s = series[*si];
            let ts: Vec<f64> = (0..s.len())
                .map(|i| s.ts[i])
                .filter(|t| *t >= start && *t <= end)
                .collect();
            if ts.len() < 2 {
                continue;
            }
            let first = s.ts.partition_point(|t| *t < start);
            let vs = &s.vs[first * s.dim..(first + ts.len()) * s.dim];
            total += polyline(
                &ts,
                vs,
                s.dim,
                tolerance_for(&s.address, scale),
                MAX_RAMP_SECONDS,
            )
            .len();
        }
    }
    total
}

// ------------------------------------------------------------------------------------- driver

/// Find the effects of a recording. `series` are the numeric addresses, `end` the last sample
/// time, `scale` the tolerance multiplier.
pub fn detect(series: &[&Series], end: f64, scale: f64) -> Detected {
    let mut out = Detected::default();
    if end < MIN_PERIOD * MIN_CYCLES || series.is_empty() {
        return out;
    }
    // stage 1: loops per address, in parallel
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(8);
    let results: Vec<AddressSearch> = {
        let chunk = series.len().div_ceil(threads).max(1);
        let mut all: Vec<Option<AddressSearch>> = (0..series.len()).map(|_| None).collect();
        std::thread::scope(|scope| {
            let handles: Vec<_> = series
                .chunks(chunk)
                .enumerate()
                .map(|(c, part)| {
                    scope.spawn(move || {
                        part.iter()
                            .enumerate()
                            .map(|(i, s)| {
                                (c * chunk + i, search_address(s, c * chunk + i, end, scale))
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            for h in handles {
                for (i, r) in h.join().unwrap_or_default() {
                    all[i] = Some(r);
                }
            }
        });
        all.into_iter()
            .map(|r| {
                r.unwrap_or(AddressSearch {
                    candidates: vec![],
                    reason: Some("not searched".into()),
                })
            })
            .collect()
    };
    let mut candidates: Vec<Candidate> = Vec::new();
    for (i, r) in results.into_iter().enumerate() {
        if let Some(reason) = r.reason {
            out.reasons.insert(series[i].address.clone(), reason);
        }
        candidates.extend(r.candidates);
    }
    let mut loops = join_fixture_loops(&candidates, series);
    // best first: longest window times most properties
    let value = |l: &FixtureLoop| (l.end - l.start) * l.properties.len() as f64;
    let mut order: Vec<usize> = (0..loops.len()).collect();
    order.sort_by(|&a, &b| value(&loops[b]).total_cmp(&value(&loops[a])));
    let mut used = vec![false; loops.len()];
    // loops that left a cluster, or are windows left over: tried without joining others
    let mut alone = vec![false; loops.len()];
    let mut queue: std::collections::VecDeque<usize> = order.into_iter().collect();
    let mut budget = 40 * loops.len() + 200;
    while let Some(seed_i) = queue.pop_front() {
        if used[seed_i] {
            continue;
        }
        if budget == 0 {
            break;
        }
        budget -= 1;
        used[seed_i] = true;
        let seed = loops[seed_i].clone();
        let mut members: Vec<(usize, f64)> = vec![(seed_i, 0.0)];
        if !alone[seed_i] {
            for j in 0..loops.len() {
                let m = &loops[j];
                if used[j]
                    || alone[j]
                    || m.fixture == seed.fixture
                    || property_set(m) != property_set(&seed)
                    || (m.period - seed.period).abs() > PERIOD_SLACK * seed.period
                    || overlap((seed.start, seed.end), (m.start, m.end)) < MIN_CYCLES * seed.period
                {
                    continue;
                }
                if let Some(o) = phase_offset(&seed, m, series, scale) {
                    members.push((j, o));
                }
            }
        }
        for &(j, _) in &members {
            used[j] = true;
        }
        // the window all members share (a member that would cut it short leaves)
        let (mut start, mut end_t) = (seed.start, seed.end);
        let mut kept: Vec<(usize, f64)> = Vec::new();
        for &(j, o) in &members {
            let l = &loops[j];
            let (s2, e2) = (start.max(l.start), end_t.min(l.end));
            if j == seed_i
                || (e2 - s2 >= 0.7 * (end_t - start) && e2 - s2 >= MIN_CYCLES * seed.period)
            {
                start = s2;
                end_t = e2;
                kept.push((j, o));
            } else {
                used[j] = false;
                alone[j] = true;
                queue.push_back(j);
            }
        }
        members = kept;
        members.truncate(MAX_EFFECT_MEMBERS);

        let mut period = seed.period;
        for _attempt in 0..8 {
            if members.is_empty() || end_t - start < MIN_CYCLES * seed.period {
                break;
            }
            let cluster = Cluster {
                loops: members.iter().map(|&(j, o)| (&loops[j], o)).collect(),
            };
            period = refine_cluster_period(&cluster, series, start, end_t, period, scale);
            let Some((effect, steps)) = build_effect(&cluster, series, start, end_t, period, scale)
            else {
                reject(&mut out, &cluster, series, "the cycle could not be built (a member has no samples in the window, or the cycle is too long)");
                break;
            };
            let eval = evaluate(&effect, &cluster, series, scale);
            let failing: Vec<usize> = eval
                .member_share
                .iter()
                .enumerate()
                .filter(|(_, s)| **s < ACCEPT_SHARE)
                .map(|(i, _)| i)
                .collect();
            if failing.is_empty() {
                let plain = plain_segments(&cluster, series, start, end_t, scale);
                if steps as f64 > MAX_STEP_SHARE * plain as f64 {
                    reject(
                        &mut out,
                        &cluster,
                        series,
                        &format!("a loop of {:.2} s fits, but its cycle needs {steps} steps against {plain} plain ramp segments (not worth it)", period),
                    );
                } else if out.effects.len() >= MAX_EFFECTS_PER_SONG {
                    reject(&mut out, &cluster, series, "too many effects in the song");
                } else {
                    let worst = eval.member_share.iter().cloned().fold(1.0, f64::min);
                    let mut properties: Vec<String> =
                        seed.properties.iter().map(|(p, _)| p.clone()).collect();
                    properties.sort();
                    out.infos.push(EffectInfo {
                        label: effect.label.clone(),
                        period_seconds: effect.period_seconds,
                        start_seconds: effect.start_seconds,
                        end_seconds: effect.end_seconds,
                        members: effect
                            .members
                            .iter()
                            .map(|m| (m.fixture.clone(), m.offset_seconds))
                            .collect(),
                        properties,
                        cycle_steps: steps,
                        replaces_segments: plain,
                        worst_share: worst,
                    });
                    let n = out.effects.len() + 1;
                    for (l, _) in &cluster.loops {
                        for (_, si) in &l.properties {
                            let address = series[*si].address.clone();
                            out.taken
                                .entry(address.clone())
                                .or_default()
                                .push((effect.start_seconds, effect.end_seconds));
                            out.reasons.insert(
                                address,
                                format!(
                                    "partly in effect {n} (period {:.3} s); outside its window the series stays plain ramps",
                                    effect.period_seconds
                                ),
                            );
                        }
                    }
                    out.effects.push(effect);
                }
                // what is left of the members' windows on either side may hold another run
                let leftovers: Vec<(usize, f64, f64)> = cluster
                    .loops
                    .iter()
                    .zip(&members)
                    .flat_map(|((l, _), (j, _))| [(*j, l.start, start), (*j, end_t, l.end)])
                    .filter(|(_, a, b)| b - a >= (MIN_CYCLES + 0.5) * period)
                    .collect();
                for (j, a, b) in leftovers {
                    let mut l = loops[j].clone();
                    l.start = a;
                    l.end = b;
                    loops.push(l);
                    used.push(false);
                    alone.push(false);
                    queue.push_back(loops.len() - 1);
                }
                break;
            }
            // not good enough: first narrow the window to its best stretch
            if let Some((a, b)) = trim(&eval, start, period) {
                if a - start > 0.25 || end_t - b > 0.25 {
                    // (the windows left outside are tried again once this effect is settled)
                    start = a;
                    end_t = b;
                    continue;
                }
            }
            // the window cannot be narrowed further: failing members leave, to try alone
            if failing.len() == members.len() {
                let worst = eval.member_share.iter().cloned().fold(1.0, f64::min);
                reject(
                    &mut out,
                    &cluster,
                    series,
                    &format!(
                        "a loop of {:.2} s was found but only {:.0} % of its best stretch is within 2x tolerance, 95 % needed: plain ramps",
                        period,
                        worst * 100.0
                    ),
                );
                break;
            }
            for &f in failing.iter().rev() {
                let (j, _) = members.remove(f);
                used[j] = false;
                alone[j] = true;
                queue.push_back(j);
            }
            // the first remaining member becomes the phase origin
            let base = members[0].1;
            for m in members.iter_mut() {
                m.1 = (m.1 - base).rem_euclid(period);
            }
        }
    }
    let mut pairs: Vec<(LightingEffect, EffectInfo)> =
        out.effects.drain(..).zip(out.infos.drain(..)).collect();
    pairs.sort_by(|a, b| a.0.start_seconds.total_cmp(&b.0.start_seconds));
    for (e, i) in pairs {
        out.effects.push(e);
        out.infos.push(i);
    }
    let _ = property_class;
    out
}

fn reject(out: &mut Detected, cluster: &Cluster, series: &[&Series], why: &str) {
    for (l, _) in &cluster.loops {
        for (_, si) in &l.properties {
            out.reasons
                .entry(series[*si].address.clone())
                .or_insert_with(|| why.to_string());
        }
    }
}
