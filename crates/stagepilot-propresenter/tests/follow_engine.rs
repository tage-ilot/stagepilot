//! Following Playback with the lyrics in ProPresenter, against the fake ProPresenter (`FakeApi` on a `FakeHost` of made-up song
//! files) and the numeric fixture of a real 54-cue song (times, slide numbers, section names and group cue counts; no words).
//! The follower is driven step by step with its own clock, so every trigger and its flat slide number can be asserted exactly,
//! and the fake says which RAW cue of the file each flat number really showed (it flattens the file's arrangement on its own).

use serde_json::Value;
use stagepilot_propresenter::follow::binding::{Binding, BindingStore, MemoryBindingStore};
use stagepilot_propresenter::follow::engine::{
    FollowState, Follower, Input, PlaybackNow, SongView, Timing, HOLD_REASON,
};
use stagepilot_propresenter::follow::prepare::{
    Preparer, Readiness, SongInput, SongStatus, ARRANGEMENT_CHANGED, FOLLOWING_BY_BARS,
    FOLLOWING_BY_CUES, LINK_NOT_READY, NOT_IN_PLAYLIST,
};
use stagepilot_propresenter::follow::{LyricCue, Section, HASH_VERSION};
use stagepilot_propresenter::link::api::fake::{entry_uuid, FakeApi, Loading, Trigger};
use stagepilot_propresenter::link::api::ApiError;
use stagepilot_propresenter::link::fake::FakeHost;
use stagepilot_propresenter::pro_file::synthetic::SyntheticSong;
use std::collections::BTreeSet;
use std::sync::Arc;

const FIXTURE: &str = include_str!("fixtures/oh-my-soul-lyric-cues.fixture.json");
/// The `Playback` arrangement of the fixture song: one entry per Playback section, as a position in the file's groups.
const ARRANGEMENT: [usize; 23] = [
    0, 1, 2, 3, 4, 5, 6, 7, 4, 8, 5, 9, 10, 10, 11, 12, 13, 14, 8, 8, 15, 16, 17,
];

struct Fixture {
    cues: Vec<LyricCue>,
    sections: Vec<Section>,
    groups: Vec<(String, usize)>,
}

fn fixture() -> Fixture {
    let v: Value = serde_json::from_str(FIXTURE).unwrap();
    Fixture {
        cues: v["cues"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| LyricCue {
                time_s: c["time_s"].as_f64().unwrap(),
                slide: c["slide"].as_u64().unwrap() as u32,
            })
            .collect(),
        sections: v["sections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| Section {
                name: s["name"].as_str().unwrap().to_string(),
                start: s["start_s"].as_f64().unwrap(),
                end: s["end_s"].as_f64().unwrap(),
            })
            .collect(),
        groups: v["file_groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| {
                (
                    g["name"].as_str().unwrap().to_string(),
                    g["cues"].as_u64().unwrap() as usize,
                )
            })
            .collect(),
    }
}

fn oh_my_soul() -> SyntheticSong {
    let f = fixture();
    let mut song = SyntheticSong::new("Oh My Soul").id(1001).seed(1);
    for (i, (name, cues)) in f.groups.iter().enumerate() {
        let slides: Vec<Option<&str>> = (0..*cues)
            .map(|_| if i == 0 { None } else { Some("la") })
            .collect();
        song = song.group_with_blanks(name, &slides);
    }
    song.arrangement("Playback", &ARRANGEMENT).selected(0)
}

/// A small song with no cues: Count Off, a 2-slide verse and a 3-slide chorus; the Playback arrangement plays them once.
fn plain_song(title: &str, id: u64, seed: u32) -> SyntheticSong {
    SyntheticSong::new(title)
        .id(id)
        .seed(seed)
        .group_with_blanks("", &[None])
        .group("Verse", &["v1", "v2"])
        .group("Chorus", &["c1", "c2", "c3"])
        .arrangement("Playback", &[0, 1, 2])
        .selected(0)
}

fn plain_sections() -> Vec<Section> {
    vec![
        Section {
            name: "Count Off".into(),
            start: 0.0,
            end: 4.0,
        },
        Section {
            name: "Verse".into(),
            start: 4.0,
            end: 12.0,
        },
        Section {
            name: "Chorus".into(),
            start: 12.0,
            end: 24.0,
        },
    ]
}

struct Rig {
    host: Arc<FakeHost>,
    api: Arc<FakeApi>,
    playlist: String,
    store: Arc<MemoryBindingStore>,
    preparer: Preparer,
}

/// A playlist `Sunday` with `Oh My Soul` (item 0), `Second Song` (item 1) and `Plain Song` (item 2), all `Playback` copies.
fn rig() -> Rig {
    rig_with(oh_my_soul())
}

/// [`rig`] with another file for `Oh My Soul` (one with slides added, say).
fn rig_with(oh_my_soul: SyntheticSong) -> Rig {
    let host = Arc::new(
        FakeHost::new()
            .with_library("Playback")
            .with_file("Playback", "Oh My Soul.pro", &oh_my_soul.build())
            .with_file(
                "Playback",
                "Second Song.pro",
                &plain_song("Second Song", 1002, 2).build(),
            )
            .with_file(
                "Playback",
                "Plain Song.pro",
                &plain_song("Plain Song", 1003, 3).build(),
            ),
    );
    let api = Arc::new(FakeApi::new(host.clone(), Loading::Immediately));
    let item = |name: &str| {
        FakeApi::presentation_item(
            &entry_uuid("Playback", &format!("{name}.pro")),
            name,
            "Playback",
            0.0,
        )
    };
    let playlist = api.add_playlist(
        "Sunday",
        None,
        vec![item("Oh My Soul"), item("Second Song"), item("Plain Song")],
    );
    let store = Arc::new(MemoryBindingStore::default());
    let preparer = Preparer::new(store.clone());
    Rig {
        host,
        api,
        playlist,
        store,
        preparer,
    }
}

