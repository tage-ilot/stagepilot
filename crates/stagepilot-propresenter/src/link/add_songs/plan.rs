//! The preview: what would be done, decided from what ProPresenter has now. Reads only.

use super::{
    guesses_of, AddSongs, ChosenBy, MappingLine, Place, Plan, PlanRequest, PlaylistPlan,
    SongAction, SongPlan, WantedSong, MAX_SONGS,
};
use crate::link::api::{ApiError, LibraryEntry, PlaylistRef};
use crate::link::{HostInfo, IndexEntry, LibraryFile, LinkError, PLAYBACK_LIBRARY, SONGS_LIBRARY};
use crate::pro_file::{parse_key, resolve, unmapped_names, How, Mapped, Song};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// The longest file name made (without `.pro`).
const MAX_NAME: usize = 80;
/// Files of one song that are opened to look for a copy that already plays the right sections.
const MAX_COPIES_LOOKED_AT: usize = 8;
/// Files of one song in `Songs` that are opened.
const MAX_SOURCES_LOOKED_AT: usize = 6;

/// Why no plan could be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The ProPresenter computer's files could not be read.
    Host(LinkError),
    /// ProPresenter's API did not answer as needed.
    Api(ApiError),
    /// No playlist with that id.
    PlaylistNotFound,
    /// No song was chosen.
    NoSongs,
    /// More songs than one change takes.
    TooMany,
}

impl PlanError {
    pub fn words(&self) -> String {
        match self {
            Self::Host(e) => e.words(),
            Self::Api(e) => e.words(),
            Self::PlaylistNotFound => {
                "ProPresenter has no playlist with that id. Pick the playlist again.".into()
            }
            Self::NoSongs => "Choose at least one song.".into(),
            Self::TooMany => format!("Choose at most {MAX_SONGS} songs at a time."),
        }
    }
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.words())
    }
}

impl std::error::Error for PlanError {}

/// A name as it may be used for a file: no dots, slashes or control characters, spaces collapsed, at most [`MAX_NAME`] letters.
pub fn clean_name(text: &str) -> String {
    let kept: String = text
        .chars()
        .filter(|c| !matches!(c, '.' | '/' | '\\') && !c.is_control())
        .collect();
    let collapsed = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .chars()
        .take(MAX_NAME)
        .collect::<String>()
        .trim()
        .to_string()
}

/// `<base> (n)`, at most [`MAX_NAME`] letters (the base is cut, never the number).
pub fn copy_file_name(base: &str, n: u32) -> String {
    let suffix = format!(" ({n})");
    let room = MAX_NAME.saturating_sub(suffix.chars().count());
    let cut: String = clean_name(base).chars().take(room).collect();
    format!("{}{suffix}", cut.trim_end())
}

/// `Song (3)` is `Song`: a copy of a copy is numbered from the song, not from the copy.
fn strip_copy_suffix(stem: &str) -> &str {
    if let Some(open) = stem.rfind(" (") {
        let tail = &stem[open + 2..];
        if let Some(digits) = tail.strip_suffix(')') {
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                return stem[..open].trim_end();
            }
        }
    }
    stem
}

pub(super) fn stem(file: &str) -> &str {
    file.strip_suffix(".pro").unwrap_or(file)
}

fn lower(name: &str) -> String {
    name.trim().to_lowercase()
}

