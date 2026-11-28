//! ProPresenter's own HTTP API (port 1025) as far as the ProPresenter Link needs it: the playlists, the libraries and the version.
//!
//! The shapes are the ones proven on a real ProPresenter 19.0.1 (`docs/propresenter.md`): a library lists its songs as
//! `<Song> - [ <Library> ]` with an id that a file ProPresenter has not loaded yet does not have; a playlist is read as
//! `{id, items}` and written back, whole, as the array of its items.
//!
//! [`HttpApi`] speaks to the real thing. [`fake::FakeApi`] keeps the same rules in memory, on top of a
//! [`super::fake::FakeHost`], for tests.

use async_trait::async_trait;
use serde_json::Value;
use stagepilot_core::config::ProPresenterSettings;
use std::time::Duration;

/// The largest answer that is read.
const MAX_BODY: usize = 8 * 1024 * 1024;

/// Why a call to ProPresenter's API did not work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// Nothing answered (or it took too long).
    NoAnswer(String),
    /// 404: the playlist or library is not there, or (for a playlist write) one of its items was refused as a whole.
    NotFound,
    /// Any other status.
    Status(u16),
    /// The answer was not in the expected shape.
    Unreadable,
    /// The id was not plain text (letters, digits, dashes).
    BadId,
}

impl ApiError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoAnswer(_) => "no_answer",
            Self::NotFound => "not_found",
            Self::Status(_) => "status",
            Self::Unreadable => "unreadable",
            Self::BadId => "bad_id",
        }
    }

    pub fn words(&self) -> String {
        match self {
            Self::NoAnswer(why) => format!(
                "ProPresenter did not answer on its network port ({why}). Check that ProPresenter is open and its Network setting is on."
            ),
            Self::NotFound => "ProPresenter does not know that playlist or library.".to_string(),
            Self::Status(code) => format!("ProPresenter answered with the error {code}."),
            Self::Unreadable => "ProPresenter answered, but not in a way StagePilot understands.".to_string(),
            Self::BadId => "That id is not valid.".to_string(),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.words())
    }
}

impl std::error::Error for ApiError {}

/// One playlist (folders are flattened; `folder` says where it was).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistRef {
    pub uuid: String,
    pub name: String,
    /// The folders it sits in, `A / B`; `None` at the top.
    pub folder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryRef {
    pub uuid: String,
    pub name: String,
}

/// One entry of a library as ProPresenter lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryEntry {
    /// Missing while ProPresenter has not loaded the file yet.
    pub uuid: Option<String>,
    /// `<Song> - [ <Library> ]`.
    pub name: String,
}

impl LibraryEntry {
    /// The name without ` - [ Library ]`.
    pub fn display_name(&self, library: &str) -> String {
        let suffix = format!(" - [ {library} ]");
        self.name
            .strip_suffix(&suffix)
            .unwrap_or(&self.name)
            .to_string()
    }
}

/// The playlist item ProPresenter triggered last on its presentation layer (`GET /v1/playlist/active`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivePlaylist {
    pub playlist_uuid: String,
    pub playlist_name: String,
    /// The item's id is the presentation's uuid for a song.
    pub item_uuid: String,
    pub item_index: usize,
}

/// Where ProPresenter's presentation layer is now (`GET /v1/presentation/slide_index`): the active presentation and the
/// slide in it, counted in the SELECTED arrangement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlidePosition {
    pub presentation_uuid: String,
    pub index: usize,
}

/// What ProPresenter says is on its outputs (`GET /v1/status/layers`).
///
/// On the real rig the answer is `{"video_input":true,"media":false,"slide":false,"announcements":false,"props":false,
/// "messages":false,"audio":false}`. A camera input (`video_input`) is often on and `audio` only says something plays, so
/// neither counts as "something on screen" ([`Layers::on_screen`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Layers {
    pub video_input: bool,
    pub media: bool,
    pub slide: bool,
    pub announcements: bool,
    pub props: bool,
    pub messages: bool,
    pub audio: bool,
}

