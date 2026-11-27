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
//! * Cues are used only when every cue resolves and there are at least three. When the highest slide number is the file's cue
//!   total minus one (nothing was added to the file) the global raw index is used as it is. When the file has up to
//!   [`MAX_EXTRA_CUES`] more cues than Playback numbers (background or blank slides someone added), the numbers are aligned
//!   group by group instead (see [`align`], design `SPEC.md`, "Revision 2"). Anything else is a `CueProblem`, said in plain
//!   words, and the song falls back to bars.
//! * Bars: the first slide of each section fires at the section start; the other text slides are spread evenly over the
//!   section (snapped to bar lines when the caller holds the beat grid). Background-only slides are never spread over.

pub mod binding;
pub mod engine;
pub mod lines;
pub mod prepare;

pub use lines::LineFit;

use crate::pro_file::Song;
use sha2::{Digest, Sha256};

/// How far before a section's start a cue may come and still be that section's (Playback sends cues ahead of the lyric).
pub const SECTION_LEAD_S: f64 = 2.0;
/// How far before a section's start the FIRST slide of a section may be sent and still be that section's. Playback sends it up to
/// one bar ahead (3.2 to 3.4 s at 71 bpm in the real recording), more than [`SECTION_LEAD_S`]; such a cue is only the next
/// section's when it cannot be the continuation of the section it is in (see [`assign`]).
pub const SECTION_LEAD_MAX_S: f64 = 4.0;
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
    /// Per cue, how many lines of text its slide shows (`cues` entries; 0 for a blank or background-only slide). A cue with
    /// `lines > 0` has text.
    pub lines: Vec<usize>,
}

impl FileGroup {
    /// The lines of text the whole group shows.
    pub fn text_lines(&self) -> usize {
        self.lines.iter().sum()
    }
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
    /// The usual number of lines of text on a slide that has text (which Playback Lyrics Bus template fits the file).
    usual_lines: Option<usize>,
}

/// Version of [`SongMap::arrangement_hash`]. A binding saved with an older one is rebuilt, not reported as changed.
pub const HASH_VERSION: u8 = 2;

