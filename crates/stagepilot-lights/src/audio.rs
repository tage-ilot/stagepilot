//! The audio input behind a trait, so tests use a fake and the real device code (cpal, behind the
//! `cpal-backend` feature) stays out of headless builds. The hub opens the chosen input only while
//! a Lightkey recording runs or the input picker's meter is on, measures levels and drops samples.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use stagepilot_core::config::SongLoudness;

use crate::lightkey_link::Clock;
use crate::loudness::{level_byte, LevelMeter, LoudnessRecorder};

/// Why an input could not be used. The messages are what the operator reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    NoDevice,
    PermissionDenied,
    DeviceLost(String),
    /// This build cannot capture audio at all (headless server, Docker).
    Unavailable(String),
    Other(String),
}

impl AudioError {
    pub fn message(&self) -> String {
        match self {
            Self::NoDevice => "No audio input was found.".into(),
            Self::PermissionDenied => {
                "Microphone access was refused. Allow StagePilot in System Settings > Privacy & Security > Microphone.".into()
            }
            Self::DeviceLost(name) => format!("The audio input {name} was lost."),
            Self::Unavailable(reason) | Self::Other(reason) => reason.clone(),
        }
    }
}

/// Mono samples (full scale 1.0) and their sample rate.
pub type SampleSink = Arc<dyn Fn(&[f32], u32) + Send + Sync>;
pub type ErrorSink = Arc<dyn Fn(AudioError) + Send + Sync>;

/// An open input; dropping it closes the device.
pub trait AudioStream: Send {}

pub trait AudioInput: Send + Sync {
    /// Names of the available inputs.
    fn devices(&self) -> Result<Vec<String>, AudioError>;
    /// Opens `device` (the system default input when `None`).
    fn open(
        &self,
        device: Option<&str>,
        samples: SampleSink,
        errors: ErrorSink,
    ) -> Result<Box<dyn AudioStream>, AudioError>;
}

/// The input of builds without audio capture.
pub struct UnavailableInput(pub String);

impl AudioInput for UnavailableInput {
    fn devices(&self) -> Result<Vec<String>, AudioError> {
        Err(AudioError::Unavailable(self.0.clone()))
    }
    fn open(
        &self,
        _: Option<&str>,
        _: SampleSink,
        _: ErrorSink,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        Err(AudioError::Unavailable(self.0.clone()))
    }
}

/// The input of this build: the real device through cpal when the `cpal-backend` feature is on,
/// otherwise one that says why there is none.
pub fn default_input() -> Arc<dyn AudioInput> {
    #[cfg(feature = "cpal-backend")]
    {
        Arc::new(crate::audio_cpal::CpalInput)
    }
    #[cfg(not(feature = "cpal-backend"))]
    {
        Arc::new(UnavailableInput(UNAVAILABLE_TEXT.into()))
    }
}

pub const UNAVAILABLE_TEXT: &str =
    "Loudness needs the desktop app: this build cannot listen to an audio input.";
/// How long the meter stays on after the picker last asked for it.
pub const METER_SECONDS: f64 = 5.0;

/// A recording's recorder, shared with the audio callback.
pub type SharedRecorder = Arc<Mutex<LoudnessRecorder>>;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MeterLevel {
    pub rms_db: Option<f64>,
    pub peak_db: Option<f64>,
    /// 0 to 255 on the stored scale, for a bar.
    pub value: u8,
}

/// What `GET .../audio` returns.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AudioInfo {
    pub available: bool,
    /// Why capture is unavailable, or the last input error; one short line.
    pub reason: Option<String>,
    pub devices: Vec<String>,
    /// The saved choice (`None` = system default input).
    pub selected: Option<String>,
    /// A line about the choice, e.g. that a saved input is gone and the default is used.
    pub note: Option<String>,
    pub level: Option<MeterLevel>,
    pub metering: bool,
}

