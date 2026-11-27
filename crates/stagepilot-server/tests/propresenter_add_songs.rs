//! Adding the songs Playback plays to a ProPresenter playlist, through the routes: the preview, the apply as a job, the one job at a
//! time, the guards (a song playing, the Link, the plan that must still be true, the PIN and the origin), the input checks and the
//! playlist list. ProPresenter's files and API are in-memory stand-ins (the engine itself is tested in the propresenter crate).

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::TimerOutput;
use stagepilot_core::settings::{PersistentSettings, RuntimeSettings, SettingsService};
use stagepilot_core::state::TimerStatus;
use stagepilot_playback::library::{
    LibraryPlan, LibraryPlanSong, LibrarySection, LibrarySong, LibraryState, MatchConfidence,
    PlanMatch,
};
use stagepilot_propresenter::link::add_songs::Timing;
use stagepilot_propresenter::link::api::fake::{entry_uuid, FakeApi, Loading};
use stagepilot_propresenter::link::api::{ApiError, Layers};
use stagepilot_propresenter::link::fake::FakeHost;
use stagepilot_propresenter::pro_file::synthetic::SyntheticSong;
use stagepilot_propresenter::pro_file::Song;
use stagepilot_server::{build_app, AppOptions, OutputBackends, Runtime};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

fn song_file(title: &str, id: u64, seed: u32) -> Vec<u8> {
    SyntheticSong::new(title)
        .id(id)
        .seed(seed)
        .group("Intro", &["i"])
        .group("Verse 1", &["v"])
        .group("Chorus", &["c"])
        .arrangement("Full", &[0, 1, 2, 1, 2])
        .selected(0)
        .build()
}

fn lib_song(
    sid: i64,
    content: Option<u64>,
    position: u32,
    title: &str,
    sections: &[&str],
) -> LibrarySong {
    LibrarySong {
        setlist_song_id: Some(sid),
        content_id: content,
        position,
        title: title.to_string(),
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
        section_count: Some(sections.len() as u32),
        sections: sections
            .iter()
            .enumerate()
            .map(|(i, name)| LibrarySection {
                name: (*name).to_string(),
                start: i as f64 * 10.0,
                end: i as f64 * 10.0 + 9.0,
            })
            .collect(),
        artwork: None,
        rental_expires_utc: None,
        lyric_cues: Vec::new(),
        lyric_cue_count: 0,
        lyric_cue_max_slide: None,
    }
}

fn lib_plan(songs: Vec<LibrarySong>) -> LibraryPlan {
    LibraryPlan {
        enabled: true,
        state: LibraryState::Ready,
        message: None,
        plan_date: Some("2026-11-22".into()),
        setlist: None,
        newest: None,
        songs: songs
            .into_iter()
            .map(|song| LibraryPlanSong {
                key: format!("k{}", song.position),
                title: song.title.clone(),
                plan_position: song.position,
                plan_seconds: None,
                plan_key: None,
                status: PlanMatch::Matched,
                confidence: None,
                library: Some(song),
                candidate: None,
                pair_choices: vec![],
            })
            .collect(),
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
    host: Arc<FakeHost>,
    api: Arc<FakeApi>,
    playlist: String,
    backups: tempfile::TempDir,
}

fn save_host(rt: &Runtime) {
    let mut next = rt.settings_service.snapshot();
    next.integration_modes.timer_output = TimerOutput::Propresenter;
    next.propresenter.host = "127.0.0.1".to_string();
    rt.settings_service.save(next).unwrap();
}

async fn world(loading: Loading) -> World {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let settings = SettingsService::ephemeral_persistent(PersistentSettings::default());
    let rt = Runtime::with_backends(Arc::new(settings), false, OutputBackends::fake_midi());
    save_host(&rt);
    rt.propresenter_link
        .set_own_address_check(Arc::new(|_| true));
    let host = Arc::new(
        FakeHost::new()
            .with_file("Songs", "Alpha.pro", &song_file("Alpha", 1001, 0xA1000000))
            .with_file("Songs", "Beta.pro", &song_file("Beta", 1002, 0xB2000000))
            .with_file(
                "Songs",
                "Echo (Live).pro",
                &song_file("Echo", 3030, 0xE1000000),
            )
            .with_file(
                "Songs",
                "Echo (Studio).pro",
                &song_file("Echo", 3030, 0xE2000000),
            ),
    );
    host.set_import_song(2002, &song_file("Delta", 2002, 0xD4000000));
    rt.propresenter_link.set_local_host_for_test(host.clone());
    let api = Arc::new(FakeApi::new(host.clone(), loading));
    let alpha = entry_uuid("Songs", "Alpha.pro");
    let playlist = api.add_playlist(
        "Sunday",
        None,
        vec![
            FakeApi::header_item("Welcome", 0.1),
            FakeApi::presentation_item(&alpha, "Alpha", "", 12.5),
            FakeApi::header_item("Worship", 0.2),
            FakeApi::header_item("Message", 0.3),
        ],
    );
    let backups = tempfile::tempdir().unwrap();
    let service = &rt.propresenter_add_songs;
    service.set_api_for_test(Some(api.clone()));
    service.set_timing_for_test(Timing::quick());
    service.set_backup_dir_for_test(backups.path().join("propresenter-playlist-backups"));
    service.set_plan_for_test(Some(lib_plan(vec![
        lib_song(11, Some(1001), 1, "Alpha", &["Intro", "Verse 1", "Chorus"]),
        lib_song(12, Some(2002), 2, "Delta", &["Intro", "Chorus"]),
        lib_song(13, Some(1002), 3, "Beta", &["Chorus", "Intro"]),
        lib_song(14, None, 4, "No Number", &["Intro"]),
        lib_song(15, Some(3030), 5, "Echo", &["Intro", "Chorus"]),
    ])));
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    World {
        rt,
        app,
        host,
        api,
        playlist,
        backups,
    }
}

async fn call(app: &Router, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let (status, value, _) = raw(app, method, path, body.map(|b| b.to_string()), &[]).await;
    (status, value)
}

async fn raw(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<String>,
    headers: &[(&str, &str)],
) -> (StatusCode, Value, String) {
    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let mut request = match body {
        Some(b) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b)),
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
        String::from_utf8_lossy(&bytes).to_string(),
    )
}

