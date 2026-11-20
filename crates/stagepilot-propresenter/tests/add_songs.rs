//! The add-songs engine against an in-memory ProPresenter computer (files) and an in-memory ProPresenter API: preview, apply,
//! the waits, the playlist write and the read-back.

use serde_json::{json, Value};
use stagepilot_propresenter::link::add_songs::{
    AddSongs, ApplyOutcome, ApplyRefusal, ChosenBy, Event, Guard, Place, PlanRequest, Progress,
    SongAction, SongStatus, Timing, TitleFound, WantedSong,
};
use stagepilot_propresenter::link::api::fake::{FakeApi, Loading};
use stagepilot_propresenter::link::api::{ApiError, Layers, ProPresenterApi};
use stagepilot_propresenter::link::fake::FakeHost;
use stagepilot_propresenter::link::{
    HostInfo, IndexEntry, LibraryFile, LinkError, ProPresenterHost, SelectOutcome,
};
use stagepilot_propresenter::pro_file::synthetic::SyntheticSong;
use stagepilot_propresenter::pro_file::{KeyScale, Song};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

const NOW: i64 = 1_800_000_000;

fn alpha() -> Vec<u8> {
    // groups: 0 blank, 1 Intro, 2 Verse 1, 3 Chorus, 4 Bridge
    SyntheticSong::new("Alpha")
        .id(1001)
        .seed(0xA1000000)
        .group("", &["count"])
        .group("Intro", &["intro words"])
        .group("Verse 1", &["verse one words"])
        .group("Chorus", &["chorus words"])
        .group("Bridge", &["bridge words"])
        .arrangement("Full", &[0, 1, 2, 3, 2, 3, 4, 3])
        .selected(0)
        .key(
            KeyScale {
                key: 7,
                minor: false,
            },
            "C",
        )
        .build()
}

fn song_numbered(title: &str, id: u64, seed: u32) -> Vec<u8> {
    SyntheticSong::new(title)
        .id(id)
        .seed(seed)
        .group("Intro", &["i"])
        .group("Verse 1", &["v"])
        .group("Chorus", &["c"])
        .arrangement("Full", &[0, 1, 2])
        .selected(0)
        .build()
}

fn sections(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_string()).collect()
}

fn wanted(ref_id: &str, title: &str, position: u32, id: Option<u64>, names: &[&str]) -> WantedSong {
    WantedSong {
        ref_id: ref_id.to_string(),
        title: title.to_string(),
        position,
        content_id: id,
        key: Some("Eb".to_string()),
        sections: sections(names),
        rental_expired: false,
        as_is: false,
    }
}

/// A song named by its title alone: the one entry of the Songs library with that title, as it is.
fn titled(ref_id: &str, title: &str, position: u32) -> WantedSong {
    WantedSong {
        ref_id: ref_id.to_string(),
        title: title.to_string(),
        position,
        content_id: None,
        key: None,
        sections: Vec::new(),
        rental_expired: false,
        as_is: true,
    }
}

#[derive(Default)]
struct Log(Mutex<Vec<Event>>);

impl Progress for Log {
    fn event(&self, event: Event) {
        self.0.lock().unwrap().push(event);
    }
}

impl Log {
    fn texts(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.text.clone())
            .collect()
    }
}

struct Quiet;

impl Guard for Quiet {
    fn check(&self) -> Result<(), String> {
        Ok(())
    }
}

struct Playing(AtomicBool);

impl Guard for Playing {
    fn check(&self) -> Result<(), String> {
        if self.0.load(Ordering::SeqCst) {
            Err("A song is playing in StagePilot.".to_string())
        } else {
            Ok(())
        }
    }
}

/// A guard that lets this many checks through and then says a song is playing.
struct AfterChecks(AtomicU32);

impl Guard for AfterChecks {
    fn check(&self) -> Result<(), String> {
        if self.0.load(Ordering::SeqCst) == 0 {
            return Err("A song is playing in StagePilot.".to_string());
        }
        self.0.fetch_sub(1, Ordering::SeqCst);
        Ok(())
    }
}

struct Rig {
    host: Arc<FakeHost>,
    api: Arc<FakeApi>,
    engine: AddSongs,
    playlist: String,
    backups: tempfile::TempDir,
}

fn counter() -> Arc<dyn Fn() -> String + Send + Sync> {
    let n = Arc::new(AtomicU32::new(0));
    Arc::new(move || {
        let i = n.fetch_add(1, Ordering::SeqCst) + 1;
        format!("DDDDDDDD-0000-4000-8000-{i:012X}")
    })
}

/// A ProPresenter with the songs Alpha (1001) and Beta (1002) in Songs, a Sunday playlist and nothing else.
fn standard_host() -> Arc<FakeHost> {
    Arc::new(
        FakeHost::new()
            .with_file("Songs", "Alpha.pro", &alpha())
            .with_file(
                "Songs",
                "Beta.pro",
                &song_numbered("Beta", 1002, 0xB2000000),
            ),
    )
}

fn rig(loading: Loading) -> Rig {
    rig_from(standard_host(), loading)
}

fn rig_from(host: Arc<FakeHost>, loading: Loading) -> Rig {
    let api = Arc::new(FakeApi::new(host.clone(), loading));
    let alpha_uuid = stagepilot_propresenter::link::api::fake::entry_uuid("Songs", "Alpha.pro");
    let playlist = api.add_playlist(
        "Sunday",
        None,
        vec![
            FakeApi::header_item("Welcome", 0.1),
            FakeApi::presentation_item(&alpha_uuid, "Alpha", "Default", 12.5),
            FakeApi::header_item("Worship", 0.2),
            FakeApi::presentation_item(&alpha_uuid, "Alpha", "Default", 80.0),
            FakeApi::header_item("Message", 0.3),
            json!({"id": {"uuid": "", "name": "Sermon placeholder", "index": 0}, "type": "placeholder", "is_hidden": true, "is_pco": false}),
        ],
    );
    let backups = tempfile::tempdir().unwrap();
    let engine = AddSongs::new(
        host.clone(),
        api.clone(),
        backups.path().join("propresenter-playlist-backups"),
        Arc::new(|| NOW),
        counter(),
    )
    .with_timing(Timing::quick());
    Rig {
        host,
        api,
        engine,
        playlist,
        backups,
    }
}

fn request(rig: &Rig, songs: Vec<WantedSong>, place: Place) -> PlanRequest {
    PlanRequest {
        songs,
        playlist_id: rig.playlist.clone(),
        place,
        sources: Default::default(),
    }
}

fn worship() -> Place {
    Place::AfterHeader("Worship".into())
}