fn inputs(with_cues: bool) -> Vec<SongInput> {
    let f = fixture();
    vec![
        SongInput {
            key: "oms".into(),
            title: "Oh My Soul".into(),
            playback_song_id: Some(11),
            content_id: Some(1001),
            sections: f.sections.clone(),
            cues: if with_cues {
                f.cues.clone()
            } else {
                Vec::new()
            },
        },
        SongInput {
            key: "second".into(),
            title: "Second Song".into(),
            playback_song_id: Some(12),
            content_id: Some(1002),
            sections: plain_sections(),
            cues: Vec::new(),
        },
        SongInput {
            key: "plain".into(),
            title: "Plain Song".into(),
            playback_song_id: Some(13),
            content_id: Some(1003),
            sections: plain_sections(),
            cues: Vec::new(),
        },
    ]
}

fn view(status: &SongStatus) -> SongView {
    SongView {
        key: status.key.clone(),
        title: status.title.clone(),
        readiness_text: status.readiness.text(),
        prepared: status.prepared.clone(),
    }
}

/// The three songs, found in the active playlist (as when the person has not used the Presenter Link on this computer).
async fn songs(rig: &Rig, with_cues: bool) -> Vec<SongView> {
    rig.api.focus_playlist(&rig.playlist, 0);
    let statuses = rig
        .preparer
        .prepare(&*rig.host, &*rig.api, &inputs(with_cues), true)
        .await;
    rig.api.clear_calls();
    statuses.iter().map(view).collect()
}

struct Clock {
    now: f64,
    position: f64,
}

impl Clock {
    fn start(position: f64) -> Self {
        Self { now: 0.0, position }
    }
}

/// Runs `secs` seconds of 100 ms steps. While playing the position moves with the clock; otherwise it stays.
async fn advance(
    follower: &mut Follower,
    api: &FakeApi,
    song: &SongView,
    clock: &mut Clock,
    secs: f64,
    playing: bool,
    offset_ms: i32,
) {
    let steps = (secs * 10.0).round() as usize;
    for _ in 0..steps {
        clock.now += 0.1;
        if playing {
            clock.position += 0.1;
        }
        let input = Input {
            now: clock.now,
            enabled: true,
            offset_ms,
            playback: Some(PlaybackNow {
                position: clock.position,
                playing,
            }),
            song: Some(song),
        };
        follower.step(api, &input).await;
    }
}

