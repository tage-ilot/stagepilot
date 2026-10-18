//! Private file contracts (`remote_files.py`): desired state, configs, atomic writes.

use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::error::{RemoteError, Res};

pub const MAX_STATE_BYTES: u64 = 4096;

fn hostname_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)+$",
        )
        .expect("hostname regex")
    })
}

fn identifier_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?:[a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})$").expect("id regex"))
}

pub fn is_hostname(value: &str) -> bool {
    value.len() <= 253 && hostname_re().is_match(value)
}

pub fn is_identifier(value: &str) -> bool {
    identifier_re().is_match(value)
}

/// `urllib.parse.urlsplit` for the fields the origin checks use.
#[derive(Debug, Default, Clone)]
pub struct SplitUrl {
    pub scheme: String,
    pub hostname: Option<String>,
    pub username: bool,
    pub password: bool,
    pub port: Option<u16>,
    pub port_invalid: bool,
    pub path: String,
    pub query: bool,
    pub fragment: bool,
}

pub fn urlsplit(raw: &str) -> SplitUrl {
    let mut out = SplitUrl::default();
    let Some((scheme, rest)) = raw.split_once("://") else {
        return out;
    };
    out.scheme = scheme.to_ascii_lowercase();
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (netloc, tail) = rest.split_at(end);
    let (before_fragment, fragment) = match tail.split_once('#') {
        Some((a, _)) => (a, true),
        None => (tail, false),
    };
    let (path, query) = match before_fragment.split_once('?') {
        Some((a, _)) => (a, true),
        None => (before_fragment, false),
    };
    out.path = path.to_owned();
    out.query = query;
    out.fragment = fragment;
    let hostport = match netloc.rsplit_once('@') {
        Some((userinfo, hostport)) => {
            match userinfo.split_once(':') {
                Some((user, _)) => {
                    out.username = !user.is_empty();
                    out.password = true;
                }
                None => out.username = !userinfo.is_empty(),
            }
            hostport
        }
        None => netloc,
    };
    let (host, port) = if let Some(stripped) = hostport.strip_prefix('[') {
        match stripped.split_once(']') {
            Some((h, p)) => (h.to_owned(), p.strip_prefix(':').map(str::to_owned)),
            None => (stripped.to_owned(), None),
        }
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_owned(), Some(p.to_owned())),
            None => (hostport.to_owned(), None),
        }
    };
    if !host.is_empty() {
        out.hostname = Some(host.to_ascii_lowercase());
    }
    if let Some(p) = port {
        match p.parse::<u32>() {
            Ok(n) if n <= 65535 && !p.is_empty() => out.port = Some(n as u16),
            _ if p.is_empty() => {}
            _ => out.port_invalid = true,
        }
    }
    out
}

/// `origin.scheme != "https" or not origin.hostname or origin.path ... `: exact HTTPS origin.
/// `allow_slash` permits a single trailing `/` path (beta control plane).
pub fn is_exact_https_origin(raw: &str, allow_slash: bool) -> bool {
    let o = urlsplit(raw);
    let path_ok = o.path.is_empty() || (allow_slash && o.path == "/");
    o.scheme == "https"
        && o.hostname.is_some()
        && path_ok
        && !o.query
        && !o.fragment
        && !o.username
        && !o.password
}

const fn default_remote_port() -> u16 {
    8766
}

const fn default_lan_port() -> u16 {
    8765
}

fn port_ok(port: u16) -> bool {
    port >= 1024
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesiredRemote {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub generation: String,
    #[serde(default)]
    pub public_origin: String,
    #[serde(default = "default_remote_port")]
    pub port: u16,
}

impl Default for DesiredRemote {
    fn default() -> Self {
        Self {
            enabled: false,
            generation: String::new(),
            public_origin: String::new(),
            port: 8766,
        }
    }
}

impl DesiredRemote {
    /// `model_validate_json`.
    pub fn from_json(text: &str) -> Res<Self> {
        let value: Self = serde_json::from_str(text)?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Res<()> {
        if !port_ok(self.port) {
            return Err(RemoteError::value("port out of range"));
        }
        if self.enabled {
            Uuid::parse_str(&self.generation).map_err(|e| RemoteError::value(e.to_string()))?;
            if !is_exact_https_origin(&self.public_origin, false) {
                return Err(RemoteError::value(
                    "Managed Remote requires an exact HTTPS origin",
                ));
            }
        }
        Ok(())
    }

    /// `model_dump_json`.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("DesiredRemote serializes")
    }

    pub fn marker(public_origin: &str, port: u16) -> Self {
        Self {
            enabled: false,
            generation: String::new(),
            public_origin: public_origin.to_owned(),
            port,
        }
    }
}

/// POSIX permission bits expose a private file to others. Windows has no such bits (privacy comes
/// from the per-user profile ACL), so this is always false there.
pub fn is_group_or_world_readable(path: &Path) -> std::io::Result<bool> {
    let metadata = std::fs::metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(metadata.permissions().mode() & 0o077 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        Ok(false)
    }
}

/// Write via a sibling temp file, fsync, atomic replace, then fsync the directory (POSIX).
pub fn atomic_write(path: &Path, text: &str) -> Res<()> {
    let parent = path
        .parent()
        .ok_or_else(|| RemoteError::io("path has no parent"))?;
    create_private_dir(parent)?;
    let mut file = tempfile::Builder::new()
        .prefix(".remote-")
        .tempfile_in(parent)?;
    file.write_all(text.as_bytes())?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|e| RemoteError::io(e.error.to_string()))?;
    #[cfg(unix)]
    {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

/// `mkdir(parents=True, exist_ok=True, mode=0o700)`.
pub fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(path)
    }
}

