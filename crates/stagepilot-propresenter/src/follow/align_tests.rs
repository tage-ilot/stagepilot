//! Extra slides in the song file (design `SPEC.md`, "Revision 2"): the numeric fixture of a real 54-cue song with blank slides
//! added in every place a person could add them, made-up song files only (every cue has its own made-up text, so a test can say
//! which cue a flat slide number shows).

use super::*;
use crate::pro_file::synthetic::SyntheticSong;
use crate::pro_file::{rtf_visible_lines, Song};
use serde_json::Value;

const FIXTURE: &str = include_str!("../../tests/fixtures/oh-my-soul-lyric-cues.fixture.json");
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

// ---- a song file as a plain description that a test can change --------------------------------------------------------------

#[derive(Clone)]
struct Layout {
    /// (uuid key, name, the text of each slide: `None` is a blank background)
    groups: Vec<(u32, String, Vec<Option<String>>)>,
    /// Positions in `groups`.
    arrangement: Vec<usize>,
}

/// The fixture song: the Count Off is one blank slide, every other slide has a text of its own (`g3c1`: group 3, second slide).
fn original() -> Layout {
    let f = fixture();
    Layout {
        groups: f
            .groups
            .iter()
            .enumerate()
            .map(|(g, (name, cues))| {
                let slides = (0..*cues)
                    .map(|c| (g != 0).then(|| format!("g{g}c{c}")))
                    .collect();
                (g as u32, name.clone(), slides)
            })
            .collect(),
        arrangement: ARRANGEMENT.to_vec(),
    }
}

impl Layout {
    /// `count` blank slides in a new group at position `at` that the arrangement does not play (a group someone added).
    fn with_new_group(&self, at: usize, count: usize) -> Layout {
        let mut out = self.clone();
        out.groups.insert(
            at,
            (900 + at as u32, "Background".to_string(), vec![None; count]),
        );
        for entry in &mut out.arrangement {
            if *entry >= at {
                *entry += 1;
            }
        }
        out
    }

    /// `count` blank slides inside group `group`, before its slide number `before` (`before == slides` is the end).
    fn with_blanks(&self, group: usize, before: usize, count: usize) -> Layout {
        let mut out = self.clone();
        for _ in 0..count {
            out.groups[group].2.insert(before, None);
        }
        out
    }

    fn song(&self) -> Song {
        let mut built = SyntheticSong::new("Test Song").id(1001).seed(7);
        for (key, name, slides) in &self.groups {
            let slides: Vec<Option<&str>> = slides.iter().map(Option::as_deref).collect();
            built = built.group_with_key(*key, name, &slides);
        }
        let bytes = built
            .arrangement("Playback", &self.arrangement)
            .selected(0)
            .build();
        Song::read(&bytes).expect("a song")
    }

    /// What ProPresenter shows at each slide number of the arrangement.
    fn flat(&self) -> Vec<Option<String>> {
        self.arrangement
            .iter()
            .flat_map(|g| self.groups[*g].2.clone())
            .collect()
    }

    /// What each cue of the file holds, in file order.
    fn raw(&self) -> Vec<Option<String>> {
        self.groups.iter().flat_map(|g| g.2.clone()).collect()
    }
}

/// The steps for the song with nothing added: what every variant must still point at.
fn baseline() -> Vec<(Step, String)> {
    let f = fixture();
    let layout = original();
    let map = SongMap::from_song(&layout.song()).unwrap();
    let steps = cue_steps(&map, &f.sections, &f.cues).unwrap();
    let flat = layout.flat();
    steps
        .into_iter()
        .map(|s| {
            let text = flat[s.flat_index].clone().expect("a lyric slide");
            (s, text)
        })
        .collect()
}

