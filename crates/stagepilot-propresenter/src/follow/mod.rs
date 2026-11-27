//! Following Playback with the lyrics in ProPresenter: the pure part (no ProPresenter contact, no clock, no files).
//!
//! Playback's Library Link gives the lyric cues of a song (`time_s` and a slide number; the slide number is the 0-based RAW
//! cue index in the song file: the cues of every group in file order, whatever the arrangement). ProPresenter's
//! `GET /v1/presentation/active/{n}/trigger` follows the SELECTED arrangement, so a raw index has to become the FLATTENED
//! position `n` in the `Playback` arrangement (the copy the Presenter Link writes: one entry per Playback section, repeats
//! separate). This module does that, checks the cues against the file, and, for a song with no usable cues, spreads the
//! text slides over each section by bars.
//!
//! Rules (design `SPEC.md`, "How it works" 2 to 4):
//! * `n = (cues in the arrangement entries before p) + (raw - first raw index of that group)`, where `p` is the arrangement
//!   entry of the section being played. A cue belongs to exactly one file group (by its raw index, never by name); of the
//!   arrangement entries that hold that group, the one nearest the section position `k` found from the time is `p` (ties go
//!   to the earlier entry). `k` is the latest section with `start - 2 s <= time_s`; the cue must also lie before that
//!   section's end. This keeps repeated groups, same-named groups and background slides right.
//! * Cues are used only when every cue resolves, the highest slide number is the file's cue total minus one (no slide is at
//!   or past the total), and there are at least three. Anything else is a `CueProblem`, said in plain words, and the song
//!   falls back to bars.
//! * Bars: the first slide of each section fires at the section start; the other text slides are spread evenly over the
//!   section (snapped to bar lines when the caller holds the beat grid). Background-only slides are never spread over.

pub mod binding;
pub mod engine;
pub mod prepare;

use crate::pro_file::Song;
use sha2::{Digest, Sha256};

/// How far before a section's start a cue may come and still be that section's (Playback sends cues ahead of the lyric).
pub const SECTION_LEAD_S: f64 = 2.0;
/// Fewest cues a song needs to be followed by them.
pub const MIN_CUES: usize = 3;
/// The arrangement the Presenter Link writes and the follower reads.
pub use crate::link::add_songs::PLAYBACK_ARRANGEMENT;

/// One lyric cue from Playback: the second it fires on Playback's played clock, and the raw cue index in the song file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LyricCue {
    pub time_s: f64,
    pub slide: u32,
}

/// One section of Playback's arrangement, in playing order (repeats are separate entries).
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub name: String,
    pub start: f64,
    pub end: f64,
}

/// One group of the song file, in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileGroup {
    pub uuid: String,
    pub name: String,
    /// How many cues (slides) the group has.
    pub cues: usize,
    /// Per cue, whether it has a text element with a visible character (`cues` entries).
    pub has_text: Vec<bool>,
}

/// Why the song file cannot be mapped at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapError {
    /// The file has no arrangement named `Playback`.
    NoPlaybackArrangement,
    /// `Playback` exists but is not the selected arrangement, so ProPresenter's slide numbers would not follow it.
    PlaybackNotSelected,
    /// The arrangement names a group the file does not have.
    UnknownGroup,
    /// A group says it has a number of cues that does not agree with its text list (a bug in the caller).
    Inconsistent,
}

/// The song file as far as following needs it: the groups in file order and the `Playback` arrangement as group positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongMap {
    groups: Vec<FileGroup>,
    /// Position in `groups` of each arrangement entry.
    arrangement: Vec<usize>,
    /// Raw index of the first cue of each group.
    group_start: Vec<usize>,
    /// Flat position of the first cue of each arrangement entry.
    entry_base: Vec<usize>,
}

impl SongMap {
    /// From groups in file order and an arrangement given as positions in `groups`.
    pub fn new(groups: Vec<FileGroup>, arrangement: Vec<usize>) -> Result<Self, MapError> {
        if groups.iter().any(|g| g.has_text.len() != g.cues) {
            return Err(MapError::Inconsistent);
        }
        if arrangement.iter().any(|g| *g >= groups.len()) {
            return Err(MapError::UnknownGroup);
        }
        let mut group_start = Vec::with_capacity(groups.len());
        let mut total = 0;
        for group in &groups {
            group_start.push(total);
            total += group.cues;
        }
        let mut entry_base = Vec::with_capacity(arrangement.len());
        let mut flat = 0;
        for g in &arrangement {
            entry_base.push(flat);
            flat += groups[*g].cues;
        }
        Ok(Self {
            groups,
            arrangement,
            group_start,
            entry_base,
        })
    }

