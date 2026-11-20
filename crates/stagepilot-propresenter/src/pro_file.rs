//! ProPresenter song files (`.pro`, protobuf): reading what StagePilot needs and making a copy with one more arrangement.
//!
//! This is the Rust port of the reference scripts that were proven on four real songs (`build_copy.py`, `build_test_pro.py`).
//! There is no protobuf library and no schema file: the file is read and written on the wire level.
//!
//! * **Reading** ([`Song::read`]): the name, the song's uuid, the groups (name, uuid, the cues in it), the arrangements and the
//!   selected one, the key (`music`) and the MultiTracks number.
//! * **Mapping** ([`resolve`]): the section names Playback plays are matched to the song's groups; a name that fits several groups gets one of them as a named guess, a name no group has is missing.
//! * **Copying** ([`build_copy`]): the bytes of a new song file that has ALL the original arrangements plus one more (named
//!   `Playback` by the caller), selected, the key written in, a new presentation uuid, every inner uuid replaced consistently
//!   and fresh dates. Only the fields that change are rewritten: every other field, known or not, is copied as the bytes it
//!   was, so a copy differs from the song only where it has to. The same input, uuid source and clock give the same bytes.
//!
//! Field numbers (ProPresenter 19 `presentation.proto`): `Presentation` 2 uuid, 3 name, 4 last_date_used,
//! 5 last_modified_date, 10 selected_arrangement, 11 arrangements, 12 cue_groups, 13 cues, 21 multi_tracks_licensing, 23 music.

use std::collections::HashMap;

// ---- the wire format ------------------------------------------------------------------------------------------------

/// A file that is not a ProPresenter song, or one that cannot be copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProError {
    /// Not protobuf, or cut short.
    NotASong,
    /// The sections could not all be mapped (the copy is never made); the list is the whole mapping.
    Unmapped(Vec<Mapped>),
    /// The copy did not read back as intended (a bug or an odd file); nothing is written.
    CopyCheckFailed(String),
}

impl ProError {
    pub fn words(&self) -> String {
        match self {
            Self::NotASong => "That file is not a ProPresenter song StagePilot can read.".into(),
            Self::Unmapped(map) => format!(
                "These sections have no single matching group in the song: {}.",
                unmapped_names(map).join(", ")
            ),
            Self::CopyCheckFailed(why) => {
                format!("The copy did not check out ({why}). Nothing was written.")
            }
        }
    }
}

impl std::fmt::Display for ProError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.words())
    }
}

impl std::error::Error for ProError {}

/// One record of a message: where it sits in the bytes.
#[derive(Debug, Clone, Copy)]
struct Rec {
    number: u32,
    wire: u8,
    /// The whole record, tag included: `data[start..end]`.
    start: usize,
    end: usize,
    /// The value: for a length-delimited record the payload, else the bytes after the tag.
    body: usize,
}

const MAX_DEPTH: usize = 40;

fn varint_at(data: &[u8], at: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for index in 0..10usize {
        let byte = *data.get(at + index)?;
        let part = u64::from(byte & 0x7f);
        if index == 9 && part > 1 {
            return None;
        }
        value |= part << (7 * index);
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
    }
    None
}

/// The records of one message. `None` when the bytes are not a whole, valid message (wire types 3 and 4, field 0, a length that
/// runs past the end, a truncated value).
fn records(data: &[u8]) -> Option<Vec<Rec>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < data.len() {
        let start = at;
        let (tag, used) = varint_at(data, at)?;
        at += used;
        let number = u32::try_from(tag >> 3)
            .ok()
            .filter(|n| *n > 0 && *n < (1 << 29))?;
        let wire = (tag & 7) as u8;
        let body = at;
        match wire {
            0 => at += varint_at(data, at)?.1,
            1 => at = at.checked_add(8).filter(|e| *e <= data.len())?,
            5 => at = at.checked_add(4).filter(|e| *e <= data.len())?,
            2 => {
                let (len, used) = varint_at(data, at)?;
                at += used;
                let len = usize::try_from(len).ok()?;
                let end = at.checked_add(len).filter(|e| *e <= data.len())?;
                out.push(Rec {
                    number,
                    wire,
                    start,
                    end,
                    body: at,
                });
                at = end;
                continue;
            }
            _ => return None,
        }
        out.push(Rec {
            number,
            wire,
            start,
            end: at,
            body,
        });
    }
    Some(out)
}

fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// A length-delimited record (tag, length, payload).
fn len_record(number: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 6);
    push_varint(&mut out, u64::from(number) << 3 | 2);
    push_varint(&mut out, payload.len() as u64);
    out.extend_from_slice(payload);
    out
}