/// Every cue of the fixture, on the song with `layout`, shows the same lyric slide as with nothing added.
fn assert_same_slides(layout: &Layout, label: &str) -> Schedule {
    let f = fixture();
    let song = layout.song();
    let map = SongMap::from_song(&song).unwrap_or_else(|e| panic!("{label}: {e:?}"));
    let schedule = build_schedule(&map, &f.sections, &f.cues, None).unwrap();
    assert_eq!(
        schedule.source,
        Source::Cues,
        "{label}: {:?}",
        schedule.cue_problem
    );
    let (flat, raw) = (layout.flat(), layout.raw());
    let want = baseline();
    assert_eq!(schedule.steps.len(), want.len(), "{label}");
    for ((base, text), step) in want.iter().zip(&schedule.steps) {
        assert_eq!(base.time_s, step.time_s, "{label}");
        assert_eq!(
            base.section_index, step.section_index,
            "{label}: the section of {step:?}"
        );
        assert_eq!(
            flat[step.flat_index].as_ref(),
            Some(text),
            "{label}: flat slide {} of {step:?} should be {text}",
            step.flat_index
        );
        // `raw` is the file's own cue index, and the cue there is the same one.
        assert_eq!(
            raw[step.raw].as_ref(),
            Some(text),
            "{label}: raw of {step:?}"
        );
        // The file says that cue has text and the arrangement says where it is.
        let group = map.group_of_raw(step.raw).unwrap();
        assert_eq!(map.arrangement()[step.section_index], group, "{label}");
        let inside = step.raw - map.groups()[..group].iter().map(|g| g.cues).sum::<usize>();
        assert!(map.groups()[group].has_text[inside], "{label}: {step:?}");
    }
    schedule
}

// ---- the same lyric slides wherever the extra slides are ------------------------------------------------------------------

#[test]
fn a_song_with_nothing_added_is_unchanged_and_not_aligned() {
    let schedule = assert_same_slides(&original(), "original");
    assert_eq!(schedule.alignment, None);
    // The steps are exactly the exact-layout path's: flat index and raw cue as the global raw index gives them.
    for (base, _) in baseline() {
        assert_eq!(
            schedule
                .steps
                .iter()
                .find(|s| s.time_s == base.time_s)
                .copied(),
            Some(base)
        );
    }
    assert!(schedule.steps.iter().all(|s| s.raw <= 53));
}

#[test]
fn extra_blank_slides_added_as_a_new_first_group() {
    for count in [1, 2] {
        let layout = original().with_new_group(0, count);
        let schedule = assert_same_slides(&layout, &format!("{count} new first group"));
        let alignment = schedule.alignment.unwrap();
        assert_eq!(alignment.extras, count);
        // The group is not in the arrangement, so ProPresenter's numbers do not move; the file's cue indexes do.
        assert!(!alignment.uncertain);
        assert!(alignment.shifts.iter().all(|d| *d == count));
        let plain = cue_steps(
            &SongMap::from_song(&original().song()).unwrap(),
            &fixture().sections,
            &fixture().cues,
        )
        .unwrap();
        for (a, b) in schedule.steps.iter().zip(&plain) {
            assert_eq!(a.flat_index, b.flat_index);
            assert_eq!(a.raw, b.raw + count);
        }
    }
}

#[test]
fn extra_blank_slides_inside_the_count_off_group() {
    for count in [1, 2] {
        // Before the Count Off's own blank slide and after it.
        for before in [0, 1] {
            let layout = original().with_blanks(0, before, count);
            let schedule =
                assert_same_slides(&layout, &format!("{count} in count off at {before}"));
            let alignment = schedule.alignment.unwrap();
            assert_eq!(alignment.extras, count);
            assert!(!alignment.uncertain, "no group has a choice");
            // Every ProPresenter number moves up by the extras, the Count Off's too.
            let plain = cue_steps(
                &SongMap::from_song(&original().song()).unwrap(),
                &fixture().sections,
                &fixture().cues,
            )
            .unwrap();
            for (a, b) in schedule.steps.iter().zip(&plain) {
                assert_eq!(a.flat_index, b.flat_index + count);
                assert_eq!(a.raw, b.raw + count);
            }
            // The groups without cues (the Count Off) take the next shift, as only blank slides lie between.
            assert_eq!(alignment.shifts[0], count);
        }
    }
}

#[test]
fn extra_blank_slides_at_the_start_of_the_first_lyric_group() {
    for count in [1, 2] {
        let layout = original().with_blanks(1, 0, count);
        let schedule = assert_same_slides(&layout, &format!("{count} at the start of the intro"));
        let alignment = schedule.alignment.unwrap();
        assert_eq!(alignment.extras, count);
        // The blanks are in front of the Intro's first lyric slide, so Playback's first cue lands on the first text slide.
        assert!(
            alignment.uncertain,
            "the extras sit inside the group, where they sit is guessed"
        );
        assert_eq!(schedule.steps[0].raw, 1 + count);
        assert_eq!(schedule.steps[0].flat_index, 1 + count);
        assert_eq!(alignment.check_section, 1);
    }
}