/// A title or a file name to compare: lower case, trimmed, single spaces.
fn same_key(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A file as `choices` lists it and as a pick writes it: `Songs / Firm Foundation.pro`.
pub(super) fn file_label(library: &str, file: &str) -> String {
    format!("{library} / {file}")
}

/// Where the person's pick is among the candidates (written as `choices` lists them); `None` when nothing was picked or the
/// pick is none of them.
pub(super) fn picked_index(pick: Option<&String>, labels: &[String]) -> Option<usize> {
    let pick = pick?;
    labels.iter().position(|l| l == pick)
}

/// The title rule: the one candidate whose file name (without `.pro`) is the song's title. `None` when none or more than one is.
pub(super) fn title_index(title: &str, files: &[&str]) -> Option<usize> {
    let wanted = same_key(title);
    if wanted.is_empty() {
        return None;
    }
    let mut hits = files
        .iter()
        .enumerate()
        .filter(|(_, f)| same_key(stem(f)) == wanted)
        .map(|(i, _)| i);
    let first = hits.next()?;
    hits.next().is_none().then_some(first)
}

/// The one plain line that says a file was used without the person having to ask for it.
pub(super) fn used_note(by: ChosenBy, file: &str, playback_title: bool) -> String {
    let name = stem(file);
    match by {
        ChosenBy::Pick => format!("Used \"{name}\": the file you picked."),
        ChosenBy::Title if playback_title => {
            format!("Used \"{name}\": its name matches the Playback title.")
        }
        ChosenBy::Title => format!("Used \"{name}\": its name matches the title."),
    }
}

/// The lowest `n` from 1 for which `<base> (n)` is not taken; the name is taken then.
fn claim_copy_name(base: &str, taken: &mut BTreeSet<String>) -> String {
    let mut n = 1u32;
    loop {
        let name = copy_file_name(base, n);
        if !taken.contains(&lower(&name)) {
            taken.insert(lower(&name));
            return name;
        }
        n += 1;
    }
}

// ---- the playlist's items ---------------------------------------------------------------------------------------------

pub(crate) fn item_kind(item: &Value) -> &str {
    item.get("type")
        .and_then(Value::as_str)
        .unwrap_or("presentation")
}

pub(crate) fn item_name(item: &Value) -> String {
    item["id"]["name"].as_str().unwrap_or_default().to_string()
}

pub(crate) fn items_of(playlist: &Value) -> Vec<Value> {
    playlist
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Where in `items` the new songs go: for `End` after the last item; for a header, before the next header after it (or the end).
pub(crate) fn insert_position(items: &[Value], place: &Place) -> Result<usize, String> {
    match place {
        Place::End => Ok(items.len()),
        Place::AfterHeader(name) => {
            let wanted = lower(name);
            let headers: Vec<usize> = items
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    item_kind(item) == "header" && lower(&item_name(item)) == wanted
                })
                .map(|(i, _)| i)
                .collect();
            match headers.as_slice() {
                [] => Err(format!(
                    "This playlist has no header named \"{}\". Pick another place.",
                    name.trim()
                )),
                [only] => Ok(items
                    .iter()
                    .enumerate()
                    .skip(only + 1)
                    .find(|(_, item)| item_kind(item) == "header")
                    .map_or(items.len(), |(i, _)| i)),
                _ => Err(format!(
                    "More than one header in this playlist is named \"{}\". Pick another place.",
                    name.trim()
                )),
            }
        }
    }
}

fn place_words(place: &Place) -> String {
    match place {
        Place::End => "at the end of the playlist".to_string(),
        Place::AfterHeader(name) => format!(
            "at the end of the section under the header \"{}\"",
            name.trim()
        ),
    }
}

/// Every uuid an item stands for.
pub(crate) fn item_uuids(items: &[Value]) -> HashSet<String> {
    let mut out = HashSet::new();
    for item in items {
        for uuid in [
            item["id"]["uuid"].as_str(),
            item["presentation_info"]["presentation_uuid"].as_str(),
        ]
        .into_iter()
        .flatten()
        .filter(|u| !u.is_empty())
        {
            out.insert(uuid.to_ascii_uppercase());
        }
    }
    out
}

fn items_hash(items: &[Value]) -> String {
    let mut digest = Sha256::new();
    for item in items {
        digest.update(item_kind(item).as_bytes());
        digest.update([0]);
        digest.update(item_name(item).as_bytes());
        digest.update([0]);
        digest.update(item["id"]["uuid"].as_str().unwrap_or_default().as_bytes());
        digest.update([0]);
        digest.update(
            item.get("duration")
                .map(Value::to_string)
                .unwrap_or_default()
                .as_bytes(),
        );
        digest.update([1]);
    }
    hex::encode(digest.finalize())[..16].to_string()
}

// ---- what is known ----------------------------------------------------------------------------------------------------

pub(crate) struct World {
    pub info: HostInfo,
    pub index: Vec<IndexEntry>,
    /// The files of the Playback library with their times.
    pub playback: Vec<LibraryFile>,
    pub playlist: PlaylistRef,
    pub playlist_json: Value,
    /// The Playback library as ProPresenter lists it (when it lists it).
    pub entries: Vec<LibraryEntry>,
    /// The Songs library as ProPresenter lists it: read only when a song is asked for by its title alone.
    pub songs: Vec<LibraryEntry>,
    pub version: Option<String>,
}