fn varint_record(number: u32, value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    push_varint(&mut out, u64::from(number) << 3);
    push_varint(&mut out, value);
    out
}

fn payload<'a>(data: &'a [u8], rec: &Rec) -> &'a [u8] {
    &data[rec.body..rec.end]
}

fn first<'a>(data: &'a [u8], recs: &[Rec], number: u32) -> Option<&'a [u8]> {
    recs.iter()
        .find(|r| r.number == number && r.wire == 2)
        .map(|r| payload(data, r))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `rv.data.UUID { string string = 1 }`: its text.
fn uuid_text(message: &[u8]) -> Option<String> {
    let recs = records(message)?;
    first(message, &recs, 1).map(text)
}

fn uuid_message(value: &str) -> Vec<u8> {
    len_record(1, value.as_bytes())
}

/// The text of a canonical UUID (`8-4-4-4-12` hex digits) when `message` is exactly a `UUID` message and nothing else.
fn exact_uuid(message: &[u8]) -> Option<&str> {
    if message.len() != 38 || message[0] != 0x0a || message[1] != 36 {
        return None;
    }
    let body = std::str::from_utf8(&message[2..]).ok()?;
    let ok = body.bytes().enumerate().all(|(i, b)| {
        if matches!(i, 8 | 13 | 18 | 23) {
            b == b'-'
        } else {
            b.is_ascii_hexdigit()
        }
    });
    ok.then_some(body)
}

const NIL_UUID: &str = "00000000-0000-0000-0000-000000000000";

/// Every `UUID` message inside `message`, at any depth, replaced by a new one (the same old uuid always becomes the same new
/// one, whatever its letter case). Returns the new bytes only when something changed; a payload that is not a message (text,
/// pictures) is left exactly as it is.
fn remap_uuids(
    message: &[u8],
    map: &mut HashMap<String, String>,
    new_uuid: &mut dyn FnMut() -> String,
    depth: usize,
) -> Option<Vec<u8>> {
    if depth > MAX_DEPTH || message.len() < 2 {
        return None;
    }
    let recs = records(message)?;
    let mut out: Vec<u8> = Vec::new();
    let mut changed = false;
    let mut copied_to = 0usize;
    for rec in recs.iter().filter(|r| r.wire == 2) {
        let inner = payload(message, rec);
        let replaced = if let Some(old) = exact_uuid(inner) {
            if old == NIL_UUID {
                None
            } else {
                let new = map
                    .entry(old.to_ascii_lowercase())
                    .or_insert_with(&mut *new_uuid)
                    .clone();
                Some(uuid_message(&new))
            }
        } else {
            remap_uuids(inner, map, new_uuid, depth + 1)
        };
        if let Some(bytes) = replaced {
            out.extend_from_slice(&message[copied_to..rec.start]);
            let mut tag = Vec::new();
            push_varint(&mut tag, u64::from(rec.number) << 3 | 2);
            out.extend_from_slice(&tag);
            push_varint(&mut out, bytes.len() as u64);
            out.extend_from_slice(&bytes);
            copied_to = rec.end;
            changed = true;
        }
    }
    if !changed {
        return None;
    }
    out.extend_from_slice(&message[copied_to..]);
    Some(out)
}

// ---- reading ----------------------------------------------------------------------------------------------------------

/// A key and its scale as ProPresenter stores them (`MusicKeyScale`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyScale {
    /// `MusicKeyScale.MusicKey`: 0 A flat, 1 A, 2 A sharp, 3 B flat ... 20 G sharp.
    pub key: u32,
    pub minor: bool,
}

impl KeyScale {
    fn to_message(self) -> Vec<u8> {
        // proto3: a zero is not written.
        let mut out = Vec::new();
        if self.key != 0 {
            out.extend(varint_record(1, u64::from(self.key)));
        }
        if self.minor {
            out.extend(varint_record(2, 1));
        }
        out
    }

    fn from_message(message: &[u8]) -> Option<Self> {
        let recs = records(message)?;
        let number = |n: u32| {
            recs.iter()
                .find(|r| r.number == n && r.wire == 0)
                .and_then(|r| varint_at(message, r.body))
                .map_or(0, |(v, _)| v)
        };
        Some(Self {
            key: u32::try_from(number(1)).ok().filter(|k| *k <= 20)?,
            minor: number(2) == 1,
        })
    }