#[test]
fn extra_blank_slides_between_two_groups() {
    for count in [1, 2] {
        for at in [1, 2, 3, 5, 9, 12, 17] {
            let layout = original().with_new_group(at, count);
            let schedule = assert_same_slides(&layout, &format!("{count} new group at {at}"));
            assert!(!schedule.alignment.unwrap().uncertain);
        }
    }
}

#[test]
fn extra_blank_slides_at_the_end() {
    for count in [1, 2] {
        // A new group at the very end, and the end of the last group.
        let at = original().groups.len();
        assert_same_slides(
            &original().with_new_group(at, count),
            &format!("{count} new last group"),
        );
        let last = at - 1;
        let slides = original().groups[last].2.len();
        assert_same_slides(
            &original().with_blanks(last, slides, count),
            &format!("{count} at the end of the last group"),
        );
    }
}

#[test]
fn extra_blank_slides_inside_a_middle_group() {
    for count in [1, 2] {
        // Group 4 is the first Chorus: played in four sections, with a cue that fires just before the section ends.
        for before in [0, 1, 2, 3, 4] {
            let layout = original().with_blanks(4, before, count);
            assert_same_slides(&layout, &format!("{count} in chorus before {before}"));
        }
        // Group 5, the Refrain whose third cue (slide 17 at 63.2 s) comes 1.3 s before its own section ends.
        for before in [0, 1, 2, 3] {
            let layout = original().with_blanks(5, before, count);
            assert_same_slides(&layout, &format!("{count} in refrain before {before}"));
        }
    }
}

#[test]
fn extra_blank_slides_in_every_place_keep_every_lyric_slide() {
    // Every group: before its first slide, between its slides, after its last; and a new group between every two groups.
    let base = original();
    for count in [1, 2] {
        for g in 0..base.groups.len() {
            let slides = base.groups[g].2.len();
            for before in 0..=slides {
                let layout = base.with_blanks(g, before, count);
                assert_same_slides(
                    &layout,
                    &format!("{count} blank in group {g} before {before}"),
                );
            }
        }
        for at in 0..=base.groups.len() {
            assert_same_slides(
                &base.with_new_group(at, count),
                &format!("{count} new group at {at}"),
            );
        }
    }
}

#[test]
fn extras_in_several_places_at_once_still_keep_every_lyric_slide() {
    let base = original();
    let layout = base
        .with_new_group(0, 1)
        .with_blanks(3, 0, 1)
        .with_blanks(9, 2, 1)
        .with_new_group(14, 1);
    let schedule = assert_same_slides(&layout, "four places");
    assert_eq!(schedule.alignment.unwrap().extras, 4);
}

#[test]
fn the_cue_that_fires_before_its_section_ends_stays_with_its_own_section() {
    // Slide 17 fires at 63.2 s, 1.3 s before the Refrain (60.5 to 64.54 s) ends; the Interlude's window opens at 62.54 s.
    for layout in [
        original().with_new_group(0, 2),
        original().with_blanks(5, 1, 1),
        original().with_blanks(5, 3, 2),
    ] {
        let schedule = assert_same_slides(&layout, "refrain");
        let step = schedule.steps.iter().find(|s| s.time_s == 63.2).unwrap();
        assert_eq!(step.section_index, 5, "{step:?}");
    }
    // And the first cue of a section that fires before the section starts goes to that section, also when the section before
    // it has extras inside (the 11.12 s cue is the first Verse 1 slide, 1 s before it starts, while the Intro ends at 12.1 s).
    for count in [1, 2] {
        let layout = original().with_blanks(1, 2, count);
        let schedule = assert_same_slides(&layout, "end of the intro");
        let step = schedule.steps.iter().find(|s| s.time_s == 11.12).unwrap();
        assert_eq!(step.section_index, 2, "{step:?}");
    }
}

// ---- what is refused ------------------------------------------------------------------------------------------------------

#[test]
fn up_to_eight_extra_slides_are_accepted_and_nine_are_refused() {
    let f = fixture();
    for count in 1..=8 {
        let layout = original().with_new_group(0, count);
        assert_same_slides(&layout, &format!("{count} extras"));
    }
    for count in [9, 10, 20] {
        let layout = original().with_new_group(0, count);
        let map = SongMap::from_song(&layout.song()).unwrap();
        assert_eq!(
            cue_steps(&map, &f.sections, &f.cues),
            Err(CueProblem::Mismatch {
                max_slide: 53,
                file_cues: 54 + count
            })
        );
        // The song is followed by bars, and the problem says the numbers.
        let schedule = build_schedule(&map, &f.sections, &f.cues, None).unwrap();
        assert_eq!(schedule.source, Source::Bars);
        assert_eq!(schedule.alignment, None);
    }
}

