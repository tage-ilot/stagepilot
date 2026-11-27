//! Findings from the rig (2026-11-27): the real song "Father's House" in Playback's 1, 2 and 4 Line templates. Numbers only (the
//! fixtures hold sections, cue times and slide numbers, group names and sizes); the song file is made up, every cue a text of its
//! own, one line each, as the real file has.
//!
//! What this song has that the Oh My Soul fixture lacks: groups of 1 and 2 lines, a 7-line Bridge, two `Tag` groups played four
//! times (the first one three times in a row, then the second), and cues that fire up to 3.4 s (one bar) before their section.

use super::align_tests::Layout;
use super::lines_tests::{assert_follows, lines_fit};
use super::*;
use serde_json::Value;

const STRUCTURE: &str = include_str!("../../tests/fixtures/fh-structure.fixture.json");
const ONE_LINE: &str = include_str!("../../tests/fixtures/fh-cues-1line.fixture.json");
const TWO_LINE: &str = include_str!("../../tests/fixtures/fh-cues-2line.fixture.json");

/// The `Playback` arrangement as positions in the file's groups. The four `Tag` entries are the file's two `Tag` groups (13 and
/// 14) in the order Playback's cues show: slides 76 and 77 three times, then 78 and 79.
const ARRANGEMENT: [usize; 19] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 13, 13, 14, 15, 16,
];
/// Section positions of the four `Tag` entries and the first flat slide of the first one.
const TAGS: std::ops::Range<usize> = 13..17;
const FIRST_TAG_FLAT: usize = 76;

fn layout() -> Layout {
    let v: Value = serde_json::from_str(STRUCTURE).unwrap();
    let groups = v["groups"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(g, pair)| {
            let cues = pair[1].as_u64().unwrap() as usize;
            let slides = (0..cues)
                .map(|c| (g != 0).then(|| format!("g{g}c{c}")))
                .collect();
            (g as u32, pair[0].as_str().unwrap().to_string(), slides)
        })
        .collect();
    Layout {
        groups,
        arrangement: ARRANGEMENT.to_vec(),
    }
}

fn read(json: &str) -> (Vec<Section>, Vec<LyricCue>) {
    let v: Value = serde_json::from_str(json).unwrap();
    let sections = v["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| Section {
            name: s[0].as_str().unwrap().to_string(),
            start: s[1].as_f64().unwrap(),
            end: s[2].as_f64().unwrap(),
        })
        .collect();
    let cues = v["cues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| LyricCue {
            time_s: c[0].as_f64().unwrap(),
            slide: c[1].as_u64().unwrap() as u32,
        })
        .collect();
    (sections, cues)
}

/// First slide number of each file group when Playback numbers `n` lines per slide.
fn starts(layout: &Layout, n: usize) -> Vec<usize> {
    let mut next = 0;
    layout
        .groups
        .iter()
        .map(|(_, _, slides)| {
            let at = next;
            let lines = slides.iter().filter(|s| s.is_some()).count();
            next += lines.div_ceil(n).max(1);
            at
        })
        .collect()
}

/// The 4 Line series: the real recording of the 2 Line template (so the real times) turned into 4 lines per slide (the series
/// itself was not saved): slide `j` of a group of 4-line slides is the first of the pair of 2-line slides `2j`, `2j + 1`.
fn four_line(layout: &Layout, cues: &[LyricCue]) -> Vec<LyricCue> {
    let (two, four) = (starts(layout, 2), starts(layout, 4));
    cues.iter()
        .filter_map(|c| {
            let g = (0..two.len()).rev().find(|g| two[*g] <= c.slide as usize)?;
            let j = c.slide as usize - two[g];
            j.is_multiple_of(2).then(|| LyricCue {
                time_s: c.time_s,
                slide: (four[g] + j / 2) as u32,
            })
        })
        .collect()
}

fn series(n: usize) -> (Vec<Section>, Vec<LyricCue>) {
    match n {
        1 => read(ONE_LINE),
        2 => read(TWO_LINE),
        _ => {
            let (sections, cues) = read(TWO_LINE);
            (sections, four_line(&layout(), &cues))
        }
    }
}

#[test]
fn the_recordings_have_the_numbers_the_rig_reported() {
    let layout = layout();
    assert_eq!(layout.raw().len(), 89);
    let (_, one) = series(1);
    let (_, two) = series(2);
    let (_, four) = series(4);
    assert_eq!([one.len(), two.len(), four.len()], [92, 48, 28]);
    assert_eq!(one.iter().map(|c| c.slide).max(), Some(88));
    assert_eq!(two.iter().map(|c| c.slide).max(), Some(46));
    assert_eq!(four.iter().map(|c| c.slide).max(), Some(26));
}

#[test]
fn every_template_follows_the_real_one_line_song_by_cues() {
    let layout = layout();
    for n in [1, 2, 4] {
        let (sections, cues) = series(n);
        let label = format!("{n} Line");
        let map = SongMap::from_song(&layout.song()).unwrap();
        assert_eq!(map.lines_per_slide(), Some(1), "{label}");
        let schedule = assert_follows(&layout, &sections, &cues, n, &label);
        assert_eq!(schedule.playback_lines, Some(n), "{label}");
        if n == 1 {
            assert_eq!(schedule.alignment, None, "{label}: the exact layout");
        } else {
            assert_eq!(
                lines_fit(&schedule),
                Some(LineFit {
                    playback: n,
                    file: 1
                }),
                "{label}"
            );
        }
    }
}

#[test]
fn the_first_slide_of_a_repeat_goes_to_that_repeat_not_the_one_before() {
    let layout = layout();
    let map = SongMap::from_song(&layout.song()).unwrap();
    let (sections, cues) = series(1);
    let schedule = build_schedule(&map, &sections, &cues, None).unwrap();
    assert_eq!(schedule.source, Source::Cues);
    // The cues of the four Tag sections (the first slide of a repeat arrives 3.2 s before its section starts).
    let tags: Vec<(f64, usize)> = schedule
        .steps
        .iter()
        .filter(|s| (287.0..318.0).contains(&s.time_s))
        .map(|s| (s.time_s, s.flat_index))
        .collect();
    let want = [
        (287.486, 76),
        (293.224, 77),
        (294.246, 78),
        (299.984, 79),
        (301.007, 80),
        (306.745, 81),
        (307.627, 82),
        (313.478, 83),
        (315.15, 84),
    ];
    assert_eq!(tags, want);
    // Every step's section is the repeat its flat slide belongs to (arrangement order).
    for step in &schedule.steps {
        let entry = step.section_index;
        let end = map
            .entry_base
            .get(entry + 1)
            .copied()
            .unwrap_or(map.flat_total());
        assert!(
            (map.entry_base[entry]..end).contains(&step.flat_index),
            "{step:?}"
        );
    }
    assert_eq!(TAGS.len(), 4);
    assert_eq!(FIRST_TAG_FLAT, map.entry_base[TAGS.start]);
}