    /// The key as Playback writes it: `Eb`, `F#`, `Am`.
    pub fn name(self) -> String {
        const NAMES: [&str; 21] = [
            "Ab", "A", "A#", "Bb", "B", "B#", "Cb", "C", "C#", "Db", "D", "D#", "Eb", "E", "E#",
            "Fb", "F", "F#", "Gb", "G", "G#",
        ];
        format!(
            "{}{}",
            NAMES.get(self.key as usize).copied().unwrap_or("?"),
            if self.minor { "m" } else { "" }
        )
    }
}

/// Playback's key text (`Eb`, `F#`, `Bb`, `Am`, `C♯ minor`) as ProPresenter's key. `None` for anything else.
pub fn parse_key(text: &str) -> Option<KeyScale> {
    let t = text.trim();
    let mut chars = t.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let base: u32 = match letter {
        'A' => 1,
        'B' => 4,
        'C' => 7,
        'D' => 10,
        'E' => 13,
        'F' => 16,
        'G' => 19,
        _ => return None,
    };
    let rest: String = chars.collect();
    let (shift, rest): (i32, &str) = if let Some(r) = rest.strip_prefix(['#', '♯']) {
        (1, r)
    } else if let Some(r) = rest.strip_prefix('♭') {
        (-1, r)
    } else if let Some(r) = rest.strip_prefix('b') {
        // `Bb` is B flat; a lone `b` after the letter is never the start of "minor"/"major".
        (-1, r)
    } else {
        (0, rest.as_str())
    };
    let scale = rest.trim().to_ascii_lowercase();
    let minor = match scale.as_str() {
        "" | "maj" | "major" => false,
        "m" | "min" | "minor" | "-" => true,
        _ => return None,
    };
    Some(KeyScale {
        key: u32::try_from(i64::from(base) + i64::from(shift)).ok()?,
        minor,
    })
}

/// A group of the song (a verse, a chorus...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub uuid: String,
    pub name: String,
    /// The cues (slides) in it, in order.
    pub cue_ids: Vec<String>,
    /// What its slides say (the text of every text element), to tell two groups of the same name apart.
    signature: Vec<Vec<Vec<u8>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrangement {
    pub uuid: String,
    pub name: String,
    pub group_ids: Vec<String>,
}

/// `Presentation.music`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Music {
    pub original_text: String,
    pub user_text: String,
    pub original: Option<KeyScale>,
    pub user: Option<KeyScale>,
}

/// What StagePilot reads of a song file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Song {
    pub name: String,
    pub uuid: String,
    pub groups: Vec<Group>,
    pub arrangements: Vec<Arrangement>,
    /// The uuid of the selected arrangement.
    pub selected: Option<String>,
    pub music: Music,
    /// `multi_tracks_licensing.song_identifier`.
    pub multitracks_id: Option<u64>,
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The text of every text element of one cue, in order (`actions[].slide.presentation.base_slide.elements[].element.text.rtf_data`).
fn cue_texts(cue: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let Some(recs) = records(cue) else {
        return out;
    };
    for action in recs.iter().filter(|r| r.number == 10 && r.wire == 2) {
        let action = payload(cue, action);
        let Some(arecs) = records(action) else {
            continue;
        };
        let Some(slide) = first(action, &arecs, 23) else {
            continue;
        };
        let Some(srecs) = records(slide) else {
            continue;
        };
        let Some(presentation) = first(slide, &srecs, 2) else {
            continue;
        };
        let Some(precs) = records(presentation) else {
            continue;
        };
        let Some(base) = first(presentation, &precs, 1) else {
            continue;
        };
        let Some(brecs) = records(base) else {
            continue;
        };
        for element in brecs.iter().filter(|r| r.number == 1 && r.wire == 2) {
            let element = payload(base, element);
            let Some(erecs) = records(element) else {
                continue;
            };
            let Some(graphic) = first(element, &erecs, 1) else {
                continue;
            };
            let Some(grecs) = records(graphic) else {
                continue;
            };
            if let Some(text_message) = first(graphic, &grecs, 13) {
                let rtf = records(text_message)
                    .and_then(|t| first(text_message, &t, 5).map(<[u8]>::to_vec))
                    .unwrap_or_default();
                out.push(rtf);
            }
        }
    }
    out
}

fn read_music(message: &[u8]) -> Music {
    let Some(recs) = records(message) else {
        return Music::default();
    };
    let key = |n: u32| first(message, &recs, n).and_then(KeyScale::from_message);
    Music {
        original_text: first(message, &recs, 1).map(text).unwrap_or_default(),
        user_text: first(message, &recs, 2).map(text).unwrap_or_default(),
        original: key(3),
        user: key(4),
    }
}

