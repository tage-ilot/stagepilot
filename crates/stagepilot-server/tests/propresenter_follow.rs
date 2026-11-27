//! Following Playback with the lyrics in ProPresenter, through the server: the switch and the offset in the settings, the status
//! route, `Try again`, the two tasks (the follower and the reader) against the fake ProPresenter, and the rules around them: off
//! means no call at all, a slow ProPresenter delays nothing else, and none of it touches the header's verdict.
//!
//! Playback's position is scripted (`set_probe_for_test`) so each step is one assignment; the real client's heartbeat is used in
//! `following_uses_the_heartbeat_the_playback_client_holds`.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::TimerOutput;
use stagepilot_core::settings::{PersistentSettings, SettingsService};
use stagepilot_playback::library::{
    LibraryPlan, LibraryPlanSong, LibrarySection, LibrarySong, LibraryState, LyricCue, PlanMatch,
};
use stagepilot_propresenter::link::api::fake::{entry_uuid, FakeApi, Loading};
use stagepilot_propresenter::link::api::ApiError;
use stagepilot_propresenter::link::fake::FakeHost;
use stagepilot_propresenter::pro_file::synthetic::SyntheticSong;
use stagepilot_server::{build_app, AppOptions, OutputBackends, Runtime};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;

const STATUS: &str = "/api/v1/propresenter-follow/status";
const TRY_AGAIN: &str = "/api/v1/propresenter-follow/try-again";

/// Count Off (one blank slide), a Verse of four slides and a Chorus of three, played Verse, Chorus, Verse, Chorus.
fn song_file() -> Vec<u8> {
    SyntheticSong::new("Test Song")
        .id(777)
        .seed(7)
        .group_with_blanks("", &[None])
        .group("Verse", &["a", "b", "c", "d"])
        .group("Chorus", &["e", "f", "g"])
        .arrangement("Playback", &[0, 1, 2, 1, 2])
        .selected(0)
        .build()
}

fn sections() -> Vec<LibrarySection> {
    [
        ("Count Off", 0.0, 4.0),
        ("Verse", 4.0, 20.0),
        ("Chorus", 20.0, 32.0),
        ("Verse", 32.0, 48.0),
        ("Chorus", 48.0, 60.0),
    ]
    .iter()
    .map(|(name, start, end)| LibrarySection {
        name: (*name).to_string(),
        start: *start,
        end: *end,
    })
    .collect()
}

/// The slide numbers are raw cue indexes in the file: Verse 1..4, Chorus 5..7, each cue a little ahead of its lyric.
const CUES: [(f64, u32); 14] = [
    (3.5, 1),
    (8.0, 2),
    (12.0, 3),
    (16.0, 4),
    (19.5, 5),
    (24.0, 6),
    (28.0, 7),
    (31.5, 1),
    (36.0, 2),
    (40.0, 3),
    (44.0, 4),
    (47.5, 5),
    (52.0, 6),
    (56.0, 7),
];

fn library_song(with_cues: bool) -> LibrarySong {
    let mut song = LibrarySong {
        setlist_song_id: Some(11),
        content_id: Some(777),
        position: 1,
        title: "Test Song".to_string(),
        key: Some("Eb".to_string()),
        key_original: Some("C".to_string()),
        key_was: None,
        key_changed_at: None,
        has_waveform: false,
        bpm: Some(70.0),
        scan_length_s: None,
        countdown_length_s: None,
        length_source: None,
        arrangement: Some("Default".to_string()),
        section_count: Some(5),
        sections: sections(),
        artwork: None,
        rental_expires_utc: None,
        lyric_cues: Vec::new(),
        lyric_cue_count: 0,
        lyric_cue_max_slide: None,
    };
    if with_cues {
        song.set_lyric_cues(
            CUES.iter()
                .map(|(time_s, slide)| LyricCue {
                    time_s: *time_s,
                    slide: *slide,
                })
                .collect(),
        );
    }
    song
}

fn library_plan(with_cues: bool) -> LibraryPlan {
    LibraryPlan {
        enabled: true,
        state: LibraryState::Ready,
        message: None,
        plan_date: Some("2026-11-29".into()),
        setlist: None,
        newest: None,
        songs: vec![LibraryPlanSong {
            key: "song-1".to_string(),
            title: "Test Song".to_string(),
            plan_position: 1,
            plan_seconds: None,
            plan_key: None,
            status: PlanMatch::Matched,
            confidence: None,
            library: Some(library_song(with_cues)),
            candidate: None,
            pair_choices: vec![],
        }],
        playback_only: vec![],
        chosen: false,
        setlists: vec![],
        stale: false,
        checked_at: None,
    }
}