impl AddSongs {
    pub(crate) async fn world(&self, playlist_id: &str) -> Result<World, PlanError> {
        let info = self.host.info().await.map_err(PlanError::Host)?;
        let index = self.host.song_index().await.map_err(PlanError::Host)?;
        let playback = if info.libraries.iter().any(|l| l == PLAYBACK_LIBRARY) {
            self.host
                .list_library(PLAYBACK_LIBRARY)
                .await
                .map_err(PlanError::Host)?
        } else {
            Vec::new()
        };
        let playlists = self.api.playlists().await.map_err(PlanError::Api)?;
        let playlist = playlists
            .into_iter()
            .find(|p| p.uuid.eq_ignore_ascii_case(playlist_id))
            .ok_or(PlanError::PlaylistNotFound)?;
        let playlist_json = self
            .api
            .playlist(&playlist.uuid)
            .await
            .map_err(|e| match e {
                ApiError::NotFound => PlanError::PlaylistNotFound,
                other => PlanError::Api(other),
            })?;
        let version = self.api.version().await.ok();
        let entries = self.playback_entries().await.unwrap_or_default();
        Ok(World {
            info,
            index,
            playback,
            playlist,
            playlist_json,
            entries,
            songs: Vec::new(),
            version,
        })
    }

    /// The Playback library as ProPresenter lists it now.
    pub(crate) async fn playback_entries(&self) -> Result<Vec<LibraryEntry>, ApiError> {
        self.library_entries(PLAYBACK_LIBRARY).await
    }

    /// A library as ProPresenter lists it now (empty when ProPresenter has no library of that name).
    pub(crate) async fn library_entries(&self, name: &str) -> Result<Vec<LibraryEntry>, ApiError> {
        let libraries = self.api.libraries().await?;
        match libraries.iter().find(|l| l.name == name) {
            Some(library) => self.api.library(&library.uuid).await,
            None => Ok(Vec::new()),
        }
    }

    async fn song_of(&self, library: &str, file: &str) -> Option<Song> {
        let bytes = self.host.read_file(library, file).await.ok()?;
        Song::read(&bytes).ok()
    }

    /// The preview. Reads ProPresenter's files and API; writes nothing.
    pub async fn preview(&self, request: &PlanRequest) -> Result<Plan, PlanError> {
        if request.songs.is_empty() {
            return Err(PlanError::NoSongs);
        }
        if request.songs.len() > MAX_SONGS {
            return Err(PlanError::TooMany);
        }
        let mut world = self.world(&request.playlist_id).await?;
        if request.songs.iter().any(|w| w.as_is) {
            world.songs = self
                .library_entries(SONGS_LIBRARY)
                .await
                .unwrap_or_default();
        }
        let items = items_of(&world.playlist_json);
        let in_playlist = item_uuids(&items);
        // Names already used by any file, in any library, plus the ones this plan claims.
        let mut taken: BTreeSet<String> =
            world.index.iter().map(|e| lower(stem(&e.file))).collect();
        let mut songs: Vec<SongPlan> = Vec::new();
        let mut ordered: Vec<&WantedSong> = request.songs.iter().collect();
        ordered.sort_by_key(|w| w.position);
        for wanted in ordered {
            songs.push(
                self.plan_song(wanted, &world, &mut taken, &in_playlist, &request.sources)
                    .await,
            );
        }
        let (insert_at, blocked) = match insert_position(&items, &request.place) {
            Ok(at) => (at, None),
            Err(why) => (items.len(), Some(why)),
        };
        let stuck = songs.iter().filter(|s| s.action.is_stuck()).count();
        let to_add = songs
            .iter()
            .filter(|s| !s.action.is_stuck() && !s.already_in_playlist)
            .count();
        let mut warnings = Vec::new();
        let already = songs.iter().filter(|s| s.already_in_playlist).count();
        if already > 0 {
            warnings.push(format!(
                "{already} of these songs are already in the playlist and will not be added again."
            ));
        }
        let playlist = PlaylistPlan {
            id: world.playlist.uuid.clone(),
            name: world.playlist.name.clone(),
            folder: world.playlist.folder.clone(),
            place: place_words(&request.place),
            insert_at: u32::try_from(insert_at).unwrap_or(u32::MAX),
            total_items: u32::try_from(items.len()).unwrap_or(u32::MAX),
            before: insert_at
                .checked_sub(1)
                .and_then(|i| items.get(i))
                .map(item_name),
            after: items.get(insert_at).map(item_name),
            untouched: u32::try_from(items.len()).unwrap_or(u32::MAX),
            blocked: blocked.clone(),
            items_hash: items_hash(&items),
        };
        let mut plan = Plan {
            plan_id: String::new(),
            can_apply: blocked.is_none() && to_add > 0,
            playlist,
            to_add: u32::try_from(to_add).unwrap_or(u32::MAX),
            stuck: u32::try_from(stuck).unwrap_or(u32::MAX),
            songs,
            propresenter: None,
            can_import: world.info.can_import,
            warnings,
        };
        plan.plan_id = plan_id(&plan, &request.sources);
        plan.propresenter = world.version;
        Ok(plan)
    }

