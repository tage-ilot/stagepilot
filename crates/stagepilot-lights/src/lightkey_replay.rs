//! The smooth replay engine: for every held fixture property the value at the song position `p`
//! is [`Timeline::value_at`] of the song's rows and effects, and the engine sends only what
//! changed. Pure: no clock, no I/O. The caller passes the monotonic time `now` and the song
//! position `p` (usually `now`-based plus the look lead) of every tick and sends the returned
//! messages itself (the plugin paces them, [`simulate`] counts them).
//!
//! Because the value is a function of `p` alone, a seek, a resume and a re-sync are all the same
//! thing: build the engine (or call [`ReplayEngine::forget_fixture`]) and tick at the new `p`.
//! Pause, stop and the end of the song simply stop ticking; nothing is cleared.
//!
//! # Scheduling (one tick for everything, no task per fixture)
//!
//! * Addresses that are *not moving* (a step, the end of a ramp, the start or end of an effect,
//!   an instant step inside an effect's cycle, the first look after a start or a take-back) are
//!   **discrete**: sent at once whatever the budget, and the engine wakes exactly at the time of
//!   the next one ([`Timeline::next_boundary`]).
//! * A row ramp that is running is sent at most every [`FAST_TICK_SECONDS`] (20 Hz) and only
//!   when it moved by at least one DMX step of the property since the last send.
//! * Addresses driven by an effect are looked at every `effect_interval`, normally 20 Hz.
//! * At most [`MAX_PACKETS_PER_SECOND`] packets leave per second: a token bucket (burst 10)
//!   fills at that rate. Discrete sends always go out (they only make the bucket negative, the
//!   caller's pacing spaces the packets). Ramps then effects spend what is left, the largest
//!   change first. When effect sends had to wait, the effect interval grows towards
//!   [`SLOW_TICK_SECONDS`] (8 Hz); it shrinks back when the budget has room.
//! * An idle engine (nothing moving, no effect) asks for the next wake only at the next
//!   boundary, or not at all.
//!
//! # DMX resolution
//!
//! Levels and colours are 8-bit (one step is 1/255), angles 16-bit on a 540 degree range (one
//! step is about 0.008 degrees). Lightkey truncates to the DMX resolution, so a value that is
//! still on its way is sent half a step higher; a value at rest is sent exactly.
//!
//! One OSC message per address: no bundles and no pattern addresses (not shown to work with
//! Lightkey; the rig test decides).

use crate::lightkey_curve::Timeline;
use crate::lightkey_fixtures::{fixture_of, FixtureArg};
use crate::lightkey_operator::normalise_fixture;
use crate::lightkey_smooth::is_angle;
use stagepilot_core::config::{LightingEffect, OscLightingCue, OscMessage};
use std::collections::BTreeSet;

/// Total packets per second, matching the plugin's 5 ms spacing.
pub const MAX_PACKETS_PER_SECOND: f64 = 200.0;
/// Tick interval while a row ramp (or an effect at full rate) is running: 20 Hz.
pub const FAST_TICK_SECONDS: f64 = 0.05;
/// Slowest effect tick interval when the budget is short: 8 Hz.
pub const SLOW_TICK_SECONDS: f64 = 0.125;
/// Spacing of two packets, seconds (the plugin's `MIN_SEND_GAP`).
pub const PACKET_GAP_SECONDS: f64 = 1.0 / MAX_PACKETS_PER_SECOND;
/// One DMX step of a level or colour (8 bit).
pub const LEVEL_STEP: f64 = 1.0 / 255.0;
/// One DMX step of an angle (16 bit over 540 degrees).
pub const ANGLE_STEP: f64 = 540.0 / 65535.0;
const BURST_PACKETS: f64 = 10.0;
const DEBT_PACKETS: f64 = 200.0;
const EPSILON: f64 = 1e-9;

/// Size of one DMX step of this address's property.
pub fn dmx_step(address: &str) -> f64 {
    if is_angle(address) {
        ANGLE_STEP
    } else {
        LEVEL_STEP
    }
}

