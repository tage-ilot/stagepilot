//! Tests with made-up song files (never a real song) and the numeric fixture of a real 54-cue song (times, slide numbers,
//! section names and group cue counts only; no words).

use super::*;
use crate::pro_file::synthetic::SyntheticSong;
use crate::pro_file::{rtf_has_visible_text, Song};
use serde_json::Value;

/// A song file with the given groups (in file order) and one selected arrangement named `Playback`.
fn song(groups: &[(&str, Vec<Option<&str>>)], arrangement: &[usize]) -> Song {
    let mut built = SyntheticSong::new("Test Song").id(4242);
    for (name, slides) in groups {
        built = built.group_with_blanks(name, slides);
    }
    let bytes = built
        .arrangement("Playback", arrangement)
        .selected(0)
        .build();
    Song::read(&bytes).expect("a song")
}

fn text(n: usize) -> Vec<Option<&'static str>> {
    vec![Some("la"); n]
}

fn section(name: &str, start: f64, end: f64) -> Section {
    Section {
        name: name.to_string(),
        start,
        end,
    }
}

fn cue(time_s: f64, slide: u32) -> LyricCue {
    LyricCue { time_s, slide }
}

// ---- the real 54-cue song ------------------------------------------------------------------------------------------

struct Fixture {
    cues: Vec<LyricCue>,
    sections: Vec<Section>,
    groups: Vec<(String, usize)>,
    file_cue_total: usize,
}

fn fixture() -> Fixture {
    let value: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/oh-my-soul-lyric-cues.fixture.json"
    ))
    .expect("fixture json");
    Fixture {
        cues: value["cues"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                cue(
                    c["time_s"].as_f64().unwrap(),
                    c["slide"].as_u64().unwrap() as u32,
                )
            })
            .collect(),
        sections: value["sections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                section(
                    s["name"].as_str().unwrap(),
                    s["start_s"].as_f64().unwrap(),
                    s["end_s"].as_f64().unwrap(),
                )
            })
            .collect(),
        groups: value["file_groups"]
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
        file_cue_total: value["file_cue_total"].as_u64().unwrap() as usize,
    }
}

/// The `Playback` arrangement of that song: one entry per Playback section, as a position in the file's groups. The two
/// `Verse 1`, the two `Interlude`, `Bridge` and the three `Chorus` and `Refrain` groups are different groups of the file; the
/// real cue numbers show which one Playback plays in each section (for instance the `Refrain` at 104.87 s plays the file's
/// SECOND `Refrain` group, slides 24..26, and the one at 112.94 s the FIRST, slides 15..17).
const ARRANGEMENT: [usize; 23] = [
    0, 1, 2, 3, 4, 5, 6, 7, 4, 8, 5, 9, 10, 10, 11, 12, 13, 14, 8, 8, 15, 16, 17,
];

fn fixture_song(f: &Fixture) -> Song {
    let groups: Vec<(&str, Vec<Option<&str>>)> = f
        .groups
        .iter()
        .enumerate()
        .map(|(i, (name, cues))| {
            // The first group is the blank Count Off: a background, no text element.
            let slides = if i == 0 {
                vec![None; *cues]
            } else {
                text(*cues)
            };
            (name.as_str(), slides)
        })
        .collect();
    song(&groups, &ARRANGEMENT)
}

/// The slide numbers ProPresenter has, written out: the raw indexes in the order the arrangement plays its groups.
fn flat_raws(map: &SongMap) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut total = 0;
    for g in map.groups() {
        starts.push(total);
        total += g.cues;
    }
    map.arrangement()
        .iter()
        .flat_map(|g| starts[*g]..starts[*g] + map.groups()[*g].cues)
        .collect()
}

