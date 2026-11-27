//! Following Playback with the song's lyrics in ProPresenter: the part that lives in StagePilot's server. The rules are
//! `stagepilot_propresenter::follow` (the schedule, the preparer that reads ProPresenter's playlists and song files, and the
//! follower that triggers slides); this module gives them a clock, Playback's position, the settings and two tasks.
//!
//! * `propresenter.presenter_follow` in the settings is the master switch (off) and the offset. With it off nothing is read from
//!   or sent to ProPresenter, Playback's library is not asked for this and the app behaves as before.
//! * Two tasks of their own, never the timer's or the lights': the FOLLOWER ticks every 100 ms and only reads what the other task
//!   last worked out; the READER (slow: the Playback library, ProPresenter's playlist and song files) works out which plan songs
//!   can be followed and how. A slow ProPresenter delays a slide at most; it can delay no timer.
//! * Playback's position is the heartbeat the Playback client already holds (`song`, `position`, `playing`), extrapolated while
//!   it plays. A plan song is found by Playback's own number for it (`setlist_song_id`).
//! * `GET /api/v1/propresenter-follow/status` and `POST /api/v1/propresenter-follow/try-again`. Neither is part of `/health`:
//!   nothing here changes the header's Ready or Check-system verdict.

use crate::playback_library::effective_settings;
use crate::runtime::Runtime;
use crate::validation::{ok, Json};
use axum::extract::State;
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Value};
use stagepilot_playback::library::{LibraryPlan, LibraryState};
use stagepilot_propresenter::follow::binding::BindingStore;
use stagepilot_propresenter::follow::engine::{
    FollowHandle, FollowState, Follower, Input, PlaybackNow, SongView, Timing,
};
use stagepilot_propresenter::follow::prepare::{Preparer, Readiness, SongInput, SongStatus};
use stagepilot_propresenter::follow::{LyricCue, Section};
use stagepilot_propresenter::link::api::{HttpApi, ProPresenterApi};
use stagepilot_propresenter::link::status::LinkState;
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

type Rt = Arc<Runtime>;

/// How often the follower looks at Playback's position.
pub const TICK: Duration = Duration::from_millis(100);
/// How often the reader looks whether anything needs working out again.
pub const READ_EVERY: Duration = Duration::from_millis(500);
/// While no song plays, the songs are worked out again at least this often (ProPresenter or Playback may have changed).
pub const READ_AGAIN: Duration = Duration::from_secs(15);

/// Playback's song, position and whether it plays: what the follower follows. Tests give their own.
pub type Probe = Arc<dyn Fn() -> Option<(i64, f64, bool)> + Send + Sync>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// What the reader last worked out.
#[derive(Default)]
struct Snapshot {
    songs: Vec<SongStatus>,
    /// The song views by Playback's own number for the song.
    views: HashMap<i64, SongView>,
}

#[derive(Default)]
struct Inner {
    snapshot: Arc<Snapshot>,
    /// What the last reading was made for.
    key: String,
    at: Option<Instant>,
}

/// The heartbeat last seen and when it was first seen (the position is moved on from there while it plays).
#[derive(Default)]
struct Heart {
    serial: u64,
    at: Option<Instant>,
}

pub struct FollowService {
    handle: FollowHandle,
    follower: tokio::sync::Mutex<Follower>,
    inner: Mutex<Inner>,
    preparer: Preparer,
    /// Moves whenever a binding was remembered, so the reader works out the songs again.
    binding_rev: AtomicU64,
    force: AtomicBool,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    api_cache: Mutex<Option<(String, Arc<dyn ProPresenterApi>)>>,
    probe: Mutex<Option<Probe>>,
    heart: Mutex<Heart>,
    began: Instant,
}

impl FollowService {
    pub fn new(store: Arc<dyn BindingStore>) -> Self {
        let follower = Follower::new(Timing::default());
        Self {
            handle: follower.handle(),
            follower: tokio::sync::Mutex::new(follower),
            inner: Mutex::new(Inner::default()),
            preparer: Preparer::new(store),
            binding_rev: AtomicU64::new(0),
            force: AtomicBool::new(true),
            tasks: Mutex::new(Vec::new()),
            api_cache: Mutex::new(None),
            probe: Mutex::new(None),
            heart: Mutex::new(Heart::default()),
            began: Instant::now(),
        }
    }

