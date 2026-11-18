//! The helpers update themselves when StagePilot starts.
//!
//! StagePilot carries a copy of the Playback "Library Link", of the Lightkey helper and of the ProPresenter Link (`stagepilot-playback`,
//! `stagepilot-lights` and `stagepilot-propresenter` assets). When StagePilot starts, and again whenever a helper that was silent answers, this module looks at each helper and, when it
//! is older than the copy StagePilot carries, sends the new files to it (`POST /update`). The helper installs them only when the
//! StagePilot project signed them; the key that signs ordinary requests is not enough, so a stolen key cannot put code on the Mac.
//!
//! What has to be true before anything is sent, for each helper:
//! * `helper_auto_update` is not switched off in `settings.json`;
//! * the helper answers, and (for the Library Link) answers to our key;
//! * it says `"updater": 1` (a helper that does not is updated once by hand, with the setup command, and then updates itself);
//! * what it runs is older than the bundled version;
//! * the bundled `SHA256SUMS.sig` is a valid Ed25519 signature of the bundled `SHA256SUMS` under the project's key (checked here too:
//!   an unsigned or wrongly signed bundle never leaves);
//! * the helper's address is on the local network (loopback, private or link-local), also after a name has been looked up;
//! * it is quiet: no song is playing in Playback (whether or not the Playback API is connected), StagePilot's song timer is not running,
//!   and the Lightkey helper is not recording.
//!
//! Not quiet: try again every 2 minutes for 30 minutes after start (or after the helper came back), then stop until the next start or
//! the next time the helper is silent and answers again. After it was sent, the helper has 60 seconds to say it runs the new version.
//! An error reply, a timeout or a `last_update_error` is a failed attempt; there are at most 3 attempts per helper per bundled version
//! per run of StagePilot, and then the window shows today's setup command with one plain sentence. Nothing here is a button, and nothing
//! here changes the header's Ready / Check-system verdict.

use crate::helper_install::is_private;
use crate::playback_library::{base_url, settings_from, Failure, PlaybackLibrary};
use crate::propresenter_link::ProPresenterLink;
use async_trait::async_trait;
use serde_json::Value;
use stagepilot_core::config::PlaybackLibrarySettings;
use stagepilot_core::helper_update::{
    public_key_bytes, version_is_older, HelperUpdateMode, HelperUpdateStatus, UpdateBundle,
    HELPER_SIGNING_PUBLIC_KEY,
};
use stagepilot_core::settings::SettingsService;
use stagepilot_core::state::TimerStatus;
use stagepilot_core::store::StateStore;
use stagepilot_lights::lightkey::{LightkeyHelper, LightkeyLink, LightkeyStatus};
use stagepilot_playback::library::{helper_state_for, HelperState, HelperStatus};
use stagepilot_playback::plugin::PlaybackPlugin;
use stagepilot_propresenter::link::status::{LinkMode, LinkState, LinkStatus};
use stagepilot_propresenter::link::LinkError;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Which helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelperKind {
    /// The Playback "Library Link".
    LibraryLink,
    /// The Lightkey helper on the Lighting computer.
    Lightkey,
    /// The ProPresenter Link helper on the ProPresenter computer (when it is another computer).
    ProPresenterLink,
}

impl HelperKind {
    /// How the status line names it: "Updating the Library Link to 0.4.0…".
    fn name(self) -> &'static str {
        match self {
            Self::LibraryLink => "the Library Link",
            Self::Lightkey => "the helper",
            Self::ProPresenterLink => "the ProPresenter Link",
        }
    }

    fn computer(self) -> &'static str {
        match self {
            Self::LibraryLink => "Playback computer",
            Self::Lightkey => "Lighting computer",
            Self::ProPresenterLink => "ProPresenter computer",
        }
    }

    fn log_name(self) -> &'static str {
        match self {
            Self::LibraryLink => "library_link",
            Self::Lightkey => "lightkey_helper",
            Self::ProPresenterLink => "propresenter_link",
        }
    }
}