#[test]
fn the_real_song_has_70_cues_all_resolved_and_the_template_matches() {
    let f = fixture();
    assert_eq!(f.cues.len(), 70);
    assert_eq!(f.sections.len(), 23);
    assert_eq!(f.file_cue_total, 54);
    let song = fixture_song(&f);
    let map = SongMap::from_song(&song).expect("map");
    assert_eq!(map.cue_total(), 54);
    assert_eq!(map.arrangement().len(), f.sections.len());
    let max = f.cues.iter().map(|c| c.slide).max().unwrap();
    assert_eq!(max, 53);
    assert_eq!(max as usize, f.file_cue_total - 1);

    let schedule = schedule_for_song(&song, &f.sections, &f.cues, None).expect("followable");
    assert_eq!(schedule.source, Source::Cues);
    assert_eq!(schedule.cue_problem, None);
    assert_eq!(schedule.steps.len(), 70);
    assert_eq!(schedule.cue_max_slide, Some(53));
    assert!(schedule.steps.iter().all(|s| s.kind == StepKind::Cue));
    assert!(schedule
        .steps
        .windows(2)
        .all(|w| w[0].time_s <= w[1].time_s));
    assert_eq!(schedule.arrangement_hash, map.arrangement_hash());
}

#[test]
fn every_cue_lands_in_the_section_named_like_its_group_at_the_arrangement_position() {
    let f = fixture();
    let song = fixture_song(&f);
    let map = SongMap::from_song(&song).unwrap();
    let schedule = build_schedule(&map, &f.sections, &f.cues, None).unwrap();
    let flat = flat_raws(&map);
    assert_eq!(flat.len(), map.flat_total());
    for step in &schedule.steps {
        // The slide number ProPresenter has at that flat position is the raw cue.
        assert_eq!(flat[step.flat_index], step.raw, "{step:?}");
        // The entry is the section whose name is the group's name, and the flat position is inside that entry.
        let group = map.group_of_raw(step.raw).unwrap();
        assert_eq!(
            f.sections[step.section_index].name, f.groups[group].0,
            "{step:?}"
        );
        assert_eq!(map.arrangement()[step.section_index], group, "{step:?}");
        let first = map
            .flat_index(
                step.section_index,
                map.groups()[..group].iter().map(|g| g.cues).sum(),
            )
            .unwrap();
        assert!(
            (first..first + map.groups()[group].cues).contains(&step.flat_index),
            "{step:?}"
        );
        // The arrangement-position rule, written out.
        let before: usize = map.arrangement()[..step.section_index]
            .iter()
            .map(|g| map.groups()[*g].cues)
            .sum();
        let group_start: usize = map.groups()[..group].iter().map(|g| g.cues).sum();
        assert_eq!(
            step.flat_index,
            before + (step.raw - group_start),
            "{step:?}"
        );
    }
}

#[test]
fn repeated_chorus_refrain_and_bridge_sections_map_to_the_right_entries() {
    let f = fixture();
    let song = fixture_song(&f);
    let map = SongMap::from_song(&song).unwrap();
    let schedule = build_schedule(&map, &f.sections, &f.cues, None).unwrap();
    let at = |time: f64| {
        *schedule
            .steps
            .iter()
            .find(|s| (s.time_s - time).abs() < 1e-9)
            .unwrap_or_else(|| panic!("no step at {time}"))
    };
    // (time of a cue, the arrangement entry it plays in, the slide ProPresenter has there)
    // Chorus the first time (entry 4) and the second (entry 8): the same raw cues 11..14, different flat positions
    // (the 24 slides of entries 0..7 come before the second one).
    assert_eq!(
        (at(44.01).section_index, at(44.01).raw, at(44.01).flat_index),
        (4, 11, 11)
    );
    assert_eq!(
        (at(88.38).section_index, at(88.38).raw, at(88.38).flat_index),
        (8, 11, 24)
    );
    // The Refrains: the file's first Refrain group (raw 15..17) is played by entries 5 and 10; the second (raw 24..26) by 9, 18, 19.
    assert_eq!(at(59.73).section_index, 5);
    assert_eq!(
        at(63.2).section_index,
        5,
        "the third line comes in the interlude's lead but belongs to the refrain"
    );
    assert_eq!(at(104.13).section_index, 9);
    assert_eq!(at(112.16).section_index, 10);
    assert_eq!(at(114.69).section_index, 10);
    assert_eq!(at(209.0).section_index, 18);
    assert_eq!(at(217.07).section_index, 19);
    assert_eq!(at(225.16).section_index, 20);
    // The Bridges: the first group (raw 29..33) is played twice, entries 12 and 13, the second group (34..38) once.
    assert_eq!(at(124.33).section_index, 12);
    assert_eq!(at(140.46).section_index, 13);
    assert_eq!(at(156.88).section_index, 14);
    // Flat positions after a repeat are shifted by it: the slide fired for the second Bridge is further on than its raw cue.
    let second_bridge = at(140.46);
    assert_eq!(second_bridge.raw, 29);
    assert_eq!(second_bridge.flat_index, map.flat_index(13, 29).unwrap());
    assert!(second_bridge.flat_index > at(124.33).flat_index);
    assert_eq!(second_bridge.flat_index - at(124.33).flat_index, 5);
}

