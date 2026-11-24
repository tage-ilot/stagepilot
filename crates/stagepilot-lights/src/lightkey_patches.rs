//! Patches of a loop: the small corrections that let a chase which repeats MOSTLY cleanly be
//! stored as one loop plus a few changes instead of thousands of rows. Pure: no I/O, no clock.
//!
//! Given a loop (its cycle and members, no patches yet) and what every member really did, each
//! member's property is compared with what the loop plays on the 25 ms grid. Wherever the two
//! differ by more than [`PATCH_TRIGGER`] tolerances for more than two frames the stretch becomes a
//! patch:
//!
//! * runs of such frames closer together than [`MERGE_GAP_FRAMES`] are one stretch (a patch edge
//!   is not free: it costs a step or two of its own),
//! * the patch spans the stretch plus one frame each side, where the loop is within the trigger of
//!   the recording, so a patch starts and ends on the loop's own value (it starts from the loop's
//!   value at its start by definition; its last step is put on the loop's value where it ends),
//! * the recording over the span is fitted with ramps, at half the tolerance like a cycle, and
//!   the ramps are the patch's steps.
//!
//! The loop is never given up for a stretch where it plays nothing like the recording: that is a
//! longer patch. Whether loop and patches together are worth it is decided by the caller, by
//! counting steps (see `lightkey_effects`).

use crate::lightkey_curve::Timeline;
use crate::lightkey_fixtures::FixtureArg;
use crate::lightkey_smooth::{polyline, tolerance_for, Series, MAX_RAMP_SECONDS};
use stagepilot_core::config::{
    EffectPatch, EffectPatchStep, LightingEffect, MAX_EFFECT_PATCHES,
    MAX_EFFECT_PATCH_STEPS_PER_PATCH,
};

/// The grid patches are looked for on, seconds: one frame of the 40 Hz replay.
pub const FRAME: f64 = 0.025;
/// The loop is wrong where it differs from the recording by more than this many tolerances.
pub const PATCH_TRIGGER: f64 = 2.0;
/// A stretch of this many frames or fewer is not worth a patch (a flash of two frames).
const MAX_IGNORED_RUN: usize = 2;
/// Stretches closer than this many frames are joined into one patch.
const MERGE_GAP_FRAMES: usize = 8;
/// Two patches of one address are tried as one when they are at most this far apart, and the
/// joined patch spans at most [`MAX_JOINED_SECONDS`]: it follows the recording where the loop
/// would have played between them, which is cheaper when the recording is simple there (dark).
const MAX_JOIN_SECONDS: f64 = 12.0;
const MAX_JOINED_SECONDS: f64 = 120.0;

/// The fixtures of an effect with the series they were fitted from: per member, in the order of
/// `effect.members`, its fixture name and `(property, series)` pairs.
pub type MemberSeries<'a> = Vec<(&'a str, Vec<(&'a str, &'a Series)>)>;

/// The patches of one effect and what they cost.
#[derive(Debug, Clone, Default)]
pub struct PatchFit {
    pub patches: Vec<EffectPatch>,
    /// Patch steps per member (the order of `effect.members`).
    pub member_steps: Vec<usize>,
    /// The fit would exceed the per-effect caps: it cannot be saved as it is.
    pub over_cap: bool,
}

impl PatchFit {
    pub fn steps(&self) -> usize {
        self.member_steps.iter().sum()
    }
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

fn args(values: &[FixtureArg]) -> Vec<f64> {
    values
        .iter()
        .map(|a| match a {
            FixtureArg::Float(f) => *f,
            _ => f64::NAN,
        })
        .collect()
}

/// The stretches `(first frame, last frame)` of `bad` that need a patch: runs longer than
/// [`MAX_IGNORED_RUN`], with runs closer than [`MERGE_GAP_FRAMES`] joined.
fn stretches(bad: &[bool]) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < bad.len() {
        if bad[i] {
            let a = i;
            while i + 1 < bad.len() && bad[i + 1] {
                i += 1;
            }
            if i + 1 - a > MAX_IGNORED_RUN {
                runs.push((a, i));
            }
        }
        i += 1;
    }
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (a, b) in runs {
        match out.last_mut() {
            Some(last) if a - last.1 <= MERGE_GAP_FRAMES => last.1 = b,
            _ => out.push((a, b)),
        }
    }
    out
}