impl Layers {
    /// The layers that show something to the people in the room, in plain words (`slides`, `media`, ...).
    pub fn on_screen(&self) -> Vec<&'static str> {
        [
            (self.slide, "slides"),
            (self.media, "media"),
            (self.announcements, "announcements"),
            (self.props, "props"),
            (self.messages, "messages"),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| name)
        .collect()
    }
}

/// Reads `GET /v1/status/layers`. The five layers that count must all be there as true or false; anything else is `None`
/// (and so treated as live). `video_input` and `audio` are read when present and never decide anything.
pub fn parse_layers(value: &Value) -> Option<Layers> {
    let object = value.as_object()?;
    let flag = |name: &str| object.get(name).and_then(Value::as_bool);
    Some(Layers {
        video_input: flag("video_input").unwrap_or(false),
        media: flag("media")?,
        slide: flag("slide")?,
        announcements: flag("announcements")?,
        props: flag("props")?,
        messages: flag("messages")?,
        audio: flag("audio").unwrap_or(false),
    })
}

/// An id that is safe to put in a path.
pub fn plain_id(id: &str) -> Result<&str, ApiError> {
    if id.is_empty() || id.len() > 80 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(ApiError::BadId);
    }
    Ok(id)
}

/// ProPresenter's API.
#[async_trait]
pub trait ProPresenterApi: Send + Sync {
    /// `GET /version`: `ProPresenter 19.0.1` and the API version.
    async fn version(&self) -> Result<String, ApiError>;
    /// `GET /v1/playlists`, folders flattened.
    async fn playlists(&self) -> Result<Vec<PlaylistRef>, ApiError>;
    /// `GET /v1/playlist/{id}`: the whole answer (`{id, items}`).
    async fn playlist(&self, id: &str) -> Result<Value, ApiError>;
    /// `PUT /v1/playlist/{id}` with the whole list of items. It replaces everything.
    async fn put_playlist(&self, id: &str, items: &[Value]) -> Result<(), ApiError>;
    /// `GET /v1/libraries`.
    async fn libraries(&self) -> Result<Vec<LibraryRef>, ApiError>;
    /// `GET /v1/library/{id}`.
    async fn library(&self, id: &str) -> Result<Vec<LibraryEntry>, ApiError>;
    /// `GET /v1/status/layers`: what is on ProPresenter's outputs right now.
    async fn layers(&self) -> Result<Layers, ApiError>;
    /// `GET /v1/playlist/active`: the playlist and item last triggered on the presentation layer; `None` when there is none.
    async fn active_playlist(&self) -> Result<Option<ActivePlaylist>, ApiError>;
    /// `GET /v1/playlist/{playlist_id}/{index}/trigger`: shows the item (its first slide) in ProPresenter.
    async fn trigger_playlist_item(&self, playlist_id: &str, index: usize) -> Result<(), ApiError>;
    /// `GET /v1/presentation/active/{index}/trigger`: shows slide `index` of the active presentation, counted in its selected
    /// arrangement (ProPresenter answers 404 for a number that arrangement does not have).
    async fn trigger_slide(&self, index: usize) -> Result<(), ApiError>;
    /// `GET /v1/presentation/active`: the uuid of the active presentation; `None` when there is none.
    async fn active_presentation(&self) -> Result<Option<String>, ApiError>;
    /// `GET /v1/presentation/slide_index`: the active presentation and its current slide; `None` when there is none.
    async fn slide_position(&self) -> Result<Option<SlidePosition>, ApiError>;
    /// `GET /v1/presentation/{uuid}`: whether ProPresenter has opened this presentation. A file that is listed in a library but
    /// that ProPresenter has not opened yet answers 404 (`false`); a playlist that holds it is then refused as a whole.
    async fn presentation_opened(&self, uuid: &str) -> Result<bool, ApiError>;
}

fn text(value: &Value, name: &str) -> Option<String> {
    value.get(name)?.as_str().map(str::to_string)
}

