//! The smooth replay engine: for every held fixture property the value at the song position `p`
//! is [`Timeline::playback_at`] (the song's rows and effects, with the seams of effects blended), and the engine sends only what
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
//! * **A very busy song** (see [`ReplayLoad`]): when for more than [`BUSY_AFTER_SECONDS`] a class
//!   (ramps or effects) is starved (served below 10% of what it asks for), the budget is shared
//!   fairly instead of ramps first: ramps may take at most [`RAMP_SHARE`] (65%) of the packets
//!   while effects wait, so effects get at least [`EFFECT_SHARE`] (35%), and a share one class
//!   does not use goes to the other. Inside a class the largest change still goes first, but an
//!   address that has waited [`SERVE_AFTER_SECONDS`] goes before the others (oldest first), so
//!   the addresses are served in turns of about a quarter of a second and none starves. A song
//!   that is not starved never leaves the plain rule above and plays exactly as before.
//! * An idle engine (nothing moving, no effect) asks for the next wake only at the next
//!   boundary, or not at all.
//!
//! # Step bursts (looks land on the beat)
//!
//! A look change can be 60 to 100 values at one row time. One message per address at 5 ms
//! spacing needs `(n - 1) x 5 ms` to send them all (490 ms for 99 values): sent from the row
//! time on, the last value of the look would arrive half a second late and no queue order can
//! change that. The schedule is known in advance, so the engine starts such a burst **before**
//! its row: the burst is spread so that its middle lands on the row time, i.e. it starts
//! `min((n - 1) x 5 ms / 2, STEP_LEAD_MAX_MS)` early (150 ms for 60 values, 242 ms for 98,
//! never more than 300 ms by default). The values are those of the row, and the fixtures are
//! held at them until the row time (their ramp and effect values in between are superseded by
//! the step that follows).
//!
//! * Only a burst that would take longer than [`STEP_BURST_MIN_MS`] (40 ms, i.e. 10 or more
//!   values) is moved; single steps and small bursts keep exact timing.
//! * Inside a burst the visually important values go first, in a fixed order: dimmer, colour,
//!   other levels and options, angles (pan, tilt, ...), and by address inside a class.
//! * A burst never starts before the song position the tick runs at (a start at position 0 or a
//!   resume or seek computes from the new position: values for rows already in the past are
//!   never sent) and never before the previous burst has finished, so bursts keep their time
//!   order; when rows are closer together than a burst takes, the later one is late instead.
//! * A seek backwards is detected (the position goes back) and forgets what was pre-sent.
//!
//! Why up to a few hundred milliseconds early is fine for lighting: a look change is a visual
//! event that the eye reads over about 100 ms or more, a fixture's dimmer already takes tens of
//! milliseconds to follow, and only part of a big look (a few fixtures of many) is early, the
//! first ones sent. The price: during the burst, some fixtures of the look change up to
//! `STEP_LEAD_MAX_MS` before the beat while the rest follows.
//!
//! # DMX resolution
//!
//! A value that is still moving is sent as it will be half a tick later (the middle of the
//! interval it will be held for), which halves the error of holding it.
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
use crate::lightkey_smooth::{is_angle, property_class, tolerance_for};
use serde::{Deserialize, Serialize};
use stagepilot_core::config::{LightingEffect, OscLightingCue, OscMessage};
use std::collections::{BTreeSet, HashMap};
use ts_rs::TS;

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
/// The most a step burst may start before its row, milliseconds (see the module docs, "Step
/// bursts"). `ReplayEngine::with_step_lead_max_ms` overrides it per engine (later: per song).
pub const STEP_LEAD_MAX_MS: f64 = 300.0;
/// A burst that would take no longer than this to send keeps its exact timing, milliseconds.
pub const STEP_BURST_MIN_MS: f64 = 40.0;
/// While steps wait in the pacing queue, every this-many-th slot goes to a waiting ramp value
/// (if there is one): a step burst takes at most 3 of 4 slots, so running ramps keep a quarter of
/// the 200 packets a second (50 a second) instead of standing still for the burst.
pub const RAMP_SHARE_EVERY: u32 = 4;
/// The least share of the packets that effects (loops) get on a very busy song, 35%.
pub const EFFECT_SHARE: f64 = 0.35;
/// The most share ramps may take while effects wait on a very busy song, 65%.
pub const RAMP_SHARE: f64 = 1.0 - EFFECT_SHARE;
/// On a very busy song an address that has waited this long for its next packet goes before
/// the ones that have not, oldest first. The turns come round in about a quarter of a second
/// (the 200 packets a second divided by the addresses with something to say), so every address
/// is served every 250 ms or a little later; none is left out.
pub const SERVE_AFTER_SECONDS: f64 = 0.1;
/// A class has to be starved for longer than this before the song counts as very busy (and the
/// budget is shared by the fair rule).
pub const BUSY_AFTER_SECONDS: f64 = 2.0;
/// Time constant of the served/asked average, seconds.
const LOAD_SECONDS: f64 = 1.0;
/// A class is starved when it is served below this share of what it asked for ...
const STARVED_BELOW: f64 = 0.10;
/// ... and a very busy song is over when every class is served above this share again ...
const RECOVER_ABOVE: f64 = 0.85;
/// ... for this long, seconds.
const RECOVER_AFTER_SECONDS: f64 = 4.0;
/// Requests a class needs in the average before it can count as short.
const MIN_LOAD_ASKED: f64 = 5.0;
const BURST_PACKETS: f64 = 10.0;
const DEBT_PACKETS: f64 = 200.0;
const EPSILON: f64 = 1e-9;

