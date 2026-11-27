//! Lines per slide (design `SPEC.md`, "Revision 3"): Playback counts 1, 2 or 4 lines per slide, the song file has the lines it was
//! made with. The numeric fixtures are one real song in Playback's three templates (numbers only); the song files are made up, every
//! cue with a text of its own, so a test can say which cue of the file a step shows.
//!
//! The ORACLE below is written apart from the code under test: it lays the file's lines out one after the other, cuts each group
//! into chunks of `n` lines, and calls chunk number `j` of a group Playback's slide `j` of it. A step must fire exactly the file
//! cues that hold the lines of the slide the cue names.

use super::align_tests::{fixture, original, Layout, ARRANGEMENT};
use super::prepare::Readiness;
use super::*;
use serde_json::Value;

const TEMPLATES_JSON: &str = include_str!("../../tests/fixtures/oh-my-soul-templates.fixture.json");

/// A Playback series of the templates fixture: `[time_s, slide]` pairs.
fn series(name: &str) -> Vec<LyricCue> {
    let v: Value = serde_json::from_str(TEMPLATES_JSON).unwrap();
    v[name]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| LyricCue {
            time_s: pair[0].as_f64().unwrap(),
            slide: pair[1].as_u64().unwrap() as u32,
        })
        .collect()
}

// ---- the oracle -------------------------------------------------------------------------------------------------------------

fn lines_of(cue: &Option<String>) -> usize {
    cue.as_deref().map_or(0, |text| {
        text.split('\n').filter(|l| !l.trim().is_empty()).count()
    })
}

/// Per file group, per slide of Playback under `n` lines per slide: the raw cues of the file that hold the slide's lines. A group
/// with no text at all is one slide that is its first cue.
fn oracle_groups(layout: &Layout, n: usize) -> Vec<Vec<Vec<usize>>> {
    let mut raw = 0;
    let mut out = Vec::new();
    for (_, _, cues) in &layout.groups {
        let mut owner = Vec::new();
        for (i, cue) in cues.iter().enumerate() {
            owner.extend(std::iter::repeat_n(raw + i, lines_of(cue)));
        }
        let slides: Vec<Vec<usize>> = if cues.is_empty() {
            Vec::new()
        } else if owner.is_empty() {
            vec![vec![raw]]
        } else {
            owner
                .chunks(n)
                .map(|chunk| {
                    let mut chunk = chunk.to_vec();
                    chunk.dedup();
                    chunk
                })
                .collect()
        };
        out.push(slides);
        raw += cues.len();
    }
    out
}

/// Playback's slide number (the index) to the raw cues it shows.
fn oracle_slides(layout: &Layout, n: usize) -> Vec<Vec<usize>> {
    oracle_groups(layout, n).into_iter().flatten().collect()
}

/// Sections of 10 s, one for each entry of the arrangement, and the cues Playback sends when each section plays every slide of its
/// group once, evenly over the section (no cue for a slide that has no text).
fn playback_run(layout: &Layout, n: usize) -> (Vec<Section>, Vec<LyricCue>) {
    let groups = oracle_groups(layout, n);
    let mut first = Vec::new();
    let mut next = 0;
    for slides in &groups {
        first.push(next);
        next += slides.len();
    }
    let raw = layout.raw();
    let (mut sections, mut cues) = (Vec::new(), Vec::new());
    for (k, g) in layout.arrangement.iter().enumerate() {
        let start = k as f64 * 10.0;
        sections.push(Section {
            name: layout.groups[*g].1.clone(),
            start,
            end: start + 10.0,
        });
        let slides = &groups[*g];
        for (j, shown) in slides.iter().enumerate() {
            if shown.iter().all(|r| raw[*r].is_none()) {
                continue;
            }
            cues.push(LyricCue {
                time_s: start + 1.0 + j as f64 * 8.0 / slides.len() as f64,
                slide: (first[*g] + j) as u32,
            });
        }
    }
    (sections, cues)
}