/// Whether playing these rows and effects needs the engine. Songs captured before smooth replay
/// (every fade zero, no effects) do not: they keep the plain row-by-row replay.
pub fn is_smooth(rows: &[OscLightingCue], effects: &[LightingEffect]) -> bool {
    effects
        .iter()
        .any(|e| !e.members.is_empty() && !e.cycle.is_empty() && e.period_seconds > 0.0)
        || rows.iter().any(|r| {
            r.fade_seconds > 0.0
                && r.outgoing()
                    .iter()
                    .any(|m| fixture_of(&m.address).is_some())
        })
}

/// Why a message was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A step, the end of a ramp, a start or hand-over, a first look: never delayed.
    Step,
    /// A running row ramp.
    Ramp,
    /// An address driven by an effect.
    Effect,
}

/// What one tick decided.
#[derive(Debug, Clone)]
pub struct Tick {
    pub messages: Vec<OscMessage>,
    /// `kinds[i]` is why `messages[i]` was sent.
    pub kinds: Vec<Kind>,
    /// Effect sends were slowed down at this tick (the effect interval is above 20 Hz's).
    pub reduced: bool,
    /// Seconds until the next tick is needed; `None` when the engine is idle.
    pub wake_in: Option<f64>,
}

#[derive(Debug, Clone)]
struct Slot {
    address: String,
    step: f64,
    /// The value last sent (including the half step), `None` before the first send.
    last: Option<Vec<FixtureArg>>,
    in_effect: bool,
}

/// Replays one song's fixture rows and effects. See the module docs.
#[derive(Debug, Clone)]
pub struct ReplayEngine {
    timeline: Timeline,
    slots: Vec<Slot>,
    tokens: f64,
    last_now: Option<f64>,
    last_p: Option<f64>,
    next_effect: Option<f64>,
    effect_interval: f64,
}

fn numbers(args: &[FixtureArg]) -> Option<Vec<f64>> {
    args.iter()
        .map(|a| match a {
            FixtureArg::Float(f) => Some(*f),
            _ => None,
        })
        .collect()
}

/// How far `new` is from `old`, in DMX steps; infinite when they are not comparable numbers.
fn distance(old: &[FixtureArg], new: &[FixtureArg], step: f64) -> f64 {
    match (numbers(old), numbers(new)) {
        (Some(a), Some(b)) if a.len() == b.len() => {
            a.iter()
                .zip(&b)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f64::max)
                / step
        }
        _ => {
            if old == new {
                0.0
            } else {
                f64::INFINITY
            }
        }
    }
}

/// `value` half a DMX step higher while it is still moving (Lightkey truncates), never pushed
/// past 1.0 or below 0.0 when it started inside that range and is not an angle.
fn with_half_step(address: &str, value: &[FixtureArg], step: f64) -> Vec<FixtureArg> {
    value
        .iter()
        .map(|a| match a {
            FixtureArg::Float(x) => {
                let mut y = x + step / 2.0;
                if !is_angle(address) && (0.0..=1.0).contains(x) {
                    y = y.min(1.0);
                }
                FixtureArg::Float(y)
            }
            other => other.clone(),
        })
        .collect()
}

impl ReplayEngine {
    /// Prepare a song's rows and effects (usually its cue map's `osc_cues` and `effects`).
    pub fn new(rows: &[OscLightingCue], effects: &[LightingEffect]) -> Self {
        let timeline = Timeline::new(rows, effects);
        let slots = timeline
            .addresses()
            .into_iter()
            .map(|a| Slot {
                address: a.to_string(),
                step: dmx_step(a),
                last: None,
                in_effect: false,
            })
            .collect();
        Self {
            timeline,
            slots,
            tokens: BURST_PACKETS,
            last_now: None,
            last_p: None,
            next_effect: None,
            effect_interval: FAST_TICK_SECONDS,
        }
    }

    /// The evaluator this engine plays.
    pub fn timeline(&self) -> &Timeline {
        &self.timeline
    }

    /// Seconds between two looks at effect addresses right now (20 Hz down to 8 Hz).
    pub fn effect_interval(&self) -> f64 {
        self.effect_interval
    }

