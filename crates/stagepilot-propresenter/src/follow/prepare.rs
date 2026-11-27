//! Turns what StagePilot knows about each plan song into "can it be followed, and how": the song's Playback data (sections,
//! lyric cues), where it lives in ProPresenter (the binding), the song file read through the Presenter Link host and the
//! schedule (`super::build_schedule`). Slow (it reads ProPresenter's playlist and files), so it never runs on the following
//! path: the follower only reads its last result.

use super::binding::{
    bindings_in_playlist, bindings_of_items, playlist_items, Binding, BindingStore, FileCache,
    LookError, PlaylistItem,
};
use super::{build_schedule, CueProblem, LyricCue, Schedule, Section, Source, Unfollowable};
use crate::link::api::{ApiError, ProPresenterApi};
use crate::link::ProPresenterHost;
use std::collections::HashMap;
use std::sync::Arc;

/// What the status shows for a song, in church-operator words (`SPEC.md`, "Per-song readiness").
pub const FOLLOWING_BY_CUES: &str = "Following by Playback cues";
pub const FOLLOWING_BY_BARS: &str = "Following by bars (no Playback cues)";
pub const NO_CUES_ADVICE: &str =
    "No cues: in Playback choose a ProPresenter 1 Line Template for this song";
pub const ARRANGEMENT_CHANGED: &str = "Arrangement was changed in ProPresenter";
pub const NOT_IN_PLAYLIST: &str = "Not in the ProPresenter playlist";
pub const LINK_NOT_READY: &str = "ProPresenter Link not ready";
pub const SWITCH_OFF: &str = "Switch is off";
/// Two more cases the design list does not name.
pub const NOT_IN_PLAYBACK: &str = "Not in Playback's setlist";
pub const SECTIONS_CHANGED: &str =
    "The song's sections changed in Playback since it was added to ProPresenter";
pub const LIBRARY_NOT_READY: &str = "Playback's Library Link not ready";
pub const NO_SECTIONS: &str = "Playback has not given this song's sections yet";

/// What a plan song needs to be judged, taken from the Playback library plan.
#[derive(Debug, Clone, PartialEq)]
pub struct SongInput {
    /// The plan song's key (`LibraryPlanSong::key`).
    pub key: String,
    pub title: String,
    /// Playback's own number for the song in its setlist (what its heartbeat names); `None` when the song is not in Playback.
    pub playback_song_id: Option<i64>,
    /// The MultiTracks number.
    pub content_id: Option<u64>,
    pub sections: Vec<Section>,
    pub cues: Vec<LyricCue>,
}

/// Whether and how a song is followed.
#[derive(Debug, Clone, PartialEq)]
pub enum Readiness {
    Cues,
    /// Followed by bars; `Some` is the test the cues failed (`None`: it has no cues, which is the plain case).
    Bars(Option<CueProblem>),
    NotInPlayback,
    NotInPlaylist,
    ArrangementChanged,
    SectionsChanged,
    NoSections,
    LinkNotReady,
    /// Playback's Library Link does not answer, so the song's sections and cues are not known.
    LibraryNotReady,
    SwitchOff,
}

impl Readiness {
    /// The words shown for the song.
    pub fn text(&self) -> String {
        match self {
            Self::Cues => FOLLOWING_BY_CUES.to_string(),
            Self::Bars(None | Some(CueProblem::NoCues | CueProblem::TooFew { .. })) => {
                FOLLOWING_BY_BARS.to_string()
            }
            Self::Bars(Some(CueProblem::Mismatch {
                max_slide,
                file_cues,
            })) => format!(
                "Cues do not match the file (cues go to slide {max_slide}, file has {file_cues} cues)"
            ),
            Self::Bars(Some(other)) => format!("Following by bars (cues not used: {})", other.words()),
            Self::NotInPlayback => NOT_IN_PLAYBACK.to_string(),
            Self::NotInPlaylist => NOT_IN_PLAYLIST.to_string(),
            Self::ArrangementChanged => ARRANGEMENT_CHANGED.to_string(),
            Self::SectionsChanged => SECTIONS_CHANGED.to_string(),
            Self::NoSections => NO_SECTIONS.to_string(),
            Self::LinkNotReady => LINK_NOT_READY.to_string(),
            Self::LibraryNotReady => LIBRARY_NOT_READY.to_string(),
            Self::SwitchOff => SWITCH_OFF.to_string(),
        }
    }

    /// A second line that says what to do, when there is something to do.
    pub fn advice(&self) -> Option<String> {
        match self {
            Self::Bars(None | Some(CueProblem::NoCues | CueProblem::TooFew { .. })) => {
                Some(NO_CUES_ADVICE.to_string())
            }
            _ => None,
        }
    }

