//! Lightkey fixture capture: pure logic for the `lightkey-api` OSC relay (no I/O, no clock, no tokio).
//!
//! `lightkey-api` 0.4 decodes Lightkey's DMX output into per-fixture state and streams every change
//! to subscribers as OSC bundles over UDP (`docs/relay.md` in that project). The bundle time tag
//! is the moment the change appeared on the Lightkey Mac's DMX output, on that Mac's clock. The
//! payload is Lightkey's own fixture OSC (`/fixture/MH1/overrides/panAngle 90.0`), so a stored
//! capture replays with no conversion.
//!
//! This file holds the rules, so they are testable without a runtime:
//!
//! * decoding a relay packet and a clock-ping answer,
//! * the clock offset from the lowest-round-trip sample,
//! * joining a command set that was split in parts, and finding lost packets,
//! * recording changes against song time and building the stored [`FixtureTrack`],
//! * the replay plan for a song position, and the commands that give fixtures back to the cues.
//!
//! The async shell is in `lightkey_link.rs` and `lightkey_relay.rs`.

use rosc::{OscBundle, OscPacket, OscTime, OscType};
use serde::{Deserialize, Serialize};
use stagepilot_core::config::OscArgument;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Relay protocol version this client understands (`/relay/hello` first argument).
pub const RELAY_VERSION: i32 = 1;
/// Seconds before a recorded change that the replay command is sent. The agent's own measurement
/// of OSC to first changed DMX frame is 40 to 50 ms; the take also stores the value it was made with.
pub const FIXTURE_LEAD_SECONDS: f64 = 0.045;
/// Most fixture changes one take can hold.
pub const MAX_FIXTURE_EVENTS: usize = 100_000;
/// Most commands one change (or the starting state) can hold.
pub const MAX_COMMANDS_PER_EVENT: usize = 2_048;
const MAX_ARGS: usize = 8;
const MAX_ADDRESS: usize = 200;
const MAX_SECONDS: f64 = 86_399.0;
/// 1 January 1900 to 1 January 1970, in seconds.
const NTP_UNIX_OFFSET: u64 = 2_208_988_800;

// ------------------------------------------------------------------------------------- model

/// One OSC argument as stored in a take and as the agent writes it: a JSON number with a decimal
/// point is a float, one without is an integer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FixtureArg {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

impl FixtureArg {
    pub fn to_osc(&self) -> OscArgument {
        match self {
            Self::Bool(v) => OscArgument::Bool(*v),
            Self::Int(v) => OscArgument::Int(*v),
            Self::Float(v) => OscArgument::Float(*v),
            Self::Text(v) => OscArgument::String(v.clone()),
        }
    }

    fn from_osc(value: &OscType) -> Option<Self> {
        Some(match value {
            OscType::Int(v) => Self::Int(i64::from(*v)),
            OscType::Long(v) => Self::Int(*v),
            OscType::Float(v) => Self::Float(f64::from(*v)),
            OscType::Double(v) => Self::Float(*v),
            OscType::String(v) => Self::Text(v.clone()),
            OscType::Bool(v) => Self::Bool(*v),
            _ => return None,
        })
    }
}

/// `{"a": "/fixture/MH1/overrides/panAngle", "v": [90.0], "approx": true}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureCommand {
    pub a: String,
    #[serde(default)]
    pub v: Vec<FixtureArg>,
}

impl FixtureCommand {
    pub fn new(a: impl Into<String>, v: Vec<FixtureArg>) -> Self {
        Self { a: a.into(), v }
    }

    /// The fixture this command addresses (`/fixture/<name>/overrides/<property>`), or `None` for
    /// any other address. A name with an OSC pattern character is refused: replay must never hit
    /// more than the one fixture that was recorded.
    pub fn fixture(&self) -> Option<&str> {
        fixture_of(&self.a)
    }

    pub fn osc_args(&self) -> Vec<OscArgument> {
        self.v.iter().map(FixtureArg::to_osc).collect()
    }
}

/// Fixture name of a `/fixture/<name>/overrides/<property>` address.
pub fn fixture_of(address: &str) -> Option<&str> {
    let rest = address.strip_prefix("/fixture/")?;
    let (name, tail) = rest.split_once('/')?;
    let property = tail.strip_prefix("overrides/")?;
    let clean = !name.is_empty()
        && !property.is_empty()
        && address.len() <= MAX_ADDRESS
        && !name.chars().any(is_pattern_or_control)
        && !property.chars().any(is_pattern_or_control);
    clean.then_some(name)
}

fn is_pattern_or_control(c: char) -> bool {
    matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | ',' | '#' | '\\') || (c as u32) < 32
}

/// `/fixture/<name>/overrides/clear`: gives every property of one fixture back to the cues.
pub fn clear_address(fixture: &str) -> String {
    format!("/fixture/{fixture}/overrides/clear")
}

/// The commands that changed at one moment, `at_seconds` into the song.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureEvent {
    pub at_seconds: f64,
    pub osc: Vec<FixtureCommand>,
}