    /// Forget what was sent to `fixture`: its next tick sends the value at that moment, whatever
    /// it was before. Used when the operator hands a fixture back.
    pub fn forget_fixture(&mut self, fixture: &str) {
        let wanted = normalise_fixture(fixture);
        for slot in &mut self.slots {
            if fixture_of(&slot.address).is_some_and(|f| normalise_fixture(f) == wanted) {
                slot.last = None;
                slot.in_effect = false;
            }
        }
    }

    /// A message was not delivered: send the address again at the next tick.
    pub fn mark_unsent(&mut self, address: &str) {
        for slot in self.slots.iter_mut().filter(|s| s.address == address) {
            slot.last = None;
            slot.in_effect = false;
        }
    }

    /// Fixtures that have no value at `p` on any address (the song has not touched them yet).
    pub fn fixtures_without_value(&self, p: f64) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut valued = BTreeSet::new();
        for slot in &self.slots {
            let Some(name) = fixture_of(&slot.address) else {
                continue;
            };
            seen.insert(name.to_string());
            if self.timeline.value_at(&slot.address, p).is_some() {
                valued.insert(name.to_string());
            }
        }
        seen.difference(&valued).cloned().collect()
    }

    /// One scheduler tick at monotonic time `now` (seconds) for song position `p`. `yielded`
    /// tells whether the operator has the fixture of an address: such an address is not driven
    /// and is sent again, at its value then, as soon as it is not yielded any more.
    pub fn tick(&mut self, now: f64, p: f64, yielded: &dyn Fn(&str) -> bool) -> Tick {
        let elapsed = self.last_now.map_or(0.0, |l| (now - l).max(0.0));
        self.last_now = Some(now);
        self.tokens = (self.tokens + MAX_PACKETS_PER_SECOND * elapsed).min(BURST_PACKETS);
        let effect_turn = self.next_effect.is_none_or(|t| now + EPSILON >= t);
        let previous_p = self.last_p.replace(p).unwrap_or(f64::NEG_INFINITY);

        struct Due {
            slot: usize,
            value: Vec<FixtureArg>,
            kind: Kind,
            weight: f64,
        }
        let mut steps: Vec<Due> = Vec::new();
        let mut ramps: Vec<Due> = Vec::new();
        let mut effects: Vec<Due> = Vec::new();
        let mut ramp_running = false;
        let mut effect_active = false;

        for (i, slot) in self.slots.iter_mut().enumerate() {
            if yielded(&slot.address) {
                slot.last = None;
                slot.in_effect = false;
                continue;
            }
            let in_effect = self.timeline.in_effect(&slot.address, p);
            effect_active |= in_effect;
            let instant = in_effect && self.timeline.instant_step_in(&slot.address, previous_p, p);
            let waiting =
                in_effect && slot.in_effect && slot.last.is_some() && !effect_turn && !instant;
            let Some(value) = self.timeline.value_at(&slot.address, p) else {
                slot.in_effect = in_effect;
                continue;
            };
            let moving = in_effect || self.timeline.is_moving(&slot.address, p);
            ramp_running |= moving && !in_effect;
            if waiting {
                continue;
            }
            let handover = slot.in_effect != in_effect;
            let kind = if slot.last.is_none() || handover || instant || !moving {
                Kind::Step
            } else if in_effect {
                Kind::Effect
            } else {
                Kind::Ramp
            };
            let target = if moving && kind != Kind::Step {
                with_half_step(&slot.address, &value, slot.step)
            } else {
                value
            };
            let weight = slot
                .last
                .as_deref()
                .map_or(f64::INFINITY, |old| distance(old, &target, slot.step));
            let due = match kind {
                Kind::Step => weight > EPSILON / slot.step,
                _ => weight >= 1.0 - EPSILON,
            };
            slot.in_effect = in_effect;
            if !due {
                continue;
            }
            let d = Due {
                slot: i,
                value: target,
                kind,
                weight,
            };
            match kind {
                Kind::Step => steps.push(d),
                Kind::Ramp => ramps.push(d),
                Kind::Effect => effects.push(d),
            }
        }

        let mut messages = Vec::new();
        let mut kinds = Vec::new();
        let mut take = |engine: &mut Self, d: Due| {
            let slot = &mut engine.slots[d.slot];
            messages.push(OscMessage {
                address: slot.address.clone(),
                args: d.value.iter().map(FixtureArg::to_osc).collect(),
            });
            kinds.push(d.kind);
            slot.last = Some(d.value);
        };
        for d in steps {
            self.tokens = (self.tokens - 1.0).max(-DEBT_PACKETS);
            take(self, d);
        }
        let by_weight = |a: &Due, b: &Due| b.weight.total_cmp(&a.weight);
        ramps.sort_by(by_weight);
        let mut deferred_ramps = 0usize;
        for d in ramps {
            if self.tokens >= 1.0 {
                self.tokens -= 1.0;
                take(self, d);
            } else {
                deferred_ramps += 1;
            }
        }
        effects.sort_by(by_weight);
        let mut deferred_effects = 0usize;
        for d in effects {
            if self.tokens >= 1.0 {
                self.tokens -= 1.0;
                take(self, d);
            } else {
                deferred_effects += 1;
            }
        }

        if effect_turn {
            if deferred_effects > 0 {
                self.effect_interval = (self.effect_interval * 1.5).min(SLOW_TICK_SECONDS);
            } else if self.tokens >= BURST_PACKETS / 2.0 {
                self.effect_interval = (self.effect_interval / 1.5).max(FAST_TICK_SECONDS);
            }
            self.next_effect = Some(now + self.effect_interval);
        }
        let reduced = self.effect_interval > FAST_TICK_SECONDS + EPSILON;

        let mut wake: Option<f64> = None;
        let mut soonest = |w: f64| wake = Some(wake.map_or(w, |x: f64| x.min(w)));
        if ramp_running {
            soonest(FAST_TICK_SECONDS);
        }
        if effect_active {
            soonest((self.next_effect.unwrap_or(now) - now).max(0.001));
        }
        if deferred_ramps + deferred_effects > 0 {
            soonest(PACKET_GAP_SECONDS * 4.0);
        }
        if let Some(b) = self.timeline.next_boundary(p) {
            soonest((b - p).max(0.0) + 1e-6);
        }
        Tick {
            messages,
            kinds,
            reduced,
            wake_in: wake,
        }
    }
}