#[test]
fn the_two_line_numbering_is_refused_with_and_without_extra_slides() {
    let f = fixture();
    // A 2 Line template gave slide numbers that stop at 30 while the file has 54 slides.
    let two_line: Vec<LyricCue> = f.cues.iter().copied().filter(|c| c.slide <= 30).collect();
    for layout in [
        original(),
        original().with_new_group(0, 1),
        original().with_blanks(1, 0, 2),
    ] {
        let map = SongMap::from_song(&layout.song()).unwrap();
        let problem = cue_steps(&map, &f.sections, &two_line).unwrap_err();
        assert_eq!(
            problem,
            CueProblem::Mismatch {
                max_slide: 30,
                file_cues: map.cue_total()
            }
        );
        let schedule = build_schedule(&map, &f.sections, &two_line, None).unwrap();
        assert_eq!(schedule.source, Source::Bars);
    }
}

#[test]
fn cues_that_fit_no_way_of_adding_slides_are_refused() {
    // Background group (not played), then A and B of three slides each: 7 cues in the file, Playback numbers 0 to 5.
    let song = |groups: &[(u32, &str, usize)]| {
        let mut built = SyntheticSong::new("Refused").id(5);
        for (key, name, count) in groups {
            let slides: Vec<Option<&str>> = (0..*count)
                .map(|_| if *key == 900 { None } else { Some("la") })
                .collect();
            built = built.group_with_key(*key, name, &slides);
        }
        Song::read(&built.arrangement("Playback", &[1, 2]).selected(0).build()).unwrap()
    };
    let song = song(&[(900, "Background", 1), (1, "A", 3), (2, "B", 3)]);
    let sections = [
        Section {
            name: "A".into(),
            start: 0.0,
            end: 10.0,
        },
        Section {
            name: "B".into(),
            start: 10.0,
            end: 20.0,
        },
    ];
    let at = |list: &[(f64, u32)]| -> Vec<LyricCue> {
        list.iter()
            .map(|(time_s, slide)| LyricCue {
                time_s: *time_s,
                slide: *slide,
            })
            .collect()
    };
    let mismatch = CueProblem::Mismatch {
        max_slide: 5,
        file_cues: 7,
    };
    let map = SongMap::from_song(&song).unwrap();
    // The numbers as they should be: fine (the extra is the background group in front).
    let good = at(&[
        (1.0, 0),
        (4.0, 1),
        (7.0, 2),
        (11.0, 3),
        (14.0, 4),
        (17.0, 5),
    ]);
    assert!(cue_steps(&map, &sections, &good).is_ok());
    // Four slides in a group of three cannot be explained by any extra slide.
    let crowded = at(&[(1.0, 0), (3.0, 1), (5.0, 2), (7.0, 3), (14.0, 4), (17.0, 5)]);
    assert_eq!(cue_steps(&map, &sections, &crowded), Err(mismatch.clone()));
    // The same slide in two groups.
    let twice = at(&[
        (1.0, 0),
        (4.0, 1),
        (7.0, 2),
        (11.0, 2),
        (14.0, 4),
        (17.0, 5),
    ]);
    assert_eq!(cue_steps(&map, &sections, &twice), Err(mismatch));
}

#[test]
fn a_song_with_one_slide_missing_at_the_end_is_an_extra_slide_at_the_end() {
    // Four slides in the file, Playback's numbers stop at slide 2: the last slide is an extra one (it never gets a cue).
    let song = SyntheticSong::new("Short")
        .id(5)
        .group("A", &["a1", "a2"])
        .group("B", &["b1", "b2"])
        .arrangement("Playback", &[0, 1])
        .selected(0)
        .build();
    let song = Song::read(&song).unwrap();
    let sections = [
        Section {
            name: "A".into(),
            start: 0.0,
            end: 10.0,
        },
        Section {
            name: "B".into(),
            start: 10.0,
            end: 20.0,
        },
    ];
    let cues = [
        LyricCue {
            time_s: 1.0,
            slide: 0,
        },
        LyricCue {
            time_s: 4.0,
            slide: 1,
        },
        LyricCue {
            time_s: 11.0,
            slide: 2,
        },
    ];
    let schedule = schedule_for_song(&song, &sections, &cues, None).unwrap();
    assert_eq!(schedule.source, Source::Cues);
    assert_eq!(
        schedule
            .steps
            .iter()
            .map(|s| (s.raw, s.flat_index))
            .collect::<Vec<_>>(),
        vec![(0, 0), (1, 1), (2, 2)]
    );
    assert_eq!(schedule.alignment.unwrap().extras, 1);
}