/// Everything captured from the fixtures for one song. `initial` is the state at song time 0,
/// then `events` change it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FixtureTrack {
    #[serde(default)]
    pub initial: Vec<FixtureCommand>,
    #[serde(default)]
    pub events: Vec<FixtureEvent>,
    /// Seconds a command is sent before its time. The value the take was recorded with.
    #[serde(default = "default_lead")]
    pub lead_seconds: f64,
}

fn default_lead() -> f64 {
    FIXTURE_LEAD_SECONDS
}

/// Why a track is refused. Same shape as `TakeError` in `lightkey.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackError(pub String);

impl std::fmt::Display for TrackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TrackError {}

fn round_ms(seconds: f64) -> f64 {
    (seconds * 1000.0).round() / 1000.0
}

impl FixtureTrack {
    /// Check every command, round times to 1 ms and sort the events by time (stable).
    pub fn normalise_and_validate(&mut self) -> Result<(), TrackError> {
        let err = |m: &str| Err(TrackError(m.to_string()));
        if self.events.len() > MAX_FIXTURE_EVENTS {
            return err("A take can hold at most 100000 fixture changes.");
        }
        if !(self.lead_seconds.is_finite() && (0.0..=1.0).contains(&self.lead_seconds)) {
            return err("lead_seconds must be between 0 and 1.");
        }
        check_commands(&self.initial)?;
        for event in &mut self.events {
            event.at_seconds = round_ms(event.at_seconds);
            if !(event.at_seconds.is_finite() && (0.0..=MAX_SECONDS).contains(&event.at_seconds)) {
                return err("A fixture change time must be between 0 and 86399 seconds.");
            }
            if event.osc.is_empty() {
                return err("A fixture change must hold at least one command.");
            }
            check_commands(&event.osc)?;
        }
        self.events
            .sort_by(|a, b| a.at_seconds.total_cmp(&b.at_seconds));
        Ok(())
    }

    /// Names of the fixtures this track drives, sorted.
    pub fn fixtures(&self) -> Vec<String> {
        let mut names = BTreeSet::new();
        for command in self
            .initial
            .iter()
            .chain(self.events.iter().flat_map(|e| e.osc.iter()))
        {
            if let Some(name) = command.fixture() {
                names.insert(name.to_string());
            }
        }
        names.into_iter().collect()
    }
}

fn check_commands(commands: &[FixtureCommand]) -> Result<(), TrackError> {
    if commands.len() > MAX_COMMANDS_PER_EVENT {
        return Err(TrackError(
            "A fixture change can hold at most 2048 commands.".into(),
        ));
    }
    for command in commands {
        if command.fixture().is_none() {
            return Err(TrackError(format!(
                "Not a fixture override address: {}",
                command.a
            )));
        }
        if command.v.len() > MAX_ARGS {
            return Err(TrackError("A command can hold at most 8 arguments.".into()));
        }
        let finite = command.v.iter().all(|a| match a {
            FixtureArg::Float(f) => f.is_finite(),
            _ => true,
        });
        if !finite {
            return Err(TrackError("A command argument must be finite.".into()));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------- ordered state

/// Latest command per address, in the order each address was first seen (the agent sends
/// `dimmer` before `color`, and replay keeps that order).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FixtureState {
    order: Vec<String>,
    latest: HashMap<String, FixtureCommand>,
}

impl FixtureState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.latest.is_empty()
    }

    pub fn len(&self) -> usize {
        self.latest.len()
    }

    pub fn get(&self, address: &str) -> Option<&FixtureCommand> {
        self.latest.get(address)
    }

    /// Set one command. Returns true when it differs from what was there.
    pub fn apply(&mut self, command: &FixtureCommand) -> bool {
        match self.latest.get(&command.a) {
            Some(existing) if existing == command => false,
            Some(_) => {
                self.latest.insert(command.a.clone(), command.clone());
                true
            }
            None => {
                self.order.push(command.a.clone());
                self.latest.insert(command.a.clone(), command.clone());
                true
            }
        }
    }

    /// Apply many; returns only the commands that changed something.
    pub fn apply_all(&mut self, commands: &[FixtureCommand]) -> Vec<FixtureCommand> {
        commands.iter().filter(|c| self.apply(c)).cloned().collect()
    }

    /// Replace everything (a keyframe is the full state). Returns the commands that differ from
    /// before. A property that is no longer reported stays as it was: the agent lists every
    /// property of every fixture, so absence only means a fixture the project no longer has.
    pub fn replace_with(&mut self, commands: &[FixtureCommand]) -> Vec<FixtureCommand> {
        let mut fresh = FixtureState::new();
        fresh.apply_all(commands);
        let changed = commands
            .iter()
            .filter(|c| self.latest.get(&c.a) != Some(*c))
            .cloned()
            .collect();
        *self = fresh;
        changed
    }

    pub fn commands(&self) -> Vec<FixtureCommand> {
        self.order
            .iter()
            .filter_map(|a| self.latest.get(a).cloned())
            .collect()
    }

    /// The commands of one set of fixtures, in order.
    pub fn commands_of(&self, fixtures: &BTreeSet<String>) -> Vec<FixtureCommand> {
        self.commands()
            .into_iter()
            .filter(|c| c.fixture().is_some_and(|f| fixtures.contains(f)))
            .collect()
    }
}

