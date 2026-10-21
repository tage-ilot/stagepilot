//! Output plugins (phase 4): MIDI input, lights and ProPresenter, built from the resolved settings
//! and registered with the plugin manager, plus the live Presentation (ProPresenter) switch.
//!
//! Registration order is the health order Python reports: lights, services (demo), the Playback
//! input (`midi_playback` when MIDI is the real source, otherwise the `playback_api` placeholder
//! phase 3 replaces), then ProPresenter when the timer output is ProPresenter.
//!
//! The Playback source arbiter (phase 3) is not wired in here. With MIDI as the selected source and
//! no Playback API connected, Python's `ArbitratedDispatcher` forwards every MIDI action straight to
//! the state service, which is what this module does (see `docs/rust-migration/phase4.md`).

use crate::demo::{DemoPlugin, PLAYBACK_DISCONNECTED_REASON};
use async_trait::async_trait;
use chrono::Utc;
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{MidiSettings, MidiSource, ProPresenterSettings, TimerOutput};
use stagepilot_core::events::{new_event, EventPayload, EventType, Integration};
use stagepilot_core::plugin::{Plugin, PluginManager};
use stagepilot_core::settings::RuntimeSettings;
use stagepilot_core::state::{ConnectionStatus, PluginHealth, PluginStatus};
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
use stagepilot_lights::{LightsOptions, LightsPlugin};
use stagepilot_midi::fake::{FakeMidiBackend, FakeOutputBackend};
use stagepilot_midi::output::MidiOutputBackendFactory;
use stagepilot_midi::{
    MidiBackend, MidiBackendFactory, MidiOutputBackend, MidiPlaybackPlugin, MidiPluginOptions,
};
use stagepilot_osc::{OscBackend, UdpOscBackend};
use stagepilot_propresenter::plugin::http_client_factory;
use stagepilot_propresenter::{ClientFactory, ProPresenterPlugin};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

pub const PROPRESENTER_PLUGIN: &str = "propresenter";
pub const MIDI_PLUGIN: &str = "midi_playback";
pub const PRESENTATION_BUSY: &str =
    "Stop the timer before switching Presentation between simulated and ProPresenter.";

/// Port names of the fake MIDI devices (shared with `scripts/contract/outputs_app.py`).
pub const FAKE_MIDI_INPUTS: [&str; 2] = ["StagePilot Test In", "Second Test In"];
pub const FAKE_MIDI_OUTPUTS: [&str; 1] = ["StagePilot Test Out"];

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Where the outputs talk to. The default is real hardware / network (MIDI only in builds with the
/// `midir-backend` feature); tests and the contract profile swap in fakes. Nothing is contacted
/// unless the matching setting enables it.
#[derive(Clone)]
pub struct OutputBackends {
    pub midi: MidiBackendFactory,
    pub midi_output: MidiOutputBackendFactory,
    pub osc: Arc<dyn Fn() -> Arc<dyn OscBackend> + Send + Sync>,
    pub propresenter: ClientFactory,
    /// The HTTP client of the Lightkey link (a fake agent in tests, `reqwest` otherwise).
    pub lightkey_http: stagepilot_lights::lightkey_link::HttpFactory,
    /// Where Lightkey takes live; `None` means next to the settings file (or in memory when the
    /// settings are in memory).
    pub lightkey_store: Option<Arc<dyn stagepilot_lights::lightkey_link::TakeStore>>,
}

impl Default for OutputBackends {
    fn default() -> Self {
        Self {
            midi: stagepilot_midi::default_backend_factory(),
            midi_output: stagepilot_midi::default_output_backend_factory(),
            osc: Arc::new(|| Arc::new(UdpOscBackend)),
            propresenter: http_client_factory(),
            lightkey_http: Arc::new(|| {
                Arc::new(stagepilot_lights::lightkey_link::ReqwestLightkeyHttp::new())
            }),
            lightkey_store: None,
        }
    }
}

impl OutputBackends {
    /// In-memory MIDI with fixed port names; OSC and ProPresenter stay real (the contract profile
    /// points ProPresenter at a local mock server and OSC at a loopback port nothing listens on).
    pub fn fake_midi() -> Self {
        let input = FakeMidiBackend::new(&FAKE_MIDI_INPUTS);
        let output = FakeOutputBackend::new(&FAKE_MIDI_OUTPUTS);
        Self {
            midi: Arc::new(move || Ok(input.clone() as Arc<dyn MidiBackend>)),
            midi_output: Arc::new(move || Ok(output.clone() as Arc<dyn MidiOutputBackend>)),
            ..Self::default()
        }
    }
}

/// What the Playback arbiter knows about MIDI: whether it is connected, and the ownership
/// revision (moves when ownership toggles), plus the `playback_connection` publication.
pub struct MidiLink {
    bus: EventBus,
    connected: AtomicBool,
    owner_midi: AtomicBool,
    revision: AtomicI64,
    published: Mutex<Option<(bool, String)>>,
}

impl MidiLink {
    fn new(bus: EventBus) -> Self {
        Self {
            bus,
            connected: AtomicBool::new(false),
            owner_midi: AtomicBool::new(false),
            revision: AtomicI64::new(0),
            published: Mutex::new(None),
        }
    }

    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// `PlaybackSourceArbiter.revision` when only MIDI can own.
    pub fn revision(&self) -> i64 {
        let owner = self.connected();
        if self.owner_midi.swap(owner, Ordering::SeqCst) != owner {
            self.revision.fetch_add(1, Ordering::SeqCst);
        }
        self.revision.load(Ordering::SeqCst)
    }

    fn reason(&self) -> &'static str {
        if self.connected() {
            "Connected through MIDI."
        } else {
            PLAYBACK_DISCONNECTED_REASON
        }
    }

    async fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::SeqCst);
        self.revision();
        self.publish().await;
    }

    /// `_publish_connection`: one `connection.changed` per distinct (connected, reason).
    pub async fn publish(&self) {
        let key = (self.connected(), self.reason().to_string());
        {
            let mut published = lock(&self.published);
            if published.as_ref() == Some(&key) {
                return;
            }
            *published = Some(key.clone());
        }
        self.bus
            .publish(new_event(
                EventType::ConnectionChanged,
                "playback_connection",
                EventPayload::Connection {
                    integration: Integration::Midi,
                    status: if key.0 {
                        ConnectionStatus::Connected
                    } else {
                        ConnectionStatus::Disconnected
                    },
                    detail: Some(key.1),
                },
            ))
            .await;
    }
}