#[test]
fn cues_that_stop_at_slide_30_of_a_54_cue_file_do_not_fit() {
    let f = fixture();
    let song = fixture_song(&f);
    // The first 40 events of the 1 Line series, which stop at slide 30: the numbers do not reach the file's 54 cues.
    let two_line: Vec<LyricCue> = f.cues.iter().copied().filter(|c| c.slide <= 30).collect();
    assert!(two_line.len() < 70 && two_line.iter().map(|c| c.slide).max() == Some(30));
    let schedule = schedule_for_song(&song, &f.sections, &two_line, None).unwrap();
    assert_eq!(schedule.source, Source::Bars);
    let problem = schedule.cue_problem.clone().unwrap();
    assert_eq!(
        problem,
        CueProblem::Mismatch {
            max_slide: 30,
            file_cues: 54
        }
    );
    assert_eq!(
        problem.words(),
        "template does not match the file (cues go to slide 30, the file has 54)"
    );
    // The fallback is still a schedule for the whole song, every step by bars.
    assert_eq!(
        schedule.steps.len(),
        SongMap::from_song(&song).unwrap().flat_total()
    );
    assert!(schedule.steps.iter().all(|s| s.kind == StepKind::Bars));
}

// ---- the failure cases of rule 4 --------------------------------------------------------------------------------------

#[test]
fn a_slide_past_the_end_of_the_file_is_a_mismatch_too() {
    let song = song(&[("A", text(2)), ("B", text(2))], &[0, 1]);
    let sections = [section("A", 0.0, 10.0), section("B", 10.0, 20.0)];
    let cues = [cue(1.0, 0), cue(4.0, 1), cue(11.0, 2), cue(15.0, 4)];
    let got = schedule_for_song(&song, &sections, &cues, None).unwrap();
    assert_eq!(
        got.cue_problem,
        Some(CueProblem::Mismatch {
            max_slide: 4,
            file_cues: 4
        })
    );
    // A template that stops short of the last slides by more than a person could have added is the same failure. (One slide
    // short is a song with an extra slide at the end: see the alignment tests.)
    let long = self::song(&[("A", text(6)), ("B", text(6))], &[0, 1]);
    let cues = [cue(1.0, 0), cue(4.0, 1), cue(11.0, 2)];
    let got = schedule_for_song(&long, &sections, &cues, None).unwrap();
    assert_eq!(
        got.cue_problem,
        Some(CueProblem::Mismatch {
            max_slide: 2,
            file_cues: 12
        })
    );
}