/// `item:<index>` and `slide:<n>`, in order.
fn log(api: &FakeApi) -> Vec<String> {
    api.triggers()
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

// ---- the scripted run ---------------------------------------------------------------------------------------------------

#[tokio::test]
async fn the_real_song_is_followed_through_play_pause_seek_loop_stop_new_song_and_play_from_the_middle(
) {
    let rig = rig();
    let all = songs(&rig, true).await;
    let (oms, second) = (&all[0], &all[1]);
    assert_eq!(oms.readiness_text, FOLLOWING_BY_CUES);
    let mut follower = Follower::new(Timing::default());
    let mut clock = Clock::start(0.0);

    // Play from the top: the item (cue 0, the blank Count Off) and then the cues at 3.53 s and 8.17 s (flat slides 1 and 2).
    advance(&mut follower, &rig.api, oms, &mut clock, 9.0, true, 0).await;
    assert_eq!(log(&rig.api), strings(&["item:0", "slide:1", "slide:2"]));
    assert_eq!(rig.api.raw_on_screen(), Some(2));
    assert_eq!(follower.handle().status().state, FollowState::Following);

    // Pause: nothing is triggered for as long as it lasts, and the slide stays.
    advance(&mut follower, &rig.api, oms, &mut clock, 5.0, false, 0).await;
    assert_eq!(log(&rig.api).len(), 3);
    assert_eq!(rig.api.raw_on_screen(), Some(2));

    // A seek back into the verse while paused is followed on resume (the cue at 11.12 s is flat slide 3).
    clock.position = 14.0;
    advance(&mut follower, &rig.api, oms, &mut clock, 2.0, false, 0).await;
    assert_eq!(log(&rig.api).len(), 3, "still paused");
    advance(&mut follower, &rig.api, oms, &mut clock, 14.0, true, 0).await;
    // 3 at once, then 15.13 s, 19.33 s, 23.12 s and 27.48 s (the second Verse 1 starts at 28.23 s).
    assert_eq!(
        log(&rig.api),
        strings(&[
            "item:0", "slide:1", "slide:2", "slide:3", "slide:4", "slide:5", "slide:6", "slide:7"
        ])
    );
    assert_eq!(rig.api.raw_on_screen(), Some(7));

    // Play on to 40 s (cues at 31.18, 35.5 and 39.3 s), then Playback loops the section back to 28.3 s.
    let before = log(&rig.api).len();
    let to_forty = 40.0 - clock.position;
    advance(&mut follower, &rig.api, oms, &mut clock, to_forty, true, 0).await;
    assert_eq!(
        &log(&rig.api)[before..],
        strings(&["slide:8", "slide:9", "slide:10"])
    );
    clock.position = 28.3;
    advance(&mut follower, &rig.api, oms, &mut clock, 5.0, true, 0).await;
    assert_eq!(
        &log(&rig.api)[before + 3..],
        strings(&["slide:7", "slide:8"]),
        "the loop is followed back to the section's first slide and on"
    );

    // Stop (the position goes back to the top and nothing plays): nothing is triggered, the last slide stays.
    let count = log(&rig.api).len();
    clock.position = 0.0;
    advance(&mut follower, &rig.api, oms, &mut clock, 4.0, false, 0).await;
    assert_eq!(log(&rig.api).len(), count);
    assert_eq!(rig.api.raw_on_screen(), Some(8));

    // A new song starts: its playlist item is shown (item 1) and the bars take over from there (flat slide 1 at 4 s).
    let mut clock = Clock::start(0.0);
    advance(&mut follower, &rig.api, second, &mut clock, 5.0, true, 0).await;
    assert_eq!(&log(&rig.api)[count..], strings(&["item:1", "slide:1"]));
    assert_eq!(rig.api.active_file().as_deref(), Some("Second Song"));

    // Play from the middle: the item first, then the slide wanted at 90 s at once (the second Chorus: cue 11 at flat 24).
    let count = log(&rig.api).len();
    let mut clock = Clock::start(90.0);
    advance(&mut follower, &rig.api, oms, &mut clock, 0.3, true, 0).await;
    assert_eq!(&log(&rig.api)[count..], strings(&["item:0", "slide:24"]));
    assert_eq!(rig.api.raw_on_screen(), Some(11));
}

#[tokio::test]
async fn every_cue_of_the_real_song_shows_the_raw_cue_playback_meant() {
    // Played right through, each trigger's flat number must show exactly the raw cue Playback's cue named. The fake works the
    // raw index out from the file's own arrangement, independently of the schedule.
    let rig = rig();
    let all = songs(&rig, true).await;
    let f = fixture();
    let mut follower = Follower::new(Timing::default());
    let mut clock = Clock::start(0.0);
    let mut raws: Vec<usize> = Vec::new();
    for cue in &f.cues {
        let to = cue.time_s + 0.05;
        let secs = to - clock.position;
        advance(
            &mut follower,
            &rig.api,
            &all[0],
            &mut clock,
            (secs * 10.0).ceil() / 10.0,
            true,
            0,
        )
        .await;
        raws.push(rig.api.raw_on_screen().unwrap());
    }
    let wanted: Vec<usize> = f.cues.iter().map(|c| c.slide as usize).collect();
    assert_eq!(raws, wanted);
}

#[tokio::test]
async fn a_song_with_no_cues_is_followed_by_bars_and_says_so() {
    let rig = rig();
    let all = songs(&rig, true).await;
    let plain = &all[2];
    assert_eq!(plain.readiness_text, FOLLOWING_BY_BARS);
    let mut follower = Follower::new(Timing::default());
    let mut clock = Clock::start(0.0);
    advance(&mut follower, &rig.api, plain, &mut clock, 22.0, true, 0).await;
    // Verse (4 s to 12 s): two slides at 4 s and 8 s. Chorus (12 s to 24 s): three slides at 12 s, 16 s and 20 s.
    assert_eq!(
        log(&rig.api),
        strings(&["item:2", "slide:1", "slide:2", "slide:3", "slide:4", "slide:5"])
    );
    assert_eq!(rig.api.raw_on_screen(), Some(5), "the last chorus slide");
}

#[tokio::test]
async fn a_cue_less_oh_my_soul_goes_to_bars_too() {
    let rig = rig();
    let all = songs(&rig, false).await;
    assert_eq!(all[0].readiness_text, FOLLOWING_BY_BARS);
    let mut follower = Follower::new(Timing::default());
    let mut clock = Clock::start(4.5);
    advance(&mut follower, &rig.api, &all[0], &mut clock, 0.2, true, 0).await;
    // Intro starts at 4.03 s: its first slide (flat 1) is wanted at 4.5 s.
    assert_eq!(log(&rig.api), strings(&["item:0", "slide:1"]));
}

#[tokio::test]
async fn the_offset_moves_every_slide_change() {
    for (offset_ms, first_at) in [(0, 4.0), (500, 3.5), (-500, 4.5), (2000, 2.0)] {
        let rig = rig();
        let all = songs(&rig, true).await;
        let plain = &all[2];
        let mut follower = Follower::new(Timing::default());
        let mut clock = Clock::start(0.0);
        // Up to just before the slide: only the item.
        advance(
            &mut follower,
            &rig.api,
            plain,
            &mut clock,
            first_at - 0.15,
            true,
            offset_ms,
        )
        .await;
        assert_eq!(
            log(&rig.api),
            strings(&["item:2"]),
            "offset {offset_ms}: too early"
        );
        advance(
            &mut follower,
            &rig.api,
            plain,
            &mut clock,
            0.3,
            true,
            offset_ms,
        )
        .await;
        assert_eq!(
            log(&rig.api),
            strings(&["item:2", "slide:1"]),
            "offset {offset_ms}: the slide is due at {first_at} s"
        );
    }
}

// ---- the operator ------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_click_in_propresenter_holds_the_song_until_the_next_section_and_then_it_follows_again() {
    let rig = rig();
    let all = songs(&rig, true).await;
    let oms = &all[0];
    let mut follower = Follower::new(Timing::default());
    let mut clock = Clock::start(10.0);
    advance(&mut follower, &rig.api, oms, &mut clock, 2.0, true, 0).await;
    assert_eq!(log(&rig.api), strings(&["item:0", "slide:2", "slide:3"]));

    // A person clicks another slide of the song.
    rig.api.click_slide(9);
    advance(&mut follower, &rig.api, oms, &mut clock, 0.5, true, 0).await;
    let status = follower.handle().status();
    assert_eq!(status.state, FollowState::Held);
    assert_eq!(status.hold_reason.as_deref(), Some(HOLD_REASON));
    assert_eq!(
        HOLD_REASON,
        "Paused: someone changed the slide in ProPresenter"
    );
    // The rest of the verse goes by (cues at 15.13, 19.33, 23.12 s): nothing is triggered, the person's slide stays.
    advance(&mut follower, &rig.api, oms, &mut clock, 14.0, true, 0).await;
    assert_eq!(log(&rig.api).len(), 3);
    assert_eq!(rig.api.on_screen().map(|s| s.1), Some(9));
    assert_eq!(follower.handle().status().state, FollowState::Held);
    // The second Verse 1 (its first cue is at 27.48 s) ends the hold and its first slide is shown.
    advance(&mut follower, &rig.api, oms, &mut clock, 2.0, true, 0).await;
    assert_eq!(log(&rig.api).last().map(String::as_str), Some("slide:7"));
    assert_eq!(log(&rig.api).len(), 4);
    let status = follower.handle().status();
    assert_eq!(status.state, FollowState::Following);
    assert_eq!(status.hold_reason, None);
    assert_eq!(rig.api.raw_on_screen(), Some(7));
}