/// One packet of a simulated replay.
#[derive(Debug, Clone)]
pub struct SentPacket {
    /// When it leaves (after the 5 ms pacing), seconds.
    pub at: f64,
    /// The song position its value was evaluated at.
    pub position: f64,
    pub message: OscMessage,
    pub kind: Kind,
    /// An effect value sent while the effect rate was lowered.
    pub reduced: bool,
}

/// Run the engine on a simulated clock from `from` to `to` (song seconds; the clock and the song
/// position agree, the position being `lead` ahead): tick when the engine asks, pace the packets
/// 5 ms apart like the plugin, return every packet. No sockets, no sleeping.
/// `yielded(now, address)` says whether the operator has that address's fixture at `now`.
pub fn simulate(
    engine: &mut ReplayEngine,
    from: f64,
    to: f64,
    lead: f64,
    yielded: &dyn Fn(f64, &str) -> bool,
) -> Vec<SentPacket> {
    let mut out = Vec::new();
    let mut free_at = f64::NEG_INFINITY;
    let mut now = from;
    while now < to {
        let tick = engine.tick(now, now + lead, &|a| yielded(now, a));
        let mut slot = free_at.max(now);
        for (message, kind) in tick.messages.into_iter().zip(tick.kinds) {
            out.push(SentPacket {
                at: slot,
                position: now + lead,
                message,
                kind,
                reduced: tick.reduced && kind == Kind::Effect,
            });
            slot += PACKET_GAP_SECONDS;
        }
        free_at = slot;
        let finished = if slot > now {
            slot - PACKET_GAP_SECONDS
        } else {
            now
        };
        let Some(wake) = tick.wake_in else {
            break;
        };
        now = (now + wake).max(finished);
    }
    out
}
