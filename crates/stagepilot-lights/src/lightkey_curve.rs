//! Evaluating a song's timeline at a position: the value of every fixture address at time `t`,
//! as a pure function of the cue map's rows and effects. Pure: no I/O, no clock.
//!
//! This is the one definition of "what the song says at `t`". The capture's reconstruction
//! report uses it, and the replay engine builds on it (seek and resume just evaluate at the new
//! position; nothing is accumulated).
//!
//! # Rows
//!
//! Only rows whose messages are `/fixture/<name>/overrides/<property>` addresses take part.
//! Rows are applied in time order; rows at the same time apply instant ones (`fade_seconds`
//! 0) first, then ramps, then in the order the cue map lists them. A message reaches its value
//! over `fade_seconds` from the address's value *just before the row*:
//!
//! * a number (or several, a colour) ramps linearly from the previous value;
//! * an option, text, a bool, a different number of arguments, or an address with no earlier
//!   value steps at the row's time;
//! * a row that starts while an earlier ramp of the same address is still running leaves the
//!   earlier ramp where it is (no jump) and ramps on from there.
//!
//! Before an address's first row the address has no value (`None`).
//!
//! # Effects
//!
//! Inside `[start_seconds, end_seconds)` an effect drives the properties named in its cycle
//! for each member fixture, and overrides the rows for those addresses. A member sees the
//! cycle at phase `(t - start - offset) mod period`. The first step of a property starts from
//! the last step's value, so the loop closes. After the window the rows apply again.
//!
//! # Playback at the seams
//!
//! [`Timeline::value_at`] is the song as captured. A replay uses [`Timeline::playback_at`]: the
//! same, except that the first and the last [`SEAM_BLEND_SECONDS`] of an effect move linearly
//! between the value the ordinary rows give there (at the time, at the start; at the end of the
//! window, at the end) and the effect's value, so an effect
//! starts from, and returns to, the rows without a jump (the window of a detected effect is
//! only approximate, so the two can differ at its edges).

use crate::lightkey_fixtures::FixtureArg;
use stagepilot_core::config::{LightingEffect, OscArgument, OscLightingCue};
use std::collections::BTreeMap;
use std::sync::Arc;

/// How long an effect takes to take over from the rows at its start, and to give back to them at
/// its end, in playback (seconds).
pub const SEAM_BLEND_SECONDS: f64 = 0.5;

/// One value of an address: numbers (any count) or something that never interpolates.
#[derive(Debug, Clone, PartialEq)]
enum Val {
    Num(Vec<f64>),
    Other(Vec<FixtureArg>),
}

impl Val {
    fn from_args(args: &[OscArgument]) -> Self {
        if !args.is_empty()
            && args
                .iter()
                .all(|a| matches!(a, OscArgument::Float(f) if f.is_finite()))
        {
            Val::Num(
                args.iter()
                    .map(|a| match a {
                        OscArgument::Float(f) => *f,
                        _ => 0.0,
                    })
                    .collect(),
            )
        } else {
            Val::Other(
                args.iter()
                    .map(|a| match a {
                        OscArgument::Int(v) => FixtureArg::Int(*v),
                        OscArgument::Float(v) => FixtureArg::Float(*v),
                        OscArgument::String(v) => FixtureArg::Text(v.clone()),
                        OscArgument::Bool(v) => FixtureArg::Bool(*v),
                    })
                    .collect(),
            )
        }
    }

    fn into_args(self) -> Vec<FixtureArg> {
        match self {
            Val::Num(v) => v.into_iter().map(FixtureArg::Float).collect(),
            Val::Other(v) => v,
        }
    }
}

/// `from` at `t0`, `to` at `t1`, linear between; steps at `t0` when the values are not both
/// numbers of the same count.
#[derive(Debug, Clone)]
struct Segment {
    t0: f64,
    t1: f64,
    from: Val,
    to: Val,
}

impl Segment {
    fn value(&self, t: f64) -> Val {
        if t >= self.t1 || self.t1 <= self.t0 {
            return self.to.clone();
        }
        match (&self.from, &self.to) {
            (Val::Num(a), Val::Num(b)) if a.len() == b.len() => {
                let f = ((t - self.t0) / (self.t1 - self.t0)).clamp(0.0, 1.0);
                Val::Num(a.iter().zip(b).map(|(a, b)| a + (b - a) * f).collect())
            }
            _ => self.to.clone(),
        }
    }
}