    async fn plan_song(
        &self,
        wanted: &WantedSong,
        world: &World,
        taken: &mut BTreeSet<String>,
        in_playlist: &HashSet<String>,
        sources: &BTreeMap<String, String>,
    ) -> SongPlan {
        let pick = sources.get(&wanted.ref_id);
        let mut plan = SongPlan {
            ref_id: wanted.ref_id.clone(),
            title: wanted.title.clone(),
            position: wanted.position,
            content_id: wanted.content_id,
            action: SongAction::Blocked,
            file: None,
            source: None,
            source_file: None,
            import_as: None,
            reason: None,
            choices: Vec::new(),
            chosen_by: None,
            chosen_file: None,
            notes: Vec::new(),
            mapping: Vec::new(),
            guesses: Vec::new(),
            sections: u32::try_from(wanted.sections.len()).unwrap_or(u32::MAX),
            key: wanted.key.clone(),
            already_in_playlist: false,
        };
        let stuck = |mut plan: SongPlan, why: String| {
            plan.reason = Some(why);
            plan
        };
        if wanted.as_is {
            return Self::plan_as_is(plan, wanted, world, in_playlist, pick);
        }
        let Some(id) = wanted.content_id else {
            return stuck(
                plan,
                "Playback did not give this song's MultiTracks number, so StagePilot cannot find it in ProPresenter."
                    .into(),
            );
        };
        if wanted.rental_expired {
            return stuck(
                plan,
                "The rental of this song has run out in Playback.".into(),
            );
        }
        if wanted.sections.is_empty() {
            return stuck(
                plan,
                "Playback gave no arrangement (no sections) for this song, so there is nothing to copy.".into(),
            );
        }
        plan.notes.extend(key_note(wanted.key.as_deref()));
        // 1. a copy in the Playback library that already plays what Playback plays
        let mut copies: Vec<&IndexEntry> = world
            .index
            .iter()
            .filter(|e| e.multitracks_id == Some(id) && e.library == PLAYBACK_LIBRARY)
            .collect();
        let mtime = |file: &str| {
            world
                .playback
                .iter()
                .find(|f| f.name == file)
                .map_or(0, |f| f.mtime)
        };
        copies.sort_by(|a, b| {
            mtime(&b.file)
                .cmp(&mtime(&a.file))
                .then_with(|| b.file.cmp(&a.file))
        });
        for copy in copies.iter().take(MAX_COPIES_LOOKED_AT) {
            if let Some(song) = self.song_of(&copy.library, &copy.file).await {
                if song.already_plays(&wanted.sections, wanted.key.as_deref()) {
                    plan.action = SongAction::Reuse;
                    plan.file = Some(stem(&copy.file).to_string());
                    plan.source = Some(format!("{PLAYBACK_LIBRARY} / {}", copy.file));
                    let wanted_name = format!("{} - [ {PLAYBACK_LIBRARY} ]", stem(&copy.file));
                    plan.already_in_playlist = world
                        .entries
                        .iter()
                        .filter(|e| e.name == wanted_name)
                        .filter_map(|e| e.uuid.as_deref())
                        .any(|u| in_playlist.contains(&u.to_ascii_uppercase()));
                    plan.notes.insert(
                        0,
                        "A copy that already plays this arrangement is in the Playback library, so nothing new is made."
                            .into(),
                    );
                    return plan;
                }
            }
        }
        // 2. the song in ProPresenter to make a copy from
        let mut sources: Vec<&IndexEntry> = world
            .index
            .iter()
            .filter(|e| e.multitracks_id == Some(id) && e.library != PLAYBACK_LIBRARY)
            .collect();
        sources.sort_by(|a, b| {
            (a.library != SONGS_LIBRARY, &a.library, &a.file).cmp(&(
                b.library != SONGS_LIBRARY,
                &b.library,
                &b.file,
            ))
        });
        if sources.is_empty() {
            if let Some(newest) = copies.first() {
                // Only earlier copies exist: build from the newest.
                return self
                    .plan_copy(plan, wanted, &[(*newest).clone()], taken, true, pick)
                    .await;
            }
            return self.plan_import(plan, wanted, world, taken);
        }
        let candidates: Vec<IndexEntry> = sources
            .into_iter()
            .take(MAX_SOURCES_LOOKED_AT)
            .cloned()
            .collect();
        self.plan_copy(plan, wanted, &candidates, taken, false, pick)
            .await
    }

