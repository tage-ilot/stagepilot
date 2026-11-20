//! Doing it: the backup, the files, the wait for ProPresenter to load them, the one playlist write and the read-back.

use super::plan::{insert_position, item_kind, item_name, item_uuids, items_of};
use super::{
    guesses_of, AddSongs, Plan, PlanError, PlanRequest, SongAction, SongGuess, SongPlan,
    WantedSong, PLAYBACK_ARRANGEMENT,
};
use crate::link::api::{ApiError, LibraryEntry};
use crate::link::{LinkError, PLAYBACK_LIBRARY, SONGS_LIBRARY};
use crate::pro_file::{build_copy, CopyRequest, KeyOutcome, ProError};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use ts_rs::TS;

/// How many playlist backups are kept.
pub const KEEP_BACKUPS: usize = 20;

/// The waits of an apply. The product's are in [`Timing::default`].
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// How often ProPresenter's library listing is looked at while a new file has no id.
    pub poll: Duration,
    /// After this long without an id the library is selected on screen, once.
    pub select_after: Duration,
    /// After this long without an id the song is reported as not loaded.
    pub give_up: Duration,
    /// How long ProPresenter is given before the playlist is read back.
    pub read_back: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_secs(2),
            select_after: Duration::from_secs(10),
            give_up: Duration::from_secs(30),
            read_back: Duration::from_secs(10),
        }
    }
}

impl Timing {
    /// Almost no waiting (tests).
    pub fn quick() -> Self {
        Self {
            poll: Duration::from_millis(5),
            select_after: Duration::from_millis(25),
            give_up: Duration::from_millis(80),
            read_back: Duration::from_millis(1),
        }
    }
}

/// The steps a job reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "AddSongsStep")]
pub enum Step {
    Check,
    Backup,
    Library,
    Import,
    Copy,
    Load,
    Playlist,
    ReadBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "AddSongsStepState")]
pub enum StepState {
    Running,
    Done,
    Failed,
    /// Something worth saying that is neither.
    Note,
}

/// One thing that happened, in plain words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "AddSongsEvent")]
pub struct Event {
    /// The song it is about (`ref_id`), or none for the playlist as a whole.
    pub song: Option<String>,
    pub step: Step,
    pub state: StepState,
    pub text: String,
}

/// Where events go.
pub trait Progress: Send + Sync {
    fn event(&self, event: Event);
}

/// Asked before anything is touched and again before every song and before the playlist is written.
pub trait Guard: Send + Sync {
    /// `Err(why)` when it is not safe to go on (a song is playing).
    fn check(&self) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "AddSongsSongStatus")]
pub enum SongStatus {
    /// In the playlist now (or, with `playlist_changed` false, would be).
    Added,
    /// Its copy was already an item of the playlist: left as it is.
    AlreadyThere,
    /// The person left it out.
    Skipped,
    Failed,
    /// Not reached (an earlier song failed).
    NotTried,
    /// The file was made but ProPresenter had not loaded it, so it is not in the playlist yet.
    NotLoaded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "AddSongsSongResult")]
pub struct SongResult {
    pub ref_id: String,
    pub title: String,
    pub status: SongStatus,
    /// The Playback-library file used or made, without `.pro`.
    pub file: Option<String>,
    pub text: String,
    /// `wrong_song`, `needs_library_open`, `unmapped`, `live_on_screen`, ...
    pub code: Option<String>,
    /// Sections that fit more than one group and were given one: check them in ProPresenter. Never a reason to stop.
    pub guesses: Vec<SongGuess>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "AddSongsOutcome")]
pub enum ApplyOutcome {
    /// Written and read back as planned.
    Done,
    /// Written; the read-back differs from the plan (see `differences`).
    DoneWithDifferences,
    /// Files are made but ProPresenter has not loaded them: the playlist was not written. Press Try again.
    NeedsLibraryOpen,
    /// A song failed; the playlist was not written.
    Stopped,
    /// A song started playing; nothing more was done and the playlist was not written.
    SongPlaying,
    /// ProPresenter is showing something on screen (or would not say): no screen step was run, nothing was typed or clicked.
    /// Files already made are kept; press Try again once it is clear.
    LiveOnScreen,
    /// ProPresenter refused the new playlist: it is unchanged.
    PlaylistRefused,
    /// The playlist write failed in another way: look at the playlist (the backup is kept).
    PlaylistFailed,
    BackupFailed,
    /// ProPresenter's API did not answer; nothing was changed.
    NotReachable,
    /// Nothing was left to add (every song was left out or is already there).
    NothingToDo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "AddSongsReport")]
pub struct ApplyReport {
    pub outcome: ApplyOutcome,
    /// One or two plain sentences.
    pub message: String,
    /// ProPresenter's own words from `GET /version`.
    pub version: Option<String>,
    /// Where the playlist was saved before anything changed.
    pub backup: Option<String>,
    pub songs: Vec<SongResult>,
    /// Everything the read-back found different from the plan.
    pub differences: Vec<String>,
    /// The playlist was written.
    pub playlist_changed: bool,
}