/// The ramps of one address, in time order.
#[derive(Debug, Clone, Default)]
struct AddressCurve {
    segments: Vec<Segment>,
}

impl AddressCurve {
    fn value_at(&self, t: f64) -> Option<Val> {
        let i = self.segments.partition_point(|s| s.t0 <= t);
        if i == 0 {
            return None;
        }
        Some(self.segments[i - 1].value(t))
    }

    /// The value of the first row, whatever the time.
    fn first_value(&self) -> Option<Val> {
        self.segments.first().map(|s| s.to.clone())
    }

    fn push(&mut self, at: f64, fade: f64, to: Val) {
        let from = match self.segments.last() {
            // An earlier ramp still running when this row starts is cut short where it is.
            Some(last) => last.value(at),
            None => to.clone(),
        };
        self.segments.push(Segment {
            t0: at,
            t1: at + fade.max(0.0),
            from,
            to,
        });
    }
}

#[derive(Debug, Clone)]
struct EffectStepN {
    at: f64,
    fade: f64,
    value: Vec<f64>,
}

/// One effect's cycle for one property of one fixture.
#[derive(Debug, Clone)]
struct EffectCurve {
    start: f64,
    end: f64,
    period: f64,
    offset: f64,
    /// Shared by every member of the effect (an effect of 256 fixtures and 4,096 steps would
    /// otherwise be a million copies).
    steps: Arc<Vec<EffectStepN>>,
    /// The cycle times of the steps without a fade, sorted: the instants a replay must not miss.
    instants: Arc<Vec<f64>>,
}

impl EffectCurve {
    /// The next instant step (a cycle step without a fade) after `at`, inside the window.
    fn next_step_after(&self, at: f64) -> Option<f64> {
        let origin = self.start + self.offset;
        let cycle = ((at - origin) / self.period).floor();
        let mut best: Option<f64> = None;
        for k in [cycle, cycle + 1.0] {
            // the first instant of this cycle after `at` (the list is sorted: a search, not a
            // scan of every step on every tick)
            let base = origin + k * self.period;
            let i = self.instants.partition_point(|s| base + s <= at);
            if let Some(s) = self.instants.get(i) {
                let t = base + s;
                if t > at && t < self.end && best.is_none_or(|b| t < b) {
                    best = Some(t);
                }
            }
        }
        best
    }

    fn value(&self, t: f64) -> Vec<f64> {
        let phase = (t - self.start - self.offset).rem_euclid(self.period);
        let n = self.steps.len();
        let idx = self.steps.partition_point(|s| s.at <= phase);
        let (k, local) = if idx == 0 {
            // Before the first step: the last step of the previous loop is still settling.
            (n - 1, phase + self.period - self.steps[n - 1].at)
        } else {
            (idx - 1, phase - self.steps[idx - 1].at)
        };
        let step = &self.steps[k];
        let from = &self.steps[(k + n - 1) % n].value;
        if step.fade > 0.0 && local < step.fade && from.len() == step.value.len() {
            let f = local / step.fade;
            from.iter()
                .zip(&step.value)
                .map(|(a, b)| a + (b - a) * f)
                .collect()
        } else {
            step.value.clone()
        }
    }
}

/// A song's rows and effects prepared for evaluation. Build once, evaluate many times.
#[derive(Debug, Clone, Default)]
pub struct Timeline {
    rows: BTreeMap<String, AddressCurve>,
    effects: BTreeMap<String, Vec<EffectCurve>>,
    /// Sorted times at which some row starts or finishes a ramp, or an effect starts or ends.
    boundaries: Vec<f64>,
}

fn is_fixture_address(address: &str) -> bool {
    crate::lightkey_fixtures::fixture_of(address).is_some()
}

