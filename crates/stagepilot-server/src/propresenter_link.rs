//! The ProPresenter Link on the StagePilot side: how the files and the screen of the ProPresenter computer are reached, and
//! whether that works. The same shape as the Playback Library Link (`playback_library.rs`), without any of its song data.
//!
//! * **Which connection.** When a ProPresenter host is saved (the ProPresenter output is chosen), the host decides: a loopback
//!   address, or one of this computer's own addresses, means StagePilot runs on the ProPresenter computer, so the files are used
//!   directly ([`LinkMode::Local`], no helper, no key, no setup). Any other computer is reached through the ProPresenter Link
//!   helper ([`LinkMode::Helper`], port 8790, signed requests). There is no switch. While no ProPresenter host is saved, nothing is
//!   asked, ever.
//! * **The key.** Made once, the first time a setup session is opened (`ensure_key`), kept in the credential store under its own
//!   name, never returned, logged or saved in `settings.json`, and never made for a Local connection.
//! * **Probing.** The helper is asked (`GET /health`, signed) when a window asks for the status, at most once per 30 s however many
//!   ask (the Check button may ask again after 2 s; a helper that refused us is not pressed for 10 s), plus a look the runtime makes
//!   by itself when it starts if a key exists: after 2 s, 5 s, 15 s and then every 30 s until the helper answered once. A computer
//!   that never had a setup (no key) is never asked in the background. One ask is 3 s at most. The last good answer is shown, marked
//!   old, for 60 s while the helper is silent.
//! * **Nothing here changes ProPresenter.** The calls T2 needs (`connect`) are read-only apart from the rules of
//!   `stagepilot_propresenter::link`.

use crate::runtime::Runtime;
use crate::validation::{ok, Json};
use axum::extract::{Query, State};
use axum::routing::get;
use axum::Router;
use serde_json::Value;
use stagepilot_core::credential_guard::GuardedCredentialStore;
use stagepilot_core::settings::{CredentialStore, MemoryCredentialStore, SettingsService};
use stagepilot_propresenter::helper_assets::{is_older_than_bundled, BUNDLED_HELPER_VERSION};
use stagepilot_propresenter::link::helper::{base_url, usable_key, Health, HelperHost};
use stagepilot_propresenter::link::local::LocalHost;
use stagepilot_propresenter::link::status::{state_words, LinkMode, LinkState, LinkStatus};
use stagepilot_propresenter::link::{HostInfo, LinkError, ProPresenterHost};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The least time between two asks from windows that only look.
pub const MIN_REFRESH: Duration = Duration::from_secs(30);
/// The least time between two asks when the operator presses Check.
pub const MIN_CHECK: Duration = Duration::from_secs(2);
/// A helper that refused our signature is asked again no sooner than this.
pub const MIN_RETRY_AFTER_REFUSAL: Duration = Duration::from_secs(10);
/// How long the last good answer is shown, marked old, while the helper is silent.
pub const KEEP_GOOD: Duration = Duration::from_secs(60);
/// Shown when no ProPresenter computer is chosen.
pub const NO_COMPUTER: &str = "Choose the ProPresenter computer first.";

fn new_key() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| "The system could not make a random key.".to_string())?;
    // base64url without padding: A-Za-z0-9-_
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(43);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
        }
    }
    Ok(out)
}

/// Settings that live in memory only (tests, demo).
#[doc(hidden)]
pub fn memory_settings() -> Arc<SettingsService> {
    Arc::new(SettingsService::new(
        Box::new(stagepilot_core::settings::MemorySettingsStore::default()),
        Box::new(MemoryCredentialStore::default()),
        std::collections::BTreeMap::new(),
    ))
}

/// The computer the saved host names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// No ProPresenter host is saved: nothing is asked.
    NoHost,
    /// This computer.
    Local,
    /// Another computer, as typed (`192.168.4.40`, `pp.local`).
    Helper(String),
}

/// Whether `ip` is an address of this computer: loopback, or an address the computer holds (it can be bound).
pub fn is_own_address(ip: IpAddr) -> bool {
    if ip.is_loopback() {
        return true;
    }
    if ip.is_unspecified() || ip.is_multicast() {
        return false;
    }
    std::net::UdpSocket::bind((ip, 0)).is_ok()
}

