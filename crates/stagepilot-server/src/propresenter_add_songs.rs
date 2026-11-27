//! Adding the songs Playback plays to a ProPresenter playlist: the routes, the one job at a time, and the guards around the engine
//! (`stagepilot_propresenter::link::add_songs`).
//!
//! * `GET  /api/v1/propresenter-link/playlists` the playlists with their headers, for the pickers (kept for 10 s).
//! * `POST /api/v1/propresenter-link/add-songs/titles` which titles ProPresenter's Songs library has (for songs asked for by title).
//! * `POST /api/v1/propresenter-link/add-songs/preview` what would be done; writes nothing anywhere.
//! * `POST /api/v1/propresenter-link/add-songs/apply` does what a preview named, as a job (answers at once with its id).
//! * `GET  /api/v1/propresenter-link/add-songs/jobs/{id}` the job: what it has done, step by step, and the result.
//!
//! The rules around them: the same dashboard PIN, origin and remote-access rules as every other write route (the guards sit in
//! front of the whole router, so a remote viewer cannot even ask for a preview); both POSTs need the ProPresenter Link ready;
//! nothing starts and nothing goes on while a song is playing (Playback says playing, or StagePilot's own countdown runs);
//! one job at a time; the body is small, strictly checked, and holds numbers and names, never a path. A preview may also carry
//! `sources` (a song's `ref_id` to the file the person picked): it is lenient, what does not fit is ignored.

use crate::runtime::Runtime;
use crate::validation::{ok, Json};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Map, Value};
use stagepilot_core::state::TimerStatus;
use stagepilot_playback::library::{LibraryPlan, LibrarySong, LibraryState, MatchConfidence};
use stagepilot_propresenter::link::add_songs::{
    AddSongs, ApplyRefusal, ApplyReport, Event, Guard, Place, PlanError, PlanRequest, Prepared,
    Progress, Step, StepState, Timing, TitleFound, WantedSong, MAX_SONGS,
};
use stagepilot_propresenter::link::api::{ApiError, HttpApi, ProPresenterApi};
use stagepilot_propresenter::link::status::LinkState;
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Rt = Arc<Runtime>;

/// The playlist list is kept this long.
const PLAYLIST_CACHE: Duration = Duration::from_secs(10);
/// A preview can be applied for this long.
const PLAN_KEPT: Duration = Duration::from_secs(30 * 60);
const MAX_PLANS: usize = 16;
const MAX_JOBS: usize = 5;
const MAX_EVENTS: usize = 400;
const MAX_BODY: usize = 16 * 1024;
const MAX_PLAYLISTS_LISTED: usize = 80;

/// Which songs were asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Selection {
    Setlist(Vec<i64>),
    Content(Vec<u64>),
    /// Titles alone (no Playback data): each is the one Songs entry with that title, added as it is. In the order given.
    Titles(Vec<String>),
}

#[derive(Clone)]
struct Stored {
    selection: Selection,
    playlist_id: String,
    place: Place,
    /// The files the person picked, by the song's `ref_id`.
    sources: BTreeMap<String, String>,
    at: Instant,
}

struct Job {
    id: String,
    plan_id: String,
    started: chrono::DateTime<chrono::Utc>,
    songs: Vec<(String, String)>,
    events: Vec<Event>,
    report: Option<ApplyReport>,
    stopped: Option<String>,
}

#[derive(Clone)]
struct PlaylistInfo {
    id: String,
    name: String,
    folder: Option<String>,
    headers: Vec<String>,
}

#[derive(Default)]
struct Inner {
    plans: VecDeque<(String, Stored)>,
    jobs: VecDeque<Job>,
    running: Option<String>,
    playlists: Option<(Instant, String, Vec<PlaylistInfo>)>,
}

/// The state of this feature: previews waiting to be applied, the jobs, the playlist list.
pub struct AddSongsService {
    inner: Mutex<Inner>,
    backup_dir: Mutex<PathBuf>,
    api: Mutex<Option<Arc<dyn ProPresenterApi>>>,
    plan: Mutex<Option<LibraryPlan>>,
    timing: Mutex<Timing>,
}

