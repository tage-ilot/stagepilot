//! The follower: every ~100 ms it is told where Playback is, and it keeps ProPresenter's slide where the song is (design
//! `SPEC.md`, rules 5, 6 and 9). It holds no clock, no task and no settings of its own: the caller gives it the time, the
//! position, the switch and the schedule of the playing song, and an API to speak to ProPresenter with, so a test can drive it
//! step by step against the fake ProPresenter.
//!
//! * The wanted slide is the last schedule step with `time <= position + offset` (flat slide 0, the Count Off, before the
//!   first one). Because it comes from the POSITION, seeks and section loops are followed by themselves.
//! * A song that starts playing is shown with the playlist item (cue 0), the active presentation is read to be sure it is the
//!   song's `Playback` copy, and then the wanted slide is triggered at once (a song started in the middle works the same).
//! * Paused, stopped or ended: nothing is triggered, the screen keeps its slide.
//! * After every trigger the expected slide is remembered. ProPresenter's current slide is read about four times a second;
//!   when it is not the expected one, more than 0.6 s after the last trigger, a person clicked: the song is held until its
//!   schedule moves into another section.
//! * A trigger that fails is not tried again for a second; five in a row pause following until the next song or a manual
//!   `Try again`. The first failure and then one a minute are logged.
//! * With the switch off nothing at all is sent to ProPresenter.
//! * The position is never waited for: it is the anchor Playback last gave (a heartbeat, or the earlier transport and seek
//!   event, see `stagepilot_playback::anchor`) moved on by the time since, so a slide is triggered at its cue within one 100 ms
//!   step. A song that STARTS from the top (`PlaybackNow::starts` moved) shows its playlist item again, at once.
//! * Every trigger is written down (`TriggerRecord`, the last 20): how late it was against its cue, how long ProPresenter took
//!   to answer, and when the poll first saw the slide. One DEBUG line per trigger, never INFO.

use super::binding::playlist_items;
use super::prepare::Prepared;
use crate::link::api::{ApiError, ProPresenterApi, SlidePosition};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use ts_rs::TS;

/// Shown while a person's click holds the song.
pub const HOLD_REASON: &str = "Paused: someone changed the slide in ProPresenter";
/// The summary while Playback is not playing the song (stopped or paused): nothing is being followed at that moment.
pub const READY_WAITING: &str = "Ready: waiting for Playback to play the song";
/// How many triggers the status keeps.
pub const RECENT_TRIGGERS: usize = 20;
/// A slide that the poll did not see this long after the trigger is given up (its record keeps no `seen_ms`).
const SEEN_GIVE_UP_S: f64 = 5.0;
/// A slide change later than this after its cue is not the cue's own timing: the position jumped (a seek, a loop, a song started in
/// the middle).
const CATCH_UP_S: f64 = 1.0;

/// The waits and limits of following. The defaults are the product's; tests may change them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    /// How often ProPresenter's current slide is read.
    pub poll_s: f64,
    /// ... and this often while a trigger waits to be seen (so `seen_ms` is not quantised to `poll_s`).
    pub fast_poll_s: f64,
    /// A slide that differs from the expected one counts as a click only this long after StagePilot's own trigger.
    pub grace_s: f64,
    /// The playlist item is triggered again at most this often (the wrong presentation is on screen).
    pub refire_s: f64,
    /// A failed trigger is not tried again for this long.
    pub retry_s: f64,
    /// One warning in the log per this long while triggers fail.
    pub warn_every_s: f64,
    /// Failed triggers in a row that pause following.
    pub max_failures: u32,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll_s: 0.25,
            fast_poll_s: 0.1,
            grace_s: 0.6,
            refire_s: 2.0,
            retry_s: 1.0,
            warn_every_s: 60.0,
            max_failures: 5,
        }
    }
}

/// Where Playback is: seconds into the song (extrapolated by the caller while it plays) and whether it plays.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlaybackNow {
    pub position: f64,
    pub playing: bool,
    /// Seconds since the heartbeat or event the position was moved on from was received.
    pub age_s: f64,
    /// Moves once for every song that starts from the top (not for a pause, a resume, a seek or a heartbeat). When it moves while
    /// the song plays, the song's playlist item is shown again.
    pub starts: u64,
}