struct World {
    rt: Arc<Runtime>,
    app: Router,
    api: Arc<FakeApi>,
    playlist: String,
    /// Playback's song, position and whether it plays.
    playback: Arc<Mutex<(i64, f64, bool)>>,
}

async fn world(with_cues: bool) -> World {
    world_with(with_cues, true).await
}

/// `focus`: ProPresenter's active playlist is the Sunday playlist (a person triggered an item of it last).
async fn world_with(with_cues: bool, focus: bool) -> World {
    world_from(with_cues, focus, PersistentSettings::default()).await
}

/// [`world_with`] on settings that already hold something (a big file of them, say).
async fn world_from(with_cues: bool, focus: bool, base: PersistentSettings) -> World {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let settings = SettingsService::ephemeral_persistent(base);
    let rt = Runtime::with_backends(Arc::new(settings), false, OutputBackends::fake_midi());
    let mut next = rt.settings_service.snapshot();
    next.integration_modes.timer_output = TimerOutput::Propresenter;
    next.propresenter.host = "127.0.0.1".to_string();
    rt.settings_service.save(next).unwrap();
    rt.propresenter_link
        .set_own_address_check(Arc::new(|_| true));
    let host = Arc::new(FakeHost::new().with_library("Playback").with_file(
        "Playback",
        "Test Song.pro",
        &song_file(),
    ));
    rt.propresenter_link.set_local_host_for_test(host.clone());
    let api = Arc::new(FakeApi::new(host, Loading::Immediately));
    let playlist = api.add_playlist(
        "Sunday",
        None,
        vec![
            FakeApi::header_item("Worship", 0.2),
            FakeApi::presentation_item(
                &entry_uuid("Playback", "Test Song.pro"),
                "Test Song",
                "Playback",
                0.0,
            ),
        ],
    );
    // ProPresenter's active playlist is this one (as when the person triggered an item of it last).
    if focus {
        api.focus_playlist(&playlist, 1);
    }
    rt.propresenter_add_songs
        .set_api_for_test(Some(api.clone()));
    rt.propresenter_add_songs
        .set_plan_for_test(Some(library_plan(with_cues)));
    let playback = Arc::new(Mutex::new((11_i64, 0.0_f64, false)));
    let probe = playback.clone();
    rt.propresenter_follow
        .set_probe_for_test(Some(Arc::new(move || {
            let state = probe.lock().unwrap();
            Some(*state)
        })));
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    World {
        rt,
        app,
        api,
        playlist,
        playback,
    }
}

impl World {
    fn start(&self) {
        self.rt.propresenter_follow.spawn(Arc::downgrade(&self.rt));
    }

    /// The switch on in the settings, without going through the route (big settings are slow to send).
    fn switch_direct(&self, enabled: bool, offset_ms: i32) {
        let mut next = self.rt.settings_service.snapshot();
        next.propresenter.presenter_follow.enabled = enabled;
        next.propresenter.presenter_follow.offset_ms = offset_ms;
        self.rt.settings_service.save(next).unwrap();
    }

    fn play(&self, position: f64) {
        *self.playback.lock().unwrap() = (11, position, true);
    }

    fn pause(&self) {
        self.playback.lock().unwrap().2 = false;
    }

    async fn switch(&self, enabled: bool, offset_ms: i32) {
        let mut value = self.rt.settings_service.snapshot().to_file_value();
        value["propresenter"]["presenter_follow"] =
            json!({"enabled": enabled, "offset_ms": offset_ms});
        let (status, body) = call(&self.app, "PUT", "/api/v1/settings", Some(value)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        // The defaults are left out of the saved file and of the answer.
        let saved = body["settings"]["propresenter"].get("presenter_follow");
        if enabled || offset_ms != 0 {
            assert_eq!(
                saved,
                Some(&json!({"enabled": enabled, "offset_ms": offset_ms}))
            );
        } else {
            assert_eq!(saved, None);
        }
    }

    async fn status(&self) -> Value {
        call(&self.app, "GET", STATUS, None).await.1
    }

    async fn until_status(&self, what: &str, state: &str) -> Value {
        for _ in 0..300 {
            let status = self.status().await;
            if status["state"] == state {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!(
            "timed out waiting for: {what}; status {}",
            self.status().await
        );
    }
}

async fn until(what: &str, mut ready: impl FnMut() -> bool) {
    for _ in 0..300 {
        if ready() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for: {what}");
}

async fn call(app: &Router, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(path);
    let mut request = match body {
        Some(b) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string())),
        None => builder.body(Body::empty()),
    }
    .unwrap();
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:50000".parse::<SocketAddr>().unwrap(),
    ));
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn log(w: &World) -> Vec<String> {
    use stagepilot_propresenter::link::api::fake::Trigger;
    w.api
        .triggers()
        .iter()
        .map(|t| match t {
            Trigger::Item { index, .. } => format!("item:{index}"),
            Trigger::Slide(n) => format!("slide:{n}"),
        })
        .collect()
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(ToString::to_string).collect()
}

// ---- the settings ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn the_switch_is_off_and_the_offset_zero_until_it_is_saved_with_the_other_settings() {
    let w = world(true).await;
    let (_, settings) = call(&w.app, "GET", "/api/v1/settings", None).await;
    // Off by default and absent from the saved file, so an older file and an older build never meet it.
    assert_eq!(
        settings["settings"]["propresenter"].get("presenter_follow"),
        None,
        "{settings}"
    );
    assert_eq!(w.status().await["enabled"], json!(false));
    w.switch(true, 250).await;
    let (_, settings) = call(&w.app, "GET", "/api/v1/settings", None).await;
    assert_eq!(
        settings["settings"]["propresenter"]["presenter_follow"],
        json!({"enabled": true, "offset_ms": 250})
    );
    let status = w.status().await;
    assert_eq!(
        (status["enabled"].clone(), status["offset_ms"].clone()),
        (json!(true), json!(250))
    );
}

