//! The whole flow through the routes against stand-ins for a ProPresenter computer, three ways, with the same result:
//!
//! * `memory`: the in-memory host and API (`FakeHost`, `FakeApi`),
//! * `local`: StagePilot on the ProPresenter computer: the real `LocalHost` on a temporary ProPresenter folder, with
//!   `tools/fake_propresenter_api.py` (ProPresenter's API, reading the same folder) over HTTP,
//! * `helper`: another computer: the real ProPresenter Link helper (`tools/fake_propresenter_link.py`, only its screen replaced)
//!   over signed requests, with the same fake API.
//!
//! The Python ones are skipped when there is no `python3` on this computer (and on a computer that is not Unix).

#![cfg(unix)]

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::TimerOutput;
use stagepilot_core::settings::{PersistentSettings, SettingsService};
use stagepilot_playback::library::{
    LibraryPlan, LibraryPlanSong, LibrarySection, LibrarySong, LibraryState, PlanMatch,
};
use stagepilot_propresenter::link::add_songs::Timing;
use stagepilot_propresenter::link::api::fake::{entry_uuid, FakeApi, Loading};
use stagepilot_propresenter::link::api::{HttpApi, ProPresenterApi};
use stagepilot_propresenter::link::fake::FakeHost;
use stagepilot_propresenter::pro_file::synthetic::SyntheticSong;
use stagepilot_propresenter::pro_file::Song;
use stagepilot_server::{build_app, AppOptions, OutputBackends, Runtime};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

const KEY: &str = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE";

fn song_file(title: &str, id: u64, seed: u32) -> Vec<u8> {
    SyntheticSong::new(title)
        .id(id)
        .seed(seed)
        .group("", &["count"])
        .group("Intro", &["i"])
        .group("Verse 1", &["v"])
        .group("Chorus", &["c"])
        .arrangement("Full", &[0, 1, 2, 3, 2, 3])
        .selected(0)
        .build()
}

fn lib_song(
    sid: i64,
    content: u64,
    position: u32,
    title: &str,
    key: &str,
    sections: &[&str],
) -> LibrarySong {
    LibrarySong {
        setlist_song_id: Some(sid),
        content_id: Some(content),
        position,
        title: title.to_string(),
        key: Some(key.to_string()),
        key_original: None,
        key_was: None,
        key_changed_at: None,
        has_waveform: false,
        bpm: None,
        scan_length_s: None,
        countdown_length_s: None,
        length_source: None,
        arrangement: Some("Default".into()),
        section_count: Some(sections.len() as u32),
        sections: sections
            .iter()
            .enumerate()
            .map(|(i, n)| LibrarySection {
                name: (*n).to_string(),
                start: i as f64,
                end: i as f64 + 0.5,
            })
            .collect(),
        artwork: None,
        rental_expires_utc: None,
        lyric_cues: Vec::new(),
        lyric_cue_count: 0,
        lyric_cue_max_slide: None,
    }
}

