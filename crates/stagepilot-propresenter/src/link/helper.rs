//! [`HelperHost`]: the ProPresenter Link when ProPresenter is on another computer. It speaks to the helper
//! (`propresenter_link.py`, port 8790) with signed requests.
//!
//! * Every request carries `X-PPL-Time` (seconds since the epoch) and `X-PPL-Sig`: the hex HMAC-SHA256, keyed with the key
//!   StagePilot made, of `METHOD\n<path and query as sent>\n<time>` and, for PUT and POST only, `\n<hex SHA-256 of the body>`.
//!   The key is read from the credential store for each request, is never logged and is never part of an error.
//! * No proxy, no redirect, private addresses only (loopback, RFC 1918, link-local): a computer that is not on this local
//!   network is never asked, not even the question. A name is looked up and every address it gives must be private; the
//!   connection then goes to the address that was checked.
//! * Time limits: 3 s for the small reads (`info`, `list_library`, `ensure_library`), 15 s for the song index, 30 s for a file
//!   and for `select_library`, 60 s for an import. An answer is read up to a fixed size and no further.

use super::paths::{
    ensure_name, import_name, library_name, playlist_name, sha256_hex, song_file_name, song_number,
};
use super::{
    HostInfo, IndexEntry, LibraryFile, LinkError, ProPresenterHost, SelectOutcome, MAX_FILE_BYTES,
    PLAYBACK_LIBRARY,
};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use stagepilot_core::config::DEFAULT_PROPRESENTER_LINK_PORT;
use stagepilot_core::settings::CredentialStore;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

const MIN_KEY_LEN: usize = 32;
const SMALL: Duration = Duration::from_secs(3);
const INDEX_TIME: Duration = Duration::from_secs(15);
const FILE_TIME: Duration = Duration::from_secs(30);
const IMPORT_TIME: Duration = Duration::from_secs(60);
const SMALL_CAP: usize = 64 * 1024;
const LIST_CAP: usize = 2 * 1024 * 1024;
const INDEX_CAP: usize = 16 * 1024 * 1024;

/// Lower-case hex HMAC-SHA256 of `message` under `key`.
pub fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    // HMAC takes a key of any length, so `new_from_slice` cannot fail.
    let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap_or_else(|_| std::process::abort());
    mac.update(message);
    hex::encode(mac.finalize().into_bytes())
}

/// The signature of one request, the same bytes the helper checks. `body` is `Some` for PUT and POST (an empty body is
/// `Some(&[])`): the hex SHA-256 of it is part of what is signed, so the body cannot be swapped under a good signature.
pub fn sign(key: &str, method: &str, target: &str, time: u64, body: Option<&[u8]>) -> String {
    let mut message = format!("{method}\n{target}\n{time}");
    if let Some(body) = body {
        message.push('\n');
        message.push_str(&hex::encode(Sha256::digest(body)));
    }
    hmac_sha256_hex(key.as_bytes(), message.as_bytes())
}

/// A key is usable when it is one line of plain characters the helper would accept.
pub fn usable_key(key: &str) -> bool {
    key.len() >= MIN_KEY_LEN && key.bytes().all(|b| (33..=126).contains(&b))
}

fn system_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// One path segment, percent-encoded so that what is signed is what is sent.
pub fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `true` for loopback, private (10/8, 172.16/12, 192.168/16, fc00::/7) and link-local addresses.
pub fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private(IpAddr::V4(v4));
            }
            v6.is_loopback()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Where to connect and what `Host` to say, for an address that has been checked to be on the local network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `http://<ip>:<port>`.
    pub url: String,
    /// `<host>:<port>` as the operator named the computer.
    pub host: String,
}

/// The base URL of a saved address (`host`, `host:port` or `http://host[:port]`): port 8790 unless it names its own.
pub fn base_url(address: &str) -> Option<String> {
    let trimmed = address.trim().trim_end_matches('/');
    let rest = trimmed.strip_prefix("http://").unwrap_or(trimmed);
    if rest.is_empty()
        || rest.contains(['/', '?', '#', ' ', '@'])
        || trimmed.starts_with("https://")
    {
        return None;
    }
    let has_port = rest.rsplit_once(':').is_some_and(|(host, port)| {
        !host.ends_with(':')
            && (!host.contains(':') || host.ends_with(']'))
            && port.parse::<u16>().is_ok()
    });
    Some(if has_port {
        format!("http://{rest}")
    } else {
        format!("http://{rest}:{DEFAULT_PROPRESENTER_LINK_PORT}")
    })
}