/// The `midi_playback` plugin Python registers when MIDI is the real source: the Playback input
/// wrapper around an optional MIDI plugin. Health is the MIDI plugin's when there is one.
pub struct MidiInput {
    bus: EventBus,
    state_service: StateService,
    link: Arc<MidiLink>,
    backends: OutputBackends,
    midi: Mutex<Option<Arc<MidiPlaybackPlugin>>>,
    running: AtomicBool,
    settings: Mutex<RuntimeSettings>,
}

impl MidiInput {
    fn build_midi(&self, settings: &MidiSettings) -> Result<Arc<MidiPlaybackPlugin>, String> {
        let plugin = Arc::new(MidiPlaybackPlugin::new(
            self.bus.clone(),
            settings.clone(),
            Arc::new(self.state_service.clone()),
            MidiPluginOptions::new(self.backends.midi.clone()),
        )?);
        let link = self.link.clone();
        plugin.set_connection_observer(Some(Arc::new(move |status| {
            let link = link.clone();
            Box::pin(async move {
                link.set_connected(status == ConnectionStatus::Connected)
                    .await
            })
        })));
        let link = self.link.clone();
        plugin.set_ownership_revision(Some(Arc::new(move || link.revision())));
        Ok(plugin)
    }

    pub fn midi(&self) -> Option<Arc<MidiPlaybackPlugin>> {
        lock(&self.midi).clone()
    }

    /// `PlaybackInputPlugin._reconfigure` for MIDI: a live cue-filter change when MIDI stays real
    /// and enabled; otherwise stop and rebuild from the new settings.
    pub async fn reconfigure(&self, settings: &RuntimeSettings) {
        let previous = lock(&self.settings).clone();
        let same = previous.integration_modes.midi_source == settings.integration_modes.midi_source
            && previous.midi == settings.midi;
        if same {
            return;
        }
        let both_real = previous.integration_modes.midi_source == MidiSource::Real
            && settings.integration_modes.midi_source == MidiSource::Real
            && previous.midi.enabled
            && settings.midi.enabled;
        let current = self.midi();
        if let (true, Some(midi), true) = (
            both_real,
            current.clone(),
            self.running.load(Ordering::SeqCst),
        ) {
            midi.reconfigure(settings.midi.clone()).await;
            *lock(&self.settings) = settings.clone();
            return;
        }
        if let Some(midi) = current {
            let _ = midi.stop().await;
        }
        self.link.set_connected(false).await;
        *lock(&self.midi) = None;
        *lock(&self.settings) = settings.clone();
        if self.running.load(Ordering::SeqCst)
            && settings.integration_modes.midi_source == MidiSource::Real
            && settings.midi.enabled
        {
            if let Ok(midi) = self.build_midi(&settings.midi) {
                let _ = midi.start().await;
                *lock(&self.midi) = Some(midi);
            }
        }
    }
}