impl Timeline {
    /// Prepare `rows` and `effects` (usually a song cue map's `osc_cues` and `effects`). Rows
    /// that are not fixture look rows are ignored. `rows` need not be sorted.
    pub fn new(rows: &[OscLightingCue], effects: &[LightingEffect]) -> Self {
        let mut order: Vec<(usize, &OscLightingCue)> = rows.iter().enumerate().collect();
        order.sort_by(|a, b| {
            a.1.at_seconds
                .total_cmp(&b.1.at_seconds)
                .then_with(|| a.1.fade_seconds.total_cmp(&b.1.fade_seconds))
                .then_with(|| a.0.cmp(&b.0))
        });
        let mut timeline = Timeline::default();
        for (_, row) in order {
            if row.messages.is_empty() {
                continue;
            }
            for message in &row.messages {
                if !is_fixture_address(&message.address) {
                    continue;
                }
                timeline
                    .rows
                    .entry(message.address.clone())
                    .or_default()
                    .push(
                        row.at_seconds,
                        row.fade_seconds,
                        Val::from_args(&message.args),
                    );
            }
        }
        for effect in effects {
            if effect.period_seconds <= 0.0
                || !effect.period_seconds.is_finite()
                || effect.cycle.is_empty()
            {
                continue;
            }
            let mut by_property: BTreeMap<&str, Vec<EffectStepN>> = BTreeMap::new();
            for step in &effect.cycle {
                by_property
                    .entry(step.property.as_str())
                    .or_default()
                    .push(EffectStepN {
                        at: step.at_seconds,
                        fade: step.fade_seconds,
                        value: step.value.clone(),
                    });
            }
            for steps in by_property.values_mut() {
                steps.sort_by(|a, b| a.at.total_cmp(&b.at));
            }
            let shared: Vec<(&str, Arc<Vec<EffectStepN>>, Arc<Vec<f64>>)> = by_property
                .iter()
                .map(|(property, steps)| {
                    let instants = steps
                        .iter()
                        .filter(|s| s.fade <= 0.0)
                        .map(|s| s.at)
                        .collect();
                    (*property, Arc::new(steps.clone()), Arc::new(instants))
                })
                .collect();
            for member in &effect.members {
                for (property, steps, instants) in &shared {
                    timeline
                        .effects
                        .entry(format!(
                            "/fixture/{}/overrides/{}",
                            member.fixture, property
                        ))
                        .or_default()
                        .push(EffectCurve {
                            start: effect.start_seconds,
                            end: effect.end_seconds,
                            period: effect.period_seconds,
                            offset: member.offset_seconds,
                            steps: steps.clone(),
                            instants: instants.clone(),
                        });
                }
            }
        }
        for list in timeline.effects.values_mut() {
            list.sort_by(|a, b| a.start.total_cmp(&b.start));
        }
        let mut boundaries: Vec<f64> = Vec::new();
        for curve in timeline.rows.values() {
            for s in &curve.segments {
                boundaries.push(s.t0);
                if s.t1 > s.t0 {
                    boundaries.push(s.t1);
                }
            }
        }
        for list in timeline.effects.values() {
            for e in list {
                boundaries.push(e.start);
                boundaries.push(e.end);
            }
        }
        boundaries.retain(|b| b.is_finite());
        boundaries.sort_by(f64::total_cmp);
        boundaries.dedup();
        timeline.boundaries = boundaries;
        timeline
    }

    /// The next time after `at` at which something discrete happens: a row starts or finishes
    /// its ramp, an effect starts or ends, or an instant step of an effect's cycle happens. A
    /// scheduler that wakes at these times never delays a step. `None` when nothing is left.
    pub fn next_boundary(&self, at: f64) -> Option<f64> {
        let i = self.boundaries.partition_point(|b| *b <= at);
        let mut next = self.boundaries.get(i).copied();
        for list in self.effects.values() {
            for effect in list.iter().filter(|e| e.start <= at && at < e.end) {
                if let Some(t) = effect.next_step_after(at) {
                    next = Some(next.map_or(t, |n| n.min(t)));
                }
            }
        }
        next
    }

    /// Every address that has a row or an effect.
    pub fn addresses(&self) -> Vec<&str> {
        let mut all: Vec<&str> = self
            .rows
            .keys()
            .chain(self.effects.keys())
            .map(String::as_str)
            .collect();
        all.sort_unstable();
        all.dedup();
        all
    }