/// [`assert_follows`] for a file that is `numbered` (the song as Playback counted it) with nothing added.
fn assert_follows(
    layout: &Layout,
    sections: &[Section],
    cues: &[LyricCue],
    n: usize,
    label: &str,
) -> Schedule {
    assert_follows_numbered(layout, layout, sections, cues, n, label)
}

/// The schedule must follow by cues and fire, for each cue, exactly the file cues that hold the lines of its slide, one after the
/// other, evenly spaced up to the next cue; a cue the step before already shows is not fired again.
///
/// Playback's slide numbers come from `numbered` (the song as it was when the template was made); `layout` is the file now, which
/// may hold extra blank cues. The cue a slide names is found in `layout` by its text.
fn assert_follows_numbered(
    layout: &Layout,
    numbered: &Layout,
    sections: &[Section],
    cues: &[LyricCue],
    n: usize,
    label: &str,
) -> Schedule {
    let map = SongMap::from_song(&layout.song()).unwrap();
    let schedule = build_schedule(&map, sections, cues, None).unwrap();
    assert_eq!(
        schedule.source,
        Source::Cues,
        "{label}: {:?}",
        schedule.cue_problem
    );
    let (raw_text, flat_text) = (layout.raw(), layout.flat());
    let slides: Vec<Vec<usize>> = oracle_slides(numbered, n)
        .into_iter()
        .map(|raws| {
            raws.into_iter()
                .map(|r| {
                    let text = numbered.raw()[r].clone();
                    raw_text.iter().position(|t| *t == text).unwrap()
                })
                .collect()
        })
        .collect();
    let mut sorted = cues.to_vec();
    sorted.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
    let mut seen = 0;
    let mut previous: Option<(usize, usize)> = None;
    for (k, cue) in sorted.iter().enumerate() {
        let until = sorted.get(k + 1).map_or(f64::INFINITY, |c| c.time_s);
        let got: Vec<&Step> = schedule
            .steps
            .iter()
            .filter(|s| s.time_s >= cue.time_s && s.time_s < until)
            .collect();
        seen += got.len();
        let mut want = slides[cue.slide as usize].clone();
        if let Some((section, raw)) = previous {
            if want.first() == Some(&raw) && got.first().is_none_or(|s| s.section_index == section)
            {
                want.remove(0);
            }
        }
        assert_eq!(
            got.iter().map(|s| s.raw).collect::<Vec<_>>(),
            want,
            "{label}: slide {} at {} s",
            cue.slide,
            cue.time_s
        );
        for step in &got {
            assert!(raw_text[step.raw].is_some(), "{label}: a blank slide fired");
            assert_eq!(
                flat_text[step.flat_index], raw_text[step.raw],
                "{label}: the flat number shows another cue than raw"
            );
        }
        // Evenly spaced, and never later than the cue after.
        let gaps: Vec<f64> = got.windows(2).map(|w| w[1].time_s - w[0].time_s).collect();
        assert!(
            gaps.windows(2).all(|g| (g[0] - g[1]).abs() < 1e-9),
            "{label}: {gaps:?}"
        );
        if let Some(last) = got.last() {
            previous = Some((last.section_index, last.raw));
        }
    }
    assert_eq!(
        seen,
        schedule.steps.len(),
        "{label}: no step before the first cue or after the last"
    );
    schedule
}

fn lines_fit(schedule: &Schedule) -> Option<LineFit> {
    schedule.alignment.as_ref().and_then(|a| a.lines)
}

// ---- Playback's numbers in the real recording --------------------------------------------------------------------------------

