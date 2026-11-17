//! The optional Playback library (read-only): what the helper on the Playback computer says about
//! the songs of a setlist, paired with the songs of the loaded service plan.
//!
//! This file holds everything that needs no network: reading the helper's JSON without ever
//! panicking (every field is optional, wrong types are ignored, text is cut to a fixed length),
//! the plain words for the helper's state, and the pairing of plan songs with Playback songs by
//! title. The HTTP side (timeouts, size cap, cache, routes) is `stagepilot-server`'s
//! `playback_library`. Nothing here is saved, and nothing here reaches the scan result, the song
//! order, the timers or any Playback command.

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use ts_rs::TS;

/// Songs read from one setlist (a service has a handful; the cap only guards odd answers).
pub const MAX_SETLIST_SONGS: usize = 200;
/// Longest text kept from the helper, in characters.
const MAX_TEXT: usize = 200;

// ---- what StagePilot reports ------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LibraryState {
    /// The connection is switched off; nothing is asked.
    Off,
    /// The helper answered and Playback's library can be read.
    Ready,
    /// The helper is up but macOS has not yet allowed it to read Playback ("not ready yet").
    Waiting,
    /// The helper has no setlist for the plan's date (the message says which one is newest).
    NoSetlist,
    /// Anything else: no address, no answer, a refusal, an answer that makes no sense.
    Problem,
}

/// What `GET /api/v1/playback-library/status` says.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct LibraryStatus {
    pub enabled: bool,
    /// As saved (`http://host[:port]`), empty when none.
    pub address: String,
    pub state: LibraryState,
    /// One plain sentence for the operator, never empty.
    pub message: String,
    /// The helper's version, when it answered.
    pub version: Option<String>,
    pub songs: Option<u32>,
    pub setlists: Option<u32>,
    /// True when Playback did not answer just now and this is the last good answer (under a minute old).
    pub stale: bool,
    /// When the helper last answered (RFC 3339).
    pub checked_at: Option<String>,
}

/// The last measure may start at most this many seconds before the end of the song; beyond that
/// the end of the song is the countdown length (Hunter, 2026-11-16).
pub const LAST_MEASURE_LIMIT_S: f64 = 6.0;

/// Where a song's countdown length comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LengthSource {
    /// The start of the song's last measure (`scan_length_s`).
    LastMeasure,
    /// The end of the song, because the song has no bar lines.
    SongEndNoGrid,
    /// The end of the song, because the last measure starts more than `LAST_MEASURE_LIMIT_S`
    /// before it (or, inconsistently, after it).
    SongEndLongLastMeasure,
}

/// Milliseconds, so two numbers are compared at 3 decimals and 6.0000000001 is 6.
fn millis(seconds: f64) -> i64 {
    (seconds * 1000.0).round() as i64
}

/// The one rule for a song's countdown length. `scan` is the start of the last measure and `end`
/// the end of the song, both already read safely (junk is `None`).
/// * both: the end when the last measure starts after it (inconsistent) or more than
///   `LAST_MEASURE_LIMIT_S` before it (a gap of exactly the limit keeps the last measure); else the last measure;
/// * only `scan`: `scan`; only `end` (no bar lines): `end`; neither: none.
pub fn countdown_length(scan: Option<f64>, end: Option<f64>) -> Option<(f64, LengthSource)> {
    match (scan, end) {
        (Some(scan), Some(end)) => {
            let gap = millis(end) - millis(scan);
            if gap < 0 || gap > millis(LAST_MEASURE_LIMIT_S) {
                Some((end, LengthSource::SongEndLongLastMeasure))
            } else {
                Some((scan, LengthSource::LastMeasure))
            }
        }
        (Some(scan), None) => Some((scan, LengthSource::LastMeasure)),
        (None, Some(end)) => Some((end, LengthSource::SongEndNoGrid)),
        (None, None) => None,
    }
}

/// The biggest cover picture the window will ever be sent, in bytes (the helper's are 8 to 77 KB).
pub const MAX_ARTWORK_BYTES: u32 = 2_000_000;
/// Sections kept per song (the longest real arrangement seen has 39).
pub const MAX_SECTIONS: usize = 200;
/// Longest section name kept, in characters.
const MAX_SECTION_NAME: usize = 40;
/// The latest second a section may name (a day).
const MAX_SECTION_SECONDS: f64 = 86_400.0;

/// The cover picture of a song, described. The picture itself is never in the plan: the window asks
/// for it on its own route (`/api/v1/playback-library/songs/<id>/artwork`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct LibraryArtwork {
    /// `image/jpeg` or `image/png`.
    #[serde(rename = "type")]
    #[ts(rename = "type")]
    pub content_type: String,
    pub bytes: u32,
    /// The helper's `ETag` for the picture, quotes included. A changed picture has a new one.
    pub etag: String,
}

/// One section of the arranged song, on Playback's played clock (seconds from the start).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct LibrarySection {
    pub name: String,
    pub start: f64,
    pub end: f64,
}