fn names_of(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .map(|i| i["id"]["name"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[tokio::test]
async fn a_copy_is_planned_for_a_song_that_is_in_songs_and_nothing_is_written() {
    let rig = rig(Loading::Immediately);
    let req = request(
        &rig,
        vec![wanted(
            "11",
            "Alpha",
            1,
            Some(1001),
            &["Count Off", "Intro", "Verse 1", "Chorus", "Chorus"],
        )],
        worship(),
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    let song = &plan.songs[0];
    assert_eq!(song.action, SongAction::Copy);
    assert_eq!(song.file.as_deref(), Some("Alpha (1)"));
    assert_eq!(song.source.as_deref(), Some("Songs / Alpha.pro"));
    assert_eq!(song.sections, 5);
    assert!(
        song.notes.iter().any(|n| n.contains("The key Eb is saved")),
        "{:?}",
        song.notes
    );
    assert_eq!(plan.playlist.name, "Sunday");
    assert_eq!(
        plan.playlist.insert_at, 4,
        "the end of the Worship section, before the header Message"
    );
    assert_eq!(plan.playlist.before.as_deref(), Some("Alpha"));
    assert_eq!(plan.playlist.after.as_deref(), Some("Message"));
    assert_eq!(plan.playlist.untouched, 6);
    assert!(plan.can_apply);
    assert_eq!(
        plan.propresenter.as_deref(),
        Some("ProPresenter 19.0.1 (API v1)")
    );
    // A preview writes nothing: no file made, no import, no screen, no playlist write.
    let calls = rig.host.calls().join("\n");
    for forbidden in [
        "create_file",
        "search_import",
        "ensure_library",
        "select_library",
    ] {
        assert!(!calls.contains(forbidden), "{forbidden}: {calls}");
    }
    assert!(rig.api.puts().is_empty());
    assert!(rig.host.files_of("Playback").is_empty());
    assert!(!rig
        .backups
        .path()
        .join("propresenter-playlist-backups")
        .exists());
    // The same question gives the same plan.
    assert_eq!(
        rig.engine.preview(&req).await.unwrap().plan_id,
        plan.plan_id
    );
}

#[tokio::test]
async fn the_copy_number_is_the_lowest_unused_one_across_every_library() {
    let other = |id: u64| song_numbered("Other", id, id as u32);
    let host = Arc::new(
        FakeHost::new()
            .with_file("Songs", "Alpha.pro", &alpha())
            .with_file("Songs", "alpha (2).pro", &other(5))
            .with_file("Playback", "Alpha (1).pro", &other(6))
            .with_file("Playback", "Alpha (3).pro", &other(7)),
    );
    let rig = rig_from(host, Loading::Immediately);
    let ask = |rig: &Rig| {
        request(
            rig,
            vec![wanted("1", "Alpha", 1, Some(1001), &["Intro", "Verse 1"])],
            Place::End,
        )
    };
    let plan = rig.engine.preview(&ask(&rig)).await.unwrap();
    assert_eq!(
        plan.songs[0].file.as_deref(),
        Some("Alpha (4)"),
        "(1), (2) and (3) are taken, in two libraries"
    );
    let host = Arc::new(
        FakeHost::new()
            .with_file("Songs", "Alpha.pro", &alpha())
            .with_file("Playback", "Alpha (1).pro", &other(6))
            .with_file("Playback", "Alpha (3).pro", &other(7)),
    );
    let rig = rig_from(host, Loading::Immediately);
    let plan = rig.engine.preview(&ask(&rig)).await.unwrap();
    assert_eq!(plan.songs[0].file.as_deref(), Some("Alpha (2)"));
    // Two songs of one plan never get the same number.
    let two = request(
        &rig,
        vec![
            wanted("1", "Alpha", 1, Some(1001), &["Intro", "Verse 1"]),
            wanted("2", "Alpha", 2, Some(1001), &["Intro"]),
        ],
        Place::End,
    );
    let plan = rig.engine.preview(&two).await.unwrap();
    assert_eq!(plan.songs[0].file.as_deref(), Some("Alpha (2)"));
    assert_eq!(plan.songs[1].file.as_deref(), Some("Alpha (4)"));
}

#[tokio::test]
async fn every_reason_a_song_is_blocked_is_said_in_plain_words() {
    let rig = rig(Loading::Immediately);
    rig.host
        .set_import(false, Some("Allow Accessibility for StagePilot."));
    let mut expired = wanted("3", "Rented", 3, Some(1001), &["Intro"]);
    expired.rental_expired = true;
    let songs = vec![
        wanted("1", "No Id", 1, None, &["Intro"]),
        wanted(
            "2",
            "Sections",
            2,
            Some(1001),
            &["Intro", "Tag", "Instrumental"],
        ),
        expired,
        wanted("4", "Nothing", 4, Some(1001), &[]),
        wanted("5", "Missing Song", 5, Some(999), &["Intro"]),
        wanted("6", "Fine", 6, Some(1002), &["Intro", "Chorus"]),
    ];
    let plan = rig
        .engine
        .preview(&request(&rig, songs, Place::End))
        .await
        .unwrap();
    let reason = |i: usize| plan.songs[i].reason.clone().unwrap_or_default();
    assert_eq!(plan.songs[0].action, SongAction::Blocked);
    assert!(reason(0).contains("MultiTracks number"), "{}", reason(0));
    assert_eq!(plan.songs[1].action, SongAction::Blocked);
    assert!(
        reason(1).contains("Tag (no group has that name)"),
        "{}",
        reason(1)
    );
    assert!(reason(1).contains("Instrumental"));
    assert!(reason(2).contains("rental"), "{}", reason(2));
    assert!(reason(3).contains("no sections"), "{}", reason(3));
    assert_eq!(plan.songs[4].action, SongAction::Blocked);
    assert!(
        reason(4).contains("cannot be imported from here"),
        "{}",
        reason(4)
    );
    assert!(reason(4).contains("Allow Accessibility"));
    assert_eq!(plan.songs[5].action, SongAction::Copy);
    assert_eq!(plan.stuck, 5);
    assert_eq!(plan.to_add, 1);
    assert!(plan.can_apply);
    assert!(!plan.can_import);
    // The songs come in the setlist order whatever the order they were given in.
    let positions: Vec<u32> = plan.songs.iter().map(|s| s.position).collect();
    assert_eq!(positions, [1, 2, 3, 4, 5, 6]);
}

#[tokio::test]
async fn a_song_not_in_propresenter_is_imported_then_copied_when_it_can_be() {
    let rig = rig(Loading::Immediately);
    let plan = rig
        .engine
        .preview(&request(
            &rig,
            vec![wanted(
                "1",
                "New Song (feat. Someone)",
                1,
                Some(2002),
                &["Intro", "Verse 1"],
            )],
            Place::End,
        ))
        .await
        .unwrap();
    let song = &plan.songs[0];
    assert_eq!(song.action, SongAction::ImportThenCopy);
    assert_eq!(
        song.import_as.as_deref(),
        Some("New Song (feat Someone)"),
        "the dots are left out"
    );
    assert_eq!(song.file.as_deref(), Some("New Song (feat Someone) (1)"));
    assert!(plan.can_import);
    // A title that another song already has is imported under another name.
    let plan = rig
        .engine
        .preview(&request(
            &rig,
            vec![wanted("1", "Beta", 1, Some(2003), &["Intro"])],
            Place::End,
        ))
        .await
        .unwrap();
    assert_eq!(
        plan.songs[0].import_as.as_deref(),
        Some("Beta (MultiTracks)")
    );
    assert_eq!(
        plan.songs[0].file.as_deref(),
        Some("Beta (MultiTracks) (1)")
    );
}

#[tokio::test]
async fn several_files_for_one_song_take_the_one_that_works_or_ask() {
    let host = Arc::new(
        FakeHost::new()
            .with_file("Songs", "Gamma one.pro", &song_numbered("Gamma", 77, 1))
            .with_file("Songs", "Gamma copy.pro", &song_numbered("Gamma", 77, 2))
            // The third one has no Full arrangement and a name that does not match.
            .with_file(
                "Songs",
                "Gamma odd.pro",
                &SyntheticSong::new("Gamma")
                    .id(77)
                    .seed(3)
                    .group("Zzz", &["z"])
                    .build(),
            ),
    );
    let api = Arc::new(FakeApi::new(host.clone(), Loading::Immediately));
    let playlist = api.add_playlist("P", None, vec![]);
    let engine = AddSongs::new(
        host.clone(),
        api,
        tempfile::tempdir().unwrap().keep(),
        Arc::new(|| NOW),
        counter(),
    );
    let ask = |names: &[&str]| PlanRequest {
        songs: vec![wanted("1", "Gamma", 1, Some(77), names)],
        playlist_id: playlist.clone(),
        place: Place::End,
        sources: Default::default(),
    };
    let plan = engine.preview(&ask(&["Intro", "Chorus"])).await.unwrap();
    assert_eq!(
        plan.songs[0].action,
        SongAction::NeedsChoice,
        "two files work: never guessed"
    );
    assert_eq!(plan.songs[0].choices.len(), 2);
    assert!(plan.songs[0].choices[0].starts_with("Songs / Gamma"));
    assert!(plan.songs[0].action.is_stuck());
    assert_eq!(plan.stuck, 1);
    assert!(!plan.can_apply);
    // Only one of them works for these sections: that one.
    let host2 = Arc::new(
        FakeHost::new()
            .with_file("Songs", "Gamma.pro", &song_numbered("Gamma", 77, 1))
            .with_file(
                "Songs",
                "Gamma odd.pro",
                &SyntheticSong::new("Gamma")
                    .id(77)
                    .seed(3)
                    .group("Intro", &["z"])
                    .build(),
            ),
    );
    let api2 = Arc::new(FakeApi::new(host2.clone(), Loading::Immediately));
    let playlist2 = api2.add_playlist("P", None, vec![]);
    let engine2 = AddSongs::new(
        host2,
        api2,
        tempfile::tempdir().unwrap().keep(),
        Arc::new(|| NOW),
        counter(),
    );
    let plan = engine2
        .preview(&PlanRequest {
            songs: vec![wanted("1", "Gamma", 1, Some(77), &["Intro", "Chorus"])],
            playlist_id: playlist2,
            place: Place::End,
            sources: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(plan.songs[0].action, SongAction::Copy);
    assert_eq!(plan.songs[0].source.as_deref(), Some("Songs / Gamma.pro"));
}

#[tokio::test]
async fn non_exact_matches_get_a_line_and_exact_ones_do_not() {
    let rig = rig(Loading::Immediately);
    // "Bridge" then "Intro": nothing lines up with Full there, the names are unique.
    let plan = rig
        .engine
        .preview(&request(
            &rig,
            vec![wanted("1", "Alpha", 1, Some(1001), &["Bridge", "Intro"])],
            Place::End,
        ))
        .await
        .unwrap();
    let song = &plan.songs[0];
    assert_eq!(song.action, SongAction::Copy);
    let lines: Vec<&str> = song
        .mapping
        .iter()
        .filter_map(|m| m.line.as_deref())
        .collect();
    assert!(!lines.is_empty());
    assert!(
        lines
            .iter()
            .any(|l| l.contains("\"Intro\"") && l.contains("the only group")),
        "{lines:?}"
    );
    let exact = rig
        .engine
        .preview(&request(
            &rig,
            vec![wanted(
                "1",
                "Alpha",
                1,
                Some(1001),
                &["Intro", "Verse 1", "Chorus", "Verse 1", "Chorus"],
            )],
            Place::End,
        ))
        .await
        .unwrap();
    assert!(exact.songs[0]
        .mapping
        .iter()
        .all(|m| m.how == "full" && m.line.is_none()));
}

#[tokio::test]
async fn a_missing_header_or_playlist_is_said_and_blocks_applying() {
    let rig = rig(Loading::Immediately);
    let songs = vec![wanted("1", "Alpha", 1, Some(1001), &["Intro"])];
    let plan = rig
        .engine
        .preview(&request(
            &rig,
            songs.clone(),
            Place::AfterHeader("Prayer".into()),
        ))
        .await
        .unwrap();
    assert!(plan
        .playlist
        .blocked
        .as_deref()
        .unwrap()
        .contains("no header named \"Prayer\""));
    assert!(!plan.can_apply);
    let mut missing = request(&rig, songs.clone(), Place::End);
    missing.playlist_id = "00000000-0000-4000-8000-0000000000FF".into();
    assert!(rig.engine.preview(&missing).await.is_err());
    let empty = request(&rig, vec![], Place::End);
    assert!(rig.engine.preview(&empty).await.is_err());
    // And apply refuses it before anything happens.
    let log = Log::default();
    let refused = rig
        .engine
        .apply(
            &request(&rig, songs, Place::AfterHeader("Prayer".into())),
            &plan.plan_id,
            &[],
            &Quiet,
            &log,
        )
        .await;
    assert!(
        matches!(refused, Err(ApplyRefusal::PlaylistBlocked(_))),
        "{refused:?}"
    );
    assert!(rig.api.puts().is_empty());
}

/// An import writes this song (made-up) under the name it is given.
fn import_returns(rig: &Rig, id: u64, title: &str, seed: u32) {
    rig.host
        .set_import_song(id, &song_numbered(title, id, seed));
}

#[tokio::test]
async fn apply_imports_copies_and_writes_the_playlist_once_keeping_every_other_item_exactly() {
    let rig = rig(Loading::AfterReads(1));
    import_returns(&rig, 2002, "Delta", 0xD4000000);
    let songs = vec![
        wanted(
            "11",
            "Alpha",
            1,
            Some(1001),
            &["Count Off", "Intro", "Verse 1", "Chorus", "Chorus"],
        ),
        wanted("12", "Delta", 2, Some(2002), &["Intro", "Chorus"]),
        wanted("13", "Beta", 3, Some(1002), &["Chorus", "Intro"]),
    ];
    let req = request(&rig, songs, worship());
    let plan = rig.engine.preview(&req).await.unwrap();
    assert_eq!(plan.songs[1].action, SongAction::ImportThenCopy);
    let before: Vec<Value> = rig.api.stored(&rig.playlist);
    let log = Log::default();
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &log)
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    assert!(report.differences.is_empty(), "{:?}", report.differences);
    assert!(report.playlist_changed);
    assert_eq!(
        report.version.as_deref(),
        Some("ProPresenter 19.0.1 (API v1)")
    );
    // The files: imported into Songs by the import only, the copies in Playback, in order, numbered.
    assert_eq!(
        rig.host.imports(),
        [("2002".to_string(), "Delta".to_string(), None)],
        "the playlist is StagePilot's to write"
    );
    let files: Vec<String> = rig
        .host
        .files_of("Playback")
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(files, ["Alpha (1).pro", "Beta (1).pro", "Delta (1).pro"]);
    assert!(
        rig.host.file("Songs", "Alpha.pro").unwrap() == alpha(),
        "a song in Songs is never edited"
    );
    let copy = Song::read(&rig.host.file("Playback", "Alpha (1).pro").unwrap()).unwrap();
    assert_eq!(copy.selected_arrangement().unwrap().name, "Playback");
    assert_eq!(
        copy.arrangements.len(),
        2,
        "the original arrangement is kept"
    );
    assert_eq!(copy.music.user_text, "Eb");
    assert_eq!(copy.multitracks_id, Some(1001));
    // ONE playlist write.
    assert_eq!(rig.api.puts().len(), 1);
    let after = rig.api.stored(&rig.playlist);
    assert_eq!(after.len(), before.len() + 3);
    assert_eq!(
        names_of(&after),
        [
            "Welcome",
            "Alpha",
            "Worship",
            "Alpha",
            "Alpha (1)",
            "Delta (1)",
            "Beta (1)",
            "Message",
            "Sermon placeholder"
        ]
    );
    // Every old item is exactly as it was read (only the index was renumbered).
    let renumber = |items: &[Value]| -> Vec<Value> {
        items
            .iter()
            .map(|i| {
                let mut i = i.clone();
                i["id"]["index"] = json!(0);
                i["presentation_info"]["arrangement_name"] = json!("");
                i
            })
            .collect()
    };
    let old_after: Vec<Value> = after[..4].iter().chain(&after[7..]).cloned().collect();
    assert_eq!(renumber(&old_after), renumber(&before));
    assert_eq!(old_after[1]["duration"], json!(12.5));
    assert_eq!(old_after[3]["duration"], json!(80.0));
    assert_eq!(old_after[2]["header_color"]["red"], json!(0.2));
    assert_eq!(old_after[5]["is_hidden"], json!(true));
    for (i, item) in after.iter().enumerate() {
        assert_eq!(item["id"]["index"], json!(i), "indexes are renumbered");
    }
    // The new items: library entry uuid twice, display name without the suffix, the arrangement.
    let item = &after[4];
    assert_eq!(item["type"], "presentation");
    assert_eq!(
        item["id"]["uuid"],
        item["presentation_info"]["presentation_uuid"]
    );
    assert_eq!(item["presentation_info"]["arrangement_name"], "Playback");
    // The backup holds the playlist as it was before.
    let backup = report.backup.clone().unwrap();
    assert!(
        backup
            .replace('\\', "/")
            .contains("propresenter-playlist-backups/Sunday-20270115-"),
        "{backup}"
    );
    let saved: Value = serde_json::from_slice(&std::fs::read(&backup).unwrap()).unwrap();
    assert_eq!(saved["items"].as_array().unwrap().len(), before.len());
    // Each step was reported.
    let texts = log.texts().join("\n");
    for expect in [
        "ProPresenter says",
        "Saved the playlist",
        "Importing the song",
        "Made \"Alpha (1)\"",
        "ProPresenter has loaded",
        "Wrote the playlist",
        "reads back as planned",
    ] {
        assert!(texts.contains(expect), "{expect}\n{texts}");
    }
    // Asking again: everything is reused, nothing new is made and nothing is added twice.
    let plan2 = rig.engine.preview(&req).await.unwrap();
    assert!(
        plan2.songs.iter().all(|s| s.action == SongAction::Reuse),
        "{:?}",
        plan2.songs.iter().map(|s| s.action).collect::<Vec<_>>()
    );
    assert!(plan2.songs.iter().all(|s| s.already_in_playlist));
    assert_eq!(plan2.to_add, 0);
    assert_eq!(
        rig.engine
            .apply(&req, &plan2.plan_id, &[], &Quiet, &log)
            .await,
        Err(ApplyRefusal::NothingToAdd)
    );
    assert_eq!(rig.api.puts().len(), 1);
}

#[tokio::test]
async fn a_changed_song_in_playback_makes_a_new_numbered_file_and_a_same_one_is_reused() {
    let rig = rig(Loading::Immediately);
    let first = request(
        &rig,
        vec![wanted(
            "1",
            "Alpha",
            1,
            Some(1001),
            &["Intro", "Verse 1", "Chorus"],
        )],
        Place::End,
    );
    let plan = rig.engine.preview(&first).await.unwrap();
    let report = rig
        .engine
        .apply(&first, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    // Same sections and key: reused.
    let again = rig.engine.preview(&first).await.unwrap();
    assert_eq!(again.songs[0].action, SongAction::Reuse);
    assert_eq!(again.songs[0].file.as_deref(), Some("Alpha (1)"));
    // Another key: a new copy, never a replaced file.
    let mut other_key = first.clone();
    other_key.songs[0].key = Some("F#".into());
    let plan = rig.engine.preview(&other_key).await.unwrap();
    assert_eq!(plan.songs[0].action, SongAction::Copy);
    assert_eq!(plan.songs[0].file.as_deref(), Some("Alpha (2)"));
    assert_eq!(
        plan.songs[0].source.as_deref(),
        Some("Songs / Alpha.pro"),
        "built from the song, not from the earlier copy"
    );
    // Other sections: also new.
    let mut other_sections = first.clone();
    other_sections.songs[0].sections = sections(&["Intro", "Chorus", "Chorus"]);
    let plan = rig.engine.preview(&other_sections).await.unwrap();
    assert_eq!(plan.songs[0].file.as_deref(), Some("Alpha (2)"));
    let original = rig.host.file("Playback", "Alpha (1).pro").unwrap();
    let report = rig
        .engine
        .apply(&other_sections, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    assert_eq!(
        rig.host.file("Playback", "Alpha (1).pro").unwrap(),
        original,
        "an earlier copy is never touched"
    );
    assert!(rig.host.file("Playback", "Alpha (2).pro").is_some());
}

#[tokio::test]
async fn a_song_from_the_playback_library_alone_is_copied_from_its_newest_copy() {
    let host = Arc::new(FakeHost::new().with_library("Playback").with_file(
        "Playback",
        "Solo (4).pro",
        &song_numbered("Solo", 55, 9),
    ));
    let api = Arc::new(FakeApi::new(host.clone(), Loading::Immediately));
    let playlist = api.add_playlist("P", None, vec![]);
    let engine = AddSongs::new(
        host,
        api,
        tempfile::tempdir().unwrap().keep(),
        Arc::new(|| NOW),
        counter(),
    );
    let plan = engine
        .preview(&PlanRequest {
            songs: vec![wanted("1", "Solo", 1, Some(55), &["Intro", "Chorus"])],
            playlist_id: playlist,
            place: Place::End,
            sources: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(plan.songs[0].action, SongAction::Copy);
    assert_eq!(
        plan.songs[0].file.as_deref(),
        Some("Solo (1)"),
        "numbered from the song, not from the copy's number"
    );
    assert_eq!(
        plan.songs[0].source.as_deref(),
        Some("Playback / Solo (4).pro")
    );
}

#[tokio::test]
async fn apply_needs_the_plan_to_still_be_true_and_every_stuck_song_left_out() {
    let rig = rig(Loading::Immediately);
    let songs = vec![
        wanted("1", "Alpha", 1, Some(1001), &["Intro"]),
        wanted("2", "Broken", 2, None, &["Intro"]),
    ];
    let req = request(&rig, songs, Place::End);
    let plan = rig.engine.preview(&req).await.unwrap();
    let log = Log::default();
    assert_eq!(
        rig.engine
            .apply(&req, &plan.plan_id, &[], &Quiet, &log)
            .await,
        Err(ApplyRefusal::StuckNotSkipped(vec!["2".into()]))
    );
    assert_eq!(
        rig.engine
            .apply(
                &req,
                &plan.plan_id,
                &["2".into(), "99".into()],
                &Quiet,
                &log
            )
            .await,
        Err(ApplyRefusal::UnknownSkip(vec!["99".into()]))
    );
    assert_eq!(
        rig.engine
            .apply(&req, "0000", &["2".into()], &Quiet, &log)
            .await,
        Err(ApplyRefusal::PlanChanged)
    );
    assert!(
        rig.host
            .calls()
            .iter()
            .all(|c| !c.starts_with("create_file")),
        "nothing was made"
    );
    assert!(rig.api.puts().is_empty());
    // A change in the playlist after the preview makes the plan stale.
    let uuid = stagepilot_propresenter::link::api::fake::entry_uuid("Songs", "Beta.pro");
    let mut items = rig.api.stored(&rig.playlist);
    items.push(FakeApi::presentation_item(&uuid, "Beta", "", 1.0));
    // Someone adds a song in ProPresenter itself.
    rig.api.put_playlist(&rig.playlist, &items).await.unwrap();
    assert_eq!(
        rig.engine
            .apply(&req, &plan.plan_id, &["2".into()], &Quiet, &log)
            .await,
        Err(ApplyRefusal::PlanChanged)
    );
    // The fresh preview works, with the stuck song left out.
    let plan = rig.engine.preview(&req).await.unwrap();
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &["2".into()], &Quiet, &log)
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    let skipped = report.songs.iter().find(|s| s.ref_id == "2").unwrap();
    assert_eq!(skipped.status, SongStatus::Skipped);
    assert_eq!(report.songs[0].status, SongStatus::Added);
}

#[tokio::test]
async fn a_song_that_is_playing_stops_the_apply_before_and_during() {
    let rig = rig(Loading::Immediately);
    let req = request(
        &rig,
        vec![
            wanted("1", "Alpha", 1, Some(1001), &["Intro"]),
            wanted("2", "Beta", 2, Some(1002), &["Intro"]),
        ],
        Place::End,
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    let log = Log::default();
    let playing = Playing(AtomicBool::new(true));
    assert_eq!(
        rig.engine
            .apply(&req, &plan.plan_id, &[], &playing, &log)
            .await,
        Err(ApplyRefusal::SongPlaying(
            "A song is playing in StagePilot.".into()
        ))
    );
    assert!(rig
        .host
        .calls()
        .iter()
        .all(|c| !c.starts_with("create_file") && !c.starts_with("ensure_library")));
    assert!(
        !rig.backups
            .path()
            .join("propresenter-playlist-backups")
            .exists(),
        "not even a backup"
    );
    // It starts to play after the first song: nothing more is done and the playlist is not written.
    let report = rig
        .engine
        .apply(
            &req,
            &plan.plan_id,
            &[],
            &AfterChecks(AtomicU32::new(2)),
            &log,
        )
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::SongPlaying, "{report:#?}");
    assert!(!report.playlist_changed);
    assert!(rig.api.puts().is_empty());
    assert_eq!(
        rig.host.files_of("Playback").len(),
        1,
        "the first copy is kept, the second was never made"
    );
}

#[tokio::test]
async fn a_wrong_song_from_the_import_stops_that_song_and_the_playlist_stays() {
    let rig = rig(Loading::Immediately);
    // The import brings a different song (number 31337) than the one asked for.
    import_returns(&rig, 2002, "Delta", 5);
    rig.host
        .set_import_song(2002, &song_numbered("Delta", 31337, 5));
    let req = request(
        &rig,
        vec![
            wanted("1", "Alpha", 1, Some(1001), &["Intro"]),
            wanted("2", "Delta", 2, Some(2002), &["Intro"]),
            wanted("3", "Beta", 3, Some(1002), &["Intro"]),
        ],
        Place::End,
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Stopped, "{report:#?}");
    let delta = &report.songs[1];
    assert_eq!(delta.status, SongStatus::Failed);
    assert_eq!(delta.code.as_deref(), Some("wrong_song"));
    assert_eq!(report.songs[2].status, SongStatus::NotTried);
    assert!(rig.api.puts().is_empty());
    assert!(report.backup.is_some());
}

#[tokio::test]
async fn sections_that_do_not_match_after_an_import_fail_that_song_in_plain_words() {
    let rig = rig(Loading::Immediately);
    rig.host
        .set_import_song(2002, &song_numbered("Delta", 2002, 5));
    let req = request(
        &rig,
        vec![wanted("1", "Delta", 1, Some(2002), &["Intro", "Tag"])],
        Place::End,
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Stopped);
    assert_eq!(report.songs[0].code.as_deref(), Some("unmapped"));
    assert!(
        report.songs[0]
            .text
            .contains("Tag (no group has that name)"),
        "{}",
        report.songs[0].text
    );
    assert!(rig.api.puts().is_empty());
}

#[tokio::test]
async fn the_library_is_selected_once_and_a_song_that_never_loads_asks_to_open_it_then_retries_safely(
) {
    let rig = rig(Loading::AfterReads(10_000));
    let req = request(
        &rig,
        vec![wanted("1", "Alpha", 1, Some(1001), &["Intro", "Verse 1"])],
        Place::End,
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    let log = Log::default();
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &log)
        .await
        .unwrap();
    assert_eq!(
        report.outcome,
        ApplyOutcome::NeedsLibraryOpen,
        "{report:#?}"
    );
    assert!(report
        .message
        .contains("Open the Playback library in ProPresenter once, then press Try again"));
    assert_eq!(report.songs[0].status, SongStatus::NotLoaded);
    assert_eq!(report.songs[0].code.as_deref(), Some("needs_library_open"));
    assert_eq!(
        rig.host.selected(),
        ["Playback"],
        "selected once, not on every poll"
    );
    assert!(rig.api.puts().is_empty(), "the playlist was not written");
    assert_eq!(rig.host.files_of("Playback").len(), 1);
    // The person opens the library: ProPresenter loads the file. Try again = preview and apply again.
    rig.api.set_loading(Loading::Immediately);
    let plan2 = rig.engine.preview(&req).await.unwrap();
    assert_eq!(
        plan2.songs[0].action,
        SongAction::Reuse,
        "the file made before is used, not made again"
    );
    assert_eq!(plan2.songs[0].file.as_deref(), Some("Alpha (1)"));
    let report = rig
        .engine
        .apply(&req, &plan2.plan_id, &[], &Quiet, &log)
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    assert_eq!(rig.host.files_of("Playback").len(), 1, "still one file");
    assert_eq!(rig.api.puts().len(), 1);
    // Trying again once more changes nothing.
    let plan3 = rig.engine.preview(&req).await.unwrap();
    assert_eq!(
        rig.engine
            .apply(&req, &plan3.plan_id, &[], &Quiet, &log)
            .await,
        Err(ApplyRefusal::NothingToAdd)
    );
    assert_eq!(rig.api.puts().len(), 1);
}

#[tokio::test]
async fn a_file_that_loads_only_after_the_library_is_selected_is_waited_for() {
    let rig = rig(Loading::AfterSelect);
    let req = request(
        &rig,
        vec![wanted("1", "Alpha", 1, Some(1001), &["Intro"])],
        Place::End,
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    let log = Log::default();
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &log)
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    assert_eq!(rig.host.selected(), ["Playback"]);
    assert!(log
        .texts()
        .iter()
        .any(|t| t.contains("Selected the Playback library on screen")));
}

#[tokio::test]
async fn a_refused_playlist_write_changes_nothing_and_keeps_the_backup() {
    let rig = rig(Loading::Immediately);
    rig.api.fail_put(Some(ApiError::NotFound));
    let req = request(
        &rig,
        vec![wanted("1", "Alpha", 1, Some(1001), &["Intro"])],
        worship(),
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    let before = rig.api.stored(&rig.playlist);
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::PlaylistRefused, "{report:#?}");
    assert!(!report.playlist_changed);
    assert!(report
        .message
        .contains("nothing in the playlist was changed"));
    let backup = report.backup.clone().unwrap();
    assert!(
        report.message.contains(&backup),
        "the backup path is in the answer"
    );
    assert!(std::path::Path::new(&backup).exists());
    assert_eq!(rig.api.stored(&rig.playlist), before);
    assert_eq!(report.songs[0].status, SongStatus::NotLoaded);
    // Another failure is its own outcome.
    rig.api.fail_put(Some(ApiError::Status(500)));
    let plan = rig.engine.preview(&req).await.unwrap();
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::PlaylistFailed);
}

#[tokio::test]
async fn a_read_back_that_differs_is_reported_item_by_item() {
    let rig = rig(Loading::Immediately);
    let req = request(
        &rig,
        vec![wanted("1", "Alpha", 1, Some(1001), &["Intro"])],
        worship(),
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    // ProPresenter resets a duration, renames an item and drops the arrangement of the new song.
    rig.api
        .corrupt_reads(Some(Arc::new(|items: &mut Vec<Value>| {
            items[3]["duration"] = json!(0.0);
            items[0]["id"]["name"] = json!("Welcome!");
            items[4]["presentation_info"]["arrangement_name"] = json!("Default");
        })));
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(
        report.outcome,
        ApplyOutcome::DoneWithDifferences,
        "{report:#?}"
    );
    assert!(report.playlist_changed);
    let all = report.differences.join("\n");
    assert!(
        all.contains("Item 4 \"Alpha\": its duration reads 0 now, it was 80"),
        "{all}"
    );
    assert!(
        all.contains("Item 1 \"Welcome\": it reads \"Welcome!\" now"),
        "{all}"
    );
    assert!(
        all.contains("shows the arrangement \"Default\" instead of \"Playback\""),
        "{all}"
    );
    assert_eq!(report.differences.len(), 3);
    // A shorter list is reported too.
    let second = rig_from(standard_host(), Loading::Immediately);
    let plan = second
        .engine
        .preview(&request(
            &second,
            vec![wanted("1", "Alpha", 1, Some(1001), &["Intro"])],
            worship(),
        ))
        .await
        .unwrap();
    second
        .api
        .corrupt_reads(Some(Arc::new(|items: &mut Vec<Value>| {
            items.pop();
        })));
    let report = second
        .engine
        .apply(
            &request(
                &second,
                vec![wanted("1", "Alpha", 1, Some(1001), &["Intro"])],
                worship(),
            ),
            &plan.plan_id,
            &[],
            &Quiet,
            &Log::default(),
        )
        .await
        .unwrap();
    assert!(
        report
            .differences
            .iter()
            .any(|d| d.contains("The playlist has 6 items now; 7 were written")),
        "{:?}",
        report.differences
    );
}

#[tokio::test]
async fn a_song_already_in_the_playlist_is_not_added_twice() {
    let rig = rig(Loading::Immediately);
    let req = request(
        &rig,
        vec![wanted("1", "Alpha", 1, Some(1001), &["Intro", "Verse 1"])],
        Place::End,
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    rig.engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    // Two songs of the setlist that are the same song with the same arrangement: one copy, listed once.
    let twice = request(
        &rig,
        vec![
            wanted("1", "Alpha", 1, Some(1001), &["Intro", "Verse 1"]),
            wanted("2", "Alpha again", 2, Some(1001), &["Intro", "Verse 1"]),
        ],
        Place::End,
    );
    let plan = rig.engine.preview(&twice).await.unwrap();
    assert!(plan.songs.iter().all(|s| s.already_in_playlist));
    assert!(plan
        .warnings
        .iter()
        .any(|w| w.contains("already in the playlist")));
    assert!(!plan.can_apply);
}

#[tokio::test]
async fn the_last_twenty_backups_are_kept() {
    let rig = rig(Loading::Immediately);
    let dir = rig.backups.path().join("propresenter-playlist-backups");
    std::fs::create_dir_all(&dir).unwrap();
    for n in 0..24 {
        std::fs::write(dir.join(format!("old-{n:02}.json")), "{}").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let req = request(
        &rig,
        vec![wanted("1", "Alpha", 1, Some(1001), &["Intro"])],
        Place::End,
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    let mut left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left.len(), 20, "{left:?}");
    assert!(std::path::Path::new(&report.backup.unwrap()).exists());
    assert!(!left.contains(&"old-00.json".to_string()));
    assert!(left.contains(&"old-23.json".to_string()));
}

#[tokio::test]
async fn nothing_answering_is_reported_before_anything_changes() {
    let rig = rig(Loading::Immediately);
    let req = request(
        &rig,
        vec![wanted("1", "Alpha", 1, Some(1001), &["Intro"])],
        Place::End,
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    rig.api.set_down(true);
    assert!(matches!(
        rig.engine
            .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
            .await,
        Err(ApplyRefusal::Plan(_))
    ));
    rig.api.set_down(false);
    // The ProPresenter computer's files fail.
    rig.host.fail_next(LinkError::NoAnswer);
    assert!(rig.engine.preview(&req).await.is_err());
}

// ---- guesses ----------------------------------------------------------------------------------------------------------

/// The shape of two real songs: four blank-named groups (Count Off) that differ, and two Interludes that differ.
fn guessable() -> Vec<u8> {
    // groups: 0-3 blank, 4 Intro, 5 Verse 1, 6 Interlude (x), 7 Interlude (y)
    SyntheticSong::new("Gamma")
        .id(3003)
        .seed(0xC3000000)
        .group("", &["count a"])
        .group("", &["count b"])
        .group("", &["count c"])
        .group("", &["count d"])
        .group("Intro", &["intro words"])
        .group("Verse 1", &["verse words"])
        .group("Interlude", &["interlude x"])
        .group("Interlude", &["interlude y"])
        .arrangement("Full", &[4, 5, 6, 7])
        .selected(0)
        .build()
}

fn gamma_rig() -> Rig {
    let host = Arc::new(
        FakeHost::new()
            .with_file("Songs", "Alpha.pro", &alpha())
            .with_file("Songs", "Gamma.pro", &guessable()),
    );
    rig_from(host, Loading::Immediately)
}

const GAMMA_SECTIONS: [&str; 6] = [
    "Count Off",
    "Intro",
    "Verse 1",
    "Interlude",
    "Interlude",
    "Interlude",
];

#[tokio::test]
async fn sections_that_fit_several_groups_are_guessed_named_and_never_block() {
    let rig = gamma_rig();
    let req = request(
        &rig,
        vec![wanted("1", "Gamma", 1, Some(3003), &GAMMA_SECTIONS)],
        Place::End,
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    let song = &plan.songs[0];
    assert_eq!(song.action, SongAction::Copy, "{song:#?}");
    assert!(plan.can_apply, "guesses do not stop the Apply button");
    assert_eq!(plan.stuck, 0);
    assert_eq!(song.reason, None);
    let texts: Vec<&str> = song.guesses.iter().map(|g| g.text.as_str()).collect();
    assert_eq!(song.guesses.len(), 2, "{texts:#?}");
    assert_eq!(song.guesses[0].section, "Count Off");
    assert_eq!(song.guesses[0].group_name, "");
    assert_eq!(
        (
            song.guesses[0].position,
            song.guesses[0].group_index,
            song.guesses[0].groups
        ),
        (1, 1, 4)
    );
    assert_eq!(song.guesses[0].rule, "in_order");
    assert_eq!(song.guesses[1].section, "Interlude");
    assert_eq!(
        (
            song.guesses[1].position,
            song.guesses[1].uses,
            song.guesses[1].group_index
        ),
        (3, 3, 2)
    );
    assert_eq!(song.guesses[1].rule, "last_reused");
    assert!(
        song.guesses[1].text.starts_with(
            "Interlude (3rd use) uses the 2nd \"Interlude\" group: check it in ProPresenter."
        ),
        "{}",
        song.guesses[1].text
    );
    assert!(song.guesses[0].text.contains("1st blank-named group"));
    assert!(song
        .mapping
        .iter()
        .any(|m| m.how == "guessed" && m.line.is_some()));
    // The guesses are listed on their own, not twice.
    assert!(!song
        .notes
        .iter()
        .any(|n| n.contains("check it in ProPresenter")));
    // They are part of what the plan is.
    let again = rig.engine.preview(&req).await.unwrap();
    assert_eq!(again.plan_id, plan.plan_id);

    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    assert_eq!(report.songs[0].status, SongStatus::Added);
    assert_eq!(report.songs[0].guesses, song.guesses);
    // The copy plays: Count Off, Intro, Verse 1, Interlude (1st), Interlude (2nd), Interlude (2nd again).
    let bytes = rig.host.file("Playback", "Gamma (1).pro").unwrap();
    let made = Song::read(&bytes).unwrap();
    let playback = made
        .arrangements
        .iter()
        .find(|a| a.name == "Playback")
        .unwrap();
    let names = made.arrangement_group_names(playback);
    assert_eq!(
        names,
        [
            "",
            "Intro",
            "Verse 1",
            "Interlude",
            "Interlude",
            "Interlude"
        ]
    );
    assert_eq!(
        playback.group_ids[4], playback.group_ids[5],
        "the extra use reuses the last group"
    );
    assert_ne!(playback.group_ids[3], playback.group_ids[4]);
}

#[tokio::test]
async fn a_section_no_group_has_still_blocks_while_guesses_do_not() {
    let rig = gamma_rig();
    let mut names = GAMMA_SECTIONS.to_vec();
    names.push("Tag");
    let req = request(
        &rig,
        vec![wanted("1", "Gamma", 1, Some(3003), &names)],
        Place::End,
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    assert_eq!(plan.songs[0].action, SongAction::Blocked);
    assert!(plan.songs[0]
        .reason
        .as_deref()
        .unwrap()
        .contains("Tag (no group has that name)"));
    assert!(!plan.can_apply);
    assert_eq!(plan.stuck, 1);
}

#[tokio::test]
async fn a_song_imported_first_reports_its_guesses_in_the_result() {
    let rig = rig(Loading::Immediately);
    rig.host.set_import_song(3003, &guessable());
    let req = request(
        &rig,
        vec![wanted("1", "Gamma", 1, Some(3003), &GAMMA_SECTIONS)],
        Place::End,
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    assert_eq!(plan.songs[0].action, SongAction::ImportThenCopy);
    assert!(
        plan.songs[0].guesses.is_empty(),
        "known only after the import"
    );
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    assert_eq!(report.songs[0].guesses.len(), 2);
    assert_eq!(report.songs[0].guesses[1].rule, "last_reused");
}

// ---- no screen steps while ProPresenter is live -------------------------------------------------------------------------

fn showing(f: impl FnOnce(&mut Layers)) -> Layers {
    let mut layers = Layers {
        video_input: true,
        ..Layers::default()
    };
    f(&mut layers);
    layers
}

/// A song that needs the import, so the screen is wanted.
fn import_request(rig: &Rig) -> PlanRequest {
    import_returns(rig, 2002, "Delta", 0xD4000000);
    request(
        rig,
        vec![wanted("1", "Delta", 1, Some(2002), &["Intro", "Chorus"])],
        Place::End,
    )
}

fn screen_calls(host: &FakeHost) -> Vec<String> {
    host.calls()
        .into_iter()
        .filter(|c| c.starts_with("search_import") || c.starts_with("select_library"))
        .collect()
}

#[tokio::test]
async fn each_layer_that_shows_something_stops_the_import_before_anything_is_typed() {
    type Set = fn(&mut Layers);
    let cases: [(&str, Set); 5] = [
        ("slides", |l| l.slide = true),
        ("media", |l| l.media = true),
        ("announcements", |l| l.announcements = true),
        ("props", |l| l.props = true),
        ("messages", |l| l.messages = true),
    ];
    for (word, set) in cases {
        let rig = rig(Loading::Immediately);
        let req = import_request(&rig);
        let plan = rig.engine.preview(&req).await.unwrap();
        rig.api.set_layers(showing(set));
        let report = rig
            .engine
            .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
            .await
            .unwrap();
        assert_eq!(
            report.outcome,
            ApplyOutcome::LiveOnScreen,
            "{word}: {report:#?}"
        );
        assert!(
            report.message.contains(&format!(
                "ProPresenter is showing something on screen ({word}). Clear it or wait, then press Try again."
            )),
            "{}",
            report.message
        );
        assert_eq!(report.songs[0].code.as_deref(), Some("live_on_screen"));
        assert_eq!(report.songs[0].status, SongStatus::NotTried);
        assert!(!report.playlist_changed);
        assert!(rig.api.puts().is_empty());
        assert_eq!(
            screen_calls(&rig.host),
            Vec::<String>::new(),
            "{word}: nothing was typed or clicked"
        );
        assert!(rig.host.files_of("Playback").is_empty());
    }
}

#[tokio::test]
async fn several_layers_are_all_named() {
    let rig = rig(Loading::Immediately);
    let req = import_request(&rig);
    let plan = rig.engine.preview(&req).await.unwrap();
    rig.api.set_layers(showing(|l| {
        l.slide = true;
        l.props = true;
    }));
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert!(
        report.message.contains("(slides, props)"),
        "{}",
        report.message
    );
}

#[tokio::test]
async fn a_camera_input_and_audio_alone_do_not_stop_anything() {
    let rig = rig(Loading::Immediately);
    let req = import_request(&rig);
    let plan = rig.engine.preview(&req).await.unwrap();
    rig.api.set_layers(Layers {
        video_input: true,
        audio: true,
        ..Layers::default()
    });
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    assert_eq!(screen_calls(&rig.host).len(), 1, "the import ran");
}

#[tokio::test]
async fn an_answer_that_cannot_be_read_counts_as_live_and_says_so() {
    let rig = rig(Loading::Immediately);
    let req = import_request(&rig);
    let plan = rig.engine.preview(&req).await.unwrap();
    rig.api.set_layers_unreadable();
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::LiveOnScreen, "{report:#?}");
    assert!(
        report
            .message
            .contains("could not read what ProPresenter is showing"),
        "{}",
        report.message
    );
    assert!(report.message.contains("press Try again"));
    assert_eq!(screen_calls(&rig.host), Vec::<String>::new());
}

#[tokio::test]
async fn it_is_asked_again_right_before_the_import_and_files_already_made_are_kept() {
    let rig = rig(Loading::Immediately);
    import_returns(&rig, 2002, "Delta", 0xD4000000);
    let req = request(
        &rig,
        vec![
            wanted("1", "Alpha", 1, Some(1001), &["Intro", "Verse 1"]),
            wanted("2", "Delta", 2, Some(2002), &["Intro", "Chorus"]),
        ],
        Place::End,
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    // The start says all clear; right before the import something is on screen.
    rig.api
        .queue_layers(vec![Ok(showing(|_| {})), Ok(showing(|l| l.slide = true))]);
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::LiveOnScreen, "{report:#?}");
    assert_eq!(report.songs[1].code.as_deref(), Some("live_on_screen"));
    assert_eq!(screen_calls(&rig.host), Vec::<String>::new());
    assert!(rig.api.puts().is_empty(), "the playlist was not written");
    assert_eq!(
        rig.host.files_of("Playback").len(),
        1,
        "the copy made before is kept"
    );
    // Clear: try again. The copy that was made is used, the import runs, the playlist is written once.
    rig.api.queue_layers(Vec::new());
    let plan2 = rig.engine.preview(&req).await.unwrap();
    let report = rig
        .engine
        .apply(&req, &plan2.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    assert_eq!(screen_calls(&rig.host).len(), 1);
    assert_eq!(rig.api.puts().len(), 1);
}

#[tokio::test]
async fn selecting_the_library_is_refused_too_and_try_again_works_once_it_is_clear() {
    let rig = rig(Loading::AfterSelect);
    let req = request(
        &rig,
        vec![wanted("1", "Alpha", 1, Some(1001), &["Intro"])],
        Place::End,
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    // No import is planned, so only the later "select the library" step could touch the screen.
    rig.api.set_layers(showing(|l| l.media = true));
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::LiveOnScreen, "{report:#?}");
    assert!(report.message.contains("(media)"), "{}", report.message);
    assert!(report.message.contains("were made are kept"));
    assert!(
        rig.host.selected().is_empty(),
        "the screen was never touched"
    );
    assert_eq!(screen_calls(&rig.host), Vec::<String>::new());
    assert!(rig.api.puts().is_empty());
    assert_eq!(rig.host.files_of("Playback").len(), 1);
    // The media is cleared: Try again selects the library and finishes.
    rig.api.set_layers(showing(|_| {}));
    let plan2 = rig.engine.preview(&req).await.unwrap();
    let report = rig
        .engine
        .apply(&req, &plan2.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    assert_eq!(rig.host.selected(), ["Playback"]);
}

// ---- the import step must leave the playlist alone ----------------------------------------------------------------------

/// A host whose import, wrongly, also adds an item to the playlist (what a stray key press in ProPresenter could do).
struct Meddling {
    inner: Arc<FakeHost>,
    api: Arc<FakeApi>,
    playlist: String,
}

#[async_trait::async_trait]
impl ProPresenterHost for Meddling {
    async fn info(&self) -> Result<HostInfo, LinkError> {
        self.inner.info().await
    }
    async fn list_library(&self, name: &str) -> Result<Vec<LibraryFile>, LinkError> {
        self.inner.list_library(name).await
    }
    async fn read_file(&self, library: &str, file: &str) -> Result<Vec<u8>, LinkError> {
        self.inner.read_file(library, file).await
    }
    async fn ensure_library(&self, name: &str) -> Result<bool, LinkError> {
        self.inner.ensure_library(name).await
    }
    async fn create_file(
        &self,
        library: &str,
        file: &str,
        bytes: &[u8],
        sha256: &str,
    ) -> Result<(), LinkError> {
        self.inner.create_file(library, file, bytes, sha256).await
    }
    async fn song_index(&self) -> Result<Vec<IndexEntry>, LinkError> {
        self.inner.song_index().await
    }
    async fn search_import(
        &self,
        multitracks_id: &str,
        name: &str,
        playlist: Option<String>,
    ) -> Result<String, LinkError> {
        let made = self
            .inner
            .search_import(multitracks_id, name, playlist)
            .await?;
        let mut items = self.api.stored(&self.playlist);
        items.push(FakeApi::header_item("Stray", 0.5));
        self.api.put_playlist(&self.playlist, &items).await.unwrap();
        Ok(made)
    }
    async fn select_library(&self, name: &str) -> Result<SelectOutcome, LinkError> {
        self.inner.select_library(name).await
    }
}

#[tokio::test]
async fn a_playlist_that_changed_during_the_import_is_reported_as_a_difference() {
    let rig = rig(Loading::Immediately);
    import_returns(&rig, 2002, "Delta", 0xD4000000);
    let meddling = Arc::new(Meddling {
        inner: rig.host.clone(),
        api: rig.api.clone(),
        playlist: rig.playlist.clone(),
    });
    let engine = AddSongs::new(
        meddling,
        rig.api.clone(),
        rig.backups.path().join("propresenter-playlist-backups"),
        Arc::new(|| NOW),
        counter(),
    )
    .with_timing(Timing::quick());
    let req = request(
        &rig,
        vec![wanted("1", "Delta", 1, Some(2002), &["Intro", "Chorus"])],
        Place::End,
    );
    let plan = engine.preview(&req).await.unwrap();
    let log = Log::default();
    let report = engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &log)
        .await
        .unwrap();
    assert_eq!(
        report.outcome,
        ApplyOutcome::DoneWithDifferences,
        "{report:#?}"
    );
    assert!(
        report
            .differences
            .iter()
            .any(|d| d.contains("had 6 item(s) before the import and has 7 now")),
        "{:?}",
        report.differences
    );
    assert!(log
        .texts()
        .iter()
        .any(|t| t.contains("meant to leave the playlist alone")));
}

#[tokio::test]
async fn an_import_that_leaves_the_playlist_alone_reports_no_difference() {
    let rig = rig(Loading::Immediately);
    let req = import_request(&rig);
    let plan = rig.engine.preview(&req).await.unwrap();
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    assert!(report.differences.is_empty());
}

// ---- songs asked for by their title alone: added as they are --------------------------------------------------------------------

#[tokio::test]
async fn a_title_is_found_in_the_songs_library_as_it_is_and_a_missing_one_is_blocked_in_plain_words(
) {
    let rig = rig(Loading::Immediately);
    let req = request(
        &rig,
        vec![
            titled("t1", "  beta ", 1),
            titled("t2", "Gamma", 2),
            titled("t3", "ALPHA", 3),
        ],
        worship(),
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    let beta = &plan.songs[0];
    assert_eq!(beta.action, SongAction::AsIs);
    assert_eq!(beta.file.as_deref(), Some("Beta"));
    assert_eq!(beta.source.as_deref(), Some("Songs / Beta.pro"));
    assert!(!beta.already_in_playlist);
    assert!(beta.mapping.is_empty() && beta.guesses.is_empty());
    assert!(
        beta.notes.iter().any(|n| n.contains("no copy is made")),
        "{:?}",
        beta.notes
    );
    let gamma = &plan.songs[1];
    assert_eq!(gamma.action, SongAction::Blocked);
    assert!(
        gamma.reason.as_deref().unwrap().contains(
            "does not have \"Gamma\" in its Songs library. Playback's data is needed to import it."
        ),
        "{:?}",
        gamma.reason
    );
    assert!(gamma.import_as.is_none(), "a title is never imported");
    // Alpha is already an item of the playlist: it is not added a second time.
    assert!(plan.songs[2].already_in_playlist);
    assert_eq!((plan.to_add, plan.stuck), (1, 1));
    // A preview of titles writes and imports nothing.
    let calls = rig.host.calls().join("\n");
    for forbidden in [
        "create_file",
        "search_import",
        "ensure_library",
        "select_library",
    ] {
        assert!(!calls.contains(forbidden), "{forbidden}: {calls}");
    }
    assert!(rig.api.puts().is_empty());
}

#[tokio::test]
async fn titles_are_looked_up_by_file_name_rules_and_only_in_the_songs_library() {
    let host = Arc::new(
        FakeHost::new()
            .with_file("Songs", "Alpha.pro", &alpha())
            .with_file(
                "Songs",
                "Beta.pro",
                &song_numbered("Beta", 1002, 0xB2000000),
            )
            .with_file(
                "Songs",
                "Beta (1).pro",
                &song_numbered("Beta", 1002, 0xB3000000),
            )
            .with_file(
                "Playback",
                "Gamma (1).pro",
                &song_numbered("Gamma", 1003, 0xC1000000),
            ),
    );
    let rig = rig_from(host, Loading::Immediately);
    let found = rig
        .engine
        .find_titles(&[
            "alpha".to_string(),
            "Gamma".to_string(),
            "Beta (1)".to_string(),
            "Nope".to_string(),
        ])
        .await
        .unwrap();
    let states: Vec<(&str, TitleFound)> =
        found.iter().map(|m| (m.title.as_str(), m.found)).collect();
    assert_eq!(
        states,
        [
            ("alpha", TitleFound::Found),
            ("Gamma", TitleFound::Missing),
            ("Beta (1)", TitleFound::Found),
            ("Nope", TitleFound::Missing),
        ]
    );
    // Reading the names writes nothing.
    assert!(!rig.host.calls().join("\n").contains("create_file"));
}

#[tokio::test]
async fn two_songs_with_one_title_are_not_guessed() {
    let host = Arc::new(
        FakeHost::new()
            .with_file(
                "Songs",
                "Beta.pro",
                &song_numbered("Beta", 1002, 0xB2000000),
            )
            .with_file(
                "Songs",
                "BETA.pro",
                &song_numbered("Beta", 1002, 0xB3000000),
            ),
    );
    let rig = rig_from(host, Loading::Immediately);
    let plan = rig
        .engine
        .preview(&request(&rig, vec![titled("t1", "beta", 1)], worship()))
        .await
        .unwrap();
    assert_eq!(plan.songs[0].action, SongAction::NeedsChoice);
    assert_eq!(plan.songs[0].choices.len(), 2);
    assert_eq!(
        rig.engine.find_titles(&["beta".to_string()]).await.unwrap()[0].found,
        TitleFound::Several
    );
}

#[tokio::test]
async fn apply_puts_a_title_into_the_playlist_as_it_is_with_no_file_made_and_nothing_imported() {
    let rig = rig(Loading::Immediately);
    let req = request(&rig, vec![titled("t1", "Beta", 1)], worship());
    let plan = rig.engine.preview(&req).await.unwrap();
    assert!(plan.can_apply);
    let before = rig.api.stored(&rig.playlist);
    let beta_file = rig.host.file("Songs", "Beta.pro").unwrap();
    let log = Log::default();
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &log)
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    assert!(report.differences.is_empty(), "{:?}", report.differences);
    assert_eq!(report.songs[0].status, SongStatus::Added);
    assert_eq!(report.songs[0].file.as_deref(), Some("Beta"));
    // No file made, nothing imported, the Playback library not even created, no screen, the song itself untouched.
    let calls = rig.host.calls().join("\n");
    for forbidden in [
        "create_file",
        "search_import",
        "ensure_library",
        "select_library",
    ] {
        assert!(!calls.contains(forbidden), "{forbidden}: {calls}");
    }
    assert!(rig.host.files_of("Playback").is_empty());
    assert_eq!(rig.host.file("Songs", "Beta.pro").unwrap(), beta_file);
    // One playlist write; the new item is Beta (the library entry's id twice, its display name), at the end of the Worship section.
    assert_eq!(rig.api.puts().len(), 1);
    let after = rig.api.stored(&rig.playlist);
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(
        names_of(&after),
        [
            "Welcome",
            "Alpha",
            "Worship",
            "Alpha",
            "Beta",
            "Message",
            "Sermon placeholder"
        ]
    );
    let item = &after[4];
    assert_eq!(
        item["id"]["uuid"],
        item["presentation_info"]["presentation_uuid"]
    );
    assert_eq!(
        item["id"]["uuid"],
        stagepilot_propresenter::link::api::fake::entry_uuid("Songs", "Beta.pro")
    );
    // It keeps the arrangement the song itself has selected: StagePilot does not choose one.
    assert_eq!(item["presentation_info"]["arrangement_name"], "Full");
    let texts = log.texts().join("\n");
    assert!(
        texts.contains("as it is in the Songs library. Nothing is copied."),
        "{texts}"
    );
    assert!(texts.contains("No copy is made"), "{texts}");
    assert!(texts.contains("reads back as planned"), "{texts}");
    // Asking again: it is there now and is not added twice.
    let again = rig.engine.preview(&req).await.unwrap();
    assert!(again.songs[0].already_in_playlist);
    assert_eq!(
        rig.engine
            .apply(&req, &again.plan_id, &[], &Quiet, &log)
            .await,
        Err(ApplyRefusal::NothingToAdd)
    );
    assert_eq!(rig.api.puts().len(), 1);
}

#[tokio::test]
async fn a_title_that_is_missing_must_be_left_out_and_then_the_rest_is_added_with_a_copy_beside_it()
{
    let rig = rig(Loading::Immediately);
    let req = request(
        &rig,
        vec![titled("t1", "Gamma", 1), titled("t2", "Beta", 2)],
        worship(),
    );
    let plan = rig.engine.preview(&req).await.unwrap();
    assert_eq!((plan.to_add, plan.stuck), (1, 1));
    assert_eq!(
        rig.engine
            .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
            .await,
        Err(ApplyRefusal::StuckNotSkipped(vec!["t1".to_string()]))
    );
    let report = rig
        .engine
        .apply(
            &req,
            &plan.plan_id,
            &["t1".to_string()],
            &Quiet,
            &Log::default(),
        )
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    assert_eq!(report.songs[0].status, SongStatus::Skipped);
    assert_eq!(report.songs[1].status, SongStatus::Added);
    assert!(
        rig.host.imports().is_empty(),
        "a missing title is never imported"
    );
    assert_eq!(
        names_of(&rig.api.stored(&rig.playlist)),
        [
            "Welcome",
            "Alpha",
            "Worship",
            "Alpha",
            "Beta",
            "Message",
            "Sermon placeholder"
        ]
    );
}

#[tokio::test]
async fn a_song_as_it_is_and_a_copy_can_go_in_together_in_the_order_asked() {
    let rig = rig(Loading::Immediately);
    let mut songs = vec![
        titled("t1", "Beta", 1),
        wanted(
            "11",
            "Alpha",
            2,
            Some(1001),
            &["Count Off", "Intro", "Verse 1", "Chorus"],
        ),
    ];
    songs.reverse();
    let req = request(&rig, songs, worship());
    let plan = rig.engine.preview(&req).await.unwrap();
    let actions: Vec<SongAction> = plan.songs.iter().map(|s| s.action).collect();
    assert_eq!(actions, [SongAction::AsIs, SongAction::Copy]);
    let report = rig
        .engine
        .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
        .await
        .unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Done, "{report:#?}");
    assert!(report.differences.is_empty(), "{:?}", report.differences);
    let after = rig.api.stored(&rig.playlist);
    assert_eq!(names_of(&after)[4..6], ["Beta", "Alpha (1)"]);
    assert_eq!(after[4]["presentation_info"]["arrangement_name"], "Full");
    assert_eq!(
        after[5]["presentation_info"]["arrangement_name"],
        "Playback"
    );
    // Only the copy was made.
    assert_eq!(
        rig.host
            .files_of("Playback")
            .into_iter()
            .map(|(n, _)| n)
            .collect::<Vec<_>>(),
        ["Alpha (1).pro"]
    );
}

/// The Section box's words, against the fake playlist: a section is every item after its header up to the next header, and the songs
/// go at its END (right before the next header). A section with items, a section with none, the last section, and End of Playlist.
#[tokio::test]
async fn songs_land_at_the_end_of_the_chosen_section_or_the_end_of_the_playlist() {
    let alpha_uuid = stagepilot_propresenter::link::api::fake::entry_uuid("Songs", "Alpha.pro");
    let items = || {
        vec![
            FakeApi::header_item("Welcome", 0.1),
            FakeApi::presentation_item(&alpha_uuid, "Alpha", "Default", 12.5),
            FakeApi::header_item("Empty", 0.2),
            FakeApi::header_item("Worship", 0.3),
            FakeApi::presentation_item(&alpha_uuid, "Alpha", "Default", 80.0),
            FakeApi::header_item("Message", 0.4),
            FakeApi::presentation_item(&alpha_uuid, "Alpha", "Default", 30.0),
        ]
    };
    let cases: [(Place, [&str; 8]); 5] = [
        (
            Place::AfterHeader("Welcome".into()),
            [
                "Welcome", "Alpha", "Beta", "Empty", "Worship", "Alpha", "Message", "Alpha",
            ],
        ),
        (
            Place::AfterHeader("Empty".into()),
            [
                "Welcome", "Alpha", "Empty", "Beta", "Worship", "Alpha", "Message", "Alpha",
            ],
        ),
        (
            Place::AfterHeader("Worship".into()),
            [
                "Welcome", "Alpha", "Empty", "Worship", "Alpha", "Beta", "Message", "Alpha",
            ],
        ),
        (
            Place::AfterHeader("Message".into()),
            [
                "Welcome", "Alpha", "Empty", "Worship", "Alpha", "Message", "Alpha", "Beta",
            ],
        ),
        (
            Place::End,
            [
                "Welcome", "Alpha", "Empty", "Worship", "Alpha", "Message", "Alpha", "Beta",
            ],
        ),
    ];
    for (place, want) in cases {
        let rig = rig(Loading::Immediately);
        let playlist = rig.api.add_playlist("Sections", None, items());
        let req = PlanRequest {
            songs: vec![titled("t1", "Beta", 1)],
            playlist_id: playlist.clone(),
            place: place.clone(),
            sources: Default::default(),
        };
        let plan = rig.engine.preview(&req).await.unwrap();
        assert!(plan.can_apply, "{place:?}: {:?}", plan.playlist.blocked);
        let report = rig
            .engine
            .apply(&req, &plan.plan_id, &[], &Quiet, &Log::default())
            .await
            .unwrap();
        assert_eq!(report.outcome, ApplyOutcome::Done, "{place:?}: {report:#?}");
        assert_eq!(names_of(&rig.api.stored(&playlist)), want, "{place:?}");
    }
}

/// Two headers with the same name: the engine does not guess which section is meant, and says so in one plain line.
#[tokio::test]
async fn a_header_name_the_playlist_has_twice_is_refused_with_a_plain_line() {
    let rig = rig(Loading::Immediately);
    let playlist = rig.api.add_playlist(
        "Twice",
        None,
        vec![
            FakeApi::header_item("Worship", 0.1),
            FakeApi::header_item("Message", 0.2),
            FakeApi::header_item("Worship", 0.3),
        ],
    );
    let req = PlanRequest {
        songs: vec![titled("t1", "Beta", 1)],
        playlist_id: playlist,
        place: Place::AfterHeader("Worship".into()),
        sources: Default::default(),
    };
    let plan = rig.engine.preview(&req).await.unwrap();
    assert_eq!(
        plan.playlist.blocked.as_deref(),
        Some("More than one header in this playlist is named \"Worship\". Pick another place.")
    );
    assert!(!plan.can_apply);
}

// ---- F12: the person picks the file of a song that has more than one --------------------------------------------------------

const FIRM: u64 = 5150;

/// One song (mtID 5150) in two files of the Songs library, both with every section the arrangement asks for.
fn firm_foundation(first: &str, second: &str) -> Rig {
    rig_from(
        Arc::new(
            FakeHost::new()
                .with_file("Songs", "Alpha.pro", &alpha())
                .with_file(
                    "Songs",
                    first,
                    &song_numbered("Firm Foundation", FIRM, 0xF1000000),
                )
                .with_file(
                    "Songs",
                    second,
                    &song_numbered("Firm Foundation", FIRM, 0xF2000000),
                ),
        ),
        Loading::Immediately,
    )
}

fn firm_request(rig: &Rig, title: &str, picks: &[(&str, &str)]) -> PlanRequest {
    let mut req = request(
        rig,
        vec![wanted(
            "7",
            title,
            1,
            Some(FIRM),
            &["Intro", "Verse 1", "Chorus"],
        )],
        worship(),
    );
    for (ref_id, file) in picks {
        req.sources
            .insert((*ref_id).to_string(), (*file).to_string());
    }
    req
}

#[tokio::test]
async fn two_files_of_one_song_ask_in_short_words_and_list_both() {
    let rig = firm_foundation("Firm Foundation (Live).pro", "Firm Foundation (Studio).pro");
    let plan = rig
        .engine
        .preview(&firm_request(&rig, "Firm Foundation", &[]))
        .await
        .unwrap();
    let song = &plan.songs[0];
    assert_eq!(song.action, SongAction::NeedsChoice);
    assert_eq!(
        song.choices,
        vec![
            "Songs / Firm Foundation (Live).pro".to_string(),
            "Songs / Firm Foundation (Studio).pro".to_string()
        ]
    );
    assert_eq!(
        song.reason.as_deref(),
        Some("Two files are this song. Pick one.")
    );
    assert_eq!((song.chosen_by, song.chosen_file.clone()), (None, None));
    assert_eq!((plan.stuck, plan.can_apply), (1, false));
}

#[tokio::test]
async fn a_pick_is_copied_from_and_another_pick_is_another_plan_that_apply_refuses_the_old_id_for()
{
    let rig = firm_foundation("Firm Foundation (Live).pro", "Firm Foundation (Studio).pro");
    let live = "Songs / Firm Foundation (Live).pro";
    let studio = "Songs / Firm Foundation (Studio).pro";
    let first = rig
        .engine
        .preview(&firm_request(&rig, "Firm Foundation", &[("7", live)]))
        .await
        .unwrap();
    let song = &first.songs[0];
    assert_eq!(song.action, SongAction::Copy);
    assert_eq!(song.source.as_deref(), Some(live));
    assert_eq!(
        song.source_file,
        Some((
            "Songs".to_string(),
            "Firm Foundation (Live).pro".to_string()
        ))
    );
    assert_eq!(song.file.as_deref(), Some("Firm Foundation (Live) (1)"));
    assert_eq!(
        (song.chosen_by, song.chosen_file.as_deref()),
        (Some(ChosenBy::Pick), Some(live))
    );
    assert_eq!(
        song.choices.len(),
        2,
        "the other file stays on offer so the person can change"
    );
    assert_eq!(song.mapping.len(), 3);
    assert!(
        song.notes
            .iter()
            .any(|n| n == "Used \"Firm Foundation (Live)\": the file you picked."),
        "{:?}",
        song.notes
    );
    assert!(first.can_apply);
    let other = rig
        .engine
        .preview(&firm_request(&rig, "Firm Foundation", &[("7", studio)]))
        .await
        .unwrap();
    assert_eq!(other.songs[0].source.as_deref(), Some(studio));
    assert_ne!(
        first.plan_id, other.plan_id,
        "a different pick is a different plan"
    );
    // The same pick is the same plan.
    let again = rig
        .engine
        .preview(&firm_request(&rig, "Firm Foundation", &[("7", live)]))
        .await
        .unwrap();
    assert_eq!(first.plan_id, again.plan_id);
    // Apply takes the request that was previewed: the old id is refused for the other pick and nothing is made.
    let log = Log::default();
    assert_eq!(
        rig.engine
            .apply(
                &firm_request(&rig, "Firm Foundation", &[("7", studio)]),
                &first.plan_id,
                &[],
                &Quiet,
                &log
            )
            .await,
        Err(ApplyRefusal::PlanChanged)
    );
    assert_eq!(
        rig.engine
            .apply(
                &firm_request(&rig, "Firm Foundation", &[]),
                &first.plan_id,
                &[],
                &Quiet,
                &log
            )
            .await,
        Err(ApplyRefusal::PlanChanged),
        "no pick: the plan is another one"
    );
    assert!(rig
        .host
        .calls()
        .iter()
        .all(|c| !c.starts_with("create_file")));
    let report = rig
        .engine
        .apply(
            &firm_request(&rig, "Firm Foundation", &[("7", live)]),
            &first.plan_id,
            &[],
            &Quiet,
            &log,
        )
        .await
        .unwrap();
    assert_eq!(
        report.outcome,
        ApplyOutcome::Done,
        "{report:#?} {:?} {:?}",
        log.texts(),
        rig.host.calls()
    );
    assert!(rig
        .host
        .file("Playback", "Firm Foundation (Live) (1).pro")
        .is_some());
}

#[tokio::test]
async fn a_pick_that_is_none_of_the_songs_files_is_ignored_and_never_an_error() {
    let rig = firm_foundation("Firm Foundation (Live).pro", "Firm Foundation (Studio).pro");
    for pick in [
        "Songs / Alpha.pro",
        "Songs / Firm Foundation (Live)",
        "firm foundation (live).pro",
        "",
        "Playback / Firm Foundation (Live) (1).pro",
    ] {
        let plan = rig
            .engine
            .preview(&firm_request(&rig, "Firm Foundation", &[("7", pick)]))
            .await
            .unwrap();
        assert_eq!(plan.songs[0].action, SongAction::NeedsChoice, "{pick:?}");
        assert_eq!(plan.songs[0].choices.len(), 2, "{pick:?}");
        assert_eq!(plan.songs[0].chosen_by, None);
    }
    // A pick for a key that is no song of the request changes nothing about the song.
    let plan = rig
        .engine
        .preview(&firm_request(
            &rig,
            "Firm Foundation",
            &[("99", "Songs / Firm Foundation (Live).pro")],
        ))
        .await
        .unwrap();
    assert_eq!(plan.songs[0].action, SongAction::NeedsChoice);
}

#[tokio::test]
async fn a_picked_file_that_cannot_make_the_arrangement_is_blocked_in_its_own_words_not_swapped() {
    let host = Arc::new(
        FakeHost::new()
            .with_file(
                "Songs",
                "Firm Foundation (Live).pro",
                &song_numbered("Firm Foundation", FIRM, 1),
            )
            .with_file(
                "Songs",
                "Firm Foundation (Short).pro",
                &SyntheticSong::new("Firm Foundation")
                    .id(FIRM)
                    .seed(2)
                    .group("Intro", &["i"])
                    .group("Verse 1", &["v"])
                    .arrangement("Full", &[0, 1])
                    .selected(0)
                    .build(),
            ),
    );
    let rig = rig_from(host, Loading::Immediately);
    // Nobody picks: only the Live file works, it is used (as before).
    let plan = rig
        .engine
        .preview(&firm_request(&rig, "Firm Foundation", &[]))
        .await
        .unwrap();
    assert_eq!(
        plan.songs[0].source.as_deref(),
        Some("Songs / Firm Foundation (Live).pro")
    );
    assert_eq!(plan.songs[0].chosen_by, None);
    // The person picks the Short one: they see why, and the other file stays on offer.
    let short = "Songs / Firm Foundation (Short).pro";
    let plan = rig
        .engine
        .preview(&firm_request(&rig, "Firm Foundation", &[("7", short)]))
        .await
        .unwrap();
    let song = &plan.songs[0];
    assert_eq!(song.action, SongAction::Blocked);
    assert!(
        song.reason
            .as_deref()
            .unwrap()
            .contains("no group with that name in the song: Chorus"),
        "{:?}",
        song.reason
    );
    assert_eq!(
        (song.chosen_by, song.chosen_file.as_deref()),
        (Some(ChosenBy::Pick), Some(short))
    );
    assert_eq!(song.choices.len(), 2);
    assert!(song.mapping.iter().any(|m| m.how == "missing"));
    assert!(!plan.can_apply);
}

#[tokio::test]
async fn the_title_rule_uses_the_one_file_named_like_the_title_and_says_so() {
    let rig = firm_foundation("Firm Foundation (Live).pro", "Firm Foundation.pro");
    let plan = rig
        .engine
        .preview(&firm_request(&rig, "Firm Foundation (Live)", &[]))
        .await
        .unwrap();
    let song = &plan.songs[0];
    assert_eq!(song.action, SongAction::Copy);
    assert_eq!(
        song.source.as_deref(),
        Some("Songs / Firm Foundation (Live).pro")
    );
    assert_eq!(
        (song.chosen_by, song.chosen_file.as_deref()),
        (
            Some(ChosenBy::Title),
            Some("Songs / Firm Foundation (Live).pro")
        )
    );
    assert!(
        song.notes
            .iter()
            .any(|n| n == "Used \"Firm Foundation (Live)\": its name matches the Playback title."),
        "{:?}",
        song.notes
    );
    assert_eq!(song.choices.len(), 2);
    assert!(plan.can_apply);
    // Case and spacing do not matter.
    let plan = rig
        .engine
        .preview(&firm_request(&rig, "  firm   FOUNDATION ", &[]))
        .await
        .unwrap();
    assert_eq!(
        plan.songs[0].source.as_deref(),
        Some("Songs / Firm Foundation.pro")
    );
    assert_eq!(plan.songs[0].chosen_by, Some(ChosenBy::Title));
    // A pick beats the rule.
    let plan = rig
        .engine
        .preview(&firm_request(
            &rig,
            "Firm Foundation",
            &[("7", "Songs / Firm Foundation (Live).pro")],
        ))
        .await
        .unwrap();
    assert_eq!(
        plan.songs[0].source.as_deref(),
        Some("Songs / Firm Foundation (Live).pro")
    );
    assert_eq!(plan.songs[0].chosen_by, Some(ChosenBy::Pick));
}

#[tokio::test]
async fn the_title_rule_does_not_decide_when_two_names_or_none_match() {
    // Two names that are the title once case and spacing are ignored.
    let rig = firm_foundation("Firm Foundation.pro", "FIRM  Foundation.pro");
    let plan = rig
        .engine
        .preview(&firm_request(&rig, "firm foundation", &[]))
        .await
        .unwrap();
    assert_eq!(plan.songs[0].action, SongAction::NeedsChoice);
    assert_eq!(plan.songs[0].chosen_by, None);
    // No name is the title.
    let rig = firm_foundation("Firm Foundation (Live).pro", "Firm Foundation (Studio).pro");
    let plan = rig
        .engine
        .preview(&firm_request(&rig, "Firm Foundation", &[]))
        .await
        .unwrap();
    assert_eq!(plan.songs[0].action, SongAction::NeedsChoice);
    // The title is a title of nothing.
    let plan = rig
        .engine
        .preview(&firm_request(&rig, "   ", &[]))
        .await
        .unwrap();
    assert_eq!(plan.songs[0].action, SongAction::NeedsChoice);
}

#[tokio::test]
async fn a_song_asked_for_by_its_title_takes_a_pick_and_the_title_rule_too() {
    let host = Arc::new(
        FakeHost::new()
            .with_file(
                "Songs",
                "Beta.pro",
                &song_numbered("Beta", 1002, 0xB2000000),
            )
            .with_file(
                "Songs",
                "BETA.pro",
                &song_numbered("Beta", 1002, 0xB3000000),
            ),
    );
    let rig = rig_from(host, Loading::Immediately);
    let ask = |picks: &[(&str, &str)]| {
        let mut req = request(&rig, vec![titled("t1", "beta", 1)], worship());
        for (k, v) in picks {
            req.sources.insert((*k).to_string(), (*v).to_string());
        }
        req
    };
    let plan = rig.engine.preview(&ask(&[])).await.unwrap();
    assert_eq!(plan.songs[0].action, SongAction::NeedsChoice);
    assert_eq!(
        plan.songs[0].reason.as_deref(),
        Some("Two files are this song. Pick one.")
    );
    assert_eq!(plan.songs[0].choices.len(), 2);
    let plan = rig
        .engine
        .preview(&ask(&[("t1", "Songs / BETA.pro")]))
        .await
        .unwrap();
    let song = &plan.songs[0];
    assert_eq!(song.action, SongAction::AsIs);
    assert_eq!(song.source.as_deref(), Some("Songs / BETA.pro"));
    assert_eq!(song.file.as_deref(), Some("BETA"));
    assert_eq!(song.chosen_by, Some(ChosenBy::Pick));
    assert_eq!(song.choices.len(), 2);
    assert!(plan.can_apply);
    // A pick that is none of the entries is ignored.
    let plan = rig
        .engine
        .preview(&ask(&[("t1", "Songs / Gamma.pro")]))
        .await
        .unwrap();
    assert_eq!(plan.songs[0].action, SongAction::NeedsChoice);
    // The title rule: only one of the two entries is named exactly like the title (ProPresenter cuts names at a dot).
    let host = Arc::new(
        FakeHost::new()
            .with_file(
                "Songs",
                "Emptied (feat Tyana Reynolds).pro",
                &song_numbered("Emptied", 3003, 1),
            )
            .with_file(
                "Songs",
                "Emptied (feat. Tyana Reynolds).pro",
                &song_numbered("Emptied", 3003, 2),
            ),
    );
    let rig = rig_from(host, Loading::Immediately);
    let plan = rig
        .engine
        .preview(&request(
            &rig,
            vec![titled("t1", "Emptied (feat. Tyana Reynolds)", 1)],
            worship(),
        ))
        .await
        .unwrap();
    let song = &plan.songs[0];
    assert_eq!(song.action, SongAction::AsIs);
    assert_eq!(
        song.source.as_deref(),
        Some("Songs / Emptied (feat. Tyana Reynolds).pro")
    );
    assert_eq!(song.chosen_by, Some(ChosenBy::Title));
    assert!(
        song.notes
            .iter()
            .any(|n| n == "Used \"Emptied (feat. Tyana Reynolds)\": its name matches the title."),
        "{:?}",
        song.notes
    );
}