impl AddSongsService {
    /// `backup_dir`: where the playlist backups go (beside the settings file).
    pub fn new(backup_dir: PathBuf) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            backup_dir: Mutex::new(backup_dir),
            api: Mutex::new(None),
            plan: Mutex::new(None),
            timing: Mutex::new(Timing::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Answers ProPresenter's API with this instead of a real connection (tests).
    #[doc(hidden)]
    pub fn set_api_for_test(&self, api: Option<Arc<dyn ProPresenterApi>>) {
        *self.api.lock().unwrap_or_else(|p| p.into_inner()) = api;
    }

    /// Other waits than the product's (tests).
    #[doc(hidden)]
    pub fn set_timing_for_test(&self, timing: Timing) {
        *self.timing.lock().unwrap_or_else(|p| p.into_inner()) = timing;
    }

    /// Where playlist backups are written.
    pub fn backup_dir(&self) -> PathBuf {
        self.backup_dir
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Writes playlist backups here (tests).
    #[doc(hidden)]
    pub fn set_backup_dir_for_test(&self, dir: PathBuf) {
        *self.backup_dir.lock().unwrap_or_else(|p| p.into_inner()) = dir;
    }

    /// Uses this as Playback's plan instead of asking the Library Link (tests).
    #[doc(hidden)]
    pub fn set_plan_for_test(&self, plan: Option<LibraryPlan>) {
        *self.plan.lock().unwrap_or_else(|p| p.into_inner()) = plan;
    }

    /// The stand-in for ProPresenter's API set by a test, if any.
    pub(crate) fn api_stand_in(&self) -> Option<Arc<dyn ProPresenterApi>> {
        self.api.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// The stand-in for Playback's plan set by a test, if any.
    pub(crate) fn plan_stand_in(&self) -> Option<LibraryPlan> {
        self.plan.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn api_for(&self, rt: &Runtime) -> Arc<dyn ProPresenterApi> {
        if let Some(api) = self.api.lock().unwrap_or_else(|p| p.into_inner()).clone() {
            return api;
        }
        Arc::new(HttpApi::new(
            &rt.settings_service.effective_snapshot().propresenter,
        ))
    }

    fn remember(&self, plan_id: &str, stored: Stored) {
        let mut inner = self.lock();
        inner
            .plans
            .retain(|(id, s)| id != plan_id && s.at.elapsed() < PLAN_KEPT);
        inner.plans.push_back((plan_id.to_string(), stored));
        while inner.plans.len() > MAX_PLANS {
            inner.plans.pop_front();
        }
    }

    fn recall(&self, plan_id: &str) -> Option<Stored> {
        self.lock()
            .plans
            .iter()
            .find(|(id, s)| id == plan_id && s.at.elapsed() < PLAN_KEPT)
            .map(|(_, s)| s.clone())
    }

    /// Takes the one place for a job; `false` while another job runs.
    fn begin_job(&self, id: &str) -> bool {
        let mut inner = self.lock();
        if inner.running.is_some() {
            return false;
        }
        inner.running = Some(id.to_string());
        true
    }

    fn release(&self, id: &str) {
        let mut inner = self.lock();
        if inner.running.as_deref() == Some(id) {
            inner.running = None;
        }
    }

    fn start_job(&self, job: Job) {
        let mut inner = self.lock();
        inner.jobs.push_back(job);
        while inner.jobs.len() > MAX_JOBS {
            inner.jobs.pop_front();
        }
    }

    fn event(&self, id: &str, event: Event) {
        let mut inner = self.lock();
        if let Some(job) = inner.jobs.iter_mut().find(|j| j.id == id) {
            if job.events.len() < MAX_EVENTS {
                job.events.push(event);
            }
        }
    }

    fn finish(&self, id: &str, report: Option<ApplyReport>, stopped: Option<String>) {
        let mut inner = self.lock();
        if let Some(job) = inner.jobs.iter_mut().find(|j| j.id == id) {
            job.report = report;
            job.stopped = stopped;
        }
        if inner.running.as_deref() == Some(id) {
            inner.running = None;
        }
        // The playlist has changed: the next list is read again.
        inner.playlists = None;
    }

    fn job_json(&self, id: &str) -> Option<Value> {
        let inner = self.lock();
        let job = inner.jobs.iter().find(|j| j.id == id)?;
        Some(render_job(job))
    }
}

fn step_name(step: Step) -> &'static str {
    match step {
        Step::Check => "check",
        Step::Backup => "backup",
        Step::Library => "library",
        Step::Import => "import",
        Step::Copy => "copy",
        Step::Load => "load",
        Step::Playlist => "playlist",
        Step::ReadBack => "read_back",
    }
}

fn state_name(state: StepState) -> &'static str {
    match state {
        StepState::Running => "running",
        StepState::Done => "done",
        StepState::Failed => "failed",
        StepState::Note => "note",
    }
}

/// The latest state of each step, in the order the steps first appeared.
fn steps_of(events: &[&Event]) -> Vec<Value> {
    let mut order: Vec<Step> = Vec::new();
    for event in events {
        if !order.contains(&event.step) {
            order.push(event.step);
        }
    }
    order
        .into_iter()
        .map(|step| {
            let mine: Vec<&&Event> = events.iter().filter(|e| e.step == step).collect();
            // The last event that is not a side note says where the step stands; notes are listed with it.
            let main = mine
                .iter()
                .rev()
                .find(|e| e.state != StepState::Note)
                .or(mine.last())
                .map(|e| (e.state, e.text.clone()))
                .unwrap_or((StepState::Running, String::new()));
            let notes: Vec<&str> = mine
                .iter()
                .filter(|e| e.state == StepState::Note)
                .map(|e| e.text.as_str())
                .collect();
            json!({"step": step_name(step), "state": state_name(main.0), "text": main.1, "notes": notes})
        })
        .collect()
}

fn render_job(job: &Job) -> Value {
    let overall: Vec<&Event> = job.events.iter().filter(|e| e.song.is_none()).collect();
    let songs: Vec<Value> = job
        .songs
        .iter()
        .map(|(ref_id, title)| {
            let mine: Vec<&Event> = job
                .events
                .iter()
                .filter(|e| e.song.as_deref() == Some(ref_id.as_str()))
                .collect();
            let result = job
                .report
                .as_ref()
                .and_then(|r| r.songs.iter().find(|s| &s.ref_id == ref_id))
                .and_then(|s| serde_json::to_value(s).ok());
            json!({"ref_id": ref_id, "title": title, "steps": steps_of(&mine), "result": result})
        })
        .collect();
    let state = if job.report.is_some() {
        "done"
    } else if job.stopped.is_some() {
        "failed"
    } else {
        "running"
    };
    json!({
        "id": job.id,
        "plan_id": job.plan_id,
        "state": state,
        "started_at": job.started.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "steps": steps_of(&overall),
        "songs": songs,
        "events": job.events,
        "report": job.report,
        "error": job.stopped,
    })
}

// ---- guards -----------------------------------------------------------------------------------------------------------

/// A song is playing: Playback says so, or StagePilot's own countdown runs.
fn song_is_playing(rt: &Runtime) -> bool {
    rt.playback
        .status()
        .heartbeat
        .is_some_and(|heartbeat| heartbeat.playing)
        || rt.store.snapshot().timer.status == TimerStatus::Running
}

const PLAYING_WORDS: &str = "A song is playing. Wait until it has ended, then try again.";

struct PlayingGuard(Rt);

impl Guard for PlayingGuard {
    fn check(&self) -> Result<(), String> {
        if song_is_playing(&self.0) {
            Err(PLAYING_WORDS.to_string())
        } else {
            Ok(())
        }
    }
}

struct JobProgress {
    rt: Rt,
    id: String,
}

impl Progress for JobProgress {
    fn event(&self, event: Event) {
        self.rt.propresenter_add_songs.event(&self.id, event);
    }
}

/// Marks the job as stopped when its task ends without an answer (a panic).
struct Ending {
    rt: Rt,
    id: String,
    done: bool,
}

impl Drop for Ending {
    fn drop(&mut self) {
        if !self.done {
            self.rt.propresenter_add_songs.finish(
                &self.id,
                None,
                Some("The job stopped unexpectedly. Look at the playlist in ProPresenter.".into()),
            );
        }
    }
}

// ---- answers ---------------------------------------------------------------------------------------------------------

fn coded(status: u16, code: &str, detail: &str) -> Json {
    Json(
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        json!({"detail": detail, "code": code}),
    )
}

fn invalid(detail: &str) -> Json {
    coded(422, "invalid", detail)
}

fn plan_error(error: &PlanError) -> Json {
    match error {
        PlanError::PlaylistNotFound => coded(404, "playlist_not_found", &error.words()),
        PlanError::NoSongs | PlanError::TooMany => invalid(&error.words()),
        PlanError::Api(ApiError::NoAnswer(_)) => {
            coded(502, "propresenter_unreachable", &error.words())
        }
        PlanError::Api(_) => coded(502, "propresenter_error", &error.words()),
        PlanError::Host(_) => coded(502, "link_error", &error.words()),
    }
}

fn refusal(error: &ApplyRefusal) -> Json {
    match error {
        ApplyRefusal::Plan(inner) => plan_error(inner),
        ApplyRefusal::PlanChanged => coded(409, "plan_changed", &error.words()),
        ApplyRefusal::PlaylistBlocked(_) => coded(409, "place_blocked", &error.words()),
        ApplyRefusal::StuckNotSkipped(ids) => Json(
            StatusCode::CONFLICT,
            json!({"detail": error.words(), "code": "songs_blocked", "songs": ids}),
        ),
        ApplyRefusal::UnknownSkip(ids) => Json(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({"detail": error.words(), "code": "unknown_skip", "songs": ids}),
        ),
        ApplyRefusal::NothingToAdd => coded(409, "nothing_to_add", &error.words()),
        ApplyRefusal::SongPlaying(_) => coded(409, "song_playing", PLAYING_WORDS),
    }
}

// ---- reading a request -----------------------------------------------------------------------------------------------

fn parse_object(body: &Bytes, allowed: &[&str]) -> Result<Map<String, Value>, Json> {
    if body.len() > MAX_BODY {
        return Err(coded(413, "too_large", "That request is too large."));
    }
    let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(body) else {
        return Err(invalid("Send a JSON object."));
    };
    if let Some(unknown) = map.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(invalid(&format!(
            "The field \"{unknown}\" is not known here."
        )));
    }
    Ok(map)
}

fn plain_text(value: &Value, what: &str, max: usize) -> Result<String, Json> {
    let text = value
        .as_str()
        .ok_or_else(|| invalid(&format!("{what} must be text.")))?;
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max || trimmed.chars().any(char::is_control)
    {
        return Err(invalid(&format!(
            "{what} must be 1 to {max} characters without control characters."
        )));
    }
    Ok(trimmed.to_string())
}

fn plain_id(value: &Value, what: &str) -> Result<String, Json> {
    let text = plain_text(value, what, 80)?;
    if !text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err(invalid(&format!(
            "{what} may only have letters, digits and dashes."
        )));
    }
    Ok(text)
}