#[test]
fn a_cue_in_no_section_or_after_the_song_end_sends_the_song_to_bars() {
    let song = song(&[("A", text(2)), ("B", text(2))], &[0, 1]);
    let gap = [section("A", 0.0, 10.0), section("B", 30.0, 40.0)];
    // 15 s lies after A's end and more than 2 s before B's start.
    let cues = [cue(1.0, 0), cue(4.0, 1), cue(15.0, 2), cue(35.0, 3)];
    let got = schedule_for_song(&song, &gap, &cues, None).unwrap();
    assert_eq!(got.source, Source::Bars);
    let problem = got.cue_problem.unwrap();
    assert_eq!(
        problem,
        CueProblem::NoSection {
            time_s: 15.0,
            slide: 2
        }
    );
    assert_eq!(problem.words(), "a cue is in no section");
    // A cue after the end of the last section.
    let sections = [section("A", 0.0, 10.0), section("B", 10.0, 20.0)];
    let cues = [cue(1.0, 0), cue(4.0, 1), cue(11.0, 2), cue(25.0, 3)];
    let got = schedule_for_song(&song, &sections, &cues, None).unwrap();
    let problem = got.cue_problem.unwrap();
    assert_eq!(
        problem,
        CueProblem::AfterEnd {
            time_s: 25.0,
            slide: 3
        }
    );
    assert_eq!(problem.words(), "cue after the song end");
    // A cue only 2 s before a section's start is that section's (cues lead the lyric).
    let cues = [cue(1.0, 0), cue(4.0, 1), cue(8.0, 2), cue(15.0, 3)];
    let got = schedule_for_song(&song, &sections, &cues, None).unwrap();
    assert_eq!(got.source, Source::Cues);
    assert_eq!(got.steps[2].section_index, 1);
    assert_eq!(got.steps[2].flat_index, 2);
}

#[test]
fn no_cues_or_too_few_fall_back_to_bars_and_say_why() {
    let song = song(&[("A", text(2)), ("B", text(2))], &[0, 1]);
    let sections = [section("A", 0.0, 10.0), section("B", 10.0, 20.0)];
    let none = schedule_for_song(&song, &sections, &[], None).unwrap();
    assert_eq!(none.source, Source::Bars);
    assert_eq!(none.cue_problem, Some(CueProblem::NoCues));
    assert_eq!(none.cue_problem.unwrap().words(), "no cues");
    assert_eq!(none.cue_max_slide, None);
    let times: Vec<(f64, usize)> = none
        .steps
        .iter()
        .map(|s| (s.time_s, s.flat_index))
        .collect();
    assert_eq!(times, vec![(0.0, 0), (5.0, 1), (10.0, 2), (15.0, 3)]);
    let few = schedule_for_song(&song, &sections, &[cue(1.0, 0), cue(2.0, 3)], None).unwrap();
    assert_eq!(few.cue_problem, Some(CueProblem::TooFew { cues: 2 }));
    assert_eq!(few.source, Source::Bars);
}

#[test]
fn cues_are_sorted_and_equal_pairs_counted_once() {
    let song = song(&[("A", text(2)), ("B", text(2))], &[0, 1]);
    let sections = [section("A", 0.0, 10.0), section("B", 10.0, 20.0)];
    let cues = [
        cue(11.0, 2),
        cue(1.0, 0),
        cue(4.0, 1),
        cue(4.0, 1),
        cue(15.0, 3),
    ];
    let got = schedule_for_song(&song, &sections, &cues, None).unwrap();
    assert_eq!(got.source, Source::Cues);
    let flats: Vec<usize> = got.steps.iter().map(|s| s.flat_index).collect();
    assert_eq!(flats, vec![0, 1, 2, 3]);
    let bad = [cue(f64::NAN, 0), cue(4.0, 1), cue(11.0, 2), cue(15.0, 3)];
    assert_eq!(
        schedule_for_song(&song, &sections, &bad, None)
            .unwrap()
            .cue_problem,
        Some(CueProblem::BadTime)
    );
}