/// Looks the address up and refuses it unless every address it leads to is private.
pub async fn private_target(base: &str) -> Result<Target, LinkError> {
    let not_here =
        || LinkError::NotAllowed("The ProPresenter computer is not on this local network.".into());
    let url = url::Url::parse(base).map_err(|_| {
        LinkError::BadPath("The ProPresenter computer's address is not valid.".into())
    })?;
    if url.scheme() != "http" {
        return Err(LinkError::BadPath(
            "The ProPresenter computer's address is not a plain local-network address.".into(),
        ));
    }
    let port = url.port_or_known_default().unwrap_or(80);
    let host = url
        .host()
        .ok_or_else(|| LinkError::BadPath("The address has no computer in it.".into()))?;
    let (ip, label) = match host {
        url::Host::Ipv4(ip) => (IpAddr::V4(ip), ip.to_string()),
        url::Host::Ipv6(ip) => (IpAddr::V6(ip), format!("[{ip}]")),
        url::Host::Domain(name) => {
            let found: Vec<std::net::SocketAddr> = tokio::net::lookup_host((name, port))
                .await
                .map_err(|_| LinkError::NoAnswer)?
                .collect();
            // IPv4 first: the helper listens on IPv4, and `localhost` often lists `::1` before `127.0.0.1`.
            let first = found
                .iter()
                .find(|a| a.is_ipv4())
                .or(found.first())
                .ok_or(LinkError::NoAnswer)?;
            if found.iter().any(|a| !is_private(a.ip())) {
                return Err(not_here());
            }
            (first.ip(), name.to_string())
        }
    };
    if !is_private(ip) {
        return Err(not_here());
    }
    let connect = match ip {
        IpAddr::V4(v4) => format!("http://{v4}:{port}"),
        IpAddr::V6(v6) => format!("http://[{v6}]:{port}"),
    };
    Ok(Target {
        url: connect,
        host: format!("{label}:{port}"),
    })
}

/// What the helper's `/health` says. Unknown fields are ignored; a helper that is older may leave some out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Health {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub version: Option<String>,
    /// `key` (answered to our signature), `required` (the bare answer of a keyed helper) or `key_too_short`.
    #[serde(default)]
    pub auth: Option<String>,
    #[serde(default)]
    pub os: Option<String>,
    #[serde(default)]
    pub data_folder: Option<String>,
    #[serde(default)]
    pub data_folder_exists: bool,
    #[serde(default)]
    pub libraries: Vec<String>,
    #[serde(default)]
    pub playback_library_exists: bool,
    #[serde(default)]
    pub can_import: bool,
    #[serde(default)]
    pub import_blocker: Option<String>,
    #[serde(default)]
    pub propresenter_version: Option<String>,
    /// The helper can install an update that StagePilot sends (`1`).
    #[serde(default)]
    pub updater: Option<u32>,
    #[serde(default)]
    pub last_update_error: Option<String>,
}

type Clock = Box<dyn Fn() -> u64 + Send + Sync>;

/// The ProPresenter Link on another computer.
pub struct HelperHost {
    address: String,
    keys: Arc<dyn CredentialStore>,
    http: reqwest::Client,
    wall: Clock,
}

struct Reply {
    status: u16,
    body: Vec<u8>,
    sha256: Option<String>,
}

impl HelperHost {
    /// `address` is the saved ProPresenter host (`host`, `host:port` or `http://host:port`); `keys` holds the helper's key.
    pub fn new(address: &str, keys: Arc<dyn CredentialStore>) -> Self {
        // Cargo unifies reqwest features across the workspace: a TLS provider must be installed before any Client is built.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .unwrap_or_default();
        Self {
            address: address.to_string(),
            keys,
            http,
            wall: Box::new(system_seconds),
        }
    }

