//! Lines per slide (design `SPEC.md`, "Revision 3"): how many lines of text each slide has, in the song file and in Playback.
//!
//! Playback's Lyrics Bus template (1, 2 or 4 lines per slide) decides how many slides it counts for each group of the song:
//! a group of `L` lines of text has `ceil(L / n)` slides for a template of `n` lines, and a group with no text at all has one
//! (the blank Count Off). The slides are numbered one group after the other in file order. The song file holds however many
//! lines its slides were made with, and people change them.
//!
//! * [`infer`] finds Playback's `n` from the cues themselves: for each template it works out where each cue would land and how
//!   many groups have exactly the slides that template says. The template that fits clearly most groups wins; a tie or no fit
//!   is "unknown".
//! * [`by_lines`] maps by lines when Playback's `n` is not the file's: Playback's slide `j` of a group shows the lines
//!   `[j * n, (j + 1) * n)` of the group, and the file cues that hold those lines are fired, in turn and evenly spaced when
//!   there are several, never twice in a row. Blank cues have no lines, so slides someone added to the file never matter.
//!
//! Both work on top of the same section-by-time rule as [`super::cue_plan`] (a cue just before a section starts goes by
//! continuity and by room).

use super::{Alignment, CuePlan, CueProblem, LyricCue, Section, SongMap, Step, StepKind};

/// Playback's Lyrics Bus templates, in lines per slide.
pub const TEMPLATES: [usize; 3] = [1, 2, 4];

/// How many slides more than the lines say a group may have played and still be that group (a slide someone added with text).
const SLIDE_TOLERANCE: usize = 1;

/// How Playback numbers the slides of a song under a template of `n` lines per slide.
struct Shape {
    /// Slides of each file group.
    slides: Vec<usize>,
    /// Slide number of each group's first slide.
    start: Vec<usize>,
}

fn shape(map: &SongMap, n: usize) -> Shape {
    let mut slides = Vec::with_capacity(map.groups.len());
    let mut start = Vec::with_capacity(map.groups.len());
    let mut next = 0;
    for group in &map.groups {
        let count = if group.cues == 0 {
            0
        } else {
            group.text_lines().div_ceil(n).max(1)
        };
        start.push(next);
        slides.push(count);
        next += count;
    }
    Shape { slides, start }
}

/// The arrangement entry (the section) of each cue, when Playback numbers `shape` (see [`super::assign`]).
fn assign(
    map: &SongMap,
    sections: &[Section],
    sorted: &[LyricCue],
    shape: &Shape,
) -> Result<Vec<usize>, CueProblem> {
    super::assign(map, sections, sorted, &|g| shape.slides[g])
}

/// The lowest and highest slide played in each file group, over every section that holds the group.
fn runs(map: &SongMap, sorted: &[LyricCue], entry: &[usize]) -> Vec<Option<(u32, u32, usize)>> {
    let mut out: Vec<Option<(u32, u32, std::collections::BTreeSet<u32>)>> =
        vec![None; map.groups.len()];
    for (cue, k) in sorted.iter().zip(entry) {
        let slot = &mut out[map.arrangement[*k]];
        match slot {
            Some((low, high, all)) => {
                *low = (*low).min(cue.slide);
                *high = (*high).max(cue.slide);
                all.insert(cue.slide);
            }
            None => *slot = Some((cue.slide, cue.slide, [cue.slide].into())),
        }
    }
    out.into_iter()
        .map(|run| run.map(|(low, high, all)| (low, high, all.len())))
        .collect()
}

/// How well one template fits the cues.
struct Fit {
    /// Groups with the number of slides the template says (all of them played, in a row).
    exact: usize,
    /// Groups the cues played at all.
    groups: usize,
}

/// `None` when the template cannot be: a group played more slides than it has (and a slide of tolerance), or two groups' slides
/// overlap (groups are numbered one after the other).
fn fit(map: &SongMap, sorted: &[LyricCue], entry: &[usize], shape: &Shape) -> Option<Fit> {
    let (mut exact, mut groups) = (0, 0);
    let mut before: Option<u32> = None;
    for (g, run) in runs(map, sorted, entry).into_iter().enumerate() {
        let Some((low, high, distinct)) = run else {
            continue;
        };
        let span = (high - low) as usize + 1;
        let room = shape.slides[g];
        if span > room + SLIDE_TOLERANCE || before.is_some_and(|b| low <= b) {
            return None;
        }
        before = Some(high);
        groups += 1;
        if span == room && distinct == room {
            exact += 1;
        }
    }
    Some(Fit { exact, groups })
}

/// Playback's lines per slide (1, 2 or 4), worked out from the cues: the template that gives exactly the slides it says for clearly
/// the most groups the cues played. `None` when no template stands out (every group fits all of them, or none fits).
///
/// `sorted` are the cues in time order, without duplicates.
pub fn infer(map: &SongMap, sections: &[Section], sorted: &[LyricCue]) -> Option<usize> {
    if sorted.is_empty() || map.arrangement.len() != sections.len() {
        return None;
    }
    let mut fits: Vec<(usize, Fit)> = Vec::new();
    for n in TEMPLATES {
        let shape = shape(map, n);
        let Ok(entry) = assign(map, sections, sorted, &shape) else {
            return None;
        };
        if let Some(found) = fit(map, sorted, &entry, &shape) {
            fits.push((n, found));
        }
    }
    let best = fits.iter().max_by_key(|(_, f)| f.exact)?;
    let clear = fits
        .iter()
        .filter(|(n, _)| *n != best.0)
        .all(|(_, f)| f.exact < best.1.exact);
    (clear && best.1.exact * 2 > best.1.groups).then_some(best.0)
}

