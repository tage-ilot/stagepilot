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
//! * the ordered live state of the fixtures and the `clear` address that gives one back.
//!
//! Recording these changes into timeline rows is `crate::lightkey_looks`.
//!
//! The async shell is in `lightkey_link.rs` and `lightkey_relay.rs`.

use rosc::{OscBundle, OscPacket, OscTime, OscType};
use serde::{Deserialize, Serialize};
use stagepilot_core::config::OscArgument;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Relay protocol version this client understands (`/relay/hello` first argument).
pub const RELAY_VERSION: i32 = 1;
const MAX_ARGS: usize = 8;
const MAX_ADDRESS: usize = 200;
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

/// Whether a command can be replayed as a fixture override: a clean fixture address, at most
/// `MAX_ARGS` arguments, finite numbers.
pub fn is_replayable(command: &FixtureCommand) -> bool {
    command.fixture().is_some()
        && command.v.len() <= MAX_ARGS
        && command.v.iter().all(|a| match a {
            FixtureArg::Float(f) => f.is_finite(),
            _ => true,
        })
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

    /// The single number the state holds for `address` (case and %-encoding aside), if any.
    pub fn number(&self, address: &str) -> Option<f64> {
        let single = |c: &FixtureCommand| match c.v.as_slice() {
            [FixtureArg::Float(x)] => Some(*x),
            _ => None,
        };
        if let Some(c) = self.latest.get(address) {
            return single(c);
        }
        let wanted = crate::lightkey_operator::normalise_fixture(address);
        self.latest
            .iter()
            .find(|(a, _)| crate::lightkey_operator::normalise_fixture(a) == wanted)
            .and_then(|(_, c)| single(c))
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

/// How long a clock sample stays in the window (seconds). Short enough that crystal drift
/// (tens of ppm) cannot add up to more than a few milliseconds, long enough to span several
/// ping bursts.
pub const CLOCK_WINDOW_SECONDS: f64 = 600.0;
/// An estimate whose best round trip is above this is "poor": another burst is worth asking for.
pub const CLOCK_POOR_ROUND_TRIP: f64 = 0.2;
/// A later estimate that moves the clock by more than this re-dates the events recorded since.
pub const CLOCK_REDATE_THRESHOLD: f64 = 0.05;
const CLOCK_MAX_SAMPLES: usize = 256;

/// Clock samples in a sliding window; the one with the smallest round trip is the estimate. A
/// sample sent before a freeze and answered after it has a huge round trip (and an offset wrong
/// by half of it), so it can never beat a good sample, and a new ping burst never throws the
/// good estimate away: it only joins the window.
#[derive(Debug, Clone, Default)]
pub struct ClockEstimate {
    /// `(taken_at, round_trip, offset)`; `taken_at` is on the caller's monotonic clock.
    samples: Vec<(f64, f64, f64)>,
}

impl ClockEstimate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a sample taken at `now` (monotonic seconds). Returns true when the estimate (the
    /// lowest-round-trip sample in the window) changed.
    pub fn add(&mut self, now: f64, round_trip: f64, offset: f64) -> bool {
        if round_trip < 0.0 || !round_trip.is_finite() || !offset.is_finite() {
            return false;
        }
        let before = self.best();
        self.samples
            .retain(|(at, _, _)| now - at <= CLOCK_WINDOW_SECONDS);
        self.samples.push((now, round_trip, offset));
        if self.samples.len() > CLOCK_MAX_SAMPLES {
            self.samples.remove(0);
        }
        self.best() != before
    }

    /// `(round_trip, offset)` of the best sample in the window.
    pub fn best(&self) -> Option<(f64, f64)> {
        self.samples.iter().fold(
            None,
            |best: Option<(f64, f64)>, &(_, rtt, offset)| match best {
                Some((b, _)) if b <= rtt => best,
                _ => Some((rtt, offset)),
            },
        )
    }

    /// No estimate yet, or the best round trip is too long to trust to a few milliseconds.
    pub fn is_poor(&self) -> bool {
        self.best()
            .is_none_or(|(rtt, _)| rtt > CLOCK_POOR_ROUND_TRIP)
    }

    /// Forget everything (a new session with the agent).
    pub fn reset(&mut self) {
        self.samples.clear();
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
    fn clock_estimate_keeps_the_lowest_round_trip_in_a_window() {
        let mut c = ClockEstimate::new();
        assert!(c.add(0.0, 0.050, 30.020));
        assert!(c.add(0.1, 0.002, 30.000));
        assert!(!c.add(0.2, 0.010, 30.004));
        assert!(!c.add(0.3, -1.0, 1.0));
        assert_eq!(c.best(), Some((0.002, 30.000)));
        assert!(!c.is_poor());
        // a late answer (sent before a freeze) has a huge round trip and never wins
        assert!(!c.add(10.0, 13.1, 36.55));
        assert_eq!(c.best(), Some((0.002, 30.000)));
        // the good sample ages out of the window; then the next best takes over
        assert!(c.add(CLOCK_WINDOW_SECONDS + 5.0, 0.004, 30.001));
        assert_eq!(c.best(), Some((0.004, 30.001)));
        c.reset();
        assert_eq!(c.best(), None);
        assert!(c.is_poor());
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
    fn clear_address_names_the_fixture() {
        assert_eq!(clear_address("MH 1"), "/fixture/MH 1/overrides/clear");
    }
}
