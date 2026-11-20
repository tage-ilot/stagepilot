//! A song asked for by its title alone (the person has no Playback data for it): it is found in ProPresenter's Songs library
//! and goes into the playlist AS IT IS. Nothing is imported, copied or edited, and no arrangement or key is changed.

use super::plan::{clean_name, file_label, picked_index, stem, title_index, used_note, World};
use super::{AddSongs, ChosenBy, PlanError, SongAction, SongPlan, WantedSong};
use crate::link::{IndexEntry, SONGS_LIBRARY};
use serde::Serialize;
use std::collections::HashSet;
use ts_rs::TS;

/// What ProPresenter's Songs library has for one title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "TitleFound")]
pub enum TitleFound {
    /// Exactly one song has this title.
    Found,
    /// No song has it: only Playback's data can bring it in (an import).
    Missing,
    /// More than one song has it, so StagePilot does not guess.
    Several,
}

/// The answer for one title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleMatch {
    pub title: String,
    pub found: TitleFound,
}

/// Two titles are the same song when they read the same without case, dots, slashes and spacing: the way a file name is made.
fn key(text: &str) -> String {
    clean_name(text).to_lowercase()
}

/// The files of the Songs library whose name is this title.
fn songs_titled<'a>(index: &'a [IndexEntry], title: &str) -> Vec<&'a IndexEntry> {
    let wanted = key(title);
    if wanted.is_empty() {
        return Vec::new();
    }
    index
        .iter()
        .filter(|e| e.library == SONGS_LIBRARY && key(stem(&e.file)) == wanted)
        .collect()
}

impl AddSongs {
    /// Which of these titles ProPresenter's Songs library has. Reads the files only; nothing is written or asked of the screen.
    pub async fn find_titles(&self, titles: &[String]) -> Result<Vec<TitleMatch>, PlanError> {
        let index = self.host.song_index().await.map_err(PlanError::Host)?;
        Ok(titles
            .iter()
            .map(|title| TitleMatch {
                title: title.clone(),
                found: match songs_titled(&index, title).len() {
                    0 => TitleFound::Missing,
                    1 => TitleFound::Found,
                    _ => TitleFound::Several,
                },
            })
            .collect())
    }

    /// The plan for a song asked for by its title: the one Songs entry with that title, as it is, or the reason it cannot be.
    /// More than one entry: the person's pick, else the one whose file name is the title, else a choice to make.
    pub(super) fn plan_as_is(
        mut plan: SongPlan,
        wanted: &WantedSong,
        world: &World,
        in_playlist: &HashSet<String>,
        pick: Option<&String>,
    ) -> SongPlan {
        let title = wanted.title.trim();
        let files = songs_titled(&world.index, title);
        let (only, by): (&IndexEntry, Option<ChosenBy>) = match files.as_slice() {
            [] => {
                plan.reason = Some(format!(
                    "ProPresenter does not have \"{title}\" in its Songs library. Playback's data is needed to import it."
                ));
                return plan;
            }
            [only] => (only, None),
            several => {
                let labels: Vec<String> = several
                    .iter()
                    .map(|e| file_label(&e.library, &e.file))
                    .collect();
                let names: Vec<&str> = several.iter().map(|e| e.file.as_str()).collect();
                let settled = match picked_index(pick, &labels) {
                    Some(i) => Some((i, ChosenBy::Pick)),
                    None => title_index(title, &names).map(|i| (i, ChosenBy::Title)),
                };
                plan.choices = labels;
                match settled {
                    Some((i, by)) => (several[i], Some(by)),
                    None => {
                        plan.action = SongAction::NeedsChoice;
                        plan.reason = Some("Two files are this song. Pick one.".into());
                        return plan;
                    }
                }
            }
        };
        let name = stem(&only.file).to_string();
        let uuid = world
            .songs
            .iter()
            .find(|e| e.display_name(SONGS_LIBRARY) == name)
            .and_then(|e| e.uuid.clone())
            .filter(|u| !u.is_empty());
        let Some(uuid) = uuid else {
            plan.reason = Some(format!(
                "ProPresenter has \"{title}\" but does not list it yet. Open the Songs library in ProPresenter once, then press the button again."
            ));
            return plan;
        };
        plan.action = SongAction::AsIs;
        plan.already_in_playlist = in_playlist.contains(&uuid.to_ascii_uppercase());
        plan.source = Some(format!("{SONGS_LIBRARY} / {}", only.file));
        plan.file = Some(name);
        if let Some(by) = by {
            plan.chosen_by = Some(by);
            plan.chosen_file = Some(file_label(&only.library, &only.file));
            plan.notes.push(used_note(by, &only.file, false));
        }
        plan.notes.push(
            "Added as it is in ProPresenter: no copy is made and its arrangement and key are not changed."
                .into(),
        );
        plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(library: &str, file: &str) -> IndexEntry {
        IndexEntry {
            library: library.into(),
            file: file.into(),
            multitracks_id: None,
        }
    }

    #[test]
    fn titles_match_like_file_names_and_only_in_songs() {
        let index = vec![
            entry("Songs", "Holy Forever.pro"),
            entry("Songs", "Emptied (feat Tyana Reynolds).pro"),
            entry("Playback", "Holy Forever (1).pro"),
            entry("Other", "Gratitude.pro"),
        ];
        assert_eq!(songs_titled(&index, " holy  FOREVER ").len(), 1);
        assert_eq!(
            songs_titled(&index, "Emptied (feat. Tyana Reynolds)").len(),
            1,
            "ProPresenter cuts names at the first dot"
        );
        assert!(songs_titled(&index, "Holy Forever (1)").is_empty());
        assert!(songs_titled(&index, "Gratitude").is_empty());
        assert!(songs_titled(&index, "   ").is_empty());
    }
}