/// The timing of the whole thing. The product uses `Default`; tests use milliseconds.
#[derive(Debug, Clone)]
pub struct UpdaterConfig {
    /// After StagePilot started, before the first look.
    pub start_delay: Duration,
    /// How often the scheduler looks at its own notes (no network).
    pub tick: Duration,
    /// How long to wait before trying again when the helper is not quiet or does not answer.
    pub retry: Duration,
    /// How long after start (or after the helper came back) it keeps trying to find a quiet moment.
    pub window: Duration,
    /// After the bundle was sent: how long the helper has to say it runs the new version.
    pub wait_for_new_version: Duration,
    /// How often it asks the helper while it waits.
    pub poll: Duration,
    /// The wait before the second and the third attempt after a failed one.
    pub backoff: Vec<Duration>,
    /// Attempts per helper, per bundled version, per run of StagePilot.
    pub max_attempts: u32,
    /// How long "Updated to 0.4.0 automatically." stays in the status.
    pub updated_shown: Duration,
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        Self {
            start_delay: Duration::from_secs(5),
            tick: Duration::from_secs(10),
            retry: Duration::from_secs(2 * 60),
            window: Duration::from_secs(30 * 60),
            wait_for_new_version: Duration::from_secs(60),
            poll: Duration::from_secs(2),
            backoff: vec![Duration::from_secs(2 * 60), Duration::from_secs(10 * 60)],
            max_attempts: 3,
            updated_shown: Duration::from_secs(5 * 60),
        }
    }
}

/// What the updater needs to know about StagePilot and the world. The product reads the real thing; tests answer by hand.
#[async_trait]
pub trait UpdateEnvironment: Send + Sync {
    /// `helper_auto_update` in the settings file (on unless it says `false`).
    fn auto_update(&self) -> bool;
    /// Where the Library Link is asked (the saved Playback computer, or the saved override).
    fn playback_settings(&self) -> PlaybackLibrarySettings;
    /// A song is playing in Playback (the API said so; no API means nothing is playing).
    fn song_playing(&self) -> bool;
    /// StagePilot's own song timer is running.
    fn timer_running(&self) -> bool;
    /// The Lightkey link: its address, whether it is connected, what the helper said, whether a take is recorded. `None`: lights off.
    async fn lightkey(&self) -> Option<LightkeyStatus>;
}

/// The product's environment, read from the live parts of StagePilot.
pub struct LiveEnvironment {
    pub settings: Arc<SettingsService>,
    pub store: StateStore,
    pub playback: Arc<PlaybackPlugin>,
    pub outputs: Arc<crate::outputs::Outputs>,
}

#[async_trait]
impl UpdateEnvironment for LiveEnvironment {
    fn auto_update(&self) -> bool {
        self.settings.snapshot().helper_auto_update
    }

    fn playback_settings(&self) -> PlaybackLibrarySettings {
        settings_from(&self.settings)
    }

    fn song_playing(&self) -> bool {
        self.playback
            .status()
            .heartbeat
            .is_some_and(|heartbeat| heartbeat.playing)
    }

    fn timer_running(&self) -> bool {
        self.store.snapshot().timer.status == TimerStatus::Running
    }

    async fn lightkey(&self) -> Option<LightkeyStatus> {
        let status = self.outputs.lights.lightkey_status().await;
        (status.link != LightkeyLink::Off).then_some(status)
    }
}

/// Why a helper was not updated this time (and, for `Updated`, that it was).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `helper_auto_update` is off.
    Off,
    /// Nothing answered, or the helper does not take this StagePilot's key: nothing can be done.
    Unreachable,
    /// The helper is not older than the bundled version.
    Current,
    /// The helper cannot update itself: it is updated once by hand.
    NotCapable,
    /// The bundled files carry no valid signature: nothing is ever sent.
    Unsigned,
    /// Something is playing, the timer runs or a take is being recorded.
    NotQuiet(&'static str),
    /// The helper's address is not on the local network: nothing was sent.
    Refused(&'static str),
    /// A failed attempt is being waited out.
    Waiting,
    /// Three attempts for this version were made and failed.
    GaveUp,
    /// The helper now runs this version.
    Updated(String),
    /// One attempt failed (a plain reason).
    Failed(String),
}

/// A place on the local network the update may go to: the address to connect to and the `Host` to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `http://<ip>:<port>`.
    pub url: String,
    /// `<host>:<port>` as the operator named the computer.
    pub host: String,
}