impl Song {
    /// Reads a song file. `Err` when it is not a protobuf message with the parts of a presentation.
    pub fn read(data: &[u8]) -> Result<Self, ProError> {
        let recs = records(data).ok_or(ProError::NotASong)?;
        let mut song = Self {
            name: String::new(),
            uuid: String::new(),
            groups: Vec::new(),
            arrangements: Vec::new(),
            selected: None,
            music: Music::default(),
            multitracks_id: None,
        };
        let mut cues: HashMap<String, Vec<Vec<u8>>> = HashMap::new();
        let mut group_cues: Vec<(usize, Vec<String>)> = Vec::new();
        for rec in recs.iter().filter(|r| r.wire == 2) {
            let body = payload(data, rec);
            match rec.number {
                2 => song.uuid = uuid_text(body).ok_or(ProError::NotASong)?,
                3 => song.name = text(body),
                10 => song.selected = uuid_text(body).filter(|u| !u.is_empty()),
                11 => {
                    let arecs = records(body).ok_or(ProError::NotASong)?;
                    song.arrangements.push(Arrangement {
                        uuid: first(body, &arecs, 1)
                            .and_then(uuid_text)
                            .ok_or(ProError::NotASong)?,
                        name: first(body, &arecs, 2).map(text).unwrap_or_default(),
                        group_ids: arecs
                            .iter()
                            .filter(|r| r.number == 3 && r.wire == 2)
                            .filter_map(|r| uuid_text(payload(body, r)))
                            .collect(),
                    });
                }
                12 => {
                    let grecs = records(body).ok_or(ProError::NotASong)?;
                    let group = first(body, &grecs, 1).ok_or(ProError::NotASong)?;
                    let inner = records(group).ok_or(ProError::NotASong)?;
                    let ids: Vec<String> = grecs
                        .iter()
                        .filter(|r| r.number == 2 && r.wire == 2)
                        .filter_map(|r| uuid_text(payload(body, r)))
                        .collect();
                    group_cues.push((song.groups.len(), ids.clone()));
                    song.groups.push(Group {
                        uuid: first(group, &inner, 1)
                            .and_then(uuid_text)
                            .ok_or(ProError::NotASong)?,
                        name: first(group, &inner, 2).map(text).unwrap_or_default(),
                        cue_ids: ids,
                        signature: Vec::new(),
                    });
                }
                13 => {
                    let crecs = records(body).ok_or(ProError::NotASong)?;
                    if let Some(id) = first(body, &crecs, 1).and_then(uuid_text) {
                        cues.insert(id.to_ascii_lowercase(), cue_texts(body));
                    }
                }
                21 => {
                    let lrecs = records(body).ok_or(ProError::NotASong)?;
                    song.multitracks_id = lrecs
                        .iter()
                        .find(|r| r.number == 1 && r.wire == 0)
                        .and_then(|r| varint_at(body, r.body))
                        .map(|(v, _)| v)
                        .filter(|v| *v > 0 && *v < (1 << 63));
                }
                23 => song.music = read_music(body),
                _ => {}
            }
        }
        for (index, ids) in group_cues {
            song.groups[index].signature = ids
                .iter()
                .map(|id| {
                    cues.get(&id.to_ascii_lowercase())
                        .cloned()
                        .unwrap_or_default()
                })
                .collect();
        }
        if song.uuid.is_empty() && song.groups.is_empty() && song.arrangements.is_empty() {
            return Err(ProError::NotASong);
        }
        Ok(song)
    }

    fn group(&self, uuid: &str) -> Option<&Group> {
        self.groups.iter().find(|g| same(&g.uuid, uuid))
    }

    /// The arrangement the file has selected.
    pub fn selected_arrangement(&self) -> Option<&Arrangement> {
        let selected = self.selected.as_deref()?;
        self.arrangements.iter().find(|a| same(&a.uuid, selected))
    }

    /// The names of the groups of an arrangement, in order (a blank group has a blank name).
    pub fn arrangement_group_names(&self, arrangement: &Arrangement) -> Vec<String> {
        arrangement
            .group_ids
            .iter()
            .filter_map(|id| self.group(id).map(|g| g.name.clone()))
            .collect()
    }

    /// Whether the selected arrangement plays exactly these sections (same groups by name, same order) and, when the key can
    /// be understood, is in this key. This is "the copy already says what Playback plays now".
    pub fn already_plays(&self, sections: &[String], key: Option<&str>) -> bool {
        let Some(selected) = self.selected_arrangement() else {
            return false;
        };
        let have: Vec<String> = self
            .arrangement_group_names(selected)
            .iter()
            .map(|n| normalise(n))
            .collect();
        if have.len() != selected.group_ids.len() {
            return false;
        }
        let want: Vec<String> = sections.iter().map(|s| section_key(s)).collect();
        if have != want || want.is_empty() {
            return false;
        }
        match key.and_then(parse_key) {
            Some(wanted) => self.music.user == Some(wanted),
            None => true,
        }
    }
}