/// `{uuid, name}` of an `id` object, or of the item itself when it has no `id`.
fn identity(entry: &Value) -> (Option<String>, Option<String>) {
    let id = entry.get("id").filter(|v| v.is_object()).unwrap_or(entry);
    (text(id, "uuid").filter(|u| !u.is_empty()), text(id, "name"))
}

/// Reads `GET /v1/playlists`: `[{id, type: "playlist" | "group", playlists: [...]}]`; a folder is entered, never listed.
pub fn parse_playlists(value: &Value) -> Option<Vec<PlaylistRef>> {
    fn walk(list: &[Value], folder: &[String], out: &mut Vec<PlaylistRef>, depth: usize) {
        if depth > 8 {
            return;
        }
        for entry in list {
            let (uuid, name) = identity(entry);
            let kind = entry.get("type").or_else(|| entry.get("field_type"));
            let children = entry
                .get("playlists")
                .or_else(|| entry.get("children"))
                .and_then(Value::as_array);
            let is_folder = matches!(kind.and_then(Value::as_str), Some("group" | "folder"))
                || (kind.is_none() && children.is_some());
            if is_folder {
                let mut inside = folder.to_vec();
                inside.push(name.unwrap_or_default());
                if let Some(children) = children {
                    walk(children, &inside, out, depth + 1);
                }
                continue;
            }
            if let (Some(uuid), Some(name)) = (uuid, name) {
                out.push(PlaylistRef {
                    uuid,
                    name,
                    folder: (!folder.is_empty()).then(|| folder.join(" / ")),
                });
            }
        }
    }
    let list = value.as_array()?;
    let mut out = Vec::new();
    walk(list, &[], &mut out, 0);
    Some(out)
}

/// Reads `GET /v1/libraries`: `[{id: {uuid, name, index}}]`.
pub fn parse_libraries(value: &Value) -> Option<Vec<LibraryRef>> {
    Some(
        value
            .as_array()?
            .iter()
            .filter_map(|entry| {
                let (uuid, name) = identity(entry);
                Some(LibraryRef {
                    uuid: uuid?,
                    name: name?,
                })
            })
            .collect(),
    )
}

/// Reads `GET /v1/library/{id}`: `{updateType, items: [{uuid, name, index}]}` (an item may also carry its fields in `id`).
pub fn parse_library(value: &Value) -> Option<Vec<LibraryEntry>> {
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .or_else(|| value.as_array())?;
    Some(
        items
            .iter()
            .filter_map(|entry| {
                let (uuid, name) = identity(entry);
                Some(LibraryEntry { uuid, name: name? })
            })
            .collect(),
    )
}

fn whole(value: &Value) -> Option<usize> {
    value.as_u64().and_then(|n| usize::try_from(n).ok())
}

/// Reads `GET /v1/playlist/active`: `{presentation: {playlist: {uuid, name, index}, item: {uuid, name, index}}, announcements}`.
/// Only the presentation layer counts; a missing or null layer is `None`.
pub fn parse_active_playlist(value: &Value) -> Option<ActivePlaylist> {
    let layer = value.get("presentation")?;
    let playlist = layer.get("playlist")?;
    let item = layer.get("item")?;
    Some(ActivePlaylist {
        playlist_uuid: text(playlist, "uuid").filter(|u| !u.is_empty())?,
        playlist_name: text(playlist, "name").unwrap_or_default(),
        item_uuid: text(item, "uuid").filter(|u| !u.is_empty())?,
        item_index: whole(item.get("index")?)?,
    })
}

/// Reads `GET /v1/presentation/active`: `{presentation: {id: {uuid, name, index}, groups: [...]}}`; the uuid only.
pub fn parse_active_presentation(value: &Value) -> Option<String> {
    let presentation = value.get("presentation")?;
    text(presentation.get("id")?, "uuid").filter(|u| !u.is_empty())
}