#[tokio::test]
async fn a_save_that_leaves_it_out_and_the_propresenter_form_keep_what_was_set() {
    let w = world(true).await;
    w.switch(true, -100).await;
    // A window that does not know the setting saves everything else.
    let mut value = w.rt.settings_service.snapshot().to_file_value();
    value["propresenter"]
        .as_object_mut()
        .unwrap()
        .remove("presenter_follow");
    value["timezone"] = json!("America/Chicago");
    let (status, body) = call(&w.app, "PUT", "/api/v1/settings", Some(value)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["settings"]["timezone"], json!("America/Chicago"));
    assert_eq!(
        body["settings"]["propresenter"]["presenter_follow"],
        json!({"enabled": true, "offset_ms": -100}),
        "{body}"
    );
    // The ProPresenter form (host, port, timer) is not where the switch lives and leaves it alone too.
    let pp = w.rt.settings_service.snapshot().propresenter;
    let (status, body) = call(
        &w.app,
        "POST",
        "/api/v1/propresenter/settings",
        Some(
            json!({"host": pp.host, "port": pp.port, "timer_name": pp.timer_name,
                    "look_id": null, "request_timeout_seconds": pp.request_timeout_seconds}),
        ),
    )
    .await;
    assert!(
        status.is_success() || status == StatusCode::CONFLICT,
        "{status} {body}"
    );
    assert_eq!(
        w.rt.settings_service
            .snapshot()
            .propresenter
            .presenter_follow,
        stagepilot_core::config::PresenterFollowSettings {
            enabled: true,
            offset_ms: -100
        }
    );
}

