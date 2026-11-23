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

/// Where the helper on the Playback computer (the "Library Link") stands. Only `ready` and
/// `outdated` let the library feed anything; every other state is quiet (no notice anywhere),
/// except that a helper that was ready and has gone silent (`unreachable`) may say so in the
/// header pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HelperState {
    /// Nothing answers at the helper's address and it was never paired.
    NotInstalled,
    /// It answers but refuses this StagePilot (no key, a different key, or a limit from an old install).
    NeedsKey,
    /// It answers and works but is older than the one bundled in StagePilot, or has no key set.
    Outdated,
    /// It answers to StagePilot's key and is the bundled version or newer.
    Ready,
    /// It was in use and does not answer now.
    Unreachable,
}

impl HelperState {
    /// The library is used (chips, arrangement, notices, the timeline song map) only in these.
    pub fn is_active(self) -> bool {
        matches!(self, HelperState::Ready | HelperState::Outdated)
    }
}

/// The helper's state in `GET /api/v1/playback-library/status`. The key is never part of it.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct HelperStatus {
    pub state: HelperState,
    /// The version the helper reported, when it answered.
    pub version: Option<String>,
    /// The version bundled in this StagePilot (what the install command installs).
    pub bundled_version: String,
    /// One plain sentence for the operator, never empty.
    pub message: String,
    /// A key exists (StagePilot made one when a setup session was first opened).
    pub paired: bool,
    /// The helper can install an update that StagePilot sends (helper 0.4.0 and newer say `"updater": 1` in `/health`). One that
    /// cannot is a bootstrap case: it is updated once by hand, with the setup command.
    pub updater: bool,
    /// Why the last automatic update did not stay (the helper put the old version back), when it says.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub last_update_error: Option<String>,
    /// What StagePilot is doing about an older helper (added by the route; left out when there is nothing to say).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(
        optional,
        as = "Option<stagepilot_core::helper_update::HelperUpdateStatus>"
    )]
    pub update: Option<stagepilot_core::helper_update::HelperUpdateStatus>,
}

/// What `GET /api/v1/playback-library/status` says.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct LibraryStatus {
    /// The library is in use: the helper is `ready` or `outdated`, or was and is silent now.
    pub enabled: bool,
    /// The Library Link's own state (`helper.state` is `ready` or `outdated`): the one gate for library features.
    pub active: bool,
    pub helper: HelperStatus,
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
    /// The song's MultiTracks number (the helper's `content_id`), which is also the number inside the song file ProPresenter
    /// has; null when the helper gave none. The ProPresenter Link finds the song in ProPresenter by it.
    #[ts(type = "number | null")]
    pub content_id: Option<u64>,
    /// Place in the Playback setlist, from 1.
    pub position: u32,
    pub title: String,
    /// The key it is played in, and the key of the original recording.
    pub key: Option<String>,
    pub key_original: Option<String>,
    /// The key it had BEFORE the last change StagePilot noticed in Playback (`key_watch`), with the moment it noticed
    /// (RFC 3339, UTC). Both null when no change was seen. Sent however old: the window decides how long it shows.
    pub key_was: Option<String>,
    pub key_changed_at: Option<String>,
    /// True while StagePilot holds this song's Playback waveform (it was asked for by a lighting timeline or by a Lightkey
    /// recording that was about to start). It is never read from the helper and never saved: the Lighting window uses it to
    /// hide the room loudness of a song that has a waveform. False means "not known to have one", not "has none".
    pub has_waveform: bool,
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
    /// Exactly one song of the plan and exactly one of Playback have this title (the same words, or the same
    /// words but for a bracket or a "feat." tail: see `MatchConfidence`).
    Matched,
    /// The titles are only alike (one starts with the other, or a letter is off): the Playback song is named in
    /// `candidate`, never used as the song's library.
    Uncertain,
    /// Playback's setlist has no song with this title.
    Missing,
    /// The plan has this title more than once, so no Playback song can be told apart.
    DuplicateInPlan,
    /// Playback's setlist has this title more than once.
    DuplicateInPlayback,
}