pub fn read_desired(path: &Path) -> DesiredRemote {
    let read = || -> Res<DesiredRemote> {
        if std::fs::metadata(path)?.len() > MAX_STATE_BYTES {
            return Ok(DesiredRemote::default());
        }
        DesiredRemote::from_json(&std::fs::read_to_string(path)?)
    };
    read().unwrap_or_default()
}

/// `safe_status`: atomically write a JSON object of the given values.
pub fn safe_status(path: &Path, values: Map<String, Value>) -> Res<()> {
    atomic_write(path, &Value::Object(values).to_string())
}

/// Build a JSON object from `key => value` pairs.
#[macro_export]
macro_rules! status_map {
    ($($key:literal => $value:expr),* $(,)?) => {{
        let mut map = serde_json::Map::new();
        $( map.insert($key.to_owned(), serde_json::json!($value)); )*
        map
    }};
}

/// `Path.resolve()` (non-strict): canonicalise the longest existing prefix, then append the rest
/// lexically.
pub fn resolve(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(real) = existing.canonicalize() {
            let mut out = real;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return normalize(&out);
        }
        match (
            existing.file_name().map(|n| n.to_owned()),
            existing.parent().map(Path::to_path_buf),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent;
            }
            _ => return normalize(path),
        }
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn is_relative_to(path: &Path, base: &Path) -> bool {
    path.starts_with(base)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlConfig {
    #[serde(default = "cloudflare")]
    pub provider: String,
    pub account_id: String,
    pub zone_id: String,
    pub hostname: String,
    pub api_token_file: PathBuf,
    pub control_dir: PathBuf,
    pub installation_dir: PathBuf,
    #[serde(default = "default_remote_port")]
    pub remote_port: u16,
    #[serde(default = "default_lan_port")]
    pub lan_port: u16,
}

fn cloudflare() -> String {
    "cloudflare".to_owned()
}

impl ControlConfig {
    pub fn from_json(text: &str) -> Res<Self> {
        let value: Self = serde_json::from_str(text)?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Res<()> {
        if self.provider != "cloudflare" {
            return Err(RemoteError::value("unsupported provider"));
        }
        if !is_identifier(&self.account_id) || !is_identifier(&self.zone_id) {
            return Err(RemoteError::value("invalid identifier"));
        }
        if !is_hostname(&self.hostname) {
            return Err(RemoteError::value("invalid hostname"));
        }
        if !port_ok(self.remote_port) || !port_ok(self.lan_port) {
            return Err(RemoteError::value("port out of range"));
        }
        for path in [
            &self.api_token_file,
            &self.control_dir,
            &self.installation_dir,
        ] {
            if !path.is_absolute() {
                return Err(RemoteError::value("Remote paths must be absolute"));
            }
        }
        if self.remote_port == self.lan_port {
            return Err(RemoteError::value("Remote and LAN ports must differ"));
        }
        let install = resolve(&self.installation_dir);
        if is_relative_to(&resolve(&self.api_token_file), &install) {
            return Err(RemoteError::value(
                "Account token must not be in the installation export",
            ));
        }
        if is_relative_to(&resolve(&self.control_dir), &install) {
            return Err(RemoteError::value(
                "Control state must not be in the installation export",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BetaControlConfig {
    pub control_plane_url: String,
    pub installation_id: String,
    pub hostname: String,
    #[serde(default)]
    pub credential_file: Option<PathBuf>,
    pub state_dir: PathBuf,
    pub installation_dir: PathBuf,
    #[serde(default = "default_remote_port")]
    pub remote_port: u16,
    #[serde(default = "default_lan_port")]
    pub lan_port: u16,
}

impl BetaControlConfig {
    pub fn from_json(text: &str) -> Res<Self> {
        let value: Self = serde_json::from_str(text)?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Res<()> {
        if !is_identifier(&self.installation_id) {
            return Err(RemoteError::value("invalid installation id"));
        }
        if !is_hostname(&self.hostname) {
            return Err(RemoteError::value("invalid hostname"));
        }
        if !port_ok(self.remote_port) || !port_ok(self.lan_port) {
            return Err(RemoteError::value("port out of range"));
        }
        if !is_exact_https_origin(&self.control_plane_url, true) {
            return Err(RemoteError::value(
                "Beta control plane requires an exact HTTPS origin",
            ));
        }
        for path in [&self.state_dir, &self.installation_dir] {
            if !path.is_absolute() {
                return Err(RemoteError::value("Remote paths must be absolute"));
            }
        }
        if let Some(credential) = &self.credential_file {
            if !credential.is_absolute() {
                return Err(RemoteError::value("Remote paths must be absolute"));
            }
        }
        let install = resolve(&self.installation_dir);
        if let Some(credential) = &self.credential_file {
            if is_relative_to(&resolve(credential), &install) {
                return Err(RemoteError::value(
                    "Provisioning credential must not be in the connector export",
                ));
            }
        }
        if is_relative_to(&resolve(&self.state_dir), &install) {
            return Err(RemoteError::value(
                "Provisioning state must not be in the connector export",
            ));
        }
        if self.remote_port == self.lan_port {
            return Err(RemoteError::value("Remote and LAN ports must differ"));
        }
        Ok(())
    }
}