#[tokio::test]
async fn a_hold_survives_a_loop_of_its_section_and_a_seek_to_another_section_ends_it() {
    let rig = rig();
    let all = songs(&rig, true).await;
    let oms = &all[0];
    let mut follower = Follower::new(Timing::default());
    let mut clock = Clock::start(13.0);
    advance(&mut follower, &rig.api, oms, &mut clock, 1.0, true, 0).await;
    rig.api.click_slide(1);
    advance(&mut follower, &rig.api, oms, &mut clock, 0.6, true, 0).await;
    assert_eq!(follower.handle().status().state, FollowState::Held);
    let count = log(&rig.api).len();
    // Playback loops this section: 25 s back to 13 s. Still the same section.
    clock.position = 13.0;
    advance(&mut follower, &rig.api, oms, &mut clock, 8.0, true, 0).await;
    assert_eq!(log(&rig.api).len(), count);
    assert_eq!(follower.handle().status().state, FollowState::Held);
    // A jump into the Intro is another section: the hold ends and the Intro's slide is shown.
    clock.position = 5.0;
    advance(&mut follower, &rig.api, oms, &mut clock, 0.2, true, 0).await;
    assert_eq!(follower.handle().status().state, FollowState::Following);
    assert_eq!(log(&rig.api).last().map(String::as_str), Some("slide:1"));
}

#[tokio::test]
async fn another_song_opened_in_propresenter_is_a_click_too() {
    let rig = rig();
    let all = songs(&rig, true).await;
    let mut follower = Follower::new(Timing::default());
    let mut clock = Clock::start(10.0);
    advance(&mut follower, &rig.api, &all[0], &mut clock, 2.0, true, 0).await;
    assert!(rig.api.open_presentation("Second Song"));
    advance(&mut follower, &rig.api, &all[0], &mut clock, 14.0, true, 0).await;
    assert_eq!(follower.handle().status().state, FollowState::Held);
    assert_eq!(rig.api.active_file().as_deref(), Some("Second Song"));
    assert_eq!(rig.api.slides_triggered(), vec![2, 3]);
    // At the next section the song's own presentation comes back (the item first), then its slide.
    advance(&mut follower, &rig.api, &all[0], &mut clock, 2.0, true, 0).await;
    assert_eq!(
        &log(&rig.api)[3..],
        strings(&["item:0", "slide:7"]),
        "the wrong presentation was on screen"
    );
    assert_eq!(rig.api.active_file().as_deref(), Some("Oh My Soul"));
}

#[tokio::test]
async fn a_wrong_presentation_is_shown_again_at_most_once_every_two_seconds() {
    let rig = rig();
    let all = songs(&rig, true).await;
    // The click reading is off here so the test sees only the check before a slide is triggered.
    let timing = Timing {
        poll_s: 1.0e9,
        ..Timing::default()
    };
    let mut follower = Follower::new(timing);
    let mut clock = Clock::start(61.0);
    advance(&mut follower, &rig.api, &all[0], &mut clock, 0.2, true, 0).await;
    assert_eq!(log(&rig.api), strings(&["item:0", "slide:15"]));
    // Someone opens another song, and ProPresenter ignores the item triggers from now on.
    assert!(rig.api.open_presentation("Second Song"));
    rig.api.ignore_item_triggers(true);
    rig.api.clear_calls();
    // The wanted slide changes at 62.25 s, 63.2 s, 64.57 s and 68.62 s. For six seconds the item is shown again no more than
    // once per two seconds (at 2.1 s, 4.1 s and 6.1 s of this run), and no slide is triggered into the wrong song.
    advance(&mut follower, &rig.api, &all[0], &mut clock, 6.0, true, 0).await;
    let items = rig.api.item_triggers();
    assert!((2..=3).contains(&items), "{:?}", log(&rig.api));
    assert!(rig.api.slides_triggered().is_empty(), "{:?}", log(&rig.api));
    // Between the tries nothing is asked, so it is not a loop of reads either.
    let reads = rig
        .api
        .calls()
        .iter()
        .filter(|c| c.starts_with("active_presentation"))
        .count();
    assert!(reads <= 10, "{reads} reads: {:?}", rig.api.calls());
    assert_eq!(rig.api.active_file().as_deref(), Some("Second Song"));
    assert_eq!(follower.handle().status().state, FollowState::Following);
}

// ---- failures ----------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn five_failed_triggers_pause_following_until_try_again() {
    let rig = rig();
    let all = songs(&rig, true).await;
    let mut follower = Follower::new(Timing::default());
    let mut clock = Clock::start(0.0);
    advance(&mut follower, &rig.api, &all[0], &mut clock, 4.0, true, 0).await;
    assert_eq!(log(&rig.api), strings(&["item:0", "slide:1"]));
    rig.api.fail_triggers(Some(ApiError::Status(500)));
    // The cue at 8.17 s cannot be shown. It is tried once a second, five times, and then not any more.
    advance(&mut follower, &rig.api, &all[0], &mut clock, 15.0, true, 0).await;
    // One earlier success, then five tries a second apart (slide 2 until the cue at 11.12 s makes slide 3 the wanted one).
    assert_eq!(rig.api.slides_triggered(), vec![1, 2, 2, 2, 3, 3]);
    let status = follower.handle().status();
    assert_eq!(status.state, FollowState::Paused);
    assert!(status.message.starts_with("Paused:"), "{}", status.message);
    assert!(status.message.contains("Try again"), "{}", status.message);
    // Nothing more is asked of ProPresenter while paused, not even to read.
    rig.api.clear_calls();
    advance(&mut follower, &rig.api, &all[0], &mut clock, 10.0, true, 0).await;
    assert!(rig.api.calls().is_empty(), "{:?}", rig.api.calls());

    // Try again clears the pause: the wanted slide is shown at the next step.
    rig.api.fail_triggers(None);
    follower.handle().try_again();
    advance(&mut follower, &rig.api, &all[0], &mut clock, 0.1, true, 0).await;
    assert_eq!(follower.handle().status().state, FollowState::Following);
    assert_eq!(rig.api.slides_triggered().len(), 1);
    assert_eq!(
        rig.api.raw_on_screen(),
        Some(7),
        "29 s: the verse's second half"
    );
}

