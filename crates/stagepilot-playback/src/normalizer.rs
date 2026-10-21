//! Port of `backend/src/stagepilot/plugins/playback_api/normalizer.py`.
//!
//! Wire observations in, typed events out. The state machine is a line-by-line
//! translation so behaviour (including the provisional/settle logic of the fast
//! transport path) stays identical; see `tests/replay.rs` for the proof.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "PlaybackEventType")]
pub enum EventType {
    #[serde(rename = "state.snapshot")]
    StateSnapshot,
    #[serde(rename = "setlist.changed")]
    SetlistChanged,
    #[serde(rename = "song.selected")]
    SongSelected,
    #[serde(rename = "song.changed")]
    SongChanged,
    #[serde(rename = "song.ended")]
    SongEnded,
    #[serde(rename = "song.started")]
    SongStarted,
    #[serde(rename = "song.resumed")]
    SongResumed,
    #[serde(rename = "song.paused")]
    SongPaused,
    #[serde(rename = "song.stopped")]
    SongStopped,
    #[serde(rename = "transport.reverted")]
    TransportReverted,
    #[serde(rename = "seek")]
    Seek,
    #[serde(rename = "section.jump")]
    SectionJump,
    #[serde(rename = "section.loop")]
    SectionLoop,
    #[serde(rename = "fade.out")]
    FadeOut,
    #[serde(rename = "fade.in")]
    FadeIn,
    #[serde(rename = "pad.requested")]
    PadRequested,
    #[serde(rename = "pad.on")]
    PadOn,
    #[serde(rename = "pad.off")]
    PadOff,
    #[serde(rename = "position.jump")]
    PositionJump,
    #[serde(rename = "message.unknown")]
    MessageUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heartbeat {
    pub song_id: i64,
    pub position: f64,
    pub playing: bool,
    pub pad: bool,
    pub setlist_version: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlaybackEvent {
    #[serde(rename = "type")]
    pub kind: EventType,
    pub timestamp: f64,
    #[serde(default)]
    pub song_id: Option<i64>,
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub previous_song_id: Option<i64>,
    #[serde(default)]
    pub continues_playing: bool,
    #[serde(default)]
    pub playing: Option<bool>,
    #[serde(default)]
    pub pad: Option<bool>,
    #[serde(default)]
    pub setlist_version: Option<i64>,
    #[serde(default)]
    pub previous_version: Option<i64>,
    #[serde(default)]
    pub section_id: Option<i64>,
    #[serde(default)]
    pub active: Option<bool>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub message_kind: Option<String>,
    #[serde(default)]
    pub provisional: bool,
}

impl PlaybackEvent {
    fn new(kind: EventType, timestamp: f64, song_id: Option<i64>) -> Self {
        PlaybackEvent {
            kind,
            timestamp,
            song_id,
            position: None,
            previous_song_id: None,
            continues_playing: false,
            playing: None,
            pad: None,
            setlist_version: None,
            previous_version: None,
            section_id: None,
            active: None,
            reason: None,
            message_kind: None,
            provisional: false,
        }
    }
    fn pos(mut self, p: f64) -> Self {
        self.position = Some(p);
        self
    }
    fn why(mut self, r: &str) -> Self {
        self.reason = Some(r.to_string());
        self
    }
    fn prov(mut self) -> Self {
        self.provisional = true;
        self
    }
}

/// Python `integer()`: an int that is not a bool. (JSON integers beyond i64 are
/// Python ints but not `i64` here: a documented, unobserved-on-the-wire difference.)
fn integer(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Number(n)) => n.as_i64(),
        _ => None,
    }
}

/// Python `_number()`: finite, non-negative int/float.
fn number(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64().filter(|f| f.is_finite() && *f >= 0.0),
        _ => None,
    }
}