/// How sure the title match is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MatchConfidence {
    /// The same title once case, punctuation, accents, "&" and a "(Live)" ending are set aside.
    Exact,
    /// The same title once every bracket and a "feat." tail are also set aside.
    Close,
    /// Alike, not the same: shown with both titles for the operator to judge.
    Uncertain,
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
    /// The key the plan names for the song (Planning Center's item key), null when it names none.
    pub plan_key: Option<String>,
    pub status: PlanMatch,
    /// How sure the pairing is (`matched` is exact or close; `uncertain` is uncertain); null when unpaired.
    pub confidence: Option<MatchConfidence>,
    pub library: Option<LibrarySong>,
    /// The Playback song an `uncertain` pairing is about. Never a source of cover, key or waveform for anything.
    pub candidate: Option<LibrarySong>,
}

/// A setlist in a few words.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct LibrarySetlist {
    /// Playback's own number for the setlist (what `/setlists/<id>` takes); null when the helper gave none.
    pub id: Option<i64>,
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
    /// Songs of the chosen setlist that go with no plan song, in Playback's order.
    pub playback_only: Vec<LibrarySong>,
    /// True when the operator chose this setlist (it is not the one on the plan's date).
    pub chosen: bool,
    /// Playback's other recent setlists, newest first, for the chooser. Given when there is none for the date or
    /// one was chosen; empty otherwise.
    pub setlists: Vec<LibrarySetlist>,
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
    /// What the helper says about authentication: `key` (this answer was signed and accepted), `none`
    /// (no key is set: anyone may ask), `required` (the bare answer of a keyed helper), `key_too_short`.
    /// Absent on helper 0.2.0 and older.
    pub auth: Option<String>,
    /// `"updater": 1` from helper 0.4.0: it can install a signed update that StagePilot sends. Absent on older helpers.
    pub updater: Option<u32>,
    /// The helper's own sentence about why the last update was undone (`last_update_error`), when there is one.
    pub last_update_error: Option<String>,
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
        auth: text_of(value, "auth").map(|v| v.chars().take(20).collect()),
        updater: count_of(value, "updater"),
        last_update_error: text_of(value, "last_update_error")
            .map(|v| v.chars().take(200).collect()),
    }
}

/// Which state an answer of `GET /health` puts the helper in. The question was signed when StagePilot
/// has a key, so `auth: "key"` is the one answer that proves the key works.
pub fn helper_state_for(health: &Health) -> HelperState {
    match health.auth.as_deref() {
        Some("key") => {
            if crate::helper_assets::is_older_than_bundled(health.version.as_deref()) {
                HelperState::Outdated
            } else {
                HelperState::Ready
            }
        }
        // The bare answer of a keyed helper (we sent no valid signature) or a key it cannot use.
        Some("required") | Some("key_too_short") => HelperState::NeedsKey,
        // `none` (0.3.0 with no key set) and no field at all (0.2.0): it answers anyone, and it is not the
        // keyed helper this StagePilot installs, so the update is offered; it keeps working meanwhile.
        _ => HelperState::Outdated,
    }
}

