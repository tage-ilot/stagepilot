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

use crate::lightkey_fixtures::FixtureArg;
use stagepilot_core::config::{LightingEffect, OscArgument, OscLightingCue};
use std::collections::BTreeMap;

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
    steps: Vec<EffectStepN>,
}

impl EffectCurve {
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
            for member in &effect.members {
                for (property, steps) in &by_property {
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
                        });
                }
            }
        }
        for list in timeline.effects.values_mut() {
            list.sort_by(|a, b| a.start.total_cmp(&b.start));
        }
        timeline
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
