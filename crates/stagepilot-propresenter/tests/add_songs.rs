//! The add-songs engine against an in-memory ProPresenter computer (files) and an in-memory ProPresenter API: preview, apply,
//! the waits, the playlist write and the read-back.

use serde_json::{json, Value};
use stagepilot_propresenter::link::add_songs::{
    AddSongs, ApplyOutcome, ApplyRefusal, Event, Guard, Place, PlanRequest, Progress, SongAction,
    SongStatus, Timing, WantedSong,
};
use stagepilot_propresenter::link::api::fake::{FakeApi, Loading};
use stagepilot_propresenter::link::api::{ApiError, ProPresenterApi};
use stagepilot_propresenter::link::fake::FakeHost;
use stagepilot_propresenter::link::LinkError;
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
            .with_file("Songs", "Gamma.pro", &song_numbered("Gamma", 77, 1))
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