// ---- mapping the sections ---------------------------------------------------------------------------------------------

/// Lower case, letters and digits only.
pub fn normalise(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// A Playback section name as compared with a group name: the same, and "Count Off" is the group with a blank name.
fn section_key(section: &str) -> String {
    let n = normalise(section);
    if n == "countoff" {
        String::new()
    } else {
        n
    }
}

/// How a section was matched to a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// In the same place as the same name in the song's own `Full` arrangement.
    Full,
    /// The only group with that name (or, for Count Off, the only blank group).
    Only,
    /// Several groups have that name and say the same thing: the one in the same turn.
    Identical,
    /// Several groups have that name and they differ: one is guessed (see [`Guess`]) and the person checks it in ProPresenter.
    Guessed,
    /// No group has that name.
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapped {
    /// The section as Playback names it.
    pub section: String,
    pub how: How,
    /// The group's uuid (original, not remapped); `None` for `Missing`.
    pub group_uuid: Option<String>,
    /// The group's name as the song has it.
    pub group_name: Option<String>,
    /// Set for `Guessed`: which group of the same name was taken and why.
    pub guess: Option<Guess>,
}

/// Which rule picked the group of a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuessRule {
    /// The Nth use takes the Nth group of that name, in the order the song has them.
    InOrder,
    /// Playback uses the name once more than the song has groups: the extra use takes the last group again.
    LastReused,
    /// Playback uses the name at least twice more than the song has groups: the groups are used in turn, round and round.
    RepeatInTurn,
}

impl GuessRule {
    pub fn word(self) -> &'static str {
        match self {
            Self::InOrder => "in_order",
            Self::LastReused => "last_reused",
            Self::RepeatInTurn => "repeat_in_turn",
        }
    }
}

/// A section that fits more than one group of the song and was given one of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guess {
    /// Which use of this name in Playback's list it is, from 1.
    pub position: usize,
    /// How many times Playback uses the name.
    pub uses: usize,
    /// Which of the song's groups with that name was taken, from 1.
    pub group_index: usize,
    /// How many groups of the song have that name.
    pub groups: usize,
    pub rule: GuessRule,
}