#[test]
fn the_one_line_numbering_gives_exactly_the_flat_slide_numbers_it_always_did() {
    let f = fixture();
    let layout = original();
    let map = SongMap::from_song(&layout.song()).unwrap();
    assert_eq!(map.lines_per_slide(), Some(1));
    let schedule = build_schedule(&map, &f.sections, &f.cues, None).unwrap();
    assert_eq!(schedule.source, Source::Cues);
    assert_eq!(
        schedule.alignment, None,
        "nothing added, one line per slide: the exact layout"
    );
    assert_eq!(schedule.playback_lines, Some(1));
    assert_eq!(schedule.steps.len(), f.cues.len());
    // The flat number of a cue is the cues before its section in the arrangement plus the cue's place in the group: counted here
    // from the group sizes alone.
    let sizes: Vec<usize> = f.groups.iter().map(|g| g.1).collect();
    let starts: Vec<usize> = sizes
        .iter()
        .scan(0, |at, c| {
            let start = *at;
            *at += c;
            Some(start)
        })
        .collect();
    let mut before = Vec::new();
    let mut flat = 0;
    for g in ARRANGEMENT {
        before.push(flat);
        flat += sizes[g];
    }
    for (step, cue) in schedule.steps.iter().zip(&f.cues) {
        assert_eq!(
            step.raw, cue.slide as usize,
            "the global raw index, as before"
        );
        let group = (0..sizes.len())
            .rev()
            .find(|g| starts[*g] <= step.raw)
            .unwrap();
        assert_eq!(ARRANGEMENT[step.section_index], group);
        assert_eq!(
            step.flat_index,
            before[step.section_index] + step.raw - starts[group]
        );
    }
}

#[test]
fn playbacks_lines_per_slide_are_read_from_the_cues_of_each_template() {
    let f = fixture();
    let map = SongMap::from_song(&original().song()).unwrap();
    assert_eq!(infer_playback_lines(&map, &f.sections, &f.cues), Some(1));
    assert_eq!(
        infer_playback_lines(&map, &f.sections, &series("two_line")),
        Some(2)
    );
    assert_eq!(
        infer_playback_lines(&map, &f.sections, &series("four_line")),
        Some(4)
    );
    // The same numbers in the same order, however the cues come.
    let mut shuffled = series("four_line");
    shuffled.reverse();
    assert_eq!(infer_playback_lines(&map, &f.sections, &shuffled), Some(4));
}

#[test]
fn the_two_line_and_four_line_series_point_at_the_file_cues_that_hold_the_same_lines() {
    let f = fixture();
    for (name, n) in [("two_line", 2), ("four_line", 4)] {
        let cues = series(name);
        let layout = original();
        let schedule = assert_follows(&layout, &f.sections, &cues, n, name);
        assert_eq!(
            lines_fit(&schedule),
            Some(LineFit {
                playback: n,
                file: 1
            }),
            "{name}"
        );
        assert_eq!(schedule.playback_lines, Some(n));
        // 54 one-line cues, one blank: every text cue of the song is shown at least once when each slide is played at all.
        let shown: std::collections::BTreeSet<usize> =
            schedule.steps.iter().map(|s| s.raw).collect();
        assert_eq!(
            shown.len(),
            53,
            "{name}: every lyric cue, the Count Off never"
        );
    }
    // The first slides of the 4 Line series, worked out by hand: slide 1 is the Intro's two lines (cues 1 and 2) over the time to
    // the next cue; slide 2 the first four-line verse, cues 3 to 6, evenly over 11.12 s to 27.48 s.
    let schedule = assert_follows(&original(), &f.sections, &series("four_line"), 4, "by hand");
    let first: Vec<(f64, usize)> = schedule
        .steps
        .iter()
        .take(6)
        .map(|s| ((s.time_s * 1000.0).round() / 1000.0, s.raw))
        .collect();
    assert_eq!(
        first,
        vec![
            (3.53, 1),
            (7.325, 2),
            (11.12, 3),
            (15.21, 4),
            (19.3, 5),
            (23.39, 6)
        ]
    );
}