    /// The effect (if any) driving `address` at `at`; the latest-starting one wins.
    fn effect_at(&self, address: &str, at: f64) -> Option<&EffectCurve> {
        self.effects
            .get(address)?
            .iter()
            .rev()
            .find(|e| e.start <= at && at < e.end)
    }

    /// Whether an instant step of the effect driving `address` at `upto` happened after `after`
    /// (up to and including `upto`).
    pub fn instant_step_in(&self, address: &str, after: f64, upto: f64) -> bool {
        self.effect_at(address, upto)
            .and_then(|e| e.next_step_after(after))
            .is_some_and(|t| t <= upto)
    }

    /// Whether an effect drives `address` at `at`.
    pub fn in_effect(&self, address: &str, at: f64) -> bool {
        self.effect_at(address, at).is_some()
    }

    /// Value of `address` at `at` seconds, or `None` before its first row.
    pub fn value_at(&self, address: &str, at: f64) -> Option<Vec<FixtureArg>> {
        if let Some(effect) = self.effect_at(address, at) {
            return Some(
                effect
                    .value(at)
                    .into_iter()
                    .map(FixtureArg::Float)
                    .collect(),
            );
        }
        self.rows.get(address)?.value_at(at).map(Val::into_args)
    }

    /// What a replay plays at `at`: [`Self::value_at`], with the seams of effects blended (see the
    /// module docs). A pure function of `at`, like `value_at`.
    pub fn playback_at(&self, address: &str, at: f64) -> Option<Vec<FixtureArg>> {
        let Some(effect) = self.effect_at(address, at) else {
            return self.rows.get(address)?.value_at(at).map(Val::into_args);
        };
        let from_start = (at - effect.start) / SEAM_BLEND_SECONDS;
        let to_end = (effect.end - at) / SEAM_BLEND_SECONDS;
        let weight = from_start.min(to_end).clamp(0.0, 1.0);
        let inside = effect.value(at);
        // Near the start the rows' value is the one they hold now. Near the end it is the one
        // they have when the effect is over, which is their first value if they only begin then.
        let curve = self.rows.get(address);
        let rows = if from_start <= to_end {
            curve.and_then(|c| c.value_at(at))
        } else {
            curve.and_then(|c| c.value_at(effect.end).or_else(|| c.first_value()))
        };
        let blended = match rows {
            Some(Val::Num(row)) if weight < 1.0 && row.len() == inside.len() => row
                .iter()
                .zip(&inside)
                .map(|(r, e)| r + (e - r) * weight)
                .collect(),
            _ => inside,
        };
        Some(blended.into_iter().map(FixtureArg::Float).collect())
    }

    /// Whether `playback_at` of `address` is inside a seam blend at `at`.
    pub fn in_seam(&self, address: &str, at: f64) -> bool {
        self.effect_at(address, at).is_some_and(|e| {
            (at - e.start < SEAM_BLEND_SECONDS || e.end - at < SEAM_BLEND_SECONDS)
                && self
                    .rows
                    .get(address)
                    .is_some_and(|c| c.value_at(at).is_some())
        })
    }

    /// Value of every address that has one at `at`.
    pub fn state_at(&self, at: f64) -> BTreeMap<String, Vec<FixtureArg>> {
        self.addresses()
            .into_iter()
            .filter_map(|a| self.value_at(a, at).map(|v| (a.to_string(), v)))
            .collect()
    }

    /// Whether `address` is between two values at `at` (a row ramp is running, or an effect
    /// drives it). A tick that finds nothing moving can sleep.
    pub fn is_moving(&self, address: &str, at: f64) -> bool {
        if self.in_effect(address, at) {
            return true;
        }
        self.rows.get(address).is_some_and(|curve| {
            let i = curve.segments.partition_point(|s| s.t0 <= at);
            i > 0
                && at < curve.segments[i - 1].t1
                && curve.segments[i - 1].from != curve.segments[i - 1].to
        })
    }
}

/// Value of `address` at `at` for these rows and effects. Builds a [`Timeline`] each call; keep
/// a `Timeline` when evaluating many times.
pub fn value_at(
    rows: &[OscLightingCue],
    effects: &[LightingEffect],
    address: &str,
    at: f64,
) -> Option<Vec<FixtureArg>> {
    Timeline::new(rows, effects).value_at(address, at)
}