#[test]
fn a_blank_background_cue_in_the_middle_still_counts_in_the_numbers() {
    // File order A (text, background, text) then B (two texts); the arrangement plays B first, then A.
    let groups = [
        ("A", vec![Some("a1"), None, Some("a2")]),
        ("B", vec![Some("b1"), Some("b2")]),
    ];
    let song = song(&groups, &[1, 0]);
    let map = SongMap::from_song(&song).unwrap();
    assert_eq!(map.cue_total(), 5);
    let sections = [section("B", 0.0, 10.0), section("A", 10.0, 22.0)];
    // Raw 3,4 are B, raw 0,1,2 are A (raw 1 is the background slide). Flat: b1=0 b2=1 a1=2 bg=3 a2=4.
    let cues = [
        cue(0.5, 3),
        cue(5.0, 4),
        cue(10.5, 0),
        cue(14.0, 1),
        cue(18.0, 2),
    ];
    let got = build_schedule(&map, &sections, &cues, None).unwrap();
    assert_eq!(got.source, Source::Cues);
    let pairs: Vec<(usize, usize)> = got.steps.iter().map(|s| (s.raw, s.flat_index)).collect();
    assert_eq!(pairs, vec![(3, 0), (4, 1), (0, 2), (1, 3), (2, 4)]);
    // Text flags come from the file.
    assert_eq!(map.groups()[0].has_text, vec![true, false, true]);

    // By bars the background slide is not a slot: A's two texts split the section in two, the background slide goes with
    // the second text and so never shows.
    let bars = build_schedule(&map, &sections, &[], None).unwrap();
    let got: Vec<(f64, usize)> = bars
        .steps
        .iter()
        .map(|s| (s.time_s, s.flat_index))
        .collect();
    assert_eq!(
        got,
        vec![(0.0, 0), (5.0, 1), (10.0, 2), (16.0, 3), (16.0, 4)]
    );
}

#[test]
fn same_named_groups_are_told_apart_by_their_cues_not_their_names() {
    // Two groups called "Verse": file order Verse(2) Verse(2) Chorus(1). Playback plays Verse#1, Chorus, Verse#2, Verse#1.
    let groups = [("Verse", text(2)), ("Verse", text(2)), ("Chorus", text(1))];
    let song = song(&groups, &[0, 2, 1, 0]);
    let map = SongMap::from_song(&song).unwrap();
    assert_eq!(map.cue_total(), 5);
    assert_eq!(map.flat_total(), 7);
    let sections = [
        section("Verse", 0.0, 10.0),
        section("Chorus", 10.0, 15.0),
        section("Verse", 15.0, 25.0),
        section("Verse", 25.0, 35.0),
    ];
    // raw: Verse#1 = 0,1; Verse#2 = 2,3; Chorus = 4. Flat: 0,1 | 2 | 3,4 | 5,6
    let cues = [
        cue(1.0, 0),
        cue(5.0, 1),
        cue(11.0, 4),
        cue(16.0, 2),
        cue(20.0, 3),
        cue(26.0, 0),
        cue(30.0, 1),
    ];
    let got = build_schedule(&map, &sections, &cues, None).unwrap();
    assert_eq!(got.source, Source::Cues);
    let pairs: Vec<(usize, usize, usize)> = got
        .steps
        .iter()
        .map(|s| (s.raw, s.section_index, s.flat_index))
        .collect();
    assert_eq!(
        pairs,
        vec![
            (0, 0, 0),
            (1, 0, 1),
            (4, 1, 2),
            (2, 2, 3),
            (3, 2, 4),
            // The first Verse group comes again: the entry nearest the time (3), not the first one (0).
            (0, 3, 5),
            (1, 3, 6)
        ]
    );
    // The first group's entries are 0 and 3.
    assert_eq!(map.entry_for(0, 0), Some(0));
    assert_eq!(map.entry_for(0, 2), Some(3));
    assert_eq!(
        map.entry_for(0, 1),
        Some(0),
        "a tie goes to the earlier entry"
    );
    assert_eq!(map.entry_for(1, 0), Some(2));
}

// ---- bars -----------------------------------------------------------------------------------------------------------------