/// The patches `effect` (a loop without patches) needs to follow `members`' recordings over its
/// window, at the tolerance `scale`.
pub fn fit_patches(effect: &LightingEffect, members: &MemberSeries, scale: f64) -> PatchFit {
    let timeline = Timeline::new(&[], std::slice::from_ref(effect));
    let (start, end) = (effect.start_seconds, effect.end_seconds);
    let frames = (((end - start) / FRAME).floor() as usize) + 1;
    let mut fit = PatchFit {
        member_steps: vec![0; members.len()],
        ..PatchFit::default()
    };
    for (mi, (fixture, properties)) in members.iter().enumerate() {
        for (property, series) in properties {
            let address = format!("/fixture/{fixture}/overrides/{property}");
            let tol = tolerance_for(&address, scale);
            let dim = series.dim;
            let mut raw = vec![0.0; dim];
            let mut bad = vec![false; frames];
            for (i, flag) in bad.iter_mut().enumerate() {
                let t = (start + i as f64 * FRAME).min(end);
                series.at(t, &mut raw);
                let Some(looped) = timeline.value_at(&address, t) else {
                    continue;
                };
                let looped = args(&looped);
                if looped.len() != dim || looped.iter().any(|v| v.is_nan()) {
                    continue;
                }
                let diff = raw
                    .iter()
                    .zip(&looped)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0, f64::max);
                *flag = diff > PATCH_TRIGGER * tol;
            }
            // one patch per stretch; neighbours are joined when one patch over both (following the
            // recording through the loop's good stretch between them) is no more steps than two
            let mut own: Vec<EffectPatch> = Vec::new();
            for (a, b) in stretches(&bad) {
                let first = a.saturating_sub(1);
                let last = (b + 1).min(frames - 1);
                let from = round3(start + first as f64 * FRAME);
                // the last frame stands for the end of the window (the grid stops short of it)
                let to = if last == frames - 1 {
                    end
                } else {
                    round3(start + last as f64 * FRAME)
                };
                if to <= from {
                    continue;
                }
                let make = |from: f64, to: f64| {
                    build_patch(
                        &timeline,
                        &address,
                        fixture,
                        property,
                        series,
                        (from, to),
                        to >= end - 1e-9,
                        tol,
                    )
                };
                let Some(patch) = make(from, to) else {
                    continue;
                };
                if let Some(last_patch) = own.last() {
                    let gap = from - last_patch.to_seconds;
                    if gap <= MAX_JOIN_SECONDS && to - last_patch.from_seconds <= MAX_JOINED_SECONDS
                    {
                        if let Some(joined) = make(last_patch.from_seconds, to) {
                            if joined.steps.len() <= last_patch.steps.len() + patch.steps.len() {
                                own.pop();
                                own.push(joined);
                                continue;
                            }
                        }
                    }
                }
                own.push(patch);
            }
            for patch in own {
                if patch.steps.len() > MAX_EFFECT_PATCH_STEPS_PER_PATCH {
                    fit.over_cap = true;
                }
                fit.member_steps[mi] += patch.steps.len();
                fit.patches.push(patch);
            }
        }
    }
    if fit.patches.len() > MAX_EFFECT_PATCHES {
        fit.over_cap = true;
    }
    fit.patches.sort_by(|a, b| {
        a.from_seconds
            .total_cmp(&b.from_seconds)
            .then_with(|| a.fixture.cmp(&b.fixture))
            .then_with(|| a.property.cmp(&b.property))
    });
    fit
}

/// One patch over `span`: the recording fitted with ramps, starting on the loop's value and (away
/// from the window's edges, where the seams blend) ending on it.
#[allow(clippy::too_many_arguments)]
fn build_patch(
    timeline: &Timeline,
    address: &str,
    fixture: &str,
    property: &str,
    series: &Series,
    span: (f64, f64),
    ends_the_window: bool,
    tol: f64,
) -> Option<EffectPatch> {
    let (from, to) = span;
    let dim = series.dim;
    let looped_at = |t: f64| -> Option<Vec<f64>> {
        let v = args(&timeline.value_at(address, t)?);
        (v.len() == dim && v.iter().all(|x| x.is_finite())).then_some(v)
    };
    let n = (((to - from) / FRAME).round() as usize).max(1);
    let mut taus: Vec<f64> = Vec::with_capacity(n + 1);
    let mut values: Vec<f64> = Vec::with_capacity((n + 1) * dim);
    let mut buf = vec![0.0; dim];
    for k in 0..=n {
        let tau = if k == n { to - from } else { k as f64 * FRAME };
        taus.push(tau);
        if k == 0 {
            values.extend(looped_at(from)?);
        } else if k == n && !ends_the_window {
            values.extend(looped_at(to)?);
        } else {
            series.at(from + tau, &mut buf);
            values.extend_from_slice(&buf);
        }
    }
    let mut steps: Vec<EffectPatchStep> = Vec::new();
    for (i, j) in polyline(&taus, &values, dim, 0.5 * tol, MAX_RAMP_SECONDS) {
        let a = &values[i * dim..(i + 1) * dim];
        let b = &values[j * dim..(j + 1) * dim];
        if a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9) {
            continue;
        }
        steps.push(EffectPatchStep {
            at_seconds: round3(taus[i]),
            value: b.iter().map(|v| round4(*v)).collect(),
            fade_seconds: round3(taus[j] - taus[i]),
        });
    }
    if steps.is_empty() {
        return None;
    }
    Some(EffectPatch {
        fixture: fixture.to_string(),
        property: property.to_string(),
        from_seconds: from,
        to_seconds: to,
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_runs_are_ignored_and_near_runs_are_joined() {
        let mut bad = vec![false; 60];
        // two frames: ignored
        bad[3] = true;
        bad[4] = true;
        // three frames, then a gap of six, then three frames: one stretch
        for i in [10, 11, 12, 19, 20, 21] {
            bad[i] = true;
        }
        // far away: a stretch of its own
        for flag in bad.iter_mut().take(45).skip(40) {
            *flag = true;
        }
        assert_eq!(stretches(&bad), vec![(10, 21), (40, 44)]);
    }
}