fn whole_numbers<T: TryFrom<u64>>(value: &Value, what: &str) -> Result<Vec<T>, Json> {
    let list = value
        .as_array()
        .ok_or_else(|| invalid(&format!("{what} must be a list of numbers.")))?;
    if list.is_empty() || list.len() > MAX_SONGS {
        return Err(invalid(&format!(
            "{what} must have 1 to {MAX_SONGS} numbers."
        )));
    }
    let mut out = Vec::new();
    for item in list {
        let n = item
            .as_u64()
            .filter(|n| *n > 0 && *n < (1 << 53))
            .and_then(|n| T::try_from(n).ok())
            .ok_or_else(|| invalid(&format!("{what} must be whole numbers above zero.")))?;
        out.push(n);
    }
    Ok(out)
}

/// 1 to [`MAX_SONGS`] song titles: plain text, no control characters, each different.
fn title_list(value: &Value) -> Result<Vec<String>, Json> {
    let list = value
        .as_array()
        .ok_or_else(|| invalid("titles must be a list of song titles."))?;
    if list.is_empty() || list.len() > MAX_SONGS {
        return Err(invalid(&format!(
            "titles must have 1 to {MAX_SONGS} song titles."
        )));
    }
    let mut out: Vec<String> = Vec::new();
    for item in list {
        let title = plain_text(item, "A title", 120)?;
        if out.iter().any(|t| t.eq_ignore_ascii_case(&title)) {
            return Err(invalid("Each title can be named once."));
        }
        out.push(title);
    }
    Ok(out)
}