#[async_trait]
impl Plugin for MidiInput {
    fn name(&self) -> &str {
        MIDI_PLUGIN
    }
    fn version(&self) -> &str {
        "0.1.0"
    }

    async fn start(&self) -> Result<(), String> {
        self.running.store(true, Ordering::SeqCst);
        self.link.publish().await;
        if let Some(midi) = self.midi() {
            midi.start().await?;
        }
        Ok(())
    }

    async fn stop(&self) -> Result<(), String> {
        self.running.store(false, Ordering::SeqCst);
        let result = match self.midi() {
            Some(midi) => midi.stop().await,
            None => Ok(()),
        };
        self.link.set_connected(false).await;
        result
    }

    async fn health(&self) -> Result<PluginHealth, String> {
        if let Some(midi) = self.midi() {
            return midi.health().await;
        }
        Ok(PluginHealth {
            name: MIDI_PLUGIN.into(),
            version: "0.1.0".into(),
            status: if self.running.load(Ordering::SeqCst) {
                PluginStatus::Running
            } else {
                PluginStatus::Stopped
            },
            last_error: None,
            last_activity_at: Some(Utc::now()),
        })
    }
}

/// Everything the routes and the runtime need from the output plugins.
pub struct Outputs {
    pub backends: OutputBackends,
    pub lights: Arc<LightsPlugin>,
    pub link: Arc<MidiLink>,
    /// `Some` when MIDI is the real source (the plugin is registered as `midi_playback`).
    pub midi_input: Option<Arc<MidiInput>>,
    /// One lock for every cue-map read-modify-write, so a capture, an undo and a cue-map save
    /// never interleave and drop each other's cues.
    pub cue_lock: tokio::sync::Mutex<()>,
    presentation: Mutex<Option<Arc<ProPresenterPlugin>>>,
    applied: Mutex<Option<ProPresenterSettings>>,
    demo: Mutex<Option<Arc<DemoPlugin>>>,
    bus: EventBus,
}

impl Outputs {
    pub fn build(
        bus: &EventBus,
        state_service: &StateService,
        settings: &RuntimeSettings,
        backends: OutputBackends,
        settings_path: Option<std::path::PathBuf>,
    ) -> Result<Self, String> {
        let mut lights_options = LightsOptions::new(backends.midi_output.clone());
        lights_options.osc_backend_factory = backends.osc.clone();
        lights_options.lightkey.http_factory = backends.lightkey_http.clone();
        // `lightkey-takes.json` sits next to `settings.json`; in-memory settings keep takes in
        // memory (Python: `MemoryTakeStore` when explicit settings were given).
        if let Some(store) = backends.lightkey_store.clone() {
            lights_options.lightkey.store = store;
        } else if let Some(path) = settings_path {
            lights_options.lightkey.store =
                Arc::new(stagepilot_lights::lightkey_link::FileTakeStore::new(
                    path.with_file_name("lightkey-takes.json"),
                ));
        }
        let lights = Arc::new(LightsPlugin::new(
            bus.clone(),
            settings.lights.clone(),
            lights_options,
        )?);
        let link = Arc::new(MidiLink::new(bus.clone()));
        let mut midi_input = None;
        if settings.integration_modes.midi_source == MidiSource::Real {
            let input = Arc::new(MidiInput {
                bus: bus.clone(),
                state_service: state_service.clone(),
                link: link.clone(),
                backends: backends.clone(),
                midi: Mutex::new(None),
                running: AtomicBool::new(false),
                settings: Mutex::new(settings.clone()),
            });
            if settings.midi.enabled {
                *lock(&input.midi) = Some(input.build_midi(&settings.midi)?);
            }
            midi_input = Some(input);
        }
        Ok(Self {
            backends,
            lights,
            link,
            midi_input,
            cue_lock: tokio::sync::Mutex::new(()),
            presentation: Mutex::new(None),
            applied: Mutex::new(None),
            demo: Mutex::new(None),
            bus: bus.clone(),
        })
    }

    /// The MIDI plugin if one is running ("midi_controller").
    pub fn midi(&self) -> Option<Arc<MidiPlaybackPlugin>> {
        self.midi_input.as_ref().and_then(|input| input.midi())
    }