/// Order inside a step burst: the values that change the picture most go first.
fn class_rank(address: &str, dim: usize) -> u8 {
    match property_class(address, dim) {
        "dimmer" => 0,
        "colour" => 1,
        "angle" => 3,
        _ => 2,
    }
}

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

/// How well the engine serves what the song asks for (the lights status' `replay_load`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReplayLoad {
    /// The song is very busy: for more than two seconds a class (ramps or loops) was starved,
    /// served below a tenth of what it asked for, so some lights update less often than
    /// recorded. Stays on until every class is served again.
    pub busy: bool,
    /// Packets sent over packets wanted (ramps and effects), a percentage from 0 to 100; 100
    /// while nothing is asked.
    pub served_percent: u8,
}

impl Default for ReplayLoad {
    fn default() -> Self {
        Self {
            busy: false,
            served_percent: 100,
        }
    }
}

/// Counts of what ramps (0) and effects (1) asked for and got, averaged over about a second,
/// and whether the song is very busy.
#[derive(Debug, Clone, Default)]
struct LoadTracker {
    asked: [f64; 2],
    got: [f64; 2],
    starved_since: Option<f64>,
    recovered_since: Option<f64>,
    busy: bool,
}

impl LoadTracker {
    /// One tick: `elapsed` seconds since the tick before, `asked` and `got` requests of this tick
    /// per class (see [`Slot::next_request`]).
    fn update(&mut self, now: f64, elapsed: f64, asked: [usize; 2], got: [usize; 2]) {
        let keep = (-elapsed / LOAD_SECONDS).exp();
        for c in 0..2 {
            self.asked[c] = self.asked[c] * keep + asked[c] as f64;
            self.got[c] = self.got[c] * keep + got[c] as f64;
        }
        let asking = |c: usize| self.asked[c] >= MIN_LOAD_ASKED;
        let starved = (0..2).any(|c| asking(c) && self.got[c] < STARVED_BELOW * self.asked[c]);
        let recovered = (0..2).all(|c| !asking(c) || self.got[c] >= RECOVER_ABOVE * self.asked[c]);
        if self.busy {
            self.recovered_since = if recovered {
                Some(self.recovered_since.unwrap_or(now))
            } else {
                None
            };
            if self
                .recovered_since
                .is_some_and(|since| now - since > RECOVER_AFTER_SECONDS)
            {
                self.busy = false;
                self.recovered_since = None;
                self.starved_since = None;
            }
        } else {
            self.starved_since = if starved {
                Some(self.starved_since.unwrap_or(now))
            } else {
                None
            };
            self.busy = self
                .starved_since
                .is_some_and(|since| now - since > BUSY_AFTER_SECONDS);
        }
    }

    fn load(&self) -> ReplayLoad {
        let asked = self.asked[0] + self.asked[1];
        let served = if asked < 1e-6 {
            100.0
        } else {
            (100.0 * (self.got[0] + self.got[1]) / asked).clamp(0.0, 100.0)
        };
        ReplayLoad {
            busy: self.busy,
            served_percent: served.round() as u8,
        }
    }
}