/// `1st`, `2nd`, `3rd`, `4th`, `11th`, `21st`.
pub fn ordinal(n: usize) -> String {
    let suffix = match (n % 100, n % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

impl Guess {
    /// The plain sentence for the person: which group was taken, and the rule.
    pub fn words(&self, section: &str, group_name: &str) -> String {
        let group = if group_name.is_empty() {
            format!("the {} blank-named group", ordinal(self.group_index))
        } else {
            format!("the {} \"{group_name}\" group", ordinal(self.group_index))
        };
        let why = match self.rule {
            GuessRule::InOrder => "The song's groups with that name are used in the order the song has them.",
            GuessRule::LastReused => "Playback uses the name once more than the song has groups, so the last group is used again.",
            GuessRule::RepeatInTurn => "Playback uses the name more often than the song has groups, so the groups are used in turn, round and round.",
        };
        format!(
            "{section} ({} use) uses {group}: check it in ProPresenter. {why}",
            ordinal(self.position)
        )
    }
}

impl Mapped {
    /// `Full` is exact; everything else that maps needs a line in the preview.
    pub fn is_exact(&self) -> bool {
        self.how == How::Full
    }

    pub fn is_mapped(&self) -> bool {
        matches!(
            self.how,
            How::Full | How::Only | How::Identical | How::Guessed
        )
    }
}

/// The section names that could not be mapped (without repeats).
pub fn unmapped_names(map: &[Mapped]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in map.iter().filter(|m| !m.is_mapped()) {
        let label = format!("{} (no group has that name)", entry.section);
        if !out.contains(&label) {
            out.push(label);
        }
    }
    out
}

/// The pairs `(i, j)` with `a[i] == b[j]` that Python's `difflib.SequenceMatcher(None, a, b, autojunk=False)` puts in its
/// "equal" blocks: repeatedly the longest block (the first one, in `a` and then in `b`, when several are as long), then the
/// same on each side of it.
fn matching_pairs(a: &[String], b: &[String]) -> Vec<(usize, usize)> {
    let mut b2j: HashMap<&str, Vec<usize>> = HashMap::new();
    for (j, item) in b.iter().enumerate() {
        b2j.entry(item.as_str()).or_default().push(j);
    }
    let longest = |alo: usize, ahi: usize, blo: usize, bhi: usize| -> (usize, usize, usize) {
        let (mut best_i, mut best_j, mut best_k) = (alo, blo, 0usize);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for (i, item) in a.iter().enumerate().take(ahi).skip(alo) {
            let mut next: HashMap<usize, usize> = HashMap::new();
            if let Some(js) = b2j.get(item.as_str()) {
                for &j in js {
                    if j < blo {
                        continue;
                    }
                    if j >= bhi {
                        break;
                    }
                    let k = j2len.get(&j.wrapping_sub(1)).copied().unwrap_or(0) + 1;
                    next.insert(j, k);
                    if k > best_k {
                        best_i = i + 1 - k;
                        best_j = j + 1 - k;
                        best_k = k;
                    }
                }
            }
            j2len = next;
        }
        (best_i, best_j, best_k)
    };
    let mut pairs = Vec::new();
    let mut queue = vec![(0usize, a.len(), 0usize, b.len())];
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let (i, j, k) = longest(alo, ahi, blo, bhi);
        if k == 0 {
            continue;
        }
        pairs.extend((0..k).map(|d| (i + d, j + d)));
        if alo < i && blo < j {
            queue.push((alo, i, blo, j));
        }
        if i + k < ahi && j + k < bhi {
            queue.push((i + k, ahi, j + k, bhi));
        }
    }
    pairs
}

/// Matches Playback's section names (in playing order, repeats as separate entries, "Count Off" included) to the groups of
/// `song`. First by laying the names alongside the song's own `Full` arrangement (longest matching runs), then by the one group
/// with that name. When several groups have the name and they differ, one is guessed ([`How::Guessed`], with its [`Guess`]):
/// the person checks it in ProPresenter. A name no group has stays [`How::Missing`].
pub fn resolve(song: &Song, sections: &[String]) -> Vec<Mapped> {
    let gname: Vec<String> = song.groups.iter().map(|g| normalise(&g.name)).collect();
    // The Full arrangement as (group index) per entry; an entry naming no group is left out.
    let full: Vec<usize> = song
        .arrangements
        .iter()
        .find(|a| a.name == "Full")
        .map(|a| {
            a.group_ids
                .iter()
                .filter_map(|id| song.groups.iter().position(|g| same(&g.uuid, id)))
                .collect()
        })
        .unwrap_or_default();
    let fn_names: Vec<String> = full.iter().map(|&g| gname[g].clone()).collect();
    let wanted: Vec<String> = sections.iter().map(|s| normalise(s)).collect();
    let mut chosen: Vec<Option<(usize, How)>> = vec![None; sections.len()];
    for (i, j) in matching_pairs(&wanted, &fn_names) {
        chosen[i] = Some((full[j], How::Full));
    }
    // How many times Playback uses each name (all uses, also the ones `Full` already placed).
    let mut uses: HashMap<&str, usize> = HashMap::new();
    for name in &wanted {
        *uses.entry(name.as_str()).or_insert(0) += 1;
    }
    let mut guesses: Vec<Option<Guess>> = vec![None; sections.len()];
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for (i, name) in wanted.iter().enumerate() {
        let turn = {
            let count = seen.entry(name.as_str()).or_insert(0);
            let turn = *count;
            *count += 1;
            turn
        };
        if chosen[i].is_some() {
            continue;
        }
        let blank = name == "countoff";
        let candidates: Vec<usize> = gname
            .iter()
            .enumerate()
            .filter(|(_, g)| if blank { g.is_empty() } else { *g == name })
            .map(|(j, _)| j)
            .collect();
        chosen[i] = Some(match candidates.as_slice() {
            [] => (usize::MAX, How::Missing),
            [only] => (*only, How::Only),
            many => {
                let first_sig = &song.groups[many[0]].signature;
                if many.iter().all(|&j| &song.groups[j].signature == first_sig) {
                    (many[turn.min(many.len() - 1)], How::Identical)
                } else {
                    let (at, guess) = guess_group(
                        &full,
                        many,
                        turn,
                        uses.get(name.as_str()).copied().unwrap_or(1),
                    );
                    guesses[i] = Some(guess);
                    (at, How::Guessed)
                }
            }
        });
    }
    sections
        .iter()
        .zip(chosen)
        .zip(guesses)
        .map(|((section, found), guess)| {
            let (index, how) = found.unwrap_or((usize::MAX, How::Missing));
            let group = song.groups.get(index);
            Mapped {
                section: section.clone(),
                how,
                group_uuid: group.map(|g| g.uuid.clone()),
                group_name: group.map(|g| g.name.clone()),
                guess,
            }
        })
        .collect()
}

/// Which of several groups with one name the `turn`th use (from 0) of that name takes, when Playback uses the name `uses`
/// times in all. The groups are taken in the order the song's own `Full` arrangement first plays them when it plays every one
/// of them, else in the song's group order. With no more uses than groups the Nth use takes the Nth group; with exactly one use
/// too many the extra use takes the last group again (A, B, B); with two or more too many the groups go round and round
/// (A, B, A, B, A).
fn guess_group(full: &[usize], candidates: &[usize], turn: usize, uses: usize) -> (usize, Guess) {
    let first_in_full = |g: usize| full.iter().position(|&f| f == g);
    let mut ordered: Vec<usize> = candidates.to_vec();
    if ordered.iter().all(|&g| first_in_full(g).is_some()) {
        ordered.sort_by_key(|&g| first_in_full(g));
    }
    let groups = ordered.len();
    let extra = uses.saturating_sub(groups);
    let (slot, rule) = match extra {
        0 => (turn, GuessRule::InOrder),
        1 if turn < groups => (turn, GuessRule::InOrder),
        1 => (groups - 1, GuessRule::LastReused),
        _ => (turn % groups, GuessRule::RepeatInTurn),
    };
    (
        ordered[slot],
        Guess {
            position: turn + 1,
            uses,
            group_index: slot + 1,
            groups,
            rule,
        },
    )
}

// ---- making the copy --------------------------------------------------------------------------------------------------

/// What happened to the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Written into the copy (the text as stored).
    Written(String),
    /// Playback has a key but its text is not one StagePilot knows: the song's key was left alone.
    Unknown(String),
    /// Playback gave no key.
    NotGiven,
}