    pub fn propresenter(&self) -> Option<Arc<ProPresenterPlugin>> {
        lock(&self.presentation).clone()
    }

    pub fn wanted_propresenter(settings: &RuntimeSettings) -> bool {
        settings.propresenter.enabled
            && settings.integration_modes.timer_output == TimerOutput::Propresenter
    }

    /// Build the startup ProPresenter plugin when the settings ask for one.
    pub fn startup_propresenter(
        &self,
        settings: &RuntimeSettings,
    ) -> Option<Arc<ProPresenterPlugin>> {
        let plugin = Self::wanted_propresenter(settings).then(|| {
            Arc::new(ProPresenterPlugin::new(
                self.bus.clone(),
                settings.propresenter.clone(),
                self.backends.propresenter.clone(),
            ))
        });
        *lock(&self.presentation) = plugin.clone();
        *lock(&self.applied) = plugin.as_ref().map(|_| settings.propresenter.clone());
        plugin
    }

    /// `PresentationController.adopt`: record the settings the running plugin has applied.
    pub fn adopt_presentation(&self, settings: ProPresenterSettings) {
        *lock(&self.applied) = Some(settings);
    }

    pub fn set_demo(&self, demo: Arc<DemoPlugin>) {
        *lock(&self.demo) = Some(demo);
    }

    /// `PresentationController.sync`: make the running Presentation output match the saved
    /// settings. `Err(message)` is the 409 "stop the timer first"; `Ok(Some(warning))` a start that
    /// failed.
    pub async fn sync_presentation(
        &self,
        settings: &RuntimeSettings,
        plugins: &PluginManager,
        store: &StateStore,
        state_service: &StateService,
    ) -> Result<Option<String>, String> {
        let wanted = Self::wanted_propresenter(settings);
        let present = self.propresenter();
        if wanted {
            if let Some(present) = &present {
                let changed = lock(&self.applied).as_ref() != Some(&settings.propresenter);
                if changed {
                    present.reconfigure(settings.propresenter.clone()).await;
                    *lock(&self.applied) = Some(settings.propresenter.clone());
                }
                return Ok(None);
            }
        }
        let demo = lock(&self.demo).clone();
        if !wanted && present.is_none() {
            if let Some(demo) = demo {
                demo.set_simulate_propresenter(true).await;
            }
            return Ok(None);
        }
        if store.snapshot().timer.status == stagepilot_core::state::TimerStatus::Running {
            return Err(PRESENTATION_BUSY.to_string());
        }
        let forget = || {
            store.mutate(|s| {
                s.plugins.remove(PROPRESENTER_PLUGIN);
            });
        };
        if wanted {
            let plugin = Arc::new(ProPresenterPlugin::new(
                self.bus.clone(),
                settings.propresenter.clone(),
                self.backends.propresenter.clone(),
            ));
            if let Some(demo) = &demo {
                // The simulation stops owning the timer: forget whatever it left behind.
                demo.set_simulate_propresenter(false).await;
            }
            state_service.clear_timer();
            let health = plugins.add(plugin.clone() as Arc<dyn Plugin>).await;
            return match health {
                Ok(health) if health.status == PluginStatus::Running => {
                    *lock(&self.presentation) = Some(plugin);
                    *lock(&self.applied) = Some(settings.propresenter.clone());
                    Ok(None)
                }
                Ok(health) => {
                    *lock(&self.presentation) = None;
                    *lock(&self.applied) = None;
                    forget();
                    Ok(Some(health.last_error.unwrap_or_else(|| {
                        "Presentation could not be started.".into()
                    })))
                }
                Err(error) => {
                    *lock(&self.presentation) = None;
                    *lock(&self.applied) = None;
                    forget();
                    Ok(Some(error))
                }
            };
        }
        plugins.remove(PROPRESENTER_PLUGIN).await;
        *lock(&self.presentation) = None;
        *lock(&self.applied) = None;
        forget();
        state_service.clear_timer();
        if let Some(demo) = demo {
            demo.set_simulate_propresenter(true).await;
        }
        Ok(None)
    }

    /// Use after a `PUT /settings`: follow MIDI source/enable/cue changes live.
    pub async fn apply_midi(&self, settings: &RuntimeSettings) {
        if let Some(input) = &self.midi_input {
            input.reconfigure(settings).await;
        }
    }
}

pub fn register(plugins: &PluginManager, plugin: Arc<dyn Plugin>) {
    if let Err(error) = plugins.register(plugin) {
        tracing::error!(%error, "plugin_register_failed");
    }
}