/// The one plain sentence for a helper state.
pub fn helper_words(state: HelperState, version: Option<&str>) -> String {
    let version = version
        .filter(|v| !v.is_empty())
        .map_or(String::new(), |v| format!(" (version {v})"));
    match state {
        HelperState::NotInstalled => {
            "The Library Link is not installed on the Playback computer.".to_string()
        }
        HelperState::NeedsKey => {
            "The Library Link does not know this StagePilot yet. Run the setup command again."
                .to_string()
        }
        HelperState::Outdated => format!(
            "The Library Link{version} can be updated. It still works; run the setup command to update it."
        ),
        HelperState::Ready => format!("The Library Link{version} is ready."),
        HelperState::Unreachable => "The Library Link did not answer.".to_string(),
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
        content_id: whole_number(item.get("content_id"))
            .and_then(|n| u64::try_from(n).ok().filter(|n| *n > 0)),
        position: count_of(item, "order")
            .filter(|n| *n > 0)
            .unwrap_or_else(|| u32::try_from(index + 1).unwrap_or(u32::MAX)),
        title,
        key: key_name(item, "key"),
        key_original: key_name(item, "key_original"),
        key_was: None,
        key_changed_at: None,
        has_waveform: false,
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
    pub id: Option<i64>,
    pub title: String,
    pub date: Option<String>,
    pub songs: Vec<LibrarySong>,
}

impl ParsedSetlist {
    pub fn summary(&self) -> LibrarySetlist {
        LibrarySetlist {
            id: self.id,
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
        id: whole_number(value.get("setlist_id")),
        title: text_of(value, "title").unwrap_or_else(|| "a setlist".to_string()),
        date: date_of(value, "performance_date"),
        songs,
    })
}

/// Setlists kept from `/setlists` (the chooser offers a handful of the newest).
pub const MAX_SETLISTS: usize = 30;

/// `/setlists` as the chooser's lines, newest first as the helper sends them. An entry with no number to ask for it by
/// is left out (it could not be chosen); a missing title is "a setlist"; a song count that is not a number is 0.
pub fn parse_setlist_list(value: &Value) -> Vec<LibrarySetlist> {
    let Some(list) = value.get("setlists").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|entry| {
            let id = whole_number(entry.get("setlist_id"))?;
            Some(LibrarySetlist {
                id: Some(id),
                title: text_of(entry, "title").unwrap_or_else(|| "a setlist".to_string()),
                date: date_of(entry, "performance_date"),
                songs: count_of(entry, "songs").unwrap_or(0),
            })
        })
        .take(MAX_SETLISTS)
        .collect()
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
/// apostrophes and thousands commas, every other punctuation mark and run of spaces one space. "A / B" and "A/B" agree.
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
    let chars: Vec<char> = stripped.chars().collect();
    for (at, &c) in chars.iter().enumerate() {
        if ('\u{300}'..='\u{36f}').contains(&c) || matches!(c, '\'' | '’' | '‘' | '`' | '´') {
            continue;
        }
        // "10,000 Reasons" and "10000 Reasons" are one title: a comma between two digits is only a thousands mark.
        if c == ','
            && at > 0
            && chars[at - 1].is_ascii_digit()
            && chars.get(at + 1).is_some_and(char::is_ascii_digit)
        {
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
    /// The key the plan names for the song, when it names one.
    pub key_name: Option<String>,
}

/// The words of a title that say which song it is: every bracket group and a "feat." tail are set aside, then the
/// same rules as `normalise_title`. "Goodness of God (Radio Edit)" and "Goodness of God feat. Someone" both read
/// "goodness of god".
pub fn loose_title(title: &str) -> String {
    let lowered: String = title
        .chars()
        .take(MAX_TEXT * 2)
        .flat_map(char::to_lowercase)
        .collect();
    let mut without_groups = String::new();
    let mut depth = 0usize;
    for c in lowered.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => without_groups.push(c),
            _ => {}
        }
    }
    let padded = format!(" {without_groups} ");
    let cut = [" feat. ", " feat ", " ft. ", " ft ", " featuring "]
        .iter()
        .filter_map(|marker| padded.find(marker))
        .min();
    let kept = cut.map_or(padded.as_str(), |at| &padded[..at]);
    let words = normalise_title(kept);
    let words = words.strip_prefix("the ").unwrap_or(&words).to_string();
    if words.is_empty() {
        normalise_title(title)
    } else {
        words
    }
}

/// Whether two different titles are alike enough to be worth showing as an uncertain pair: one is the other's first
/// words ("Oceans" and "Oceans Where Feet May Fail"), or they differ by at most two letters (a typo) and are at least
/// six letters long, or they share most of their words (at least three, and at least 60 % of all words in both).
fn alike(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() || a == b {
        return false;
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if short.len() >= 4
        && long.starts_with(short)
        && long.as_bytes().get(short.len()) == Some(&b' ')
    {
        return true;
    }
    if short.chars().count() >= 6 && edit_distance(a, b) <= 2 {
        return true;
    }
    let words_a: std::collections::HashSet<&str> = a.split(' ').collect();
    let words_b: std::collections::HashSet<&str> = b.split(' ').collect();
    let shared = words_a.intersection(&words_b).count();
    let all = words_a.union(&words_b).count();
    shared >= 3 && shared * 5 >= all * 3
}

/// Letters to change, add or take away to turn one text into the other.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(ca != cb));
            diagonal = above;
        }
    }
    row[b.len()]
}

/// What `match_setlist` found.
#[derive(Debug, Clone, PartialEq)]
pub struct SetlistMatch {
    /// One entry per plan song, in plan order.
    pub songs: Vec<LibraryPlanSong>,
    /// The setlist's songs that go with no plan song (and are nobody's uncertain candidate), in Playback's order.
    pub playback_only: Vec<LibrarySong>,
}