/// One song of a Playback setlist as far as StagePilot shows it.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct LibrarySong {
    #[ts(type = "number | null")]
    pub setlist_song_id: Option<i64>,
    /// Place in the Playback setlist, from 1.
    pub position: u32,
    pub title: String,
    /// The key it is played in, and the key of the original recording.
    pub key: Option<String>,
    pub key_original: Option<String>,
    pub bpm: Option<f64>,
    /// The start of the song's last measure, in seconds, which is where the song goes quiet (the
    /// helper's `scan_length_s`, the same number StagePilot's own scan measures). Null for a song
    /// with no beat grid.
    pub scan_length_s: Option<f64>,
    /// Where StagePilot's countdown for the song ends, in seconds, resolved by `countdown_length`:
    /// the start of the last measure, unless the song has no beat grid or that is more than
    /// `LAST_MEASURE_LIMIT_S` before the end of the song (the helper's `length_s`), in which case
    /// the end of the song; null when the helper gave neither. This is the one length the window
    /// shows and compares with the plan.
    pub countdown_length_s: Option<f64>,
    /// Which of the two `countdown_length_s` is (null when there is no length).
    pub length_source: Option<LengthSource>,
    /// The arrangement's name.
    pub arrangement: Option<String>,
    /// Number of sections of the arrangement as the helper counts them (needs the song's own
    /// answer; null when not read). `sections` below holds only the usable ones.
    pub section_count: Option<u32>,
    /// The arrangement in playing order (repeats are separate entries), from the song's own answer.
    /// Entries with odd numbers are left out; empty when none is usable or the answer was not read.
    pub sections: Vec<LibrarySection>,
    /// The cover picture, when the helper holds one (helper 0.2 and later).
    pub artwork: Option<LibraryArtwork>,
    /// When a rented song stops being available (RFC 3339), null when it is not a rental.
    pub rental_expires_utc: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PlanMatch {
    /// Exactly one song of the plan and exactly one of Playback have this title.
    Matched,
    /// Playback's setlist has no song with this title.
    Missing,
    /// The plan has this title more than once, so no Playback song can be told apart.
    DuplicateInPlan,
    /// Playback's setlist has this title more than once.
    DuplicateInPlayback,
}

/// One plan song, in plan order, with its Playback song when there is exactly one.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct LibraryPlanSong {
    /// The key the Lighting panel uses for this plan song (`source_song_id`, else `id`).
    pub key: String,
    pub title: String,
    /// Place among the plan's songs, from 1.
    pub plan_position: u32,
    /// The plan's length for the song in seconds.
    pub plan_seconds: Option<u32>,
    pub status: PlanMatch,
    pub library: Option<LibrarySong>,
}

/// A setlist in a few words.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct LibrarySetlist {
    pub title: String,
    /// `YYYY-MM-DD`, the date on the Playback computer.
    pub date: Option<String>,
    pub songs: u32,
}

/// What `GET /api/v1/playback-library/plan` says.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct LibraryPlan {
    pub enabled: bool,
    pub state: LibraryState,
    /// A plain sentence when the state is not `ready` (or the answer is old), else null.
    pub message: Option<String>,
    /// The loaded plan's date (`YYYY-MM-DD`), null when no plan is loaded.
    pub plan_date: Option<String>,
    /// Playback's setlist for that date, when it has one.
    pub setlist: Option<LibrarySetlist>,
    /// Playback's newest setlist, given when it has none for the date.
    pub newest: Option<LibrarySetlist>,
    /// One entry per plan song, in plan order. Empty with no plan or an empty plan.
    pub songs: Vec<LibraryPlanSong>,
    /// True when Playback did not answer just now and this is the last good answer (under a minute old).
    pub stale: bool,
    pub checked_at: Option<String>,
}

// ---- reading the helper's answers ------------------------------------------------------------

/// Text from the helper: control characters dropped, whitespace trimmed, cut to `MAX_TEXT` characters.
fn clean_text(value: &Value) -> Option<String> {
    let text: String = value
        .as_str()?
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT)
        .collect();
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn text_of(object: &Value, name: &str) -> Option<String> {
    clean_text(object.get(name)?)
}

fn whole_number(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(n) = value.as_i64() {
        return Some(n);
    }
    let f = value.as_f64()?;
    (f.is_finite() && f.fract() == 0.0 && f.abs() < 9.0e15).then_some(f as i64)
}

fn count_of(object: &Value, name: &str) -> Option<u32> {
    u32::try_from(whole_number(object.get(name))?).ok()
}

/// A positive, finite number in a believable range, else nothing.
fn number_in(object: &Value, name: &str, max: f64) -> Option<f64> {
    let n = object.get(name)?.as_f64()?;
    (n.is_finite() && n > 0.0 && n <= max).then_some(n)
}

fn key_name(object: &Value, name: &str) -> Option<String> {
    let key = object.get(name)?;
    match key {
        Value::String(_) => clean_text(key).map(|t| t.chars().take(12).collect()),
        _ => text_of(key, "name").map(|t| t.chars().take(12).collect()),
    }
}

/// An RFC 3339 time as the same instant in UTC, else nothing.
fn instant_of(object: &Value, name: &str) -> Option<String> {
    let text = text_of(object, name)?;
    let at = chrono::DateTime::parse_from_rfc3339(&text).ok()?;
    Some(
        at.with_timezone(&chrono::Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    )
}

/// `database` of `GET /health`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Database {
    Ok,
    /// The agent is up and macOS has not yet answered its one question.
    Waiting,
    Denied,
    Missing,
    Unreadable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    pub ok: bool,
    pub version: Option<String>,
    pub database: Database,
    pub detail: Option<String>,
    pub songs: Option<u32>,
    pub setlists: Option<u32>,
    /// The helper's `library_revision`: an opaque token that differs whenever Playback's database
    /// changed. Compared for equality only. Absent on helper 0.1.x, and then "not available".
    pub library_revision: Option<String>,
}