#[tokio::test]
async fn an_offset_outside_two_seconds_is_refused_and_changes_nothing() {
    let w = world(true).await;
    for bad in [2001, -2001] {
        let mut value = w.rt.settings_service.snapshot().to_file_value();
        value["propresenter"]["presenter_follow"] = json!({"enabled": true, "offset_ms": bad});
        let (status, body) = call(&w.app, "PUT", "/api/v1/settings", Some(value)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(
            body["detail"][0]["loc"],
            json!(["body", "propresenter", "presenter_follow", "offset_ms"])
        );
    }
    assert!(
        !w.rt
            .settings_service
            .snapshot()
            .propresenter
            .presenter_follow
            .enabled
    );
}

// ---- the switch off -------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn with_the_switch_off_nothing_is_asked_of_propresenter_while_a_song_plays() {
    let w = world(true).await;
    w.api.clear_calls();
    w.start();
    w.play(12.0);
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert!(w.api.calls().is_empty(), "{:?}", w.api.calls());
    let status = w.status().await;
    assert_eq!(status["state"], "off");
    assert_eq!(status["message"], "Switch is off");
    assert_eq!(status["slide"], Value::Null);
    // Switched on, it follows; switched off again, it stops asking.
    w.switch(true, 0).await;
    until("a slide", || !w.api.slides_triggered().is_empty()).await;
    w.switch(false, 0).await;
    until("it is off", || {
        let rt = w.rt.clone();
        !rt.settings_service
            .effective_runtime_settings()
            .propresenter
            .presenter_follow
            .enabled
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    w.api.clear_calls();
    w.play(30.0);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(w.api.calls().is_empty(), "{:?}", w.api.calls());
    assert_eq!(w.status().await["state"], "off");
}

#[tokio::test]
async fn the_status_with_the_switch_off_lists_the_plan_songs_as_off_without_reading_anything() {
    let w = world(true).await;
    let plan: stagepilot_core::state::ServicePlan = serde_json::from_value(json!({
        "id": "p1", "title": "Sunday", "date": "2026-11-29", "service_type": "Sunday",
        "songs": [{"id": "s1", "title": "Test Song", "duration_seconds": 240, "order": 1}]
    }))
    .unwrap();
    w.rt.store.mutate(|s| s.plan = Some(plan));
    w.api.clear_calls();
    let status = w.status().await;
    assert_eq!(status["songs"][0]["title"], "Test Song");
    assert_eq!(status["songs"][0]["text"], "Switch is off");
    assert_eq!(status["songs"][0]["code"], "switch_off");
    assert!(w.api.calls().is_empty());
}

#[tokio::test]
async fn the_headers_verdict_is_the_same_with_following_off_on_and_paused() {
    let w = world(true).await;
    let health = |app: Router| async move {
        let (_, body) = call(&app, "GET", "/api/v1/health", None).await;
        let plugins: Vec<(Value, Value)> = body["plugins"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|p| (p["name"].clone(), p["status"].clone()))
                    .collect()
            })
            .unwrap_or_default();
        (
            body["status"].clone(),
            body["application_status"].clone(),
            plugins,
        )
    };
    // A first save makes the ProPresenter output part of the app; the verdict is taken with the switch off after it.
    w.switch(false, 0).await;
    let off = health(w.app.clone()).await;
    w.start();
    w.switch(true, 0).await;
    w.play(12.0);
    until("a slide", || !w.api.slides_triggered().is_empty()).await;
    let on = health(w.app.clone()).await;
    // Even with every trigger failing (the follower pauses), the verdict does not move.
    w.api.fail_triggers(Some(ApiError::Status(500)));
    w.play(26.0);
    w.until_status("paused", "paused").await;
    let paused = health(w.app.clone()).await;
    assert_eq!(off, on);
    assert_eq!(off, paused);
}

// ---- following ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_song_is_followed_by_its_playback_cues_through_play_loop_click_and_the_offset() {
    let w = world(true).await;
    w.api.clear_calls();
    w.start();
    w.switch(true, 0).await;
    // The reader finds the song in the active playlist and says how it is followed.
    let status = loop_status(&w, |s| s["songs"][0]["code"] == "cues").await;
    assert_eq!(status["songs"][0]["text"], "Following by Playback cues");
    assert_eq!(status["songs"][0]["following"], "cues");
    // The status carries Playback's own lines per slide next to the file's (the cues here say one line per slide).
    assert_eq!(status["songs"][0]["lines_per_slide"], 1);
    assert_eq!(status["songs"][0]["playback_lines"], 1);
    // Play from 5 s: the playlist item (item 1: a header is item 0), then the cue at 3.5 s (flat slide 1).
    w.play(5.0);
    until("slide 1", || log(&w) == strings(&["item:1", "slide:1"])).await;
    let status = w.until_status("following", "following").await;
    assert_eq!(status["song"]["title"], "Test Song");
    assert_eq!(status["section"], json!({"index": 1, "name": "Verse"}));
    assert_eq!(status["slide"], json!(1));
    // 25 s: the Chorus' second cue (6 at 24 s) is flat 6.
    w.play(25.0);
    until("slide 6", || w.api.raw_on_screen() == Some(6)).await;
    // Playback loops back into the second Verse (cue 1 at 31.5 s is flat 8, the Verse played a second time).
    w.play(33.0);
    until("slide 8", || w.api.slides_triggered().last() == Some(&8)).await;
    assert_eq!(w.api.raw_on_screen(), Some(1));
    // A person clicks a slide in ProPresenter: following holds, with the reason, until the next section.
    w.api.click_slide(11);
    let held = w.until_status("held", "held").await;
    assert_eq!(
        held["hold_reason"],
        "Paused: someone changed the slide in ProPresenter"
    );
    w.play(37.0);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(w.api.slides_triggered().last(), Some(&8), "{:?}", log(&w));
    w.play(49.0);
    until("the next section", || w.api.raw_on_screen() == Some(5)).await;
    assert_eq!(w.api.slides_triggered().last(), Some(&12));
    w.until_status("following again", "following").await;
    // The offset: +2 s makes the cue at 52 s due at 50 s.
    w.switch(true, 2000).await;
    w.play(50.2);
    until("slide 13", || w.api.slides_triggered().last() == Some(&13)).await;
    assert_eq!(w.api.raw_on_screen(), Some(6));
}