const PREVIEW: &str = "/api/v1/propresenter-link/add-songs/preview";
const APPLY: &str = "/api/v1/propresenter-link/add-songs/apply";

fn ask(w: &World, ids: &[i64], place: Value) -> Value {
    let mut body = json!({"setlist_song_ids": ids, "playlist_id": w.playlist});
    for (k, v) in place.as_object().unwrap() {
        body[k] = v.clone();
    }
    body
}

fn worship() -> Value {
    json!({"after_header": "Worship"})
}

async fn finished(app: &Router, job: &str) -> Value {
    for _ in 0..400 {
        let (status, body) = call(
            app,
            "GET",
            &format!("/api/v1/propresenter-link/add-songs/jobs/{job}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if body["state"] != "running" {
            return body;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the job did not finish");
}

#[tokio::test]
async fn a_preview_says_what_would_happen_and_changes_nothing() {
    let w = world(Loading::Immediately).await;
    let (status, plan) = call(
        &w.app,
        "POST",
        PREVIEW,
        Some(ask(&w, &[13, 11, 12], worship())),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(plan["plan_id"].as_str().unwrap().len(), 32);
    let songs = plan["songs"].as_array().unwrap();
    let shown: Vec<(&str, &str, &str)> = songs
        .iter()
        .map(|s| {
            (
                s["ref_id"].as_str().unwrap(),
                s["action"].as_str().unwrap(),
                s["file"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("11", "copy", "Alpha (1)"),
            ("12", "import_then_copy", "Delta (1)"),
            ("13", "copy", "Beta (1)")
        ],
        "setlist order, whatever order was asked"
    );
    assert_eq!(plan["playlist"]["name"], "Sunday");
    assert_eq!(
        plan["playlist"]["insert_at"], 3,
        "the end of the Worship section: before the header Message"
    );
    assert_eq!(plan["to_add"], 3);
    assert_eq!(plan["can_apply"], true);
    // Nothing was written anywhere.
    assert!(w.host.files_of("Playback").is_empty());
    assert!(w.api.puts().is_empty());
    assert!(w.host.imports().is_empty());
    let calls = w.host.calls().join(" ");
    assert!(
        !calls.contains("create_file")
            && !calls.contains("ensure_library")
            && !calls.contains("search_import")
    );
    // By content number too.
    let (status, again) = call(
        &w.app,
        "POST",
        PREVIEW,
        Some(json!({"content_ids": [1001, 2002, 1002], "playlist_id": w.playlist, "place": "end"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["playlist"]["insert_at"], 4);
    // A song that is not in the setlist, or a playlist that is not there.
    let (status, body) = call(&w.app, "POST", PREVIEW, Some(ask(&w, &[11, 99], worship()))).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("unknown_song"))
    );
    assert_eq!(body["songs"], json!(["99"]));
    let mut other = ask(&w, &[11], worship());
    other["playlist_id"] = json!("00000000-0000-4000-8000-0000000000FF");
    let (status, body) = call(&w.app, "POST", PREVIEW, Some(other)).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("playlist_not_found"))
    );
    // The error shape is the existing one (`detail`), with a code beside it.
    assert!(body["detail"].as_str().unwrap().contains("playlist"));
}

#[tokio::test]
async fn a_song_paired_by_hand_is_found_by_playbacks_number_and_added_under_the_plan_songs_own_name(
) {
    let w = world(Loading::Immediately).await;
    let mut plan = lib_plan(vec![
        lib_song(11, Some(1001), 1, "Alpha", &["Intro", "Verse 1", "Chorus"]),
        lib_song(12, Some(2002), 2, "Delta", &["Intro", "Chorus"]),
        lib_song(13, Some(1002), 3, "Beta", &["Chorus", "Intro"]),
    ]);
    // The plan calls Playback's Alpha and Delta something else; a person paired them. Beta matches by name.
    for (i, name) in [(0, "Our Own Alpha"), (1, "Our Own Delta")] {
        plan.songs[i].title = name.to_string();
        plan.songs[i].confidence = Some(MatchConfidence::ByHand);
    }
    w.rt.propresenter_add_songs.set_plan_for_test(Some(plan));
    let (status, preview) = call(
        &w.app,
        "POST",
        PREVIEW,
        Some(ask(&w, &[11, 12, 13], worship())),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let shown: Vec<(&str, &str, &str, &str)> = preview["songs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["title"].as_str().unwrap(),
                s["action"].as_str().unwrap(),
                s["file"].as_str().unwrap(),
                s["source"].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            (
                "Our Own Alpha",
                "copy",
                "Our Own Alpha (1)",
                "Songs / Alpha.pro"
            ),
            ("Our Own Delta", "import_then_copy", "Our Own Delta (1)", ""),
            ("Beta", "copy", "Beta (1)", "Songs / Beta.pro"),
        ],
        "found by Playback's number, named after the plan song"
    );
    assert_eq!(preview["songs"][0]["content_id"], 1001);
    assert_eq!(preview["songs"][1]["import_as"], "Our Own Delta");
    let job = run_job(&w, &[11, 12, 13]).await;
    assert_eq!(job["state"], "done", "{job}");
    let names: Vec<String> = w
        .host
        .files_of("Playback")
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(
        names,
        [
            "Beta (1).pro",
            "Our Own Alpha (1).pro",
            "Our Own Delta (1).pro"
        ]
    );
}

#[tokio::test]
async fn apply_is_a_job_that_adds_the_songs_and_reports_each_song_and_step() {
    let w = world(Loading::AfterReads(1)).await;
    let (_, plan) = call(
        &w.app,
        "POST",
        PREVIEW,
        Some(ask(&w, &[11, 12, 13], worship())),
    )
    .await;
    let (status, started) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": plan["plan_id"], "skip": []})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    let job_id = started["job_id"].as_str().unwrap().to_string();
    let job = finished(&w.app, &job_id).await;
    assert_eq!(job["state"], "done", "{job}");
    assert_eq!(job["report"]["outcome"], "done", "{}", job["report"]);
    assert_eq!(job["report"]["differences"], json!([]));
    // Following Playback needs to know where each song went: the job remembered every song it added or found.
    let bindings = w.rt.propresenter_follow.bindings();
    let mut ids: Vec<u64> = bindings.keys().copied().collect();
    ids.sort_unstable();
    assert_eq!(ids, vec![1001, 1002, 2002], "{bindings:?}");
    assert!(bindings.values().all(|b| b.playlist_id == w.playlist));
    assert!(job["report"]["version"]
        .as_str()
        .unwrap()
        .contains("ProPresenter 19.0.1"));
    let backup = job["report"]["backup"].as_str().unwrap();
    assert!(
        std::path::Path::new(backup).starts_with(w.backups.path()),
        "{backup}"
    );
    // Overall steps and per-song steps are there in plain words.
    let overall: Vec<&str> = job["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["step"].as_str().unwrap())
        .collect();
    assert_eq!(
        overall,
        [
            "check",
            "backup",
            "library",
            "load",
            "playlist",
            "read_back"
        ]
    );
    let delta = job["songs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["ref_id"] == "12")
        .unwrap();
    let steps: Vec<&str> = delta["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["step"].as_str().unwrap())
        .collect();
    assert_eq!(steps, ["import", "copy"]);
    assert_eq!(delta["result"]["status"], "added");
    assert_eq!(delta["result"]["file"], "Delta (1)");
    // The playlist: three new songs at the end of the Worship section, everything else as it was.
    let stored = w.api.stored(&w.playlist);
    let names: Vec<&str> = stored
        .iter()
        .map(|i| i["id"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "Welcome",
            "Alpha",
            "Worship",
            "Alpha (1)",
            "Delta (1)",
            "Beta (1)",
            "Message"
        ]
    );
    assert_eq!(stored[1]["duration"], json!(12.5));
    let copy = Song::read(&w.host.file("Playback", "Delta (1).pro").unwrap()).unwrap();
    assert_eq!(copy.selected_arrangement().unwrap().name, "Playback");
    assert_eq!(copy.music.user_text, "Eb");
    // The job is still there to look at, and an unknown one is not.
    let (status, _) = call(
        &w.app,
        "GET",
        "/api/v1/propresenter-link/add-songs/jobs/00000000000000000000000000000000",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &w.app,
        "GET",
        "/api/v1/propresenter-link/add-songs/jobs/not-a-job",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn only_one_job_runs_at_a_time() {
    let w = world(Loading::Immediately).await;
    w.rt.propresenter_add_songs.set_timing_for_test(Timing {
        read_back: Duration::from_millis(500),
        ..Timing::quick()
    });
    let (_, plan) = call(&w.app, "POST", PREVIEW, Some(ask(&w, &[11], worship()))).await;
    let body = json!({"plan_id": plan["plan_id"], "skip": []});
    let (status, started) = call(&w.app, "POST", APPLY, Some(body.clone())).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    let (status, busy) = call(&w.app, "POST", APPLY, Some(body)).await;
    assert_eq!(
        (status, busy["code"].as_str()),
        (StatusCode::CONFLICT, Some("busy")),
        "{busy}"
    );
    let job = finished(&w.app, started["job_id"].as_str().unwrap()).await;
    assert_eq!(job["report"]["outcome"], "done");
    // When it has finished, the place is free again (the plan is stale by now, which is its own answer).
    let (status, again) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": plan["plan_id"]})),
    )
    .await;
    assert_ne!(again["code"].as_str(), Some("busy"), "{status} {again}");
}

#[tokio::test]
async fn nothing_starts_while_a_song_is_playing() {
    let w = world(Loading::Immediately).await;
    let (_, plan) = call(&w.app, "POST", PREVIEW, Some(ask(&w, &[11], worship()))).await;
    w.rt.store.mutate(|s| s.timer.status = TimerStatus::Running);
    let (status, body) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": plan["plan_id"]})),
    )
    .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some("song_playing")),
        "{body}"
    );
    assert!(body["detail"].as_str().unwrap().contains("song is playing"));
    assert!(w.host.files_of("Playback").is_empty());
    assert!(w.api.puts().is_empty());
    assert!(
        !w.backups
            .path()
            .join("propresenter-playlist-backups")
            .exists(),
        "no backup, nothing at all"
    );
    // A song that stopped lets it go on.
    w.rt.store.mutate(|s| s.timer.status = TimerStatus::Stopped);
    let (status, body) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": plan["plan_id"]})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    finished(&w.app, body["job_id"].as_str().unwrap()).await;
}

#[tokio::test]
async fn a_plan_that_is_not_true_any_more_is_refused() {
    let w = world(Loading::Immediately).await;
    let (_, plan) = call(&w.app, "POST", PREVIEW, Some(ask(&w, &[11], worship()))).await;
    // An unknown or malformed plan.
    let (status, body) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": "0".repeat(32)})),
    )
    .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("unknown_plan"))
    );
    // Someone changes the playlist in ProPresenter after the preview.
    let mut items = w.api.stored(&w.playlist);
    items.push(FakeApi::presentation_item(
        &entry_uuid("Songs", "Beta.pro"),
        "Beta",
        "",
        5.0,
    ));
    stagepilot_propresenter::link::api::ProPresenterApi::put_playlist(&*w.api, &w.playlist, &items)
        .await
        .unwrap();
    let (status, body) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": plan["plan_id"]})),
    )
    .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some("plan_changed")),
        "{body}"
    );
    assert!(w.host.files_of("Playback").is_empty());
    // Playback's arrangement changes after the preview: the same.
    let (_, plan) = call(&w.app, "POST", PREVIEW, Some(ask(&w, &[11], worship()))).await;
    w.rt.propresenter_add_songs
        .set_plan_for_test(Some(lib_plan(vec![lib_song(
            11,
            Some(1001),
            1,
            "Alpha",
            &["Intro", "Chorus"],
        )])));
    let (status, body) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": plan["plan_id"]})),
    )
    .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some("plan_changed")),
        "{body}"
    );
}