/// What [`build_copy`] gives back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    pub bytes: Vec<u8>,
    pub mapping: Vec<Mapped>,
    pub key: KeyOutcome,
    /// The new presentation's uuid.
    pub uuid: String,
}

/// Puts `new` where `number` belongs: replacing the records of that number, or before the first record of a higher number.
fn set_record(records: &mut Vec<(u32, Vec<u8>)>, number: u32, new: Vec<u8>) {
    if let Some(at) = records.iter().position(|(n, _)| *n == number) {
        records[at] = (number, new);
        let mut seen = false;
        records.retain(|(n, _)| {
            if *n != number {
                return true;
            }
            let keep = !seen;
            seen = true;
            keep
        });
        return;
    }
    let at = records
        .iter()
        .position(|(n, _)| *n > number)
        .unwrap_or(records.len());
    records.insert(at, (number, new));
}

/// Adds a record of a repeated field after the last one of that number (or where it belongs).
fn add_record(records: &mut Vec<(u32, Vec<u8>)>, number: u32, new: Vec<u8>) {
    let at = records
        .iter()
        .rposition(|(n, _)| *n == number)
        .map(|p| p + 1)
        .or_else(|| records.iter().position(|(n, _)| *n > number))
        .unwrap_or(records.len());
    records.insert(at, (number, new));
}

fn split(data: &[u8]) -> Option<Vec<(u32, Vec<u8>)>> {
    Some(
        records(data)?
            .iter()
            .map(|r| (r.number, data[r.start..r.end].to_vec()))
            .collect(),
    )
}

fn join(records: &[(u32, Vec<u8>)]) -> Vec<u8> {
    records
        .iter()
        .flat_map(|(_, bytes)| bytes.iter().copied())
        .collect()
}

fn timestamp(seconds: i64) -> Vec<u8> {
    // `Timestamp { int64 seconds = 1; int32 nanos = 2 }` with nanos 0 (not written).
    varint_record(1, u64::try_from(seconds).unwrap_or(0))
}

/// How a copy is asked for.
#[derive(Debug, Clone)]
pub struct CopyRequest<'a> {
    /// The sections Playback plays, in order (repeats included, "Count Off" included).
    pub sections: &'a [String],
    /// Playback's key text.
    pub key: Option<&'a str>,
    /// The new arrangement's name (`Playback`).
    pub arrangement_name: &'a str,
    /// Seconds since the epoch for `last_date_used` and `last_modified_date`.
    pub now: i64,
}