    /// Starts the two tasks (they stop when the runtime is gone, or at `stop`).
    pub fn spawn(self: &Arc<Self>, rt: Weak<Runtime>) {
        let mut tasks = lock(&self.tasks);
        if !tasks.is_empty() {
            return;
        }
        let (service, runtime) = (self.clone(), rt.clone());
        tasks.push(tokio::spawn(async move {
            loop {
                tokio::time::sleep(TICK).await;
                let Some(rt) = runtime.upgrade() else { break };
                service.tick(&rt).await;
            }
        }));
        let service = self.clone();
        tasks.push(tokio::spawn(async move {
            loop {
                tokio::time::sleep(READ_EVERY).await;
                let Some(rt) = rt.upgrade() else { break };
                service.read_if_due(&rt).await;
            }
        }));
    }

    pub fn stop(&self) {
        for task in lock(&self.tasks).drain(..) {
            task.abort();
        }
    }

    /// Playback's song, position and play state, from this instead of the real client (tests).
    #[doc(hidden)]
    pub fn set_probe_for_test(&self, probe: Option<Probe>) {
        *lock(&self.probe) = probe;
    }

    /// Other waits than the product's (tests).
    #[doc(hidden)]
    pub async fn set_timing_for_test(&self, timing: Timing) {
        *self.follower.lock().await = Follower::with_handle(timing, self.handle.clone());
    }

    /// Where each song was remembered to be in ProPresenter.
    pub fn bindings(
        &self,
    ) -> std::collections::BTreeMap<u64, stagepilot_propresenter::follow::binding::Binding> {
        self.preparer.store().all()
    }

    /// Asks the reader to work the songs out again at once.
    pub fn work_out_again(&self) {
        self.force.store(true, Ordering::SeqCst);
    }

    /// `Try again`: clears a pause made by failed triggers.
    pub fn try_again(&self) {
        self.handle.try_again();
    }

    /// Remembers where the songs a job added went (the Presenter Link calls this when it has written the playlist).
    pub async fn remember_after_add(
        &self,
        rt: &Runtime,
        playlist_id: &str,
        files: BTreeSet<String>,
    ) {
        if files.is_empty() {
            return;
        }
        let Some((_, host)) = rt.propresenter_link.connect().await else {
            return;
        };
        let api = self.api_for(rt);
        match self
            .preparer
            .remember_playlist(host.as_ref(), api.as_ref(), playlist_id, &files)
            .await
        {
            Ok(kept) if kept > 0 => {
                self.binding_rev.fetch_add(1, Ordering::SeqCst);
                self.work_out_again();
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(error = %error.words(), "presenter_follow_bindings_not_remembered")
            }
        }
    }

    fn api_for(&self, rt: &Runtime) -> Arc<dyn ProPresenterApi> {
        if let Some(api) = rt.propresenter_add_songs.api_stand_in() {
            return api;
        }
        let settings = rt
            .settings_service
            .effective_runtime_settings()
            .propresenter;
        let key = format!(
            "{}:{}:{}",
            settings.host, settings.port, settings.request_timeout_seconds
        );
        let mut cache = lock(&self.api_cache);
        match cache.as_ref() {
            Some((cached, api)) if *cached == key => api.clone(),
            _ => {
                let api: Arc<dyn ProPresenterApi> = Arc::new(HttpApi::new(&settings));
                *cache = Some((key, api.clone()));
                api
            }
        }
    }

    /// Where Playback is now, with the song Playback names.
    fn playback_now(&self, rt: &Runtime) -> Option<(i64, PlaybackNow)> {
        if let Some(probe) = lock(&self.probe).clone() {
            return probe()
                .map(|(song, position, playing)| (song, PlaybackNow { position, playing }));
        }
        let status = rt.playback.status();
        if !status.connected {
            return None;
        }
        let beat = status.heartbeat?;
        let mut heart = lock(&self.heart);
        if heart.serial != status.heartbeat_serial || heart.at.is_none() {
            heart.serial = status.heartbeat_serial;
            heart.at = Some(Instant::now());
        }
        // Playback beats about once a second; the position moves on from the beat while it plays.
        let moved = match (beat.playing, heart.at) {
            (true, Some(at)) => at.elapsed().as_secs_f64().min(5.0),
            _ => 0.0,
        };
        Some((
            beat.song_id,
            PlaybackNow {
                position: beat.position + moved,
                playing: beat.playing,
            },
        ))
    }

