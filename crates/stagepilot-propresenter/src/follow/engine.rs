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

use super::binding::playlist_items;
use super::prepare::Prepared;
use crate::link::api::{ApiError, ProPresenterApi};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Shown while a person's click holds the song.
pub const HOLD_REASON: &str = "Paused: someone changed the slide in ProPresenter";

/// The waits and limits of following. The defaults are the product's; tests may change them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    /// How often ProPresenter's current slide is read.
    pub poll_s: f64,
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
            grace_s: 0.6,
            refire_s: 2.0,
            retry_s: 1.0,
            warn_every_s: 60.0,
            max_failures: 5,
        }
    }
}

/// Where Playback is: seconds into the song (extrapolated by the caller while it plays) and whether it plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackNow {
    pub position: f64,
    pub playing: bool,
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

/// The part of the follower that other tasks and the routes reach: the last status, and the `Try again` request.
#[derive(Clone)]
pub struct FollowHandle {
    status: Arc<Mutex<FollowStatus>>,
    try_again: Arc<AtomicBool>,
}

impl Default for FollowHandle {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(FollowStatus::bare(
                FollowState::Off,
                "Switch is off",
            ))),
            try_again: Arc::new(AtomicBool::new(false)),
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

/// Shows the song's playlist item (its first slide) and checks that its presentation is the active one.
async fn fire_item(api: &dyn ProPresenterApi, prepared: &Prepared) -> Result<(), FireError> {
    let binding = &prepared.binding;
    let playlist = api.playlist(&binding.playlist_id).await?;
    let item = playlist_items(&playlist)
        .into_iter()
        .find(|i| i.presentation_uuid == binding.presentation_uuid)
        .ok_or(FireError::NotInPlaylist)?;
    api.trigger_playlist_item(&binding.playlist_id, item.index)
        .await?;
    match api.active_presentation().await? {
        Some(uuid) if uuid == binding.presentation_uuid => Ok(()),
        _ => Err(FireError::Wrong),
    }
}

/// Keeps ProPresenter's slide with Playback's position. See the module notes.
pub struct Follower {
    timing: Timing,
    run: Option<Run>,
    handle: FollowHandle,
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
        }
    }

    pub fn handle(&self) -> FollowHandle {
        self.handle.clone()
    }

    /// The switch is off: the song being followed is forgotten and nothing is sent. The same as a step with `enabled: false`, for
    /// a caller that has no API to give.
    pub fn switch_off(&mut self) {
        self.run = None;
        self.handle.try_again.store(false, Ordering::SeqCst);
        self.handle
            .set(FollowStatus::bare(FollowState::Off, "Switch is off"));
    }

    /// Forgets the song being followed (the next step starts it again, as a new song).
    pub fn reset(&mut self) {
        self.run = None;
    }

    /// One look. Never fails: what went wrong is counted and shown in the status.
    pub async fn step(&mut self, api: &dyn ProPresenterApi, input: &Input<'_>) {
        let status = self.drive(api, input).await;
        self.handle.set(status);
    }

    async fn drive(&mut self, api: &dyn ProPresenterApi, input: &Input<'_>) -> FollowStatus {
        let now = input.now;
        let timing = self.timing;
        let retry = self.handle.try_again.swap(false, Ordering::SeqCst);
        if !input.enabled {
            self.run = None;
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
            _ => self.run = Some(Run::new(&song.key, target)),
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

        // Where the schedule is: the last step at or before the position (the first slide before the first step).
        let at = playback.position + f64::from(input.offset_ms) / 1000.0;
        let steps = &prepared.schedule.steps;
        let reached = steps.partition_point(|s| s.time_s <= at);
        let (wanted, section) = match reached.checked_sub(1).and_then(|i| steps.get(i)) {
            Some(step) => (step.flat_index, step.section_index),
            None => (0, 0),
        };

        if !run.paused {
            // A person's click: read where ProPresenter is, about four times a second.
            if run.started && run.hold.is_none() && now - run.last_poll_at >= timing.poll_s {
                run.last_poll_at = now;
                match api.slide_position().await {
                    Ok(position) => {
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
                Self::bring_up_to_date(api, run, prepared, wanted, now, &timing).await;
            }
        }
        self.status_of(song, prepared, playback, section)
    }

    /// Shows the song's item when it has not been shown, then the wanted slide when it is not the one on screen.
    async fn bring_up_to_date(
        api: &dyn ProPresenterApi,
        run: &mut Run,
        prepared: &Prepared,
        wanted: usize,
        now: f64,
        timing: &Timing,
    ) {
        let mut checked = false;
        if !run.started {
            if run.last_item_at.is_some_and(|t| now - t < timing.refire_s) {
                return;
            }
            run.last_item_at = Some(now);
            match fire_item(api, prepared).await {
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
        if run.last_fired == Some(wanted) {
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
                match fire_item(api, prepared).await {
                    Ok(()) => {
                        run.last_fired = Some(0);
                        run.last_fire_at = now;
                    }
                    Err(error) => {
                        run.failed(now, timing, &error.words());
                        return;
                    }
                }
                if run.last_fired == Some(wanted) {
                    run.succeeded();
                    return;
                }
            }
            Err(error) => {
                run.failed(now, timing, &error.words());
                return;
            }
        }
        match api.trigger_slide(wanted).await {
            Ok(()) => {
                run.last_fired = Some(wanted);
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
        } else if !run.started && !playback.playing {
            status.state = FollowState::Ready;
        }
        status
    }
}