/// Reads `GET /health`. Anything odd gives `Database::Unknown`, never a panic.
pub fn parse_health(value: &Value) -> Health {
    let database = match value.get("database").and_then(Value::as_str) {
        Some("ok") => Database::Ok,
        Some("waiting") | Some("not read yet") => Database::Waiting,
        Some("denied") => Database::Denied,
        Some("missing") => Database::Missing,
        Some("unreadable") => Database::Unreadable,
        _ => Database::Unknown,
    };
    Health {
        ok: value.get("ok").and_then(Value::as_bool).unwrap_or(false),
        version: text_of(value, "version").map(|v| v.chars().take(40).collect()),
        database,
        detail: text_of(value, "detail"),
        songs: count_of(value, "songs"),
        setlists: count_of(value, "setlists"),
        library_revision: text_of(value, "library_revision").map(|v| v.chars().take(64).collect()),
    }
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The state and the one plain sentence for what `/health` said.
pub fn health_words(health: &Health) -> (LibraryState, String) {
    let detail = |fallback: &str| {
        health.detail.as_deref().map_or_else(
            || fallback.to_string(),
            |d| d.trim_end_matches('.').to_string(),
        )
    };
    match health.database {
        Database::Ok if health.ok => {
            let version = health
                .version
                .as_deref()
                .map_or(String::new(), |v| format!(" {v}"));
            let counts = match (health.songs, health.setlists) {
                (Some(songs), Some(setlists)) => format!(
                    ": {} in {}",
                    plural(songs, "song", "songs"),
                    plural(setlists, "setlist", "setlists")
                ),
                _ => String::new(),
            };
            (
                LibraryState::Ready,
                format!("Playback library{version} is ready{counts}."),
            )
        }
        Database::Waiting => (
            LibraryState::Waiting,
            "The Playback library is running but not ready yet: macOS has to be told to allow it. \
             Click Allow on the Playback computer, or wait a moment."
                .to_string(),
        ),
        Database::Denied => (
            LibraryState::Problem,
            format!(
                "macOS has not allowed the Playback library to read Playback. {}.",
                detail("Allow it on the Playback computer")
            ),
        ),
        Database::Missing => (
            LibraryState::Problem,
            format!(
                "The Playback library cannot find Playback's data on that computer. {}.",
                detail("Open Playback there once")
            ),
        ),
        Database::Unreadable => (
            LibraryState::Problem,
            format!(
                "The Playback library cannot read Playback's data right now. {}.",
                detail("Try again in a moment")
            ),
        ),
        _ => (
            LibraryState::Problem,
            "The Playback library answered, but not in a way StagePilot understands.".to_string(),
        ),
    }
}

/// One item of a setlist. The song's own answer adds the section count (`apply_detail`).
fn parse_song(item: &Value, index: usize) -> Option<LibrarySong> {
    if !item.is_object() {
        return None;
    }
    let title = text_of(item, "title")?;
    let scan_length_s = number_in(item, "scan_length_s", 86_400.0);
    let resolved = countdown_length(scan_length_s, number_in(item, "length_s", 86_400.0));
    Some(LibrarySong {
        setlist_song_id: whole_number(item.get("setlist_song_id")),
        position: count_of(item, "order")
            .filter(|n| *n > 0)
            .unwrap_or_else(|| u32::try_from(index + 1).unwrap_or(u32::MAX)),
        title,
        key: key_name(item, "key"),
        key_original: key_name(item, "key_original"),
        bpm: number_in(item, "bpm", 1000.0),
        scan_length_s,
        countdown_length_s: resolved.map(|(seconds, _)| seconds),
        length_source: resolved.map(|(_, source)| source),
        arrangement: text_of(item, "arrangement"),
        section_count: None,
        sections: Vec::new(),
        artwork: artwork_of(item),
        rental_expires_utc: instant_of(item, "rental_expires_utc"),
    })
}

/// A setlist as the helper gives it (`/setlists/<date>`, `/setlists/latest`).
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedSetlist {
    pub title: String,
    pub date: Option<String>,
    pub songs: Vec<LibrarySong>,
}

impl ParsedSetlist {
    pub fn summary(&self) -> LibrarySetlist {
        LibrarySetlist {
            title: self.title.clone(),
            date: self.date.clone(),
            songs: u32::try_from(self.songs.len()).unwrap_or(u32::MAX),
        }
    }
}

/// `YYYY-MM-DD` only (the helper's `performance_date`).
fn date_of(object: &Value, name: &str) -> Option<String> {
    let text = text_of(object, name)?;
    chrono::NaiveDate::parse_from_str(&text, "%Y-%m-%d")
        .ok()
        .map(|d| d.format("%Y-%m-%d").to_string())
}

/// Reads a setlist. `None` when the answer is not a setlist at all (no list of items).
pub fn parse_setlist(value: &Value) -> Option<ParsedSetlist> {
    let items = value.get("items")?.as_array()?;
    let songs = items
        .iter()
        .take(MAX_SETLIST_SONGS)
        .enumerate()
        .filter_map(|(index, item)| parse_song(item, index))
        .collect();
    Some(ParsedSetlist {
        title: text_of(value, "title").unwrap_or_else(|| "a setlist".to_string()),
        date: date_of(value, "performance_date"),
        songs,
    })
}

/// A section's time: finite, not negative, at most a day.
fn section_second(value: Option<&Value>) -> Option<f64> {
    let n = value?.as_f64()?;
    (n.is_finite() && (0.0..=MAX_SECTION_SECONDS).contains(&n)).then_some(n)
}

/// `arrangement_detail.sections[]` as `{name, start, end}` in playing order. An entry whose times are
/// missing, not numbers, negative, over a day or with its end not after its start is left out; a name
/// is trimmed and cut at 40 characters (a missing one is "Section"); at most 200 are kept.
pub fn parse_sections(sections: &Value) -> Vec<LibrarySection> {
    let Some(list) = sections.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|entry| {
            let start = section_second(entry.get("performed_start"))?;
            let end = section_second(entry.get("performed_end"))?;
            if end <= start {
                return None;
            }
            let name: String = entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>()
                .trim()
                .chars()
                .take(MAX_SECTION_NAME)
                .collect();
            let name = name.trim().to_string();
            Some(LibrarySection {
                name: if name.is_empty() {
                    "Section".to_string()
                } else {
                    name
                },
                start,
                end,
            })
        })
        .take(MAX_SECTIONS)
        .collect()
}

/// A song's `artwork` object (`{path, type, bytes, etag}`) as far as the window needs it. Anything
/// odd (not a JPEG or PNG, no etag or one that is not plain text, a size that is not a number or
/// over the cap) is no picture: a song without one shows no cover.
pub fn artwork_of(object: &Value) -> Option<LibraryArtwork> {
    let art = object.get("artwork")?;
    let content_type = text_of(art, "type")?.to_ascii_lowercase();
    if !matches!(content_type.as_str(), "image/jpeg" | "image/png") {
        return None;
    }
    let bytes = u32::try_from(whole_number(art.get("bytes"))?).ok()?;
    if bytes == 0 || bytes > MAX_ARTWORK_BYTES {
        return None;
    }
    let etag = art.get("etag")?.as_str()?;
    let plain = !etag.is_empty()
        && etag.len() <= 80
        && etag
            .bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && b != b',');
    plain.then(|| LibraryArtwork {
        content_type,
        bytes,
        etag: etag.to_string(),
    })
}