impl SongMap {
    /// From groups in file order and an arrangement given as positions in `groups`.
    pub fn new(groups: Vec<FileGroup>, arrangement: Vec<usize>) -> Result<Self, MapError> {
        if groups
            .iter()
            .any(|g| g.has_text.len() != g.cues || g.lines.len() != g.cues)
        {
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
            usual_lines: None,
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
                // A slide with no visible text shows no line, whatever the file's text element holds.
                lines: g
                    .cue_lines
                    .iter()
                    .zip(&g.cue_has_text)
                    .map(|(lines, text)| if *text { (*lines).max(1) } else { 0 })
                    .collect(),
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
        let mut map = Self::new(groups, entries)?;
        map.usual_lines = song.usual_lines();
        Ok(map)
    }

    /// The usual lines of text per slide that has text (1, 2 or 4 are the Lyrics Bus templates Playback has).
    pub fn lines_per_slide(&self) -> Option<usize> {
        self.usual_lines
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

    /// A stable fingerprint of the arrangement StagePilot follows: the ordered ids of the groups the `Playback` arrangement plays
    /// (the map only exists while `Playback` is the selected arrangement). Slides added to or removed from a group, or a group the
    /// arrangement does not play, do not change it (the map is read again from the file every time); another order, another length
    /// or another group do. Version [`HASH_VERSION`].
    pub fn arrangement_hash(&self) -> String {
        let mut text = String::from("stagepilot-follow-arrangement-2\nselected Playback\n");
        for g in &self.arrangement {
            text.push_str(&format!(
                "a {}\n",
                self.groups[*g].uuid.to_ascii_lowercase()
            ));
        }
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
    /// Set when the cues were used with the file's extra slides taken into account (see [`align`]).
    pub alignment: Option<Alignment>,
    /// Playback's lines per slide (1, 2 or 4) worked out from the cues, whether or not they were used (see [`lines::infer`]).
    pub playback_lines: Option<usize>,
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

/// Most cues the file may have beyond the highest slide number Playback sends plus one (blank or background slides someone added
/// to the song). More than that is not a person adding a slide: it is a Lyrics Bus template that does not fit the file (a 2 or
/// 4 Line template against a 1 Line file leaves many slides unnumbered).
pub const MAX_EXTRA_CUES: usize = 8;

/// How the file's extra cues were worked around (see [`align`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alignment {
    /// The cues the file has beyond Playback's numbering: file cue total minus (highest slide + 1).
    pub extras: usize,
    /// Per file group: the cues the file has in front of what Playback counts there (the shift `d` of the design).
    pub shifts: Vec<usize>,
    /// The arrangement entry (the Playback section) whose first slide is worth a look: the first one whose group could not be
    /// placed with certainty (extras inside the group), else the first section that has a cue.
    pub check_section: usize,
    /// Whether the place of the extras inside some group had to be guessed (from which slides have text).
    pub uncertain: bool,
    /// Set when the cues were mapped by lines because Playback's template and the file count lines per slide differently (see
    /// [`lines::by_lines`]); then `extras`, `shifts` and `uncertain` do not apply.
    pub lines: Option<LineFit>,
}

/// The slides to fire for a song followed by Playback's cues.
#[derive(Debug, Clone, PartialEq)]
pub struct CuePlan {
    pub steps: Vec<Step>,
    /// `None` for the exact layout (nothing was added to the file).
    pub alignment: Option<Alignment>,
}

/// Checks Playback's cues against the file and the sections (rules 3 and 4) and turns them into steps.
pub fn cue_steps(
    map: &SongMap,
    sections: &[Section],
    cues: &[LyricCue],
) -> Result<Vec<Step>, CueProblem> {
    cue_plan(map, sections, cues).map(|plan| plan.steps)
}

/// [`cue_steps`] with how the numbers were matched to the file.
///
/// The exact layout (the highest slide number plus one is the file's cue total) is tried first and is unchanged: the slide
/// number is the global raw index. When the file has more cues than that, up to [`MAX_EXTRA_CUES`] more, [`align`] matches the
/// numbers to the file group by group. Anything else is a `Mismatch`.
pub fn cue_plan(
    map: &SongMap,
    sections: &[Section],
    cues: &[LyricCue],
) -> Result<CuePlan, CueProblem> {
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
    let mismatch = CueProblem::Mismatch {
        max_slide,
        file_cues: total,
    };
    // Revision 3: Playback's lines per slide against the file's. When they differ, or the file's slides are not the ones that
    // many lines make (a 3-line slide among 2-line ones), the song is followed by lines.
    let file_lines = map.lines_per_slide();
    let playback_lines = lines::infer(map, sections, &sorted);
    if let Some(playback) = playback_lines {
        let differ = file_lines != Some(playback);
        if differ || !lines::slides_match(map, playback) {
            let by_lines = lines::by_lines(
                map,
                sections,
                &sorted,
                playback,
                file_lines.unwrap_or(playback),
                mismatch.clone(),
            );
            if by_lines.is_ok() || differ {
                return by_lines;
            }
        }
    }
    exact_or_aligned(map, sections, &sorted, mismatch)
}

/// Rules 3 and 4 and Revision 2: the exact layout, else the cues aligned group by group.
fn exact_or_aligned(
    map: &SongMap,
    sections: &[Section],
    sorted: &[LyricCue],
    mismatch: CueProblem,
) -> Result<CuePlan, CueProblem> {
    let total = map.cue_total();
    let max_slide = sorted.iter().map(|c| c.slide).max().unwrap_or(0);
    let numbered = max_slide as usize + 1;
    if total == 0 || numbered > total {
        return Err(mismatch);
    }
    if numbered != total {
        let extras = total - numbered;
        if extras > MAX_EXTRA_CUES {
            return Err(mismatch);
        }
        return align(map, sections, sorted, extras, mismatch);
    }
    let sections_of = if map.arrangement.len() == sections.len() {
        assign(map, sections, sorted, &|g| map.groups[g].cues)?
    } else {
        late_sections(sections, sorted)?
    };
    let mut steps = Vec::with_capacity(sorted.len());
    for (cue, k) in sorted.iter().zip(sections_of) {
        let raw = cue.slide as usize;
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
    Ok(CuePlan {
        steps,
        alignment: None,
    })
}

/// The section position of a cue by its time, or why it has none.
fn section_for(sections: &[Section], cue: &LyricCue) -> Result<usize, CueProblem> {
    section_at(sections, cue.time_s).map_err(|problem| match problem {
        CueProblemAt::AfterEnd => CueProblem::AfterEnd {
            time_s: cue.time_s,
            slide: cue.slide,
        },
        CueProblemAt::NoSection => CueProblem::NoSection {
            time_s: cue.time_s,
            slide: cue.slide,
        },
    })
}

/// The section a cue could belong to, looking up to [`SECTION_LEAD_MAX_S`] ahead: the section `section_for` finds, or the next
/// one when the cue comes within that time of its start and an earlier cue already sits in the section `section_for` finds or
/// later (so this cue is not the first of that section, and may well be the first slide of the next one). A section's own first
/// cue is never moved on, however short the section.
pub(super) fn late_sections(
    sections: &[Section],
    sorted: &[LyricCue],
) -> Result<Vec<usize>, CueProblem> {
    let mut placed: Vec<usize> = Vec::with_capacity(sorted.len());
    for cue in sorted {
        let k = section_for(sections, cue)?;
        let far = sections
            .get(k + 1)
            .is_some_and(|next| next.start - SECTION_LEAD_MAX_S <= cue.time_s);
        placed.push(if far && placed.last().is_some_and(|before| *before >= k) {
            k + 1
        } else {
            k
        });
    }
    Ok(placed)
}

/// The arrangement entry (the section) of each cue. A cue just before the next section starts and still inside the one before
/// goes by continuity: it continues the earlier section when its slide is the previous slide plus one and that section's group
/// (`room` cues of it, in the numbering the cues use) still has room, else it starts the next. That is also how the first slide
/// of a repeated section reaches its own repeat when it arrives a bar early.
pub(super) fn assign(
    map: &SongMap,
    sections: &[Section],
    sorted: &[LyricCue],
    room: &dyn Fn(usize) -> usize,
) -> Result<Vec<usize>, CueProblem> {
    let late = late_sections(sections, sorted)?;
    let mut entry: Vec<usize> = Vec::with_capacity(sorted.len());
    let mut seen: Vec<Option<(u32, u32)>> = vec![None; sections.len()];
    for (i, cue) in sorted.iter().enumerate() {
        let late = late[i];
        let early = late
            .checked_sub(1)
            .filter(|before| cue.time_s < sections[*before].end);
        let pick = match early {
            Some(before)
                if i > 0
                    && entry[i - 1] == before
                    && sorted[i - 1].slide.checked_add(1) == Some(cue.slide)
                    && seen[before].is_some_and(|(low, high)| {
                        ((high.max(cue.slide) - low.min(cue.slide)) as usize)
                            < room(map.arrangement[before])
                    }) =>
            {
                before
            }
            _ => late,
        };
        seen[pick] = Some(match seen[pick] {
            Some((low, high)) => (low.min(cue.slide), high.max(cue.slide)),
            None => (cue.slide, cue.slide),
        });
        entry.push(pick);
    }
    Ok(entry)
}

// ---- extra slides in the file (SPEC "Revision 2") -----------------------------------------------------------------------

/// How many ways of splitting the cues near section boundaries `align` tries at most.
const MAX_SEARCH_NODES: usize = 20_000;

/// A run of cues that could be the end of the section before a boundary or the start of the section after it.
struct Boundary {
    early: usize,
    late: usize,
    /// Indexes of the cues (time order, consecutive).
    cues: Vec<usize>,
}

/// One way of putting every cue in a section, worked out.
struct Placed {
    /// Distinct slides that land on a file cue with text.
    text_hits: usize,
    /// Each distinct slide (ascending) and the cues the file has in front of it that Playback does not count.
    shift: Vec<(u32, usize)>,
    /// Per file group, whether the file holds cues in it that no slide number lands on (extras inside it: where they sit was
    /// guessed from which cues have text).
    uncertain: Vec<bool>,
}

struct Search<'a> {
    map: &'a SongMap,
    cues: &'a [LyricCue],
    extras: usize,
    boundaries: Vec<Boundary>,
    /// The section of each cue (the ones near a boundary are filled in as the search goes).
    section: Vec<usize>,
    nodes: usize,
    /// (cues on text, boundaries that went against the continuity rule, the sections, how it was placed)
    best: Option<(usize, usize, Vec<usize>, Placed)>,
}

/// The cues of one song when the file has extra slides: Playback numbers the ORIGINAL layout, in file-group order, with no
/// extras; the file has `extras` more cues. Group `g` starts at file cue `F_g` and holds `c_g` cues. A slide `s` is file cue
/// `s + d(s)`, where `d(s)` is the number of extra cues in front of it, so `d` never decreases from one slide to the next (extras
/// only add cues; for the first slide of a group this is the design's `d_g = F_g - s_g`).
///
/// 1. Each cue goes to a section by time. Where two sections qualify (a cue just before the next section starts), the cue
///    continues the earlier section when its slide is the previous slide plus one and that section's group still has room,
///    else it starts the next one. Because extras inside a group make its room look bigger than it was, the other way of
///    splitting each boundary is tried too, and the way that puts the most cues on slides with text wins; the continuity rule
///    decides when that is a tie.
/// 2. For each slide the feasible shifts are those with `F_g <= s + d` and `s + d < F_g + c_g` (`g` the group of its section) and
///    `0 <= d <= extras`; the shifts never decrease along the slides. When several fit (the extras sit inside the group) the
///    placement that puts most slides on cues with text wins (blank slides are the likely extras), then the smallest shifts.
/// 3. A group with no cue (never sampled, like the Count Off) takes the previous shift; a group before the first one that has
///    cues takes that one's shift when the groups in between hold only blank slides, else 0.
/// 4. The flat slide number is `entry_base(k) + (s + d - F_g)`; `raw` is the file's cue index `s + d`.
///
/// `Err(mismatch)` when no way of placing the cues fits the file; a cue in no section is that cue's own problem. The song is
/// then not followed by cues.
pub fn align(
    map: &SongMap,
    sections: &[Section],
    sorted: &[LyricCue],
    extras: usize,
    mismatch: CueProblem,
) -> Result<CuePlan, CueProblem> {
    // Which sections each cue could be in: the one `section_at` finds and the one before it when the cue is also before that
    // one's end.
    let late = late_sections(sections, sorted)?;
    let early: Vec<Option<usize>> = sorted
        .iter()
        .zip(&late)
        .map(|(cue, k)| {
            k.checked_sub(1)
                .filter(|before| cue.time_s < sections[*before].end)
        })
        .collect();
    let mut boundaries: Vec<Boundary> = Vec::new();
    for i in 0..sorted.len() {
        let Some(before) = early[i] else { continue };
        match boundaries.last_mut() {
            Some(b) if b.late == late[i] && b.cues.last() == Some(&(i - 1)) => b.cues.push(i),
            _ => boundaries.push(Boundary {
                early: before,
                late: late[i],
                cues: vec![i],
            }),
        }
    }
    let mut search = Search {
        map,
        cues: sorted,
        extras,
        boundaries,
        section: late,
        nodes: 0,
        best: None,
    };
    search.go(0, 0);
    match search.best.take() {
        Some((_, _, section, placed)) => Ok(search.plan(&section, &placed)),
        None => Err(mismatch),
    }
}

impl Search<'_> {
    /// The cues that continue the earlier section at boundary `b`, by the continuity rule, given the cues placed before it.
    fn preferred_split(&self, b: &Boundary) -> usize {
        let first = b.cues[0];
        let cap = self.map.groups[self.map.arrangement[b.early]].cues;
        let (mut low, mut high) = (u32::MAX, 0u32);
        for i in 0..first {
            if self.section[i] == b.early {
                low = low.min(self.cues[i].slide);
                high = high.max(self.cues[i].slide);
            }
        }
        let mut previous =
            (first > 0).then(|| (self.section[first - 1], self.cues[first - 1].slide));
        let mut split = 0;
        for i in &b.cues {
            let slide = self.cues[*i].slide;
            let continues = previous.is_some_and(|(section, last)| {
                section == b.early && last.checked_add(1) == Some(slide)
            }) && ((high.max(slide) - low.min(slide)) as usize) < cap;
            if !continues {
                break;
            }
            low = low.min(slide);
            high = high.max(slide);
            previous = Some((b.early, slide));
            split += 1;
        }
        split
    }

    /// Whether the cues placed so far in `section` span more slides than its group has cues.
    fn overfull(&self, section: usize, upto: usize) -> bool {
        let (mut low, mut high) = (u32::MAX, 0u32);
        for i in 0..=upto {
            if self.section[i] == section {
                low = low.min(self.cues[i].slide);
                high = high.max(self.cues[i].slide);
            }
        }
        low != u32::MAX
            && (high - low) as usize >= self.map.groups[self.map.arrangement[section]].cues
    }

    /// Tries every split of boundary `j` (the continuity rule's first), then the next boundary. `against`: boundaries so far
    /// split otherwise than the rule says.
    fn go(&mut self, j: usize, against: usize) {
        if self.nodes >= MAX_SEARCH_NODES {
            return;
        }
        self.nodes += 1;
        if j == self.boundaries.len() {
            if let Some(placed) = self.place() {
                let better = match &self.best {
                    None => true,
                    Some((hits, other, _, _)) => {
                        (placed.text_hits, std::cmp::Reverse(against))
                            > (*hits, std::cmp::Reverse(*other))
                    }
                };
                if better {
                    self.best = Some((placed.text_hits, against, self.section.clone(), placed));
                }
            }
            return;
        }
        let (early, late, cues) = {
            let b = &self.boundaries[j];
            (b.early, b.late, b.cues.clone())
        };
        let preferred = self.preferred_split(&self.boundaries[j]);
        let mut order: Vec<usize> = (0..=cues.len()).collect();
        order.sort_by_key(|split| (split.abs_diff(preferred), *split));
        for split in order {
            for (n, i) in cues.iter().enumerate() {
                self.section[*i] = if n < split { early } else { late };
            }
            let last = *cues.last().unwrap_or(&0);
            if self.overfull(early, last) || self.overfull(late, last) {
                continue;
            }
            self.go(j + 1, against + usize::from(split != preferred));
        }
    }

    /// The shifts for the sections as they are now, or `None` when the slides cannot be placed in the file.
    fn place(&self) -> Option<Placed> {
        let map = self.map;
        // Each distinct slide and the file group of the sections that play it.
        let mut slides: Vec<(u32, usize)> = Vec::new();
        for (i, cue) in self.cues.iter().enumerate() {
            let g = map.arrangement[self.section[i]];
            match slides.iter().find(|(s, _)| *s == cue.slide) {
                Some((_, other)) if *other != g => return None,
                Some(_) => {}
                None => slides.push((cue.slide, g)),
            }
        }
        slides.sort_unstable();
        // Per slide: the shifts that put it inside its group.
        let top = self.extras as i64;
        let mut room: Vec<(i64, i64)> = Vec::with_capacity(slides.len());
        for (slide, g) in &slides {
            let start = map.group_start[*g] as i64;
            let lo = (start - i64::from(*slide)).max(0);
            let hi = (start + map.groups[*g].cues as i64 - 1 - i64::from(*slide)).min(top);
            if lo > hi {
                return None;
            }
            room.push((lo, hi));
        }
        let on_text = |slide: u32, g: usize, d: i64| {
            let at = i64::from(slide) + d - map.group_start[g] as i64;
            usize::try_from(at)
                .ok()
                .and_then(|at| map.groups[g].has_text.get(at))
                .copied()
                .unwrap_or(false)
        };
        // Shifts never decrease along the slides. The best placement has the most slides on text, then the smallest shifts:
        // `best[i][d]` is the best value of slides 0..=i with slide i at shift d, and `from[i][d]` the shift of slide i - 1.
        type Value = (usize, std::cmp::Reverse<usize>);
        let width = self.extras + 1;
        let mut best: Vec<Vec<Option<Value>>> = vec![vec![None; width]; slides.len()];
        let mut from: Vec<Vec<usize>> = vec![vec![0; width]; slides.len()];
        for (i, (slide, g)) in slides.iter().enumerate() {
            for d in room[i].0..=room[i].1 {
                let d = d as usize;
                let own = (usize::from(on_text(*slide, *g, d as i64)), d);
                let before = if i == 0 {
                    Some((0, 0, 0))
                } else {
                    (0..=d)
                        .filter_map(|e| {
                            best[i - 1][e].map(|(hits, std::cmp::Reverse(sum))| (hits, sum, e))
                        })
                        .max_by_key(|(hits, sum, e)| {
                            (*hits, std::cmp::Reverse(*sum), std::cmp::Reverse(*e))
                        })
                };
                if let Some((hits, sum, e)) = before {
                    best[i][d] = Some((hits + own.0, std::cmp::Reverse(sum + own.1)));
                    from[i][d] = e;
                }
            }
        }
        let last = slides.len().checked_sub(1)?;
        let mut d = (0..width)
            .filter_map(|d| best[last][d].map(|value| (value, d)))
            .max_by_key(|(value, d)| (*value, std::cmp::Reverse(*d)))?
            .1;
        let text_hits = best[last][d].map_or(0, |(hits, _)| hits);
        let mut shift = vec![(0u32, 0usize); slides.len()];
        for i in (0..slides.len()).rev() {
            shift[i] = (slides[i].0, d);
            d = from[i][d];
        }
        // A group is uncertain when the file has cues in it that no slide number lands on.
        let mut landed = vec![0usize; map.groups.len()];
        for (slide, g) in &slides {
            let _ = slide;
            landed[*g] += 1;
        }
        let uncertain = (0..map.groups.len())
            .map(|g| landed[g] > 0 && landed[g] < map.groups[g].cues)
            .collect();
        Some(Placed {
            text_hits,
            shift,
            uncertain,
        })
    }

    /// The steps and the alignment for the winning placement.
    fn plan(&self, section: &[usize], placed: &Placed) -> CuePlan {
        let map = self.map;
        let mut steps = Vec::with_capacity(self.cues.len());
        let mut check = None;
        let mut first_shift: Vec<Option<usize>> = vec![None; map.groups.len()];
        let mut smallest: Vec<Option<u32>> = vec![None; map.groups.len()];
        for (i, cue) in self.cues.iter().enumerate() {
            let k = section[i];
            let g = map.arrangement[k];
            let d = placed
                .shift
                .iter()
                .find(|(slide, _)| *slide == cue.slide)
                .map_or(0, |(_, d)| *d);
            let raw = cue.slide as usize + d;
            steps.push(Step {
                time_s: cue.time_s,
                flat_index: map.entry_base[k] + (raw - map.group_start[g]),
                raw,
                section_index: k,
                kind: StepKind::Cue,
            });
            if placed.uncertain[g] {
                check = check.map_or(Some(k), |c: usize| Some(c.min(k)));
            }
            // The shift of the first slide of the group (the smallest slide seen in it).
            if smallest[g].is_none_or(|first| cue.slide < first) {
                smallest[g] = Some(cue.slide);
                first_shift[g] = Some(d);
            }
        }
        let check_section = check
            .or_else(|| steps.first().map(|s| s.section_index))
            .unwrap_or(0);
        // Groups with no cue: the previous shift; before the first group with cues, that one's shift while only blank slides lie
        // between (else 0).
        let mut shifts = vec![0usize; map.groups.len()];
        let mut last = None;
        for g in 0..map.groups.len() {
            if let Some(d) = first_shift[g] {
                last = Some(d);
            }
            shifts[g] = last.unwrap_or(0);
        }
        if let Some(first) = first_shift.iter().position(Option::is_some) {
            let d = first_shift[first].unwrap_or(0);
            for g in (0..first).rev() {
                if map.groups[g].has_text.iter().any(|t| *t) {
                    break;
                }
                shifts[g] = d;
            }
        }
        CuePlan {
            steps,
            alignment: Some(Alignment {
                extras: self.extras,
                shifts,
                check_section,
                uncertain: placed.uncertain.iter().any(|u| *u),
                lines: None,
            }),
        }
    }
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
    let playback_lines = infer_playback_lines(map, sections, cues);
    let arrangement_hash = map.arrangement_hash();
    match cue_plan(map, sections, cues) {
        Ok(plan) => Ok(Schedule {
            source: Source::Cues,
            cue_problem: None,
            steps: plan.steps,
            arrangement_hash,
            alignment: plan.alignment,
            playback_lines,
            cue_max_slide,
        }),
        Err(problem) => Ok(Schedule {
            source: Source::Bars,
            cue_problem: Some(problem),
            steps: bars_steps(map, sections, bars),
            arrangement_hash,
            alignment: None,
            playback_lines,
            cue_max_slide,
        }),
    }
}

/// Playback's lines per slide (1, 2 or 4), worked out from the cues as they come (see [`lines::infer`]); `None` when no template
/// stands out, or the cues cannot be placed in the sections.
pub fn infer_playback_lines(
    map: &SongMap,
    sections: &[Section],
    cues: &[LyricCue],
) -> Option<usize> {
    if cues.iter().any(|c| !c.time_s.is_finite()) {
        return None;
    }
    let mut sorted: Vec<LyricCue> = cues.to_vec();
    sorted.sort_by(|a, b| a.time_s.total_cmp(&b.time_s).then(a.slide.cmp(&b.slide)));
    sorted.dedup_by(|a, b| a.time_s == b.time_s && a.slide == b.slide);
    lines::infer(map, sections, &sorted)
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
mod align_tests;
#[cfg(test)]
mod lines_tests;
#[cfg(test)]
mod rig_tests;
#[cfg(test)]
mod tests;