struct Shared {
    clock: Arc<dyn Clock>,
    recorder: Mutex<Option<SharedRecorder>>,
    meter: LevelMeter,
    error: Mutex<Option<String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The open device; held so it stays open, dropped to close it.
struct Open(#[allow(dead_code)] Box<dyn AudioStream>);

/// How long the list of inputs is reused: the picker polls, listing devices is not free.
const DEVICES_TTL_SECONDS: f64 = 2.0;

type DeviceList = (Vec<String>, bool, Option<String>);

#[derive(Default)]
struct HubState {
    open: Option<Open>,
    meter_until: f64,
    wanted_device: Option<String>,
    note: Option<String>,
    /// The last device listing and when it was taken.
    devices: Option<(f64, DeviceList)>,
}

/// Opening a device can sit on the microphone permission dialog, and closing one joins its audio
/// thread. The hub therefore never holds `state` across either, and the calls that open or close
/// run one at a time on `reconcile_lock`; the ones that only poll skip the work when another is
/// already in progress (A-01). Callers on an async thread go through `spawn_blocking`.
pub struct AudioHub {
    input: Arc<dyn AudioInput>,
    shared: Arc<Shared>,
    state: Mutex<HubState>,
    reconcile_lock: Mutex<()>,
}

impl AudioHub {
    pub fn new(input: Arc<dyn AudioInput>, clock: Arc<dyn Clock>) -> Self {
        Self {
            input,
            shared: Arc::new(Shared {
                clock,
                recorder: Mutex::new(None),
                meter: LevelMeter::default(),
                error: Mutex::new(None),
            }),
            state: Mutex::new(HubState::default()),
            reconcile_lock: Mutex::new(()),
        }
    }

    /// The saved input choice changed (or is read for the first time).
    pub fn set_device(&self, device: Option<String>) -> Option<String> {
        let closing = {
            let mut state = lock(&self.state);
            if state.wanted_device != device {
                state.wanted_device = device;
                state.open.take() // reopened on the new choice at the next reconcile
            } else {
                None
            }
        };
        drop(closing); // joins the audio thread, outside the state lock
        self.reconcile()
    }

    /// Starts (`Some`) or ends (`None`) feeding a recording. `Err` carries why the input could not
    /// be opened; the recording itself carries on without loudness.
    pub fn set_recorder(&self, recorder: Option<SharedRecorder>) -> Result<(), String> {
        *lock(&self.shared.recorder) = recorder;
        match self.reconcile() {
            Some(error) if lock(&self.shared.recorder).is_some() => Err(error),
            _ => Ok(()),
        }
    }

    /// Switches the picker's meter on or off (it also switches itself off after a few seconds).
    pub fn set_meter(&self, on: bool) -> Option<String> {
        let now = self.shared.clock.wall();
        lock(&self.state).meter_until = if on { now + METER_SECONDS } else { 0.0 };
        if !on {
            self.shared.meter.clear();
        }
        // A poll never queues behind an open that is already in progress.
        self.try_reconcile()
    }

    /// The picker's data with the input listing refreshed when it is older than a couple of
    /// seconds. May call the device layer: run it on a blocking thread.
    pub fn info(&self) -> AudioInfo {
        // Lets an expired meter close the input.
        let _ = self.try_reconcile();
        let now = self.shared.clock.wall();
        let cached = lock(&self.state)
            .devices
            .as_ref()
            .filter(|(at, _)| now - *at < DEVICES_TTL_SECONDS && now >= *at)
            .map(|(_, list)| list.clone());
        let list = cached.unwrap_or_else(|| {
            let list = match self.input.devices() {
                Ok(devices) => (devices, true, None),
                Err(error) => (
                    Vec::new(),
                    !matches!(error, AudioError::Unavailable(_)),
                    Some(error.message()),
                ),
            };
            lock(&self.state).devices = Some((now, list.clone()));
            list
        });
        self.assemble(list)
    }

    /// What is known without touching the device layer: the last listing, the meter and the last
    /// error. For a caller that gave up waiting.
    pub fn snapshot(&self) -> AudioInfo {
        let list = lock(&self.state)
            .devices
            .as_ref()
            .map(|(_, list)| list.clone())
            .unwrap_or((Vec::new(), true, None));
        self.assemble(list)
    }

    fn assemble(&self, (devices, available, mut reason): DeviceList) -> AudioInfo {
        let state = lock(&self.state);
        let level = self.shared.meter.read().map(|w| MeterLevel {
            rms_db: w.rms_db.is_finite().then_some(w.rms_db),
            peak_db: w.peak_db.is_finite().then_some(w.peak_db),
            value: level_byte(w.rms_db),
        });
        if reason.is_none() {
            reason = lock(&self.shared.error).clone();
        }
        AudioInfo {
            available,
            reason,
            devices,
            selected: state.wanted_device.clone(),
            note: state.note.clone(),
            level,
            metering: state.meter_until > self.shared.clock.wall(),
        }
    }

    fn needed(&self) -> bool {
        lock(&self.shared.recorder).is_some()
            || lock(&self.state).meter_until > self.shared.clock.wall()
    }

    /// Opens or closes the device to match what is needed. `Some` is why it could not be opened.
    /// Waits for an open or close already in progress.
    fn reconcile(&self) -> Option<String> {
        let _turn = lock(&self.reconcile_lock);
        self.reconcile_now()
    }

    /// Like `reconcile`, but does nothing when another one is in progress.
    fn try_reconcile(&self) -> Option<String> {
        let _turn = match self.reconcile_lock.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        self.reconcile_now()
    }

    fn reconcile_now(&self) -> Option<String> {
        if !self.needed() {
            let closing = lock(&self.state).open.take();
            drop(closing); // joins the audio thread, outside the state lock
            return None;
        }
        let wanted = {
            let state = lock(&self.state);
            if state.open.is_some() {
                return None;
            }
            state.wanted_device.clone()
        };
        // A saved input that is gone falls back to the default.
        let mut device = wanted.clone();
        let mut note = None;
        if let (Some(name), Ok(devices)) = (&wanted, self.input.devices()) {
            if !devices.contains(name) {
                note = Some(format!("{name} is not available. Using the default input."));
                device = None;
            }
        }
        let shared = self.shared.clone();
        let samples: SampleSink = Arc::new(move |block, rate| {
            shared.meter.feed(block);
            let recorder = lock(&shared.recorder).clone();
            if let Some(recorder) = recorder {
                lock(&recorder).push(shared.clock.wall(), rate, block);
            }
        });
        let shared = self.shared.clone();
        let errors: ErrorSink = Arc::new(move |error| {
            *lock(&shared.error) = Some(error.message());
        });
        // The slow part (a permission dialog, a device start) runs with no hub lock held.
        match self.input.open(device.as_deref(), samples, errors) {
            Ok(stream) => {
                *lock(&self.shared.error) = None;
                let mut state = lock(&self.state);
                state.note = note;
                state.open = Some(Open(stream));
                None
            }
            Err(error) => {
                let message = error.message();
                *lock(&self.shared.error) = Some(message.clone());
                lock(&self.state).note = note;
                Some(message)
            }
        }
    }

    /// Takes the stored scale of `recorder` for a song of `duration` seconds.
    pub fn finish(recorder: &SharedRecorder, duration: f64) -> Option<SongLoudness> {
        lock(recorder).finish(duration)
    }
}