/// Adds what `/songs/<id>` knows to a song: the sections (their number and the usable ones), the
/// arrangement's name when the setlist did not give one, and the cover when the setlist item had
/// none. Odd answers change nothing.
pub fn apply_detail(song: &mut LibrarySong, detail: &Value) {
    if song.artwork.is_none() {
        song.artwork = artwork_of(detail);
    }
    let Some(arrangement) = detail.get("arrangement_detail") else {
        return;
    };
    if let Some(sections) = arrangement.get("sections").and_then(Value::as_array) {
        song.section_count = u32::try_from(sections.len()).ok();
        song.sections = parse_sections(&arrangement["sections"]);
    }
    if song.arrangement.is_none() {
        song.arrangement = text_of(arrangement, "title");
    }
}

// ---- titles -----------------------------------------------------------------------------------

/// The plain letter for an accented one (Latin letters; anything else is kept as it is).
fn fold(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'ç' | 'ć' | 'č' => "c",
        'ď' | 'đ' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
        'ğ' => "g",
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i",
        'ł' => "l",
        'ñ' | 'ń' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
        'ř' => "r",
        'š' | 'ś' | 'ş' => "s",
        'ť' | 'ț' => "t",
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => "u",
        'ý' | 'ÿ' => "y",
        'ž' | 'ź' | 'ż' => "z",
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        _ => return None,
    })
}

/// A trailing `(Live)`, `[Live at the Arena]`, `- Live` or `- Live at the Arena`, in lower case.
fn without_live_suffix(text: &str) -> &str {
    let text = text.trim();
    let is_live = |inner: &str| {
        let inner = inner.trim();
        inner == "live" || inner.starts_with("live ")
    };
    for (open, close) in [('(', ')'), ('[', ']')] {
        if text.ends_with(close) {
            if let Some(start) = text.rfind(open) {
                if is_live(&text[start + 1..text.len() - 1]) {
                    return text[..start].trim_end();
                }
            }
        }
    }
    let mut cut: Option<usize> = None;
    for dash in ["-", "–", "—"] {
        let marker = format!(" {dash} ");
        if let Some(start) = text.rfind(&marker) {
            if is_live(&text[start + marker.len()..]) {
                cut = Some(cut.map_or(start, |c: usize| c.max(start)));
            }
        }
    }
    cut.map_or(text, |start| text[..start].trim_end())
}

/// A title as compared: lower case, no accents, "&" as "and", no "(Live)" or "- Live" ending, no
/// apostrophes, every other punctuation mark and run of spaces one space. "A / B" and "A/B" agree.
pub fn normalise_title(title: &str) -> String {
    let lowered: String = title
        .chars()
        .take(MAX_TEXT * 2)
        .flat_map(char::to_lowercase)
        .collect();
    let stripped = without_live_suffix(&lowered);
    let stripped = if stripped.is_empty() {
        &lowered
    } else {
        stripped
    };
    let mut out = String::new();
    let mut gap = false;
    let push = |out: &mut String, gap: &mut bool, piece: &str| {
        if *gap && !out.is_empty() {
            out.push(' ');
        }
        *gap = false;
        out.push_str(piece);
    };
    for c in stripped.chars() {
        if ('\u{300}'..='\u{36f}').contains(&c) || matches!(c, '\'' | '’' | '‘' | '`' | '´') {
            continue;
        }
        if c == '&' {
            gap = true;
            push(&mut out, &mut gap, "and");
            gap = true;
        } else if let Some(plain) = fold(c) {
            push(&mut out, &mut gap, plain);
        } else if c.is_alphanumeric() {
            let mut buffer = [0u8; 4];
            push(&mut out, &mut gap, c.encode_utf8(&mut buffer));
        } else {
            gap = true;
        }
    }
    out
}

// ---- pairing ----------------------------------------------------------------------------------

/// A song of the loaded plan, as far as pairing needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTitle {
    pub key: String,
    pub title: String,
    pub seconds: Option<u32>,
}