    fn plan_import(
        &self,
        mut plan: SongPlan,
        wanted: &WantedSong,
        world: &World,
        taken: &mut BTreeSet<String>,
    ) -> SongPlan {
        if !world.info.can_import {
            plan.reason = Some(format!(
                "ProPresenter does not have this song and it cannot be imported from here. {}",
                world.info.import_blocker.clone().unwrap_or_else(|| {
                    "Import it in ProPresenter once, then look again.".into()
                })
            ));
            return plan;
        }
        let base = clean_name(&wanted.title);
        if base.is_empty() {
            plan.reason =
                Some("The song's title has no letters StagePilot can use as a file name.".into());
            return plan;
        }
        // The name the import is given: the title, or `<title> (MultiTracks)` when another song already has the title.
        let mut import_as = base.clone();
        let mut n = 1;
        while taken.contains(&lower(&import_as)) {
            n += 1;
            import_as = if n == 2 {
                format!("{base} (MultiTracks)")
            } else {
                format!("{base} (MultiTracks {})", n - 1)
            };
            if n > 12 {
                plan.reason = Some("Too many songs already have this title.".into());
                return plan;
            }
        }
        taken.insert(lower(&import_as));
        let copy = claim_copy_name(&import_as, taken);
        plan.action = SongAction::ImportThenCopy;
        plan.notes.insert(
            0,
            format!(
                "ProPresenter does not have this song. It is imported through ProPresenter's own Search as \"{import_as}\", then copied."
            ),
        );
        plan.import_as = Some(import_as);
        plan.file = Some(copy);
        // The sections cannot be checked before the song is there: said plainly.
        plan.notes.push(
            "The sections can only be matched after the song is imported; if one has no group with that name, nothing is added for this song. Sections that fit more than one group are guessed and listed after the import."
                .into(),
        );
        plan
    }