#[test]
fn bars_spread_text_slides_evenly_and_fire_blank_groups_at_their_start() {
    let groups = [
        ("", vec![None]),
        ("Verse", text(4)),
        ("Solo", vec![None, None]),
        ("Last", vec![Some("x"), None]),
    ];
    let song = song(&groups, &[0, 1, 2, 3]);
    let sections = [
        section("Count Off", 0.0, 4.0),
        section("Verse", 4.0, 20.0),
        section("Solo", 20.0, 30.0),
        section("Last", 30.0, 40.0),
    ];
    let got = schedule_for_song(&song, &sections, &[], None).unwrap();
    let steps: Vec<(f64, usize)> = got.steps.iter().map(|s| (s.time_s, s.flat_index)).collect();
    assert_eq!(
        steps,
        vec![
            (0.0, 0),
            (4.0, 1),
            (8.0, 2),
            (12.0, 3),
            (16.0, 4),
            (20.0, 5),
            (20.0, 6),
            (30.0, 7),
            // a background slide after the last text one waits for the end of the section
            (40.0, 8),
        ]
    );
    assert!(got.steps.iter().all(|s| s.kind == StepKind::Bars));
    assert!(got
        .steps
        .iter()
        .enumerate()
        .all(|(i, s)| s.raw == i && s.section_index <= 3));
}

#[test]
fn bars_snap_to_bar_lines_when_the_beat_grid_is_known_and_stay_even_when_they_cannot() {
    let song = song(&[("Verse", text(4))], &[0]);
    let sections = [section("Verse", 0.0, 16.0)];
    // Even slots are 0, 4, 8, 12. Bar lines every 2.6 s: 5.2 (nearest to 4), 7.8 (to 8) and 13.0 (to 12).
    let bar_lines: Vec<f64> = (1..6).map(|i| 2.6 * f64::from(i)).collect();
    let got = schedule_for_song(&song, &sections, &[], Some(&bar_lines)).unwrap();
    let times: Vec<f64> = got.steps.iter().map(|s| s.time_s).collect();
    assert_eq!(times[0], 0.0);
    assert!(
        (times[1] - 5.2).abs() < 1e-9
            && (times[2] - 7.8).abs() < 1e-9
            && (times[3] - 13.0).abs() < 1e-9
    );
    // One bar line only: two slots would take it, so the section keeps its even spread.
    let got = schedule_for_song(&song, &sections, &[], Some(&[6.0])).unwrap();
    let times: Vec<f64> = got.steps.iter().map(|s| s.time_s).collect();
    assert_eq!(times, vec![0.0, 4.0, 8.0, 12.0]);
}

// ---- when a song cannot be followed -------------------------------------------------------------------------------------

#[test]
fn a_file_that_is_not_the_playback_copy_is_not_followed() {
    let sections = [section("A", 0.0, 10.0), section("B", 10.0, 20.0)];
    let build = |selected: Option<usize>, name: &str| {
        let mut built = SyntheticSong::new("T")
            .group("A", &["a"])
            .group("B", &["b"])
            .arrangement(name, &[0, 1]);
        if let Some(i) = selected {
            built = built.selected(i);
        }
        Song::read(&built.build()).unwrap()
    };
    let result = |song: &Song| schedule_for_song(song, &sections, &[], None).unwrap_err();
    assert_eq!(
        result(&build(Some(0), "Default")),
        Unfollowable::File(MapError::NoPlaybackArrangement)
    );
    assert_eq!(
        result(&build(None, "Playback")),
        Unfollowable::File(MapError::PlaybackNotSelected)
    );
    // The right arrangement, but not one entry per section.
    let song = build(Some(0), "Playback");
    assert_eq!(
        schedule_for_song(&song, &sections[..1], &[], None).unwrap_err(),
        Unfollowable::ArrangementDiffers {
            entries: 2,
            sections: 1
        }
    );
    assert_eq!(
        schedule_for_song(&song, &[], &[], None).unwrap_err(),
        Unfollowable::NoSections
    );
    // The name is not case-sensitive, like ProPresenter's.
    assert!(schedule_for_song(&build(Some(0), "playback"), &sections, &[], None).is_ok());
}

#[test]
fn a_group_the_file_does_not_have_is_refused() {
    let group = |cues: usize| FileGroup {
        uuid: "u".into(),
        name: "G".into(),
        cues,
        has_text: vec![true; cues],
        lines: vec![1; cues],
    };
    assert_eq!(
        SongMap::new(vec![group(1)], vec![0, 1]),
        Err(MapError::UnknownGroup)
    );
    let mut odd = group(2);
    odd.has_text.pop();
    odd.lines.pop();
    assert_eq!(
        SongMap::new(vec![odd], vec![0]),
        Err(MapError::Inconsistent)
    );
}