/// What one tick decided.
#[derive(Debug, Clone)]
pub struct Tick {
    pub messages: Vec<OscMessage>,
    /// `kinds[i]` is why `messages[i]` was sent.
    pub kinds: Vec<Kind>,
    /// `positions[i]` is the song position `messages[i]`'s value was evaluated for (the tick's
    /// position, or the row time of a step burst sent early).
    pub positions: Vec<f64>,
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
    /// Its row's step was sent early: nothing else is sent for it before this song position.
    hold_until: Option<f64>,
    /// Since when a running ramp or effect value of it has been due but not sent (monotonic
    /// seconds); only the fair rule of a very busy song uses it.
    due_since: Option<f64>,
    /// A due ramp or effect value counts once as a request (for [`ReplayLoad`]) per this many
    /// seconds, however many ticks it waits: it is "asked for" again at this time.
    next_request: f64,
    /// The class (0 ramp, 1 effect) of a request not yet answered by a packet.
    open: Option<usize>,
}

/// Counters of one engine (see [`ReplayEngine::stats`]).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EngineStats {
    /// Running ramp values that had to wait for budget (each is up to 50 ms of a ramp not
    /// updated).
    pub ramp_deferred: u64,
    /// Seconds (of the engine's clock) in which at least one ramp value was waiting for budget.
    pub ramp_skipped_seconds: f64,
    /// Step bursts that were started early.
    pub bursts_early: u64,
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
    lead_max: f64,
    /// Song position up to which the steps are already sent (a burst sent early covers its row).
    sent_through: f64,
    /// Song position at which the last early burst has left the 5 ms pacing.
    burst_end: f64,
    stats: EngineStats,
    ramps_waiting_since: Option<f64>,
    load: LoadTracker,
    /// Packets ramps may still take while effects wait (the fair rule of a very busy song).
    ramp_allow: f64,
}