/// Makes the bytes of a copy of `source`: all its arrangements kept, one more added from the sections and selected, the key
/// written in, every uuid new (`new_uuid` gives them; the presentation's own too), fresh dates. The song's name is not changed.
/// Fails (and makes nothing) when a section has no single group, or when the result does not read back as intended.
pub fn build_copy(
    source: &[u8],
    request: &CopyRequest<'_>,
    new_uuid: &mut dyn FnMut() -> String,
) -> Result<Copy, ProError> {
    let song = Song::read(source)?;
    let mapping = resolve(&song, request.sections);
    if request.sections.is_empty() || mapping.iter().any(|m| !m.is_mapped()) {
        return Err(ProError::Unmapped(mapping));
    }
    // 1. every uuid replaced
    let mut top = split(source).ok_or(ProError::NotASong)?;
    let mut map: HashMap<String, String> = HashMap::new();
    for (number, bytes) in &mut top {
        let rec = records(bytes)
            .and_then(|r| r.into_iter().next())
            .ok_or(ProError::NotASong)?;
        if rec.wire != 2 {
            continue;
        }
        let inner = &bytes[rec.body..rec.end];
        let replaced = if let Some(old) = exact_uuid(inner).filter(|u| *u != NIL_UUID) {
            let new = map
                .entry(old.to_ascii_lowercase())
                .or_insert_with(&mut *new_uuid)
                .clone();
            Some(uuid_message(&new))
        } else {
            remap_uuids(inner, &mut map, new_uuid, 1)
        };
        if let Some(new_payload) = replaced {
            *bytes = len_record(*number, &new_payload);
        }
    }
    // 2. the new arrangement from the remapped group ids
    let renamed = |old: &str| -> Result<String, ProError> {
        map.get(&old.to_ascii_lowercase()).cloned().ok_or_else(|| {
            ProError::CopyCheckFailed("a group of the song has no uuid to keep".into())
        })
    };
    let arrangement_uuid = new_uuid();
    let mut arrangement = len_record(1, &uuid_message(&arrangement_uuid));
    arrangement.extend(len_record(2, request.arrangement_name.as_bytes()));
    for entry in &mapping {
        let old = entry.group_uuid.as_deref().unwrap_or_default();
        arrangement.extend(len_record(3, &uuid_message(&renamed(old)?)));
    }
    add_record(&mut top, 11, len_record(11, &arrangement));
    set_record(
        &mut top,
        10,
        len_record(10, &uuid_message(&arrangement_uuid)),
    );
    // 3. dates
    for number in [4, 5] {
        set_record(
            &mut top,
            number,
            len_record(number, &timestamp(request.now)),
        );
    }
    // 4. the key
    let key = match request.key.map(str::trim).filter(|k| !k.is_empty()) {
        None => KeyOutcome::NotGiven,
        Some(text) => match parse_key(text) {
            None => KeyOutcome::Unknown(text.to_string()),
            Some(scale) => {
                let old = top
                    .iter()
                    .find(|(n, _)| *n == 23)
                    .and_then(|(_, bytes)| records(bytes).map(|r| (bytes.clone(), r)));
                let mut music = match old {
                    Some((bytes, recs)) => {
                        let rec = recs.first().ok_or(ProError::NotASong)?;
                        split(&bytes[rec.body..rec.end]).ok_or(ProError::NotASong)?
                    }
                    None => Vec::new(),
                };
                let name = scale.name();
                set_record(&mut music, 2, len_record(2, name.as_bytes()));
                set_record(&mut music, 4, len_record(4, &scale.to_message()));
                set_record(&mut top, 23, len_record(23, &join(&music)));
                KeyOutcome::Written(name)
            }
        },
    };
    let bytes = join(&top);
    // 5. it must read back as intended
    let check = Song::read(&bytes)?;
    let selected_ok = check
        .selected_arrangement()
        .is_some_and(|a| a.name == request.arrangement_name && a.group_ids.len() == mapping.len());
    let groups_ok = check.selected_arrangement().is_some_and(|a| {
        a.group_ids
            .iter()
            .all(|id| check.groups.iter().any(|g| same(&g.uuid, id)))
    });
    if !selected_ok || !groups_ok || check.name != song.name {
        return Err(ProError::CopyCheckFailed(
            "the new arrangement was not found".into(),
        ));
    }
    if check.arrangements.len() != song.arrangements.len() + 1
        || check.groups.len() != song.groups.len()
    {
        return Err(ProError::CopyCheckFailed(
            "the arrangements or groups changed in number".into(),
        ));
    }
    Ok(Copy {
        bytes,
        mapping,
        key,
        uuid: check.uuid,
    })
}

#[doc(hidden)]
pub mod synthetic;

#[cfg(test)]
mod tests;