/// The host part of what is saved (`host`, `host:port`, `http://host:port/`).
pub fn host_part(saved: &str) -> Option<String> {
    let trimmed = saved.trim();
    let rest = trimmed.strip_prefix("http://").unwrap_or(trimmed);
    let rest = rest.split(['/', '?', '#']).next().unwrap_or("");
    if rest.is_empty() || rest.contains(['@', ' ']) {
        return None;
    }
    let host = if let Some(inner) = rest.strip_prefix('[') {
        format!("[{}]", inner.split(']').next()?)
    } else if rest.matches(':').count() == 1 {
        rest.split(':').next()?.to_string()
    } else {
        rest.to_string()
    };
    (!host.is_empty()).then_some(host)
}

/// `true` when the host names this computer: an address that is its own, `localhost`, or a name that leads to one.
pub async fn names_this_computer(host: &str, own: &(dyn Fn(IpAddr) -> bool + Send + Sync)) -> bool {
    let bare = host.trim_matches(['[', ']']);
    if bare.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Ok(ip) = bare.parse::<IpAddr>() {
        return own(ip);
    }
    match tokio::net::lookup_host((bare, 0)).await {
        Ok(found) => found.map(|a| a.ip()).any(own),
        Err(_) => false,
    }
}

/// What one look found.
#[derive(Debug, Clone)]
struct Seen {
    /// `key` when the helper answered to our signature; `None` for a local look.
    auth: Option<String>,
    version: Option<String>,
    updater: bool,
    last_update_error: Option<String>,
    info: HostInfo,
}

impl Seen {
    fn from_health(health: Health) -> Self {
        let info = HostInfo {
            version: health.version.clone().unwrap_or_default(),
            propresenter_version: health.propresenter_version,
            os: health.os.unwrap_or_default(),
            data_folder: health.data_folder.unwrap_or_default(),
            data_folder_exists: health.data_folder_exists,
            libraries: health.libraries,
            can_import: health.can_import,
            import_blocker: health.import_blocker,
        };
        Self {
            auth: health.auth,
            version: health.version,
            updater: health.updater.is_some_and(|level| level >= 1),
            last_update_error: health.last_update_error,
            info,
        }
    }

    fn from_info(info: HostInfo) -> Self {
        Self {
            auth: None,
            version: Some(info.version.clone()),
            updater: false,
            last_update_error: None,
            info,
        }
    }
}

#[derive(Default)]
struct Slot {
    /// Whom the slot belongs to; it is dropped when the saved computer (or folder) changes.
    base: String,
    asked: Option<Instant>,
    last: Option<Result<Seen, LinkError>>,
    good: Option<(Instant, Seen)>,
}

impl Slot {
    fn due(&self, gap: Duration, now: Instant) -> bool {
        self.asked
            .is_none_or(|at| now.saturating_duration_since(at) >= gap)
    }
}

type OwnCheck = Arc<dyn Fn(IpAddr) -> bool + Send + Sync>;

pub struct ProPresenterLink {
    settings: Arc<SettingsService>,
    /// The key StagePilot made for the helper. Guarded: a keychain that waits on a dialog never holds a request up.
    keys: Arc<dyn CredentialStore>,
    /// Two setup sessions opened at once must not make two keys.
    key_gate: Mutex<()>,
    slot: Mutex<Slot>,
    /// Only one look at a time; a window that asks meanwhile reads what is held.
    asking: tokio::sync::Mutex<()>,
    own: Mutex<OwnCheck>,
    /// The helper's port (8790; tests use another).
    helper_port: AtomicU16,
    /// Added to the clock (tests move it to pass minutes at once; it is 0 in the product).
    skew_ms: AtomicU64,
    local: Mutex<Option<(Option<String>, Arc<LocalHost>)>>,
    /// A host that stands in for the real one (tests).
    stand_in: Mutex<Option<Arc<dyn ProPresenterHost>>>,
}

