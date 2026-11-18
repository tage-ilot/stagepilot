//! Putting the songs Playback plays into a ProPresenter playlist, with the arrangement and key Playback plays them in.
//!
//! Two steps, and the second can only be asked for after the first:
//!
//! * [`AddSongs::preview`] writes nothing. It says, for every chosen song, whether ProPresenter has it (found by its
//!   MultiTracks number), whether a copy that already plays the right sections exists in the `Playback` library, or what would be
//!   made: an import through ProPresenter's own Search, then a copy `<Song> (n)` in the library `Playback` whose selected
//!   arrangement is the one Playback plays. It also says where in the playlist the songs go and what stays as it is.
//! * [`AddSongs::apply`] does it, for a preview whose `plan_id` still matches what is true now. It backs up the playlist, makes
//!   the files (never replacing or editing any), waits for ProPresenter to load them and then writes the playlist ONCE, whole,
//!   with every item that was there exactly as it was read. It reads the playlist back and reports every difference.
//!
//! Nothing is ever deleted or overwritten in ProPresenter, no file in `Songs` is edited (only ProPresenter's own import adds
//! one). A section that fits more than one group of a song is guessed (see `pro_file::resolve`) and the guess is named in the
//! preview and in the result; a section no group has blocks that song until the person leaves it out.

mod apply;
mod plan;

pub use apply::{
    ApplyOutcome, ApplyRefusal, ApplyReport, Event, Guard, Prepared, Progress, SongResult,
    SongStatus, Step, StepState, Timing,
};
pub use plan::{clean_name, copy_file_name, PlanError};

use super::api::ProPresenterApi;
use super::ProPresenterHost;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use ts_rs::TS;

/// The name of the arrangement StagePilot adds to a copy.
pub const PLAYBACK_ARRANGEMENT: &str = "Playback";

/// At most this many songs in one change.
pub const MAX_SONGS: usize = 60;

/// A song from Playback's plan as far as this needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct WantedSong {
    /// What the person (and the caller) calls this song in a request: the Playback setlist-song number as text.
    pub ref_id: String,
    pub title: String,
    /// Place in the Playback setlist (songs are added in this order).
    pub position: u32,
    /// The MultiTracks number.
    pub content_id: Option<u64>,
    /// Playback's key text.
    pub key: Option<String>,
    /// The arrangement Playback plays: its sections in order, repeats included.
    pub sections: Vec<String>,
    /// The rental ran out.
    pub rental_expired: bool,
}

/// Where in the playlist the songs go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// After the last item of the playlist.
    End,
    /// At the end of the section under the header with this name.
    AfterHeader(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanRequest {
    pub songs: Vec<WantedSong>,
    /// The playlist's id (uuid) as `GET /v1/playlists` lists it.
    pub playlist_id: String,
    pub place: Place,
}

/// What will happen to one song.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "AddSongsAction")]
pub enum SongAction {
    /// A copy that already plays what Playback plays is in the Playback library: it is used, nothing is made.
    Reuse,
    /// A new file `<Song> (n)` is made in the Playback library from the song ProPresenter has.
    Copy,
    /// ProPresenter does not have the song: it is imported through ProPresenter's Search, then copied as above.
    ImportThenCopy,
    /// More than one file in ProPresenter could be this song and StagePilot does not guess which.
    NeedsChoice,
    /// Nothing can be done for this song, and `reason` says why.
    Blocked,
}

impl SongAction {
    /// The song must be left out (listed in `skip`) before the change can be applied.
    pub fn is_stuck(self) -> bool {
        matches!(self, Self::NeedsChoice | Self::Blocked)
    }
}

/// A section that fits more than one group of the song and was given one of them. It never blocks; the person checks it in
/// ProPresenter after the import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "AddSongsGuess")]
pub struct SongGuess {
    /// The section as Playback names it.
    pub section: String,
    /// Which use of this name in Playback's list it is, from 1.
    pub position: u32,
    /// How many times Playback uses the name.
    pub uses: u32,
    /// The group's name in the song (blank for Count Off).
    pub group_name: String,
    /// Which of the song's groups with that name was taken, from 1.
    pub group_index: u32,
    /// How many groups of the song have that name.
    pub groups: u32,
    /// The rule that picked it: `in_order`, `last_reused` or `repeat_in_turn`.
    pub rule: String,
    /// The plain sentence: `Interlude (3rd use) uses the 2nd "Interlude" group: check it in ProPresenter. ...`.
    pub text: String,
}

/// The guesses of a mapping, in playing order.
pub fn guesses_of(mapping: &[crate::pro_file::Mapped]) -> Vec<SongGuess> {
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    mapping
        .iter()
        .filter_map(|m| {
            let guess = m.guess.as_ref()?;
            let group_name = m.group_name.clone().unwrap_or_default();
            Some(SongGuess {
                section: m.section.clone(),
                position: count(guess.position),
                uses: count(guess.uses),
                group_index: count(guess.group_index),
                groups: count(guess.groups),
                rule: guess.rule.word().to_string(),
                text: guess.words(&m.section, &group_name),
                group_name,
            })
        })
        .collect()
}

/// How one section was matched to a group of the song.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "AddSongsMapping")]
pub struct MappingLine {
    pub section: String,
    /// The group's name in the song (blank for Count Off).
    pub group: Option<String>,
    /// `full`, `only`, `identical`, `guessed` or `missing`.
    pub how: String,
    /// A plain sentence when the match is not the exact one.
    pub line: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "AddSongsSong")]