    /// A copy from one of `candidates` (files with this song's number); `from_copy` when the candidate is an earlier copy.
    async fn plan_copy(
        &self,
        mut plan: SongPlan,
        wanted: &WantedSong,
        candidates: &[IndexEntry],
        taken: &mut BTreeSet<String>,
        from_copy: bool,
        pick: Option<&String>,
    ) -> SongPlan {
        struct Candidate {
            entry: IndexEntry,
            mapping: Vec<Mapped>,
            full: bool,
        }
        let mut read: Vec<Candidate> = Vec::new();
        for entry in candidates {
            if let Some(song) = self.song_of(&entry.library, &entry.file).await {
                let mapping = resolve(&song, &wanted.sections);
                let full = song.arrangements.iter().any(|a| a.name == "Full");
                read.push(Candidate {
                    entry: entry.clone(),
                    mapping,
                    full,
                });
            }
        }
        if read.is_empty() {
            plan.reason = Some(
                "ProPresenter's file for this song could not be read, so nothing can be copied from it.".into(),
            );
            return plan;
        }
        let mapped = |c: &Candidate| c.mapping.iter().all(Mapped::is_mapped);
        let label = |c: &Candidate| file_label(&c.entry.library, &c.entry.file);
        let mut settled: Option<ChosenBy> = None;
        let mut offered: Vec<String> = Vec::new();
        let chosen: &Candidate = if read.len() == 1 {
            &read[0]
        } else {
            let labels: Vec<String> = read.iter().map(label).collect();
            let good: Vec<&Candidate> = read.iter().filter(|c| c.full && mapped(c)).collect();
            if let Some(i) = picked_index(pick, &labels) {
                // The person's pick is used as it is: a file that cannot make the arrangement says why below.
                settled = Some(ChosenBy::Pick);
                offered = if good.len() > 1 && good.iter().any(|c| label(c) == labels[i]) {
                    good.iter().map(|c| label(c)).collect()
                } else {
                    labels
                };
                &read[i]
            } else {
                match good.as_slice() {
                    [one] => one,
                    [] => {
                        // None works: say why for the one that is closest.
                        read.iter()
                            .min_by_key(|c| c.mapping.iter().filter(|m| !m.is_mapped()).count())
                            .unwrap_or(&read[0])
                    }
                    several => {
                        let names: Vec<&str> =
                            several.iter().map(|c| c.entry.file.as_str()).collect();
                        offered = several.iter().map(|c| label(c)).collect();
                        match title_index(&wanted.title, &names) {
                            Some(i) => {
                                settled = Some(ChosenBy::Title);
                                several[i]
                            }
                            None => {
                                plan.action = SongAction::NeedsChoice;
                                plan.choices = offered;
                                plan.reason = Some("Two files are this song. Pick one.".into());
                                return plan;
                            }
                        }
                    }
                }
            }
        };
        if let Some(by) = settled {
            plan.chosen_by = Some(by);
            plan.chosen_file = Some(label(chosen));
            plan.choices = offered;
            plan.notes.push(used_note(by, &chosen.entry.file, true));
        }
        plan.mapping = chosen
            .mapping
            .iter()
            .map(|m| MappingLine {
                section: m.section.clone(),
                group: m.group_name.clone(),
                how: how_word(m.how).to_string(),
                line: confidence_line(m),
            })
            .collect();
        if !mapped(chosen) {
            plan.reason = Some(format!(
                "These sections have no group with that name in the song: {}. Nothing is made for this song.",
                unmapped_names(&chosen.mapping).join(", ")
            ));
            return plan;
        }
        plan.guesses = guesses_of(&chosen.mapping);
        // The guesses are listed on their own (`guesses`); the other lines are notes.
        plan.notes.extend(
            plan.mapping
                .iter()
                .filter(|m| m.how != "guessed")
                .filter_map(|m| m.line.clone()),
        );
        let source_stem = stem(&chosen.entry.file);
        let base = if from_copy || chosen.entry.library == PLAYBACK_LIBRARY {
            strip_copy_suffix(source_stem)
        } else {
            source_stem
        };
        plan.action = SongAction::Copy;
        plan.file = Some(claim_copy_name(base, taken));
        plan.source = Some(format!("{} / {}", chosen.entry.library, chosen.entry.file));
        plan.source_file = Some((chosen.entry.library.clone(), chosen.entry.file.clone()));
        plan
    }
}

pub(crate) fn how_word(how: How) -> &'static str {
    match how {
        How::Full => "full",
        How::Only => "only",
        How::Identical => "identical",
        How::Guessed => "guessed",
        How::Missing => "missing",
    }
}

/// A plain sentence for every match that is not the exact one.
fn confidence_line(m: &Mapped) -> Option<String> {
    let group = m.group_name.clone().unwrap_or_default();
    let shown = if group.is_empty() {
        "the blank group".to_string()
    } else {
        format!("\"{group}\"")
    };
    match m.how {
        How::Full => None,
        How::Only => Some(format!(
            "\"{}\": matched by name to {shown}, the only group with that name.",
            m.section
        )),
        How::Identical => Some(format!(
            "\"{}\": the song has several groups with that name and the same words; the one in turn ({shown}) is used.",
            m.section
        )),
        How::Guessed => m
            .guess
            .as_ref()
            .map(|g| g.words(&m.section, m.group_name.as_deref().unwrap_or_default())),
        How::Missing => Some(format!("\"{}\": no group has that name.", m.section)),
    }
}

fn key_note(key: Option<&str>) -> Option<String> {
    let key = key.map(str::trim).filter(|k| !k.is_empty())?;
    Some(match parse_key(key) {
        Some(scale) => format!(
            "The key {} is saved in the copy. ProPresenter does not show it for these songs; StagePilot does.",
            scale.name()
        ),
        None => format!(
            "Playback's key \"{key}\" is not one StagePilot knows, so the song's own key stays."
        ),
    })
}