struct PreviewBody {
    selection: Selection,
    playlist_id: String,
    place: Place,
    sources: BTreeMap<String, String>,
}

/// The longest file text a pick may have (`Songs / <name>.pro`).
const MAX_PICK: usize = 240;

/// The picks of a preview request: song `ref_id` to the file as `choices` lists it. Never an error: what is not text, too long
/// or has a control character is left out, and the engine ignores a pick that is none of that song's files.
fn parse_sources(value: Option<&Value>) -> BTreeMap<String, String> {
    let Some(Value::Object(map)) = value else {
        return BTreeMap::new();
    };
    map.iter()
        .filter_map(|(ref_id, file)| {
            let file = file.as_str()?;
            let plain = |t: &str| {
                !t.is_empty() && t.chars().count() <= MAX_PICK && !t.chars().any(char::is_control)
            };
            (plain(ref_id) && plain(file)).then(|| (ref_id.clone(), file.to_string()))
        })
        .take(MAX_SONGS)
        .collect()
}

fn parse_preview(body: &Bytes) -> Result<PreviewBody, Json> {
    let map = parse_object(
        body,
        &[
            "setlist_song_ids",
            "content_ids",
            "titles",
            "playlist_id",
            "after_header",
            "place",
            "sources",
        ],
    )?;
    let selection = match (
        map.get("setlist_song_ids"),
        map.get("content_ids"),
        map.get("titles"),
    ) {
        (Some(ids), None, None) => {
            Selection::Setlist(whole_numbers::<i64>(ids, "setlist_song_ids")?)
        }
        (None, Some(ids), None) => Selection::Content(whole_numbers::<u64>(ids, "content_ids")?),
        (None, None, Some(titles)) => Selection::Titles(title_list(titles)?),
        (None, None, None) => {
            return Err(invalid("Send setlist_song_ids, content_ids or titles."));
        }
        _ => {
            return Err(invalid(
                "Send setlist_song_ids, content_ids or titles, only one of them.",
            ))
        }
    };
    let playlist_id = plain_id(
        map.get("playlist_id")
            .ok_or_else(|| invalid("Send playlist_id."))?,
        "playlist_id",
    )?;
    let place = match (map.get("after_header"), map.get("place")) {
        (Some(name), None) => Place::AfterHeader(plain_text(name, "after_header", 120)?),
        (None, Some(Value::String(p))) if p == "end" => Place::End,
        (None, Some(_)) => return Err(invalid("place can only be \"end\".")),
        (None, None) => {
            return Err(invalid(
                "Send after_header (a header's name) or place: \"end\".",
            ))
        }
        _ => return Err(invalid("Send after_header or place, not both.")),
    };
    Ok(PreviewBody {
        selection,
        playlist_id,
        place,
        sources: parse_sources(map.get("sources")),
    })
}