    /// From a song file read by `pro_file::Song::read`: its `Playback` arrangement, which must also be the selected one.
    pub fn from_song(song: &Song) -> Result<Self, MapError> {
        let arrangement = song
            .arrangements
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(PLAYBACK_ARRANGEMENT))
            .ok_or(MapError::NoPlaybackArrangement)?;
        if song.selected.as_deref().map(str::to_ascii_lowercase)
            != Some(arrangement.uuid.to_ascii_lowercase())
        {
            return Err(MapError::PlaybackNotSelected);
        }
        let groups: Vec<FileGroup> = song
            .groups
            .iter()
            .map(|g| FileGroup {
                uuid: g.uuid.clone(),
                name: g.name.clone(),
                cues: g.cue_ids.len(),
                has_text: g.cue_has_text.clone(),
            })
            .collect();
        let entries = arrangement
            .group_ids
            .iter()
            .map(|id| {
                song.groups
                    .iter()
                    .position(|g| g.uuid.eq_ignore_ascii_case(id))
                    .ok_or(MapError::UnknownGroup)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(groups, entries)
    }

    pub fn groups(&self) -> &[FileGroup] {
        &self.groups
    }

    /// The group (position in `groups`) of each arrangement entry.
    pub fn arrangement(&self) -> &[usize] {
        &self.arrangement
    }

    /// Cues in the whole file (what Playback's highest slide number plus one must equal).
    pub fn cue_total(&self) -> usize {
        self.groups.iter().map(|g| g.cues).sum()
    }

    /// Cues the arrangement plays in all, repeats counted (the flat slide numbers run `0..flat_total()`).
    pub fn flat_total(&self) -> usize {
        self.arrangement.iter().map(|g| self.groups[*g].cues).sum()
    }

    /// The file group a raw cue index belongs to.
    pub fn group_of_raw(&self, raw: usize) -> Option<usize> {
        if raw >= self.cue_total() {
            return None;
        }
        // The last group that starts at or before `raw` and has cues (empty groups share a start with the next).
        (0..self.groups.len())
            .rev()
            .find(|g| self.groups[*g].cues > 0 && self.group_start[*g] <= raw)
    }

    /// Of the arrangement entries that hold `group`, the one nearest section position `k` (the earlier on a tie).
    pub fn entry_for(&self, group: usize, k: usize) -> Option<usize> {
        self.arrangement
            .iter()
            .enumerate()
            .filter(|(_, g)| **g == group)
            .map(|(p, _)| p)
            .min_by_key(|p| (p.abs_diff(k), *p))
    }

    /// ProPresenter's slide number for raw cue `raw` as played in arrangement entry `entry`.
    pub fn flat_index(&self, entry: usize, raw: usize) -> Option<usize> {
        let group = *self.arrangement.get(entry)?;
        let inside = raw.checked_sub(self.group_start[group])?;
        (inside < self.groups[group].cues).then(|| self.entry_base[entry] + inside)
    }

    /// A stable fingerprint of what the numbers depend on: every file group (its uuid and cue count, in file order) and the
    /// order of the groups in the arrangement. A changed arrangement or a slide added or removed changes it.
    pub fn arrangement_hash(&self) -> String {
        let mut text = String::from("stagepilot-follow-arrangement-1\n");
        for group in &self.groups {
            text.push_str(&format!(
                "g {} {}\n",
                group.uuid.to_ascii_lowercase(),
                group.cues
            ));
        }
        let order: Vec<String> = self.arrangement.iter().map(usize::to_string).collect();
        text.push_str(&format!("a {}\n", order.join(",")));
        hex::encode(Sha256::digest(text.as_bytes()))
    }
}

/// Where a schedule entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// A Playback lyric cue.
    Cue,
    /// Spread over the section by bars (no usable cues).
    Bars,
}

/// One slide to fire: at `time_s` (Playback's played clock) trigger slide `flat_index` of the `Playback` arrangement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub time_s: f64,
    /// ProPresenter's slide number in the selected (`Playback`) arrangement.
    pub flat_index: usize,
    /// The raw cue index in the song file (Playback's slide number).
    pub raw: usize,
    /// The arrangement entry, which is also the Playback section, this slide belongs to.
    pub section_index: usize,
    pub kind: StepKind,
}

/// Which timing a song is followed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Cues,
    Bars,
}

/// Why Playback's cues are not used for a song (it falls back to bars).
#[derive(Debug, Clone, PartialEq)]
pub enum CueProblem {
    /// The library has no lyric cues for the song.
    NoCues,
    /// Fewer than `MIN_CUES`.
    TooFew { cues: usize },
    /// The highest slide number is not the file's cue total minus one: the Playback template does not fit the file.
    Mismatch { max_slide: u32, file_cues: usize },
    /// A time that is not a number.
    BadTime,
    /// A cue is later than the end of the song.
    AfterEnd { time_s: f64, slide: u32 },
    /// A cue falls in no section.
    NoSection { time_s: f64, slide: u32 },
}