/// Reads `GET /v1/presentation/slide_index`: `{presentation_index: {index, presentation_id: {uuid, name, index}} | null}`.
pub fn parse_slide_position(value: &Value) -> Option<SlidePosition> {
    let at = value.get("presentation_index")?;
    Some(SlidePosition {
        presentation_uuid: text(at.get("presentation_id")?, "uuid").filter(|u| !u.is_empty())?,
        index: whole(at.get("index")?)?,
    })
}

/// The real thing, over plain HTTP.
pub struct HttpApi {
    http: reqwest::Client,
    base_url: String,
}

impl HttpApi {
    pub fn new(settings: &ProPresenterSettings) -> Self {
        // Cargo unifies reqwest's TLS features across the workspace: a crypto provider must be installed before any client.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let timeout = Duration::from_secs_f64(settings.request_timeout_seconds.clamp(0.5, 60.0));
        let http = reqwest::Client::builder()
            .connect_timeout(timeout)
            .read_timeout(timeout.max(Duration::from_secs(5)))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .unwrap_or_default();
        let host = if settings.host.contains(':') && !settings.host.starts_with('[') {
            format!("[{}]", settings.host)
        } else {
            settings.host.clone()
        };
        Self {
            http,
            base_url: format!("http://{host}:{}", settings.port),
        }
    }

    /// For tests: talk to a server at `http://host:port`.
    #[doc(hidden)]
    pub fn at(base_url: &str) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        Self {
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Option<Value>, ApiError> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url, path));
        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.to_string());
        }
        let response = request.send().await.map_err(|e| {
            ApiError::NoAnswer(if e.is_timeout() {
                "it took too long".to_string()
            } else {
                "the connection failed".to_string()
            })
        })?;
        let status = response.status().as_u16();
        if status == 404 {
            return Err(ApiError::NotFound);
        }
        if !(200..300).contains(&status) {
            return Err(ApiError::Status(status));
        }
        let bytes = response.bytes().await.map_err(|_| ApiError::Unreadable)?;
        if bytes.len() > MAX_BODY {
            return Err(ApiError::Unreadable);
        }
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok(None);
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| ApiError::Unreadable)
    }

    async fn get(&self, path: &str) -> Result<Value, ApiError> {
        self.call(reqwest::Method::GET, path, None)
            .await?
            .ok_or(ApiError::Unreadable)
    }
}

#[async_trait]
impl ProPresenterApi for HttpApi {
    async fn version(&self) -> Result<String, ApiError> {
        let value = self.get("/version").await?;
        let host = text(&value, "host_description").ok_or(ApiError::Unreadable)?;
        Ok(match text(&value, "api_version") {
            Some(api) => format!("{host} (API {api})"),
            None => host,
        })
    }

    async fn playlists(&self) -> Result<Vec<PlaylistRef>, ApiError> {
        parse_playlists(&self.get("/v1/playlists").await?).ok_or(ApiError::Unreadable)
    }

    async fn playlist(&self, id: &str) -> Result<Value, ApiError> {
        let value = self.get(&format!("/v1/playlist/{}", plain_id(id)?)).await?;
        value
            .get("items")
            .and_then(Value::as_array)
            .map(|_| value.clone())
            .ok_or(ApiError::Unreadable)
    }

    async fn put_playlist(&self, id: &str, items: &[Value]) -> Result<(), ApiError> {
        let body = Value::Array(items.to_vec());
        self.call(
            reqwest::Method::PUT,
            &format!("/v1/playlist/{}", plain_id(id)?),
            Some(&body),
        )
        .await
        .map(|_| ())
    }

    async fn libraries(&self) -> Result<Vec<LibraryRef>, ApiError> {
        parse_libraries(&self.get("/v1/libraries").await?).ok_or(ApiError::Unreadable)
    }

    async fn library(&self, id: &str) -> Result<Vec<LibraryEntry>, ApiError> {
        parse_library(&self.get(&format!("/v1/library/{}", plain_id(id)?)).await?)
            .ok_or(ApiError::Unreadable)
    }