#[tokio::test]
async fn songs_that_cannot_be_added_must_be_left_out_by_name() {
    let w = world(Loading::Immediately).await;
    let (_, plan) = call(&w.app, "POST", PREVIEW, Some(ask(&w, &[11, 14], worship()))).await;
    assert_eq!(plan["songs"][1]["action"], "blocked");
    assert!(plan["songs"][1]["reason"]
        .as_str()
        .unwrap()
        .contains("MultiTracks number"));
    assert_eq!(plan["stuck"], 1);
    let (status, body) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": plan["plan_id"]})),
    )
    .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some("songs_blocked")),
        "{body}"
    );
    assert_eq!(body["songs"], json!(["14"]));
    let (status, body) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": plan["plan_id"], "skip": ["77"]})),
    )
    .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("unknown_skip"))
    );
    let (status, body) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": plan["plan_id"], "skip": ["14"]})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job = finished(&w.app, body["job_id"].as_str().unwrap()).await;
    assert_eq!(job["report"]["outcome"], "done");
    let left_out = job["report"]["songs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["ref_id"] == "14")
        .unwrap();
    assert_eq!(left_out["status"], "skipped");
}

#[tokio::test]
async fn a_library_that_will_not_load_asks_to_be_opened_and_try_again_works() {
    let w = world(Loading::AfterReads(100_000)).await;
    let (_, plan) = call(&w.app, "POST", PREVIEW, Some(ask(&w, &[11], worship()))).await;
    let (_, started) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": plan["plan_id"]})),
    )
    .await;
    let job = finished(&w.app, started["job_id"].as_str().unwrap()).await;
    assert_eq!(job["report"]["outcome"], "needs_library_open", "{job}");
    assert!(job["report"]["message"]
        .as_str()
        .unwrap()
        .contains("Open the Playback library in ProPresenter once, then press Try again"));
    assert!(w.api.puts().is_empty());
    // The person opens it; "Try again" is a new preview and apply, and the file made before is used.
    w.api.set_loading(Loading::Immediately);
    let (_, plan) = call(&w.app, "POST", PREVIEW, Some(ask(&w, &[11], worship()))).await;
    assert_eq!(plan["songs"][0]["action"], "reuse");
    let (_, started) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": plan["plan_id"]})),
    )
    .await;
    let job = finished(&w.app, started["job_id"].as_str().unwrap()).await;
    assert_eq!(job["report"]["outcome"], "done", "{job}");
    assert_eq!(w.host.files_of("Playback").len(), 1);
}