impl PlaybackNow {
    pub fn new(position: f64, playing: bool) -> Self {
        Self {
            position,
            playing,
            age_s: 0.0,
            starts: 0,
        }
    }
}

/// The plan song Playback is on, as the follower needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct SongView {
    pub key: String,
    pub title: String,
    /// The words that say whether and how the song is followed.
    pub readiness_text: String,
    pub prepared: Option<Arc<Prepared>>,
}

/// One look at the world, given to [`Follower::step`].
pub struct Input<'a> {
    /// Seconds on any steady clock.
    pub now: f64,
    pub enabled: bool,
    pub offset_ms: i32,
    /// `None` when Playback is not connected or has no song.
    pub playback: Option<PlaybackNow>,
    /// `None` when Playback's song is not a song of the plan.
    pub song: Option<&'a SongView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowState {
    /// The switch is off.
    Off,
    /// Nothing to follow: Playback has no song, or one that is not in the plan.
    Idle,
    /// A followable song is loaded and not playing yet.
    Ready,
    Following,
    /// A person's click holds the song until the next section.
    Held,
    /// Paused after failures, until the next song or `Try again`.
    Paused,
    /// The song that plays cannot be followed (the message says why).
    NotFollowed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SectionNow {
    pub index: usize,
    pub name: String,
}

/// What the status shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FollowStatus {
    pub state: FollowState,
    pub message: String,
    pub song_key: Option<String>,
    pub song_title: Option<String>,
    pub section: Option<SectionNow>,
    /// ProPresenter's slide number (flat, in the `Playback` arrangement) StagePilot last showed.
    pub slide: Option<usize>,
    pub hold_reason: Option<String>,
}