    /// Replaces the clock that stamps requests (tests).
    #[must_use]
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.wall = clock;
        self
    }

    fn key(&self) -> Option<String> {
        self.keys
            .get_secret()
            .ok()
            .flatten()
            .filter(|key| usable_key(key))
    }

    async fn target(&self) -> Result<Target, LinkError> {
        let base = base_url(&self.address)
            .ok_or_else(|| LinkError::BadPath("Choose the ProPresenter computer first.".into()))?;
        private_target(&base).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn request(
        &self,
        target: &Target,
        method: reqwest::Method,
        path: &str,
        body: Option<Vec<u8>>,
        extra: &[(&str, String)],
        timeout: Duration,
        cap: usize,
    ) -> Result<Reply, LinkError> {
        let mut builder = self
            .http
            .request(method.clone(), format!("{}{path}", target.url))
            .header(reqwest::header::HOST, &target.host)
            .header("accept", "application/json, application/octet-stream")
            .timeout(timeout);
        if let Some(key) = self.key() {
            let time = (self.wall)();
            builder = builder.header("X-PPL-Time", time.to_string()).header(
                "X-PPL-Sig",
                sign(&key, method.as_str(), path, time, body.as_deref()),
            );
        }
        for (name, value) in extra {
            builder = builder.header(*name, value);
        }
        if let Some(body) = body {
            builder = builder.body(body);
        }
        let mut response = builder.send().await.map_err(|_| LinkError::NoAnswer)?;
        let status = response.status().as_u16();
        let sha256 = response
            .headers()
            .get("X-Sha256")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        if response.content_length().is_some_and(|n| n > cap as u64) {
            return Err(LinkError::Unreadable);
        }
        let mut bytes: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| LinkError::NoAnswer)? {
            if bytes.len() + chunk.len() > cap {
                return Err(LinkError::Unreadable);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Reply {
            status,
            body: bytes,
            sha256,
        })
    }

    /// The JSON of a 200, or the error the helper's answer stands for.
    fn json_of(reply: &Reply) -> Result<Value, LinkError> {
        if reply.status == 200 {
            return serde_json::from_slice(&reply.body).map_err(|_| LinkError::Unreadable);
        }
        let value: Value = serde_json::from_slice(&reply.body).unwrap_or(Value::Null);
        Err(LinkError::from_wire(
            reply.status,
            value.get("code").and_then(Value::as_str).unwrap_or(""),
            value.get("error").and_then(Value::as_str).unwrap_or(""),
        ))
    }

    async fn get_json(
        &self,
        path: &str,
        timeout: Duration,
        cap: usize,
    ) -> Result<Value, LinkError> {
        let target = self.target().await?;
        let reply = self
            .request(&target, reqwest::Method::GET, path, None, &[], timeout, cap)
            .await?;
        Self::json_of(&reply)
    }

    async fn post_json(
        &self,
        path: &str,
        body: &Value,
        timeout: Duration,
    ) -> Result<Value, LinkError> {
        let target = self.target().await?;
        let bytes = serde_json::to_vec(body).unwrap_or_default();
        let reply = self
            .request(
                &target,
                reqwest::Method::POST,
                path,
                Some(bytes),
                &[("content-type", "application/json".to_string())],
                timeout,
                SMALL_CAP,
            )
            .await?;
        Self::json_of(&reply)
    }

    /// One `/health` now, to the address that was checked (`url` and `host` come from a caller that has checked it): the
    /// answer parsed, or why there was none.
    pub async fn health_at(&self, url: &str, host: &str) -> Result<Health, LinkError> {
        let target = Target {
            url: url.to_string(),
            host: host.to_string(),
        };
        let reply = self
            .request(
                &target,
                reqwest::Method::GET,
                "/health",
                None,
                &[],
                SMALL,
                SMALL_CAP,
            )
            .await?;
        let value = Self::json_of(&reply)?;
        serde_json::from_value(value).map_err(|_| LinkError::Unreadable)
    }

    /// `/health` of the saved computer.
    pub async fn health(&self) -> Result<Health, LinkError> {
        let target = self.target().await?;
        self.health_at(&target.url, &target.host).await
    }

    /// `POST /update` to a helper that can update itself: the body is signed with the key, and its SHA-256 is part of the
    /// signature. Without a usable key nothing is sent. Returns the helper's status and its JSON answer (at most 64 KiB).
    pub async fn post_update_at(
        &self,
        url: &str,
        host: &str,
        body: Vec<u8>,
    ) -> Result<(u16, Value), LinkError> {
        if self.key().is_none() {
            return Err(LinkError::NoAnswer);
        }
        let target = Target {
            url: url.to_string(),
            host: host.to_string(),
        };
        let reply = self
            .request(
                &target,
                reqwest::Method::POST,
                "/update",
                Some(body),
                &[("content-type", "application/json".to_string())],
                Duration::from_secs(20),
                SMALL_CAP,
            )
            .await?;
        Ok((
            reply.status,
            serde_json::from_slice(&reply.body).unwrap_or(Value::Null),
        ))
    }
}