    /// `cues` or `bars` when the song is followed, else `None`.
    pub fn following(&self) -> Option<&'static str> {
        match self {
            Self::Cues => Some("cues"),
            Self::Bars(_) => Some("bars"),
            _ => None,
        }
    }

    /// A short code for the window.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Cues => "cues",
            Self::Bars(None | Some(CueProblem::NoCues | CueProblem::TooFew { .. })) => "bars",
            Self::Bars(Some(CueProblem::Mismatch { .. })) => "cues_do_not_match",
            Self::Bars(Some(_)) => "bars_cues_unusable",
            Self::NotInPlayback => "not_in_playback",
            Self::NotInPlaylist => "not_in_playlist",
            Self::ArrangementChanged => "arrangement_changed",
            Self::SectionsChanged => "sections_changed",
            Self::NoSections => "no_sections",
            Self::LinkNotReady => "link_not_ready",
            Self::LibraryNotReady => "library_not_ready",
            Self::SwitchOff => "switch_off",
        }
    }
}

/// Everything the follower needs for one followable song.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub key: String,
    pub title: String,
    pub schedule: Schedule,
    pub binding: Binding,
    /// The name of each section (arrangement entry), in playing order.
    pub section_names: Vec<String>,
}

/// One plan song's verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct SongStatus {
    pub key: String,
    pub title: String,
    pub playback_song_id: Option<i64>,
    pub readiness: Readiness,
    /// Present when the song is followed.
    pub prepared: Option<Arc<Prepared>>,
}

/// The reader of ProPresenter's playlists and song files.
pub struct Preparer {
    store: Arc<dyn BindingStore>,
    cache: tokio::sync::Mutex<FileCache>,
}

/// What one pass has already read, so each playlist is read once however many songs ask.
#[derive(Default)]
struct Looked {
    playlists: HashMap<String, Option<Vec<PlaylistItem>>>,
    /// The followable songs of the active playlist (`Err`: it could not be read).
    active: Option<Result<Vec<Binding>, ()>>,
}

enum Lookup {
    Found(Binding),
    Missing,
    Down,
}

impl Preparer {
    pub fn new(store: Arc<dyn BindingStore>) -> Self {
        Self {
            store,
            cache: tokio::sync::Mutex::new(FileCache::default()),
        }
    }

    pub fn store(&self) -> &Arc<dyn BindingStore> {
        &self.store
    }

    /// Reads the playlist of a job that has just added songs and remembers the songs it can bind (`only`: the files the job
    /// added or found, with `.pro`). Returns how many were remembered.
    pub async fn remember_playlist(
        &self,
        host: &dyn ProPresenterHost,
        api: &dyn ProPresenterApi,
        playlist_id: &str,
        only: &std::collections::BTreeSet<String>,
    ) -> Result<usize, LookError> {
        let mut cache = self.cache.lock().await;
        cache.refresh(host).await?;
        let playlist = api.playlist(playlist_id).await?;
        let found = bindings_of_items(
            host,
            &mut cache,
            playlist_id,
            &playlist_items(&playlist),
            Some(only),
        )
        .await?;
        let mut kept = 0;
        for binding in found {
            if self.store.remember(binding).is_ok() {
                kept += 1;
            }
        }
        Ok(kept)
    }

    /// Every song with the same reason, nothing read from anywhere (a song Playback does not have is said so instead).
    pub fn unavailable(songs: &[SongInput], why: Readiness) -> Vec<SongStatus> {
        songs
            .iter()
            .map(|song| SongStatus {
                key: song.key.clone(),
                title: song.title.clone(),
                playback_song_id: song.playback_song_id,
                readiness: if song.playback_song_id.is_none() && why != Readiness::SwitchOff {
                    Readiness::NotInPlayback
                } else {
                    why.clone()
                },
                prepared: None,
            })
            .collect()
    }

    /// Judges every song. `link_ready`: the ProPresenter Link and API are set up (nothing is read from ProPresenter when not).
    pub async fn prepare(
        &self,
        host: &dyn ProPresenterHost,
        api: &dyn ProPresenterApi,
        songs: &[SongInput],
        link_ready: bool,
    ) -> Vec<SongStatus> {
        let verdict = |song: &SongInput, readiness: Readiness| SongStatus {
            key: song.key.clone(),
            title: song.title.clone(),
            playback_song_id: song.playback_song_id,
            readiness,
            prepared: None,
        };
        if !link_ready {
            return Self::unavailable(songs, Readiness::LinkNotReady);
        }
        let mut cache = self.cache.lock().await;
        if cache.refresh(host).await.is_err() {
            return songs
                .iter()
                .map(|s| verdict(s, Readiness::LinkNotReady))
                .collect();
        }
        let stored = self.store.all();
        let mut looked = Looked::default();
        let mut out = Vec::with_capacity(songs.len());
        for song in songs {
            if song.playback_song_id.is_none() {
                out.push(verdict(song, Readiness::NotInPlayback));
                continue;
            }
            let Some(content_id) = song.content_id else {
                out.push(verdict(song, Readiness::NotInPlaylist));
                continue;
            };
            let lookup = self
                .binding_for(
                    host,
                    api,
                    content_id,
                    stored.get(&content_id),
                    &mut cache,
                    &mut looked,
                )
                .await;
            let binding = match lookup {
                Lookup::Found(b) => b,
                Lookup::Missing => {
                    out.push(verdict(song, Readiness::NotInPlaylist));
                    continue;
                }
                Lookup::Down => {
                    out.push(verdict(song, Readiness::LinkNotReady));
                    continue;
                }
            };
            match self.judge(host, song, binding, &mut cache).await {
                Ok(prepared) => {
                    let readiness = match prepared.schedule.source {
                        Source::Cues => Readiness::Cues,
                        Source::Bars => Readiness::Bars(prepared.schedule.cue_problem.clone()),
                    };
                    out.push(SongStatus {
                        key: song.key.clone(),
                        title: song.title.clone(),
                        playback_song_id: song.playback_song_id,
                        readiness,
                        prepared: Some(Arc::new(prepared)),
                    });
                }
                Err(readiness) => out.push(verdict(song, readiness)),
            }
        }
        out
    }