impl ProPresenterLink {
    /// A link whose key is held in memory only (tests, demo): the product uses the credential store.
    pub fn new(settings: Arc<SettingsService>) -> Self {
        Self::with_credentials(settings, Arc::new(MemoryCredentialStore::default()))
    }

    /// A link whose key lives in `credentials` (the OS keychain in the product).
    pub fn with_credentials(
        settings: Arc<SettingsService>,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            settings,
            keys: Arc::new(GuardedCredentialStore::new(credentials)),
            key_gate: Mutex::new(()),
            slot: Mutex::new(Slot::default()),
            asking: tokio::sync::Mutex::new(()),
            own: Mutex::new(Arc::new(is_own_address)),
            helper_port: AtomicU16::new(stagepilot_core::config::DEFAULT_PROPRESENTER_LINK_PORT),
            skew_ms: AtomicU64::new(0),
            local: Mutex::new(None),
            stand_in: Mutex::new(None),
        }
    }

    /// Replaces the question "is this address one of this computer's" (tests).
    #[must_use]
    pub fn with_own_address_check(self, own: OwnCheck) -> Self {
        self.set_own_address_check(own);
        self
    }

    /// The same on a link that is already shared (tests).
    #[doc(hidden)]
    pub fn set_own_address_check(&self, own: OwnCheck) {
        *self.own.lock().unwrap_or_else(|p| p.into_inner()) = own;
    }

    /// Asks the helper at another port than 8790 (tests).
    #[doc(hidden)]
    pub fn set_helper_port(&self, port: u16) {
        self.helper_port.store(port, Ordering::SeqCst);
    }

    /// Moves the clock forward (tests).
    #[doc(hidden)]
    pub fn advance_clock(&self, by: Duration) {
        self.skew_ms
            .fetch_add(by.as_millis() as u64, Ordering::SeqCst);
    }

    fn now(&self) -> Instant {
        Instant::now() + Duration::from_millis(self.skew_ms.load(Ordering::SeqCst))
    }

    /// The port the helper is asked at.
    pub fn helper_port(&self) -> u16 {
        self.helper_port.load(Ordering::SeqCst)
    }

    /// `host:port` of the helper for a saved host.
    fn helper_address(&self, host: &str) -> String {
        let host = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        format!("{host}:{}", self.helper_port())
    }

    /// Makes `connect` answer with this host in Local mode (tests).
    #[doc(hidden)]
    pub fn set_local_host_for_test(&self, host: Arc<dyn ProPresenterHost>) {
        *self.stand_in.lock().unwrap_or_else(|p| p.into_inner()) = Some(host);
    }

    // ---- the key -------------------------------------------------------------------------------------------------------

    fn key(&self) -> Option<String> {
        self.keys
            .get_secret()
            .ok()
            .flatten()
            .filter(|key| usable_key(key))
    }

    /// A key exists: StagePilot made one (a setup session was opened).
    pub fn is_paired(&self) -> bool {
        self.key().is_some()
    }

    /// Makes the key when there is none (the first setup session does this). `true` when it made one.
    pub fn ensure_key(&self) -> Result<bool, String> {
        let _one_at_a_time = self.key_gate.lock().unwrap_or_else(|p| p.into_inner());
        if self.key().is_some() {
            return Ok(false);
        }
        let key = new_key()?;
        self.keys.set_secret(&key).map_err(|_| {
            "StagePilot could not save the key in the secure credential store.".to_string()
        })?;
        Ok(true)
    }

    /// The key for the installer StagePilot serves to the ProPresenter computer, and for nothing else.
    pub(crate) fn key_for_installer(&self) -> Option<String> {
        self.key()
    }

    /// Takes the key away (tests only).
    #[doc(hidden)]
    pub fn clear_key_for_test(&self) {
        let _ = self.keys.remove_secret();
    }

    /// Replaces the key (tests only).
    #[doc(hidden)]
    pub fn set_key_for_test(&self, key: &str) {
        let _ = self.keys.set_secret(key);
    }

    // ---- which computer --------------------------------------------------------------------------------------------------

    /// The saved ProPresenter host while the ProPresenter output is in use; `None` when none is saved.
    pub fn saved_host(&self) -> Option<String> {
        let saved = self.settings.effective_snapshot().propresenter;
        (saved.enabled).then(|| host_part(&saved.host)).flatten()
    }

    /// The computer the saved host names.
    pub async fn choose(&self) -> Choice {
        let Some(host) = self.saved_host() else {
            return Choice::NoHost;
        };
        let own = self.own.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if names_this_computer(&host, own.as_ref()).await {
            Choice::Local
        } else {
            Choice::Helper(host)
        }
    }

    fn data_folder(&self) -> Option<String> {
        self.settings.snapshot().propresenter.link.data_folder
    }

    fn local_host(&self) -> Arc<dyn ProPresenterHost> {
        if let Some(host) = self
            .stand_in
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            return host;
        }
        let folder = self.data_folder();
        let mut held = self.local.lock().unwrap_or_else(|p| p.into_inner());
        match held.as_ref() {
            Some((for_folder, host)) if *for_folder == folder => host.clone(),
            _ => {
                let host = Arc::new(LocalHost::new(folder.as_deref()));
                *held = Some((folder, host.clone()));
                host
            }
        }
    }

    /// The host to do things with: this computer's own files, or the helper. `None` while no ProPresenter host is saved.
    pub async fn connect(&self) -> Option<(LinkMode, Arc<dyn ProPresenterHost>)> {
        match self.choose().await {
            Choice::NoHost => None,
            Choice::Local => Some((LinkMode::Local, self.local_host())),
            Choice::Helper(host) => Some((
                LinkMode::Helper,
                Arc::new(HelperHost::new(
                    &self.helper_address(&host),
                    self.keys.clone(),
                )),
            )),
        }
    }

    /// The helper's base URL (`http://host:8790`) when the saved computer is another one.
    pub async fn helper_base(&self) -> Option<String> {
        match self.choose().await {
            Choice::Helper(host) => base_url(&self.helper_address(&host)),
            _ => None,
        }
    }

    // ---- the status ------------------------------------------------------------------------------------------------------

    fn slot(&self) -> std::sync::MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Drops every answer held (a new key, or another computer).
    pub fn reset(&self) {
        *self.slot() = Slot::default();
    }

    /// Whether the last ask of `/health` was answered: `Some(true)` it was (also with a refusal), `Some(false)` nothing was
    /// there, `None` nothing was asked yet. Asks nothing; the automatic update uses it to notice that a silent helper is back.
    pub fn last_health_reachable(&self) -> Option<bool> {
        match &self.slot().last {
            Some(Ok(_)) => Some(true),
            Some(Err(LinkError::NoAnswer)) => Some(false),
            Some(Err(_)) => Some(true),
            None => None,
        }
    }

    /// One signed `GET /health` now, whatever the throttle says, to an address the caller has checked (the automatic update).
    pub(crate) async fn fresh_health(&self, url: &str, host: &str) -> Result<Health, LinkError> {
        HelperHost::new(url, self.keys.clone())
            .health_at(url, host)
            .await
    }

    /// `POST /update` to a helper that can update itself (the automatic update).
    pub(crate) async fn post_update(
        &self,
        url: &str,
        host: &str,
        body: Vec<u8>,
    ) -> Result<(u16, Value), LinkError> {
        HelperHost::new(url, self.keys.clone())
            .post_update_at(url, host, body)
            .await
    }

    async fn look(&self, choice: &Choice) -> Result<Seen, LinkError> {
        match choice {
            Choice::NoHost => Err(LinkError::BadPath(NO_COMPUTER.to_string())),
            Choice::Local => self.local_host().info().await.map(Seen::from_info),
            Choice::Helper(host) => {
                let host = HelperHost::new(&self.helper_address(host), self.keys.clone());
                host.health().await.map(Seen::from_health)
            }
        }
    }

    /// Where the computer stands. `check` is the Check button: it may ask again 2 s after the last ask instead of 30 s.
    pub async fn status(&self, check: bool) -> LinkStatus {
        let choice = self.choose().await;
        let (mode, whom) = match &choice {
            Choice::NoHost => return self.render(None, None, &choice),
            Choice::Local => (
                LinkMode::Local,
                format!("local:{}", self.data_folder().unwrap_or_default()),
            ),
            Choice::Helper(host) => (LinkMode::Helper, format!("helper:{host}")),
        };
        let gap = {
            let mut slot = self.slot();
            if slot.base != whom {
                *slot = Slot {
                    base: whom,
                    ..Slot::default()
                };
            }
            let mut gap = if check { MIN_CHECK } else { MIN_REFRESH };
            if matches!(slot.last, Some(Err(LinkError::Refused(_)))) {
                gap = gap.max(MIN_RETRY_AFTER_REFUSAL);
            }
            gap
        };
        let due = self.slot().due(gap, self.now());
        if due {
            if let Ok(_one) = self.asking.try_lock() {
                // Asked and recorded, whatever the answer: a silent computer is not asked again and again.
                let answer = self.look(&choice).await;
                let now = self.now();
                let mut slot = self.slot();
                slot.asked = Some(now);
                if let Ok(seen) = &answer {
                    slot.good = Some((now, seen.clone()));
                }
                slot.last = Some(answer);
            }
        }
        let now = self.now();
        let (last, good) = {
            let slot = self.slot();
            (
                slot.last.clone(),
                slot.good
                    .as_ref()
                    .filter(|(at, _)| now.saturating_duration_since(*at) < KEEP_GOOD)
                    .map(|(_, seen)| seen.clone()),
            )
        };
        let shown = match last {
            None => None,
            Some(Ok(seen)) => Some((Ok(seen), false)),
            Some(Err(error)) => match good {
                Some(seen) => Some((Ok(seen), true)),
                None => Some((Err(error), false)),
            },
        };
        self.render(Some(mode), shown, &choice)
    }

    fn render(
        &self,
        mode: Option<LinkMode>,
        shown: Option<(Result<Seen, LinkError>, bool)>,
        choice: &Choice,
    ) -> LinkStatus {
        let paired = mode == Some(LinkMode::Helper) && self.is_paired();
        let computer_address = match choice {
            Choice::Helper(host) => Some(host.clone()),
            _ => None,
        };
        let mut status = LinkStatus {
            mode,
            state: LinkState::NotInstalled,
            message: String::new(),
            version: None,
            bundled_version: BUNDLED_HELPER_VERSION.to_string(),
            can_import: false,
            import_blocker: None,
            libraries: vec![],
            playback_library_exists: false,
            computer_address,
            data_folder: None,
            paired,
            updater: false,
            stale: false,
            last_update_error: None,
            update: None,
        };
        let mut message: Option<String> = None;
        match (mode, shown) {
            (None, _) => {}
            (Some(_), None) => status.state = LinkState::Checking,
            (Some(mode), Some((Ok(seen), stale))) => {
                status.stale = stale;
                status.data_folder = Some(seen.info.data_folder.clone()).filter(|f| !f.is_empty());
                status.can_import = seen.info.can_import;
                status.import_blocker = seen.info.import_blocker.clone();
                status.libraries = seen.info.libraries.clone();
                status.playback_library_exists = seen
                    .info
                    .libraries
                    .iter()
                    .any(|l| l == stagepilot_propresenter::link::PLAYBACK_LIBRARY);
                status.updater = seen.updater;
                status.last_update_error = seen.last_update_error.clone();
                status.version = seen.version.clone();
                status.state = match mode {
                    LinkMode::Local if seen.info.data_folder_exists => LinkState::Ready,
                    LinkMode::Local => LinkState::NotAnswering,
                    LinkMode::Helper if seen.auth.as_deref() != Some("key") => {
                        // The bare answer of a helper that does not know our key: it is there, and it will not talk to us.
                        status.can_import = false;
                        status.libraries.clear();
                        status.playback_library_exists = false;
                        status.data_folder = None;
                        LinkState::NeedsCommand
                    }
                    LinkMode::Helper if is_older_than_bundled(seen.version.as_deref()) => {
                        LinkState::Outdated
                    }
                    LinkMode::Helper => LinkState::Ready,
                };
                if status.stale {
                    message = Some(format!(
                        "The ProPresenter computer did not answer just now; this is what it said a moment ago. {}",
                        state_words(Some(mode), status.state, status.version.as_deref())
                    ));
                }
            }
            (Some(_), Some((Err(error), _))) => {
                status.state = match &error {
                    LinkError::Refused(_) => LinkState::NeedsCommand,
                    LinkError::BadPath(_) => LinkState::NotInstalled,
                    _ if mode == Some(LinkMode::Local) => LinkState::NotAnswering,
                    _ if paired => LinkState::NotAnswering,
                    _ => LinkState::NotInstalled,
                };
                if matches!(error, LinkError::NotAllowed(_)) {
                    message = Some(error.words());
                }
            }
        }
        status.message = message
            .unwrap_or_else(|| state_words(status.mode, status.state, status.version.as_deref()));
        status
    }
}