// ---- what is asked of Playback -----------------------------------------------------------------------------------------

fn wanted(
    song: &LibrarySong,
    plan_name: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> WantedSong {
    WantedSong {
        ref_id: song
            .setlist_song_id
            .map_or_else(|| format!("pos{}", song.position), |id| id.to_string()),
        title: song.title.clone(),
        position: song.position,
        content_id: song.content_id,
        key: song.key.clone(),
        sections: song.sections.iter().map(|s| s.name.clone()).collect(),
        rental_expired: song
            .rental_expires_utc
            .as_deref()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .is_some_and(|t| t.with_timezone(&chrono::Utc) <= now),
        as_is: false,
        plan_name: plan_name.map(str::to_string),
    }
}

/// The chosen songs of Playback's plan, by what the person named them by.
fn pick_songs(plan: &LibraryPlan, selection: &Selection) -> Result<Vec<WantedSong>, Json> {
    if let Selection::Titles(titles) = selection {
        return Ok(title_songs(titles));
    }
    let now = chrono::Utc::now();
    // A song a person paired by hand goes in under the plan song's own name; every other song under Playback's.
    let all: Vec<(&LibrarySong, Option<&str>)> = plan
        .songs
        .iter()
        .filter_map(|s| {
            let named = (s.confidence == Some(MatchConfidence::ByHand)).then_some(s.title.as_str());
            Some((s.library.as_ref()?, named))
        })
        .chain(plan.playback_only.iter().map(|s| (s, None)))
        .collect();
    let mut out: Vec<WantedSong> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    match selection {
        Selection::Setlist(ids) => {
            for id in ids {
                match all.iter().find(|(s, _)| s.setlist_song_id == Some(*id)) {
                    Some((song, named)) => out.push(wanted(song, *named, now)),
                    None => missing.push(id.to_string()),
                }
            }
        }
        Selection::Content(ids) => {
            for id in ids {
                match all.iter().find(|(s, _)| s.content_id == Some(*id)) {
                    Some((song, named)) => out.push(wanted(song, *named, now)),
                    None => missing.push(id.to_string()),
                }
            }
        }
        Selection::Titles(_) => {}
    }
    if !missing.is_empty() {
        return Err(Json(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({
                "detail": "Some of those songs are not in Playback's setlist now. Look at the setlist again.",
                "code": "unknown_song",
                "songs": missing
            }),
        ));
    }
    out.dedup_by(|a, b| a.ref_id == b.ref_id);
    Ok(out)
}

/// Songs named by their title alone: `t1`, `t2`... in the order asked, each to be added as it is in ProPresenter's Songs library.
fn title_songs(titles: &[String]) -> Vec<WantedSong> {
    titles
        .iter()
        .zip(1u32..)
        .map(|(title, position)| WantedSong {
            ref_id: format!("t{position}"),
            title: title.clone(),
            position,
            content_id: None,
            key: None,
            sections: Vec::new(),
            rental_expired: false,
            as_is: true,
            plan_name: None,
        })
        .collect()
}

/// The Playback plan as the window gets it.
async fn library_plan(rt: &Rt) -> Result<LibraryPlan, Json> {
    if let Some(plan) = rt
        .propresenter_add_songs
        .plan
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
    {
        return Ok(plan);
    }
    let settings = crate::playback_library::effective_settings(rt);
    let loaded = rt.store.snapshot().plan;
    let plan = rt
        .playback_library
        .plan(&settings, loaded.as_ref(), false)
        .await;
    if !plan.enabled || plan.state != LibraryState::Ready {
        return Err(coded(
            409,
            "library_not_ready",
            plan.message
                .as_deref()
                .unwrap_or("The Playback Library Link is not connected."),
        ));
    }
    Ok(plan)
}

/// The songs a selection names: from Playback's plan, or (by title) with no Playback data at all.
async fn songs_for(rt: &Rt, selection: &Selection) -> Result<Vec<WantedSong>, Json> {
    if let Selection::Titles(titles) = selection {
        return Ok(title_songs(titles));
    }
    let plan = library_plan(rt).await?;
    pick_songs(&plan, selection)
}

/// The engine for the saved ProPresenter computer, when the Link is ready.
async fn engine(rt: &Rt) -> Result<AddSongs, Json> {
    let settings = rt.settings_service.effective_snapshot().propresenter;
    if !settings.enabled {
        return Err(coded(
            409,
            "no_propresenter",
            crate::propresenter_link::NO_COMPUTER,
        ));
    }
    let status = rt.propresenter_link.status(false).await;
    if status.state != LinkState::Ready {
        return Err(coded(409, "link_not_ready", &status.message));
    }
    let Some((_, host)) = rt.propresenter_link.connect().await else {
        return Err(coded(
            409,
            "no_propresenter",
            crate::propresenter_link::NO_COMPUTER,
        ));
    };
    let service = &rt.propresenter_add_songs;
    let timing = *service.timing.lock().unwrap_or_else(|p| p.into_inner());
    Ok(AddSongs::new(
        host,
        service.api_for(rt),
        service.backup_dir(),
        Arc::new(|| chrono::Utc::now().timestamp()),
        Arc::new(|| uuid::Uuid::new_v4().to_string().to_uppercase()),
    )
    .with_timing(timing))
}

// ---- the routes -----------------------------------------------------------------------------------------------------

/// `GET /api/v1/propresenter-link/playlists`: every playlist (folders flattened) with the names of its headers.
async fn playlists(State(rt): State<Rt>) -> Json {
    let settings = rt.settings_service.effective_snapshot().propresenter;
    if !settings.enabled {
        return coded(
            409,
            "no_propresenter",
            crate::propresenter_link::NO_COMPUTER,
        );
    }
    let key = format!("{}:{}", settings.host, settings.port);
    {
        let inner = rt.propresenter_add_songs.lock();
        if let Some((at, for_key, list)) = &inner.playlists {
            if *for_key == key && at.elapsed() < PLAYLIST_CACHE {
                return ok(playlists_json(list));
            }
        }
    }
    let api = rt.propresenter_add_songs.api_for(&rt);
    let listed = match api.playlists().await {
        Ok(listed) => listed,
        Err(error) => return plan_error(&PlanError::Api(error)),
    };
    let reads = listed.iter().take(MAX_PLAYLISTS_LISTED).map(|p| {
        let api = api.clone();
        let uuid = p.uuid.clone();
        async move {
            api.playlist(&uuid).await.ok().map(|value| {
                value
                    .get("items")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter(|i| i.get("type").and_then(Value::as_str) == Some("header"))
                            .filter_map(|i| i["id"]["name"].as_str().map(str::to_string))
                            .collect::<Vec<String>>()
                    })
                    .unwrap_or_default()
            })
        }
    });
    let headers = futures_util::future::join_all(reads).await;
    let list: Vec<PlaylistInfo> = listed
        .iter()
        .take(MAX_PLAYLISTS_LISTED)
        .zip(headers)
        .map(|(p, headers)| PlaylistInfo {
            id: p.uuid.clone(),
            name: p.name.clone(),
            folder: p.folder.clone(),
            headers: headers.unwrap_or_default(),
        })
        .collect();
    let body = playlists_json(&list);
    rt.propresenter_add_songs.lock().playlists = Some((Instant::now(), key, list));
    ok(body)
}