async fn loop_status(w: &World, ready: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..300 {
        let status = w.status().await;
        if ready(&status) {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out; status {}", w.status().await);
}

#[tokio::test]
async fn a_song_with_no_cues_is_followed_by_bars_and_the_status_gives_the_advice() {
    let w = world(false).await;
    w.start();
    w.switch(true, 0).await;
    let status = loop_status(&w, |s| s["songs"][0]["code"] == "bars").await;
    assert_eq!(
        status["songs"][0]["text"],
        "Following by bars (no Playback cues)"
    );
    assert_eq!(
        status["songs"][0]["advice"],
        "No cues: in Playback choose a ProPresenter 1 Line Template for this song"
    );
    // The file's slides have one line of text each: the template that fits it.
    assert_eq!(status["songs"][0]["lines_per_slide"], 1);
    assert!(
        status["songs"][0]["playback_lines"].is_null(),
        "no cues, nothing to read it from"
    );
    assert_eq!(
        status["songs"][0]["template"],
        "ProPresenter 1 Line Template"
    );
    // The Verse (4 s to 20 s) has four slides, one every four seconds: at 9 s the second is due (flat 2).
    w.play(9.0);
    until("flat 2", || w.api.slides_triggered().last() == Some(&2)).await;
    assert_eq!(log(&w), strings(&["item:1", "slide:2"]));
    assert_eq!(w.api.raw_on_screen(), Some(2));
}

#[tokio::test]
async fn pausing_triggers_nothing_and_a_song_not_in_the_playlist_is_not_followed_and_says_why() {
    let w = world(true).await;
    w.start();
    w.switch(true, 0).await;
    w.play(5.0);
    until("slide 1", || w.api.slides_triggered().last() == Some(&1)).await;
    w.pause();
    let count = log(&w).len();
    w.playback.lock().unwrap().1 = 25.0;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(log(&w).len(), count, "paused: the slide stays");
    // The song leaves the playlist (ProPresenter shows a placeholder where it was).
    let mut items = w.api.stored(&w.playlist);
    items.pop();
    stagepilot_propresenter::link::api::ProPresenterApi::put_playlist(&*w.api, &w.playlist, &items)
        .await
        .unwrap();
    w.rt.propresenter_follow.work_out_again();
    let status = loop_status(&w, |s| s["songs"][0]["code"] == "not_in_playlist").await;
    assert_eq!(
        status["songs"][0]["text"],
        "Not in the ProPresenter playlist"
    );
}

#[tokio::test]
async fn five_failed_triggers_pause_following_and_try_again_starts_it() {
    let w = world(true).await;
    w.rt.propresenter_follow
        .set_timing_for_test(stagepilot_propresenter::follow::engine::Timing {
            retry_s: 0.05,
            ..Default::default()
        })
        .await;
    w.start();
    w.switch(true, 0).await;
    loop_status(&w, |s| s["songs"][0]["code"] == "cues").await;
    w.play(5.0);
    until("slide 1", || w.api.slides_triggered().last() == Some(&1)).await;
    w.api.fail_triggers(Some(ApiError::Status(500)));
    w.play(25.0);
    let paused = w.until_status("paused", "paused").await;
    assert_eq!(paused["can_try_again"], json!(true));
    assert!(
        paused["message"].as_str().unwrap().starts_with("Paused:"),
        "{paused}"
    );
    // Five tries and no more, not even a read.
    let calls = w.api.calls().len();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(w.api.calls().len(), calls);
    // The route clears the pause; with ProPresenter answering again the slide is shown.
    w.api.fail_triggers(None);
    let (status, body) = call(&w.app, "POST", TRY_AGAIN, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    until("slide 6", || w.api.raw_on_screen() == Some(6)).await;
    w.until_status("following", "following").await;
}

#[tokio::test]
async fn a_slow_propresenter_delays_neither_the_status_nor_anything_else() {
    let w = world(true).await;
    w.start();
    w.switch(true, 0).await;
    loop_status(&w, |s| s["songs"][0]["code"] == "cues").await;
    w.api.set_delay(Duration::from_millis(1500));
    w.api.clear_calls();
    w.play(5.0);
    until("the follower is waiting on ProPresenter", || {
        !w.api.calls().is_empty()
    })
    .await;
    // While it waits: the status route, the state store and the event bus (what the timer and the lights run on) answer at once.
    let started = std::time::Instant::now();
    let status = w.status().await;
    assert_eq!(status["enabled"], json!(true));
    w.rt.store
        .mutate(|s| s.application_status = stagepilot_core::state::ApplicationStatus::Running);
    tokio::time::timeout(
        Duration::from_millis(300),
        w.rt.bus.publish(stagepilot_core::events::empty_event(
            stagepilot_core::events::EventType::ApplicationStarted,
            "test",
        )),
    )
    .await
    .expect("the bus answers while ProPresenter is slow");
    assert!(
        started.elapsed() < Duration::from_millis(700),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn songs_the_presenter_link_just_added_are_remembered_and_the_binding_is_used() {
    // Nothing is active in ProPresenter now, so only a remembered binding can find the song.
    let w = world_with(true, false).await;
    w.start();
    w.switch(true, 0).await;
    let status = loop_status(&w, |s| s["songs"][0]["code"] == "not_in_playlist").await;
    assert_eq!(status["songs"][0]["following"], Value::Null);
    let files: std::collections::BTreeSet<String> =
        ["Test Song.pro".to_string()].into_iter().collect();
    w.rt.propresenter_follow
        .remember_after_add(&w.rt, &w.playlist, files)
        .await;
    let bindings = w.rt.propresenter_follow.bindings();
    assert_eq!(bindings[&777].playlist_id, w.playlist);
    assert_eq!(bindings[&777].file, "Test Song.pro");
    assert_eq!(bindings[&777].item_index, 1);
    let status = loop_status(&w, |s| s["songs"][0]["code"] == "cues").await;
    assert_eq!(status["songs"][0]["following"], "cues");
}

#[tokio::test]
async fn an_arrangement_changed_after_it_was_added_is_not_followed() {
    let w = world(true).await;
    let files: std::collections::BTreeSet<String> =
        ["Test Song.pro".to_string()].into_iter().collect();
    w.rt.propresenter_follow
        .remember_after_add(&w.rt, &w.playlist, files)
        .await;
    assert_eq!(w.rt.propresenter_follow.bindings().len(), 1);
    // Someone changes the song in ProPresenter: the verse is played once more.
    let edited = SyntheticSong::new("Test Song")
        .id(777)
        .seed(7)
        .group_with_blanks("", &[None])
        .group("Verse", &["a", "b", "c", "d"])
        .group("Chorus", &["e", "f", "g"])
        .arrangement("Playback", &[0, 1, 2, 1, 2, 1])
        .selected(0)
        .build();
    let host = Arc::new(FakeHost::new().with_library("Playback").with_file(
        "Playback",
        "Test Song.pro",
        &edited,
    ));
    w.rt.propresenter_link.set_local_host_for_test(host);
    w.start();
    w.switch(true, 0).await;
    let status = loop_status(&w, |s| s["songs"][0]["code"] == "arrangement_changed").await;
    assert_eq!(
        status["songs"][0]["text"],
        "Arrangement was changed in ProPresenter"
    );
    w.play(5.0);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(w.api.triggers().is_empty());
    assert_eq!(w.status().await["state"], "not_followed");
}

// ---- the real client ------------------------------------------------------------------------------------------------------

/// The real Playback client, talking to a fake Playback that beats only when told to, and a Runtime that follows it.
async fn live() -> (
    Arc<Runtime>,
    stagepilot_playback::testkit::FakePlayback,
    Arc<FakeApi>,
) {
    use stagepilot_playback::plugin::PluginOptions;
    use stagepilot_playback::testkit::FakePlayback;
    let w = world(true).await;
    // The real Playback client, talking to a fake Playback that beats only when told to; the scripted probe is taken away.
    let fake = FakePlayback::start().await;
    {
        let mut s = fake.state.lock().unwrap();
        s.songs = vec![11];
        s.song = 11;
        s.length = 120.0;
    }
    let service = SettingsService::ephemeral_persistent(PersistentSettings::default());
    let options = PluginOptions {
        step_timeout: Duration::from_secs(2),
        ..PluginOptions::default()
    };
    let rt = Runtime::with_playback_options(Arc::new(service), false, options);
    let mut next = rt.settings_service.snapshot();
    next.integration_modes.timer_output = TimerOutput::Propresenter;
    next.propresenter.host = "127.0.0.1".to_string();
    rt.settings_service.save(next).unwrap();
    rt.propresenter_link
        .set_own_address_check(Arc::new(|_| true));
    rt.propresenter_link.set_local_host_for_test(Arc::new(
        FakeHost::new().with_library("Playback").with_file(
            "Playback",
            "Test Song.pro",
            &song_file(),
        ),
    ));
    let api = w.api.clone();
    rt.propresenter_add_songs
        .set_api_for_test(Some(api.clone()));
    rt.propresenter_add_songs
        .set_plan_for_test(Some(library_plan(true)));
    rt.start().await;
    let mut persistent = rt.settings_service.snapshot();
    persistent.playback_api.host = Some("127.0.0.1".into());
    persistent.playback_api.port = fake.port;
    persistent.playback_api.auto_scan = false;
    persistent.integration_modes.midi_source = stagepilot_core::config::MidiSource::PlaybackApi;
    persistent.propresenter.presenter_follow.enabled = true;
    rt.playback.save_settings(persistent).await.unwrap();
    for _ in 0..300 {
        fake.beat();
        if rt.playback.connection().connected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    until("connected", || rt.playback.connection().connected).await;
    (rt, fake, api)
}

#[tokio::test]
async fn following_uses_the_heartbeat_the_playback_client_holds() {
    let (rt, fake, api) = live().await;
    // Playback plays song 11 at 26 s: the cue at 24 s (flat slide 6) is the one to show.
    {
        let mut s = fake.state.lock().unwrap();
        s.playing = true;
        s.position = 26.0;
    }
    fake.beat();
    until("the slide follows the heartbeat", || {
        api.raw_on_screen() == Some(6)
    })
    .await;
    // A seek back to 9 s on Playback moves the slide back (the cue at 8 s is flat 2).
    {
        let mut s = fake.state.lock().unwrap();
        s.position = 9.0;
    }
    fake.beat();
    until("the seek is followed", || api.raw_on_screen() == Some(2)).await;
    rt.stop().await;
}

// ---- the first slide at the moment Play starts -------------------------------------------------------------------------------

#[tokio::test]
async fn the_first_slide_comes_with_playbacks_play_command_not_with_the_next_heartbeat() {
    let (rt, fake, api) = live().await;
    // The follower is driven by hand (the tasks are stopped), so every step is one call; only the websocket takes real time.
    rt.propresenter_follow.stop();
    let service = &rt.propresenter_follow;
    service.read_if_due_for_test(&rt).await;
    assert_eq!(service.status_json(&rt)["songs"][0]["code"], "cues");
    let items = || {
        use stagepilot_propresenter::link::api::fake::Trigger;
        api.triggers()
            .iter()
            .map(|t| match t {
                Trigger::Item { index, .. } => format!("item:{index}"),
                Trigger::Slide(n) => format!("slide:{n}"),
            })
            .collect::<Vec<_>>()
    };

    // The song is selected and stays stopped (a heartbeat after it proves the select was taken): nothing is triggered.
    fake.send_raw(r#"{"setlistSelectSong":{"setlistSongID":11}}"#);
    let beats = rt.playback.status().heartbeat_serial;
    fake.beat();
    until("the heartbeat after the select", || {
        rt.playback.status().heartbeat_serial > beats
    })
    .await;
    service.tick_for_test(&rt).await;
    assert!(items().is_empty(), "{:?}", items());
    let beats = rt.playback.status().heartbeat_serial;

    // Play: the command reaches StagePilot before any heartbeat shows it; the item is triggered in the next step.
    let sent = std::time::Instant::now();
    fake.send_raw(r#"{"transportPlay":{"playing":true}}"#);
    until("the play command", || {
        rt.playback.moment().is_some_and(|m| m.playing)
    })
    .await;
    let seen = sent.elapsed();
    assert_eq!(
        rt.playback.status().heartbeat_serial,
        beats,
        "no heartbeat since the play"
    );
    service.tick_for_test(&rt).await;
    assert_eq!(items(), strings(&["item:1"]));
    assert!(seen < Duration::from_millis(300), "{seen:?}");
    assert_eq!(rt.playback.status().heartbeat_serial, beats);
    let status = service.status_json(&rt);
    assert_eq!(status["recent_triggers"][0]["kind"], "item");
    assert_eq!(status["recent_triggers"][0]["ok"], true);

    // Pause and play on again: the same start, so nothing is shown again.
    fake.send_raw(r#"{"transportPlay":{"playing":false}}"#);
    until("the pause", || {
        rt.playback.moment().is_some_and(|m| !m.playing)
    })
    .await;
    service.tick_for_test(&rt).await;
    assert_eq!(service.status_json(&rt)["state"], "ready");
    fake.send_raw(r#"{"transportPlay":{"playing":true}}"#);
    until("the resume", || {
        rt.playback.moment().is_some_and(|m| m.playing)
    })
    .await;
    service.tick_for_test(&rt).await;
    assert_eq!(items(), strings(&["item:1"]));

    // A seek on Playback is followed in the next step, without a heartbeat: 26 s is the cue at 24 s (flat slide 6).
    fake.send_raw(r#"{"waveformSeek":{"sequenceTime":26.0}}"#);
    until("the seek", || {
        rt.playback.moment().is_some_and(|m| m.position > 25.0)
    })
    .await;
    service.tick_for_test(&rt).await;
    assert_eq!(items(), strings(&["item:1", "slide:6"]));
    assert_eq!(rt.playback.status().heartbeat_serial, beats);

    // The song is stopped, taken back to the top and played again: its item is shown again, at once.
    fake.send_raw(r#"{"transportPlay":{"playing":false}}"#);
    fake.send_raw(r#"{"waveformSeek":{"sequenceTime":0.0}}"#);
    until("the stop and the seek", || {
        rt.playback
            .moment()
            .is_some_and(|m| !m.playing && m.position < 1.0)
    })
    .await;
    fake.send_raw(r#"{"transportPlay":{"playing":true}}"#);
    until("the second start", || {
        rt.playback
            .moment()
            .is_some_and(|m| m.playing && m.starts == 2)
    })
    .await;
    service.tick_for_test(&rt).await;
    assert_eq!(items(), strings(&["item:1", "slide:6", "item:1"]));
    assert_eq!(rt.playback.status().heartbeat_serial, beats);
    rt.stop().await;
}

#[tokio::test]
async fn a_play_command_for_a_song_that_cannot_be_followed_or_with_the_switch_off_triggers_nothing()
{
    let (rt, fake, api) = live().await;
    rt.propresenter_follow.stop();
    let service = &rt.propresenter_follow;
    service.read_if_due_for_test(&rt).await;
    // The switch off: the play command is seen and nothing is sent.
    let mut next = rt.settings_service.snapshot();
    next.propresenter.presenter_follow.enabled = false;
    rt.settings_service.save(next).unwrap();
    fake.send_raw(r#"{"transportPlay":{"playing":true}}"#);
    until("the play command", || {
        rt.playback.moment().is_some_and(|m| m.playing)
    })
    .await;
    service.tick_for_test(&rt).await;
    assert!(api.triggers().is_empty());
    // The link down (the reader's songs are not followable): the same.
    let mut next = rt.settings_service.snapshot();
    next.propresenter.presenter_follow.enabled = true;
    next.propresenter.enabled = false;
    next.integration_modes.timer_output = TimerOutput::Simulated;
    rt.settings_service.save(next).unwrap();
    service.work_out_again();
    service.read_if_due_for_test(&rt).await;
    service.tick_for_test(&rt).await;
    assert!(api.triggers().is_empty(), "{:?}", api.triggers());
    rt.stop().await;
}

// ---- the cost of following ------------------------------------------------------------------------------------------------

/// About 50 MB of saved settings: 20 000 songs' lighting data with a long name each (what the rig's settings.json was).
fn fifty_megabytes() -> PersistentSettings {
    let mut base = PersistentSettings::default();
    let name = "x".repeat(2_500);
    for i in 0..20_000 {
        let key = format!("song-{i}");
        base.lights.cue_maps.insert(
            key.clone(),
            serde_json::from_value(json!({"song_key": key, "song_title": name})).unwrap(),
        );
    }
    base
}

#[tokio::test]
async fn the_tick_and_the_reader_never_copy_the_settings_however_big_they_are() {
    let w = world_from(true, true, fifty_megabytes()).await;
    w.switch_direct(true, 0);
    let service = &w.rt.propresenter_follow;
    // The reader works the songs out once; the follower follows a playing song.
    service.read_if_due_for_test(&w.rt).await;
    w.play(5.0);
    service.tick_for_test(&w.rt).await;
    assert_eq!(log(&w), strings(&["item:1", "slide:1"]));

    // 100 ticks and 20 looks of the reader (the reader looks twice a second, the tick ten times): no copy of the settings.
    let copies = w.rt.settings_service.copies();
    let began = std::time::Instant::now();
    for _ in 0..100 {
        service.tick_for_test(&w.rt).await;
    }
    for _ in 0..20 {
        service.read_if_due_for_test(&w.rt).await;
    }
    let spent = began.elapsed();
    println!("100 ticks and 20 reader looks with 50 MB of settings: {spent:?}");
    assert_eq!(
        w.rt.settings_service.copies(),
        copies,
        "a tick or a look copied the settings"
    );
    assert!(
        spent < Duration::from_secs(5),
        "100 ticks took {spent:?}: something big is copied or parsed per tick"
    );

    // A save is noticed: the next tick reads the section again (once), and the new offset is in use.
    let mut next = w.rt.settings_service.snapshot();
    next.propresenter.presenter_follow.offset_ms = 500;
    w.rt.settings_service.save(next).unwrap();
    let copies = w.rt.settings_service.copies();
    service.tick_for_test(&w.rt).await;
    service.tick_for_test(&w.rt).await;
    assert!(w.rt.settings_service.copies() <= copies + 1);
    assert_eq!(w.status().await["offset_ms"], 500);
}