/// Where an address leads, or why it must not be used: only loopback, private and link-local addresses are fine, and when a name is
/// given every address it resolves to must be one of those. The connection then goes to the address that was checked, so a name that
/// answers differently the second time cannot lead somewhere else.
pub async fn lan_target(base: &str) -> Result<Target, &'static str> {
    let url = url::Url::parse(base).map_err(|_| "The address is not valid.")?;
    if url.scheme() != "http" {
        return Err("The address is not a plain local-network address.");
    }
    let port = url.port_or_known_default().unwrap_or(80);
    let host = url.host().ok_or("The address has no computer in it.")?;
    let (ip, label) = match host {
        url::Host::Ipv4(ip) => (check_private(IpAddr::V4(ip))?, ip.to_string()),
        url::Host::Ipv6(ip) => (check_private(IpAddr::V6(ip))?, format!("[{ip}]")),
        url::Host::Domain(name) => {
            let found: Vec<std::net::SocketAddr> = tokio::net::lookup_host((name, port))
                .await
                .map_err(|_| "The computer's name could not be found.")?
                .collect();
            // IPv4 first: the helpers listen on IPv4, and `localhost` often lists `::1` before `127.0.0.1`.
            let first = found
                .iter()
                .find(|address| address.is_ipv4())
                .or(found.first())
                .ok_or("The computer's name could not be found.")?;
            for address in &found {
                check_private(address.ip())?;
            }
            (first.ip(), name.to_string())
        }
    };
    let connect = match ip {
        IpAddr::V4(v4) => format!("http://{v4}:{port}"),
        IpAddr::V6(v6) => format!("http://[{v6}]:{port}"),
    };
    Ok(Target {
        url: connect,
        host: format!("{label}:{port}"),
    })
}

fn check_private(ip: IpAddr) -> Result<IpAddr, &'static str> {
    if is_private(ip) {
        Ok(ip)
    } else {
        Err("The computer is not on this local network.")
    }
}

/// `true` when `bundle.sig` is a valid Ed25519 signature of `bundle.sums` under `key` (32 raw bytes).
pub fn signature_valid(bundle: &UpdateBundle, key: &[u8; 32]) -> bool {
    let Some(signature) = bundle.signature_bytes() else {
        return false;
    };
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
        .verify(bundle.sums, &signature)
        .is_ok()
}

#[derive(Debug, Clone, Copy)]
struct Window {
    /// No new look after this (a failed attempt may stretch it).
    until: Instant,
    /// The next look.
    try_at: Instant,
}

#[derive(Default)]
struct KindState {
    window: Option<Window>,
    /// What was last seen of whether the helper answers, without asking it (`None`: not seen yet).
    seen: Option<bool>,
    attempts: HashMap<String, u32>,
    gave_up: HashSet<String>,
    next_attempt: Option<Instant>,
    updating: Option<String>,
    updated: Option<(String, Instant)>,
}

#[derive(Default)]
struct Kinds {
    library: KindState,
    lightkey: KindState,
    propresenter: KindState,
}

impl Kinds {
    fn of(&mut self, kind: HelperKind) -> &mut KindState {
        match kind {
            HelperKind::LibraryLink => &mut self.library,
            HelperKind::Lightkey => &mut self.lightkey,
            HelperKind::ProPresenterLink => &mut self.propresenter,
        }
    }
}