/// Pairs plan songs with Playback songs by title. A pair needs exactly one song with that title on
/// each side; otherwise the plan song stays unpaired and says why. Neither list is reordered or
/// changed: the result has one entry per plan song, in plan order.
pub fn match_plan(plan: &[PlanTitle], playback: &[LibrarySong]) -> Vec<LibraryPlanSong> {
    let keys: Vec<String> = plan
        .iter()
        .map(|song| normalise_title(&song.title))
        .collect();
    let mut in_plan: HashMap<&str, usize> = HashMap::new();
    for key in keys.iter().filter(|k| !k.is_empty()) {
        *in_plan.entry(key.as_str()).or_default() += 1;
    }
    let mut in_playback: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, song) in playback.iter().enumerate() {
        let key = normalise_title(&song.title);
        if !key.is_empty() {
            in_playback.entry(key).or_default().push(index);
        }
    }
    plan.iter()
        .zip(&keys)
        .enumerate()
        .map(|(index, (song, key))| {
            let found = in_playback.get(key.as_str()).map_or(&[][..], Vec::as_slice);
            let (status, library) = if key.is_empty() || found.is_empty() {
                (PlanMatch::Missing, None)
            } else if in_plan.get(key.as_str()).copied().unwrap_or(0) > 1 {
                (PlanMatch::DuplicateInPlan, None)
            } else if found.len() > 1 {
                (PlanMatch::DuplicateInPlayback, None)
            } else {
                (PlanMatch::Matched, playback.get(found[0]).cloned())
            };
            LibraryPlanSong {
                key: song.key.clone(),
                title: song.title.clone(),
                plan_position: u32::try_from(index + 1).unwrap_or(u32::MAX),
                plan_seconds: song.seconds,
                status,
                library,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn song(title: &str, order: u32) -> LibrarySong {
        LibrarySong {
            setlist_song_id: Some(i64::from(order) + 100),
            position: order,
            title: title.into(),
            key: Some("G".into()),
            key_original: Some("G".into()),
            bpm: Some(72.0),
            scan_length_s: Some(300.0),
            countdown_length_s: Some(300.0),
            length_source: Some(LengthSource::LastMeasure),
            arrangement: None,
            section_count: None,
            sections: Vec::new(),
            artwork: None,
            rental_expires_utc: None,
        }
    }

    fn plan_song(title: &str) -> PlanTitle {
        PlanTitle {
            key: format!("key-{title}"),
            title: title.into(),
            seconds: Some(300),
        }
    }

    #[test]
    fn titles_that_mean_the_same_song_compare_equal() {
        let same = [
            ("Song One", "song one"),
            ("Song  One", "Song One"),
            ("Song One (Live)", "Song One"),
            ("Song One - Live", "Song One"),
            ("Song One – Live at the Arena", "Song One"),
            ("Song One [Live]", "Song One"),
            ("Song One (live at Somewhere)", "SONG ONE"),
            ("Faith & Glory", "Faith and Glory"),
            ("Faith And Glory", "faith & glory"),
            ("Don't Stop", "Dont Stop"),
            ("Don’t Stop", "Don't Stop"),
            ("Song One: Part 2!", "Song One Part 2"),
            ("Café Song", "Cafe Song"),
            ("Cafe\u{301} Song", "Café Song"),
            ("Señor Song", "Senor Song"),
            ("Song A / Song B", "Song A/Song B"),
            ("Song A / Song B (Live)", "song a  /  song b"),
            ("  Song One  ", "Song One"),
            ("Song-One", "Song One"),
        ];
        for (a, b) in same {
            assert_eq!(normalise_title(a), normalise_title(b), "{a:?} vs {b:?}");
            assert!(!normalise_title(a).is_empty(), "{a:?}");
        }
    }

    #[test]
    fn titles_that_are_different_songs_stay_different() {
        let different = [
            ("Song One", "Song Two"),
            ("Song One", "Song One Reprise"),
            ("Song A / Song B", "Song A"),
            ("Live Forever", "Forever"),
            ("Alive", "A"),
            ("Song One (Acoustic)", "Song One"),
            ("Song 1", "Song 2"),
        ];
        for (a, b) in different {
            assert_ne!(normalise_title(a), normalise_title(b), "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn a_title_that_is_only_live_keeps_its_words() {
        assert_eq!(normalise_title("Live"), "live");
        assert_eq!(normalise_title("(Live)"), "live");
        assert_eq!(normalise_title(""), "");
        assert_eq!(normalise_title("!!!"), "");
    }

    #[test]
    fn a_plan_pairs_with_a_setlist_by_title_and_never_reorders() {
        let plan = [
            plan_song("Song One"),
            plan_song("Faith & Glory"),
            plan_song("Song Three (Live)"),
            plan_song("Song Four"),
        ];
        let playback = [
            song("Song Three", 1),
            song("Faith and Glory", 2),
            song("Song One - Live", 3),
        ];
        let before = (plan.to_vec(), playback.to_vec());
        let matched = match_plan(&plan, &playback);
        assert_eq!((plan.to_vec(), playback.to_vec()), before);
        let titles: Vec<&str> = matched.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Song One",
                "Faith & Glory",
                "Song Three (Live)",
                "Song Four"
            ]
        );
        let status: Vec<PlanMatch> = matched.iter().map(|m| m.status).collect();
        assert_eq!(
            status,
            [
                PlanMatch::Matched,
                PlanMatch::Matched,
                PlanMatch::Matched,
                PlanMatch::Missing
            ]
        );
        assert_eq!(matched[0].library.as_ref().map(|l| l.position), Some(3));
        assert_eq!(matched[2].library.as_ref().map(|l| l.position), Some(1));
        let positions: Vec<u32> = matched.iter().map(|m| m.plan_position).collect();
        assert_eq!(positions, [1, 2, 3, 4]);
    }

    #[test]
    fn medleys_accents_and_the_same_song_twice() {
        let plan = [
            plan_song("Song A / Song B"),
            plan_song("Café Song"),
            plan_song("Song Twice"),
            plan_song("Song Twice"),
            plan_song("Song Doubled"),
            plan_song("Song Only Here"),
        ];
        let playback = [
            song("Song A/Song B", 1),
            song("Cafe Song", 2),
            song("Song Twice", 3),
            song("Song Doubled", 4),
            song("Song Doubled (Live)", 5),
        ];
        let status: Vec<PlanMatch> = match_plan(&plan, &playback)
            .iter()
            .map(|m| m.status)
            .collect();
        assert_eq!(
            status,
            [
                PlanMatch::Matched,
                PlanMatch::Matched,
                PlanMatch::DuplicateInPlan,
                PlanMatch::DuplicateInPlan,
                PlanMatch::DuplicateInPlayback,
                PlanMatch::Missing,
            ]
        );
    }

    #[test]
    fn the_same_song_twice_on_both_sides_is_not_matched() {
        let plan = [plan_song("Song Twice"), plan_song("Song Twice")];
        let playback = [song("Song Twice", 1), song("Song Twice", 2)];
        let result = match_plan(&plan, &playback);
        assert!(result
            .iter()
            .all(|m| m.status == PlanMatch::DuplicateInPlan));
        assert!(result.iter().all(|m| m.library.is_none()));
    }

    #[test]
    fn an_empty_plan_and_an_empty_setlist_pair_nothing() {
        assert!(match_plan(&[], &[song("Song One", 1)]).is_empty());
        let result = match_plan(&[plan_song("Song One")], &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, PlanMatch::Missing);
    }

    #[test]
    fn a_setlist_is_read_with_everything_optional() {
        let answer = json!({
            "title": "November 22nd", "performance_date": "2026-11-22",
            "items": [
                {"setlist_song_id": 2000001, "order": 1, "title": "Song One", "artist": "x",
                 "key": {"id": 3, "name": "Ab"}, "key_original": {"name": "C"}, "bpm": 65.0,
                 "length_s": 392.9, "scan_length_s": 388.64, "arrangement": "Default",
                 "rental_expires_utc": "2026-11-24T01:36:00+00:00"},
                {"title": "Song Two"},
                {"order": 3},
                "not an object",
                null
            ]
        });
        let parsed = parse_setlist(&answer).expect("a setlist");
        assert_eq!(parsed.title, "November 22nd");
        assert_eq!(parsed.date.as_deref(), Some("2026-11-22"));
        assert_eq!(parsed.songs.len(), 2);
        let one = &parsed.songs[0];
        assert_eq!(one.setlist_song_id, Some(2000001));
        assert_eq!(one.key.as_deref(), Some("Ab"));
        assert_eq!(one.key_original.as_deref(), Some("C"));
        assert_eq!(one.bpm, Some(65.0));
        assert_eq!(one.scan_length_s, Some(388.64));
        assert_eq!(one.countdown_length_s, Some(388.64));
        assert_eq!(one.length_source, Some(LengthSource::LastMeasure));
        assert_eq!(one.arrangement.as_deref(), Some("Default"));
        assert_eq!(
            one.rental_expires_utc.as_deref(),
            Some("2026-11-24T01:36:00Z")
        );
        let two = &parsed.songs[1];
        assert_eq!(two.setlist_song_id, None);
        assert_eq!(two.position, 2);
        assert_eq!(
            (two.key.clone(), two.bpm, two.scan_length_s),
            (None, None, None)
        );
    }

    /// The countdown length of one setlist item: (scan_length_s, countdown_length_s, length_source).
    fn lengths(item: Value) -> (Option<f64>, Option<f64>, Option<LengthSource>) {
        let mut item = item;
        item["title"] = json!("Song One");
        let song = parse_song(&item, 0).expect("a song");
        (
            song.scan_length_s,
            song.countdown_length_s,
            song.length_source,
        )
    }

    #[test]
    fn the_countdown_length_rule_as_a_table() {
        use LengthSource::{LastMeasure, SongEndLongLastMeasure, SongEndNoGrid};
        type Row = (Option<f64>, Option<f64>, Option<(f64, LengthSource)>);
        let table: [Row; 14] = [
            // both present, the gap is small: the start of the last measure.
            (Some(297.25), Some(300.5), Some((297.25, LastMeasure))),
            (Some(300.5), Some(300.5), Some((300.5, LastMeasure))),
            // a gap of exactly 6.0 keeps the last measure, 6.001 does not.
            (Some(294.5), Some(300.5), Some((294.5, LastMeasure))),
            (
                Some(294.499),
                Some(300.5),
                Some((300.5, SongEndLongLastMeasure)),
            ),
            // 6.0000000001 is 6 (compared at 3 decimals).
            (
                Some(294.4999999999),
                Some(300.5),
                Some((294.4999999999, LastMeasure)),
            ),
            // the last measure after the end of the song is inconsistent data: the end.
            (
                Some(301.0),
                Some(300.5),
                Some((300.5, SongEndLongLastMeasure)),
            ),
            (
                Some(300.501),
                Some(300.5),
                Some((300.5, SongEndLongLastMeasure)),
            ),
            // the real-world gaps are small: nothing changes.
            (Some(388.64), Some(392.9), Some((388.64, LastMeasure))),
            (Some(0.0), Some(5.0), Some((0.0, LastMeasure))),
            (
                Some(0.0),
                Some(6.001),
                Some((6.001, SongEndLongLastMeasure)),
            ),
            // only one side.
            (None, Some(300.5), Some((300.5, SongEndNoGrid))),
            (Some(297.25), None, Some((297.25, LastMeasure))),
            (None, None, None),
            (Some(0.0), None, Some((0.0, LastMeasure))),
        ];
        for (scan, end, expected) in table {
            assert_eq!(
                countdown_length(scan, end),
                expected,
                "scan {scan:?}, end {end:?}"
            );
        }
        assert_eq!(LAST_MEASURE_LIMIT_S, 6.0);
    }

    #[test]
    fn the_countdown_length_is_read_from_the_helper_with_junk_as_missing() {
        let none = (None, None, None);
        // both present and close: the last measure wins.
        assert_eq!(
            lengths(json!({"length_s": 300.5, "scan_length_s": 297.25})),
            (Some(297.25), Some(297.25), Some(LengthSource::LastMeasure))
        );
        // far apart (a long last measure): the end of the song.
        assert_eq!(
            lengths(json!({"length_s": 300.5, "scan_length_s": 290.0})),
            (
                Some(290.0),
                Some(300.5),
                Some(LengthSource::SongEndLongLastMeasure)
            )
        );
        // no beat grid, null or missing: the end of the song.
        let no_grid = (None, Some(300.5), Some(LengthSource::SongEndNoGrid));
        assert_eq!(
            lengths(json!({"length_s": 300.5, "scan_length_s": null})),
            no_grid
        );
        assert_eq!(lengths(json!({"length_s": 300.5})), no_grid);
        // both null or missing: no length.
        assert_eq!(
            lengths(json!({"length_s": null, "scan_length_s": null})),
            none
        );
        assert_eq!(lengths(json!({})), none);
        // a junk end of the song with no beat grid: no length; with a beat grid: the beat grid.
        for junk in [
            json!("300.5"),
            json!(-1),
            json!(1e300),
            json!([300]),
            json!(true),
            json!(null),
        ] {
            assert_eq!(
                lengths(json!({"length_s": junk, "scan_length_s": null})),
                none,
                "{junk}"
            );
            assert_eq!(
                lengths(json!({"length_s": junk, "scan_length_s": 297.25})),
                (Some(297.25), Some(297.25), Some(LengthSource::LastMeasure)),
                "{junk}"
            );
        }
        // a junk start of the last measure is the same as a missing one: the end of the song.
        for junk in [json!("297.25"), json!(-1), json!(1e300), json!({"a": 1})] {
            assert_eq!(
                lengths(json!({"length_s": 300.5, "scan_length_s": junk})),
                no_grid,
                "{junk}"
            );
        }
    }

    #[test]
    fn odd_answers_never_panic_and_give_nothing_rather_than_nonsense() {
        let huge = "x".repeat(1_000_000);
        let odd = [
            json!(null),
            json!([]),
            json!("text"),
            json!(42),
            json!({}),
            json!({"items": "no"}),
            json!({"items": [[], {}, 1, "a", {"title": 5}, {"title": []}]}),
            json!({"items": [{"title": huge, "key": huge, "bpm": "fast", "scan_length_s": -1, "length_s": 5.0,
                              "order": -4, "setlist_song_id": 1e300, "rental_expires_utc": "soon",
                              "arrangement": {"a": 1}}]}),
            json!({"items": [{"title": "T", "bpm": 1e308, "scan_length_s": f64::MAX, "order": 1e30,
                              "key": {"name": 7}, "key_original": [1, 2]}]}),
            json!({"title": huge, "performance_date": "2026-13-45", "items": []}),
            json!({"database": 7, "songs": "many", "setlists": -3, "version": huge, "ok": "yes"}),
        ];
        for value in &odd {
            let _ = parse_setlist(value);
            let health = parse_health(value);
            let _ = health_words(&health);
            let mut s = song("T", 1);
            apply_detail(&mut s, value);
        }
        let long = parse_setlist(&odd[7]).expect("a setlist").songs;
        assert_eq!(long.len(), 1);
        assert_eq!(long[0].title.chars().count(), MAX_TEXT);
        assert_eq!(long[0].bpm, None);
        assert_eq!(long[0].scan_length_s, None);
        // `length_s` 5.0 is a fine number, so a junk scan length falls back to the end of the song.
        assert_eq!(long[0].countdown_length_s, Some(5.0));
        assert_eq!(long[0].length_source, Some(LengthSource::SongEndNoGrid));
        assert_eq!(long[0].position, 1);
        assert_eq!(long[0].setlist_song_id, None);
        assert_eq!(long[0].rental_expires_utc, None);
        let big = parse_setlist(&odd[8]).expect("a setlist").songs;
        assert_eq!(
            (big[0].bpm, big[0].scan_length_s, big[0].position),
            (None, None, 1)
        );
        assert_eq!(
            (big[0].countdown_length_s, big[0].length_source),
            (None, None)
        );
        assert_eq!(parse_setlist(&odd[9]).expect("a setlist").date, None);
    }

    #[test]
    fn a_setlist_is_cut_to_the_cap() {
        let items: Vec<Value> = (0..500)
            .map(|n| json!({"title": format!("Song {n}"), "order": n + 1}))
            .collect();
        let parsed = parse_setlist(&json!({"items": items})).expect("a setlist");
        assert_eq!(parsed.songs.len(), MAX_SETLIST_SONGS);
    }

    #[test]
    fn a_song_answer_adds_sections_and_a_missing_arrangement_name() {
        let mut s = song("Song One", 1);
        apply_detail(
            &mut s,
            &json!({"arrangement_detail": {"title": "Short", "sections": [{}, {}, {}]}}),
        );
        assert_eq!(s.section_count, Some(3));
        assert_eq!(s.arrangement.as_deref(), Some("Short"));
        s.arrangement = Some("Default".into());
        apply_detail(
            &mut s,
            &json!({"arrangement_detail": {"title": "Other", "sections": []}}),
        );
        assert_eq!(s.section_count, Some(0));
        assert_eq!(s.arrangement.as_deref(), Some("Default"));
        apply_detail(&mut s, &json!({"arrangement_detail": {"sections": "many"}}));
        assert_eq!(s.section_count, Some(0));
    }

    #[test]
    fn the_helpers_state_is_told_in_plain_words() {
        let words = |v: Value| health_words(&parse_health(&v));
        let (state, text) = words(
            json!({"ok": true, "version": "0.1.1", "database": "ok", "songs": 406, "setlists": 86}),
        );
        assert_eq!(state, LibraryState::Ready);
        assert_eq!(
            text,
            "Playback library 0.1.1 is ready: 406 songs in 86 setlists."
        );
        let (_, one) = words(json!({"ok": true, "database": "ok", "songs": 1, "setlists": 1}));
        assert_eq!(one, "Playback library is ready: 1 song in 1 setlist.");
        let (state, text) = words(json!({"ok": true, "database": "waiting"}));
        assert_eq!(state, LibraryState::Waiting);
        assert!(text.contains("not ready yet") && !text.to_lowercase().contains("error"));
        assert_eq!(
            words(json!({"database": "not read yet"})).0,
            LibraryState::Waiting
        );
        for (database, detail) in [
            ("denied", "Allow it in System Settings."),
            ("missing", "Open Playback once."),
            ("unreadable", "Try later."),
        ] {
            let (state, text) = words(json!({"ok": false, "database": database, "detail": detail}));
            assert_eq!(state, LibraryState::Problem);
            assert!(text.contains(detail.trim_end_matches('.')), "{text}");
        }
        assert_eq!(
            words(json!({"ok": false, "database": "ok"})).0,
            LibraryState::Problem
        );
        assert_eq!(
            words(json!({"database": "strange"})).0,
            LibraryState::Problem
        );
    }

    // ---- sections and cover info (F-P1c) ----

    fn names(list: &[LibrarySection]) -> Vec<(&str, f64, f64)> {
        list.iter()
            .map(|s| (s.name.as_str(), s.start, s.end))
            .collect()
    }

    #[test]
    fn good_sections_keep_their_order_and_repeats() {
        let value = json!([
            {"name": "Count Off", "performed_start": 0.0, "performed_end": 3.5},
            {"name": "Verse 1", "performed_start": 3.5, "performed_end": 20},
            {"name": "Verse 1", "performed_start": 20, "performed_end": 36.25},
        ]);
        assert_eq!(
            names(&parse_sections(&value)),
            [
                ("Count Off", 0.0, 3.5),
                ("Verse 1", 3.5, 20.0),
                ("Verse 1", 20.0, 36.25)
            ]
        );
    }

    #[test]
    fn junk_sections_are_dropped_one_by_one() {
        let value = json!([
            {"name": "Missing end", "performed_start": 1.0},
            {"name": "Missing start", "performed_end": 1.0},
            {"name": "Text", "performed_start": "1", "performed_end": "2"},
            {"name": "Negative", "performed_start": -1.0, "performed_end": 2.0},
            {"name": "Too late", "performed_start": 1.0, "performed_end": 86_400.5},
            {"name": "Zero length", "performed_start": 5.0, "performed_end": 5.0},
            {"name": "Backwards", "performed_start": 9.0, "performed_end": 4.0},
            null, 7, "x", [],
            {"name": "Fine", "performed_start": 0.0, "performed_end": 86_400.0},
        ]);
        assert_eq!(names(&parse_sections(&value)), [("Fine", 0.0, 86_400.0)]);
        // Not a list at all, and a list of nothing usable: no sections, no bar.
        assert!(parse_sections(&json!({"a": 1})).is_empty());
        assert!(parse_sections(&json!(null)).is_empty());
        assert!(parse_sections(&json!([{"name": "x"}])).is_empty());
    }

    #[test]
    fn a_name_is_trimmed_cut_at_forty_characters_and_never_empty() {
        let long = format!("  {}  ", "é".repeat(60));
        let value = json!([
            {"name": long, "performed_start": 0.0, "performed_end": 1.0},
            {"name": "  Chorus \u{7}1 ", "performed_start": 1.0, "performed_end": 2.0},
            {"name": "   ", "performed_start": 2.0, "performed_end": 3.0},
            {"name": 5, "performed_start": 3.0, "performed_end": 4.0},
        ]);
        let got = parse_sections(&value);
        assert_eq!(got[0].name.chars().count(), 40);
        assert_eq!(got[1].name, "Chorus 1");
        assert_eq!(
            (got[2].name.as_str(), got[3].name.as_str()),
            ("Section", "Section")
        );
    }

    #[test]
    fn at_most_two_hundred_sections_are_kept() {
        let many: Vec<Value> = (0..500)
            .map(|n| json!({"name": format!("S{n}"), "performed_start": n as f64, "performed_end": n as f64 + 0.5}))
            .collect();
        let got = parse_sections(&Value::Array(many));
        assert_eq!(got.len(), MAX_SECTIONS);
        assert_eq!(got[199].name, "S199");
    }

    #[test]
    fn the_detail_adds_sections_and_counts_every_entry_even_the_dropped_ones() {
        let mut one = song("Song One", 1);
        apply_detail(
            &mut one,
            &json!({"arrangement_detail": {"title": "Short", "sections": [
                {"name": "Intro", "performed_start": 0.0, "performed_end": 4.0},
                {"name": "Broken"},
            ]}}),
        );
        assert_eq!(one.section_count, Some(2));
        assert_eq!(one.sections.len(), 1);
        assert_eq!(one.arrangement.as_deref(), Some("Short"));
        // A detail with no sections list leaves them as they were.
        apply_detail(&mut one, &json!({"arrangement_detail": {"title": "x"}}));
        assert_eq!((one.section_count, one.sections.len()), (Some(2), 1));
    }

    #[test]
    fn cover_info_needs_a_picture_type_a_size_in_range_and_a_plain_etag() {
        let ok = json!({"artwork": {"path": "/songs/1/artwork", "type": "image/jpeg", "bytes": 46874, "etag": "\"0123456789abcdef\""}});
        let art = artwork_of(&ok).unwrap();
        assert_eq!(
            (art.content_type.as_str(), art.bytes, art.etag.as_str()),
            ("image/jpeg", 46874, "\"0123456789abcdef\"")
        );
        assert_eq!(
            artwork_of(&json!({"artwork": {"type": "IMAGE/PNG", "bytes": 10, "etag": "W/\"a\""}}))
                .map(|a| a.content_type),
            Some("image/png".to_string())
        );
        for bad in [
            json!({}),
            json!({"artwork": null}),
            json!({"artwork": "x"}),
            json!({"artwork": {"type": "image/gif", "bytes": 10, "etag": "\"a\""}}),
            json!({"artwork": {"type": "text/html", "bytes": 10, "etag": "\"a\""}}),
            json!({"artwork": {"type": "image/png", "bytes": 0, "etag": "\"a\""}}),
            json!({"artwork": {"type": "image/png", "bytes": 2_000_001, "etag": "\"a\""}}),
            json!({"artwork": {"type": "image/png", "bytes": -4, "etag": "\"a\""}}),
            json!({"artwork": {"type": "image/png", "bytes": "10", "etag": "\"a\""}}),
            json!({"artwork": {"type": "image/png", "bytes": 10}}),
            json!({"artwork": {"type": "image/png", "bytes": 10, "etag": ""}}),
            json!({"artwork": {"type": "image/png", "bytes": 10, "etag": "a b"}}),
            json!({"artwork": {"type": "image/png", "bytes": 10, "etag": "a\nb"}}),
            json!({"artwork": {"type": "image/png", "bytes": 10, "etag": "x".repeat(81)}}),
        ] {
            assert_eq!(artwork_of(&bad), None, "{bad}");
        }
    }

    #[test]
    fn health_reads_the_revision_when_there_is_one() {
        let with = parse_health(
            &json!({"ok": true, "database": "ok", "library_revision": "a1b2c3d4e5f6"}),
        );
        assert_eq!(with.library_revision.as_deref(), Some("a1b2c3d4e5f6"));
        for none in [
            json!({"ok": true}),
            json!({"library_revision": 5}),
            json!({"library_revision": "  "}),
        ] {
            assert_eq!(parse_health(&none).library_revision, None);
        }
    }
}