#[test]
fn a_two_line_template_with_extra_blank_slides_in_the_file_still_fires_the_right_lyrics() {
    let f = fixture();
    let cues = series("two_line");
    let baseline = assert_follows(&original(), &f.sections, &cues, 2, "nothing added");
    let texts = |layout: &Layout, schedule: &Schedule| -> Vec<Option<String>> {
        let raw = layout.raw();
        schedule.steps.iter().map(|s| raw[s.raw].clone()).collect()
    };
    let want = texts(&original(), &baseline);
    let layouts = [
        ("new first group of 2", original().with_new_group(0, 2)),
        (
            "1 at the start of the Count Off",
            original().with_blanks(0, 0, 1),
        ),
        (
            "2 at the start of the Intro",
            original().with_blanks(1, 0, 2),
        ),
        ("1 inside the first verse", original().with_blanks(2, 2, 1)),
        ("2 inside a bridge", original().with_blanks(10, 3, 2)),
        ("between two groups", original().with_new_group(7, 2)),
        ("1 at the end of the file", original().with_blanks(17, 1, 1)),
        (
            "9 at the start (more than Rev 2 allows)",
            original().with_new_group(0, 9),
        ),
    ];
    for (label, layout) in &layouts {
        let schedule = assert_follows_numbered(layout, &original(), &f.sections, &cues, 2, label);
        assert_eq!(texts(layout, &schedule), want, "{label}");
        assert_eq!(
            lines_fit(&schedule),
            Some(LineFit {
                playback: 2,
                file: 1
            }),
            "{label}"
        );
        assert_eq!(
            schedule.steps.iter().map(|s| s.time_s).collect::<Vec<_>>(),
            baseline.steps.iter().map(|s| s.time_s).collect::<Vec<_>>(),
            "{label}: the same times"
        );
    }
}

#[test]
fn the_four_line_series_with_extra_slides_too() {
    let f = fixture();
    let cues = series("four_line");
    for layout in [
        original().with_new_group(0, 2),
        original().with_blanks(1, 1, 1),
        original().with_blanks(13, 4, 2),
    ] {
        assert_follows_numbered(
            &layout,
            &original(),
            &f.sections,
            &cues,
            4,
            "four line with extras",
        );
    }
}

// ---- made-up songs with the lines they say ------------------------------------------------------------------------------------