/// The automatic update of both helpers.
pub struct HelperUpdater {
    env: Arc<dyn UpdateEnvironment>,
    library: Arc<PlaybackLibrary>,
    http: reqwest::Client,
    config: UpdaterConfig,
    library_bundle: UpdateBundle,
    lightkey_bundle: UpdateBundle,
    library_signed: bool,
    lightkey_signed: bool,
    /// The ProPresenter Link: asked about only when one is given (`with_propresenter_link`).
    propresenter: Option<Arc<ProPresenterLink>>,
    propresenter_bundle: UpdateBundle,
    propresenter_signed: bool,
    state: Mutex<Kinds>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl HelperUpdater {
    /// The product: the bundles inside this build, the project's key, the real timings.
    pub fn new(env: Arc<dyn UpdateEnvironment>, library: Arc<PlaybackLibrary>) -> Self {
        let key = public_key_bytes(HELPER_SIGNING_PUBLIC_KEY).unwrap_or([0; 32]);
        Self::with_parts(
            env,
            library,
            stagepilot_playback::helper_assets::update_bundle(),
            stagepilot_lights::lightkey_helper_assets::update_bundle(),
            &key,
            UpdaterConfig::default(),
        )
    }

    /// Bundles, key and timings given (tests). The product never calls this with another key than the project's.
    #[doc(hidden)]
    pub fn with_parts(
        env: Arc<dyn UpdateEnvironment>,
        library: Arc<PlaybackLibrary>,
        library_bundle: UpdateBundle,
        lightkey_bundle: UpdateBundle,
        key: &[u8; 32],
        config: UpdaterConfig,
    ) -> Self {
        let propresenter_bundle = stagepilot_propresenter::helper_assets::update_bundle();
        // The same proxy-free, redirect-free client every other helper call uses.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .unwrap_or_default();
        Self {
            env,
            library,
            http,
            config,
            library_signed: signature_valid(&library_bundle, key),
            lightkey_signed: signature_valid(&lightkey_bundle, key),
            library_bundle,
            lightkey_bundle,
            propresenter: None,
            propresenter_signed: signature_valid(&propresenter_bundle, key),
            propresenter_bundle,
            state: Mutex::new(Kinds::default()),
        }
    }

    /// Adds the ProPresenter Link, with the bundle inside this build and the project's key (the product).
    #[must_use]
    pub fn with_propresenter_link(self, link: Arc<ProPresenterLink>) -> Self {
        let key = public_key_bytes(HELPER_SIGNING_PUBLIC_KEY).unwrap_or([0; 32]);
        self.with_propresenter_parts(
            link,
            stagepilot_propresenter::helper_assets::update_bundle(),
            &key,
        )
    }

    /// The same with the bundle and the key given (tests).
    #[doc(hidden)]
    #[must_use]
    pub fn with_propresenter_parts(
        mut self,
        link: Arc<ProPresenterLink>,
        bundle: UpdateBundle,
        key: &[u8; 32],
    ) -> Self {
        self.propresenter_signed = signature_valid(&bundle, key);
        self.propresenter_bundle = bundle;
        self.propresenter = Some(link);
        self
    }

    fn bundle(&self, kind: HelperKind) -> &UpdateBundle {
        match kind {
            HelperKind::LibraryLink => &self.library_bundle,
            HelperKind::Lightkey => &self.lightkey_bundle,
            HelperKind::ProPresenterLink => &self.propresenter_bundle,
        }
    }

    fn signed(&self, kind: HelperKind) -> bool {
        match kind {
            HelperKind::LibraryLink => self.library_signed,
            HelperKind::Lightkey => self.lightkey_signed,
            HelperKind::ProPresenterLink => self.propresenter_signed,
        }
    }

    // ---------------------------------------------------------------- what the window is told

    /// What the Library Link status says about the update, for a helper in this state. `None`: nothing to say.
    pub fn playback_view(&self, helper: &HelperStatus) -> Option<HelperUpdateStatus> {
        if !matches!(helper.state, HelperState::Ready | HelperState::Outdated) {
            return None;
        }
        self.view(
            HelperKind::LibraryLink,
            helper.version.as_deref(),
            helper.updater,
        )
    }

    /// What the ProPresenter Link status says about the update, for a helper in this state. `None`: nothing to say (also for the
    /// Local connection, which has no helper).
    pub fn propresenter_view(&self, status: &LinkStatus) -> Option<HelperUpdateStatus> {
        if status.mode != Some(LinkMode::Helper)
            || !matches!(status.state, LinkState::Ready | LinkState::Outdated)
        {
            return None;
        }
        self.view(
            HelperKind::ProPresenterLink,
            status.version.as_deref(),
            status.updater,
        )
    }

    /// The same for the Lightkey helper.
    pub fn lightkey_view(&self, helper: &LightkeyHelper) -> Option<HelperUpdateStatus> {
        self.view(
            HelperKind::Lightkey,
            helper.version.as_deref(),
            helper.updater,
        )
    }

    /// Adds the update state to the helper's part of a Lightkey status.
    pub fn annotate_lightkey(&self, status: &mut LightkeyStatus) {
        if let Some(helper) = status.helper.as_mut() {
            helper.update = self.lightkey_view(helper);
        }
    }

    fn view(
        &self,
        kind: HelperKind,
        running: Option<&str>,
        updater: bool,
    ) -> Option<HelperUpdateStatus> {
        let bundle = self.bundle(kind);
        let version = bundle.version.clone();
        let say = |mode, message: Option<String>| {
            Some(HelperUpdateStatus {
                mode,
                version: version.clone(),
                message,
            })
        };
        let mut kinds = lock(&self.state);
        let st = kinds.of(kind);
        if !version_is_older(running, &version) {
            return match &st.updated {
                Some((done, at))
                    if *done == version && at.elapsed() < self.config.updated_shown =>
                {
                    say(
                        HelperUpdateMode::Updated,
                        Some(format!("Updated to {version} automatically.")),
                    )
                }
                _ => None,
            };
        }
        if st.updating.as_deref() == Some(version.as_str()) {
            return say(
                HelperUpdateMode::Updating,
                Some(format!("Updating {} to {version}…", kind.name())),
            );
        }
        if st.gave_up.contains(&version) {
            return say(
                HelperUpdateMode::Manual,
                Some(format!(
                    "{} could not update itself. Run the setup command on the {} to update it.",
                    capital(kind.name()),
                    kind.computer()
                )),
            );
        }
        if !updater || !self.env.auto_update() || !self.signed(kind) {
            return say(HelperUpdateMode::Manual, None);
        }
        say(
            HelperUpdateMode::Automatic,
            Some(format!(
                "A newer version is ready. StagePilot will update {} by itself when nothing is playing.",
                kind.name()
            )),
        )
    }

    // ---------------------------------------------------------------- one look at one helper

    /// Looks at the Library Link and, when every condition holds, updates it. Nothing is sent otherwise.
    pub async fn evaluate_playback(&self) -> Outcome {
        let kind = HelperKind::LibraryLink;
        if !self.env.auto_update() {
            return Outcome::Off;
        }
        let bundle = self.bundle(kind).clone();
        if let Some(stop) = self.blocked(kind, &bundle) {
            return stop;
        }
        let settings = self.env.playback_settings();
        let Some(base) = base_url(&settings.address) else {
            return Outcome::Unreachable;
        };
        // Everything below goes to the address that was checked here, and to no other: nothing leaves for a computer that is not on
        // this network, not even the question.
        let target = match lan_target(&base).await {
            Ok(target) => target,
            Err(why) => return Outcome::Refused(why),
        };
        let health = match self.library.fresh_health(&target.url, &target.host).await {
            Ok(health) => health,
            Err(_) => {
                return Outcome::Unreachable;
            }
        };
        if !matches!(
            helper_state_for(&health),
            HelperState::Ready | HelperState::Outdated
        ) || health.auth.as_deref() != Some("key")
        {
            // It answers, but not to our key (or has none): there is nothing this StagePilot may do to it.
            return Outcome::Unreachable;
        }
        if !version_is_older(health.version.as_deref(), &bundle.version) {
            return Outcome::Current;
        }
        if health.updater.is_none_or(|level| level < 1) {
            return Outcome::NotCapable;
        }
        if self.env.song_playing() {
            return Outcome::NotQuiet("a song is playing");
        }
        if self.env.timer_running() {
            return Outcome::NotQuiet("the song timer is running");
        }
        let Some(body) = bundle.request_body() else {
            return Outcome::Unsigned;
        };
        self.begin(kind, &bundle.version);
        let reply = self
            .library
            .post_update(&target.url, &target.host, body)
            .await;
        let sent = match reply {
            Ok((200, answer)) if answer.get("updating") == Some(&Value::Bool(true)) => Ok(()),
            Ok((_, answer)) => Err(error_text(&answer)),
            Err(failure) => Err(match failure {
                Failure::NoAnswer => "The Library Link did not answer.".to_string(),
                _ => {
                    "The Library Link answered in a way StagePilot does not understand.".to_string()
                }
            }),
        };
        if let Err(message) = sent {
            return self.failed(kind, &bundle.version, message);
        }
        // Sent. Now the helper has a minute to restart and say which version it runs.
        let deadline = Instant::now() + self.config.wait_for_new_version;
        loop {
            tokio::time::sleep(self.config.poll).await;
            if let Ok(now) = self.library.fresh_health(&target.url, &target.host).await {
                if let Some(problem) = now.last_update_error.clone() {
                    return self.failed(kind, &bundle.version, problem);
                }
                if now.version.as_deref() == Some(bundle.version.as_str()) {
                    return self.succeeded(kind, &bundle.version);
                }
            }
            if Instant::now() >= deadline {
                return self.failed(
                    kind,
                    &bundle.version,
                    "The Library Link did not come back with the new version in time.".into(),
                );
            }
        }
    }

    /// Looks at the ProPresenter Link and, when every condition holds, updates it. Nothing is sent otherwise. The Local connection
    /// has no helper: nothing is looked at.
    pub async fn evaluate_propresenter(&self) -> Outcome {
        let kind = HelperKind::ProPresenterLink;
        let Some(link) = self.propresenter.clone() else {
            return Outcome::Unreachable;
        };
        if !self.env.auto_update() {
            return Outcome::Off;
        }
        let bundle = self.bundle(kind).clone();
        if let Some(stop) = self.blocked(kind, &bundle) {
            return stop;
        }
        let Some(base) = link.helper_base().await else {
            return Outcome::Unreachable;
        };
        // Everything below goes to the address that was checked here, and to no other.
        let target = match lan_target(&base).await {
            Ok(target) => target,
            Err(why) => return Outcome::Refused(why),
        };
        let health = match link.fresh_health(&target.url, &target.host).await {
            Ok(health) => health,
            Err(_) => return Outcome::Unreachable,
        };
        if health.auth.as_deref() != Some("key") {
            // It answers, but not to our key: there is nothing this StagePilot may do to it.
            return Outcome::Unreachable;
        }
        if !version_is_older(health.version.as_deref(), &bundle.version) {
            return Outcome::Current;
        }
        if health.updater.is_none_or(|level| level < 1) {
            return Outcome::NotCapable;
        }
        if self.env.song_playing() {
            return Outcome::NotQuiet("a song is playing");
        }
        if self.env.timer_running() {
            return Outcome::NotQuiet("the song timer is running");
        }
        let Some(body) = bundle.request_body() else {
            return Outcome::Unsigned;
        };
        self.begin(kind, &bundle.version);
        let sent = match link.post_update(&target.url, &target.host, body).await {
            Ok((200, answer)) if answer.get("updating") == Some(&Value::Bool(true)) => Ok(()),
            Ok((_, answer)) => Err(error_text(&answer)),
            Err(LinkError::NoAnswer) => Err("The ProPresenter Link did not answer.".to_string()),
            Err(_) => Err(
                "The ProPresenter Link answered in a way StagePilot does not understand."
                    .to_string(),
            ),
        };
        if let Err(message) = sent {
            return self.failed(kind, &bundle.version, message);
        }
        let deadline = Instant::now() + self.config.wait_for_new_version;
        loop {
            tokio::time::sleep(self.config.poll).await;
            if let Ok(now) = link.fresh_health(&target.url, &target.host).await {
                if let Some(problem) = now.last_update_error.clone() {
                    return self.failed(kind, &bundle.version, problem);
                }
                if now.version.as_deref() == Some(bundle.version.as_str()) {
                    return self.succeeded(kind, &bundle.version);
                }
            }
            if Instant::now() >= deadline {
                return self.failed(
                    kind,
                    &bundle.version,
                    "The ProPresenter Link did not come back with the new version in time.".into(),
                );
            }
        }
    }

    /// Looks at the Lightkey helper and, when every condition holds, updates it. Nothing is sent otherwise.
    pub async fn evaluate_lightkey(&self) -> Outcome {
        let kind = HelperKind::Lightkey;
        if !self.env.auto_update() {
            return Outcome::Off;
        }
        let bundle = self.bundle(kind).clone();
        if let Some(stop) = self.blocked(kind, &bundle) {
            return stop;
        }
        let Some(status) = self.env.lightkey().await else {
            return Outcome::Unreachable;
        };
        let connected = status.link == LightkeyLink::Connected;
        let Some(helper) = status.helper.as_ref().filter(|_| connected) else {
            return Outcome::Unreachable;
        };
        if !version_is_older(helper.version.as_deref(), &bundle.version) {
            return Outcome::Current;
        }
        if !helper.updater {
            return Outcome::NotCapable;
        }
        if self.env.song_playing() {
            return Outcome::NotQuiet("a song is playing");
        }
        if self.env.timer_running() {
            return Outcome::NotQuiet("the song timer is running");
        }
        if status.recording || status.recording_song.is_some() || status.saving.is_some() {
            return Outcome::NotQuiet("a recording is being made");
        }
        let target = match lan_target(&status.api_url).await {
            Ok(target) => target,
            Err(why) => return Outcome::Refused(why),
        };
        // Straight from the helper, just before: the version it runs now and whether it is recording a take.
        let Some(fresh) = self.lightkey_get(&target, "/health").await else {
            return Outcome::Unreachable;
        };
        let running = fresh.get("version").and_then(Value::as_str);
        if !version_is_older(running, &bundle.version) {
            return Outcome::Current;
        }
        match self.lightkey_get(&target, "/record").await {
            Some(record) if record.get("recording") == Some(&Value::Bool(false)) => {}
            _ => return Outcome::NotQuiet("a take is being recorded"),
        }
        let Some(body) = bundle.request_body() else {
            return Outcome::Unsigned;
        };
        self.begin(kind, &bundle.version);
        let sent = match self.lightkey_post(&target, body).await {
            Ok((200, answer)) if answer.get("updating") == Some(&Value::Bool(true)) => Ok(()),
            Ok((_, answer)) => Err(error_text(&answer)),
            Err(()) => Err("The helper did not answer.".to_string()),
        };
        if let Err(message) = sent {
            return self.failed(kind, &bundle.version, message);
        }
        let deadline = Instant::now() + self.config.wait_for_new_version;
        loop {
            tokio::time::sleep(self.config.poll).await;
            if let Some(now) = self.lightkey_get(&target, "/health").await {
                if let Some(problem) = now.get("last_update_error").and_then(Value::as_str) {
                    return self.failed(kind, &bundle.version, problem.to_string());
                }
                if now.get("version").and_then(Value::as_str) == Some(bundle.version.as_str()) {
                    return self.succeeded(kind, &bundle.version);
                }
            }
            if Instant::now() >= deadline {
                return self.failed(
                    kind,
                    &bundle.version,
                    "The helper did not come back with the new version in time.".into(),
                );
            }
        }
    }

    /// The conditions that do not depend on the world: a signed bundle, attempts left, no failed attempt being waited out.
    fn blocked(&self, kind: HelperKind, bundle: &UpdateBundle) -> Option<Outcome> {
        if !self.signed(kind) {
            return Some(Outcome::Unsigned);
        }
        let mut kinds = lock(&self.state);
        let st = kinds.of(kind);
        if st.gave_up.contains(&bundle.version) {
            return Some(Outcome::GaveUp);
        }
        if st.next_attempt.is_some_and(|at| Instant::now() < at) {
            return Some(Outcome::Waiting);
        }
        None
    }

    fn begin(&self, kind: HelperKind, version: &str) {
        tracing::info!(
            helper = kind.log_name(),
            to = version,
            "helper_update_sending"
        );
        lock(&self.state).of(kind).updating = Some(version.to_string());
    }

    fn succeeded(&self, kind: HelperKind, version: &str) -> Outcome {
        tracing::info!(helper = kind.log_name(), to = version, "helper_update_done");
        let mut kinds = lock(&self.state);
        let st = kinds.of(kind);
        st.updating = None;
        st.updated = Some((version.to_string(), Instant::now()));
        st.next_attempt = None;
        Outcome::Updated(version.to_string())
    }

    fn failed(&self, kind: HelperKind, version: &str, message: String) -> Outcome {
        let mut kinds = lock(&self.state);
        let st = kinds.of(kind);
        st.updating = None;
        let attempts = {
            let n = st.attempts.entry(version.to_string()).or_insert(0);
            *n += 1;
            *n
        };
        tracing::warn!(
            helper = kind.log_name(),
            to = version,
            attempt = attempts,
            "helper_update_failed"
        );
        if attempts >= self.config.max_attempts {
            st.gave_up.insert(version.to_string());
            st.next_attempt = None;
        } else {
            let wait = self
                .config
                .backoff
                .get(attempts as usize - 1)
                .or(self.config.backoff.last())
                .copied()
                .unwrap_or(self.config.retry);
            st.next_attempt = Some(Instant::now() + wait);
        }
        Outcome::Failed(message)
    }

    // ---------------------------------------------------------------- the Lightkey helper over HTTP

    async fn lightkey_get(&self, target: &Target, path: &str) -> Option<Value> {
        let mut response = self
            .http
            .get(format!("{}{path}", target.url))
            .header(reqwest::header::HOST, &target.host)
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .ok()?;
        if response.status().as_u16() != 200 {
            return None;
        }
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if body.len() + chunk.len() > 64 * 1024 {
                return None;
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).ok()
    }

    async fn lightkey_post(&self, target: &Target, body: Vec<u8>) -> Result<(u16, Value), ()> {
        let mut response = self
            .http
            .post(format!("{}/update", target.url))
            .header(reqwest::header::HOST, &target.host)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .timeout(Duration::from_secs(20))
            .body(body)
            .send()
            .await
            .map_err(|_| ())?;
        let status = response.status().as_u16();
        let mut answer: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
            if answer.len() + chunk.len() > 16 * 1024 {
                return Err(());
            }
            answer.extend_from_slice(&chunk);
        }
        Ok((
            status,
            serde_json::from_slice(&answer).unwrap_or(Value::Null),
        ))
    }