impl FollowStatus {
    fn bare(state: FollowState, message: &str) -> Self {
        Self {
            state,
            message: message.to_string(),
            song_key: None,
            song_title: None,
            section: None,
            slide: None,
            hold_reason: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TriggerKind {
    /// The song's playlist item (its first slide with the song's background).
    Item,
    /// A slide of the song's presentation.
    Slide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TriggerWhy {
    /// The song started playing.
    Play,
    /// The position reached the slide's cue.
    Cue,
    /// The position jumped (seek, loop, a song started in the middle): the cue was passed long ago, `late_ms` says nothing.
    CatchUp,
}

/// What one trigger cost, for the rig (numbers in milliseconds, one decimal).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct TriggerRecord {
    /// 1, 2, 3, ... in the order sent (this process).
    pub seq: u32,
    pub song: String,
    pub kind: TriggerKind,
    pub why: TriggerWhy,
    /// ProPresenter's slide number (flat) that was triggered; 0 for the item.
    pub slide: u32,
    /// ProPresenter took it.
    pub ok: bool,
    /// How long after its moment on Playback's clock it was sent: for a slide, the position (with the offset) minus the cue's time
    /// when the request left; for the item, the time since the follower learned the song plays. `None` where there is no such
    /// moment (the slide before the first cue).
    pub late_ms: Option<f64>,
    /// How long ProPresenter took to answer the trigger.
    pub answer_ms: f64,
    /// How long after the trigger the status poll first saw that slide on screen; `None` until (or unless) it did.
    pub seen_ms: Option<f64>,
}

fn ms(seconds: f64) -> f64 {
    (seconds * 10_000.0).round() / 10.0
}

/// The part of the follower that other tasks and the routes reach: the last status, and the `Try again` request.
#[derive(Clone)]
pub struct FollowHandle {
    status: Arc<Mutex<FollowStatus>>,
    try_again: Arc<AtomicBool>,
    recent: Arc<Mutex<Vec<TriggerRecord>>>,
}

impl Default for FollowHandle {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(FollowStatus::bare(
                FollowState::Off,
                "Switch is off",
            ))),
            try_again: Arc::new(AtomicBool::new(false)),
            recent: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl FollowHandle {
    pub fn status(&self) -> FollowStatus {
        self.status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// The last triggers (at most [`RECENT_TRIGGERS`]), oldest first.
    pub fn recent_triggers(&self) -> Vec<TriggerRecord> {
        self.recent
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Asks the follower to clear a failure pause (the next step does it).
    pub fn try_again(&self) {
        self.try_again.store(true, Ordering::SeqCst);
    }

    fn set(&self, status: FollowStatus) {
        *self.status.lock().unwrap_or_else(|p| p.into_inner()) = status;
    }
}

#[derive(Debug, Clone, Copy)]
struct Hold {
    /// The section the person clicked in.
    section: usize,
}

struct Run {
    key: String,
    /// (playlist id, presentation uuid) the run was started for.
    target: (String, String),
    /// The playlist item was shown and the presentation checked.
    started: bool,
    last_item_at: Option<f64>,
    /// The slide StagePilot believes is on screen; `None` when that is not known (nothing shown yet, or a click was seen).
    last_fired: Option<usize>,
    last_fire_at: f64,
    last_poll_at: f64,
    hold: Option<Hold>,
    failures: u32,
    paused: bool,
    last_error: Option<String>,
    retry_at: f64,
    last_warn_at: Option<f64>,
    /// The `starts` count this run has dealt with.
    seen_starts: u64,
    /// When (on the input's clock) the follower learned the song plays and the item was not shown yet.
    signal_at: Option<f64>,
}

impl Run {
    fn new(key: &str, target: (String, String)) -> Self {
        Self {
            key: key.to_string(),
            target,
            started: false,
            last_item_at: None,
            last_fired: None,
            last_fire_at: f64::NEG_INFINITY,
            last_poll_at: f64::NEG_INFINITY,
            hold: None,
            failures: 0,
            paused: false,
            last_error: None,
            retry_at: f64::NEG_INFINITY,
            last_warn_at: None,
            seen_starts: 0,
            signal_at: None,
        }
    }

    fn succeeded(&mut self) {
        self.failures = 0;
        self.last_error = None;
    }

    fn failed(&mut self, now: f64, timing: &Timing, why: &str) {
        self.failures += 1;
        self.last_error = Some(why.to_string());
        self.retry_at = now + timing.retry_s;
        if self
            .last_warn_at
            .is_none_or(|w| now - w >= timing.warn_every_s)
        {
            self.last_warn_at = Some(now);
            tracing::warn!(failures = self.failures, "presenter_follow_trigger_failed");
        }
        if self.failures >= timing.max_failures {
            self.paused = true;
        }
    }
}

/// Why showing the song's playlist item did not work.
enum FireError {
    Api(ApiError),
    NotInPlaylist,
    Wrong,
}

impl FireError {
    fn words(&self) -> String {
        match self {
            Self::Api(e) => e.words(),
            Self::NotInPlaylist => "The song is not in the ProPresenter playlist any more.".into(),
            Self::Wrong => "ProPresenter did not show the song's presentation.".into(),
        }
    }
}

impl From<ApiError> for FireError {
    fn from(e: ApiError) -> Self {
        Self::Api(e)
    }
}

/// Shows the song's playlist item (its first slide) and checks that its presentation is the active one. Gives back the clock's
/// reading when the trigger request was sent and how long ProPresenter took to answer it (seconds on `clock`).
async fn fire_item(
    api: &dyn ProPresenterApi,
    prepared: &Prepared,
    clock: &Clock,
) -> Result<(f64, f64), FireError> {
    let binding = &prepared.binding;
    let playlist = api.playlist(&binding.playlist_id).await?;
    let item = playlist_items(&playlist)
        .into_iter()
        .find(|i| i.presentation_uuid == binding.presentation_uuid)
        .ok_or(FireError::NotInPlaylist)?;
    let sent = clock();
    api.trigger_playlist_item(&binding.playlist_id, item.index)
        .await?;
    let answer = clock() - sent;
    match api.active_presentation().await? {
        Some(uuid) if uuid == binding.presentation_uuid => Ok((sent, answer)),
        _ => Err(FireError::Wrong),
    }
}

/// Seconds on a steady clock; measures how long ProPresenter takes (the engine's own `now` is the caller's).
pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

fn steady_clock() -> Clock {
    let began = std::time::Instant::now();
    Arc::new(move || began.elapsed().as_secs_f64())
}

struct Pending {
    seq: u32,
    sent_at: f64,
    slide: usize,
    presentation: String,
}

/// The last triggers and the ones whose slide the poll has not seen yet.
struct Recorder {
    clock: Clock,
    records: VecDeque<TriggerRecord>,
    seq: u32,
    pending: Vec<Pending>,
    changed: bool,
}

/// One trigger as it left, before it is written down.
struct Sent<'a> {
    song: &'a str,
    kind: TriggerKind,
    why: TriggerWhy,
    slide: usize,
    ok: bool,
    late_s: Option<f64>,
    answer_s: f64,
    now: f64,
    presentation: &'a str,
}

impl Recorder {
    fn new(clock: Clock) -> Self {
        Self {
            clock,
            records: VecDeque::new(),
            seq: 0,
            pending: Vec::new(),
            changed: false,
        }
    }

    fn add(&mut self, sent: Sent<'_>) {
        self.seq = self.seq.wrapping_add(1);
        let record = TriggerRecord {
            seq: self.seq,
            song: sent.song.to_string(),
            kind: sent.kind,
            why: sent.why,
            slide: u32::try_from(sent.slide).unwrap_or(u32::MAX),
            ok: sent.ok,
            late_ms: sent.late_s.map(ms),
            answer_ms: ms(sent.answer_s.max(0.0)),
            seen_ms: None,
        };
        if sent.ok {
            self.pending.push(Pending {
                seq: record.seq,
                sent_at: sent.now,
                slide: sent.slide,
                presentation: sent.presentation.to_string(),
            });
        } else {
            log_trigger(&record);
        }
        self.records.push_back(record);
        while self.records.len() > RECENT_TRIGGERS {
            self.records.pop_front();
        }
        self.changed = true;
    }

    fn waiting(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The poll saw `position` on screen at `now`.
    fn seen(&mut self, now: f64, position: Option<&SlidePosition>) {
        let Some(position) = position else { return };
        let mut i = 0;
        while i < self.pending.len() {
            let p = &self.pending[i];
            if p.slide == position.index && p.presentation == position.presentation_uuid {
                let (seq, seen) = (p.seq, ms(now - p.sent_at));
                self.pending.remove(i);
                self.finish(seq, Some(seen));
            } else {
                i += 1;
            }
        }
    }

    /// Triggers the poll did not see within [`SEEN_GIVE_UP_S`] are finished without it.
    fn expire(&mut self, now: f64) {
        let mut i = 0;
        while i < self.pending.len() {
            if now - self.pending[i].sent_at > SEEN_GIVE_UP_S {
                let seq = self.pending.remove(i).seq;
                self.finish(seq, None);
            } else {
                i += 1;
            }
        }
    }

    /// Everything still waiting is finished (the song or the switch changed).
    fn drop_pending(&mut self) {
        for p in std::mem::take(&mut self.pending) {
            self.finish(p.seq, None);
        }
    }

    fn finish(&mut self, seq: u32, seen_ms: Option<f64>) {
        if let Some(record) = self.records.iter_mut().find(|r| r.seq == seq) {
            record.seen_ms = seen_ms;
            log_trigger(record);
            self.changed = true;
        }
    }
}

/// The one line per trigger, at DEBUG: never in the default log.
fn log_trigger(r: &TriggerRecord) {
    tracing::debug!(
        seq = r.seq,
        kind = ?r.kind,
        why = ?r.why,
        slide = r.slide,
        ok = r.ok,
        late_ms = ?r.late_ms,
        answer_ms = r.answer_ms,
        seen_ms = ?r.seen_ms,
        "presenter_follow_trigger"
    );
}

/// Keeps ProPresenter's slide with Playback's position. See the module notes.
pub struct Follower {
    timing: Timing,
    run: Option<Run>,
    handle: FollowHandle,
    rec: Recorder,
}

impl Follower {
    pub fn new(timing: Timing) -> Self {
        Self::with_handle(timing, FollowHandle::default())
    }

    /// A follower that publishes to (and listens to) this handle, so a caller that holds the handle keeps it when the follower is
    /// replaced.
    pub fn with_handle(timing: Timing, handle: FollowHandle) -> Self {
        Self {
            timing,
            run: None,
            handle,
            rec: Recorder::new(steady_clock()),
        }
    }

    /// Another clock for the durations of the answers (tests).
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.rec.clock = clock;
        self
    }

    pub fn handle(&self) -> FollowHandle {
        self.handle.clone()
    }

    /// The switch is off: the song being followed is forgotten and nothing is sent. The same as a step with `enabled: false`, for
    /// a caller that has no API to give.
    pub fn switch_off(&mut self) {
        self.run = None;
        self.rec.drop_pending();
        self.publish_recent();
        self.handle.try_again.store(false, Ordering::SeqCst);
        self.handle
            .set(FollowStatus::bare(FollowState::Off, "Switch is off"));
    }

    /// Forgets the song being followed (the next step starts it again, as a new song).
    pub fn reset(&mut self) {
        self.run = None;
        self.rec.drop_pending();
        self.publish_recent();
    }

    fn publish_recent(&mut self) {
        if std::mem::take(&mut self.rec.changed) {
            *self.handle.recent.lock().unwrap_or_else(|p| p.into_inner()) =
                self.rec.records.iter().cloned().collect();
        }
    }

    /// One look. Never fails: what went wrong is counted and shown in the status.
    pub async fn step(&mut self, api: &dyn ProPresenterApi, input: &Input<'_>) {
        let status = self.drive(api, input).await;
        self.publish_recent();
        self.handle.set(status);
    }

    async fn drive(&mut self, api: &dyn ProPresenterApi, input: &Input<'_>) -> FollowStatus {
        let now = input.now;
        let tick_clock = (self.rec.clock)();
        let timing = self.timing;
        let retry = self.handle.try_again.swap(false, Ordering::SeqCst);
        if !input.enabled {
            self.run = None;
            self.rec.drop_pending();
            return FollowStatus::bare(FollowState::Off, "Switch is off");
        }
        let Some(playback) = input.playback.filter(|p| p.position.is_finite()) else {
            return FollowStatus::bare(FollowState::Idle, "Playback is not on a song");
        };
        let Some(song) = input.song else {
            return FollowStatus::bare(
                FollowState::Idle,
                "Playback is on a song that is not in the plan",
            );
        };
        let Some(prepared) = song.prepared.as_ref() else {
            self.run = None;
            self.rec.drop_pending();
            return FollowStatus {
                song_key: Some(song.key.clone()),
                song_title: Some(song.title.clone()),
                ..FollowStatus::bare(FollowState::NotFollowed, &song.readiness_text)
            };
        };
        let target = (
            prepared.binding.playlist_id.clone(),
            prepared.binding.presentation_uuid.clone(),
        );
        match self.run.as_mut() {
            Some(run) if run.key == song.key => {
                if run.target != target {
                    // The song moved to another playlist item or presentation: it has to be shown again.
                    run.target = target;
                    run.started = false;
                    run.last_fired = None;
                }
            }
            _ => {
                self.rec.drop_pending();
                let mut run = Run::new(&song.key, target);
                run.seen_starts = playback.starts;
                self.run = Some(run);
            }
        }
        let Some(run) = self.run.as_mut() else {
            return FollowStatus::bare(FollowState::Idle, "Nothing to follow");
        };
        if retry {
            run.failures = 0;
            run.paused = false;
            run.last_error = None;
            run.retry_at = f64::NEG_INFINITY;
        }
        // The song started from the top again (a start Playback announced, not a resume): its item is shown again, at once.
        if playback.starts != run.seen_starts {
            run.seen_starts = playback.starts;
            if playback.playing {
                run.started = false;
                run.last_fired = None;
                run.last_item_at = None;
                run.retry_at = f64::NEG_INFINITY;
                run.hold = None;
                run.signal_at = None;
            }
        }
        if playback.playing && !run.started && run.signal_at.is_none() {
            run.signal_at = Some(now - playback.age_s.max(0.0));
        }

        // Where the schedule is: the last step at or before the position (the first slide before the first step).
        let at = playback.position + f64::from(input.offset_ms) / 1000.0;
        let steps = &prepared.schedule.steps;
        let reached = steps.partition_point(|s| s.time_s <= at);
        let (wanted, section, cue_s) = match reached.checked_sub(1).and_then(|i| steps.get(i)) {
            Some(step) => (step.flat_index, step.section_index, Some(step.time_s)),
            None => (0, 0, None),
        };

        self.rec.expire(now);
        if !run.paused {
            // A person's click: read where ProPresenter is, about four times a second (more often while a trigger waits to be
            // seen, so the time it took is not rounded to the poll).
            let every = if self.rec.waiting() {
                timing.fast_poll_s.min(timing.poll_s)
            } else {
                timing.poll_s
            };
            if run.started && run.hold.is_none() && now - run.last_poll_at >= every - 1e-9 {
                run.last_poll_at = now;
                match api.slide_position().await {
                    Ok(position) => {
                        self.rec.seen(now, position.as_ref());
                        if let (Some(expected), true) =
                            (run.last_fired, now - run.last_fire_at >= timing.grace_s)
                        {
                            let same = position.as_ref().is_some_and(|p| {
                                p.presentation_uuid == prepared.binding.presentation_uuid
                                    && p.index == expected
                            });
                            if !same {
                                run.hold = Some(Hold { section });
                                run.last_fired = None;
                            }
                        }
                    }
                    Err(_) => {
                        if run
                            .last_warn_at
                            .is_none_or(|w| now - w >= timing.warn_every_s)
                        {
                            run.last_warn_at = Some(now);
                            tracing::warn!("presenter_follow_slide_read_failed");
                        }
                    }
                }
            }
            if let Some(hold) = run.hold {
                if section != hold.section {
                    run.hold = None;
                    run.last_fired = None;
                }
            }
            if run.hold.is_none() && playback.playing && now >= run.retry_at {
                let want = Want {
                    slide: wanted,
                    cue_s,
                    at,
                };
                Self::bring_up_to_date(
                    api,
                    run,
                    &mut self.rec,
                    &song.title,
                    prepared,
                    &want,
                    (now, tick_clock),
                    &timing,
                )
                .await;
            }
        }
        self.status_of(song, prepared, playback, section)
    }

    /// Shows the song's item and records the trigger.
    async fn show_item(
        api: &dyn ProPresenterApi,
        run: &mut Run,
        rec: &mut Recorder,
        title: &str,
        prepared: &Prepared,
        (now, tick_clock): (f64, f64),
    ) -> Result<(), FireError> {
        let began = (rec.clock)();
        let result = fire_item(api, prepared, &rec.clock).await;
        let presentation = prepared.binding.presentation_uuid.as_str();
        match &result {
            Ok((sent, answer)) => {
                let late_s = run
                    .signal_at
                    .take()
                    .map(|s| (now - s) + (sent - tick_clock));
                rec.add(Sent {
                    song: title,
                    kind: TriggerKind::Item,
                    why: TriggerWhy::Play,
                    slide: 0,
                    ok: true,
                    late_s,
                    answer_s: *answer,
                    now: now + (sent - tick_clock),
                    presentation,
                });
            }
            Err(_) => {
                let spent = (rec.clock)() - began;
                rec.add(Sent {
                    song: title,
                    kind: TriggerKind::Item,
                    why: TriggerWhy::Play,
                    slide: 0,
                    ok: false,
                    late_s: None,
                    answer_s: spent,
                    now,
                    presentation,
                });
            }
        }
        result.map(|_| ())
    }

    /// Shows the song's item when it has not been shown, then the wanted slide when it is not the one on screen.
    #[allow(clippy::too_many_arguments)]
    async fn bring_up_to_date(
        api: &dyn ProPresenterApi,
        run: &mut Run,
        rec: &mut Recorder,
        title: &str,
        prepared: &Prepared,
        want: &Want,
        times: (f64, f64),
        timing: &Timing,
    ) {
        let (now, tick_clock) = times;
        let mut checked = false;
        if !run.started {
            if run.last_item_at.is_some_and(|t| now - t < timing.refire_s) {
                return;
            }
            run.last_item_at = Some(now);
            match Self::show_item(api, run, rec, title, prepared, times).await {
                Ok(()) => {
                    run.started = true;
                    run.last_fired = Some(0);
                    run.last_fire_at = now;
                    run.succeeded();
                    checked = true;
                }
                Err(error) => {
                    run.failed(now, timing, &error.words());
                    return;
                }
            }
        }
        if run.last_fired == Some(want.slide) {
            return;
        }
        // The song's presentation must be the one on screen before a slide is triggered (it was just checked when the
        // item was shown a moment ago).
        let active = if checked {
            Ok(Some(prepared.binding.presentation_uuid.clone()))
        } else {
            api.active_presentation().await
        };
        match active {
            Ok(Some(uuid)) if uuid == prepared.binding.presentation_uuid => {}
            Ok(_) => {
                // The wrong presentation is on screen: the item is shown again, but at most once per `refire_s`. Until then
                // nothing is asked, so this is not a loop of reads.
                if let Some(at) = run.last_item_at.filter(|t| now - t < timing.refire_s) {
                    run.retry_at = at + timing.refire_s;
                    return;
                }
                run.last_item_at = Some(now);
                match Self::show_item(api, run, rec, title, prepared, times).await {
                    Ok(()) => {
                        run.last_fired = Some(0);
                        run.last_fire_at = now;
                    }
                    Err(error) => {
                        run.failed(now, timing, &error.words());
                        return;
                    }
                }
                if run.last_fired == Some(want.slide) {
                    run.succeeded();
                    return;
                }
            }
            Err(error) => {
                run.failed(now, timing, &error.words());
                return;
            }
        }
        let before = (rec.clock)();
        let result = api.trigger_slide(want.slide).await;
        let answer_s = (rec.clock)() - before;
        let late_s = want.cue_s.map(|cue| want.at - cue + (before - tick_clock));
        let why = match late_s {
            Some(late) if late <= CATCH_UP_S => TriggerWhy::Cue,
            _ => TriggerWhy::CatchUp,
        };
        rec.add(Sent {
            song: title,
            kind: TriggerKind::Slide,
            why,
            slide: want.slide,
            ok: result.is_ok(),
            late_s,
            answer_s,
            now: now + (before - tick_clock),
            presentation: &prepared.binding.presentation_uuid,
        });
        match result {
            Ok(()) => {
                run.last_fired = Some(want.slide);
                run.last_fire_at = now;
                run.succeeded();
            }
            Err(error) => run.failed(now, timing, &error.words()),
        }
    }

    fn status_of(
        &self,
        song: &SongView,
        prepared: &Prepared,
        playback: PlaybackNow,
        section: usize,
    ) -> FollowStatus {
        let Some(run) = self.run.as_ref() else {
            return FollowStatus::bare(FollowState::Idle, "Nothing to follow");
        };
        let mut status = FollowStatus {
            state: FollowState::Following,
            message: song.readiness_text.clone(),
            song_key: Some(song.key.clone()),
            song_title: Some(song.title.clone()),
            section: prepared.section_names.get(section).map(|name| SectionNow {
                index: section,
                name: name.clone(),
            }),
            slide: run.last_fired,
            hold_reason: None,
        };
        if run.paused {
            status.state = FollowState::Paused;
            status.section = None;
            status.message = format!(
                "Paused: ProPresenter did not take {} slide changes in a row ({}) Following starts again with the next song, or press Try again.",
                run.failures,
                run.last_error.clone().unwrap_or_default()
            );
        } else if run.hold.is_some() {
            status.state = FollowState::Held;
            status.hold_reason = Some(HOLD_REASON.to_string());
            status.message = HOLD_REASON.to_string();
        } else if !playback.playing {
            // Stopped or paused: nothing is being followed at this moment, so the summary must not say it is.
            status.state = FollowState::Ready;
            if run.started {
                status.message = READY_WAITING.to_string();
            }
        }
        status
    }
}

/// The slide the position asks for and the cue's time it comes from.
struct Want {
    slide: usize,
    /// The schedule step's time; `None` before the first step.
    cue_s: Option<f64>,
    /// The position with the offset.
    at: f64,
}