/// Pairs plan songs with Playback songs by title, in three steps that only look at what the earlier step left:
/// 1. exact: exactly one song with the same normalised title on each side;
/// 2. close: exactly one with the same title once brackets and a "feat." tail are set aside;
/// 3. uncertain: exactly one alike title (see `alike`) on each side, both ways. Never taken as the song's library.
///
/// A title that is on one side more than once is never paired (the status says which side). Neither list is
/// reordered or changed.
pub fn match_setlist(plan: &[PlanTitle], playback: &[LibrarySong]) -> SetlistMatch {
    let keys: Vec<String> = plan
        .iter()
        .map(|song| normalise_title(&song.title))
        .collect();
    let mut in_plan: HashMap<&str, usize> = HashMap::new();
    for key in keys.iter().filter(|k| !k.is_empty()) {
        *in_plan.entry(key.as_str()).or_default() += 1;
    }
    let playback_keys: Vec<String> = playback
        .iter()
        .map(|song| normalise_title(&song.title))
        .collect();
    let mut in_playback: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, key) in playback_keys
        .iter()
        .enumerate()
        .filter(|(_, k)| !k.is_empty())
    {
        in_playback.entry(key.as_str()).or_default().push(index);
    }
    let mut status: Vec<PlanMatch> = Vec::with_capacity(plan.len());
    let mut paired: Vec<Option<(usize, MatchConfidence)>> = vec![None; plan.len()];
    let mut taken = vec![false; playback.len()];
    for (index, key) in keys.iter().enumerate() {
        let found = in_playback.get(key.as_str()).map_or(&[][..], Vec::as_slice);
        status.push(if key.is_empty() || found.is_empty() {
            PlanMatch::Missing
        } else if in_plan.get(key.as_str()).copied().unwrap_or(0) > 1 {
            PlanMatch::DuplicateInPlan
        } else if found.len() > 1 {
            PlanMatch::DuplicateInPlayback
        } else {
            paired[index] = Some((found[0], MatchConfidence::Exact));
            taken[found[0]] = true;
            PlanMatch::Matched
        });
    }
    // A Playback song whose exact title is in the plan is spoken for (paired, or a duplicate): steps 2 and 3 skip it.
    let spoken_for = |taken: &[bool], index: usize| {
        taken[index] || in_plan.contains_key(playback_keys[index].as_str())
    };
    // Step 2: close.
    let loose_plan: Vec<String> = plan.iter().map(|song| loose_title(&song.title)).collect();
    let loose_playback: Vec<String> = playback
        .iter()
        .map(|song| loose_title(&song.title))
        .collect();
    let open_plan = |i: usize| status[i] == PlanMatch::Missing && !keys[i].is_empty();
    for i in (0..plan.len()).filter(|i| open_plan(*i)) {
        let same_plan = (0..plan.len())
            .filter(|j| open_plan(*j) && loose_plan[*j] == loose_plan[i])
            .count();
        let candidates: Vec<usize> = (0..playback.len())
            .filter(|k| !spoken_for(&taken, *k) && loose_playback[*k] == loose_plan[i])
            .collect();
        if same_plan == 1 && candidates.len() == 1 {
            paired[i] = Some((candidates[0], MatchConfidence::Close));
            taken[candidates[0]] = true;
        }
    }
    // Step 3: uncertain, only when the choice is one to one both ways among what is left.
    let left_plan: Vec<usize> = (0..plan.len())
        .filter(|i| open_plan(*i) && paired[*i].is_none())
        .collect();
    let left_playback: Vec<usize> = (0..playback.len())
        .filter(|k| !spoken_for(&taken, *k))
        .collect();
    let mut candidate_of: Vec<Option<usize>> = vec![None; plan.len()];
    for &i in &left_plan {
        let alike_to: Vec<usize> = left_playback
            .iter()
            .copied()
            .filter(|k| alike(&loose_plan[i], &loose_playback[*k]))
            .collect();
        if let [only] = alike_to[..] {
            let back = left_plan
                .iter()
                .filter(|j| alike(&loose_plan[**j], &loose_playback[only]))
                .count();
            if back == 1 {
                candidate_of[i] = Some(only);
            }
        }
    }
    let mut candidate_taken = vec![false; playback.len()];
    for k in candidate_of.iter().flatten() {
        candidate_taken[*k] = true;
    }
    let songs = plan
        .iter()
        .enumerate()
        .map(|(index, song)| {
            let mut entry = LibraryPlanSong {
                key: song.key.clone(),
                title: song.title.clone(),
                plan_position: u32::try_from(index + 1).unwrap_or(u32::MAX),
                plan_seconds: song.seconds,
                plan_key: song.key_name.clone(),
                status: status[index],
                confidence: None,
                library: None,
                candidate: None,
            };
            if let Some((k, confidence)) = paired[index] {
                entry.status = PlanMatch::Matched;
                entry.confidence = Some(confidence);
                entry.library = playback.get(k).cloned();
            } else if let Some(k) = candidate_of[index] {
                entry.status = PlanMatch::Uncertain;
                entry.confidence = Some(MatchConfidence::Uncertain);
                entry.candidate = playback.get(k).cloned();
            }
            entry
        })
        .collect();
    let playback_only = playback
        .iter()
        .enumerate()
        .filter(|(k, _)| !taken[*k] && !candidate_taken[*k])
        .map(|(_, song)| song.clone())
        .collect();
    SetlistMatch {
        songs,
        playback_only,
    }
}