pub struct SongPlan {
    pub ref_id: String,
    pub title: String,
    pub position: u32,
    #[ts(type = "number | null")]
    pub content_id: Option<u64>,
    pub action: SongAction,
    /// The Playback-library file that will be used (reuse) or made (copy), without `.pro`.
    pub file: Option<String>,
    /// Where a copy is made from: `Songs / Alpha.pro`.
    pub source: Option<String>,
    #[serde(skip)]
    #[ts(skip)]
    pub source_file: Option<(String, String)>,
    /// What the imported file will be called (without `.pro`), for `import_then_copy`.
    pub import_as: Option<String>,
    /// Why nothing can be done (`blocked`) or why a choice is needed (`needs_choice`).
    pub reason: Option<String>,
    /// For `needs_choice`: the files that could be it.
    pub choices: Vec<String>,
    /// Plain lines about this song: how sections were matched, what happens to the key.
    pub notes: Vec<String>,
    pub mapping: Vec<MappingLine>,
    /// Sections that fit more than one group and were given one: check them in ProPresenter. Never blocks.
    pub guesses: Vec<SongGuess>,
    pub sections: u32,
    pub key: Option<String>,
    /// The song's copy is already an item of the playlist: it is not added a second time.
    pub already_in_playlist: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "AddSongsPlaylist")]
pub struct PlaylistPlan {
    pub id: String,
    pub name: String,
    pub folder: Option<String>,
    /// A plain sentence: `at the end of the playlist`, `at the end of the section under the header "Worship"`.
    pub place: String,
    /// Position among the items where the new songs go (0 is first).
    pub insert_at: u32,
    pub total_items: u32,
    /// The item just before and just after the place.
    pub before: Option<String>,
    pub after: Option<String>,
    /// How many items stay exactly as they are (all of them).
    pub untouched: u32,
    /// Set when the playlist side cannot be done.
    pub blocked: Option<String>,
    /// A fingerprint of the items as read (names, kinds, durations): a change makes the plan stale.
    pub items_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "AddSongsPlan")]
pub struct Plan {
    /// Names exactly this plan; `apply` refuses when what is true now would give another.
    pub plan_id: String,
    pub playlist: PlaylistPlan,
    pub songs: Vec<SongPlan>,
    /// Songs that would be added (not stuck).
    pub to_add: u32,
    /// Songs that are `blocked` or `needs_choice`.
    pub stuck: u32,
    /// The playlist side is fine and at least one song can be added.
    pub can_apply: bool,
    /// ProPresenter's own words (`ProPresenter 19.0.1 (API v1)`), not part of the plan's identity.
    pub propresenter: Option<String>,
    /// A song can be imported through ProPresenter's Search from here.
    pub can_import: bool,
    pub warnings: Vec<String>,
}

/// The engine: a ProPresenter computer's files and screen, and ProPresenter's HTTP API.
pub struct AddSongs {
    pub(crate) host: Arc<dyn ProPresenterHost>,
    pub(crate) api: Arc<dyn ProPresenterApi>,
    pub(crate) backup_dir: PathBuf,
    pub(crate) timing: Timing,
    pub(crate) now: Arc<dyn Fn() -> i64 + Send + Sync>,
    pub(crate) new_uuid: Arc<dyn Fn() -> String + Send + Sync>,
}

impl AddSongs {
    /// `backup_dir` is where playlist backups are kept (beside the settings file); `now` gives seconds since the epoch and
    /// `new_uuid` an upper-case uuid.
    pub fn new(
        host: Arc<dyn ProPresenterHost>,
        api: Arc<dyn ProPresenterApi>,
        backup_dir: PathBuf,
        now: Arc<dyn Fn() -> i64 + Send + Sync>,
        new_uuid: Arc<dyn Fn() -> String + Send + Sync>,
    ) -> Self {
        Self {
            host,
            api,
            backup_dir,
            timing: Timing::default(),
            now,
            new_uuid,
        }
    }

    /// Other waits than the product's (tests).
    #[must_use]
    pub fn with_timing(mut self, timing: Timing) -> Self {
        self.timing = timing;
        self
    }
}