fn playlists_json(list: &[PlaylistInfo]) -> Value {
    json!({"playlists": list.iter().map(|p| json!({
        "id": p.id, "name": p.name, "folder": p.folder, "headers": p.headers
    })).collect::<Vec<_>>()})
}

/// `POST /api/v1/propresenter-link/add-songs/titles` `{titles: [...]}`: which of those titles ProPresenter's Songs library has.
/// Reads file names only; nothing is written and nothing is asked of the screen.
async fn titles(State(rt): State<Rt>, body: Bytes) -> Json {
    let map = match parse_object(&body, &["titles"]) {
        Ok(map) => map,
        Err(error) => return error,
    };
    let list = match map.get("titles").map(title_list) {
        Some(Ok(list)) => list,
        Some(Err(error)) => return error,
        None => return invalid("Send titles."),
    };
    let engine = match engine(&rt).await {
        Ok(engine) => engine,
        Err(error) => return error,
    };
    match engine.find_titles(&list).await {
        Ok(found) => ok(json!({"titles": found.iter().map(|m| json!({
            "title": m.title,
            "state": match m.found {
                TitleFound::Found => "found",
                TitleFound::Missing => "missing",
                TitleFound::Several => "several",
            },
        })).collect::<Vec<_>>()})),
        Err(error) => plan_error(&error),
    }
}

/// `POST /api/v1/propresenter-link/add-songs/preview`.
async fn preview(State(rt): State<Rt>, body: Bytes) -> Json {
    let asked = match parse_preview(&body) {
        Ok(asked) => asked,
        Err(error) => return error,
    };
    let engine = match engine(&rt).await {
        Ok(engine) => engine,
        Err(error) => return error,
    };
    let songs = match songs_for(&rt, &asked.selection).await {
        Ok(songs) => songs,
        Err(error) => return error,
    };
    let request = PlanRequest {
        songs,
        playlist_id: asked.playlist_id.clone(),
        place: asked.place.clone(),
        sources: asked.sources.clone(),
    };
    match engine.preview(&request).await {
        Ok(made) => {
            rt.propresenter_add_songs.remember(
                &made.plan_id,
                Stored {
                    selection: asked.selection,
                    playlist_id: asked.playlist_id,
                    place: asked.place,
                    sources: asked.sources,
                    at: Instant::now(),
                },
            );
            ok(serde_json::to_value(&made).unwrap_or(Value::Null))
        }
        Err(error) => plan_error(&error),
    }
}