#[tokio::test]
async fn a_failure_pause_ends_with_the_next_song_and_a_good_trigger_forgets_earlier_failures() {
    let rig = rig();
    let all = songs(&rig, true).await;
    let mut follower = Follower::new(Timing::default());
    let mut clock = Clock::start(0.0);
    advance(&mut follower, &rig.api, &all[0], &mut clock, 1.0, true, 0).await;
    // Four failures, then it works again: not paused.
    rig.api
        .fail_triggers(Some(ApiError::NoAnswer("it is not there".into())));
    advance(&mut follower, &rig.api, &all[0], &mut clock, 6.0, true, 0).await;
    assert_eq!(
        follower.handle().status().state,
        FollowState::Following,
        "four failures do not pause"
    );
    rig.api.fail_triggers(None);
    advance(&mut follower, &rig.api, &all[0], &mut clock, 2.0, true, 0).await;
    assert_eq!(follower.handle().status().state, FollowState::Following);
    // Five failures in a row pause; the next song starts following again.
    rig.api.fail_triggers(Some(ApiError::Status(503)));
    advance(&mut follower, &rig.api, &all[0], &mut clock, 12.0, true, 0).await;
    assert_eq!(follower.handle().status().state, FollowState::Paused);
    rig.api.fail_triggers(None);
    let mut clock = Clock::start(0.0);
    advance(&mut follower, &rig.api, &all[1], &mut clock, 0.3, true, 0).await;
    assert_eq!(follower.handle().status().state, FollowState::Following);
    assert_eq!(log(&rig.api).last().map(String::as_str), Some("item:1"));
}

// ---- off, and nothing to follow ------------------------------------------------------------------------------------------

#[tokio::test]
async fn with_the_switch_off_the_fake_sees_no_call_at_all() {
    let rig = rig();
    let all = songs(&rig, true).await;
    let mut follower = Follower::new(Timing::default());
    let mut clock = Clock::start(0.0);
    for _ in 0..300 {
        clock.now += 0.1;
        clock.position += 0.1;
        let input = Input {
            now: clock.now,
            enabled: false,
            offset_ms: 0,
            playback: Some(PlaybackNow {
                position: clock.position,
                playing: true,
            }),
            song: Some(&all[0]),
        };
        follower.step(&*rig.api, &input).await;
    }
    assert!(rig.api.calls().is_empty(), "{:?}", rig.api.calls());
    assert!(rig.api.triggers().is_empty());
    assert_eq!(follower.handle().status().state, FollowState::Off);
    assert_eq!(follower.handle().status().message, "Switch is off");
}

#[tokio::test]
async fn a_song_that_cannot_be_followed_and_no_song_at_all_send_nothing() {
    let rig = rig();
    let statuses = rig
        .preparer
        .prepare(&*rig.host, &*rig.api, &inputs(true), true)
        .await; // no active playlist: not found
    rig.api.clear_calls();
    let not_there = view(&statuses[0]);
    assert_eq!(not_there.prepared, None);
    let mut follower = Follower::new(Timing::default());
    let mut clock = Clock::start(5.0);
    advance(
        &mut follower,
        &rig.api,
        &not_there,
        &mut clock,
        3.0,
        true,
        0,
    )
    .await;
    assert!(rig.api.calls().is_empty());
    let status = follower.handle().status();
    assert_eq!(status.state, FollowState::NotFollowed);
    assert_eq!(status.message, NOT_IN_PLAYLIST);
    assert_eq!(status.song_title.as_deref(), Some("Oh My Soul"));
    // Playback with no song, and a song that is not in the plan.
    for (playback, song) in [
        (None, None),
        (
            Some(PlaybackNow {
                position: 1.0,
                playing: true,
            }),
            None,
        ),
    ] {
        follower
            .step(
                &*rig.api,
                &Input {
                    now: 99.0,
                    enabled: true,
                    offset_ms: 0,
                    playback,
                    song,
                },
            )
            .await;
        assert_eq!(follower.handle().status().state, FollowState::Idle);
    }
    assert!(rig.api.calls().is_empty());
}

#[tokio::test]
async fn a_song_that_is_loaded_but_not_playing_shows_nothing_yet() {
    let rig = rig();
    let all = songs(&rig, true).await;
    let mut follower = Follower::new(Timing::default());
    let mut clock = Clock::start(0.0);
    advance(&mut follower, &rig.api, &all[0], &mut clock, 2.0, false, 0).await;
    assert!(rig.api.triggers().is_empty());
    assert_eq!(follower.handle().status().state, FollowState::Ready);
}

// ---- finding and judging the songs --------------------------------------------------------------------------------------

#[tokio::test]
async fn songs_are_found_in_the_active_playlist_and_the_binding_is_remembered() {
    let rig = rig();
    rig.api.focus_playlist(&rig.playlist, 1);
    let statuses = rig
        .preparer
        .prepare(&*rig.host, &*rig.api, &inputs(true), true)
        .await;
    let readiness: Vec<String> = statuses.iter().map(|s| s.readiness.text()).collect();
    assert_eq!(
        readiness,
        vec![FOLLOWING_BY_CUES, FOLLOWING_BY_BARS, FOLLOWING_BY_BARS]
    );
    let remembered = rig.store.all();
    assert_eq!(remembered.len(), 3);
    assert_eq!(remembered[&1002].item_index, 1);
    assert_eq!(remembered[&1002].file, "Second Song.pro");
    assert_eq!(remembered[&1001].playlist_id, rig.playlist);
    assert_eq!(remembered[&1001].group_cues.iter().sum::<usize>(), 54);
}