fn health_to_info(health: Health) -> Result<HostInfo, LinkError> {
    if health.auth.as_deref() != Some("key") {
        return Err(LinkError::Refused(401));
    }
    Ok(HostInfo {
        version: health.version.unwrap_or_default(),
        propresenter_version: health.propresenter_version,
        os: health.os.unwrap_or_default(),
        data_folder: health.data_folder.unwrap_or_default(),
        data_folder_exists: health.data_folder_exists,
        libraries: health.libraries,
        can_import: health.can_import,
        import_blocker: health.import_blocker,
    })
}

#[async_trait]
impl ProPresenterHost for HelperHost {
    async fn info(&self) -> Result<HostInfo, LinkError> {
        health_to_info(self.health().await?)
    }

    async fn list_library(&self, name: &str) -> Result<Vec<LibraryFile>, LinkError> {
        let path = format!("/libraries/{}/files", encode_segment(library_name(name)?));
        let value = self.get_json(&path, SMALL, LIST_CAP).await?;
        serde_json::from_value(value.get("files").cloned().unwrap_or(Value::Null))
            .map_err(|_| LinkError::Unreadable)
    }

    async fn read_file(&self, library: &str, file: &str) -> Result<Vec<u8>, LinkError> {
        let path = format!(
            "/libraries/{}/files/{}",
            encode_segment(library_name(library)?),
            encode_segment(song_file_name(file)?)
        );
        let target = self.target().await?;
        let reply = self
            .request(
                &target,
                reqwest::Method::GET,
                &path,
                None,
                &[],
                FILE_TIME,
                MAX_FILE_BYTES,
            )
            .await?;
        if reply.status != 200 {
            return Self::json_of(&reply).map(|_| vec![]);
        }
        // The helper says what it sent; the bytes must match.
        if let Some(said) = &reply.sha256 {
            if *said != hex::encode(Sha256::digest(&reply.body)) {
                return Err(LinkError::HashMismatch);
            }
        }
        Ok(reply.body)
    }

    async fn ensure_library(&self, name: &str) -> Result<bool, LinkError> {
        let path = format!("/libraries/{}", encode_segment(ensure_name(name)?));
        let target = self.target().await?;
        let reply = self
            .request(
                &target,
                reqwest::Method::POST,
                &path,
                Some(b"{}".to_vec()),
                &[("content-type", "application/json".to_string())],
                SMALL,
                SMALL_CAP,
            )
            .await?;
        let value = Self::json_of(&reply)?;
        value
            .get("created")
            .and_then(Value::as_bool)
            .ok_or(LinkError::Unreadable)
    }