fn plan(songs: Vec<LibrarySong>) -> LibraryPlan {
    LibraryPlan {
        enabled: true,
        state: LibraryState::Ready,
        message: None,
        plan_date: None,
        setlist: None,
        newest: None,
        songs: songs
            .into_iter()
            .map(|s| LibraryPlanSong {
                key: format!("k{}", s.position),
                title: s.title.clone(),
                plan_position: s.position,
                plan_seconds: None,
                plan_key: None,
                status: PlanMatch::Matched,
                confidence: None,
                library: Some(s),
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

struct Proc(Child);

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn tools(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools")
        .join(name)
}

fn spawn(args: &[String]) -> Option<(Proc, u16)> {
    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| p.parse().ok())?;
    let child = Command::new("python3")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let proc = Proc(child);
    for _ in 0..100 {
        if std::net::TcpStream::connect_timeout(
            &([127, 0, 0, 1], port).into(),
            Duration::from_millis(100),
        )
        .is_ok()
        {
            return Some((proc, port));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

fn lay_out(root: &Path) -> PathBuf {
    let folder = root.join("ProPresenter");
    std::fs::create_dir_all(folder.join("Libraries/Songs")).unwrap();
    for (file, title, id, seed) in [
        ("Alpha.pro", "Alpha", 1001, 0xA1000000u32),
        ("Beta.pro", "Beta", 1002, 0xB2000000),
    ] {
        std::fs::write(
            folder.join("Libraries/Songs").join(file),
            song_file(title, id, seed),
        )
        .unwrap();
    }
    folder
}

/// What a run leaves behind, in a form that is the same wherever ProPresenter's files are.
#[derive(Debug, PartialEq)]
struct Left {
    outcome: String,
    actions: Vec<String>,
    playlist: Vec<(String, String, String)>,
    playback_files: Vec<String>,
    copies: Vec<(String, String, String, Vec<String>)>,
    differences: Vec<String>,
}

fn describe(bytes: &[u8]) -> (String, String, Vec<String>) {
    let song = Song::read(bytes).unwrap();
    let selected = song.selected_arrangement().unwrap();
    (
        selected.name.clone(),
        song.music.user_text.clone(),
        song.arrangement_group_names(selected),
    )
}

async fn run(app: &Router, playlist: &str) -> (Value, Value) {
    let (status, preview) = call(
        app,
        "POST",
        "/api/v1/propresenter-link/add-songs/preview",
        Some(json!({"setlist_song_ids": [11, 12], "playlist_id": playlist, "after_header": "Worship"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let (status, started) = call(
        app,
        "POST",
        "/api/v1/propresenter-link/add-songs/apply",
        Some(json!({"plan_id": preview["plan_id"], "skip": []})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    let job_id = started["job_id"].as_str().unwrap().to_string();
    for _ in 0..1000 {
        let (_, job) = call(
            app,
            "GET",
            &format!("/api/v1/propresenter-link/add-songs/jobs/{job_id}"),
            None,
        )
        .await;
        if job["state"] != "running" {
            return (preview, job);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the job did not finish");
}

fn timing() -> Timing {
    Timing {
        poll: Duration::from_millis(100),
        select_after: Duration::from_millis(400),
        give_up: Duration::from_secs(4),
        read_back: Duration::from_millis(50),
    }
}

fn runtime() -> Arc<Runtime> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let settings = SettingsService::ephemeral_persistent(PersistentSettings::default());
    let rt = Runtime::with_backends(Arc::new(settings), false, OutputBackends::fake_midi());
    let service = &rt.propresenter_add_songs;
    service.set_timing_for_test(timing());
    service.set_plan_for_test(Some(plan(vec![
        lib_song(
            11,
            1001,
            1,
            "Alpha",
            "Eb",
            &["Count Off", "Intro", "Verse 1", "Chorus", "Chorus"],
        ),
        lib_song(12, 1002, 2, "Beta", "F#m", &["Intro", "Chorus", "Verse 1"]),
    ])));
    rt
}

fn seen(preview: &Value, job: &Value) -> (String, Vec<String>, Vec<String>) {
    (
        job["report"]["outcome"]
            .as_str()
            .unwrap_or("none")
            .to_string(),
        preview["songs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                format!(
                    "{} {}",
                    s["action"].as_str().unwrap(),
                    s["file"].as_str().unwrap_or("-")
                )
            })
            .collect(),
        job["report"]["differences"]
            .as_array()
            .map(|d| d.iter().map(|x| x.as_str().unwrap().to_string()).collect())
            .unwrap_or_default(),
    )
}

async fn playlist_of(api: &dyn ProPresenterApi, id: &str) -> Vec<(String, String, String)> {
    api.playlist(id).await.unwrap()["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["id"]["name"].as_str().unwrap().to_string(),
                i["type"].as_str().unwrap_or("presentation").to_string(),
                i["presentation_info"]["arrangement_name"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
            )
        })
        .collect()
}

fn playlist_items(songs_uuid: &str) -> Vec<Value> {
    vec![
        FakeApi::header_item("Welcome", 0.1),
        FakeApi::presentation_item(songs_uuid, "Alpha", "", 12.5),
        FakeApi::header_item("Worship", 0.2),
        FakeApi::header_item("Message", 0.3),
    ]
}

async fn in_memory() -> Left {
    let rt = runtime();
    let mut next = rt.settings_service.snapshot();
    next.integration_modes.timer_output = TimerOutput::Propresenter;
    next.propresenter.host = "127.0.0.1".into();
    rt.settings_service.save(next).unwrap();
    rt.propresenter_link
        .set_own_address_check(Arc::new(|_| true));
    let host = Arc::new(
        FakeHost::new()
            .with_file("Songs", "Alpha.pro", &song_file("Alpha", 1001, 0xA1000000))
            .with_file("Songs", "Beta.pro", &song_file("Beta", 1002, 0xB2000000)),
    );
    rt.propresenter_link.set_local_host_for_test(host.clone());
    let api = Arc::new(FakeApi::new(host.clone(), Loading::AfterSelect));
    let playlist = api.add_playlist(
        "Sunday",
        None,
        playlist_items(&entry_uuid("Songs", "Alpha.pro")),
    );
    let backups = tempfile::tempdir().unwrap();
    rt.propresenter_add_songs
        .set_api_for_test(Some(api.clone()));
    rt.propresenter_add_songs
        .set_backup_dir_for_test(backups.path().join("b"));
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    let (preview, job) = run(&app, &playlist).await;
    let (outcome, actions, differences) = seen(&preview, &job);
    let mut files: Vec<(String, Vec<u8>)> = host.files_of("Playback");
    files.sort();
    Left {
        outcome,
        actions,
        playlist: playlist_of(&*api, &playlist).await,
        playback_files: files.iter().map(|(n, _)| n.clone()).collect(),
        copies: files
            .iter()
            .map(|(n, b)| {
                let (a, k, g) = describe(b);
                (n.clone(), a, k, g)
            })
            .collect(),
        differences,
    }
}

/// `local` (the real LocalHost on the folder) or `helper` (the helper program), with the fake API over HTTP on the same folder.
async fn python(helper: bool) -> Option<Left> {
    let root = tempfile::tempdir().unwrap();
    let folder = lay_out(root.path());
    let api_port = free_port();
    let lazy = if helper { "select" } else { "0.5" };
    let (_api_proc, _) = spawn(&[
        tools("fake_propresenter_api.py").display().to_string(),
        "--port".into(),
        api_port.to_string(),
        "--folder".into(),
        folder.display().to_string(),
        "--lazy".into(),
        lazy.into(),
        "--playlist".into(),
        "Sunday".into(),
    ])?;
    let api = HttpApi::at(&format!("http://127.0.0.1:{api_port}"));
    let playlist = api.playlists().await.unwrap()[0].uuid.clone();
    let songs = api
        .libraries()
        .await
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Songs")
        .unwrap();
    let alpha = api
        .library(&songs.uuid)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.name.starts_with("Alpha"))
        .unwrap();
    api.put_playlist(&playlist, &playlist_items(alpha.uuid.as_deref().unwrap()))
        .await
        .unwrap();

    let rt = runtime();
    let mut next = rt.settings_service.snapshot();
    next.integration_modes.timer_output = TimerOutput::Propresenter;
    next.propresenter.host = "127.0.0.1".into();
    next.propresenter.port = api_port;
    let helper_proc;
    if helper {
        let key_file = root.path().join("key.txt");
        std::fs::write(&key_file, format!("{KEY}\n")).unwrap();
        let port = free_port();
        helper_proc = Some(spawn(&[
            tools("fake_propresenter_link.py").display().to_string(),
            "--port".into(),
            port.to_string(),
            "--key-file".into(),
            key_file.display().to_string(),
            "--allow-loopback".into(),
            "--folder".into(),
            folder.display().to_string(),
        ])?);
        rt.settings_service.save(next).unwrap();
        rt.propresenter_link
            .set_own_address_check(Arc::new(|_| false));
        rt.propresenter_link.set_helper_port(port);
        rt.propresenter_link.set_key_for_test(KEY);
    } else {
        helper_proc = None;
        next.propresenter.link.data_folder = Some(folder.display().to_string());
        rt.settings_service.save(next).unwrap();
        rt.propresenter_link
            .set_own_address_check(Arc::new(|_| true));
    }
    let backups = tempfile::tempdir().unwrap();
    rt.propresenter_add_songs
        .set_backup_dir_for_test(backups.path().join("b"));
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    let (preview, job) = run(&app, &playlist).await;
    let (outcome, actions, differences) = seen(&preview, &job);
    let playback = folder.join("Libraries/Playback");
    let mut names: Vec<String> = std::fs::read_dir(&playback)
        .map(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    drop(helper_proc);
    Some(Left {
        outcome,
        actions,
        playlist: playlist_of(&api, &playlist).await,
        copies: names
            .iter()
            .map(|n| {
                let (a, k, g) = describe(&std::fs::read(playback.join(n)).unwrap());
                (n.clone(), a, k, g)
            })
            .collect(),
        playback_files: names,
        differences,
    })
}

fn expected() -> Left {
    let named = |n: &str| n.to_string();
    Left {
        outcome: "done".into(),
        actions: vec!["copy Alpha (1)".into(), "copy Beta (1)".into()],
        playlist: vec![
            (named("Welcome"), named("header"), named("")),
            (named("Alpha"), named("presentation"), named("Full")),
            (named("Worship"), named("header"), named("")),
            (named("Alpha (1)"), named("presentation"), named("Playback")),
            (named("Beta (1)"), named("presentation"), named("Playback")),
            (named("Message"), named("header"), named("")),
        ],
        playback_files: vec!["Alpha (1).pro".into(), "Beta (1).pro".into()],
        copies: vec![
            (
                "Alpha (1).pro".into(),
                "Playback".into(),
                "Eb".into(),
                ["", "Intro", "Verse 1", "Chorus", "Chorus"]
                    .map(String::from)
                    .to_vec(),
            ),
            (
                "Beta (1).pro".into(),
                "Playback".into(),
                "F#m".into(),
                ["Intro", "Chorus", "Verse 1"].map(String::from).to_vec(),
            ),
        ],
        differences: vec![],
    }
}

#[tokio::test]
async fn the_in_memory_computer_adds_the_songs() {
    assert_eq!(in_memory().await, expected());
}

#[tokio::test]
async fn the_computer_that_is_this_one_adds_the_songs_with_the_real_local_host() {
    let Some(left) = python(false).await else {
        eprintln!("skipped: no python3 on this computer");
        return;
    };
    assert_eq!(left, expected());
}

#[tokio::test]
async fn another_computer_with_the_helper_adds_the_same_songs_the_same_way() {
    let Some(left) = python(true).await else {
        eprintln!("skipped: no python3 on this computer");
        return;
    };
    assert_eq!(left, expected());
}

#[tokio::test]
async fn a_song_that_is_not_in_propresenter_is_imported_through_the_helper_then_copied() {
    let root = tempfile::tempdir().unwrap();
    let folder = lay_out(root.path());
    let imports = root.path().join("imports");
    std::fs::create_dir_all(&imports).unwrap();
    std::fs::write(
        imports.join("2002.pro"),
        song_file("Delta", 2002, 0xD4000000),
    )
    .unwrap();
    let api_port = free_port();
    let Some((_api, _)) = spawn(&[
        tools("fake_propresenter_api.py").display().to_string(),
        "--port".into(),
        api_port.to_string(),
        "--folder".into(),
        folder.display().to_string(),
        "--lazy".into(),
        "select".into(),
        "--playlist".into(),
        "Sunday".into(),
    ]) else {
        eprintln!("skipped: no python3 on this computer");
        return;
    };
    let key_file = root.path().join("key.txt");
    std::fs::write(&key_file, format!("{KEY}\n")).unwrap();
    let link_port = free_port();
    let Some((_link, _)) = spawn(&[
        tools("fake_propresenter_link.py").display().to_string(),
        "--port".into(),
        link_port.to_string(),
        "--key-file".into(),
        key_file.display().to_string(),
        "--allow-loopback".into(),
        "--folder".into(),
        folder.display().to_string(),
        "--import-dir".into(),
        imports.display().to_string(),
    ]) else {
        return;
    };
    let api = HttpApi::at(&format!("http://127.0.0.1:{api_port}"));
    let playlist = api.playlists().await.unwrap()[0].uuid.clone();
    api.put_playlist(&playlist, &[FakeApi::header_item("Worship", 0.2)])
        .await
        .unwrap();
    let rt = runtime();
    rt.propresenter_add_songs
        .set_plan_for_test(Some(plan(vec![lib_song(
            21,
            2002,
            1,
            "Delta (feat. Someone)",
            "A",
            &["Intro", "Verse 1", "Chorus"],
        )])));
    let mut next = rt.settings_service.snapshot();
    next.integration_modes.timer_output = TimerOutput::Propresenter;
    next.propresenter.host = "127.0.0.1".into();
    next.propresenter.port = api_port;
    rt.settings_service.save(next).unwrap();
    rt.propresenter_link
        .set_own_address_check(Arc::new(|_| false));
    rt.propresenter_link.set_helper_port(link_port);
    rt.propresenter_link.set_key_for_test(KEY);
    let backups = tempfile::tempdir().unwrap();
    rt.propresenter_add_songs
        .set_backup_dir_for_test(backups.path().join("b"));
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    let (status, preview) = call(
        &app,
        "POST",
        "/api/v1/propresenter-link/add-songs/preview",
        Some(json!({"setlist_song_ids": [21], "playlist_id": playlist, "place": "end"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(
        preview["songs"][0]["action"], "import_then_copy",
        "the fake helper can import: {preview}"
    );
    assert_eq!(preview["songs"][0]["import_as"], "Delta (feat Someone)");
    let (_, started) = call(
        &app,
        "POST",
        "/api/v1/propresenter-link/add-songs/apply",
        Some(json!({"plan_id": preview["plan_id"]})),
    )
    .await;
    let job_id = started["job_id"].as_str().unwrap().to_string();
    let mut job = Value::Null;
    for _ in 0..1000 {
        (_, job) = call(
            &app,
            "GET",
            &format!("/api/v1/propresenter-link/add-songs/jobs/{job_id}"),
            None,
        )
        .await;
        if job["state"] != "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(job["report"]["outcome"], "done", "{job}");
    assert!(
        folder
            .join("Libraries/Songs/Delta (feat Someone).pro")
            .exists(),
        "the import wrote the song into Songs"
    );
    assert!(folder
        .join("Libraries/Playback/Delta (feat Someone) (1).pro")
        .exists());
    let items = playlist_of(&api, &playlist).await;
    assert_eq!(
        items.last().unwrap(),
        &(
            "Delta (feat Someone) (1)".to_string(),
            "presentation".to_string(),
            "Playback".to_string()
        )
    );
    assert_eq!(items.len(), 2);
}