/// Whether every slide of the file is `n` lines, as Playback numbers them for `n` lines per slide: in each group every slide that
/// has text has `n` lines, except that the last may have fewer. Then slide `j` of Playback is cue `j` of the group's text slides.
/// A file with a slide of more or fewer lines than the rest does not, and then the lines, not the count of cues, say which cue a
/// number means.
pub fn slides_match(map: &SongMap, n: usize) -> bool {
    map.groups.iter().all(|group| {
        let text: Vec<usize> = group.lines.iter().copied().filter(|l| *l > 0).collect();
        match text.split_last() {
            Some((last, rest)) => *last <= n && rest.iter().all(|l| *l == n),
            None => true,
        }
    })
}

/// The cues of group `g` that hold the lines of its slide `j` when each slide has `n` lines: those whose lines overlap
/// `[j * n, (j + 1) * n)`. A group with no text at all has its first cue (the blank slide Playback numbers).
pub fn cues_of_slide(map: &SongMap, g: usize, j: usize, n: usize) -> Vec<usize> {
    let group = &map.groups[g];
    let total = group.text_lines();
    if total == 0 {
        return (group.cues > 0).then_some(0).into_iter().collect();
    }
    let (from, to) = (j * n, ((j + 1) * n).min(total));
    let mut at = 0;
    let mut out = Vec::new();
    for (i, lines) in group.lines.iter().enumerate() {
        if *lines > 0 {
            if at < to && at + lines > from {
                out.push(i);
            }
            at += lines;
        }
    }
    out
}

/// The steps for a song whose file has `file` lines per slide while Playback numbers `n` lines per slide (design `SPEC.md`,
/// "Revision 3").
///
/// Playback's slide numbers are anchored per group: a group whose slides were all played starts at its lowest slide; one that was
/// only played in part starts where the slides before it say, within what its played slides allow (the shift of the design's
/// Revision 2, in slides of Playback). Each cue is then slide `j` of its group, which shows the lines `[j * n, (j + 1) * n)`. The
/// file cues that hold them are fired in turn, evenly spaced from the cue to the next cue (or the end of the section); a cue the
/// previous step already shows is not fired again.
///
/// `Err(mismatch)` when no way of numbering fits: a group played more slides than its lines make, or groups overlap.
pub fn by_lines(
    map: &SongMap,
    sections: &[Section],
    sorted: &[LyricCue],
    n: usize,
    file: usize,
    mismatch: CueProblem,
) -> Result<CuePlan, CueProblem> {
    if map.arrangement.len() != sections.len() {
        return Err(mismatch);
    }
    let shape = shape(map, n);
    let entry = assign(map, sections, sorted, &shape)?;
    let runs = runs(map, sorted, &entry);
    // The slide number of each played group's first slide.
    let mut base: Vec<usize> = vec![0; map.groups.len()];
    let mut carry: i64 = 0;
    let mut before: Option<u32> = None;
    for (g, run) in runs.iter().enumerate() {
        let Some((low, high, _)) = run else {
            continue;
        };
        let room = shape.slides[g];
        let span = (high - low) as usize + 1;
        if span > room + SLIDE_TOLERANCE || before.is_some_and(|b| *low <= b) {
            return Err(mismatch);
        }
        before = Some(*high);
        let expected = shape.start[g] as i64;
        base[g] = if span >= room {
            carry = i64::from(*low) - expected;
            *low as usize
        } else {
            let least = (i64::from(*high) + 1 - room as i64).max(0);
            (expected + carry).clamp(least, i64::from(*low)) as usize
        };
    }
    let mut steps: Vec<Step> = Vec::with_capacity(sorted.len());
    for (i, cue) in sorted.iter().enumerate() {
        let k = entry[i];
        let g = map.arrangement[k];
        let room = shape.slides[g].max(1);
        let j = (cue.slide as usize).saturating_sub(base[g]).min(room - 1);
        let mut targets = cues_of_slide(map, g, j, n);
        if targets.first().is_some_and(|first| {
            steps.last().map(|s| s.flat_index) == Some(map.entry_base[k] + first)
        }) {
            targets.remove(0);
        }
        if targets.is_empty() {
            continue;
        }
        // The cues of the span, evenly spaced from this cue to the next one (or the end of the section).
        let next = sorted[i + 1..]
            .iter()
            .map(|c| c.time_s)
            .find(|t| *t > cue.time_s);
        let limit = next.map_or(sections[k].end, |t| t.min(sections[k].end));
        let span = (limit - cue.time_s).max(0.0);
        let count = targets.len();
        for (m, inside) in targets.into_iter().enumerate() {
            steps.push(Step {
                time_s: cue.time_s + span * m as f64 / count as f64,
                flat_index: map.entry_base[k] + inside,
                raw: map.group_start[g] + inside,
                section_index: k,
                kind: StepKind::Cue,
            });
        }
    }
    if steps.is_empty() {
        return Err(mismatch);
    }
    let check_section = steps.first().map_or(0, |s| s.section_index);
    Ok(CuePlan {
        steps,
        alignment: Some(Alignment {
            extras: 0,
            shifts: vec![0; map.groups.len()],
            check_section,
            uncertain: false,
            lines: Some(LineFit { playback: n, file }),
        }),
    })
}

/// Playback and the file count lines per slide differently, and the song is followed by lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineFit {
    /// Lines per slide of Playback's Lyrics Bus template (1, 2 or 4).
    pub playback: usize,
    /// The usual lines per slide of the song file.
    pub file: usize,
}