#[tokio::test]
async fn a_remembered_binding_needs_no_active_playlist_and_follows_the_item_when_it_moves() {
    let rig = rig();
    let only: BTreeSet<String> = ["Oh My Soul.pro", "Second Song.pro", "Plain Song.pro"]
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        rig.preparer
            .remember_playlist(&*rig.host, &*rig.api, &rig.playlist, &only)
            .await
            .unwrap(),
        3
    );
    assert_eq!(rig.store.all().len(), 3);
    // The Presenter Link remembered them: nothing is asked of the active playlist.
    let statuses = rig
        .preparer
        .prepare(&*rig.host, &*rig.api, &inputs(true), true)
        .await;
    assert!(statuses.iter().all(|s| s.prepared.is_some()));
    assert!(!rig.api.calls().iter().any(|c| c == "active_playlist"));
    // A header put in front moves every item by one: the items are found again where they are now.
    let mut items: Vec<Value> = rig.api.stored(&rig.playlist);
    items.insert(0, FakeApi::header_item("Worship", 0.5));
    stagepilot_propresenter::link::api::ProPresenterApi::put_playlist(
        &*rig.api,
        &rig.playlist,
        &items,
    )
    .await
    .unwrap();
    let statuses = rig
        .preparer
        .prepare(&*rig.host, &*rig.api, &inputs(true), true)
        .await;
    let moved = statuses[1].prepared.as_ref().unwrap();
    assert_eq!(moved.binding.item_index, 2);
    // ...and the follower shows the item at the index it has now.
    let all: Vec<SongView> = statuses.iter().map(view).collect();
    rig.api.clear_calls();
    let mut follower = Follower::new(Timing::default());
    let mut clock = Clock::start(0.0);
    advance(&mut follower, &rig.api, &all[1], &mut clock, 0.2, true, 0).await;
    assert_eq!(log(&rig.api), strings(&["item:2"]));
}

#[tokio::test]
async fn a_song_that_is_not_in_the_playlist_or_the_link_is_not_ready_is_not_followed() {
    let rig = rig();
    // Not in the active playlist and nothing remembered.
    let statuses = rig
        .preparer
        .prepare(&*rig.host, &*rig.api, &inputs(true), true)
        .await;
    assert!(statuses
        .iter()
        .all(|s| s.readiness == Readiness::NotInPlaylist));
    // The link is not ready: nothing is read from ProPresenter at all.
    rig.api.clear_calls();
    let statuses = rig
        .preparer
        .prepare(&*rig.host, &*rig.api, &inputs(true), false)
        .await;
    assert!(statuses
        .iter()
        .all(|s| s.readiness.text() == LINK_NOT_READY));
    assert!(rig.api.calls().is_empty());
    // ProPresenter's API is down: the remembered songs cannot be checked.
    let only: BTreeSet<String> = ["Oh My Soul.pro".to_string()].into_iter().collect();
    rig.preparer
        .remember_playlist(&*rig.host, &*rig.api, &rig.playlist, &only)
        .await
        .unwrap();
    rig.api.set_down(true);
    let statuses = rig
        .preparer
        .prepare(&*rig.host, &*rig.api, &inputs(true), true)
        .await;
    assert_eq!(statuses[0].readiness, Readiness::LinkNotReady);
    // A song with no MultiTracks number cannot be in the playlist as a Playback copy; one Playback does not have says so.
    rig.api.set_down(false);
    let mut odd = inputs(true);
    odd[1].content_id = None;
    odd[2].playback_song_id = None;
    let statuses = rig
        .preparer
        .prepare(&*rig.host, &*rig.api, &odd, true)
        .await;
    assert_eq!(statuses[1].readiness, Readiness::NotInPlaylist);
    assert_eq!(statuses[2].readiness, Readiness::NotInPlayback);
}

#[tokio::test]
async fn an_arrangement_changed_after_the_song_was_added_is_not_followed() {
    let rig = rig();
    let only: BTreeSet<String> = ["Oh My Soul.pro".to_string(), "Second Song.pro".to_string()]
        .into_iter()
        .collect();
    rig.preparer
        .remember_playlist(&*rig.host, &*rig.api, &rig.playlist, &only)
        .await
        .unwrap();
    // Someone edits the song in ProPresenter: the verse is played twice now.
    let edited = SyntheticSong::new("Second Song")
        .id(1002)
        .seed(2)
        .group_with_blanks("", &[None])
        .group("Verse", &["v1", "v2"])
        .group("Chorus", &["c1", "c2", "c3"])
        .arrangement("Playback", &[0, 1, 2, 1])
        .selected(0)
        .build();
    let host = Arc::new(
        FakeHost::new()
            .with_library("Playback")
            .with_file("Playback", "Oh My Soul.pro", &oh_my_soul().build())
            .with_file("Playback", "Second Song.pro", &edited),
    );
    let preparer = Preparer::new(rig.store.clone());
    let statuses = preparer
        .prepare(&*host, &*rig.api, &inputs(true), true)
        .await;
    assert_eq!(statuses[0].readiness, Readiness::Cues);
    assert_eq!(statuses[1].readiness.text(), ARRANGEMENT_CHANGED);
    assert_eq!(statuses[1].prepared, None);
    // The same when another arrangement is selected.
    let other = SyntheticSong::new("Oh My Soul")
        .id(1001)
        .seed(1)
        .group_with_blanks("", &[None])
        .group("Verse", &["v1"])
        .arrangement("Playback", &[0, 1])
        .arrangement("Full", &[0, 1, 1])
        .selected(1)
        .build();
    let host = Arc::new(FakeHost::new().with_library("Playback").with_file(
        "Playback",
        "Oh My Soul.pro",
        &other,
    ));
    let preparer = Preparer::new(rig.store.clone());
    let statuses = preparer
        .prepare(&*host, &*rig.api, &inputs(true), true)
        .await;
    assert_eq!(statuses[0].readiness.text(), ARRANGEMENT_CHANGED);
}

#[tokio::test]
async fn cues_that_do_not_fit_the_file_are_said_in_plain_words_and_the_song_goes_by_bars() {
    let rig = rig();
    rig.api.focus_playlist(&rig.playlist, 0);
    let mut odd = inputs(true);
    // A 2 Line template: the cues stop at slide 30 while the file has 54 slides.
    odd[0].cues.retain(|c| c.slide <= 30);
    let statuses = rig
        .preparer
        .prepare(&*rig.host, &*rig.api, &odd, true)
        .await;
    assert_eq!(
        statuses[0].readiness.text(),
        "Cues do not match the file (cues go to slide 30, file has 54 cues)"
    );
    assert_eq!(statuses[0].readiness.following(), Some("bars"));
    assert!(statuses[0].prepared.is_some());
    // No cues at all: bars, with the advice for Playback.
    let statuses = rig
        .preparer
        .prepare(&*rig.host, &*rig.api, &inputs(false), true)
        .await;
    assert_eq!(statuses[0].readiness.text(), FOLLOWING_BY_BARS);
    assert_eq!(
        statuses[0].readiness.advice().as_deref(),
        Some("No cues: in Playback choose a ProPresenter 1 Line Template for this song")
    );
    // Playback gives no sections, or sections that no longer fit the copy.
    let mut odd = inputs(true);
    odd[1].sections.clear();
    odd[2].sections.pop();
    let statuses = rig
        .preparer
        .prepare(&*rig.host, &*rig.api, &odd, true)
        .await;
    assert_eq!(statuses[1].readiness, Readiness::NoSections);
    assert_eq!(statuses[2].readiness, Readiness::SectionsChanged);
}