    // ---------------------------------------------------------------- when to look

    /// One pass of the scheduler, at `now`: notices a helper that came back, and looks at each helper whose turn it is.
    pub async fn tick(&self, now: Instant) {
        for kind in self.kinds() {
            self.tick_one(kind, now).await;
        }
    }

    /// The helpers this updater looks after: the ProPresenter Link only when one was given.
    fn kinds(&self) -> Vec<HelperKind> {
        let mut kinds = vec![HelperKind::LibraryLink, HelperKind::Lightkey];
        if self.propresenter.is_some() {
            kinds.push(HelperKind::ProPresenterLink);
        }
        kinds
    }

    /// Opens a window for both helpers: StagePilot has just started.
    pub fn open_windows(&self, now: Instant) {
        for kind in self.kinds() {
            self.open_window(kind, now);
        }
    }

    fn open_window(&self, kind: HelperKind, now: Instant) {
        lock(&self.state).of(kind).window = Some(Window {
            until: now + self.config.window,
            try_at: now,
        });
    }

    /// What is known without asking anything: does the helper answer.
    async fn observed(&self, kind: HelperKind) -> Option<bool> {
        match kind {
            HelperKind::LibraryLink => self.library.last_health_reachable(),
            HelperKind::ProPresenterLink => self
                .propresenter
                .as_ref()
                .and_then(|link| link.last_health_reachable()),
            HelperKind::Lightkey => self
                .env
                .lightkey()
                .await
                .map(|status| status.link == LightkeyLink::Connected && status.helper.is_some()),
        }
    }