/// A value (slot, value, row time) sent ahead of its row.
type EarlyValue = (usize, Vec<FixtureArg>, f64);
/// The steps (slot, value) of the row at a time.
type Row = (f64, Vec<(usize, Vec<FixtureArg>)>);

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
    /// A due ramp or effect value that got no packet this tick: it keeps waiting since the
    /// first time.
    fn defer(slot: &mut Slot, now: f64) {
        slot.due_since.get_or_insert(now);
    }

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
                hold_until: None,
                due_since: None,
                next_request: f64::NEG_INFINITY,
                open: None,
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
            lead_max: STEP_LEAD_MAX_MS / 1000.0,
            sent_through: f64::NEG_INFINITY,
            burst_end: f64::NEG_INFINITY,
            stats: EngineStats::default(),
            ramps_waiting_since: None,
            load: LoadTracker::default(),
            ramp_allow: BURST_PACKETS * RAMP_SHARE,
        }
    }

    /// How well the song's ramps and effects are served right now (see [`ReplayLoad`]).
    pub fn load(&self) -> ReplayLoad {
        self.load.load()
    }

    /// Override how early a step burst may start, in milliseconds (0 turns the lead off). The
    /// default is [`STEP_LEAD_MAX_MS`]; a per-song setting is meant to use this later.
    pub fn with_step_lead_max_ms(mut self, ms: f64) -> Self {
        self.lead_max = (ms / 1000.0).max(0.0);
        self
    }

    /// Counters since the engine was built.
    pub fn stats(&self) -> EngineStats {
        self.stats
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
                slot.hold_until = None;
            }
        }
    }

    /// A message was not delivered: send the address again at the next tick.
    pub fn mark_unsent(&mut self, address: &str) {
        for slot in self.slots.iter_mut().filter(|s| s.address == address) {
            slot.last = None;
            slot.in_effect = false;
            slot.hold_until = None;
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
            if self.timeline.playback_at(&slot.address, p).is_some() {
                valued.insert(name.to_string());
            }
        }
        seen.difference(&valued).cloned().collect()
    }

    /// The steps that are due at song position `pos` for slots not held by the operator, as the
    /// tick at `pos` would send them (`prev` is the position of the tick before). `overlay` holds
    /// values (and effect states) of earlier rows that are assumed sent.
    fn steps_at(
        &self,
        pos: f64,
        prev: f64,
        yielded: &dyn Fn(&str) -> bool,
        overlay: &HashMap<usize, (Vec<FixtureArg>, bool)>,
    ) -> Vec<(usize, Vec<FixtureArg>)> {
        let mut out = Vec::new();
        for (i, slot) in self.slots.iter().enumerate() {
            if yielded(&slot.address) || slot.hold_until.is_some_and(|h| pos + EPSILON < h) {
                continue;
            }
            let (last, was_in_effect) = match overlay.get(&i) {
                Some((v, e)) => (Some(v.as_slice()), *e),
                None => (slot.last.as_deref(), slot.in_effect),
            };
            let in_effect = self.timeline.in_effect(&slot.address, pos);
            let instant = in_effect && self.timeline.instant_step_in(&slot.address, prev, pos);
            let Some(value) = self.timeline.playback_at(&slot.address, pos) else {
                continue;
            };
            let moving = in_effect || self.timeline.is_moving(&slot.address, pos);
            let step = last.is_none() || was_in_effect != in_effect || instant || !moving;
            let due = last.is_none_or(|old| distance(old, &value, slot.step) > EPSILON / slot.step);
            if step && due {
                out.push((i, value));
            }
        }
        out
    }

    /// How many addresses have a different value just after `b` than just before it.
    fn changes_at(&self, b: f64) -> usize {
        self.slots
            .iter()
            .filter(|s| {
                let before = self.timeline.playback_at(&s.address, b - 1e-6);
                let after = self.timeline.playback_at(&s.address, b + 1e-6);
                match (before, after) {
                    (Some(x), Some(y)) => distance(&x, &y, s.step) > EPSILON / s.step,
                    (None, Some(_)) => true,
                    _ => false,
                }
            })
            .count()
    }

    /// Start the step bursts whose time has come (see the module docs, "Step bursts"): returns
    /// the values to send now, with the row time each is for, and in how many seconds the next
    /// burst has to be started (`None` when no burst is ahead).
    fn presend(
        &mut self,
        p: f64,
        yielded: &dyn Fn(&str) -> bool,
    ) -> (Vec<EarlyValue>, Option<f64>) {
        let mut sent = Vec::new();
        if self.lead_max <= 0.0 {
            return (sent, None);
        }
        let ramps_running = self.slots.iter().any(|s| {
            !self.timeline.in_effect(&s.address, p) && self.timeline.is_moving(&s.address, p)
        });
        // Seconds the pacing needs for `n` values (a quarter more while ramps take their share).
        let span_of = |n: usize| {
            let mut span = n.saturating_sub(1) as f64 * PACKET_GAP_SECONDS;
            if ramps_running {
                span *= 1.0 + 1.0 / (RAMP_SHARE_EVERY - 1) as f64;
            }
            span
        };
        let mut cursor = p.max(self.sent_through);
        // Rows ahead with too few values to move (their steps go out at their own time unless a
        // big burst right behind them starts before them and takes them along).
        let mut pending: Vec<Row> = Vec::new();
        let mut overlay: HashMap<usize, (Vec<FixtureArg>, bool)> = HashMap::new();
        // When the earliest burst ahead has to start (a later row can pull it earlier).
        let mut wake: Option<f64> = None;
        loop {
            let Some(b) = self.timeline.next_boundary(cursor) else {
                return (sent, wake);
            };
            if b - self.lead_max > p + EPSILON {
                // Too far ahead for any burst to start. A row that changes enough values to
                // make a burst is looked at again when its burst could start; any other
                // boundary needs no earlier look (an idle engine stays idle).
                let span = span_of(self.changes_at(b));
                if span > STEP_BURST_MIN_MS / 1000.0 {
                    let w = b - (span / 2.0).min(self.lead_max) - p;
                    wake = Some(wake.map_or(w, |x| x.min(w)));
                }
                return (sent, wake);
            }
            let pos = b + 1e-6;
            let items = self.steps_at(pos, cursor, yielded, &overlay);
            if span_of(items.len()) <= STEP_BURST_MIN_MS / 1000.0 {
                if !items.is_empty() {
                    for (i, v) in &items {
                        let e = self.timeline.in_effect(&self.slots[*i].address, pos);
                        overlay.insert(*i, (v.clone(), e));
                    }
                    pending.push((b, items));
                }
                cursor = pos;
                continue;
            }
            // A burst. Rows right before it that start after it would have to start take part.
            let mut first = 0;
            let start = loop {
                let n = items.len() + pending[first..].iter().map(|r| r.1.len()).sum::<usize>();
                let lead = (span_of(n) / 2.0).min(self.lead_max);
                let start = (b - lead).max(self.burst_end).max(0.0);
                if first < pending.len() && pending[first].0 < start - EPSILON {
                    first += 1;
                } else {
                    break start;
                }
            };
            if start > p + EPSILON {
                // Not yet. It stays a candidate for a burst right behind it to take along.
                let w = start - p;
                wake = Some(wake.map_or(w, |x| x.min(w)));
                for (i, v) in &items {
                    let e = self.timeline.in_effect(&self.slots[*i].address, pos);
                    overlay.insert(*i, (v.clone(), e));
                }
                pending.push((b, items));
                cursor = pos;
                continue;
            }
            self.stats.bursts_early += 1;
            let group_n = items.len() + pending[first..].iter().map(|r| r.1.len()).sum::<usize>();
            let mut rows: Vec<Row> = pending.drain(first..).collect();
            rows.push((b, items));
            for (row_time, mut row) in rows {
                row.sort_by(|a, b| {
                    let key = |i: usize, v: &[FixtureArg]| {
                        (class_rank(&self.slots[i].address, v.len()), i)
                    };
                    key(a.0, &a.1).cmp(&key(b.0, &b.1))
                });
                let row_pos = row_time + 1e-6;
                for (i, value) in row {
                    let in_effect = self.timeline.in_effect(&self.slots[i].address, row_pos);
                    let slot = &mut self.slots[i];
                    slot.last = Some(value.clone());
                    slot.in_effect = in_effect;
                    slot.hold_until = Some(row_time);
                    sent.push((i, value, row_time));
                }
            }
            self.burst_end = p + span_of(group_n);
            self.sent_through = pos;
            cursor = pos;
            pending.clear();
            overlay.clear();
            wake = None;
        }
    }

    /// One scheduler tick at monotonic time `now` (seconds) for song position `p`. `yielded`
    /// tells whether the operator has the fixture of an address: such an address is not driven
    /// and is sent again, at its value then, as soon as it is not yielded any more.
    pub fn tick(&mut self, now: f64, p: f64, yielded: &dyn Fn(&str) -> bool) -> Tick {
        let elapsed = self.last_now.map_or(0.0, |l| (now - l).max(0.0));
        self.last_now = Some(now);
        self.tokens = (self.tokens + MAX_PACKETS_PER_SECOND * elapsed).min(BURST_PACKETS);
        // A very busy song shares the budget fairly (see the module docs); any other song
        // keeps the plain rule, ramps first.
        let fair = self.load.busy;
        self.ramp_allow = if fair {
            (self.ramp_allow + MAX_PACKETS_PER_SECOND * RAMP_SHARE * elapsed)
                .min(BURST_PACKETS * RAMP_SHARE)
        } else {
            BURST_PACKETS * RAMP_SHARE
        };
        let effect_turn = self.next_effect.is_none_or(|t| now + EPSILON >= t);
        let previous_p = self.last_p.replace(p).unwrap_or(f64::NEG_INFINITY);
        if p < previous_p - EPSILON {
            // The position went back (a seek the caller did not rebuild the engine for): what
            // was sent early belongs to a future that is not coming; send the values at `p`.
            for slot in &mut self.slots {
                if slot.hold_until.take().is_some() {
                    slot.last = None;
                    slot.in_effect = false;
                }
            }
            self.sent_through = f64::NEG_INFINITY;
            self.burst_end = f64::NEG_INFINITY;
        }

        struct Due {
            slot: usize,
            value: Vec<FixtureArg>,
            kind: Kind,
            weight: f64,
            /// Seconds it has been due and not sent.
            waited: f64,
        }
        let mut steps: Vec<Due> = Vec::new();
        let mut ramps: Vec<Due> = Vec::new();
        let mut effects: Vec<Due> = Vec::new();
        let mut asked = [0usize; 2];
        let mut got = [0usize; 2];
        let mut ramp_running = false;
        let mut effect_active = false;

        for (i, slot) in self.slots.iter_mut().enumerate() {
            if yielded(&slot.address) {
                slot.last = None;
                slot.in_effect = false;
                slot.hold_until = None;
                slot.due_since = None;
                slot.open = None;
                continue;
            }
            if slot.hold_until.is_some_and(|h| p + EPSILON < h) {
                continue; // its row's step went out early
            }
            slot.hold_until = None;
            let in_effect = self.timeline.in_effect(&slot.address, p);
            effect_active |= in_effect;
            let instant = in_effect && self.timeline.instant_step_in(&slot.address, previous_p, p);
            // An effect value is looked at on the effect's turn only, except on a very busy
            // song, where one that is still waiting for its packet stays in the running.
            let waiting = in_effect
                && slot.in_effect
                && slot.last.is_some()
                && !effect_turn
                && !instant
                && !(fair && slot.due_since.is_some());
            let Some(now_value) = self.timeline.playback_at(&slot.address, p) else {
                slot.in_effect = in_effect;
                slot.due_since = None;
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
            let target = if kind == Kind::Step {
                now_value
            } else {
                // A value held until the next send is best when it is the one from the middle of
                // that interval; and Lightkey truncates, so it also goes half a DMX step up.
                let interval = if kind == Kind::Effect {
                    self.effect_interval
                } else {
                    FAST_TICK_SECONDS
                };
                let ahead = self
                    .timeline
                    .playback_at(&slot.address, p + interval / 2.0)
                    .unwrap_or(now_value);
                with_half_step(&slot.address, &ahead, slot.step)
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
            if !due || kind == Kind::Step {
                slot.due_since = None;
            }
            if !due {
                continue;
            }
            if kind != Kind::Step && now + EPSILON >= slot.next_request {
                let class = usize::from(kind == Kind::Effect);
                asked[class] += 1;
                slot.open = Some(class);
                slot.next_request = now
                    + if kind == Kind::Effect {
                        self.effect_interval
                    } else {
                        FAST_TICK_SECONDS
                    };
            }
            let waited = slot.due_since.map_or(0.0, |since| (now - since).max(0.0));
            let d = Due {
                slot: i,
                value: target,
                kind,
                weight,
                waited,
            };
            match kind {
                Kind::Step => steps.push(d),
                Kind::Ramp => ramps.push(d),
                Kind::Effect => effects.push(d),
            }
        }

        let mut messages = Vec::new();
        let mut kinds = Vec::new();
        let mut positions = Vec::new();
        let mut take = |engine: &mut Self, d: Due| {
            let slot = &mut engine.slots[d.slot];
            messages.push(OscMessage {
                address: slot.address.clone(),
                args: d.value.iter().map(FixtureArg::to_osc).collect(),
            });
            kinds.push(d.kind);
            positions.push(p);
            slot.last = Some(d.value);
            slot.due_since = None;
            if let Some(class) = slot.open.take() {
                got[class] += 1;
            }
        };
        for d in steps {
            self.tokens = (self.tokens - 1.0).max(-DEBT_PACKETS);
            take(self, d);
        }
        let by_weight = |a: &Due, b: &Due| b.weight.total_cmp(&a.weight);
        // Ramps compete by how far they are from their value in units of their own tolerance,
        // not in DMX steps: an angle has 184 times more steps per tolerance than a level, and
        // counting steps would let the angles take the whole budget while a dimmer stands still.
        let tolerance = |d: &Due| tolerance_for(&self.slots[d.slot].address, 1.0);
        ramps.sort_by(|a, b| {
            let key = |d: &Due| d.weight * self.slots[d.slot].step / tolerance(d);
            key(b).total_cmp(&key(a))
        });
        effects.sort_by(by_weight);
        let mut deferred_ramps = 0usize;
        let mut deferred_effects = 0usize;
        if fair {
            // A very busy song. Ramps and effects share the budget (ramps at most 65% while
            // effects wait, a share one class does not use goes to the other), and inside the
            // order an address that has waited 250 ms goes first, oldest first, so none starves.
            let effects_pending = !effects.is_empty();
            if !effects_pending {
                self.ramp_allow = BURST_PACKETS * RAMP_SHARE;
            }
            let (mut late, rest): (Vec<Due>, Vec<Due>) = ramps
                .into_iter()
                .chain(effects)
                .partition(|d| d.waited >= SERVE_AFTER_SECONDS);
            late.sort_by(|a, b| b.waited.total_cmp(&a.waited));
            let mut held_back: Vec<Due> = Vec::new();
            for d in late.into_iter().chain(rest) {
                if self.tokens < 1.0 {
                    Self::defer(&mut self.slots[d.slot], now);
                    match d.kind {
                        Kind::Ramp => deferred_ramps += 1,
                        _ => deferred_effects += 1,
                    }
                } else if d.kind == Kind::Ramp && effects_pending && self.ramp_allow < 1.0 {
                    held_back.push(d); // effects have this share
                } else {
                    self.tokens -= 1.0;
                    if d.kind == Kind::Ramp {
                        self.ramp_allow -= 1.0;
                    }
                    take(self, d);
                }
            }
            // The effects that were waiting are served: what they did not use goes to ramps.
            for d in held_back {
                if self.tokens >= 1.0 {
                    self.tokens -= 1.0;
                    take(self, d);
                } else {
                    Self::defer(&mut self.slots[d.slot], now);
                    deferred_ramps += 1;
                }
            }
        } else {
            for d in ramps {
                if self.tokens >= 1.0 {
                    self.tokens -= 1.0;
                    take(self, d);
                } else {
                    Self::defer(&mut self.slots[d.slot], now);
                    deferred_ramps += 1;
                }
            }
            for d in effects {
                if self.tokens >= 1.0 {
                    self.tokens -= 1.0;
                    take(self, d);
                } else {
                    Self::defer(&mut self.slots[d.slot], now);
                    deferred_effects += 1;
                }
            }
        }
        self.load.update(now, elapsed, asked, got);

        // Step bursts whose time to start has come (or is near): after the steps of this tick,
        // which belong to earlier rows.
        let (early, burst_wake) = self.presend(p, yielded);
        for (slot, value, at) in early {
            // Not charged to the bucket: the pacing queue gives ramps their share of the slots
            // while a burst drains, so the ramps keep their budget (see `RAMP_SHARE_EVERY`).
            let s = &mut self.slots[slot];
            messages.push(OscMessage {
                address: s.address.clone(),
                args: value.iter().map(FixtureArg::to_osc).collect(),
            });
            kinds.push(Kind::Step);
            positions.push(at);
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
        self.stats.ramp_deferred += deferred_ramps as u64;
        if let Some(since) = self.ramps_waiting_since.take() {
            self.stats.ramp_skipped_seconds += (now - since).max(0.0);
        }
        if deferred_ramps > 0 {
            self.ramps_waiting_since = Some(now);
        }

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
        if let Some(w) = burst_wake {
            soonest(w);
        }
        if let Some(b) = self.timeline.next_boundary(p) {
            soonest((b - p).max(0.0) + 1e-6);
        }
        Tick {
            messages,
            kinds,
            positions,
            reduced,
            wake_in: wake,
        }
    }
}

/// A message waiting in the [`PacingQueue`].
#[derive(Debug, Clone)]
pub struct Queued {
    pub message: OscMessage,
    pub kind: Kind,
    /// When the tick that chose this value queued it, seconds.
    pub queued_at: f64,
    /// An effect value chosen while the effect rate was lowered.
    pub reduced: bool,
    /// The song position the value was evaluated for, when not the one of the tick (a step
    /// sent early for its row).
    pub position: Option<f64>,
    seq: u64,
}

fn rank(kind: Kind) -> u8 {
    match kind {
        Kind::Step => 0,
        Kind::Ramp => 1,
        Kind::Effect => 2,
    }
}

/// The queue between the engine's ticks and the 5 ms pacing. Steps leave first, then ramp
/// values, then effect values, oldest first inside a class. A ramp or effect value that is
/// still waiting when a newer value for the same address is queued is replaced by it (only the
/// latest value of an address matters, and a stale one is never sent after a newer one); the
/// replacement keeps the place of the value it replaces when both are of one class. Two steps
/// of one address are both kept, in order. Pure: the caller owns the clock.
#[derive(Debug, Clone, Default)]
pub struct PacingQueue {
    items: Vec<Queued>,
    seq: u64,
    next_free: f64,
    steps_in_a_row: u32,
}

impl PacingQueue {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            seq: 0,
            next_free: f64::NEG_INFINITY,
            steps_in_a_row: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Drop everything waiting (a seek, a stop).
    pub fn clear(&mut self) {
        self.items.clear();
        self.steps_in_a_row = 0;
    }

    /// Queue `message` chosen at time `now`.
    pub fn push(&mut self, message: OscMessage, kind: Kind, now: f64, reduced: bool) {
        self.push_for(message, kind, now, reduced, None);
    }

    /// Like [`PacingQueue::push`], for a value evaluated at song position `position`.
    pub fn push_for(
        &mut self,
        message: OscMessage,
        kind: Kind,
        now: f64,
        reduced: bool,
        position: Option<f64>,
    ) {
        let mut seq = None;
        // A waiting ramp or effect value of this address is older, hence stale.
        if let Some(i) = self
            .items
            .iter()
            .position(|q| q.kind != Kind::Step && q.message.address == message.address)
        {
            let old = self.items.remove(i);
            if old.kind == kind {
                seq = Some(old.seq);
            }
        }
        let seq = seq.unwrap_or_else(|| {
            self.seq += 1;
            self.seq
        });
        self.items.push(Queued {
            message,
            kind,
            queued_at: now,
            reduced,
            position,
            seq,
        });
    }

    /// Drop every waiting message `gone` says is not wanted any more (the operator took its
    /// fixture over): it takes no 5 ms slot, the others do not wait for it.
    pub fn drop_where(&mut self, gone: &dyn Fn(&OscMessage) -> bool) -> usize {
        let before = self.items.len();
        self.items.retain(|q| !gone(&q.message));
        before - self.items.len()
    }

    /// Take the next message to send (highest class, oldest first), ignoring the clock.
    pub fn pop(&mut self) -> Option<Queued> {
        let mut best = self
            .items
            .iter()
            .enumerate()
            .min_by_key(|(_, q)| (rank(q.kind), q.seq))
            .map(|(i, _)| i)?;
        if self.items[best].kind == Kind::Step && self.steps_in_a_row + 1 >= RAMP_SHARE_EVERY {
            // The ramp share: after three steps in a row the oldest waiting ramp value goes.
            if let Some((i, _)) = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, q)| q.kind == Kind::Ramp)
                .min_by_key(|(_, q)| q.seq)
            {
                best = i;
            }
        }
        let item = self.items.remove(best);
        self.steps_in_a_row = if item.kind == Kind::Step {
            self.steps_in_a_row + 1
        } else {
            0
        };
        Some(item)
    }

    /// The earliest time a packet may leave, not before `now`.
    pub fn next_slot(&self, now: f64) -> f64 {
        self.next_free.max(now)
    }

    /// The next message when the 5 ms spacing allows one at `now`, spending that slot.
    pub fn pop_due(&mut self, now: f64) -> Option<Queued> {
        if now + EPSILON < self.next_free {
            return None;
        }
        let item = self.pop()?;
        self.next_free = now.max(self.next_free) + PACKET_GAP_SECONDS;
        Some(item)
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

/// How loaded the engine was during a simulation (see [`simulate_with_load`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadReport {
    /// Seconds of the simulation in which the engine said the song is very busy.
    pub busy_seconds: f64,
    /// The lowest `served_percent` seen after any tick.
    pub min_served_percent: u8,
    /// The load after the last tick.
    pub last: ReplayLoad,
}

/// Run the engine on a simulated clock from `from` to `to` (song seconds; the clock and the song
/// position agree, the position being `lead` ahead): tick when the engine asks (on time, not
/// after the queue has drained), send through a [`PacingQueue`] 5 ms apart like the plugin,
/// return every packet. No sockets, no sleeping.
/// `yielded(now, address)` says whether the operator has that address's fixture at `now`.
pub fn simulate(
    engine: &mut ReplayEngine,
    from: f64,
    to: f64,
    lead: f64,
    yielded: &dyn Fn(f64, &str) -> bool,
) -> Vec<SentPacket> {
    simulate_with_load(engine, from, to, lead, yielded).0
}

/// [`simulate`], and how loaded the engine said it was while it ran.
pub fn simulate_with_load(
    engine: &mut ReplayEngine,
    from: f64,
    to: f64,
    lead: f64,
    yielded: &dyn Fn(f64, &str) -> bool,
) -> (Vec<SentPacket>, LoadReport) {
    // Send what the 5 ms spacing allows before `until` (all of it when `None`); everything
    // waiting was queued at or before `avail`.
    fn send_until(
        queue: &mut PacingQueue,
        avail: f64,
        until: Option<f64>,
        lead: f64,
        yielded: &dyn Fn(f64, &str) -> bool,
        out: &mut Vec<SentPacket>,
    ) {
        while !queue.is_empty() {
            let at = queue.next_slot(avail);
            if until.is_some_and(|t| at >= t) {
                break;
            }
            queue.drop_where(&|m| yielded(at, &m.address));
            if queue.is_empty() {
                break;
            }
            let Some(item) = queue.pop_due(at) else {
                break;
            };
            out.push(SentPacket {
                at,
                position: item.position.unwrap_or(item.queued_at + lead),
                message: item.message,
                kind: item.kind,
                reduced: item.reduced,
            });
        }
    }

    let mut out = Vec::new();
    let mut queue = PacingQueue::new();
    let mut now = from;
    let mut report = LoadReport {
        busy_seconds: 0.0,
        min_served_percent: 100,
        last: ReplayLoad::default(),
    };
    while now < to {
        let tick = engine.tick(now, now + lead, &|a| yielded(now, a));
        for ((message, kind), position) in tick
            .messages
            .into_iter()
            .zip(tick.kinds)
            .zip(tick.positions)
        {
            let reduced = tick.reduced && kind == Kind::Effect;
            let early = (position - (now + lead)).abs() > EPSILON;
            queue.push_for(message, kind, now, reduced, early.then_some(position));
        }
        report.last = engine.load();
        report.min_served_percent = report.min_served_percent.min(report.last.served_percent);
        let Some(wake) = tick.wake_in else {
            break;
        };
        if report.last.busy {
            report.busy_seconds += wake.min(to - now);
        }
        send_until(&mut queue, now, Some(now + wake), lead, yielded, &mut out);
        now += wake;
    }
    send_until(&mut queue, now, None, lead, yielded, &mut out);
    (out, report)
}