/// `POST /api/v1/propresenter-link/add-songs/apply` `{plan_id, skip: [...]}`.
async fn apply(State(rt): State<Rt>, body: Bytes) -> Json {
    let map = match parse_object(&body, &["plan_id", "skip"]) {
        Ok(map) => map,
        Err(error) => return error,
    };
    let plan_id = match map.get("plan_id").and_then(Value::as_str) {
        Some(id) if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) => id.to_string(),
        _ => return invalid("plan_id is the number the preview gave."),
    };
    let skip: Vec<String> = match map.get("skip") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(list)) if list.len() <= MAX_SONGS => {
            let mut out = Vec::new();
            for item in list {
                match item.as_str() {
                    Some(id)
                        if !id.is_empty()
                            && id.len() <= 40
                            && id
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') =>
                    {
                        out.push(id.to_string());
                    }
                    _ => return invalid("skip is a list of the songs' ids from the preview."),
                }
            }
            out
        }
        _ => return invalid("skip is a list of the songs' ids from the preview."),
    };
    let service = &rt.propresenter_add_songs;
    let Some(stored) = service.recall(&plan_id) else {
        return coded(
            404,
            "unknown_plan",
            "That preview is too old or not known. Look at the preview again.",
        );
    };
    let job_id = uuid::Uuid::new_v4().simple().to_string();
    if !service.begin_job(&job_id) {
        return coded(
            409,
            "busy",
            "StagePilot is already adding songs. Wait until it has finished.",
        );
    }
    // From here every way out must give the place back.
    let started = prepare(&rt, &stored, &plan_id, &skip).await;
    let (engine, prepared) = match started {
        Ok(started) => started,
        Err(error) => {
            service.release(&job_id);
            return error;
        }
    };
    let songs: Vec<(String, String)> = prepared
        .plan
        .songs
        .iter()
        .map(|s| (s.ref_id.clone(), s.title.clone()))
        .collect();
    service.start_job(Job {
        id: job_id.clone(),
        plan_id: plan_id.clone(),
        started: chrono::Utc::now(),
        songs,
        events: Vec::new(),
        report: None,
        stopped: None,
    });
    let task_rt = rt.clone();
    let task_id = job_id.clone();
    let playlist_id = stored.playlist_id.clone();
    tokio::spawn(async move {
        let mut ending = Ending {
            rt: task_rt.clone(),
            id: task_id.clone(),
            done: false,
        };
        let progress = JobProgress {
            rt: task_rt.clone(),
            id: task_id.clone(),
        };
        let guard = PlayingGuard(task_rt.clone());
        let report = engine.run(&prepared, &guard, &progress).await;
        // Following needs to know where each song went: remembered now, while the playlist is as the job left it.
        let files: std::collections::BTreeSet<String> = report
            .songs
            .iter()
            .filter(|s| {
                matches!(
                    s.status,
                    stagepilot_propresenter::link::add_songs::SongStatus::Added
                        | stagepilot_propresenter::link::add_songs::SongStatus::AlreadyThere
                )
            })
            .filter_map(|s| s.file.as_ref().map(|f| format!("{f}.pro")))
            .collect();
        if report.playlist_changed || !files.is_empty() {
            task_rt
                .propresenter_follow
                .remember_after_add(&task_rt, &playlist_id, files)
                .await;
        }
        ending.done = true;
        task_rt
            .propresenter_add_songs
            .finish(&task_id, Some(report), None);
    });
    Json(
        StatusCode::ACCEPTED,
        json!({"job_id": job_id, "plan_id": plan_id, "state": "running"}),
    )
}

/// Everything that is checked before a job may start.
async fn prepare(
    rt: &Rt,
    stored: &Stored,
    plan_id: &str,
    skip: &[String],
) -> Result<(AddSongs, Prepared), Json> {
    if song_is_playing(rt) {
        return Err(coded(409, "song_playing", PLAYING_WORDS));
    }
    let engine = engine(rt).await?;
    let songs = songs_for(rt, &stored.selection).await?;
    let request = PlanRequest {
        songs,
        playlist_id: stored.playlist_id.clone(),
        place: stored.place.clone(),
        sources: stored.sources.clone(),
    };
    let guard = PlayingGuard(rt.clone());
    let prepared = engine
        .begin(&request, plan_id, skip, &guard)
        .await
        .map_err(|e| refusal(&e))?;
    Ok((engine, prepared))
}

/// `GET /api/v1/propresenter-link/add-songs/jobs/{id}`.
async fn job(State(rt): State<Rt>, Path(id): Path<String>) -> Json {
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return coded(404, "unknown_job", "No such job.");
    }
    match rt.propresenter_add_songs.job_json(&id) {
        Some(job) => ok(job),
        None => coded(404, "unknown_job", "No such job."),
    }
}