    /// One step of the follower.
    async fn tick(&self, rt: &Runtime) {
        let settings = rt
            .settings_service
            .effective_runtime_settings()
            .propresenter;
        let follow = settings.presenter_follow;
        let mut follower = self.follower.lock().await;
        if !follow.enabled {
            follower.switch_off();
            return;
        }
        let playing = self.playback_now(rt);
        let snapshot = lock(&self.inner).snapshot.clone();
        let song = playing.as_ref().and_then(|(id, _)| snapshot.views.get(id));
        let api = self.api_for(rt);
        let input = Input {
            now: self.began.elapsed().as_secs_f64(),
            enabled: true,
            offset_ms: follow.offset_ms,
            playback: playing.as_ref().map(|(_, now)| *now),
            song,
        };
        follower.step(api.as_ref(), &input).await;
    }

    /// What the reader's last pass was made for: the plan, the switch and where ProPresenter is, and the bindings.
    fn reading_key(&self, rt: &Runtime) -> String {
        let settings = rt
            .settings_service
            .effective_runtime_settings()
            .propresenter;
        let plan = rt.store.snapshot().plan;
        let songs: Vec<String> = plan
            .as_ref()
            .map(|p| {
                p.songs
                    .iter()
                    .map(|s| {
                        format!(
                            "{}={}",
                            s.source_song_id.as_deref().unwrap_or(&s.id),
                            s.title
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        format!(
            "{}|{}|{}:{}|{}|{}|{}",
            plan.as_ref().map(|p| p.id.as_str()).unwrap_or(""),
            songs.join(";"),
            settings.host,
            settings.port,
            settings.enabled,
            settings.link.data_folder.as_deref().unwrap_or(""),
            self.binding_rev.load(Ordering::SeqCst)
        )
    }

    async fn read_if_due(&self, rt: &Runtime) {
        let settings = rt
            .settings_service
            .effective_runtime_settings()
            .propresenter;
        if !settings.presenter_follow.enabled {
            // Off: nothing is read. What was read is forgotten so that turning it on works the songs out afresh.
            let mut inner = lock(&self.inner);
            if inner.at.is_some() {
                *inner = Inner::default();
                self.force.store(true, Ordering::SeqCst);
            }
            return;
        }
        let key = self.reading_key(rt);
        let (same, age) = {
            let inner = lock(&self.inner);
            (inner.key == key, inner.at.map(|at| at.elapsed()))
        };
        let forced = self.force.swap(false, Ordering::SeqCst);
        let song_playing = self.playback_now(rt).is_some_and(|(_, now)| now.playing);
        let due = forced || !same || age.is_none_or(|age| age >= READ_AGAIN && !song_playing);
        if !due {
            return;
        }
        let snapshot = self.read(rt).await;
        let mut inner = lock(&self.inner);
        inner.snapshot = Arc::new(snapshot);
        inner.key = key;
        inner.at = Some(Instant::now());
    }

    /// Works out every plan song: its Playback data, where it is in ProPresenter and whether it can be followed.
    async fn read(&self, rt: &Runtime) -> Snapshot {
        let settings = rt
            .settings_service
            .effective_runtime_settings()
            .propresenter;
        let plan = rt.store.snapshot().plan;
        let library = match rt.propresenter_add_songs.plan_stand_in() {
            Some(plan) => plan,
            None => {
                rt.playback_library
                    .plan(&effective_settings(rt), plan.as_ref(), false)
                    .await
            }
        };
        let inputs = song_inputs(&library, plan.as_ref());
        let statuses = if library.state != LibraryState::Ready || !library.enabled {
            Preparer::unavailable(&inputs, Readiness::LibraryNotReady)
        } else if !settings.enabled {
            Preparer::unavailable(&inputs, Readiness::LinkNotReady)
        } else {
            let link = rt.propresenter_link.status(false).await;
            match (link.state == LinkState::Ready).then(|| rt.propresenter_link.connect()) {
                Some(connecting) => match connecting.await {
                    Some((_, host)) => {
                        let api = self.api_for(rt);
                        self.preparer
                            .prepare(host.as_ref(), api.as_ref(), &inputs, true)
                            .await
                    }
                    None => Preparer::unavailable(&inputs, Readiness::LinkNotReady),
                },
                None => Preparer::unavailable(&inputs, Readiness::LinkNotReady),
            }
        };
        let views = statuses
            .iter()
            .filter_map(|s| {
                Some((
                    s.playback_song_id?,
                    SongView {
                        key: s.key.clone(),
                        title: s.title.clone(),
                        readiness_text: s.readiness.text(),
                        prepared: s.prepared.clone(),
                    },
                ))
            })
            .collect();
        Snapshot {
            songs: statuses,
            views,
        }
    }

    /// The status the window shows.
    pub fn status_json(&self, rt: &Runtime) -> Value {
        let settings = rt
            .settings_service
            .effective_runtime_settings()
            .propresenter;
        let follow = settings.presenter_follow;
        let status = self.handle.status();
        let songs: Vec<Value> = if follow.enabled {
            lock(&self.inner)
                .snapshot
                .songs
                .iter()
                .map(song_json)
                .collect()
        } else {
            // Off: nothing is read, so the songs are the plan's own, all saying so.
            rt.store
                .snapshot()
                .plan
                .map(|plan| {
                    plan.songs
                        .iter()
                        .map(|s| {
                            song_json(&SongStatus {
                                key: s.source_song_id.clone().unwrap_or_else(|| s.id.clone()),
                                title: s.title.clone(),
                                playback_song_id: None,
                                readiness: Readiness::SwitchOff,
                                prepared: None,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        json!({
            "enabled": follow.enabled,
            "offset_ms": follow.offset_ms,
            "state": serde_json::to_value(status.state).unwrap_or(Value::Null),
            "message": status.message,
            "song": status.song_key.as_ref().map(|key| json!({"key": key, "title": status.song_title})),
            "section": status.section,
            "slide": status.slide,
            "hold_reason": status.hold_reason,
            "can_try_again": status.state == FollowState::Paused,
            "songs": songs,
        })
    }
}

fn song_json(song: &SongStatus) -> Value {
    json!({
        "key": song.key,
        "title": song.title,
        "following": song.readiness.following(),
        "code": song.readiness.code(),
        "text": song.readiness.text(),
        "advice": song.readiness.advice(),
    })
}

/// The Playback library's plan as the songs to judge. A plan song Playback has no (certain) match for is not in Playback.
fn song_inputs(
    library: &LibraryPlan,
    plan: Option<&stagepilot_core::state::ServicePlan>,
) -> Vec<SongInput> {
    if library.songs.is_empty() {
        // The library has nothing (not asked, not ready): the plan's own songs, so the status still lists them.
        return plan
            .map(|plan| {
                plan.songs
                    .iter()
                    .map(|s| SongInput {
                        key: s.source_song_id.clone().unwrap_or_else(|| s.id.clone()),
                        title: s.title.clone(),
                        playback_song_id: None,
                        content_id: None,
                        sections: Vec::new(),
                        cues: Vec::new(),
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
    library
        .songs
        .iter()
        .map(|song| {
            let own = song.library.as_ref();
            SongInput {
                key: song.key.clone(),
                title: song.title.clone(),
                playback_song_id: own.and_then(|l| l.setlist_song_id),
                content_id: own.and_then(|l| l.content_id),
                sections: own
                    .map(|l| {
                        l.sections
                            .iter()
                            .map(|s| Section {
                                name: s.name.clone(),
                                start: s.start,
                                end: s.end,
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                cues: own
                    .map(|l| {
                        l.lyric_cues
                            .iter()
                            .map(|c| LyricCue {
                                time_s: c.time_s,
                                slide: c.slide,
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        })
        .collect()
}

// ---- the routes -----------------------------------------------------------------------------------------------------------

async fn status(State(rt): State<Rt>) -> Json {
    ok(rt.propresenter_follow.status_json(&rt))
}

async fn try_again(State(rt): State<Rt>) -> Json {
    rt.propresenter_follow.try_again();
    ok(rt.propresenter_follow.status_json(&rt))
}

pub fn routes() -> Router<Rt> {
    Router::new()
        .route("/api/v1/propresenter-follow/status", get(status))
        .route("/api/v1/propresenter-follow/try-again", post(try_again))
}