// ------------------------------------------------------------------------------ relay packets

/// `/relay/seq` kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketKind {
    Event,
    Keyframe,
    Hello,
}

/// `/relay/hello version stream project fixtures map`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayHello {
    pub version: i32,
    pub stream: i64,
    pub project: String,
    pub fixtures: i64,
    pub map: i64,
}

/// One decoded relay packet.
#[derive(Debug, Clone, PartialEq)]
pub struct RelayPacket {
    /// The bundle time tag: Unix seconds on the Lightkey Mac's clock.
    pub time: f64,
    pub seq: i64,
    pub kind: PacketKind,
    pub part: i64,
    pub parts: i64,
    pub commands: Vec<FixtureCommand>,
    pub hello: Option<RelayHello>,
}

/// OSC time tag to Unix seconds.
pub fn time_tag_to_unix(tag: OscTime) -> f64 {
    let seconds = u64::from(tag.seconds) as f64 - NTP_UNIX_OFFSET as f64;
    seconds + f64::from(tag.fractional) / 4_294_967_296.0
}

/// Decode a datagram from the relay. Packets that are not a relay bundle are an error, never a
/// panic: the port is open to the network.
pub fn parse_packet(data: &[u8]) -> Result<RelayPacket, String> {
    let (_, packet) = rosc::decoder::decode_udp(data).map_err(|e| format!("not OSC: {e:?}"))?;
    let OscPacket::Bundle(OscBundle { timetag, content }) = packet else {
        return Err("not a bundle".into());
    };
    let mut elements = content.into_iter();
    let Some(OscPacket::Message(head)) = elements.next() else {
        return Err("no header".into());
    };
    if head.addr != "/relay/seq" {
        return Err("first element is not /relay/seq".into());
    }
    let (seq, kind_text, part, parts) = match head.args.as_slice() {
        [OscType::Int(s), OscType::String(k), OscType::Int(p), OscType::Int(n)] => {
            (i64::from(*s), k.as_str(), i64::from(*p), i64::from(*n))
        }
        _ => return Err("bad /relay/seq arguments".into()),
    };
    let kind = match kind_text {
        "event" => PacketKind::Event,
        "keyframe" => PacketKind::Keyframe,
        "hello" => PacketKind::Hello,
        other => return Err(format!("unknown kind {other}")),
    };
    if !(1..=64).contains(&parts) || !(1..=parts).contains(&part) {
        return Err("bad part numbers".into());
    }
    let mut commands = Vec::new();
    let mut hello = None;
    for element in elements {
        let OscPacket::Message(message) = element else {
            continue;
        };
        if message.addr == "/relay/hello" {
            if let [OscType::Int(version), OscType::Int(stream), OscType::String(project), OscType::Int(fixtures), OscType::Int(map)] =
                message.args.as_slice()
            {
                hello = Some(RelayHello {
                    version: *version,
                    stream: i64::from(*stream),
                    project: project.clone(),
                    fixtures: i64::from(*fixtures),
                    map: i64::from(*map),
                });
            }
            continue;
        }
        let args: Option<Vec<FixtureArg>> = message.args.iter().map(FixtureArg::from_osc).collect();
        let command = FixtureCommand::new(message.addr, args.ok_or("unsupported argument type")?);
        if command.fixture().is_some() && command.v.len() <= MAX_ARGS {
            commands.push(command);
        }
    }
    if kind == PacketKind::Hello && hello.is_none() {
        return Err("hello without /relay/hello".into());
    }
    Ok(RelayPacket {
        time: time_tag_to_unix(timetag),
        seq,
        kind,
        part,
        parts,
        commands,
        hello,
    })
}

/// `/relay/pong t1 t2 t3` (doubles): our send time, the agent's receive and send times.
pub fn parse_pong(data: &[u8]) -> Option<(f64, f64, f64)> {
    let (_, packet) = rosc::decoder::decode_udp(data).ok()?;
    let OscPacket::Message(message) = packet else {
        return None;
    };
    if message.addr != "/relay/pong" {
        return None;
    }
    match message.args.as_slice() {
        [OscType::Double(t1), OscType::Double(t2), OscType::Double(t3)] => Some((*t1, *t2, *t3)),
        _ => None,
    }
}

/// One clock sample (NTP style). `t1` we sent, `t2` the agent received, `t3` the agent replied,
/// `t4` we received. Returns `(round_trip, offset)` where `offset = agent_clock - our_clock`.
pub fn clock_sample(t1: f64, t2: f64, t3: f64, t4: f64) -> (f64, f64) {
    let round_trip = (t4 - t1) - (t3 - t2);
    let offset = ((t2 - t1) + (t3 - t4)) / 2.0;
    (round_trip, offset)
}