pub fn routes() -> Router<Rt> {
    Router::new()
        .route("/api/v1/propresenter-link/playlists", get(playlists))
        .route("/api/v1/propresenter-link/add-songs/titles", post(titles))
        .route("/api/v1/propresenter-link/add-songs/preview", post(preview))
        .route("/api/v1/propresenter-link/add-songs/apply", post(apply))
        .route("/api/v1/propresenter-link/add-songs/jobs/{id}", get(job))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(value: Value) -> Bytes {
        Bytes::from(value.to_string())
    }

    #[test]
    fn a_preview_body_is_checked_strictly() {
        let good = parse_preview(&body(json!({
            "setlist_song_ids": [3, 1, 2], "playlist_id": "ABC-123", "after_header": " Worship "
        })))
        .map_err(|_| "refused")
        .unwrap();
        assert_eq!(good.selection, Selection::Setlist(vec![3, 1, 2]));
        assert_eq!(good.place, Place::AfterHeader("Worship".into()));
        let end = parse_preview(&body(json!({
            "content_ids": [5000001], "playlist_id": "x", "place": "end"
        })))
        .map_err(|_| "refused")
        .unwrap();
        assert_eq!(end.selection, Selection::Content(vec![5_000_001]));
        assert_eq!(end.place, Place::End);
        let titled = parse_preview(&body(json!({
            "titles": [" Holy Forever ", "Gratitude"], "playlist_id": "x", "place": "end"
        })))
        .map_err(|_| "refused")
        .unwrap();
        assert_eq!(
            titled.selection,
            Selection::Titles(vec!["Holy Forever".into(), "Gratitude".into()])
        );
        let bad = |value: Value| parse_preview(&body(value)).is_err();
        assert!(bad(
            json!({"titles": ["A"], "setlist_song_ids": [1], "playlist_id": "x", "place": "end"})
        ));
        assert!(bad(
            json!({"titles": ["A", "a"], "playlist_id": "x", "place": "end"})
        ));
        assert!(bad(
            json!({"titles": [], "playlist_id": "x", "place": "end"})
        ));
        assert!(bad(json!({"playlist_id": "x", "place": "end"})), "no songs");
        assert!(bad(
            json!({"setlist_song_ids": [1], "content_ids": [2], "playlist_id": "x", "place": "end"})
        ));
        assert!(bad(
            json!({"setlist_song_ids": [], "playlist_id": "x", "place": "end"})
        ));
        assert!(bad(
            json!({"setlist_song_ids": [0], "playlist_id": "x", "place": "end"})
        ));
        assert!(bad(
            json!({"setlist_song_ids": [-1], "playlist_id": "x", "place": "end"})
        ));
        assert!(bad(
            json!({"setlist_song_ids": [1.5], "playlist_id": "x", "place": "end"})
        ));
        assert!(bad(
            json!({"setlist_song_ids": ["1"], "playlist_id": "x", "place": "end"})
        ));
        assert!(bad(
            json!({"setlist_song_ids": vec![1; 61], "playlist_id": "x", "place": "end"})
        ));
        assert!(bad(
            json!({"setlist_song_ids": [1], "playlist_id": "../x", "place": "end"})
        ));
        assert!(
            bad(json!({"setlist_song_ids": [1], "playlist_id": "x"})),
            "no place"
        );
        assert!(bad(
            json!({"setlist_song_ids": [1], "playlist_id": "x", "place": "start"})
        ));
        assert!(bad(
            json!({"setlist_song_ids": [1], "playlist_id": "x", "place": "end", "after_header": "A"})
        ));
        assert!(bad(
            json!({"setlist_song_ids": [1], "playlist_id": "x", "after_header": "A\nB"})
        ));
        assert!(bad(
            json!({"setlist_song_ids": [1], "playlist_id": "x", "after_header": "  "})
        ));
        assert!(bad(
            json!({"setlist_song_ids": [1], "playlist_id": "x", "after_header": "a".repeat(121)})
        ));
        assert!(bad(
            json!({"setlist_song_ids": [1], "playlist_id": "x", "place": "end", "path": "/etc"})
        ));
        assert!(parse_preview(&Bytes::from_static(b"[1]")).is_err());
        // The picks are lenient: what does not fit is left out and never refuses the preview.
        let picks = |sources: Value| {
            parse_preview(&body(json!({
                "setlist_song_ids": [1], "playlist_id": "x", "place": "end", "sources": sources
            })))
            .map_err(|_| "refused")
            .unwrap()
            .sources
        };
        assert_eq!(
            picks(
                json!({"1": "Songs / A.pro", "2": 5, "3": null, "": "Songs / A.pro", "4": "x\ny"})
            ),
            BTreeMap::from([("1".to_string(), "Songs / A.pro".to_string())])
        );
        assert!(picks(json!("Songs / A.pro")).is_empty());
        assert!(picks(json!(["a"])).is_empty());
        assert!(picks(json!({"1": "y".repeat(241)})).is_empty());
        assert!(parse_preview(&Bytes::from(vec![b' '; MAX_BODY + 1])).is_err());
    }
}