    /// The song's binding: the remembered one while its item is still in its playlist, else the one found in the active playlist
    /// (and then remembered).
    async fn binding_for(
        &self,
        host: &dyn ProPresenterHost,
        api: &dyn ProPresenterApi,
        content_id: u64,
        remembered: Option<&Binding>,
        cache: &mut FileCache,
        looked: &mut Looked,
    ) -> Lookup {
        if let Some(binding) = remembered {
            if !looked.playlists.contains_key(&binding.playlist_id) {
                let read = match api.playlist(&binding.playlist_id).await {
                    Ok(value) => Some(playlist_items(&value)),
                    Err(ApiError::NotFound) => None,
                    Err(_) => return Lookup::Down,
                };
                looked.playlists.insert(binding.playlist_id.clone(), read);
            }
            let item = looked
                .playlists
                .get(&binding.playlist_id)
                .and_then(Option::as_ref)
                .and_then(|items| {
                    items
                        .iter()
                        .find(|i| i.presentation_uuid == binding.presentation_uuid)
                });
            if let Some(item) = item {
                let mut binding = binding.clone();
                binding.item_index = item.index;
                return Lookup::Found(binding);
            }
        }
        if looked.active.is_none() {
            looked.active = Some(match api.active_playlist().await {
                Ok(None) => Ok(Vec::new()),
                Ok(Some(found)) => {
                    match find_all_in(host, api, cache, &found.playlist_uuid).await {
                        Ok(list) => Ok(list),
                        Err(_) => Err(()),
                    }
                }
                Err(_) => Err(()),
            });
        }
        match looked.active.as_ref() {
            Some(Ok(list)) => match list.iter().find(|b| b.content_id == content_id) {
                Some(found) => {
                    let _ = self.store.remember(found.clone());
                    Lookup::Found(found.clone())
                }
                None => Lookup::Missing,
            },
            Some(Err(())) => Lookup::Down,
            None => Lookup::Missing,
        }
    }

    /// Reads the song's file and builds its schedule; the file must be the arrangement the binding remembers.
    async fn judge(
        &self,
        host: &dyn ProPresenterHost,
        song: &SongInput,
        binding: Binding,
        cache: &mut FileCache,
    ) -> Result<Prepared, Readiness> {
        let parsed = match cache.parsed(host, &binding.file).await {
            Ok(Some(parsed)) => parsed,
            // The copy is gone or is not a song any more.
            Ok(None) => return Err(Readiness::NotInPlaylist),
            Err(_) => return Err(Readiness::LinkNotReady),
        };
        let Ok(map) = parsed.map.as_ref() else {
            return Err(Readiness::ArrangementChanged);
        };
        if map.arrangement_hash() != binding.arrangement_hash
            || parsed.multitracks_id != Some(binding.content_id)
        {
            return Err(Readiness::ArrangementChanged);
        }
        let schedule =
            build_schedule(map, &song.sections, &song.cues, None).map_err(|e| match e {
                Unfollowable::NoSections => Readiness::NoSections,
                Unfollowable::ArrangementDiffers { .. } => Readiness::SectionsChanged,
                Unfollowable::File(_) => Readiness::ArrangementChanged,
            })?;
        Ok(Prepared {
            key: song.key.clone(),
            title: song.title.clone(),
            schedule,
            binding,
            section_names: song.sections.iter().map(|s| s.name.clone()).collect(),
        })
    }
}

/// Every followable binding of one playlist (the active one), for a pass that asks about several songs.
async fn find_all_in(
    host: &dyn ProPresenterHost,
    api: &dyn ProPresenterApi,
    cache: &mut FileCache,
    playlist_id: &str,
) -> Result<Vec<Binding>, LookError> {
    bindings_in_playlist(host, api, cache, playlist_id, None).await
}