/// A plan that passed [`AddSongs::begin`]: ready to run.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub plan: Plan,
    request: PlanRequest,
    skip: Vec<String>,
}

/// Why an apply did not start at all (nothing was changed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyRefusal {
    Plan(PlanError),
    /// What is true now would give another plan than the one shown.
    PlanChanged,
    /// The playlist side cannot be done (the header is not there, ...).
    PlaylistBlocked(String),
    /// These songs are stuck and were not left out.
    StuckNotSkipped(Vec<String>),
    /// `skip` names songs that are not in the plan.
    UnknownSkip(Vec<String>),
    NothingToAdd,
    /// A song is playing.
    SongPlaying(String),
}

impl ApplyRefusal {
    pub fn words(&self) -> String {
        match self {
            Self::Plan(e) => e.words(),
            Self::PlanChanged => {
                "Something changed in ProPresenter or in Playback since the preview. Look at the preview again.".into()
            }
            Self::PlaylistBlocked(why) => why.clone(),
            Self::StuckNotSkipped(ids) => format!(
                "{} song(s) cannot be added. Leave them out to go on.",
                ids.len()
            ),
            Self::UnknownSkip(_) => "A song to leave out is not in the preview.".into(),
            Self::NothingToAdd => "No song is left to add.".into(),
            Self::SongPlaying(why) => why.clone(),
        }
    }
}

impl std::fmt::Display for ApplyRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.words())
    }
}

impl std::error::Error for ApplyRefusal {}

/// A song that cannot go on: a code and a sentence.
struct Failure {
    code: &'static str,
    text: String,
}

fn failure(code: &'static str, text: impl Into<String>) -> Failure {
    Failure {
        code,
        text: text.into(),
    }
}

fn link_failure(step: &str, error: &LinkError) -> Failure {
    failure("link", format!("{step}: {}", error.words()))
}

struct Run<'a> {
    progress: &'a dyn Progress,
}