    async fn create_file(
        &self,
        library: &str,
        file: &str,
        bytes: &[u8],
        sha256: &str,
    ) -> Result<(), LinkError> {
        if library != PLAYBACK_LIBRARY {
            return Err(LinkError::NotAllowed(format!(
                "Files can only be added to the library named {PLAYBACK_LIBRARY}."
            )));
        }
        let file = song_file_name(file)?;
        let sha256 = sha256_hex(sha256)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(LinkError::TooLarge);
        }
        let path = format!(
            "/libraries/{PLAYBACK_LIBRARY}/files/{}",
            encode_segment(file)
        );
        let target = self.target().await?;
        let reply = self
            .request(
                &target,
                reqwest::Method::PUT,
                &path,
                Some(bytes.to_vec()),
                &[
                    ("x-sha256", sha256.to_string()),
                    ("content-type", "application/octet-stream".to_string()),
                ],
                FILE_TIME,
                SMALL_CAP,
            )
            .await?;
        Self::json_of(&reply).map(|_| ())
    }

    async fn song_index(&self) -> Result<Vec<IndexEntry>, LinkError> {
        let value = self.get_json("/index", INDEX_TIME, INDEX_CAP).await?;
        serde_json::from_value(value.get("songs").cloned().unwrap_or(Value::Null))
            .map_err(|_| LinkError::Unreadable)
    }

    async fn search_import(
        &self,
        multitracks_id: &str,
        name: &str,
        playlist: Option<String>,
    ) -> Result<String, LinkError> {
        let id = song_number(multitracks_id)?;
        let name = import_name(name)?;
        let playlist = playlist.unwrap_or_default();
        playlist_name(&playlist)?;
        let value = self
            .post_json(
                "/import",
                &json!({"id": id, "name": name, "playlist": playlist}),
                IMPORT_TIME,
            )
            .await?;
        value
            .get("file")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or(LinkError::Unreadable)
    }

    async fn select_library(&self, name: &str) -> Result<SelectOutcome, LinkError> {
        let value = self
            .post_json(
                "/select-library",
                &json!({"name": library_name(name)?}),
                FILE_TIME,
            )
            .await?;
        serde_json::from_value(value).map_err(|_| LinkError::Unreadable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signature_is_the_hmac_of_method_target_time_and_for_a_body_its_hash() {
        // RFC 4231 test case 2.
        assert_eq!(
            hmac_sha256_hex(b"Jefe", b"what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        let key = "k3y_for-the-helper-TEST-0123456789ABCDEFG";
        assert_eq!(
            sign(key, "GET", "/health", 1000, None),
            hmac_sha256_hex(key.as_bytes(), b"GET\n/health\n1000")
        );
        let digest = hex::encode(Sha256::digest(b"abc"));
        assert_eq!(
            sign(key, "PUT", "/x", 7, Some(b"abc")),
            hmac_sha256_hex(key.as_bytes(), format!("PUT\n/x\n7\n{digest}").as_bytes())
        );
        assert_ne!(
            sign(key, "POST", "/x", 7, Some(b"")),
            sign(key, "POST", "/x", 7, None),
            "an empty body is still signed"
        );
    }

    #[test]
    fn a_path_is_encoded_so_that_what_is_signed_is_what_is_sent() {
        assert_eq!(encode_segment("Song (1).pro"), "Song%20%281%29.pro");
        assert_eq!(encode_segment("Plain-name_1.pro"), "Plain-name_1.pro");
        assert_eq!(encode_segment("é"), "%C3%A9");
        let parsed = url::Url::parse(&format!(
            "http://h:1/libraries/x/files/{}",
            encode_segment("a b(1).pro")
        ))
        .unwrap();
        assert_eq!(parsed.path(), "/libraries/x/files/a%20b%281%29.pro");
    }

    #[test]
    fn only_a_local_network_address_is_private() {
        for ok in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.9",
            "192.168.4.33",
            "169.254.1.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.1.5",
        ] {
            assert!(is_private(ok.parse().unwrap()), "{ok}");
        }
        for no in [
            "8.8.8.8",
            "203.0.113.9",
            "172.32.0.1",
            "2001:db8::1",
            "::ffff:8.8.8.8",
        ] {
            assert!(!is_private(no.parse().unwrap()), "{no}");
        }
    }

    #[test]
    fn a_saved_address_gets_the_helpers_port() {
        assert_eq!(
            base_url("192.168.4.40").as_deref(),
            Some("http://192.168.4.40:8790")
        );
        assert_eq!(
            base_url("http://pp.local").as_deref(),
            Some("http://pp.local:8790")
        );
        assert_eq!(
            base_url("192.168.4.40:9000").as_deref(),
            Some("http://192.168.4.40:9000")
        );
        assert_eq!(
            base_url("[fd00::5]").as_deref(),
            Some("http://[fd00::5]:8790")
        );
        assert_eq!(
            base_url("[fd00::5]:8791").as_deref(),
            Some("http://[fd00::5]:8791")
        );
        for bad in ["", "  ", "https://x", "a/b", "a b", "u@h"] {
            assert_eq!(base_url(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn only_a_key_the_helper_would_accept_is_used() {
        assert!(usable_key(&"a".repeat(32)));
        assert!(!usable_key(&"a".repeat(31)));
        assert!(!usable_key(&format!("{} b", "a".repeat(40))));
    }

    #[tokio::test]
    async fn an_address_that_is_not_private_is_never_asked() {
        assert_eq!(
            private_target("http://203.0.113.9:8790").await,
            Err(LinkError::NotAllowed(
                "The ProPresenter computer is not on this local network.".into()
            ))
        );
        let ok = private_target("http://192.168.4.40:8790").await.unwrap();
        assert_eq!(ok.url, "http://192.168.4.40:8790");
        assert_eq!(ok.host, "192.168.4.40:8790");
    }
}