/// A song: a blank Count Off, then each given group (the lines of each slide; `0` is a blank slide), played in `arrangement`.
fn made_up(groups: &[(&str, &[usize])], arrangement: &[usize]) -> Layout {
    let mut out = vec![(0u32, String::new(), vec![None])];
    for (g, (name, slides)) in groups.iter().enumerate() {
        let cues = slides
            .iter()
            .enumerate()
            .map(|(c, lines)| {
                (*lines > 0).then(|| {
                    (0..*lines)
                        .map(|l| format!("g{}c{c}l{l}", g + 1))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            })
            .collect();
        out.push((g as u32 + 1, name.to_string(), cues));
    }
    Layout {
        groups: out,
        arrangement: arrangement.to_vec(),
    }
}

#[test]
fn four_line_verses_of_one_line_slides_are_followed_by_lines() {
    let layout = made_up(
        &[
            ("Verse", &[1, 1, 1, 1]),
            ("Chorus", &[1, 1, 1, 1, 1, 1, 1, 1]),
        ],
        &[0, 1, 2, 1, 2],
    );
    let (sections, cues) = playback_run(&layout, 4);
    let schedule = assert_follows(&layout, &sections, &cues, 4, "4 line");
    assert_eq!(schedule.playback_lines, Some(4));
    assert_eq!(
        lines_fit(&schedule),
        Some(LineFit {
            playback: 4,
            file: 1
        })
    );
    // The verse is one Playback slide: its four cues are fired one after another, a quarter of the way apart from the cue at 11 s
    // to the end of the section at 20 s (the next cue is in the chorus).
    let verse: Vec<f64> = schedule
        .steps
        .iter()
        .filter(|s| s.section_index == 1)
        .map(|s| s.time_s)
        .collect();
    assert_eq!(verse, vec![11.0, 13.25, 15.5, 17.75]);
    // The same song in 2 Line numbering.
    let (sections, cues) = playback_run(&layout, 2);
    assert_follows(&layout, &sections, &cues, 2, "2 line");
}

#[test]
fn a_file_of_two_line_slides_with_a_one_line_template_fires_each_cue_once() {
    let layout = made_up(
        &[("Verse", &[2, 2, 2]), ("Chorus", &[2, 2, 1])],
        &[0, 1, 2, 1, 2],
    );
    let (sections, cues) = playback_run(&layout, 1);
    let schedule = assert_follows(&layout, &sections, &cues, 1, "1 line over 2 line slides");
    assert_eq!(schedule.playback_lines, Some(1));
    assert_eq!(
        lines_fit(&schedule),
        Some(LineFit {
            playback: 1,
            file: 2
        })
    );
    // Six lines of the verse are six Playback slides but three file cues: no cue is fired twice in a row.
    let verse = schedule
        .steps
        .iter()
        .filter(|s| s.section_index == 1)
        .count();
    assert_eq!(verse, 3);
}

#[test]
fn a_three_line_slide_among_two_line_slides_is_followed_by_lines_though_both_count_two() {
    let layout = made_up(
        &[("Verse", &[2, 3, 2]), ("Chorus", &[2, 2, 2, 2])],
        &[0, 1, 2, 1, 2],
    );
    let map = SongMap::from_song(&layout.song()).unwrap();
    assert_eq!(map.lines_per_slide(), Some(2));
    assert_eq!(map.groups()[1].lines, vec![2, 3, 2]);
    assert_eq!(map.groups()[1].text_lines(), 7);
    let (sections, cues) = playback_run(&layout, 2);
    // 7 lines are 4 slides of Playback, 3 slides of the file.
    assert_eq!(oracle_groups(&layout, 2)[1].len(), 4);
    let schedule = assert_follows(&layout, &sections, &cues, 2, "3 line slide");
    assert_eq!(schedule.playback_lines, Some(2));
    assert_eq!(
        lines_fit(&schedule),
        Some(LineFit {
            playback: 2,
            file: 2
        })
    );
    // Slides 1..4 of the verse show lines 0-1, 2-3, 4-5 and 6: cues 1, 2, 3 (cue 2 holds lines 2 to 4, cue 3 lines 5 and 6) with
    // the repeats left out.
    let verse: Vec<usize> = schedule
        .steps
        .iter()
        .filter(|s| s.section_index == 1)
        .map(|s| s.raw)
        .collect();
    assert_eq!(verse, vec![1, 2, 3]);
}

#[test]
fn blank_slides_inside_the_groups_have_no_lines_and_are_never_fired() {
    let layout = made_up(
        &[
            ("Verse", &[1, 0, 1, 1, 0, 1]),
            ("Chorus", &[0, 1, 1, 1, 1]),
            ("Tag", &[0, 0]),
        ],
        &[0, 1, 2, 3, 2],
    );
    let (sections, cues) = playback_run(&layout, 2);
    let schedule = assert_follows(&layout, &sections, &cues, 2, "blanks inside");
    assert!(schedule.steps.iter().all(|s| layout.raw()[s.raw].is_some()));
    let (sections, cues) = playback_run(&layout, 4);
    assert_follows(&layout, &sections, &cues, 4, "blanks inside, 4 lines");
}

#[test]
fn a_song_whose_template_cannot_be_told_falls_back_to_bars_and_says_why() {
    // 20 groups of one slide each: every template numbers them the same way, so the cues do not say which one it was, and the
    // numbers stop far short of the file.
    let slides: Vec<(String, Vec<usize>)> = (0..20).map(|g| (format!("G{g}"), vec![1])).collect();
    let groups: Vec<(&str, &[usize])> = slides
        .iter()
        .map(|(n, s)| (n.as_str(), s.as_slice()))
        .collect();
    let arrangement: Vec<usize> = (0..=20).collect();
    let layout = made_up(&groups, &arrangement);
    let (sections, _) = playback_run(&layout, 1);
    let cues: Vec<LyricCue> = (1..=3)
        .map(|slide| LyricCue {
            time_s: slide as f64 * 10.0 + 1.0,
            slide,
        })
        .collect();
    let map = SongMap::from_song(&layout.song()).unwrap();
    assert_eq!(infer_playback_lines(&map, &sections, &cues), None);
    let schedule = build_schedule(&map, &sections, &cues, None).unwrap();
    assert_eq!(schedule.source, Source::Bars);
    assert_eq!(schedule.playback_lines, None);
    assert!(matches!(
        schedule.cue_problem,
        Some(CueProblem::Mismatch { max_slide: 3, .. })
    ));
    assert!(schedule.steps.iter().all(|s| s.kind == StepKind::Bars));
}

#[test]
fn a_group_that_fits_no_template_is_a_real_non_fit_and_stays_a_mismatch() {
    // Eight one-line slides in each group; the first group is played over nine slide numbers that are not all of them: no template
    // makes that (1 would need all eight, 2 and 4 have too few slides), and the numbers stop far short of the file's 25 cues.
    let layout = made_up(
        &[("A", &[1; 8]), ("B", &[1; 8]), ("C", &[1; 8])],
        &[0, 1, 2, 3],
    );
    let (sections, _) = playback_run(&layout, 1);
    let cues: Vec<LyricCue> = [
        (1.0, 1),
        (2.0, 3),
        (3.0, 5),
        (4.0, 7),
        (5.0, 8),
        (6.0, 9),
        (11.0, 10),
        (12.0, 11),
        (21.0, 12),
    ]
    .iter()
    .map(|(t, s)| LyricCue {
        time_s: 10.0 + t,
        slide: *s,
    })
    .collect();
    let map = SongMap::from_song(&layout.song()).unwrap();
    let schedule = build_schedule(&map, &sections, &cues, None).unwrap();
    assert_eq!(schedule.source, Source::Bars);
    assert!(matches!(
        schedule.cue_problem,
        Some(CueProblem::Mismatch { .. })
    ));
}

#[test]
fn which_cues_hold_the_lines_of_a_slide() {
    let layout = made_up(&[("Verse", &[2, 3, 2]), ("Blank", &[0, 0])], &[0, 1, 2]);
    let map = SongMap::from_song(&layout.song()).unwrap();
    let of = |g, j, n| lines::cues_of_slide(&map, g, j, n);
    assert_eq!(of(1, 0, 2), vec![0]);
    assert_eq!(of(1, 1, 2), vec![1]);
    assert_eq!(of(1, 2, 2), vec![1, 2]);
    assert_eq!(of(1, 3, 2), vec![2]);
    assert_eq!(of(1, 0, 4), vec![0, 1]);
    assert_eq!(of(1, 1, 4), vec![1, 2]);
    assert_eq!(of(1, 0, 1), vec![0]);
    assert_eq!(of(1, 2, 1), vec![1]);
    // A group with no text is its first cue; the Count Off too.
    assert_eq!(of(2, 0, 2), vec![0]);
    assert_eq!(of(0, 0, 4), vec![0]);
}

#[test]
fn the_words_for_a_song_followed_by_lines() {
    let by_lines = Readiness::CuesByLines {
        playback: 2,
        file: 1,
    };
    assert_eq!(
        by_lines.text(),
        "Aligned by lines (Playback 2 per slide, the file 1 per slide)"
    );
    assert_eq!(by_lines.code(), "cues_by_lines");
    assert_eq!(by_lines.following(), Some("cues"));
    assert_eq!(
        by_lines.advice_for(Some(1)).as_deref(),
        Some("Matching the template is still best: in Playback choose a ProPresenter 1 Line Template for this song")
    );
    assert_eq!(
        by_lines.advice_for(Some(4)).as_deref(),
        Some("Matching the template is still best: in Playback choose the ProPresenter 4 Line Template for this song")
    );
}