/// Keeps the sample with the smallest round trip.
#[derive(Debug, Clone, Default)]
pub struct ClockEstimate {
    best: Option<(f64, f64)>,
}

impl ClockEstimate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true when this sample became the best.
    pub fn add(&mut self, round_trip: f64, offset: f64) -> bool {
        if round_trip < 0.0 || !round_trip.is_finite() || !offset.is_finite() {
            return false;
        }
        if self.best.is_none_or(|(best, _)| round_trip < best) {
            self.best = Some((round_trip, offset));
            return true;
        }
        false
    }

    pub fn best(&self) -> Option<(f64, f64)> {
        self.best
    }

    /// Start a new burst. The previous offset stays in use until a new sample beats it.
    pub fn reset(&mut self) {
        self.best = None;
    }
}

/// What the tracker made of the next packet.
#[derive(Debug, Clone, PartialEq)]
pub enum Joined {
    /// The packet was one part of several, or a repeat; nothing to apply yet.
    Waiting,
    /// A complete command set.
    Complete {
        kind: PacketKind,
        time: f64,
        commands: Vec<FixtureCommand>,
    },
}

/// Per-subscriber sequence numbers and multi-part packets.
#[derive(Debug, Default)]
pub struct StreamTracker {
    stream: Option<i64>,
    last_seq: Option<i64>,
    gaps: u64,
    pending: Option<PendingSet>,
}

/// Parts of one split command set, waiting for the rest.
#[derive(Debug)]
struct PendingSet {
    first_seq: i64,
    kind: PacketKind,
    time: f64,
    parts: i64,
    got: BTreeMap<i64, Vec<FixtureCommand>>,
}

/// Sequence numbers count up by 1 and wrap here.
const SEQ_MODULUS: i64 = 1 << 31;

impl StreamTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn gaps(&self) -> u64 {
        self.gaps
    }

    pub fn stream(&self) -> Option<i64> {
        self.stream
    }

    /// A hello. Returns true when the stream id changed (the agent restarted) and everything
    /// learned from the old stream must go.
    pub fn on_hello(&mut self, hello: &RelayHello) -> bool {
        let restarted = self.stream.is_some_and(|s| s != hello.stream);
        if restarted {
            self.last_seq = None;
            self.pending = None;
        }
        self.stream = Some(hello.stream);
        restarted
    }

    /// Feed one packet. Returns what completed, and whether a packet was lost before it.
    pub fn on_packet(&mut self, packet: &RelayPacket) -> (Joined, bool) {
        // The reply to a subscribe is numbered 0 and sits outside the count. The hellos the relay
        // repeats every 2 s are numbered like any other packet, so they take part in the check.
        if packet.kind == PacketKind::Hello && packet.seq == 0 {
            return (Joined::Waiting, false);
        }
        let lost = match self.last_seq {
            Some(last) => packet.seq != (last + 1) % SEQ_MODULUS && packet.seq != last,
            None => false,
        };
        if packet.seq == self.last_seq.unwrap_or(-1) {
            return (Joined::Waiting, false); // a repeat
        }
        self.last_seq = Some(packet.seq);
        if lost {
            self.gaps += 1;
            self.pending = None;
        }
        if packet.kind == PacketKind::Hello {
            return (Joined::Waiting, lost);
        }
        if packet.parts == 1 {
            return (
                Joined::Complete {
                    kind: packet.kind,
                    time: packet.time,
                    commands: packet.commands.clone(),
                },
                lost,
            );
        }
        // Parts of one command set come one after the other with consecutive numbers.
        let first_seq = packet.seq - (packet.part - 1);
        let fresh = || PendingSet {
            first_seq,
            kind: packet.kind,
            time: packet.time,
            parts: packet.parts,
            got: BTreeMap::new(),
        };
        let entry = self.pending.get_or_insert_with(fresh);
        if entry.first_seq != first_seq || entry.parts != packet.parts {
            *entry = fresh();
        }
        entry.got.insert(packet.part, packet.commands.clone());
        let complete = i64::try_from(entry.got.len()).is_ok_and(|n| n == packet.parts);
        if !complete {
            return (Joined::Waiting, lost);
        }
        let Some(done) = self.pending.take() else {
            return (Joined::Waiting, lost);
        };
        let commands = done.got.into_values().flatten().collect();
        (
            Joined::Complete {
                kind: done.kind,
                time: done.time,
                commands,
            },
            lost,
        )
    }
}

// ------------------------------------------------------------------------------- recording

/// Turns timed command sets into a [`FixtureTrack`].
#[derive(Debug, Clone)]
pub struct FixtureRecorder {
    baseline: FixtureState,
    current: FixtureState,
    observed: Vec<(f64, Vec<FixtureCommand>)>,
}

impl FixtureRecorder {
    /// `baseline` is the fixture state when the song started.
    pub fn new(baseline: &FixtureState) -> Self {
        Self {
            baseline: baseline.clone(),
            current: baseline.clone(),
            observed: Vec::new(),
        }
    }