/// `match_setlist` for a caller that wants only the plan's side.
pub fn match_plan(plan: &[PlanTitle], playback: &[LibrarySong]) -> Vec<LibraryPlanSong> {
    match_setlist(plan, playback).songs
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn what_the_helper_says_about_authentication_decides_its_state() {
        let bundled = crate::helper_assets::BUNDLED_HELPER_VERSION;
        let state = |auth: Option<&str>, version: &str| {
            helper_state_for(&parse_health(&json!({
                "ok": true, "version": version, "database": "ok", "auth": auth
            })))
        };
        assert_eq!(state(Some("key"), bundled), HelperState::Ready);
        assert_eq!(state(Some("key"), "0.10.0"), HelperState::Ready);
        assert_eq!(
            state(Some("key"), "0.3.9"),
            HelperState::Outdated,
            "older than the bundled one"
        );
        assert_eq!(
            state(Some("none"), bundled),
            HelperState::Outdated,
            "no key set on the helper"
        );
        assert_eq!(
            state(None, "0.2.0"),
            HelperState::Outdated,
            "0.2.0 has no auth field at all"
        );
        assert_eq!(state(Some("required"), bundled), HelperState::NeedsKey);
        assert_eq!(state(Some("key_too_short"), bundled), HelperState::NeedsKey);
        assert!(HelperState::Ready.is_active() && HelperState::Outdated.is_active());
        for quiet in [
            HelperState::NotInstalled,
            HelperState::NeedsKey,
            HelperState::Unreachable,
        ] {
            assert!(!quiet.is_active(), "{quiet:?}");
            assert!(!helper_words(quiet, None).is_empty());
        }
        assert!(helper_words(HelperState::Ready, Some("0.3.0")).contains("0.3.0"));
    }

    fn song(title: &str, order: u32) -> LibrarySong {
        LibrarySong {
            setlist_song_id: Some(i64::from(order) + 100),
            content_id: None,
            position: order,
            title: title.into(),
            key: Some("G".into()),
            key_original: Some("G".into()),
            key_was: None,
            key_changed_at: None,
            has_waveform: false,
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
            key_name: None,
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
    fn a_song_says_which_key_it_had_before_a_change_only_when_one_was_seen() {
        let plain = serde_json::to_value(song("Song One", 1)).unwrap();
        assert_eq!(plain["key_was"], Value::Null);
        assert_eq!(plain["key_changed_at"], Value::Null);
        assert_eq!(
            plain["key_original"],
            json!("G"),
            "the original key is still sent"
        );
        let mut changed = song("Song One", 1);
        changed.key = Some("Ab".into());
        changed.key_was = Some("G".into());
        changed.key_changed_at = Some("2026-11-20T03:00:00Z".into());
        let value = serde_json::to_value(changed).unwrap();
        assert_eq!(value["key"], json!("Ab"));
        assert_eq!(value["key_was"], json!("G"));
        assert_eq!(value["key_changed_at"], json!("2026-11-20T03:00:00Z"));
        // The helper never says it: reading a setlist never makes it up.
        let parsed = parse_setlist(&json!({"items": [{"title": "Song One", "key": {"name": "Ab"}, "key_was": {"name": "C"}}]})).unwrap();
        assert_eq!(
            (
                parsed.songs[0].key_was.clone(),
                parsed.songs[0].key_changed_at.clone()
            ),
            (None, None)
        );
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
                {"setlist_song_id": 2000001, "order": 1, "title": "Song One", "artist": "x", "content_id": 5551234,
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
        assert_eq!(one.content_id, Some(5_551_234));
        assert_eq!(parsed.songs[1].content_id, None);
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

    fn found(
        songs: &[LibraryPlanSong],
        index: usize,
    ) -> (PlanMatch, Option<MatchConfidence>, Option<u32>) {
        let song = &songs[index];
        (
            song.status,
            song.confidence,
            song.library
                .as_ref()
                .or(song.candidate.as_ref())
                .map(|l| l.position),
        )
    }

    #[test]
    fn real_titles_that_differ_only_in_punctuation_case_a_thousands_comma_or_live_are_exact() {
        let plan = [
            plan_song("HOLY, HOLY, HOLY"),
            plan_song("10,000 Reasons"),
            plan_song("Goodness of God (Live)"),
            plan_song("Oceans (Where Feet May Fail)"),
        ];
        let playback = [
            song("Holy Holy Holy", 1),
            song("10000 Reasons", 2),
            song("Goodness Of God", 3),
            song("Oceans Where Feet May Fail", 4),
        ];
        let result = match_setlist(&plan, &playback);
        for i in 0..4 {
            assert_eq!(
                found(&result.songs, i),
                (
                    PlanMatch::Matched,
                    Some(MatchConfidence::Exact),
                    Some(i as u32 + 1)
                ),
                "{i}"
            );
        }
        assert!(result.playback_only.is_empty());
    }

    #[test]
    fn a_bracket_or_a_feat_tail_makes_a_close_match() {
        let plan = [
            plan_song("Build My Life"),
            plan_song("Graves Into Gardens (feat. Brandon Lake)"),
            plan_song("Oceans"),
            plan_song("Gratitude ft. Someone"),
            plan_song("The Blessing [Radio Edit]"),
        ];
        let playback = [
            song("Build My Life (Housefires)", 1),
            song("Graves Into Gardens", 2),
            song("Oceans (Where Feet May Fail)", 3),
            song("Gratitude", 4),
            song("Blessing", 5),
        ];
        let result = match_setlist(&plan, &playback);
        for i in 0..5 {
            assert_eq!(
                found(&result.songs, i),
                (
                    PlanMatch::Matched,
                    Some(MatchConfidence::Close),
                    Some(i as u32 + 1)
                ),
                "{i}"
            );
            assert!(result.songs[i].library.is_some() && result.songs[i].candidate.is_none());
        }
    }

    #[test]
    fn alike_titles_are_uncertain_and_never_become_the_songs_library() {
        let plan = [
            plan_song("Way Maker"),
            plan_song("Holy Spirit"),
            plan_song("Great Are You Lord"),
        ];
        let playback = [
            song("Waymaker", 1),
            song("Holy Spirit Come Down", 2),
            song("Great Are You, Lord, Tonight", 3),
        ];
        let result = match_setlist(&plan, &playback);
        for i in 0..3 {
            let (status, confidence, position) = found(&result.songs, i);
            assert_eq!(
                (status, confidence),
                (PlanMatch::Uncertain, Some(MatchConfidence::Uncertain)),
                "{i}"
            );
            assert_eq!(position, Some(i as u32 + 1));
            assert!(result.songs[i].library.is_none(), "{i}");
            assert!(result.songs[i].candidate.is_some(), "{i}");
        }
        assert!(
            result.playback_only.is_empty(),
            "a candidate is not a song that only Playback has"
        );
    }

    #[test]
    fn a_song_only_in_the_plan_or_only_in_playback_is_not_invented_a_partner() {
        let plan = [
            plan_song("Song One"),
            plan_song("Only In Plan Here"),
            plan_song("Song Three"),
        ];
        let playback = [
            song("Song One", 1),
            song("Something Else Entirely", 2),
            song("Song Three", 3),
            song("Extra Song", 4),
        ];
        let result = match_setlist(&plan, &playback);
        assert_eq!(found(&result.songs, 1), (PlanMatch::Missing, None, None));
        let only: Vec<&str> = result
            .playback_only
            .iter()
            .map(|s| s.title.as_str())
            .collect();
        assert_eq!(only, ["Something Else Entirely", "Extra Song"]);
    }

    #[test]
    fn two_plan_songs_alike_to_one_playback_song_stay_unpaired_and_the_same_title_twice_is_never_paired(
    ) {
        // Both plan songs are alike to the one Playback song, and it is alike to both: nobody is chosen.
        let plan = [plan_song("Holy Spirit"), plan_song("Holy Spirit Come")];
        let playback = [song("Holy Spirit Come Down", 1)];
        let result = match_setlist(&plan, &playback);
        assert_eq!(found(&result.songs, 0).0, PlanMatch::Missing);
        assert_eq!(found(&result.songs, 1).0, PlanMatch::Missing);
        assert_eq!(result.playback_only.len(), 1);
        // The same title twice: the exact title is there, so no loose step may pair it with a bracketed one.
        let plan = [plan_song("Amazing Grace"), plan_song("Amazing Grace")];
        let playback = [
            song("Amazing Grace", 1),
            song("Amazing Grace (My Chains Are Gone)", 2),
        ];
        let result = match_setlist(&plan, &playback);
        assert!(result
            .songs
            .iter()
            .all(|s| s.status == PlanMatch::DuplicateInPlan && s.library.is_none()));
        assert_eq!(result.playback_only.len(), 2);
    }

    #[test]
    fn two_close_candidates_are_not_chosen_between() {
        let plan = [plan_song("Oceans")];
        let playback = [
            song("Oceans (Live)", 1).clone(),
            song("Oceans (Acoustic)", 2),
            song("Oceans (Where Feet May Fail)", 3),
        ];
        // "Oceans (Live)" is exact; the rest are left to Playback only.
        let result = match_setlist(&plan, &playback);
        assert_eq!(
            found(&result.songs, 0),
            (PlanMatch::Matched, Some(MatchConfidence::Exact), Some(1))
        );
        let plan = [plan_song("Oceans")];
        let playback = [
            song("Oceans (Acoustic)", 1),
            song("Oceans (Where Feet May Fail)", 2),
        ];
        let result = match_setlist(&plan, &playback);
        assert_eq!(found(&result.songs, 0).0, PlanMatch::Missing);
        assert_eq!(result.playback_only.len(), 2);
    }

    #[test]
    fn short_or_unrelated_titles_are_never_alike() {
        for (a, b) in [
            ("Hope", "Hopeless Romantic"),
            ("Joy", "Joyful"),
            ("Song One", "Song Two"),
            ("Amen", "Amend"),
            ("A B C", "A B D"),
        ] {
            assert!(!alike(&loose_title(a), &loose_title(b)), "{a:?} vs {b:?}");
        }
        assert!(alike("way maker", "waymaker"));
        assert!(alike("oceans", "oceans where feet may fail"));
    }

    #[test]
    fn loose_title_sets_brackets_feat_and_a_leading_the_aside_but_never_leaves_nothing() {
        assert_eq!(loose_title("The Blessing (Live) [Radio Edit]"), "blessing");
        assert_eq!(
            loose_title("Gratitude feat. Brandon Lake & Co"),
            "gratitude"
        );
        assert_eq!(loose_title("Gratitude (feat. Brandon Lake)"), "gratitude");
        assert_eq!(loose_title("(Live)"), "live");
        assert_eq!(loose_title("(Intro)"), "intro");
        assert_eq!(loose_title(""), "");
        assert_eq!(loose_title("Featuring Love"), "featuring love");
    }

    #[test]
    fn a_setlist_list_reads_the_ids_titles_dates_and_counts_and_leaves_out_what_cannot_be_chosen() {
        let list = parse_setlist_list(&serde_json::json!({"setlists": [
            {"setlist_id": 12, "title": "Nov 22", "performance_date": "2026-11-22", "songs": 5},
            {"setlist_id": 11, "title": "Nov 15 night", "performance_date": "2026-11-15", "songs": "five"},
            {"title": "No number"},
            {"setlist_id": 9, "performance_date": "someday"},
        ]}));
        assert_eq!(list.len(), 3);
        assert_eq!(
            (
                list[0].id,
                list[0].title.as_str(),
                list[0].date.as_deref(),
                list[0].songs
            ),
            (Some(12), "Nov 22", Some("2026-11-22"), 5)
        );
        assert_eq!(
            (list[1].songs, list[2].title.as_str(), list[2].date.clone()),
            (0, "a setlist", None)
        );
        assert!(parse_setlist_list(&serde_json::json!({"setlists": "x"})).is_empty());
        assert!(parse_setlist_list(&serde_json::json!(null)).is_empty());
    }
}