impl CueProblem {
    /// A short plain phrase for the status line.
    pub fn words(&self) -> String {
        match self {
            Self::NoCues => "no cues".to_string(),
            Self::TooFew { cues } => format!("only {cues} cues (at least {MIN_CUES} are needed)"),
            Self::Mismatch {
                max_slide,
                file_cues,
            } => format!(
                "template does not match the file (cues go to slide {max_slide}, the file has {file_cues})"
            ),
            Self::BadTime => "a cue has no usable time".to_string(),
            Self::AfterEnd { .. } => "cue after the song end".to_string(),
            Self::NoSection { .. } => "a cue is in no section".to_string(),
        }
    }
}

/// Why a song cannot be followed at all, by cues or by bars.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unfollowable {
    /// The song file cannot be mapped (see `MapError`): not the arrangement StagePilot wrote.
    File(MapError),
    /// Playback has no usable section.
    NoSections,
    /// The `Playback` arrangement does not have one entry per Playback section, so it is not the copy of this arrangement.
    ArrangementDiffers { entries: usize, sections: usize },
}

/// What is known about one song: how it is followed and the slides to fire.
#[derive(Debug, Clone, PartialEq)]
pub struct Schedule {
    pub source: Source,
    /// Set when `source` is `Bars`: the test the cues failed.
    pub cue_problem: Option<CueProblem>,
    /// Ordered by `time_s`.
    pub steps: Vec<Step>,
    pub arrangement_hash: String,
    /// The highest slide number among the cues given, whether or not they were used (for the plan's summary line).
    pub cue_max_slide: Option<u32>,
}

/// The section position `k` for a time: the latest section with `start - 2 s <= time_s` that the time is also before the end of.
pub fn section_at(sections: &[Section], time_s: f64) -> Result<usize, CueProblemAt> {
    let k = sections
        .iter()
        .rposition(|s| s.start - SECTION_LEAD_S <= time_s);
    match k {
        Some(k) if time_s < sections[k].end => Ok(k),
        Some(_) => {
            if sections.last().is_some_and(|s| time_s >= s.end) {
                Err(CueProblemAt::AfterEnd)
            } else {
                Err(CueProblemAt::NoSection)
            }
        }
        None => Err(CueProblemAt::NoSection),
    }
}

/// Why `section_at` found no section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueProblemAt {
    AfterEnd,
    NoSection,
}

/// Checks Playback's cues against the file and the sections (rules 3 and 4) and turns them into steps.
pub fn cue_steps(
    map: &SongMap,
    sections: &[Section],
    cues: &[LyricCue],
) -> Result<Vec<Step>, CueProblem> {
    if cues.is_empty() {
        return Err(CueProblem::NoCues);
    }
    if cues.iter().any(|c| !c.time_s.is_finite()) {
        return Err(CueProblem::BadTime);
    }
    let mut sorted: Vec<LyricCue> = cues.to_vec();
    sorted.sort_by(|a, b| a.time_s.total_cmp(&b.time_s).then(a.slide.cmp(&b.slide)));
    sorted.dedup_by(|a, b| a.time_s == b.time_s && a.slide == b.slide);
    if sorted.len() < MIN_CUES {
        return Err(CueProblem::TooFew { cues: sorted.len() });
    }
    let total = map.cue_total();
    let max_slide = sorted.iter().map(|c| c.slide).max().unwrap_or(0);
    if total == 0 || max_slide as usize + 1 != total {
        return Err(CueProblem::Mismatch {
            max_slide,
            file_cues: total,
        });
    }
    let mut steps = Vec::with_capacity(sorted.len());
    for cue in &sorted {
        let raw = cue.slide as usize;
        let at = |problem: CueProblemAt| match problem {
            CueProblemAt::AfterEnd => CueProblem::AfterEnd {
                time_s: cue.time_s,
                slide: cue.slide,
            },
            CueProblemAt::NoSection => CueProblem::NoSection {
                time_s: cue.time_s,
                slide: cue.slide,
            },
        };
        let k = section_at(sections, cue.time_s).map_err(at)?;
        let unresolved = || CueProblem::NoSection {
            time_s: cue.time_s,
            slide: cue.slide,
        };
        let group = map.group_of_raw(raw).ok_or_else(unresolved)?;
        let entry = map.entry_for(group, k).ok_or_else(unresolved)?;
        let flat_index = map.flat_index(entry, raw).ok_or_else(unresolved)?;
        steps.push(Step {
            time_s: cue.time_s,
            flat_index,
            raw,
            section_index: entry,
            kind: StepKind::Cue,
        });
    }
    Ok(steps)
}