// ---- the same wording and checks for the song --------------------------------------------------------------------------------

#[test]
fn the_check_names_the_first_section_worth_a_look_and_the_hash_does_not_depend_on_extras() {
    let base = original();
    let hash = |layout: &Layout| {
        SongMap::from_song(&layout.song())
            .unwrap()
            .arrangement_hash()
    };
    let want = hash(&base);
    for layout in [
        base.with_new_group(0, 2),
        base.with_blanks(0, 0, 1),
        base.with_blanks(1, 1, 2),
        base.with_blanks(9, 0, 1),
        base.with_new_group(7, 1),
    ] {
        assert_eq!(hash(&layout), want);
    }
    // Another order of the arrangement is another hash.
    let mut swapped = base.clone();
    swapped.arrangement.swap(2, 3);
    assert_ne!(hash(&swapped), want);
    let mut shorter = base.clone();
    shorter.arrangement.pop();
    assert_ne!(hash(&shorter), want);
}

// ---- lines of text per slide ------------------------------------------------------------------------------------------------

#[test]
fn lines_of_text_are_counted_between_line_breaks() {
    let lines = |text: &str| rtf_visible_lines(text.as_bytes());
    assert_eq!(lines(""), 0);
    assert_eq!(lines("  \n "), 0);
    assert_eq!(lines("one"), 1);
    assert_eq!(lines("one\ntwo"), 2);
    assert_eq!(lines("one\n\ntwo\n"), 2, "a blank line is not a line");
    assert_eq!(lines("a\nb\nc\nd"), 4);
    assert_eq!(
        lines(r"{\rtf1\ansi{\fonttbl\f0 Helvetica;}\pard\f0\fs96 \cf0 Hello\par World\par}"),
        2
    );
    assert_eq!(
        lines("{\\rtf1\\ansi{\\fonttbl\\f0 Arial;}\\f0\\fs96 First line\\\nSecond line}"),
        2
    );
    assert_eq!(lines(r"{\rtf1\ansi A\line B\line C\line D}"), 4);
    assert_eq!(
        lines(r"{\rtf1\ansi{\colortbl;\red0\green0\blue0;}\pard\par }"),
        0
    );
    assert_eq!(
        lines(r"{\rtf1\ansi \pard Just one}"),
        1,
        "\\pard is not a line break"
    );
}

#[test]
fn the_file_says_the_usual_lines_per_text_slide() {
    let song = |groups: &[(&str, Vec<Option<&str>>)]| {
        let mut built = SyntheticSong::new("Lines").id(9);
        for (name, slides) in groups {
            built = built.group_with_blanks(name, slides);
        }
        let bytes = built.arrangement("Playback", &[0]).selected(0).build();
        Song::read(&bytes).unwrap()
    };
    let one = song(&[("A", vec![None, Some("a"), Some("b"), Some("c\nd")])]);
    assert_eq!(one.usual_lines(), Some(1));
    assert_eq!(SongMap::from_song(&one).unwrap().lines_per_slide(), Some(1));
    let two = song(&[
        ("A", vec![None, Some("a\nb"), Some("c\nd"), Some("e")]),
        ("B", vec![Some("f\ng")]),
    ]);
    assert_eq!(two.usual_lines(), Some(2));
    assert_eq!(two.groups[0].cue_lines, vec![0, 2, 2, 1]);
    let four = song(&[("A", vec![Some("a\nb\nc\nd"), Some("a\nb\nc\nd")])]);
    assert_eq!(four.usual_lines(), Some(4));
    // A tie goes to the smaller number; a song with no text has none.
    assert_eq!(
        song(&[("A", vec![Some("a"), Some("b\nc")])]).usual_lines(),
        Some(1)
    );
    assert_eq!(song(&[("A", vec![None, None])]).usual_lines(), None);
    assert_eq!(
        SongMap::new(Vec::new(), Vec::new())
            .unwrap()
            .lines_per_slide(),
        None
    );
}