impl Run<'_> {
    fn say(&self, song: Option<&str>, step: Step, state: StepState, text: impl Into<String>) {
        self.progress.event(Event {
            song: song.map(str::to_string),
            step,
            state,
            text: text.into(),
        });
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// What a song of the plan needs ProPresenter to load before the playlist can name it.
fn needs_entry(song: &SongPlan) -> bool {
    matches!(
        song.action,
        SongAction::Reuse | SongAction::Copy | SongAction::ImportThenCopy
    ) && !song.already_in_playlist
}

impl AddSongs {
    /// Does the change the preview named: [`Self::begin`] and then [`Self::run`].
    pub async fn apply(
        &self,
        request: &PlanRequest,
        expected_plan_id: &str,
        skip: &[String],
        guard: &dyn Guard,
        progress: &dyn Progress,
    ) -> Result<ApplyReport, ApplyRefusal> {
        let prepared = self.begin(request, expected_plan_id, skip, guard).await?;
        Ok(self.run(&prepared, guard, progress).await)
    }

    /// The checks that come before anything is touched: the plan is still what is true now, every stuck song is left out,
    /// something is left to add and nothing is playing. `Err` means nothing was started and nothing was changed.
    pub async fn begin(
        &self,
        request: &PlanRequest,
        expected_plan_id: &str,
        skip: &[String],
        guard: &dyn Guard,
    ) -> Result<Prepared, ApplyRefusal> {
        let plan = self.preview(request).await.map_err(ApplyRefusal::Plan)?;
        if plan.plan_id != expected_plan_id {
            return Err(ApplyRefusal::PlanChanged);
        }
        if let Some(why) = &plan.playlist.blocked {
            return Err(ApplyRefusal::PlaylistBlocked(why.clone()));
        }
        let unknown: Vec<String> = skip
            .iter()
            .filter(|id| !plan.songs.iter().any(|s| &s.ref_id == *id))
            .cloned()
            .collect();
        if !unknown.is_empty() {
            return Err(ApplyRefusal::UnknownSkip(unknown));
        }
        let stuck: Vec<String> = plan
            .songs
            .iter()
            .filter(|s| s.action.is_stuck() && !skip.contains(&s.ref_id))
            .map(|s| s.ref_id.clone())
            .collect();
        if !stuck.is_empty() {
            return Err(ApplyRefusal::StuckNotSkipped(stuck));
        }
        if !plan
            .songs
            .iter()
            .any(|s| !skip.contains(&s.ref_id) && !s.action.is_stuck() && !s.already_in_playlist)
        {
            return Err(ApplyRefusal::NothingToAdd);
        }
        guard.check().map_err(ApplyRefusal::SongPlaying)?;
        Ok(Prepared {
            plan,
            request: request.clone(),
            skip: skip.to_vec(),
        })
    }

    /// Does it. Everything that goes wrong from here on is in the report, never an `Err`.
    pub async fn run(
        &self,
        prepared: &Prepared,
        guard: &dyn Guard,
        progress: &dyn Progress,
    ) -> ApplyReport {
        let Ok(report) = self.run_inner(prepared, guard, progress).await;
        report
    }

    async fn run_inner(
        &self,
        prepared: &Prepared,
        guard: &dyn Guard,
        progress: &dyn Progress,
    ) -> Result<ApplyReport, std::convert::Infallible> {
        let (plan, request, skip) = (&prepared.plan, &prepared.request, &prepared.skip[..]);
        let active: Vec<&SongPlan> = plan
            .songs
            .iter()
            .filter(|s| !skip.contains(&s.ref_id) && !s.action.is_stuck())
            .collect();
        let run = Run { progress };
        let mut results: Vec<SongResult> = plan
            .songs
            .iter()
            .map(|s| SongResult {
                ref_id: s.ref_id.clone(),
                title: s.title.clone(),
                status: if skip.contains(&s.ref_id) || s.action.is_stuck() {
                    SongStatus::Skipped
                } else if s.already_in_playlist {
                    SongStatus::AlreadyThere
                } else {
                    SongStatus::NotTried
                },
                file: s.file.clone().filter(|_| !s.action.is_stuck()),
                text: if skip.contains(&s.ref_id) {
                    "Left out.".into()
                } else if s.already_in_playlist {
                    "Already in the playlist; left as it is.".into()
                } else {
                    String::new()
                },
                code: None,
                guesses: s.guesses.clone(),
            })
            .collect();
        let mut report = ApplyReport {
            outcome: ApplyOutcome::Done,
            message: String::new(),
            version: None,
            backup: None,
            songs: Vec::new(),
            differences: Vec::new(),
            playlist_changed: false,
        };
        let done = |mut report: ApplyReport,
                    results: Vec<SongResult>,
                    outcome: ApplyOutcome,
                    message: String| {
            report.outcome = outcome;
            report.message = message;
            report.songs = results;
            report
        };

        // 1. ProPresenter answers
        run.say(
            None,
            Step::Check,
            StepState::Running,
            "Asking ProPresenter for its version.",
        );
        match self.api.version().await {
            Ok(version) => {
                run.say(
                    None,
                    Step::Check,
                    StepState::Done,
                    format!("ProPresenter says: {version}."),
                );
                report.version = Some(version);
            }
            Err(error) => {
                run.say(None, Step::Check, StepState::Failed, error.words());
                return Ok(done(
                    report,
                    results,
                    ApplyOutcome::NotReachable,
                    format!("{} Nothing was changed.", error.words()),
                ));
            }
        }

        // 1b. nothing on screen, before any screen step is even planned
        let will_use_screen = active
            .iter()
            .any(|s| s.action == SongAction::ImportThenCopy && !s.already_in_playlist);
        if will_use_screen {
            if let Err(failure) = self.screen_is_free().await {
                run.say(None, Step::Check, StepState::Failed, failure.text.clone());
                for (result, song) in results.iter_mut().zip(&plan.songs) {
                    if song.action == SongAction::ImportThenCopy && !song.already_in_playlist {
                        result.code = Some("live_on_screen".into());
                        result.text = failure.text.clone();
                    }
                }
                return Ok(done(
                    report,
                    results,
                    ApplyOutcome::LiveOnScreen,
                    format!("{} Nothing was changed.", failure.text),
                ));
            }
        }

        // 2. the backup
        run.say(
            None,
            Step::Backup,
            StepState::Running,
            "Saving the playlist before anything changes.",
        );
        let before = match self.api.playlist(&plan.playlist.id).await {
            Ok(value) => value,
            Err(error) => {
                run.say(None, Step::Backup, StepState::Failed, error.words());
                return Ok(done(
                    report,
                    results,
                    ApplyOutcome::NotReachable,
                    format!("{} Nothing was changed.", error.words()),
                ));
            }
        };
        let items_before = items_of(&before).len();
        match self.write_backup(&plan.playlist.name, &before) {
            Ok(path) => {
                run.say(
                    None,
                    Step::Backup,
                    StepState::Done,
                    format!("Saved the playlist as {}.", path.display()),
                );
                report.backup = Some(path.display().to_string());
            }
            Err(why) => {
                run.say(None, Step::Backup, StepState::Failed, why.clone());
                return Ok(done(
                    report,
                    results,
                    ApplyOutcome::BackupFailed,
                    format!("{why} Nothing was changed."),
                ));
            }
        }

        // 3. the Playback library
        run.say(
            None,
            Step::Library,
            StepState::Running,
            "Making sure the Playback library exists.",
        );
        match self.host.ensure_library(PLAYBACK_LIBRARY).await {
            Ok(made) => run.say(
                None,
                Step::Library,
                StepState::Done,
                if made {
                    "Made the library named Playback."
                } else {
                    "The library named Playback is there."
                },
            ),
            Err(error) => {
                run.say(None, Step::Library, StepState::Failed, error.words());
                return Ok(done(
                    report,
                    results,
                    ApplyOutcome::Stopped,
                    format!("{} Nothing was changed in the playlist.", error.words()),
                ));
            }
        }

        // 4. the files, in order
        let mut stems: HashMap<String, String> = HashMap::new();
        let mut stopped = false;
        for song in &active {
            let at = results
                .iter()
                .position(|r| r.ref_id == song.ref_id)
                .unwrap_or(0);
            if song.already_in_playlist {
                continue;
            }
            if let Err(why) = guard.check() {
                run.say(
                    Some(&song.ref_id),
                    Step::Copy,
                    StepState::Failed,
                    why.clone(),
                );
                return Ok(done(
                    report,
                    results,
                    ApplyOutcome::SongPlaying,
                    format!("{why} The playlist was not changed."),
                ));
            }
            let wanted = request.songs.iter().find(|w| w.ref_id == song.ref_id);
            let result = match (wanted, song.action) {
                (Some(_), SongAction::Reuse) => {
                    let stem = song.file.clone().unwrap_or_default();
                    run.say(
                        Some(&song.ref_id),
                        Step::Copy,
                        StepState::Done,
                        format!(
                            "Using the copy \"{stem}\" that is already in the Playback library."
                        ),
                    );
                    Ok((stem, Vec::new()))
                }
                (Some(wanted), SongAction::Copy | SongAction::ImportThenCopy) => {
                    self.make_song(
                        &run,
                        wanted,
                        song,
                        &plan.playlist.id,
                        items_before,
                        &mut report.differences,
                    )
                    .await
                }
                _ => Err(failure("not_possible", "This song cannot be added.")),
            };
            match result {
                Ok((stem, guesses)) => {
                    results[at].file = Some(stem.clone());
                    results[at].text = "The copy is ready.".into();
                    if !guesses.is_empty() {
                        results[at].guesses = guesses;
                    }
                    stems.insert(song.ref_id.clone(), stem);
                }
                Err(Failure {
                    code: "live_on_screen",
                    text,
                }) => {
                    run.say(
                        Some(&song.ref_id),
                        Step::Import,
                        StepState::Failed,
                        text.clone(),
                    );
                    results[at].code = Some("live_on_screen".into());
                    results[at].text = text.clone();
                    return Ok(done(
                        report,
                        results,
                        ApplyOutcome::LiveOnScreen,
                        format!("{text} The playlist was not changed. Files already made are kept and used next time."),
                    ));
                }
                Err(Failure { code, text }) => {
                    run.say(
                        Some(&song.ref_id),
                        Step::Copy,
                        StepState::Failed,
                        text.clone(),
                    );
                    results[at].status = SongStatus::Failed;
                    results[at].text = text;
                    results[at].code = Some(code.to_string());
                    stopped = true;
                    break;
                }
            }
        }
        if stopped {
            let why = results
                .iter()
                .find(|r| r.status == SongStatus::Failed)
                .map(|r| format!("{}: {}", r.title, r.text))
                .unwrap_or_default();
            return Ok(done(
                report,
                results,
                ApplyOutcome::Stopped,
                format!("{why} The playlist was not changed. Files already made are kept and used next time."),
            ));
        }

        // 5. ProPresenter loads the new files
        let wait_for: Vec<(String, String)> = active
            .iter()
            .filter(|s| needs_entry(s))
            .filter_map(|s| {
                stems
                    .get(&s.ref_id)
                    .map(|stem| (s.ref_id.clone(), stem.clone()))
            })
            .collect();
        let uuids = match self.wait_loaded(&run, &wait_for).await {
            Ok(found) => found,
            Err(Waited::Api(error)) => {
                run.say(None, Step::Load, StepState::Failed, error.words());
                return Ok(done(
                    report,
                    results,
                    ApplyOutcome::NotReachable,
                    format!("{} The playlist was not changed.", error.words()),
                ));
            }
            Err(Waited::Live(text)) => {
                run.say(None, Step::Load, StepState::Failed, text.clone());
                return Ok(done(
                    report,
                    results,
                    ApplyOutcome::LiveOnScreen,
                    format!("{text} The playlist was not changed; the copies that were made are kept and used next time."),
                ));
            }
            Err(Waited::Missing(missing)) => {
                for result in &mut results {
                    if missing.contains(&result.ref_id) {
                        result.status = SongStatus::NotLoaded;
                        result.code = Some("needs_library_open".into());
                        result.text = "ProPresenter has not loaded this song's copy yet.".into();
                    }
                }
                let text = "Open the Playback library in ProPresenter once, then press Try again.";
                run.say(None, Step::Load, StepState::Failed, text);
                return Ok(done(
                    report,
                    results,
                    ApplyOutcome::NeedsLibraryOpen,
                    format!("{text} The playlist was not changed; the copies that were made are kept and used next time."),
                ));
            }
        };

        // 6. the playlist, once
        if let Err(why) = guard.check() {
            run.say(None, Step::Playlist, StepState::Failed, why.clone());
            return Ok(done(
                report,
                results,
                ApplyOutcome::SongPlaying,
                format!("{why} The playlist was not changed."),
            ));
        }
        run.say(
            None,
            Step::Playlist,
            StepState::Running,
            "Writing the playlist.",
        );
        let fresh = match self.api.playlist(&plan.playlist.id).await {
            Ok(value) => value,
            Err(error) => {
                run.say(None, Step::Playlist, StepState::Failed, error.words());
                return Ok(done(
                    report,
                    results,
                    ApplyOutcome::NotReachable,
                    format!("{} The playlist was not changed.", error.words()),
                ));
            }
        };
        let items = items_of(&fresh);
        if items_of(&before) != items {
            run.say(
                None,
                Step::Playlist,
                StepState::Note,
                "The playlist changed while the songs were being made; its newest version is used.",
            );
        }
        let at = match insert_position(&items, &request.place) {
            Ok(at) => at,
            Err(why) => {
                run.say(None, Step::Playlist, StepState::Failed, why.clone());
                return Ok(done(
                    report,
                    results,
                    ApplyOutcome::PlaylistFailed,
                    format!("{why} The playlist was not changed."),
                ));
            }
        };
        let mut present = item_uuids(&items);
        let mut added: Vec<Value> = Vec::new();
        for song in &active {
            let Some(stem) = stems.get(&song.ref_id) else {
                continue;
            };
            let Some(uuid) = uuids.get(&song.ref_id) else {
                continue;
            };
            let slot = results
                .iter()
                .position(|r| r.ref_id == song.ref_id)
                .unwrap_or(0);
            if !present.insert(uuid.to_ascii_uppercase()) {
                results[slot].status = SongStatus::AlreadyThere;
                results[slot].text = "Already in the playlist; left as it is.".into();
                continue;
            }
            added.push(json!({
                "id": {"uuid": uuid, "name": stem, "index": 0},
                "type": "presentation",
                "is_hidden": false,
                "is_pco": false,
                "presentation_info": {"presentation_uuid": uuid, "arrangement_name": PLAYBACK_ARRANGEMENT}
            }));
            results[slot].status = SongStatus::Added;
            results[slot].text = "Added to the playlist.".into();
        }
        if added.is_empty() {
            run.say(
                None,
                Step::Playlist,
                StepState::Done,
                "Every song is already in the playlist.",
            );
            return Ok(done(
                report,
                results,
                ApplyOutcome::NothingToDo,
                "Every song is already in the playlist, so it was not changed.".into(),
            ));
        }
        let mut list: Vec<Value> = items[..at.min(items.len())].to_vec();
        let new_from = list.len();
        list.extend(added.iter().cloned());
        list.extend(items[at.min(items.len())..].iter().cloned());
        for (index, item) in list.iter_mut().enumerate() {
            if item.is_object() {
                item["id"]["index"] = json!(index);
            }
        }
        let new_count = added.len();
        if let Err(error) = self.api.put_playlist(&plan.playlist.id, &list).await {
            let backup = report.backup.clone().unwrap_or_default();
            run.say(None, Step::Playlist, StepState::Failed, error.words());
            for result in &mut results {
                if result.status == SongStatus::Added {
                    result.status = SongStatus::NotLoaded;
                    result.text = "Not added: ProPresenter did not take the new playlist.".into();
                }
            }
            let (outcome, message) = match error {
                ApiError::NotFound => (
                    ApplyOutcome::PlaylistRefused,
                    format!("ProPresenter refused the new playlist (404), so nothing in the playlist was changed. The playlist as it was is saved at {backup}."),
                ),
                other => (
                    ApplyOutcome::PlaylistFailed,
                    format!("{} Look at the playlist in ProPresenter; it is saved as it was at {backup}.", other.words()),
                ),
            };
            return Ok(done(report, results, outcome, message));
        }
        report.playlist_changed = true;
        run.say(
            None,
            Step::Playlist,
            StepState::Done,
            format!("Wrote the playlist with {new_count} new song(s)."),
        );

        // 7. read it back
        run.say(
            None,
            Step::ReadBack,
            StepState::Running,
            "Reading the playlist back from ProPresenter.",
        );
        tokio::time::sleep(self.timing.read_back).await;
        let differences = match self.api.playlist(&plan.playlist.id).await {
            Ok(back) => compare(&list, new_from, new_count, &items_of(&back)),
            Err(error) => vec![format!(
                "The playlist could not be read back: {}",
                error.words()
            )],
        };
        report.differences.extend(differences);
        let differences = report.differences.clone();
        let (outcome, message) = if differences.is_empty() {
            run.say(
                None,
                Step::ReadBack,
                StepState::Done,
                "The playlist reads back as planned.",
            );
            (
                ApplyOutcome::Done,
                format!("Added {new_count} song(s) to the playlist \"{}\" and read it back: all as planned.", plan.playlist.name),
            )
        } else {
            run.say(
                None,
                Step::ReadBack,
                StepState::Failed,
                format!("{} difference(s) found.", differences.len()),
            );
            (
                ApplyOutcome::DoneWithDifferences,
                format!("Added {new_count} song(s) to the playlist \"{}\", but ProPresenter shows {} difference(s) from the plan.", plan.playlist.name, differences.len()),
            )
        };
        Ok(done(report, results, outcome, message))
    }

    /// Imports (when the song is not in ProPresenter), then makes the copy. Returns the new file's name without `.pro`.
    async fn make_song(
        &self,
        run: &Run<'_>,
        wanted: &WantedSong,
        song: &SongPlan,
        playlist_id: &str,
        items_before: usize,
        differences: &mut Vec<String>,
    ) -> Result<(String, Vec<SongGuess>), Failure> {
        let id = wanted.content_id.ok_or_else(|| {
            failure(
                "no_id",
                "Playback gave no MultiTracks number for this song.",
            )
        })?;
        let ref_id = Some(song.ref_id.as_str());
        let (library, file) = if song.action == SongAction::ImportThenCopy {
            let name = song.import_as.clone().unwrap_or_default();
            run.say(ref_id, Step::Import, StepState::Running, format!("Importing the song through ProPresenter's Search as \"{name}\". This takes a few seconds."));
            // Right before the screen is touched, ask again: something may have gone live since the check at the start.
            self.screen_is_free().await?;
            let imported = self.host.search_import(&id.to_string(), &name, None).await;
            // The import is made with Playlist "None": whatever it did, the playlist must have the items it had.
            if let Some(difference) = self.playlist_unchanged(playlist_id, items_before).await {
                run.say(ref_id, Step::Import, StepState::Note, difference.clone());
                differences.push(difference);
            }
            let made = imported.map_err(|e| link_failure("The import", &e))?;
            let index = self
                .host
                .song_index()
                .await
                .map_err(|e| link_failure("Looking at the imported song", &e))?;
            let found = index
                .iter()
                .find(|e| e.library == SONGS_LIBRARY && e.file == made);
            match found {
                Some(entry) if entry.multitracks_id == Some(id) => {
                    run.say(ref_id, Step::Import, StepState::Done, format!("Imported \"{}\".", made.trim_end_matches(".pro")));
                    (entry.library.clone(), entry.file.clone())
                }
                Some(entry) => {
                    return Err(failure(
                        "wrong_song",
                        format!(
                            "ProPresenter imported a different song (number {}) than the one asked for ({id}). It was not used.",
                            entry.multitracks_id.map_or_else(|| "unknown".to_string(), |n| n.to_string())
                        ),
                    ))
                }
                None => {
                    return Err(failure(
                        "wrong_song",
                        "The imported song did not show up in the Songs library, so it cannot be checked.",
                    ))
                }
            }
        } else {
            song.source_file
                .clone()
                .ok_or_else(|| failure("no_source", "The song to copy from is not known."))?
        };
        run.say(
            ref_id,
            Step::Copy,
            StepState::Running,
            format!("Making the copy from \"{library} / {file}\"."),
        );
        let source = self
            .host
            .read_file(&library, &file)
            .await
            .map_err(|e| link_failure("Reading the song", &e))?;
        let gen = self.new_uuid.clone();
        let mut next = move || gen();
        let copy = build_copy(
            &source,
            &CopyRequest {
                sections: &wanted.sections,
                key: wanted.key.as_deref(),
                arrangement_name: PLAYBACK_ARRANGEMENT,
                now: (self.now)(),
            },
            &mut next,
        )
        .map_err(|e| match e {
            ProError::Unmapped(_) => failure(
                "unmapped",
                format!("{} Nothing was added for this song.", e.words()),
            ),
            other => failure("copy_failed", other.words()),
        })?;
        let guesses = guesses_of(&copy.mapping);
        let digest = sha256_hex(&copy.bytes);
        let mut name = song.file.clone().unwrap_or_default();
        let base = super::plan::clean_name(&strip_n(&name));
        for attempt in 0..6u32 {
            match self
                .host
                .create_file(
                    PLAYBACK_LIBRARY,
                    &format!("{name}.pro"),
                    &copy.bytes,
                    &digest,
                )
                .await
            {
                Ok(()) => {
                    let key = match &copy.key {
                        KeyOutcome::Written(k) => format!(" The key {k} is saved in it."),
                        KeyOutcome::Unknown(k) => format!(" Playback's key \"{k}\" is not one StagePilot knows, so the song's own key stays."),
                        KeyOutcome::NotGiven => String::new(),
                    };
                    run.say(
                        ref_id,
                        Step::Copy,
                        StepState::Done,
                        format!("Made \"{name}\" in the Playback library.{key}"),
                    );
                    return Ok((name, guesses));
                }
                Err(LinkError::AlreadyExists(_)) => {
                    // Taken since the preview: the same song already says the right thing (an earlier try), or the next number.
                    if self.same_copy_exists(&name, id, wanted).await {
                        run.say(
                            ref_id,
                            Step::Copy,
                            StepState::Done,
                            format!(
                                "\"{name}\" is already there and plays this arrangement; using it."
                            ),
                        );
                        return Ok((name, guesses));
                    }
                    name = super::plan::copy_file_name(&base, next_number(&name) + attempt);
                }
                Err(e) => return Err(link_failure("Saving the copy", &e)),
            }
        }
        Err(failure(
            "name_taken",
            "No free name was found for the copy.",
        ))
    }

    /// `Ok` only when ProPresenter's layers say nothing is on screen (`slide`, `media`, `announcements`, `props`, `messages`
    /// all off; a camera input and audio do not count). An answer that cannot be read counts as live. Asked right before every
    /// screen step: nothing is typed or clicked unless this said yes.
    async fn screen_is_free(&self) -> Result<(), Failure> {
        match self.api.layers().await {
            Ok(layers) => {
                let showing = layers.on_screen();
                if showing.is_empty() {
                    Ok(())
                } else {
                    Err(failure(
                        "live_on_screen",
                        format!(
                            "ProPresenter is showing something on screen ({}). Clear it or wait, then press Try again.",
                            showing.join(", ")
                        ),
                    ))
                }
            }
            Err(_) => Err(failure(
                "live_on_screen",
                "StagePilot could not read what ProPresenter is showing on screen, so it treats the screen as live and does not touch it. Check that ProPresenter is open and its Network setting is on, then press Try again.",
            )),
        }
    }

    /// `None` when the playlist still has as many items as before the import step; else a sentence saying what differs.
    async fn playlist_unchanged(&self, playlist_id: &str, items_before: usize) -> Option<String> {
        match self.api.playlist(playlist_id).await {
            Ok(now) => {
                let after = items_of(&now).len();
                (after != items_before).then(|| {
                    format!(
                        "The playlist had {items_before} item(s) before the import and has {after} now. The import was meant to leave the playlist alone: look at it in ProPresenter."
                    )
                })
            }
            Err(error) => Some(format!(
                "The playlist could not be read to check that the import left it alone: {}",
                error.words()
            )),
        }
    }

    async fn same_copy_exists(&self, stem: &str, id: u64, wanted: &WantedSong) -> bool {
        let file = format!("{stem}.pro");
        let Ok(bytes) = self.host.read_file(PLAYBACK_LIBRARY, &file).await else {
            return false;
        };
        crate::pro_file::Song::read(&bytes).is_ok_and(|song| {
            song.multitracks_id == Some(id)
                && song.already_plays(&wanted.sections, wanted.key.as_deref())
        })
    }

    /// Waits until every `(ref_id, file)` has an id in ProPresenter's Playback library. Selects the library on screen once.
    async fn wait_loaded(
        &self,
        run: &Run<'_>,
        wanted: &[(String, String)],
    ) -> Result<HashMap<String, String>, Waited> {
        let mut found: HashMap<String, String> = HashMap::new();
        if wanted.is_empty() {
            return Ok(found);
        }
        run.say(
            None,
            Step::Load,
            StepState::Running,
            "Waiting for ProPresenter to load the copies.",
        );
        let started = tokio::time::Instant::now();
        let mut selected = false;
        let mut listed = false;
        let mut last_error: Option<ApiError> = None;
        loop {
            match self.playback_entries().await {
                Ok(entries) => {
                    listed = true;
                    for (ref_id, stem) in wanted {
                        if let Some(uuid) = entry_for(&entries, stem) {
                            found.insert(ref_id.clone(), uuid);
                        }
                    }
                }
                Err(error) => last_error = Some(error),
            }
            if found.len() == wanted.len() {
                run.say(
                    None,
                    Step::Load,
                    StepState::Done,
                    "ProPresenter has loaded the copies.",
                );
                return Ok(found);
            }
            let waited = started.elapsed();
            if waited >= self.timing.give_up {
                break;
            }
            if !selected && waited >= self.timing.select_after {
                selected = true;
                if let Err(Failure { text, .. }) = self.screen_is_free().await {
                    return Err(Waited::Live(text));
                }
                let said = match self.host.select_library(PLAYBACK_LIBRARY).await {
                    Ok(outcome) if outcome.tried => {
                        "Selected the Playback library on screen so ProPresenter loads it."
                            .to_string()
                    }
                    Ok(_) => {
                        "StagePilot could not select the Playback library on screen.".to_string()
                    }
                    Err(e) => format!(
                        "Selecting the Playback library on screen failed: {}",
                        e.words()
                    ),
                };
                run.say(None, Step::Load, StepState::Note, said);
            }
            tokio::time::sleep(self.timing.poll).await;
        }
        if !listed {
            if let Some(error) = last_error {
                return Err(Waited::Api(error));
            }
        }
        Err(Waited::Missing(
            wanted
                .iter()
                .filter(|(r, _)| !found.contains_key(r))
                .map(|(r, _)| r.clone())
                .collect(),
        ))
    }

    /// `propresenter-playlist-backups/<playlist>-<time>.json`, the newest [`KEEP_BACKUPS`] kept.
    fn write_backup(&self, playlist: &str, value: &Value) -> Result<PathBuf, String> {
        let fail = |e: std::io::Error| {
            format!("StagePilot could not save a backup of the playlist ({e}).")
        };
        std::fs::create_dir_all(&self.backup_dir).map_err(fail)?;
        let safe: String = playlist
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .take(40)
            .collect();
        let safe = safe.trim().to_string();
        let safe = if safe.is_empty() {
            "playlist".to_string()
        } else {
            safe
        };
        let when = chrono::DateTime::from_timestamp((self.now)(), 0)
            .map(|t| t.format("%Y%m%d-%H%M%S").to_string())
            .unwrap_or_else(|| "now".to_string());
        let mut path = self.backup_dir.join(format!("{safe}-{when}.json"));
        let mut n = 1;
        while path.exists() {
            n += 1;
            path = self.backup_dir.join(format!("{safe}-{when}-{n}.json"));
        }
        let temp = self.backup_dir.join(format!(".writing-{when}-{n}.tmp"));
        let body = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
        std::fs::write(&temp, body).map_err(fail)?;
        std::fs::rename(&temp, &path).map_err(fail)?;
        // Keep the newest twenty.
        let mut backups: Vec<(std::time::SystemTime, PathBuf)> =
            std::fs::read_dir(&self.backup_dir)
                .map_err(fail)?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
                .collect();
        backups.sort();
        let extra = backups.len().saturating_sub(KEEP_BACKUPS);
        for (_, old) in backups.into_iter().take(extra) {
            if old != path {
                let _ = std::fs::remove_file(old);
            }
        }
        Ok(path)
    }
}

enum Waited {
    Missing(Vec<String>),
    Api(ApiError),
    /// ProPresenter is showing something (or would not say): the screen was not touched. The words for the person.
    Live(String),
}

/// The id of the entry for a file in ProPresenter's listing of the Playback library.
fn entry_for(entries: &[LibraryEntry], stem: &str) -> Option<String> {
    entries
        .iter()
        .find(|e| e.display_name(PLAYBACK_LIBRARY) == stem)
        .and_then(|e| e.uuid.clone())
        .filter(|u| !u.is_empty())
}

/// `Song (3)` is `Song`.
fn strip_n(name: &str) -> String {
    match name.rfind(" (") {
        Some(open)
            if name.ends_with(')')
                && name[open + 2..name.len() - 1]
                    .bytes()
                    .all(|b| b.is_ascii_digit()) =>
        {
            name[..open].to_string()
        }
        _ => name.to_string(),
    }
}

/// The `n` of `Song (n)`, or 0.
fn next_number(name: &str) -> u32 {
    name.rfind(" (")
        .and_then(|open| name[open + 2..].strip_suffix(')')?.parse::<u32>().ok())
        .unwrap_or(0)
        + 1
}

fn number_of(item: &Value) -> Option<f64> {
    item.get("duration").and_then(Value::as_f64)
}

/// What the read-back shows that the plan did not say: order, names, kinds, durations and the arrangement of the new songs.
fn compare(sent: &[Value], new_from: usize, new_count: usize, back: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    if sent.len() != back.len() {
        out.push(format!(
            "The playlist has {} items now; {} were written.",
            back.len(),
            sent.len()
        ));
    }
    for (index, (want, got)) in sent.iter().zip(back).enumerate() {
        let place = format!("Item {} \"{}\"", index + 1, item_name(want));
        if item_kind(want) != item_kind(got) {
            out.push(format!(
                "{place}: it is a {} now, not a {}.",
                item_kind(got),
                item_kind(want)
            ));
        }
        if item_name(want) != item_name(got) {
            out.push(format!("{place}: it reads \"{}\" now.", item_name(got)));
        }
        let is_new = (new_from..new_from + new_count).contains(&index);
        if is_new {
            let arrangement = got["presentation_info"]["arrangement_name"]
                .as_str()
                .unwrap_or_default();
            if arrangement != PLAYBACK_ARRANGEMENT {
                out.push(format!(
                    "{place}: ProPresenter shows the arrangement \"{arrangement}\" instead of \"{PLAYBACK_ARRANGEMENT}\"."
                ));
            }
        } else {
            let (a, b) = (
                number_of(want).unwrap_or(0.0),
                number_of(got).unwrap_or(0.0),
            );
            if (a - b).abs() > 1e-6 {
                out.push(format!("{place}: its duration reads {b} now, it was {a}."));
            }
        }
    }
    out
}