/// The identity of a plan: a hash of everything that decides what apply would do.
fn plan_id(plan: &Plan, sources: &BTreeMap<String, String>) -> String {
    let mut copy = plan.clone();
    copy.plan_id = String::new();
    copy.propresenter = None;
    let bytes = serde_json::to_vec(&copy).unwrap_or_default();
    let mut digest = Sha256::new();
    digest.update(&bytes);
    // The picks the person made are part of what was previewed: another pick is another plan.
    for (ref_id, file) in sources {
        digest.update(ref_id.as_bytes());
        digest.update([2]);
        digest.update(file.as_bytes());
        digest.update([3]);
    }
    // `source_file` is not part of the JSON but is part of what apply does.
    for song in &copy.songs {
        if let Some((library, file)) = &song.source_file {
            digest.update(library.as_bytes());
            digest.update([0]);
            digest.update(file.as_bytes());
            digest.update([1]);
        }
    }
    hex::encode(digest.finalize())[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_have_no_dots_or_slashes_and_are_at_most_eighty_letters() {
        assert_eq!(
            clean_name("Emptied (feat. Tyana Reynolds)"),
            "Emptied (feat Tyana Reynolds)"
        );
        assert_eq!(clean_name("  A/B \\ C   D.E  "), "AB C DE");
        assert_eq!(clean_name("tab\there\n"), "tabhere");
        let long = "x".repeat(200);
        assert_eq!(clean_name(&long).chars().count(), 80);
        assert_eq!(copy_file_name("Song", 1), "Song (1)");
        assert_eq!(copy_file_name("Song (feat. X)", 12), "Song (feat X) (12)");
        let cut = copy_file_name(&long, 3);
        assert_eq!(cut.chars().count(), 80);
        assert!(cut.ends_with(" (3)"));
        assert_eq!(copy_file_name(&"y".repeat(78), 10).chars().count(), 80);
    }

    #[test]
    fn the_lowest_unused_number_is_taken_and_a_copy_of_a_copy_starts_from_the_song() {
        let mut taken: BTreeSet<String> = ["song (1)", "song (3)"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert_eq!(claim_copy_name("Song", &mut taken), "Song (2)");
        assert_eq!(claim_copy_name("Song", &mut taken), "Song (4)");
        assert_eq!(claim_copy_name("SONG", &mut taken), "SONG (5)");
        assert_eq!(strip_copy_suffix("Song (3)"), "Song");
        assert_eq!(strip_copy_suffix("Song (live)"), "Song (live)");
        assert_eq!(strip_copy_suffix("Song (feat X) (12)"), "Song (feat X)");
        assert_eq!(strip_copy_suffix("Song"), "Song");
        assert_eq!(strip_copy_suffix("Song ()"), "Song ()");
    }

    fn header(name: &str) -> Value {
        serde_json::json!({"id": {"uuid": "", "name": name, "index": 0}, "type": "header"})
    }

    fn song(name: &str) -> Value {
        serde_json::json!({"id": {"uuid": "U", "name": name, "index": 0}, "type": "presentation"})
    }

    #[test]
    fn a_header_place_is_the_end_of_that_headers_section() {
        let items = vec![
            header("Welcome"),
            song("a"),
            header("Worship"),
            song("b"),
            song("c"),
            header("Message"),
            song("d"),
        ];
        assert_eq!(insert_position(&items, &Place::End), Ok(7));
        assert_eq!(
            insert_position(&items, &Place::AfterHeader("Worship".into())),
            Ok(5)
        );
        assert_eq!(
            insert_position(&items, &Place::AfterHeader(" message ".into())),
            Ok(7)
        );
        assert_eq!(
            insert_position(&items, &Place::AfterHeader("Welcome".into())),
            Ok(2)
        );
        assert!(insert_position(&items, &Place::AfterHeader("Nope".into()))
            .unwrap_err()
            .contains("no header named \"Nope\""));
        let twice = vec![header("A"), header("A")];
        assert!(insert_position(&twice, &Place::AfterHeader("a".into()))
            .unwrap_err()
            .contains("More than one"));
        assert_eq!(insert_position(&[], &Place::End), Ok(0));
        // A song named like a header is not a header.
        assert!(
            insert_position(&[song("Worship")], &Place::AfterHeader("Worship".into())).is_err()
        );
    }

    #[test]
    fn the_key_note_says_what_happens_to_the_key() {
        assert!(key_note(Some("Eb"))
            .unwrap()
            .contains("The key Eb is saved"));
        assert!(key_note(Some("Am")).unwrap().contains("Am"));
        assert!(key_note(Some("H"))
            .unwrap()
            .contains("not one StagePilot knows"));
        assert_eq!(key_note(Some(" ")), None);
        assert_eq!(key_note(None), None);
    }
}