/// The look the runtime makes when it starts, with no window open: only when a key exists (a setup was once done) and the saved
/// computer is another one. While the helper is silent it is tried again after 2 s, 5 s, 15 s and then every 30 s, until it has
/// answered once. One task, never awaited by start-up.
pub fn connect_at_start(rt: std::sync::Weak<Runtime>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tries = 0usize;
        loop {
            {
                let Some(rt) = rt.upgrade() else { return };
                let link = rt.propresenter_link.clone();
                if !link.is_paired() {
                    return;
                }
                // Local needs no helper and nothing is asked while no computer is chosen.
                if !matches!(link.choose().await, Choice::Helper(_)) {
                    return;
                }
                let status = link.status(true).await;
                if matches!(
                    status.state,
                    LinkState::Ready | LinkState::Outdated | LinkState::NeedsCommand
                ) {
                    return;
                }
            }
            tries += 1;
            tokio::time::sleep(start_wait(tries)).await;
        }
    })
}

/// The wait before try number `tries` (1-based): 2 s, 5 s, 15 s, then 30 s.
pub fn start_wait(tries: usize) -> Duration {
    Duration::from_secs(match tries {
        0 | 1 => 2,
        2 => 5,
        3 => 15,
        _ => 30,
    })
}

type Rt = Arc<Runtime>;