// ---- extra slides in the song file --------------------------------------------------------------------------------------------

/// The fixture song with blank background slides added: `at_start` as a new first group that the `Playback` arrangement does not
/// play, `in_count_off` more in the Count Off group in front of its own blank slide, `in_intro` at the start of the Intro group.
fn oh_my_soul_with_extras(at_start: usize, in_count_off: usize, in_intro: usize) -> SyntheticSong {
    let f = fixture();
    let mut song = SyntheticSong::new("Oh My Soul").id(1001).seed(1);
    let offset = usize::from(at_start > 0);
    if at_start > 0 {
        song = song.group_with_key(900, "Background", &vec![None; at_start]);
    }
    for (i, (name, cues)) in f.groups.iter().enumerate() {
        let mut slides: Vec<Option<&str>> = (0..*cues)
            .map(|_| if i == 0 { None } else { Some("la") })
            .collect();
        let extra = match i {
            0 => in_count_off,
            1 => in_intro,
            _ => 0,
        };
        for _ in 0..extra {
            slides.insert(0, None);
        }
        song = song.group_with_key(i as u32, name, &slides);
    }
    let arrangement: Vec<usize> = ARRANGEMENT.iter().map(|g| g + offset).collect();
    song.arrangement("Playback", &arrangement).selected(0)
}

/// Plays the song right through, one check just after each Playback cue: the trigger log and, for each cue, the RAW cue index
/// of the file that is on screen (the fake works it out from the file's own arrangement).
async fn play_through(song: SyntheticSong) -> (Vec<String>, Vec<usize>, SongView) {
    let rig = rig_with(song);
    let all = songs(&rig, true).await;
    assert!(
        all[0].readiness_text.starts_with(FOLLOWING_BY_CUES),
        "{}",
        all[0].readiness_text
    );
    let f = fixture();
    let mut follower = Follower::new(Timing::default());
    let mut clock = Clock::start(0.0);
    let mut raws = Vec::new();
    for cue in &f.cues {
        let secs = cue.time_s + 0.05 - clock.position;
        advance(
            &mut follower,
            &rig.api,
            &all[0],
            &mut clock,
            (secs * 10.0).ceil() / 10.0,
            true,
            0,
        )
        .await;
        raws.push(rig.api.raw_on_screen().unwrap());
    }
    (log(&rig.api), raws, all[0].clone())
}

#[tokio::test]
async fn the_fixture_with_two_extra_background_slides_at_the_start_triggers_the_exact_same_lyric_slides(
) {
    let f = fixture();
    let (plain_log, plain_raws, _) = play_through(oh_my_soul()).await;
    assert_eq!(
        plain_raws,
        f.cues.iter().map(|c| c.slide as usize).collect::<Vec<_>>(),
        "nothing added: the raw cue Playback named"
    );
    assert_eq!(
        &plain_log[..4],
        &strings(&["item:0", "slide:1", "slide:2", "slide:3"])[..]
    );

    // Two blank slides in front of the Count Off's own: every slide of the arrangement is two further on, and so is every raw cue.
    let (log_in_count_off, raws, view) = play_through(oh_my_soul_with_extras(0, 2, 0)).await;
    assert_eq!(
        &log_in_count_off[..4],
        &strings(&["item:0", "slide:3", "slide:4", "slide:5"])[..],
        "Playback's slide 1 is the Intro's first slide, now the file's slide 3"
    );
    let want: Vec<String> = plain_log
        .iter()
        .map(|entry| match entry.strip_prefix("slide:") {
            Some(n) => format!("slide:{}", n.parse::<usize>().unwrap() + 2),
            None => entry.clone(),
        })
        .collect();
    assert_eq!(log_in_count_off, want);
    let want_raws: Vec<usize> = f.cues.iter().map(|c| c.slide as usize + 2).collect();
    assert_eq!(raws, want_raws);
    assert!(
        view.readiness_text.contains("aligned with extra slides"),
        "{}",
        view.readiness_text
    );

    // Two blank slides as a new first group the arrangement does not play: the numbers ProPresenter has do not move, the file's
    // own cue indexes do.
    let (log_new_group, raws, view) = play_through(oh_my_soul_with_extras(2, 0, 0)).await;
    assert_eq!(log_new_group, plain_log);
    assert_eq!(raws, want_raws);
    assert_eq!(
        view.readiness_text,
        "Following by Playback cues, aligned with extra slides (check the first slide of Intro)"
    );
    let prepared = view.prepared.as_ref().unwrap();
    assert_eq!(prepared.schedule.alignment.as_ref().unwrap().extras, 2);
    assert_eq!(
        prepared.schedule.source,
        stagepilot_propresenter::follow::Source::Cues
    );

    // Two blank slides at the start of the Intro, in front of the first lyric slide, are in the middle of the arrangement.
    let (log_in_intro, raws, _) = play_through(oh_my_soul_with_extras(0, 0, 2)).await;
    assert_eq!(
        &log_in_intro[..4],
        &strings(&["item:0", "slide:3", "slide:4", "slide:5"])[..]
    );
    assert_eq!(raws, want_raws);
}