#[test]
fn the_arrangement_hash_depends_only_on_the_order_of_the_groups_the_arrangement_plays() {
    let a = || song(&[("A", text(2)), ("B", text(3))], &[0, 1, 0]);
    let hash = |s: &Song| SongMap::from_song(s).unwrap().arrangement_hash();
    let base = hash(&a());
    assert_eq!(base.len(), 64);
    assert_eq!(base, hash(&a()), "the same file gives the same hash");
    // Another order.
    assert_ne!(
        base,
        hash(&song(&[("A", text(2)), ("B", text(3))], &[0, 0, 1]))
    );
    // Another arrangement length.
    assert_ne!(
        base,
        hash(&song(&[("A", text(2)), ("B", text(3))], &[0, 1]))
    );
    // Another group in the arrangement.
    assert_ne!(
        base,
        hash(&song(
            &[("A", text(2)), ("B", text(3)), ("C", text(1))],
            &[0, 2, 0]
        ))
    );
    // Slides added to or taken out of a group, a group the arrangement does not play, what the slides say and a group's name
    // are not part of it: the song stays followed.
    assert_eq!(
        base,
        hash(&song(&[("A", text(3)), ("B", text(3))], &[0, 1, 0]))
    );
    assert_eq!(
        base,
        hash(&song(&[("A", text(2)), ("B", text(1))], &[0, 1, 0]))
    );
    let reworded = song(
        &[("A", vec![Some("x"), Some("y")]), ("B", text(3))],
        &[0, 1, 0],
    );
    assert_eq!(base, hash(&reworded));
    let renamed = song(&[("Verse", text(2)), ("Chorus", text(3))], &[0, 1, 0]);
    assert_eq!(base, hash(&renamed));
}

// ---- which slides have text -------------------------------------------------------------------------------------------

#[test]
fn the_file_says_which_cues_have_text() {
    let song = song(
        &[(
            "G",
            vec![
                Some("words"),
                None,
                Some(""),
                Some("   "),
                Some(
                    r"{\rtf1\ansi{\fonttbl\f0 Helvetica;}{\colortbl;\red255\green255\blue255;}\f0\fs96 \cf1 Hello}",
                ),
                Some(r"{\rtf1\ansi{\fonttbl\f0 Helvetica;}\pard\f0\fs96 \cf0 }"),
            ],
        )],
        &[0],
    );
    assert_eq!(
        song.groups[0].cue_has_text,
        vec![true, false, false, false, true, false]
    );
    assert_eq!(
        song.groups[0].cue_ids.len(),
        song.groups[0].cue_has_text.len()
    );
}

#[test]
fn rtf_text_is_told_from_rtf_furniture() {
    let yes = |s: &str| rtf_has_visible_text(s.as_bytes());
    assert!(yes("Hello"));
    assert!(yes(
        r"{\rtf1\ansi\deff0 {\fonttbl{\f0 Arial;}}\pard Hi\par}"
    ));
    assert!(yes(r"{\rtf1 \'e9}"), "an escaped letter shows");
    assert!(yes(r"{\rtf1 \u8217'}"), "a unicode character shows");
    assert!(yes(r"{\rtf1 \\}"), "an escaped backslash shows");
    assert!(!yes(""));
    assert!(!yes("  \n "));
    assert!(!yes(
        r"{\rtf1\ansi{\fonttbl{\f0 Arial;}}{\colortbl;\red0\green0\blue0;}{\*\expandedcolortbl;;}\pard\par\line\tab }"
    ));
    assert!(!yes(r"{\rtf1 {\info{\title Hidden}} \u160 }"));
    assert!(!yes(r"{\rtf1 \~ }"), "a hard space shows nothing");
    // Garbage never panics.
    for text in [
        r"{\rtf1 \",
        r"{\rtf1 {",
        r"{\rtf1 \'",
        r"{\rtf1 \u",
        "}}}",
        r"{\rtf1 \'zz}",
    ] {
        let _ = yes(text);
    }
}