    pub fn events(&self) -> usize {
        self.observed.len()
    }

    /// Record the commands of one change at `position` seconds into the song. Commands that do
    /// not change anything are dropped. Returns how many were kept.
    pub fn observe(&mut self, position: f64, commands: &[FixtureCommand]) -> usize {
        if self.observed.len() >= MAX_FIXTURE_EVENTS {
            return 0;
        }
        let changed = self.current.apply_all(commands);
        let kept = changed.len();
        if kept > 0 {
            self.observed.push((position, changed));
        }
        kept
    }

    /// Build the track. Changes at or before time 0 become part of the starting state. With
    /// `all_fixtures` false (the default) the starting state names only fixtures that change
    /// during the song, so the fixtures a song never touches stay with the cues and the operator.
    /// Returns `None` when nothing changed: a constant look is not worth holding.
    pub fn finish(
        self,
        duration: f64,
        all_fixtures: bool,
    ) -> Result<Option<FixtureTrack>, TrackError> {
        let mut initial = self.baseline.clone();
        let mut events: Vec<FixtureEvent> = Vec::new();
        let mut ordered = self.observed;
        ordered.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (raw_time, commands) in ordered {
            let time = raw_time.max(0.0).min(duration);
            if time <= 0.0 {
                initial.apply_all(&commands);
                continue;
            }
            match events.last_mut() {
                Some(last) if (round_ms(time) - round_ms(last.at_seconds)).abs() < 0.0005 => {
                    last.osc.extend(commands);
                }
                _ => events.push(FixtureEvent {
                    at_seconds: time,
                    osc: commands,
                }),
            }
        }
        if events.is_empty() {
            return Ok(None);
        }
        let moved: BTreeSet<String> = events
            .iter()
            .flat_map(|e| e.osc.iter())
            .filter_map(|c| c.fixture().map(str::to_string))
            .collect();
        let initial = if all_fixtures {
            initial.commands()
        } else {
            initial.commands_of(&moved)
        };
        let mut track = FixtureTrack {
            initial,
            events,
            lead_seconds: FIXTURE_LEAD_SECONDS,
        };
        track.normalise_and_validate()?;
        Ok(Some(track))
    }
}

// --------------------------------------------------------------------------------- replay

/// What to send for a start or reposition at some song position.
#[derive(Debug, Clone, PartialEq)]
pub struct FixtureReplay {
    /// Force every managed fixture to its captured state at the position, at once.
    pub sync_now: Vec<FixtureCommand>,
    /// Changes after the position, in time order.
    pub later: Vec<FixtureEvent>,
    /// Every fixture the track drives: these are cleared when replay ends.
    pub fixtures: BTreeSet<String>,
}

/// Plan the replay of `track` from `position`.
pub fn fixture_replay_plan(track: &FixtureTrack, position: f64) -> FixtureReplay {
    let mut state = FixtureState::new();
    state.apply_all(&track.initial);
    let mut later = Vec::new();
    for event in &track.events {
        if event.at_seconds <= position {
            state.apply_all(&event.osc);
        } else {
            later.push(event.clone());
        }
    }
    FixtureReplay {
        sync_now: state.commands(),
        later,
        fixtures: track.fixtures().into_iter().collect(),
    }
}