    async fn tick_one(&self, kind: HelperKind, now: Instant) {
        // A helper that was silent and answers now: a new window, once.
        if let Some(seen) = self.observed(kind).await {
            let mut kinds = lock(&self.state);
            let st = kinds.of(kind);
            if seen && st.seen == Some(false) {
                st.window = Some(Window {
                    until: now + self.config.window,
                    try_at: now,
                });
            }
            st.seen = Some(seen);
        }
        let due = lock(&self.state)
            .of(kind)
            .window
            .is_some_and(|w| now >= w.try_at);
        if !due {
            return;
        }
        let outcome = match kind {
            HelperKind::LibraryLink => self.evaluate_playback().await,
            HelperKind::Lightkey => self.evaluate_lightkey().await,
            HelperKind::ProPresenterLink => self.evaluate_propresenter().await,
        };
        let mut kinds = lock(&self.state);
        let st = kinds.of(kind);
        let Some(window) = st.window.as_mut() else {
            return;
        };
        match outcome {
            // Not now: try again in a while, for as long as the window lasts.
            Outcome::Unreachable | Outcome::NotQuiet(_) | Outcome::Waiting => {
                window.try_at = now + self.config.retry;
                if window.try_at > window.until {
                    st.window = None;
                }
            }
            // One attempt failed, with attempts left: the next one after the wait, window or not.
            Outcome::Failed(_) => match st.next_attempt {
                Some(at) => {
                    window.try_at = at;
                    window.until = window.until.max(at);
                }
                None => st.window = None,
            },
            // Done, or nothing more can be done until StagePilot starts again or the helper comes back.
            Outcome::Off
            | Outcome::Current
            | Outcome::NotCapable
            | Outcome::Unsigned
            | Outcome::Refused(_)
            | Outcome::GaveUp
            | Outcome::Updated(_) => st.window = None,
        }
    }

    /// The scheduler: once StagePilot is up it looks at both helpers, then keeps to the windows above.
    pub fn spawn(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(me.config.start_delay).await;
            me.open_windows(Instant::now());
            loop {
                me.tick(Instant::now()).await;
                tokio::time::sleep(me.config.tick).await;
            }
        })
    }
}

fn capital(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// The helper's own sentence for a refusal, cut short; a generic one when it said none.
fn error_text(answer: &Value) -> String {
    answer
        .get("error")
        .and_then(Value::as_str)
        .map(|text| text.chars().take(200).collect::<String>())
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| "The helper did not accept the update.".to_string())
}