#[tokio::test]
async fn a_refused_playlist_write_is_a_result_with_the_backup_kept() {
    let w = world(Loading::Immediately).await;
    w.api.fail_put(Some(ApiError::NotFound));
    let (_, plan) = call(&w.app, "POST", PREVIEW, Some(ask(&w, &[11], worship()))).await;
    let (_, started) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": plan["plan_id"]})),
    )
    .await;
    let job = finished(&w.app, started["job_id"].as_str().unwrap()).await;
    assert_eq!(job["report"]["outcome"], "playlist_refused");
    assert_eq!(job["report"]["playlist_changed"], false);
    let backup = job["report"]["backup"].as_str().unwrap();
    assert!(std::path::Path::new(backup).exists());
    assert!(job["report"]["message"].as_str().unwrap().contains(backup));
}

#[tokio::test]
async fn without_a_ready_link_nothing_is_asked_of_propresenter() {
    // No ProPresenter computer chosen.
    let settings = SettingsService::ephemeral_persistent(PersistentSettings::default());
    let rt = Runtime::with_backends(Arc::new(settings), false, OutputBackends::fake_midi());
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    let body = json!({"setlist_song_ids": [1], "playlist_id": "x", "place": "end"});
    let (status, answer) = call(&app, "POST", PREVIEW, Some(body.clone())).await;
    assert_eq!(
        (status, answer["code"].as_str()),
        (StatusCode::CONFLICT, Some("no_propresenter")),
        "{answer}"
    );
    let (status, answer) = call(&app, "GET", "/api/v1/propresenter-link/playlists", None).await;
    assert_eq!(
        (status, answer["code"].as_str()),
        (StatusCode::CONFLICT, Some("no_propresenter"))
    );
    // A computer that is not this one and has no helper.
    save_host(&rt);
    rt.propresenter_link
        .set_own_address_check(Arc::new(|_| false));
    rt.propresenter_link.set_helper_port(1);
    let (status, answer) = call(&app, "POST", PREVIEW, Some(body)).await;
    assert_eq!(
        (status, answer["code"].as_str()),
        (StatusCode::CONFLICT, Some("link_not_ready")),
        "{answer}"
    );
    assert!(!answer["detail"].as_str().unwrap().is_empty());
    let (status, answer) = call(
        &app,
        "POST",
        APPLY,
        Some(json!({"plan_id": "0".repeat(32)})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
}

#[tokio::test]
async fn every_input_is_checked_and_no_path_is_taken() {
    let w = world(Loading::Immediately).await;
    let id = &w.playlist;
    let bad = |body: Value| {
        let app = w.app.clone();
        async move {
            let (status, answer) = call(&app, "POST", PREVIEW, Some(body)).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
            assert!(answer["detail"].as_str().is_some_and(|d| !d.is_empty()));
        }
    };
    bad(json!({"playlist_id": id, "place": "end"})).await;
    bad(json!({"setlist_song_ids": [], "playlist_id": id, "place": "end"})).await;
    bad(json!({"setlist_song_ids": ["11"], "playlist_id": id, "place": "end"})).await;
    bad(json!({"setlist_song_ids": [-5], "playlist_id": id, "place": "end"})).await;
    bad(
        json!({"setlist_song_ids": [11], "content_ids": [1001], "playlist_id": id, "place": "end"}),
    )
    .await;
    bad(json!({"setlist_song_ids": vec![11; 61], "playlist_id": id, "place": "end"})).await;
    bad(json!({"setlist_song_ids": [11], "playlist_id": "../../etc/passwd", "place": "end"})).await;
    bad(json!({"setlist_song_ids": [11], "playlist_id": id})).await;
    bad(json!({"setlist_song_ids": [11], "playlist_id": id, "place": "end", "after_header": "x"}))
        .await;
    bad(json!({"setlist_song_ids": [11], "playlist_id": id, "after_header": "x\u{7}"})).await;
    // Nothing from the caller names a file, a library or a folder.
    for field in ["path", "file", "library", "folder", "backup_dir", "source"] {
        let mut body = json!({"setlist_song_ids": [11], "playlist_id": id, "place": "end"});
        body[field] = json!("/etc");
        bad(body).await;
    }
    let (status, _) = call(&w.app, "POST", APPLY, Some(json!({"plan_id": "short"}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": "a".repeat(32), "skip": [1]})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": "a".repeat(32), "skip": ["../x"]})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": "a".repeat(32), "extra": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, _) = raw(&w.app, "POST", PREVIEW, Some("[1, 2]".into()), &[]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, _) = raw(&w.app, "POST", PREVIEW, Some("x".repeat(20_000)), &[]).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let (status, _, _) = raw(&w.app, "POST", APPLY, Some("not json".into()), &[]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // Other methods are not routes.
    let (status, _) = call(&w.app, "GET", PREVIEW, None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    let (status, _) = call(
        &w.app,
        "DELETE",
        "/api/v1/propresenter-link/add-songs/jobs/00000000000000000000000000000000",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn the_playlist_list_has_the_headers_and_is_kept_for_ten_seconds() {
    let w = world(Loading::Immediately).await;
    w.api.add_playlist(
        "Evening",
        Some("Services"),
        vec![
            FakeApi::header_item("Opening", 0.1),
            FakeApi::header_item("Closing", 0.2),
        ],
    );
    let (status, body) = call(&w.app, "GET", "/api/v1/propresenter-link/playlists", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let list = body["playlists"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["name"], "Sunday");
    assert_eq!(list[0]["headers"], json!(["Welcome", "Worship", "Message"]));
    assert_eq!(list[0]["id"], json!(w.playlist));
    assert_eq!(list[1]["folder"], "Services");
    assert_eq!(list[1]["headers"], json!(["Opening", "Closing"]));
    let asked = w
        .api
        .calls()
        .iter()
        .filter(|c| c.as_str() == "playlists")
        .count();
    let (_, again) = call(&w.app, "GET", "/api/v1/propresenter-link/playlists", None).await;
    assert_eq!(again, body);
    assert_eq!(
        w.api
            .calls()
            .iter()
            .filter(|c| c.as_str() == "playlists")
            .count(),
        asked,
        "the second ask is served from what was read less than ten seconds ago"
    );
    // ProPresenter not answering is one plain sentence and a 502.
    let fresh = world(Loading::Immediately).await;
    fresh.api.set_down(true);
    let (status, body) = call(
        &fresh.app,
        "GET",
        "/api/v1/propresenter-link/playlists",
        None,
    )
    .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::BAD_GATEWAY, Some("propresenter_unreachable"))
    );
    assert!(body["detail"].as_str().unwrap().contains("Network"));
}

#[tokio::test]
async fn the_pin_and_the_origin_rules_of_the_other_write_routes_apply() {
    // A dashboard with a PIN: nobody who is not signed in gets a preview, a job or the playlists.
    let rt = Runtime::new(
        Arc::new(SettingsService::ephemeral(RuntimeSettings::demo())),
        true,
    );
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    let lan =
        |app: &Router, method: &'static str, path: &'static str, body: Option<&'static str>| {
            let app = app.clone();
            async move {
                let mut request = Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.unwrap_or("")))
                    .unwrap();
                request.extensions_mut().insert(ConnectInfo(
                    "192.168.4.77:50000".parse::<SocketAddr>().unwrap(),
                ));
                app.oneshot(request).await.unwrap().status()
            }
        };
    let preview = r#"{"setlist_song_ids":[1],"playlist_id":"x","place":"end"}"#;
    assert_eq!(
        lan(&app, "POST", PREVIEW, Some(preview)).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        lan(&app, "POST", APPLY, Some(r#"{"plan_id":"x"}"#)).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        lan(&app, "GET", "/api/v1/propresenter-link/playlists", None).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        lan(
            &app,
            "GET",
            "/api/v1/propresenter-link/add-songs/jobs/00000000000000000000000000000000",
            None
        )
        .await,
        StatusCode::UNAUTHORIZED
    );
    // A page from another site cannot post to it, even from this computer.
    let w = world(Loading::Immediately).await;
    let (status, _, text) = raw(
        &w.app,
        "POST",
        PREVIEW,
        Some(ask(&w, &[11], worship()).to_string()),
        &[
            ("origin", "http://evil.example"),
            ("host", "127.0.0.1:8100"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{text}");
    assert!(
        w.host.calls().is_empty(),
        "nothing was asked of ProPresenter's computer"
    );
}

// ---- guesses and the screen, through the routes ------------------------------------------------------------------------------

/// A song with two different blank groups (Count Off) and two different Interludes.
fn gamma() -> Vec<u8> {
    SyntheticSong::new("Gamma")
        .id(3003)
        .seed(0xC3000000)
        .group("", &["count a"])
        .group("", &["count b"])
        .group("Intro", &["intro"])
        .group("Verse 1", &["verse"])
        .group("Interlude", &["interlude x"])
        .group("Interlude", &["interlude y"])
        .arrangement("Full", &[2, 3, 4, 5])
        .selected(0)
        .build()
}

async fn run_job(w: &World, ids: &[i64]) -> Value {
    let (status, plan) = call(&w.app, "POST", PREVIEW, Some(ask(w, ids, worship()))).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    let (status, started) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": plan["plan_id"]})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    finished(&w.app, started["job_id"].as_str().unwrap()).await
}

#[tokio::test]
async fn guessed_sections_are_listed_in_the_preview_and_in_the_result_and_never_block() {
    let w = world(Loading::Immediately).await;
    w.host.set_import_song(3003, &gamma());
    let sections = [
        "Count Off",
        "Intro",
        "Verse 1",
        "Interlude",
        "Interlude",
        "Interlude",
    ];
    w.rt.propresenter_add_songs
        .set_plan_for_test(Some(lib_plan(vec![
            lib_song(21, Some(3003), 1, "Gamma", &sections),
            lib_song(11, Some(1001), 2, "Alpha", &["Intro", "Verse 1"]),
        ])));
    // Before the import the sections cannot be matched: the preview says so and has no guesses yet.
    let (_, plan) = call(&w.app, "POST", PREVIEW, Some(ask(&w, &[21], worship()))).await;
    assert_eq!(plan["songs"][0]["action"], "import_then_copy");
    assert_eq!(plan["songs"][0]["guesses"], json!([]));
    assert_eq!(plan["can_apply"], true);
    let job = run_job(&w, &[21]).await;
    assert_eq!(job["report"]["outcome"], "done", "{job}");
    let guesses = &job["report"]["songs"][0]["guesses"];
    assert_eq!(guesses.as_array().unwrap().len(), 2, "{guesses}");
    assert_eq!(guesses[0]["section"], "Count Off");
    assert_eq!(guesses[0]["group_name"], "");
    assert_eq!(guesses[1]["section"], "Interlude");
    assert_eq!(guesses[1]["position"], 3);
    assert_eq!(guesses[1]["group_index"], 2);
    assert_eq!(guesses[1]["rule"], "last_reused");
    assert!(guesses[1]["text"].as_str().unwrap().starts_with(
        "Interlude (3rd use) uses the 2nd \"Interlude\" group: check it in ProPresenter."
    ));
    assert_eq!(job["songs"][0]["result"]["guesses"], *guesses);
}

#[tokio::test]
async fn nothing_on_screen_is_touched_while_propresenter_shows_something_and_try_again_works() {
    let w = world(Loading::Immediately).await;
    w.api.set_layers(Layers {
        video_input: true,
        slide: true,
        ..Layers::default()
    });
    let job = run_job(&w, &[12]).await;
    let report = &job["report"];
    assert_eq!(report["outcome"], "live_on_screen", "{job}");
    assert!(report["message"]
        .as_str()
        .unwrap()
        .contains("ProPresenter is showing something on screen (slides). Clear it or wait, then press Try again."));
    assert_eq!(report["songs"][0]["code"], "live_on_screen");
    assert_eq!(report["playlist_changed"], false);
    assert!(w
        .host
        .calls()
        .iter()
        .all(|c| !c.starts_with("search_import") && !c.starts_with("select_library")));
    assert!(w.api.puts().is_empty());
    // The slides are cleared (a camera input stays on): Try again.
    w.api.set_layers(Layers {
        video_input: true,
        ..Layers::default()
    });
    let job = run_job(&w, &[12]).await;
    assert_eq!(job["report"]["outcome"], "done", "{job}");
    assert_eq!(w.host.imports().len(), 1);
}

// ---- songs asked for by title alone (no Playback data) --------------------------------------------------------------------------

const TITLES: &str = "/api/v1/propresenter-link/add-songs/titles";

#[tokio::test]
async fn titles_are_looked_up_and_added_as_they_are_with_no_playback_data_at_all() {
    let w = world(Loading::Immediately).await;
    // Playback is not asked: with no plan at all the title route still works.
    w.rt.propresenter_add_songs.set_plan_for_test(None);
    let (status, found) = call(
        &w.app,
        "POST",
        TITLES,
        Some(json!({"titles": ["beta", "Nowhere Song"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(
        found,
        json!({"titles": [
            {"title": "beta", "state": "found"},
            {"title": "Nowhere Song", "state": "missing"},
        ]})
    );
    let body = json!({"titles": ["Nowhere Song", "Beta"], "playlist_id": w.playlist, "after_header": "Worship"});
    let (status, plan) = call(&w.app, "POST", PREVIEW, Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    let songs = plan["songs"].as_array().unwrap();
    let shown: Vec<(&str, &str, &str)> = songs
        .iter()
        .map(|s| {
            (
                s["ref_id"].as_str().unwrap(),
                s["title"].as_str().unwrap(),
                s["action"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [("t1", "Nowhere Song", "blocked"), ("t2", "Beta", "as_is")]
    );
    assert!(songs[0]["reason"]
        .as_str()
        .unwrap()
        .contains("Playback's data is needed to import it"));
    assert_eq!(
        (plan["to_add"].as_u64(), plan["stuck"].as_u64()),
        (Some(1), Some(1))
    );
    // The missing one is left out by name, then the job adds Beta as it is.
    let (status, started) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": plan["plan_id"], "skip": ["t1"]})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    let job = finished(&w.app, started["job_id"].as_str().unwrap()).await;
    assert_eq!(job["state"], "done", "{job}");
    assert_eq!(job["report"]["outcome"], "done", "{job}");
    let statuses: Vec<&str> = job["report"]["songs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["skipped", "added"]);
    assert!(w.host.imports().is_empty());
    assert!(w.host.files_of("Playback").is_empty());
    let names: Vec<String> = w
        .api
        .stored(&w.playlist)
        .iter()
        .map(|i| i["id"]["name"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(names, ["Welcome", "Alpha", "Worship", "Beta", "Message"]);
}

#[tokio::test]
async fn the_title_inputs_are_checked_too() {
    let w = world(Loading::Immediately).await;
    let id = &w.playlist;
    let bad = |path: &'static str, body: Value| {
        let app = w.app.clone();
        async move {
            let (status, answer) = call(&app, "POST", path, Some(body)).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
            assert!(answer["detail"].as_str().is_some_and(|d| !d.is_empty()));
        }
    };
    bad(TITLES, json!({})).await;
    bad(TITLES, json!({"titles": []})).await;
    bad(TITLES, json!({"titles": ["A", "a"]})).await;
    bad(TITLES, json!({"titles": [5]})).await;
    bad(TITLES, json!({"titles": ["bell\u{7}"]})).await;
    bad(TITLES, json!({"titles": ["x".repeat(121)]})).await;
    bad(TITLES, json!({"titles": vec!["x"; 61]})).await;
    bad(TITLES, json!({"titles": ["A"], "path": "/etc"})).await;
    bad(
        PREVIEW,
        json!({"titles": [], "playlist_id": id, "place": "end"}),
    )
    .await;
    bad(
        PREVIEW,
        json!({"titles": ["A"], "setlist_song_ids": [11], "playlist_id": id, "place": "end"}),
    )
    .await;
    bad(
        PREVIEW,
        json!({"titles": ["A"], "content_ids": [1001], "playlist_id": id, "place": "end"}),
    )
    .await;
    // Without a ready Link nothing is asked of ProPresenter for titles either.
    let settings = SettingsService::ephemeral_persistent(PersistentSettings::default());
    let rt = Runtime::with_backends(Arc::new(settings), false, OutputBackends::fake_midi());
    let app = build_app(rt, AppOptions { web_root: None });
    let (status, answer) = call(&app, "POST", TITLES, Some(json!({"titles": ["A"]}))).await;
    assert_eq!(
        (status, answer["code"].as_str()),
        (StatusCode::CONFLICT, Some("no_propresenter")),
        "{answer}"
    );
}

#[tokio::test]
async fn a_pick_goes_in_the_preview_is_part_of_the_plan_and_is_what_apply_does() {
    let w = world(Loading::Immediately).await;
    let live = "Songs / Echo (Live).pro";
    let studio = "Songs / Echo (Studio).pro";
    let with = |sources: Value| {
        let mut body = ask(&w, &[15], worship());
        body["sources"] = sources;
        body
    };
    // Nobody picked: two files are this song, in short words, and both are listed.
    let (status, plain) = call(&w.app, "POST", PREVIEW, Some(ask(&w, &[15], worship()))).await;
    assert_eq!(status, StatusCode::OK, "{plain}");
    assert_eq!(plain["songs"][0]["action"], "needs_choice");
    assert_eq!(
        plain["songs"][0]["reason"],
        "Two files are this song. Pick one."
    );
    assert_eq!(plain["songs"][0]["choices"], json!([live, studio]));
    assert_eq!(plain["songs"][0]["chosen_by"], Value::Null);
    // A pick for song 15 (its setlist number): used, said, and another pick is another plan.
    let (status, picked) = call(&w.app, "POST", PREVIEW, Some(with(json!({"15": studio})))).await;
    assert_eq!(status, StatusCode::OK, "{picked}");
    let song = &picked["songs"][0];
    assert_eq!(
        (
            song["action"].as_str(),
            song["source"].as_str(),
            song["chosen_by"].as_str(),
            song["chosen_file"].as_str()
        ),
        (Some("copy"), Some(studio), Some("pick"), Some(studio))
    );
    assert_eq!(song["choices"], json!([live, studio]));
    let (_, other) = call(&w.app, "POST", PREVIEW, Some(with(json!({"15": live})))).await;
    assert_ne!(picked["plan_id"], other["plan_id"]);
    // What does not fit is ignored, never an error: a wrong type, another song's key, a file that is none of its files.
    for junk in [
        json!("Songs / Echo (Live).pro"),
        json!([live]),
        json!({"15": 7}),
        json!({"15": null}),
        json!({"15": "Songs / Beta.pro"}),
        json!({"99": live, "": live}),
        json!({"15": "x".repeat(500)}),
        json!({"15": "bad\u{7}"}),
    ] {
        let (status, answer) = call(&w.app, "POST", PREVIEW, Some(with(junk.clone()))).await;
        assert_eq!(status, StatusCode::OK, "{junk}: {answer}");
        assert_eq!(answer["songs"][0]["action"], "needs_choice", "{junk}");
    }
    // Apply takes the plan_id and does what was previewed with the pick: the copy is made from the picked file.
    let (_, preview) = call(&w.app, "POST", PREVIEW, Some(with(json!({"15": studio})))).await;
    let (status, started) = call(
        &w.app,
        "POST",
        APPLY,
        Some(json!({"plan_id": preview["plan_id"]})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    let done = finished(&w.app, started["job_id"].as_str().unwrap()).await;
    assert_eq!(done["report"]["outcome"], "done", "{done}");
    let made: Vec<String> = w
        .host
        .files_of("Playback")
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(made, ["Echo (Studio) (1).pro"]);
    // The old id of another pick still refers to ITS pick only; the plan without a pick is another plan again.
    assert_ne!(plain["plan_id"], preview["plan_id"]);
}