    async fn layers(&self) -> Result<Layers, ApiError> {
        parse_layers(&self.get("/v1/status/layers").await?).ok_or(ApiError::Unreadable)
    }

    async fn active_playlist(&self) -> Result<Option<ActivePlaylist>, ApiError> {
        match self.get("/v1/playlist/active").await {
            Ok(value) => Ok(parse_active_playlist(&value)),
            Err(ApiError::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn trigger_playlist_item(&self, playlist_id: &str, index: usize) -> Result<(), ApiError> {
        self.call(
            reqwest::Method::GET,
            &format!("/v1/playlist/{}/{index}/trigger", plain_id(playlist_id)?),
            None,
        )
        .await
        .map(|_| ())
    }

    async fn trigger_slide(&self, index: usize) -> Result<(), ApiError> {
        self.call(
            reqwest::Method::GET,
            &format!("/v1/presentation/active/{index}/trigger"),
            None,
        )
        .await
        .map(|_| ())
    }

    async fn active_presentation(&self) -> Result<Option<String>, ApiError> {
        match self.get("/v1/presentation/active").await {
            Ok(value) => Ok(parse_active_presentation(&value)),
            Err(ApiError::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn presentation_opened(&self, uuid: &str) -> Result<bool, ApiError> {
        match self
            .get(&format!("/v1/presentation/{}", plain_id(uuid)?))
            .await
        {
            Ok(_) => Ok(true),
            Err(ApiError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    async fn slide_position(&self) -> Result<Option<SlidePosition>, ApiError> {
        match self.get("/v1/presentation/slide_index").await {
            Ok(value) => Ok(parse_slide_position(&value)),
            Err(ApiError::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

pub mod fake;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn playlists_are_flattened_with_their_folders() {
        let value = json!([
            {"id": {"uuid": "A1", "name": "Sunday", "index": 0}, "type": "playlist"},
            {"id": {"uuid": "B1", "name": "Evenings", "index": 1}, "type": "group", "playlists": [
                {"id": {"uuid": "B2", "name": "Evening 1", "index": 0}, "type": "playlist"},
                {"id": {"uuid": "C1", "name": "Deep", "index": 1}, "type": "group", "playlists": [
                    {"id": {"uuid": "C2", "name": "Late", "index": 0}, "type": "playlist"}]}]},
            {"id": {"uuid": "", "name": "No id"}, "type": "playlist"},
            {"id": {"uuid": "E1", "name": "Empty folder"}, "type": "group"}
        ]);
        let list = parse_playlists(&value).unwrap();
        let shown: Vec<(&str, Option<&str>)> = list
            .iter()
            .map(|p| (p.name.as_str(), p.folder.as_deref()))
            .collect();
        assert_eq!(
            shown,
            [
                ("Sunday", None),
                ("Evening 1", Some("Evenings")),
                ("Late", Some("Evenings / Deep"))
            ]
        );
        assert_eq!(parse_playlists(&json!({"x": 1})), None);
    }

    #[test]
    fn library_entries_may_have_no_id_yet() {
        let value = json!({"updateType": "all", "items": [
            {"index": 0, "name": "A - [ Playback ]", "uuid": "U1"},
            {"index": 1, "name": "B (1) - [ Playback ]", "uuid": ""},
            {"id": {"index": 2, "name": "C - [ Playback ]", "uuid": "U3"}},
            {"index": 3, "name": "D - [ Playback ]"}
        ]});
        let entries = parse_library(&value).unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].uuid.as_deref(), Some("U1"));
        assert_eq!(entries[1].uuid, None);
        assert_eq!(entries[2].uuid.as_deref(), Some("U3"));
        assert_eq!(entries[3].uuid, None);
        assert_eq!(entries[1].display_name("Playback"), "B (1)");
        assert_eq!(entries[1].display_name("Songs"), "B (1) - [ Playback ]");
        let libraries =
            parse_libraries(&json!([{"id": {"uuid": "L1", "name": "Songs", "index": 0}}])).unwrap();
        assert_eq!(libraries[0].name, "Songs");
    }

    #[test]
    fn layers_are_read_as_on_screen_or_not() {
        let rig = json!({"video_input": true, "media": false, "slide": false, "announcements": false,
            "props": false, "messages": false, "audio": true});
        let layers = parse_layers(&rig).unwrap();
        assert!(layers.video_input && layers.audio);
        // A camera input and audio alone are not "something on screen".
        assert!(layers.on_screen().is_empty());
        for (layer, word) in [
            ("slide", "slides"),
            ("media", "media"),
            ("announcements", "announcements"),
            ("props", "props"),
            ("messages", "messages"),
        ] {
            let mut live = rig.clone();
            live[layer] = json!(true);
            assert_eq!(parse_layers(&live).unwrap().on_screen(), [word]);
        }
        let mut two = rig.clone();
        two["slide"] = json!(true);
        two["props"] = json!(true);
        assert_eq!(parse_layers(&two).unwrap().on_screen(), ["slides", "props"]);
        // A missing or non-boolean layer that counts, or a non-object, cannot be read.
        for bad in [
            json!([]),
            json!("slide"),
            json!({}),
            json!({"video_input": true, "audio": false}),
            json!({"media": false, "slide": "no", "announcements": false, "props": false, "messages": false}),
            json!({"media": false, "slide": null, "announcements": false, "props": false, "messages": false}),
        ] {
            assert_eq!(parse_layers(&bad), None, "{bad}");
        }
    }

    #[test]
    fn only_plain_ids_go_into_a_path() {
        assert!(plain_id("942C3FC3-C4B2-44F7-A55D-4CC913BB8A5D").is_ok());
        for bad in ["", "a/b", "a b", "../x", "a?b", &"a".repeat(81)] {
            assert_eq!(plain_id(bad), Err(ApiError::BadId), "{bad:?}");
        }
    }

    #[test]
    fn the_follow_reads_are_parsed_and_junk_is_none() {
        let active = json!({"presentation": {"playlist": {"uuid": "PL", "name": "Sunday", "index": 2},
            "item": {"uuid": "IT", "name": "Song", "index": 4}}, "announcements": null});
        assert_eq!(
            parse_active_playlist(&active),
            Some(ActivePlaylist {
                playlist_uuid: "PL".into(),
                playlist_name: "Sunday".into(),
                item_uuid: "IT".into(),
                item_index: 4
            })
        );
        for none in [
            json!({"presentation": null}),
            json!({"announcements": {}}),
            json!({"presentation": {"playlist": {"uuid": "", "name": "x", "index": 0}, "item": {"uuid": "I", "name": "x", "index": 0}}}),
            json!({"presentation": {"playlist": {"uuid": "P", "name": "x", "index": 0}, "item": {"uuid": "I", "name": "x", "index": -1}}}),
            json!([]),
        ] {
            assert_eq!(parse_active_playlist(&none), None, "{none}");
        }
        let presentation = json!({"presentation": {"id": {"uuid": "PU", "name": "Song", "index": 0}, "groups": []}});
        assert_eq!(
            parse_active_presentation(&presentation).as_deref(),
            Some("PU")
        );
        assert_eq!(
            parse_active_presentation(&json!({"presentation": null})),
            None
        );
        assert_eq!(parse_active_presentation(&json!({})), None);
        let slide = json!({"presentation_index": {"index": 13, "presentation_id": {"uuid": "PU", "name": "Song", "index": 0}}});
        assert_eq!(
            parse_slide_position(&slide),
            Some(SlidePosition {
                presentation_uuid: "PU".into(),
                index: 13
            })
        );
        assert_eq!(
            parse_slide_position(&json!({"presentation_index": null})),
            None
        );
        assert_eq!(
            parse_slide_position(&json!({"presentation_index": {"index": "3"}})),
            None
        );
    }
}