#[derive(serde::Deserialize, Default)]
struct Ask {
    /// The Check button: ask again after 2 s instead of 30 s.
    check: Option<String>,
}

impl Ask {
    fn check(&self) -> bool {
        matches!(self.check.as_deref(), Some("1" | "true"))
    }
}

/// `GET /api/v1/propresenter-link/status[?check=1]`.
async fn status(State(rt): State<Rt>, Query(ask): Query<Ask>) -> Json {
    let mut status = rt.propresenter_link.status(ask.check()).await;
    status.update = rt.helper_updater.propresenter_view(&status);
    ok(serde_json::to_value(&status).unwrap_or(Value::Null))
}

pub fn routes() -> Router<Rt> {
    Router::new().route("/api/v1/propresenter-link/status", get(status))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_key_is_43_url_safe_characters_and_never_the_same_twice() {
        let a = new_key().unwrap();
        let b = new_key().unwrap();
        assert_eq!(a.len(), 43);
        assert!(a
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
        assert_ne!(a, b);
        assert!(usable_key(&a));
    }

    #[test]
    fn the_host_part_of_what_is_saved() {
        assert_eq!(host_part("192.168.4.40").as_deref(), Some("192.168.4.40"));
        assert_eq!(host_part(" pp.local ").as_deref(), Some("pp.local"));
        assert_eq!(
            host_part("http://192.168.4.40:1025/").as_deref(),
            Some("192.168.4.40")
        );
        assert_eq!(host_part("[fd00::5]:1025").as_deref(), Some("[fd00::5]"));
        assert_eq!(host_part("fd00::5").as_deref(), Some("fd00::5"));
        for bad in ["", "  ", "a b", "u@h", "http://"] {
            assert_eq!(host_part(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn waits_at_start_are_two_five_fifteen_then_thirty_seconds() {
        let waits: Vec<u64> = (1..=6).map(|n| start_wait(n).as_secs()).collect();
        assert_eq!(waits, [2, 5, 15, 30, 30, 30]);
    }
}