fn boolean(v: Option<&Value>) -> Option<bool> {
    match v {
        Some(Value::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn object(v: Option<&Value>) -> Option<&Map<String, Value>> {
    match v {
        Some(Value::Object(m)) => Some(m),
        _ => None,
    }
}

/// Python `parse_message()`: exactly one top-level key whose value is an object.
pub fn parse_message(raw: &str) -> Option<(String, Map<String, Value>)> {
    let decoded: Value = serde_json::from_str(raw).ok()?;
    let Value::Object(map) = decoded else {
        return None;
    };
    if map.len() != 1 {
        return None;
    }
    let (kind, value) = map.into_iter().next()?;
    match value {
        Value::Object(body) => Some((kind, body)),
        _ => None,
    }
}

pub fn parse_heartbeat(body: &Map<String, Value>) -> Option<Heartbeat> {
    let state = object(body.get("stateData"))?;
    let song = integer(state.get("setlistSongID"))?;
    let position = number(state.get("sequenceTime"))?;
    let transport = object(state.get("sequencerPlayState"))?;
    let pad = object(state.get("padPlayerPlayState"))?;
    let only = |m: &Map<String, Value>| -> Option<bool> {
        if m.len() != 1 {
            return None;
        }
        if m.contains_key("playing") {
            Some(true)
        } else if m.contains_key("stopped") {
            Some(false)
        } else {
            None
        }
    };
    Some(Heartbeat {
        song_id: song,
        position,
        playing: only(transport)?,
        pad: only(pad)?,
        setlist_version: integer(state.get("setlistCloudVersion")),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum SetlistId {
    Int(i64),
    Str(String),
}

pub struct Normalizer {
    pub heartbeat: Option<Heartbeat>,
    /// Incremented for every valid heartbeat (Python compares object identity).
    pub heartbeat_serial: u64,
    pub setlist_id: Option<SetlistId>,
    fast_transport: bool,
    expected: Option<bool>,
    expected_song: Option<i64>,
    pending: Vec<PlaybackEvent>,
    pending_timestamp: f64,
    pending_heartbeats: u32,
    armed_before: bool,
    armed: bool,
    selected: Option<(i64, f64)>,
    return_timestamp: f64,
    command_timestamp: f64,
    heartbeat_timestamp: f64,
}

const IGNORED: &[&str] = &[
    "transportNextSong",
    "transportPreviousSong",
    "mixerInfiniteLoop",
    "mixerLoop",
    "mixerMuteMIDI",
    "setlistSelectSongTransition",
    "contentUpdateSetlist",
    "audioDeviceChanged",
    "mixerTrackVolume",
    "mixerTrackMute",
    "mixerTrackSolo",
    "transportNavigateToSongMapElementIndex",
];

impl Normalizer {
    pub fn new(fast_transport: bool) -> Self {
        Normalizer {
            heartbeat: None,
            heartbeat_serial: 0,
            setlist_id: None,
            fast_transport,
            expected: None,
            expected_song: None,
            pending: Vec::new(),
            pending_timestamp: 0.0,
            pending_heartbeats: 0,
            armed_before: true,
            armed: true,
            selected: None,
            return_timestamp: f64::NEG_INFINITY,
            command_timestamp: f64::NEG_INFINITY,
            heartbeat_timestamp: 0.0,
        }
    }

    pub fn feed(&mut self, raw: &str, ts: f64) -> Vec<PlaybackEvent> {
        let Some((kind, body)) = parse_message(raw) else {
            return vec![];
        };
        let song = self.heartbeat.map(|h| h.song_id);
        match kind.as_str() {
            "heartbeat" => {
                let Some(current) = parse_heartbeat(&body) else {
                    return vec![];
                };
                let armed = self.armed;
                let events = self.on_heartbeat(current, ts);
                if !self.pending.is_empty() {
                    self.armed = armed;
                    return self.settle(current, ts, events);
                }
                events
            }
            "contentLoadSetlist" => {
                if self.heartbeat.is_none() {
                    return vec![];
                }
                let data = object(body.get("setlistData"));
                let identity = data.and_then(|d| d.get("setlistID"));
                match identity {
                    Some(Value::String(s)) if !s.is_empty() => {
                        self.setlist_id = Some(SetlistId::Str(s.clone()))
                    }
                    other => {
                        if let Some(i) = integer(other) {
                            self.setlist_id = Some(SetlistId::Int(i));
                        }
                    }
                }
                vec![]
            }
            "setlistSelectSong" => {
                let Some(selected) = integer(body.get("setlistSongID")) else {
                    return vec![];
                };
                self.selected = Some((selected, ts));
                let armed = self.armed;
                self.armed = true;
                let mut events =
                    vec![
                        PlaybackEvent::new(EventType::SongSelected, ts, Some(selected))
                            .why("select"),
                    ];
                if self.fast_transport
                    && self.heartbeat.is_some()
                    && Some(selected) != song
                    && self.playing_now()
                {
                    let pos = self.heartbeat.map(|h| h.position).unwrap_or(0.0);
                    events.extend(
                        self.provisional(
                            PlaybackEvent::new(EventType::SongStopped, ts, song)
                                .pos(pos)
                                .why("song-selected")
                                .prov(),
                            false,
                            Some(selected),
                            armed,
                        ),
                    );
                }
                events
            }
            "transportReturnToStart" => {
                self.return_timestamp = ts;
                let armed = self.armed;
                self.armed = true;
                if self.fast_transport
                    && self.heartbeat.is_some()
                    && (self.playing_now() || self.song_position(ts).1 > 0.0)
                {
                    let (s, _) = self.song_position(ts);
                    return self.provisional(
                        PlaybackEvent::new(EventType::SongStopped, ts, s)
                            .pos(0.0)
                            .why("return-to-start")
                            .prov(),
                        false,
                        s,
                        armed,
                    );
                }
                vec![]
            }
            "waveformSeek" => {
                let Some(position) = number(body.get("sequenceTime")) else {
                    return vec![];
                };
                self.command_timestamp = ts;
                self.armed = position < 2.0;
                vec![PlaybackEvent::new(EventType::Seek, ts, song).pos(position)]
            }
            "waveformDoubleTap" | "waveformLoop" => {
                self.command_timestamp = ts;
                let mut e = PlaybackEvent::new(
                    if kind == "waveformDoubleTap" {
                        EventType::SectionJump
                    } else {
                        EventType::SectionLoop
                    },
                    ts,
                    song,
                );
                e.section_id = integer(body.get("setlistSongSectionID"));
                e.active = boolean(body.get("active"));
                vec![e]
            }
            "transportFade" => match integer(body.get("direction")) {
                Some(1) => vec![PlaybackEvent::new(EventType::FadeOut, ts, song)],
                Some(0) => vec![PlaybackEvent::new(EventType::FadeIn, ts, song)],
                _ => vec![],
            },
            "transportPad" => {
                let mut e = PlaybackEvent::new(EventType::PadRequested, ts, song);
                e.active = boolean(body.get("playing"));
                vec![e]
            }
            "transportPlay" => {
                let want = boolean(body.get("playing"));
                let (true, Some(_), Some(want)) = (self.fast_transport, self.heartbeat, want)
                else {
                    return vec![];
                };
                if want == self.playing_now() {
                    return vec![];
                }
                let (s, position) = self.song_position(ts);
                let armed = self.armed;
                let started = armed && position < 2.0;
                let kind = if want {
                    if started {
                        EventType::SongStarted
                    } else {
                        EventType::SongResumed
                    }
                } else {
                    EventType::SongPaused
                };
                if want {
                    self.armed = false;
                }
                let mut e = PlaybackEvent::new(kind, ts, s).pos(position).prov();
                if want {
                    e = e.why("play");
                }
                self.provisional(e, want, s, armed)
            }
            k if IGNORED.contains(&k) => {
                if k == "transportNavigateToSongMapElementIndex" {
                    self.command_timestamp = ts;
                }
                vec![]
            }
            _ => {
                let mut e = PlaybackEvent::new(EventType::MessageUnknown, ts, song);
                e.message_kind = Some(kind);
                vec![e]
            }
        }
    }

    fn song_position(&self, ts: f64) -> (Option<i64>, f64) {
        let Some(hb) = self.heartbeat else {
            return (None, 0.0);
        };
        if let Some((sel, at)) = self.selected {
            if sel != hb.song_id && ts - at < 5.0 {
                return (Some(sel), 0.0);
            }
        }
        if self.pending.last().and_then(|e| e.reason.as_deref()) == Some("return-to-start") {
            return (Some(hb.song_id), 0.0);
        }
        (Some(hb.song_id), hb.position)
    }

    fn playing_now(&self) -> bool {
        self.expected
            .unwrap_or_else(|| self.heartbeat.is_some_and(|h| h.playing))
    }

    fn provisional(
        &mut self,
        event: PlaybackEvent,
        expected: bool,
        song: Option<i64>,
        armed: bool,
    ) -> Vec<PlaybackEvent> {
        if self.pending.is_empty() {
            self.armed_before = armed;
            self.pending_timestamp = event.timestamp;
            self.pending_heartbeats = 0;
        }
        self.pending.push(event.clone());
        self.expected = Some(expected);
        self.expected_song = song;
        vec![event]
    }

    fn settle(
        &mut self,
        current: Heartbeat,
        ts: f64,
        events: Vec<PlaybackEvent>,
    ) -> Vec<PlaybackEvent> {
        let returning =
            self.pending.last().and_then(|e| e.reason.as_deref()) == Some("return-to-start");
        // Start/resume classifications may differ at heartbeat time: both promise playing.
        let key = |e: &PlaybackEvent| -> (EventType, Option<i64>) {
            let t = match e.kind {
                EventType::SongStopped | EventType::SongPaused if returning => {
                    EventType::SongStopped
                }
                EventType::SongStarted | EventType::SongResumed => EventType::SongStarted,
                other => other,
            };
            (t, e.song_id)
        };
        let mut pending: Vec<_> = self.pending.iter().map(key).collect();
        let mut kept = Vec::new();
        for event in events {
            let m = key(&event);
            if let Some(i) = pending.iter().position(|p| *p == m) {
                pending.remove(i);
            } else {
                kept.push(event);
            }
        }
        if Some(current.playing) == self.expected
            && Some(current.song_id) == self.expected_song
            && (!returning || current.position == 0.0)
        {
            self.pending.clear();
            self.expected = None;
        } else {
            self.pending_heartbeats += 1;
            if self.pending_heartbeats >= 2 && ts - self.pending_timestamp >= 3.0 {
                let started = self
                    .pending
                    .iter()
                    .any(|e| e.kind == EventType::SongStarted);
                self.pending.clear();
                self.expected = None;
                self.armed = self.armed_before;
                let mut e =
                    PlaybackEvent::new(EventType::TransportReverted, ts, Some(current.song_id))
                        .pos(current.position);
                e.playing = Some(current.playing);
                if started {
                    e = e.why("song.started");
                }
                kept.push(e);
            }
        }
        kept
    }

    fn on_heartbeat(&mut self, current: Heartbeat, ts: f64) -> Vec<PlaybackEvent> {
        let previous = self.heartbeat.replace(current);
        self.heartbeat_serial += 1;
        let previous_ts = std::mem::replace(&mut self.heartbeat_timestamp, ts);
        let Some(previous) = previous else {
            self.armed = current.position < 2.0;
            let mut e = PlaybackEvent::new(EventType::StateSnapshot, ts, Some(current.song_id))
                .pos(current.position);
            e.playing = Some(current.playing);
            e.pad = Some(current.pad);
            e.setlist_version = current.setlist_version;
            return vec![e];
        };
        let mut events = Vec::new();
        if current.setlist_version != previous.setlist_version {
            let mut e = PlaybackEvent::new(EventType::SetlistChanged, ts, None);
            e.setlist_version = current.setlist_version;
            e.previous_version = previous.setlist_version;
            events.push(e);
        }
        let changed = current.song_id != previous.song_id;
        if changed {
            let selected = self
                .selected
                .is_some_and(|(s, at)| s == current.song_id && ts - at < 5.0);
            self.selected = None;
            if selected && previous.playing && !current.playing {
                events.push(
                    PlaybackEvent::new(EventType::SongStopped, ts, Some(previous.song_id))
                        .pos(previous.position)
                        .why("song-selected"),
                );
            }
            if !selected {
                if previous.playing {
                    events.push(
                        PlaybackEvent::new(EventType::SongEnded, ts, Some(previous.song_id))
                            .pos(previous.position),
                    );
                    let mut e =
                        PlaybackEvent::new(EventType::SongChanged, ts, Some(current.song_id))
                            .why("auto-advance");
                    e.previous_song_id = Some(previous.song_id);
                    e.continues_playing = current.playing;
                    events.push(e);
                } else {
                    events.push(
                        PlaybackEvent::new(EventType::SongSelected, ts, Some(current.song_id))
                            .why("navigation"),
                    );
                }
            }
            self.armed = true;
        }
        if current.playing && (!previous.playing || changed) {
            let started = (changed && previous.playing) || (self.armed && current.position < 2.0);
            events.push(
                PlaybackEvent::new(
                    if started {
                        EventType::SongStarted
                    } else {
                        EventType::SongResumed
                    },
                    ts,
                    Some(current.song_id),
                )
                .pos(current.position)
                .why(if changed && previous.playing {
                    "auto-advance"
                } else {
                    "play"
                }),
            );
            self.armed = false;
        } else if !current.playing && previous.playing && !changed {
            let stopped = current.position == 0.0 && ts - self.return_timestamp < 3.0;
            events.push(
                PlaybackEvent::new(
                    if stopped {
                        EventType::SongStopped
                    } else {
                        EventType::SongPaused
                    },
                    ts,
                    Some(current.song_id),
                )
                .pos(current.position),
            );
        } else if !current.playing
            && !previous.playing
            && !changed
            && current.position == 0.0
            && previous.position > 0.0
        {
            self.armed = true;
            events.push(
                PlaybackEvent::new(EventType::SongStopped, ts, Some(current.song_id)).pos(0.0),
            );
        }
        if current.pad != previous.pad {
            events.push(PlaybackEvent::new(
                if current.pad {
                    EventType::PadOn
                } else {
                    EventType::PadOff
                },
                ts,
                Some(current.song_id),
            ));
        }
        if current.playing
            && previous.playing
            && !changed
            && ts - self.command_timestamp > 2.0
            && (current.position - previous.position - (ts - previous_ts)).abs() > 1.5
        {
            events.push(
                PlaybackEvent::new(EventType::PositionJump, ts, Some(current.song_id))
                    .pos(current.position),
            );
        }
        events
    }
}