// ------------------------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::{encoder, OscMessage};

    fn cmd(a: &str, v: Vec<FixtureArg>) -> FixtureCommand {
        FixtureCommand::new(a, v)
    }
    fn f(x: f64) -> FixtureArg {
        FixtureArg::Float(x)
    }
    fn dim(name: &str, x: f64) -> FixtureCommand {
        cmd(&format!("/fixture/{name}/overrides/dimmer"), vec![f(x)])
    }
    fn col(name: &str, r: f64, g: f64, b: f64) -> FixtureCommand {
        cmd(
            &format!("/fixture/{name}/overrides/color"),
            vec![f(r), f(g), f(b)],
        )
    }

    fn bundle(time: f64, head: [OscType; 4], messages: Vec<OscMessage>) -> Vec<u8> {
        let tag = OscTime {
            seconds: (time.trunc() as u64 + NTP_UNIX_OFFSET) as u32,
            fractional: ((time.fract()) * 4_294_967_296.0) as u32,
        };
        let mut content = vec![OscPacket::Message(OscMessage {
            addr: "/relay/seq".into(),
            args: head.to_vec(),
        })];
        content.extend(messages.into_iter().map(OscPacket::Message));
        encoder::encode(&OscPacket::Bundle(OscBundle {
            timetag: tag,
            content,
        }))
        .unwrap()
    }

    fn head(seq: i32, kind: &str, part: i32, parts: i32) -> [OscType; 4] {
        [
            OscType::Int(seq),
            OscType::String(kind.into()),
            OscType::Int(part),
            OscType::Int(parts),
        ]
    }

    fn msg(addr: &str, args: Vec<OscType>) -> OscMessage {
        OscMessage {
            addr: addr.into(),
            args,
        }
    }

    #[test]
    fn fixture_address_rules() {
        assert_eq!(fixture_of("/fixture/MH1/overrides/panAngle"), Some("MH1"));
        assert_eq!(
            fixture_of("/fixture/Pixel 3/overrides/color"),
            Some("Pixel 3")
        );
        for bad in [
            "/fixture/*/overrides/dimmer",
            "/fixture/P[1-4]/overrides/dimmer",
            "/fixture/P{1,3}/overrides/dimmer",
            "/fixture/P1/overrides/",
            "/fixture//overrides/dimmer",
            "/fixture/P1/dimmer",
            "/live/*/cue/Stars/activate",
            "/fixture/P1/overrides/a\u{1}b",
        ] {
            assert_eq!(fixture_of(bad), None, "{bad}");
        }
    }

    #[test]
    fn args_round_trip_through_json_keeping_float_and_int_apart() {
        let c = cmd(
            "/fixture/MH1/overrides/gobo1",
            vec![
                FixtureArg::Int(5),
                f(1.0),
                FixtureArg::Text("stopped".into()),
                FixtureArg::Bool(true),
            ],
        );
        let text = serde_json::to_string(&c).unwrap();
        assert_eq!(
            text,
            r#"{"a":"/fixture/MH1/overrides/gobo1","v":[5,1.0,"stopped",true]}"#
        );
        assert_eq!(serde_json::from_str::<FixtureCommand>(&text).unwrap(), c);
        // the agent also writes "approx": ignored, not an error
        let agent = r#"{"a":"/fixture/MH1/overrides/gobo1","v":[5],"approx":true}"#;
        assert_eq!(
            serde_json::from_str::<FixtureCommand>(agent).unwrap().v,
            vec![FixtureArg::Int(5)]
        );
    }

    #[test]
    fn parses_an_event_bundle_and_its_time_tag() {
        let data = bundle(
            1_792_650_000.250,
            head(7, "event", 1, 1),
            vec![msg(
                "/fixture/P1/overrides/color",
                vec![
                    OscType::Float(1.0),
                    OscType::Float(0.0),
                    OscType::Float(0.5),
                ],
            )],
        );
        let p = parse_packet(&data).unwrap();
        assert!((p.time - 1_792_650_000.250).abs() < 1e-6);
        assert_eq!(
            (p.seq, p.kind, p.part, p.parts),
            (7, PacketKind::Event, 1, 1)
        );
        assert_eq!(p.commands, vec![col("P1", 1.0, 0.0, 0.5)]);
    }

    #[test]
    fn parses_hello_and_rejects_junk_without_panicking() {
        let data = bundle(
            1.0,
            head(0, "hello", 1, 1),
            vec![msg(
                "/relay/hello",
                vec![
                    OscType::Int(1),
                    OscType::Int(99),
                    OscType::String("Show".into()),
                    OscType::Int(66),
                    OscType::Int(5),
                ],
            )],
        );
        let h = parse_packet(&data).unwrap().hello.unwrap();
        assert_eq!(
            (h.version, h.stream, h.project.as_str(), h.fixtures),
            (1, 99, "Show", 66)
        );
        assert!(parse_packet(b"").is_err());
        assert!(parse_packet(b"not osc at all").is_err());
        assert!(parse_packet(&bundle(1.0, head(1, "event", 2, 1), vec![])).is_err()); // part > parts
        assert!(parse_packet(&bundle(1.0, head(1, "bogus", 1, 1), vec![])).is_err());
        for n in 0..data.len() {
            let _ = parse_packet(&data[..n]); // truncated packets never panic
        }
    }

    #[test]
    fn drops_commands_that_are_not_fixture_overrides() {
        let data = bundle(
            1.0,
            head(1, "event", 1, 1),
            vec![
                msg("/live/*/cue/Stars/activate", vec![]),
                msg("/fixture/*/overrides/clear", vec![]),
                msg("/fixture/P1/overrides/dimmer", vec![OscType::Float(1.0)]),
            ],
        );
        assert_eq!(parse_packet(&data).unwrap().commands, vec![dim("P1", 1.0)]);
    }

    #[test]
    fn pong_and_clock_sample() {
        let data = encoder::encode(&OscPacket::Message(msg(
            "/relay/pong",
            vec![
                OscType::Double(10.0),
                OscType::Double(40.001),
                OscType::Double(40.002),
            ],
        )))
        .unwrap();
        assert_eq!(parse_pong(&data), Some((10.0, 40.001, 40.002)));
        assert_eq!(parse_pong(b"junk"), None);
        // the agent's clock is 30 s ahead; 2 ms each way
        let (rtt, offset) = clock_sample(10.0, 40.002, 40.003, 10.005);
        assert!((rtt - 0.004).abs() < 1e-9);
        assert!((offset - 30.0).abs() < 1e-9);
    }

    #[test]
    fn clock_estimate_keeps_the_lowest_round_trip() {
        let mut c = ClockEstimate::new();
        assert!(c.add(0.050, 30.020));
        assert!(c.add(0.002, 30.000));
        assert!(!c.add(0.010, 30.004));
        assert!(!c.add(-1.0, 1.0));
        assert_eq!(c.best(), Some((0.002, 30.000)));
        c.reset();
        assert_eq!(c.best(), None);
    }

    fn pk(
        seq: i64,
        kind: PacketKind,
        part: i64,
        parts: i64,
        cmds: Vec<FixtureCommand>,
    ) -> RelayPacket {
        RelayPacket {
            time: 5.0,
            seq,
            kind,
            part,
            parts,
            commands: cmds,
            hello: None,
        }
    }

    #[test]
    fn tracker_joins_parts_in_order_and_flags_a_missing_packet() {
        let mut t = StreamTracker::new();
        assert!(
            !t.on_packet(&pk(1, PacketKind::Event, 1, 1, vec![dim("P1", 1.0)]))
                .1
        );
        let (j, lost) = t.on_packet(&pk(2, PacketKind::Keyframe, 1, 2, vec![dim("P1", 0.5)]));
        assert_eq!(j, Joined::Waiting);
        assert!(!lost);
        let (j, lost) = t.on_packet(&pk(3, PacketKind::Keyframe, 2, 2, vec![dim("P2", 0.25)]));
        assert!(!lost);
        assert_eq!(
            j,
            Joined::Complete {
                kind: PacketKind::Keyframe,
                time: 5.0,
                commands: vec![dim("P1", 0.5), dim("P2", 0.25)]
            }
        );
        let (_, lost) = t.on_packet(&pk(5, PacketKind::Event, 1, 1, vec![]));
        assert!(lost);
        assert_eq!(t.gaps(), 1);
    }

    #[test]
    fn tracker_drops_a_split_set_when_a_part_is_lost() {
        let mut t = StreamTracker::new();
        t.on_packet(&pk(1, PacketKind::Event, 1, 3, vec![dim("P1", 1.0)]));
        let (j, lost) = t.on_packet(&pk(3, PacketKind::Event, 3, 3, vec![dim("P3", 1.0)]));
        assert!(lost);
        assert_eq!(j, Joined::Waiting); // part 2 never arrived: never apply a partial set
    }

    #[test]
    fn periodic_hellos_count_in_the_sequence_but_the_subscribe_reply_does_not() {
        let mut t = StreamTracker::new();
        let hello_packet = |seq| RelayPacket {
            hello: Some(RelayHello {
                version: 1,
                stream: 1,
                project: String::new(),
                fixtures: 3,
                map: 1,
            }),
            ..pk(seq, PacketKind::Hello, 1, 1, vec![])
        };
        assert!(!t.on_packet(&hello_packet(0)).1); // reply to subscribe: outside the count
        assert!(!t.on_packet(&pk(1, PacketKind::Keyframe, 1, 1, vec![])).1);
        assert!(!t.on_packet(&hello_packet(2)).1); // a periodic hello took number 2
        assert!(
            !t.on_packet(&pk(3, PacketKind::Event, 1, 1, vec![dim("P1", 1.0)]))
                .1
        );
        assert_eq!(t.gaps(), 0);
        assert!(t.on_packet(&hello_packet(5)).1); // 4 never arrived
    }

    #[test]
    fn tracker_ignores_repeats_wraps_and_resets_on_a_new_stream() {
        let mut t = StreamTracker::new();
        t.on_packet(&pk(SEQ_MODULUS - 1, PacketKind::Event, 1, 1, vec![]));
        assert!(!t.on_packet(&pk(0, PacketKind::Event, 1, 1, vec![])).1); // wrap is not a gap
        assert_eq!(
            t.on_packet(&pk(0, PacketKind::Event, 1, 1, vec![])).0,
            Joined::Waiting
        ); // repeat
        let hello = |s| RelayHello {
            version: 1,
            stream: s,
            project: String::new(),
            fixtures: 0,
            map: 0,
        };
        assert!(!t.on_hello(&hello(1)));
        assert!(!t.on_hello(&hello(1)));
        assert!(t.on_hello(&hello(2)));
        assert!(!t.on_packet(&pk(40, PacketKind::Event, 1, 1, vec![])).1); // numbering restarted
    }

    #[test]
    fn state_keeps_first_seen_order_and_reports_changes_only() {
        let mut s = FixtureState::new();
        assert_eq!(
            s.apply_all(&[dim("P1", 1.0), col("P1", 1.0, 0.0, 0.0)])
                .len(),
            2
        );
        assert!(s.apply_all(&[dim("P1", 1.0)]).is_empty());
        assert_eq!(s.apply_all(&[col("P1", 0.0, 1.0, 0.0)]).len(), 1);
        let order: Vec<String> = s.commands().into_iter().map(|c| c.a).collect();
        assert_eq!(
            order,
            vec![
                "/fixture/P1/overrides/dimmer",
                "/fixture/P1/overrides/color"
            ]
        );
        let changed = s.replace_with(&[dim("P1", 1.0), col("P1", 0.0, 1.0, 0.0), dim("P2", 0.5)]);
        assert_eq!(changed, vec![dim("P2", 0.5)]);
    }

    #[test]
    fn recorder_builds_initial_events_and_drops_untouched_fixtures() {
        let mut base = FixtureState::new();
        base.apply_all(&[dim("P1", 0.0), col("P1", 0.0, 0.0, 0.0), dim("P2", 0.5)]);
        let mut r = FixtureRecorder::new(&base);
        assert_eq!(r.observe(1.5, &[dim("P1", 1.0), dim("P2", 0.5)]), 1); // P2 did not change
        r.observe(4.0, &[col("P1", 1.0, 0.0, 0.0)]);
        r.observe(4.0004, &[col("P3", 0.0, 0.0, 1.0)]);
        let track = r.finish(10.0, false).unwrap().unwrap();
        assert_eq!(track.events.len(), 2);
        assert_eq!(track.events[1].osc.len(), 2); // two changes in the same millisecond share one event
        assert_eq!(track.fixtures(), vec!["P1", "P3"]);
        // P2 never changed: not in the starting state
        assert!(track.initial.iter().all(|c| c.fixture() == Some("P1")));
        let all = {
            let mut r = FixtureRecorder::new(&base);
            r.observe(1.0, &[dim("P1", 1.0)]);
            r.finish(10.0, true).unwrap().unwrap()
        };
        assert!(all.initial.iter().any(|c| c.fixture() == Some("P2")));
    }

    #[test]
    fn recorder_folds_time_zero_clamps_and_returns_none_when_nothing_moved() {
        let mut r = FixtureRecorder::new(&FixtureState::new());
        r.observe(-0.2, &[dim("P1", 1.0)]);
        assert!(r.finish(10.0, false).unwrap().is_none());
        let mut r = FixtureRecorder::new(&FixtureState::new());
        r.observe(0.0, &[dim("P1", 1.0)]);
        r.observe(99.0, &[dim("P1", 0.0)]);
        let track = r.finish(10.0, false).unwrap().unwrap();
        assert_eq!(track.initial, vec![dim("P1", 1.0)]);
        assert_eq!(track.events[0].at_seconds, 10.0);
    }

    #[test]
    fn track_validation() {
        let ok = |c: FixtureCommand| {
            FixtureTrack {
                initial: vec![c],
                events: vec![],
                lead_seconds: 0.045,
            }
            .normalise_and_validate()
        };
        assert!(ok(dim("P1", 1.0)).is_ok());
        assert!(ok(cmd("/fixture/*/overrides/dimmer", vec![f(1.0)])).is_err());
        assert!(ok(cmd("/live/x/cue/y/activate", vec![])).is_err());
        assert!(ok(cmd("/fixture/P1/overrides/dimmer", vec![f(f64::NAN)])).is_err());
        let mut t = FixtureTrack {
            initial: vec![],
            events: vec![
                FixtureEvent {
                    at_seconds: 5.0004,
                    osc: vec![dim("P1", 1.0)],
                },
                FixtureEvent {
                    at_seconds: 2.0,
                    osc: vec![dim("P1", 0.0)],
                },
            ],
            lead_seconds: 0.045,
        };
        t.normalise_and_validate().unwrap();
        assert_eq!(t.events[0].at_seconds, 2.0);
        assert_eq!(t.events[1].at_seconds, 5.0);
        t.events.push(FixtureEvent {
            at_seconds: 1.0,
            osc: vec![],
        });
        assert!(t.normalise_and_validate().is_err());
    }

    #[test]
    fn replay_plan_syncs_state_at_the_position_and_keeps_later_events() {
        let track = FixtureTrack {
            initial: vec![dim("P1", 0.0), col("P1", 0.0, 0.0, 0.0)],
            events: vec![
                FixtureEvent {
                    at_seconds: 2.0,
                    osc: vec![dim("P1", 1.0)],
                },
                FixtureEvent {
                    at_seconds: 6.0,
                    osc: vec![col("P1", 1.0, 0.0, 0.0), dim("P2", 0.5)],
                },
                FixtureEvent {
                    at_seconds: 9.0,
                    osc: vec![dim("P1", 0.0)],
                },
            ],
            lead_seconds: 0.045,
        };
        let plan = fixture_replay_plan(&track, 6.0);
        assert_eq!(
            plan.sync_now,
            vec![dim("P1", 1.0), col("P1", 1.0, 0.0, 0.0), dim("P2", 0.5)]
        );
        assert_eq!(plan.later.len(), 1);
        assert_eq!(
            plan.fixtures.iter().cloned().collect::<Vec<_>>(),
            vec!["P1", "P2"]
        );
        let start = fixture_replay_plan(&track, 0.0);
        assert_eq!(
            start.sync_now,
            vec![dim("P1", 0.0), col("P1", 0.0, 0.0, 0.0)]
        );
        assert_eq!(start.later.len(), 3);
        assert_eq!(clear_address("MH 1"), "/fixture/MH 1/overrides/clear");
    }
}