#[tokio::test]
async fn a_binding_saved_before_the_hash_changed_is_rebuilt_silently() {
    let rig = rig();
    let only: BTreeSet<String> = ["Oh My Soul.pro".to_string()].into_iter().collect();
    rig.preparer
        .remember_playlist(&*rig.host, &*rig.api, &rig.playlist, &only)
        .await
        .unwrap();
    // What an older StagePilot saved: a hash that counted every group's slides, and no version.
    let saved = rig.store.all()[&1001].clone();
    assert_eq!(saved.hash_version, HASH_VERSION);
    let old = Binding {
        arrangement_hash: "hash-of-the-slide-counts".into(),
        hash_version: 0,
        ..saved.clone()
    };
    rig.store.remember(old).unwrap();
    let statuses = rig
        .preparer
        .prepare(&*rig.host, &*rig.api, &inputs(true), true)
        .await;
    assert_eq!(statuses[0].readiness, Readiness::Cues);
    assert_eq!(
        rig.store.all()[&1001],
        saved,
        "remembered again with the new hash"
    );
    // The next pass compares the new hash.
    let statuses = rig
        .preparer
        .prepare(&*rig.host, &*rig.api, &inputs(true), true)
        .await;
    assert_eq!(statuses[0].readiness, Readiness::Cues);
}

#[tokio::test]
async fn slides_added_after_the_song_was_added_keep_it_followed_and_a_new_order_does_not() {
    let rig = rig();
    let only: BTreeSet<String> = ["Oh My Soul.pro".to_string()].into_iter().collect();
    rig.preparer
        .remember_playlist(&*rig.host, &*rig.api, &rig.playlist, &only)
        .await
        .unwrap();
    let with_file = |song: SyntheticSong| {
        Arc::new(FakeHost::new().with_library("Playback").with_file(
            "Playback",
            "Oh My Soul.pro",
            &song.build(),
        ))
    };
    // Two slides added in front of the Intro, and a new group at the front: still the same arrangement.
    for song in [
        oh_my_soul_with_extras(0, 0, 2),
        oh_my_soul_with_extras(1, 1, 0),
    ] {
        let host = with_file(song);
        let preparer = Preparer::new(rig.store.clone());
        let statuses = preparer
            .prepare(&*host, &*rig.api, &inputs(true), true)
            .await;
        assert!(
            matches!(statuses[0].readiness, Readiness::CuesAligned { .. }),
            "{:?}",
            statuses[0].readiness
        );
        assert_eq!(statuses[0].readiness.following(), Some("cues"));
        assert_eq!(statuses[0].readiness.code(), "cues_aligned");
        assert!(statuses[0].prepared.is_some());
    }
    // The same slides, played in another order: not the arrangement StagePilot wrote.
    let mut swapped = ARRANGEMENT;
    swapped.swap(2, 3);
    let f = fixture();
    let mut song = SyntheticSong::new("Oh My Soul").id(1001).seed(1);
    for (i, (name, cues)) in f.groups.iter().enumerate() {
        let slides: Vec<Option<&str>> = (0..*cues)
            .map(|_| if i == 0 { None } else { Some("la") })
            .collect();
        song = song.group_with_blanks(name, &slides);
    }
    let host = with_file(song.arrangement("Playback", &swapped).selected(0));
    let preparer = Preparer::new(rig.store.clone());
    let statuses = preparer
        .prepare(&*host, &*rig.api, &inputs(true), true)
        .await;
    assert_eq!(statuses[0].readiness, Readiness::ArrangementChanged);
    assert_eq!(statuses[0].prepared, None);
}

#[tokio::test]
async fn the_advice_names_the_template_that_fits_the_file() {
    // A song whose slides have two lines each: Count Off, then a 6-slide Verse and a 6-slide Chorus.
    let two_lines = |title: &str, id: u64, lines: &str| {
        let slide = |n: usize| format!("w{n}{lines}x{n}");
        let verse: Vec<String> = (0..6).map(slide).collect();
        let chorus: Vec<String> = (6..12).map(slide).collect();
        let verse: Vec<&str> = verse.iter().map(String::as_str).collect();
        let chorus: Vec<&str> = chorus.iter().map(String::as_str).collect();
        SyntheticSong::new(title)
            .id(id)
            .seed(5)
            .group_with_blanks("", &[None])
            .group("Verse", &verse)
            .group("Chorus", &chorus)
            .arrangement("Playback", &[0, 1, 2])
            .selected(0)
    };
    for (lines, template) in [
        ("", "a ProPresenter 1 Line Template"),
        ("\n", "the ProPresenter 2 Line Template"),
        (
            "\ny\n",
            "a ProPresenter Line Template that matches the file",
        ),
    ] {
        let host = Arc::new(FakeHost::new().with_library("Playback").with_file(
            "Playback",
            "Plain Song.pro",
            &two_lines("Plain Song", 1003, lines).build(),
        ));
        let api = Arc::new(FakeApi::new(host.clone(), Loading::Immediately));
        let playlist = api.add_playlist(
            "Sunday",
            None,
            vec![FakeApi::presentation_item(
                &entry_uuid("Playback", "Plain Song.pro"),
                "Plain Song",
                "Playback",
                0.0,
            )],
        );
        api.focus_playlist(&playlist, 0);
        let preparer = Preparer::new(Arc::new(MemoryBindingStore::default()));
        let song = |cues: Vec<LyricCue>| SongInput {
            key: "plain".into(),
            title: "Plain Song".into(),
            playback_song_id: Some(13),
            content_id: Some(1003),
            sections: plain_sections(),
            cues,
        };
        // No cues.
        let no_cues = preparer
            .prepare(&*host, &*api, &[song(Vec::new())], true)
            .await;
        assert_eq!(
            no_cues[0].advice().as_deref(),
            Some(format!("No cues: in Playback choose {template} for this song").as_str())
        );
        // Cues that stop far short of the file's 13 slides: the numbers do not fit, and the advice names the same template.
        let short = vec![
            LyricCue {
                time_s: 4.5,
                slide: 1,
            },
            LyricCue {
                time_s: 8.0,
                slide: 2,
            },
            LyricCue {
                time_s: 14.0,
                slide: 3,
            },
        ];
        let mismatch = preparer.prepare(&*host, &*api, &[song(short)], true).await;
        assert_eq!(
            mismatch[0].readiness.text(),
            "Cues do not match the file (cues go to slide 3, file has 13 cues)"
        );
        assert_eq!(
            mismatch[0].advice().as_deref(),
            Some(format!("In Playback choose {template} for this song").as_str())
        );
    }
}