/// Even slots over one section, then snapped to bar lines when they can be (see `bars_steps`).
fn slot_times(start: f64, end: f64, slots: usize, bars: Option<&[f64]>) -> Vec<f64> {
    let even: Vec<f64> = (0..slots)
        .map(|i| start + (end - start) * i as f64 / slots as f64)
        .collect();
    let Some(bars) = bars else {
        return even;
    };
    // The first slot stays on the section start. The others move to the nearest bar line inside the section, as long as the
    // result still has every slot later than the one before; otherwise the section keeps its even spread.
    let mut snapped = even.clone();
    let mut last = start;
    for (i, time) in even.iter().enumerate().skip(1) {
        let nearest = bars
            .iter()
            .copied()
            .filter(|b| b.is_finite() && *b > start && *b < end)
            .min_by(|a, b| (a - time).abs().total_cmp(&(b - time).abs()));
        match nearest {
            Some(bar) if bar > last => {
                snapped[i] = bar;
                last = bar;
            }
            _ => return even,
        }
    }
    snapped
}

/// The fallback schedule (rule 2): by section, from the arrangement and the sections. `bars` are the song's bar lines on
/// Playback's played clock, when the library holds the beat grid.
///
/// Per arrangement entry `p` with section `p`: the group's first slide fires at the section start (a blank first slide
/// clears the screen); the text slides are spread evenly over the section, the first of them on the start. A slide with no
/// text is never a slot of its own: it fires with the next text slide (which comes after it and so is the one that stays),
/// or, with no text slide after it, at the end of the section. A group with no text at all fires all its slides at the start.
pub fn bars_steps(map: &SongMap, sections: &[Section], bars: Option<&[f64]>) -> Vec<Step> {
    let mut steps = Vec::new();
    for (entry, section) in sections.iter().enumerate() {
        let Some(group) = map.arrangement.get(entry).map(|g| &map.groups[*g]) else {
            break;
        };
        let raw_start = map.group_start[map.arrangement[entry]];
        let base = map.entry_base[entry];
        let text: Vec<usize> = (0..group.cues).filter(|i| group.has_text[*i]).collect();
        let slots = if text.is_empty() {
            Vec::new()
        } else {
            slot_times(section.start, section.end, text.len(), bars)
        };
        for i in 0..group.cues {
            let time_s = if text.is_empty() {
                section.start
            } else if let Some(slot) = text.iter().position(|t| *t == i) {
                slots[slot]
            } else if let Some(next) = text.iter().position(|t| *t > i) {
                slots[next]
            } else {
                section.end
            };
            steps.push(Step {
                time_s,
                flat_index: base + i,
                raw: raw_start + i,
                section_index: entry,
                kind: StepKind::Bars,
            });
        }
    }
    steps.sort_by(|a, b| {
        a.time_s
            .total_cmp(&b.time_s)
            .then(a.flat_index.cmp(&b.flat_index))
    });
    steps
}

/// The follow schedule and the readiness verdict for one song.
///
/// `cues` are Playback's lyric cues (empty when the library has none). With usable cues the schedule is the cues; with none,
/// or when one of the tests of rule 4 fails, it is the bars schedule and `cue_problem` says which test failed. `Err` when
/// even the bars cannot be built: the file is not the `Playback` copy, or it does not fit Playback's sections.
pub fn build_schedule(
    map: &SongMap,
    sections: &[Section],
    cues: &[LyricCue],
    bars: Option<&[f64]>,
) -> Result<Schedule, Unfollowable> {
    if sections.is_empty() {
        return Err(Unfollowable::NoSections);
    }
    if map.arrangement.len() != sections.len() {
        return Err(Unfollowable::ArrangementDiffers {
            entries: map.arrangement.len(),
            sections: sections.len(),
        });
    }
    let cue_max_slide = cues.iter().map(|c| c.slide).max();
    let arrangement_hash = map.arrangement_hash();
    match cue_steps(map, sections, cues) {
        Ok(steps) => Ok(Schedule {
            source: Source::Cues,
            cue_problem: None,
            steps,
            arrangement_hash,
            cue_max_slide,
        }),
        Err(problem) => Ok(Schedule {
            source: Source::Bars,
            cue_problem: Some(problem),
            steps: bars_steps(map, sections, bars),
            arrangement_hash,
            cue_max_slide,
        }),
    }
}

/// `build_schedule` straight from a song file: its `Playback` arrangement must be the selected one.
pub fn schedule_for_song(
    song: &Song,
    sections: &[Section],
    cues: &[LyricCue],
    bars: Option<&[f64]>,
) -> Result<Schedule, Unfollowable> {
    let map = SongMap::from_song(song).map_err(Unfollowable::File)?;
    build_schedule(&map, sections, cues, bars)
}

#[cfg(test)]
mod tests;
