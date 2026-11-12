//! Loudness while recording, on the song clock: a fake audio input and a fake wall clock drive the
//! real Lightkey link. Nothing touches a device, a network or the real clock.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{LightkeySettings, SongLoudness};
use stagepilot_core::events::{new_event, EventPayload, EventType, TimerPayload};
use stagepilot_core::state::Song;
use stagepilot_lights::audio::{
    AudioError, AudioHub, AudioInput, AudioStream, ErrorSink, SampleSink,
};
use stagepilot_lights::lightkey::LightkeyLink as Link;
use stagepilot_lights::lightkey_link::{
    Clock, HttpFailure, LightkeyHttp, LightkeyLink, LinkOptions, LinkSettings, LoudnessSink,
    SongLoudnessTake,
};
use stagepilot_lights::loudness::{byte_db, level_byte};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ------------------------------------------------------------------------------------ fakes

struct FakeClock {
    wall: Mutex<f64>,
    started: Instant,
}
impl FakeClock {
    fn set(&self, wall: f64) {
        *self.wall.lock().unwrap() = wall;
    }
    fn get(&self) -> f64 {
        *self.wall.lock().unwrap()
    }
}
impl Clock for FakeClock {
    fn wall(&self) -> f64 {
        self.get()
    }
    fn mono(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }
}

struct FakeHttp(Arc<FakeClock>);
#[async_trait]
impl LightkeyHttp for FakeHttp {
    async fn get_json(&self, url: &str, _q: &[(&str, String)]) -> Result<Value, HttpFailure> {
        let ms = self.0.get() * 1000.0;
        if url.ends_with("/time") {
            return Ok(json!({"t": ms}));
        }
        Ok(
            json!({"events": [], "state": {"online": true, "active": [], "seq": 1, "server_time": ms}}),
        )
    }
}

struct FakeStream(Arc<AtomicUsize>);
impl AudioStream for FakeStream {}
impl Drop for FakeStream {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct FakeInput {
    devices: Mutex<Vec<String>>,
    fail: Mutex<Option<AudioError>>,
    sink: Mutex<Option<SampleSink>>,
    errors: Mutex<Option<ErrorSink>>,
    opened: Mutex<Vec<Option<String>>>,
    open_now: Arc<AtomicUsize>,
    /// How many `open` calls have returned a stream so far (open or since closed).
    opens_returned: AtomicUsize,
    /// How long `open` blocks, like a microphone permission dialog.
    open_delay: Mutex<Duration>,
    /// When set, `open` never returns until `release_opens`: a permission dialog nobody answers.
    open_forever: std::sync::atomic::AtomicBool,
    gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
}
impl FakeInput {
    fn new(devices: &[&str]) -> Arc<Self> {
        let me = Self::default();
        *me.devices.lock().unwrap() = devices.iter().map(ToString::to_string).collect();
        Arc::new(me)
    }
    /// Delivers a block the way a device callback would (the clock is already where it should be).
    fn block(&self, samples: &[f32]) {
        let sink = self.sink.lock().unwrap().clone();
        if let Some(sink) = sink {
            sink(samples, 1000);
        }
    }
    fn release_opens(&self) {
        *self.gate.0.lock().unwrap() = true;
        self.gate.1.notify_all();
    }
    fn open_count(&self) -> usize {
        self.open_now.load(Ordering::SeqCst)
    }
}
impl AudioInput for FakeInput {
    fn devices(&self) -> Result<Vec<String>, AudioError> {
        Ok(self.devices.lock().unwrap().clone())
    }
    fn open(
        &self,
        device: Option<&str>,
        samples: SampleSink,
        errors: ErrorSink,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        if self.open_forever.load(Ordering::SeqCst) {
            let mut released = self.gate.0.lock().unwrap();
            while !*released {
                released = self.gate.1.wait(released).unwrap();
            }
        }
        let delay = *self.open_delay.lock().unwrap();
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        if let Some(error) = self.fail.lock().unwrap().clone() {
            return Err(error);
        }
        self.opened.lock().unwrap().push(device.map(str::to_string));
        *self.sink.lock().unwrap() = Some(samples);
        *self.errors.lock().unwrap() = Some(errors);
        self.open_now.fetch_add(1, Ordering::SeqCst);
        self.opens_returned.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeStream(self.open_now.clone())))
    }
}

// -------------------------------------------------------------------------------------- rig

struct Rig {
    bus: EventBus,
    clock: Arc<FakeClock>,
    input: Arc<FakeInput>,
    saved: Arc<Mutex<Vec<SongLoudnessTake>>>,
    link: LightkeyLink,
    settings: Arc<Mutex<LinkSettings>>,
}

fn song(duration: u32) -> Song {
    Song {
        id: "item-1".into(),
        title: "Holy Forever".into(),
        duration_seconds: Some(duration),
        order: 1,
        service_sequence: None,
        is_generic: false,
        source_song_id: Some("song-1".into()),
        description: None,
        formatted_duration: None,
    }
}

fn at(wall: f64) -> DateTime<Utc> {
    DateTime::from_timestamp(wall.trunc() as i64, (wall.fract() * 1e9) as u32).unwrap()
}

impl Rig {
    async fn new(input: Arc<FakeInput>) -> Self {
        let clock = Arc::new(FakeClock {
            wall: Mutex::new(1_800_000_000.0),
            started: Instant::now(),
        });
        let settings = Arc::new(Mutex::new((
            true,
            true,
            "127.0.0.1".to_string(),
            LightkeySettings {
                api_url: "http://127.0.0.1:1".into(),
                ..LightkeySettings::default()
            },
        )));
        let provider_settings = settings.clone();
        let saved: Arc<Mutex<Vec<SongLoudnessTake>>> = Arc::default();
        let sink_saved = saved.clone();
        let sink: LoudnessSink = Arc::new(move |take| {
            sink_saved.lock().unwrap().push(take);
            Box::pin(async { Ok(()) })
        });
        let http_clock = clock.clone();
        let bus = EventBus::new();
        let link = LightkeyLink::new(
            bus.clone(),
            Arc::new(move || provider_settings.lock().unwrap().clone()),
            LinkOptions {
                loudness_sink: Some(sink),
                audio: Some(input.clone()),
                clock: clock.clone(),
                http_factory: Arc::new(move || Arc::new(FakeHttp(http_clock.clone()))),
                ..LinkOptions::default()
            },
        );
        Self {
            bus,
            clock,
            input,
            saved,
            link,
            settings,
        }
    }

    async fn armed(input: Arc<FakeInput>) -> Self {
        let rig = Self::new(input).await;
        rig.link.start().await;
        wait_for(|| rig.link.status().link == Link::Connected).await;
        rig.link.set_recording(true).await;
        assert!(rig.link.armed());
        rig
    }

    async fn publish(&self, kind: EventType, payload: TimerPayload) {
        self.bus
            .publish(new_event(kind, "t", EventPayload::Timer(payload)))
            .await;
    }

    /// The song starts at the clock's current second.
    async fn start(&self, s: &Song) {
        self.bus
            .publish(new_event(
                EventType::SongStarted,
                "t",
                EventPayload::Song {
                    song: s.clone(),
                    index: 0,
                    position_seconds: None,
                    observed_at: None,
                },
            ))
            .await;
        self.publish(
            EventType::TimerStarted,
            TimerPayload {
                duration_seconds: s.duration_seconds.unwrap_or(0),
                song: Some(s.clone()),
                started_at: Some(at(self.clock.get())),
                running: true,
                ..TimerPayload::default()
            },
        )
        .await;
    }

    /// `seconds` of constant `level` in 100 ms blocks, the clock moving with them.
    fn hear(&self, seconds: f64, level: f32) {
        for _ in 0..(seconds * 10.0).round() as usize {
            self.clock.set(self.clock.get() + 0.1);
            self.input.block(&[level; 100]);
        }
    }

    async fn stop(&self, s: &Song, position: f64) {
        self.publish(
            EventType::TimerStopped,
            TimerPayload {
                song: Some(s.clone()),
                position_seconds: Some(position),
                ..TimerPayload::default()
            },
        )
        .await;
    }

    async fn resume(&self, s: &Song, position: f64) {
        self.publish(
            EventType::TimerResumed,
            TimerPayload {
                duration_seconds: s.duration_seconds.unwrap_or(0),
                song: Some(s.clone()),
                position_seconds: Some(position),
                running: true,
                started_at: Some(at(self.clock.get())),
                ..TimerPayload::default()
            },
        )
        .await;
    }

    async fn seek(&self, s: &Song, position: f64) {
        self.publish(
            EventType::TimerSeeked,
            TimerPayload {
                duration_seconds: s.duration_seconds.unwrap_or(0),
                song: Some(s.clone()),
                position_seconds: Some(position),
                running: true,
                started_at: Some(at(self.clock.get())),
                ..TimerPayload::default()
            },
        )
        .await;
    }
}

/// Waits for an event the code under test will cause. The limit is a hang guard that only catches
/// a real hang: it was 3 s, which a congested runner (or 16 busy loops) out-waited.
async fn wait_for(check: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(120);
    while Instant::now() < end {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not reached in time");
}

fn db(loudness: &SongLoudness, slot: usize) -> f64 {
    byte_db(loudness.values[slot]).unwrap_or(f64::NAN)
}

// ------------------------------------------------------------------------------------ tests

#[tokio::test]
async fn a_blocked_audio_open_does_not_hold_up_the_song_start() {
    // A-01: the first open can sit on the microphone permission dialog. The song start must go on.
    let input = FakeInput::new(&["Built-in Microphone"]);
    *input.open_delay.lock().unwrap() = Duration::from_secs(4);
    let rig = Rig::armed(input.clone()).await;
    let started = Instant::now();
    rig.start(&song(10)).await;
    let waited = started.elapsed();
    assert!(
        waited < Duration::from_millis(2500),
        "song start waited {waited:?} for the audio input"
    );
    assert_eq!(
        rig.link
            .status()
            .message
            .as_deref()
            .map(|m| m.starts_with("Recording")),
        Some(true),
        "{:?}",
        rig.link.status().message
    );
    // The open finishes on its own and loudness starts then.
    wait_for(|| input.open_count() == 1).await;
}

#[tokio::test]
async fn an_audio_open_that_never_returns_never_holds_up_any_song() {
    // The microphone dialog is never answered: the open never returns. Every song start, stop and
    // the next song still go on within the documented wait (1.5 s) plus a generous margin, the
    // lights recording carries on, and the line says why there is no loudness.
    let input = FakeInput::new(&["Built-in Microphone"]);
    input.open_forever.store(true, Ordering::SeqCst);
    let rig = Rig::armed(input.clone()).await;
    for round in 0..3 {
        let started = Instant::now();
        rig.start(&song(10)).await;
        let waited = started.elapsed();
        assert!(
            waited < Duration::from_millis(4000),
            "round {round}: song start waited {waited:?} for an open that never returns"
        );
        let message = rig.link.status().message.unwrap_or_default();
        assert!(message.starts_with("Recording"), "round {round}: {message}");
        let stopped = Instant::now();
        rig.stop(&song(10), 5.0).await;
        assert!(
            stopped.elapsed() < Duration::from_millis(4000),
            "round {round}: stop waited {:?}",
            stopped.elapsed()
        );
    }
    assert_eq!(input.open_count(), 0, "nothing ever opened");
    // The dialog is finally answered: the stuck open finishes, nothing hangs. An input that
    // opens after the song it was opened for has stopped is not closed by the open itself: it
    // stays open until the next stop or poll reconciles the hub (see the PR for F-C1). So the
    // test does not wait for it to "close again" by itself, which is a race between the late
    // open and whichever queued reconcile runs last; it waits for the open to return and then
    // gives the hub its next stop, after which nothing may be open.
    input.release_opens();
    wait_for(|| input.opens_returned.load(Ordering::SeqCst) >= 1).await;
    if std::env::var("FC1_PROBE").is_ok() {
        eprintln!("PROBE open_count before the next stop: {}", input.open_count());
    }
    rig.stop(&song(10), 5.0).await;
    wait_for(|| input.open_count() == 0).await;
}

#[tokio::test]
async fn the_picker_does_not_wait_for_a_blocked_open_or_pile_up_opens() {
    use stagepilot_lights::audio::AudioHub;
    let input = FakeInput::new(&["Built-in Microphone"]);
    *input.open_delay.lock().unwrap() = Duration::from_secs(2);
    let clock = Arc::new(FakeClock {
        wall: Mutex::new(1_800_000_000.0),
        started: Instant::now(),
    });
    let hub = Arc::new(AudioHub::new(input.clone(), clock));
    let first = {
        let hub = hub.clone();
        tokio::task::spawn_blocking(move || hub.set_meter(true))
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    // While the first open is blocked, polls return at once and start no second open.
    let started = Instant::now();
    for _ in 0..20 {
        let _ = hub.set_meter(true);
        let _ = hub.snapshot();
    }
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "{:?}",
        started.elapsed()
    );
    first.await.unwrap();
    assert_eq!(
        input.opened.lock().unwrap().len(),
        1,
        "one open, not one per poll"
    );
}

#[tokio::test]
async fn windows_follow_the_song_clock_and_are_saved() {
    let input = FakeInput::new(&["Built-in Microphone"]);
    let rig = Rig::armed(input.clone()).await;
    let s = song(4);
    rig.start(&s).await;
    assert_eq!(
        input.open_count(),
        1,
        "the input opens when the song starts"
    );
    rig.hear(1.0, 0.1); // -20 dBFS for song 0..1
    rig.hear(3.0, 0.01); // -40 dBFS for song 1..4
    rig.stop(&s, 4.0).await; // the song's end: kept
    let saved = rig.saved.lock().unwrap().clone();
    assert_eq!(saved.len(), 1, "{:?}", rig.link.status().message);
    let l = &saved[0].loudness;
    assert_eq!(
        (saved[0].song_key.as_str(), l.step_seconds, l.takes),
        ("song-1", 0.25, 1)
    );
    assert_eq!(l.values.len(), 16);
    assert!(
        (0..4).all(|i| (db(l, i) + 20.0).abs() < 0.4),
        "{:?}",
        l.values
    );
    assert!(
        (4..16).all(|i| (db(l, i) + 40.0).abs() < 0.4),
        "{:?}",
        l.values
    );
    assert_eq!(
        input.open_count(),
        0,
        "the input closes when the recording ends"
    );
}

#[tokio::test]
async fn under_half_the_song_saves_nothing() {
    let input = FakeInput::new(&["Mic"]);
    let rig = Rig::armed(input.clone()).await;
    let s = song(20);
    rig.start(&s).await;
    rig.hear(4.0, 0.1);
    rig.stop(&s, 4.0).await;
    assert!(rig.saved.lock().unwrap().is_empty());
    assert!(
        rig.link.status().message.unwrap().starts_with("Paused"),
        "a short stop is only a pause"
    );
}

#[tokio::test]
async fn a_pause_is_a_gap_and_resume_continues_in_place() {
    let input = FakeInput::new(&["Mic"]);
    let rig = Rig::armed(input.clone()).await;
    let s = song(10);
    rig.start(&s).await;
    rig.hear(2.0, 0.1); // song 0..2
    rig.stop(&s, 2.0).await; // pause (under half)
    rig.hear(3.0, 0.5); // paused: loud room noise must not be recorded
    rig.resume(&s, 2.0).await;
    rig.hear(8.0, 0.1); // song 2..10
    rig.stop(&s, 10.0).await; // the end of the song
    let l = rig.saved.lock().unwrap()[0].loudness.clone();
    assert!(l.values[..24].iter().all(|v| *v != 0), "{:?}", l.values);
    assert!(
        l.values.iter().all(|v| db_ok(*v)),
        "the paused 0.5 (-6 dBFS) leaked: {:?}",
        l.values
    );
}

fn db_ok(v: u8) -> bool {
    byte_db(v).is_none_or(|d| d < -15.0)
}

/// Seeking forward skips a stretch: it is a gap (slots with no data), the take goes on.
#[tokio::test]
async fn seeking_forward_leaves_a_gap_and_the_take_goes_on() {
    let input = FakeInput::new(&["Mic"]);
    let rig = Rig::armed(input.clone()).await;
    let s = song(10);
    rig.start(&s).await;
    rig.hear(3.0, 0.1); // song 0..3
    rig.seek(&s, 7.0).await;
    assert_eq!(input.open_count(), 1, "the input stays open");
    rig.hear(3.0, 0.1); // song 7..10
    rig.stop(&s, 10.0).await;
    let l = rig.saved.lock().unwrap()[0].loudness.clone();
    assert!(l.values[..11].iter().all(|v| *v != 0), "{:?}", l.values);
    assert!(
        l.values[13..27].iter().all(|v| *v == 0),
        "the skipped stretch is empty"
    );
    assert!(l.values[29..].iter().all(|v| *v != 0), "{:?}", l.values);
}

/// Seeking back records the same stretch again: the latest recording of a slot wins.
#[tokio::test]
async fn seeking_back_replaces_what_was_heard_in_the_stretch() {
    let input = FakeInput::new(&["Mic"]);
    let rig = Rig::armed(input.clone()).await;
    let s = song(10);
    rig.start(&s).await;
    rig.hear(4.0, 0.5); // song 0..4, loud (-6 dBFS)
    rig.seek(&s, 2.0).await;
    rig.hear(8.0, 0.05); // song 2..10, quiet (-26 dBFS)
    rig.stop(&s, 10.0).await;
    let l = rig.saved.lock().unwrap()[0].loudness.clone();
    assert!(
        l.values[..7].iter().all(|v| !db_ok(*v)),
        "0..2 s stays loud"
    );
    assert!(
        l.values[10..].iter().all(|v| db_ok(*v)),
        "2..4 s is the later, quiet take: {:?}",
        l.values
    );
}

#[tokio::test]
async fn a_refused_microphone_never_breaks_the_lights_recording() {
    let input = FakeInput::new(&["Mic"]);
    *input.fail.lock().unwrap() = Some(AudioError::PermissionDenied);
    let rig = Rig::armed(input.clone()).await;
    let s = song(4);
    rig.start(&s).await;
    let status = rig.link.status();
    assert_eq!(
        status.recording_song.as_deref(),
        Some("Holy Forever"),
        "lights still record"
    );
    let message = status.message.unwrap();
    assert!(message.contains("Loudness is not recorded"), "{message}");
    assert!(
        message.contains("Microphone access was refused"),
        "{message}"
    );
    rig.clock.set(rig.clock.get() + 4.0);
    rig.stop(&s, 4.0).await;
    assert!(rig.saved.lock().unwrap().is_empty());
    let message = rig.link.status().message.unwrap();
    assert!(
        message.contains("Not saved: no fixture changes"),
        "the lights path still ran: {message}"
    );
    assert!(message.contains("Loudness not saved"), "{message}");
}

#[tokio::test]
async fn nothing_opens_while_record_is_off_or_no_song_runs() {
    let input = FakeInput::new(&["Mic"]);
    let rig = Rig::new(input.clone()).await;
    rig.link.start().await;
    wait_for(|| rig.link.status().link == Link::Connected).await;
    let s = song(4);
    rig.start(&s).await; // Record is not armed
    assert_eq!(input.open_count(), 0);
    rig.link.set_recording(true).await;
    assert_eq!(
        input.open_count(),
        0,
        "armed but no song running: still closed"
    );
    assert!(input.opened.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_saved_device_is_used_and_a_vanished_one_falls_back_to_the_default() {
    let input = FakeInput::new(&["Built-in Microphone", "Scarlett 2i2"]);
    let rig = Rig::armed(input.clone()).await;
    rig.settings.lock().unwrap().3.audio_device = Some("Scarlett 2i2".into());
    rig.link.settings_changed();
    let s = song(4);
    rig.start(&s).await;
    assert_eq!(
        input.opened.lock().unwrap().last().unwrap().as_deref(),
        Some("Scarlett 2i2")
    );
    rig.stop(&s, 1.0).await;

    // The interface is unplugged: the default input is opened and the picker says so.
    *input.devices.lock().unwrap() = vec!["Built-in Microphone".into()];
    rig.start(&s).await;
    assert_eq!(*input.opened.lock().unwrap().last().unwrap(), None);
    let info = rig.link.audio().unwrap().info();
    assert_eq!(info.selected.as_deref(), Some("Scarlett 2i2"));
    assert!(info.note.unwrap().contains("Using the default input"));
}

#[tokio::test]
async fn the_meter_opens_on_request_reads_levels_and_closes() {
    let input = FakeInput::new(&["Mic"]);
    let clock = Arc::new(FakeClock {
        wall: Mutex::new(100.0),
        started: Instant::now(),
    });
    let hub = AudioHub::new(input.clone(), clock.clone());
    assert_eq!(input.open_count(), 0);
    assert!(hub.info().level.is_none());
    assert!(hub.set_meter(true).is_none());
    assert_eq!(input.open_count(), 1);
    input.block(&[0.1; 200]);
    let level = hub.info().level.unwrap();
    assert!((level.rms_db.unwrap() + 20.0).abs() < 0.1);
    assert_eq!(level.value, level_byte(-20.0));
    // Switched off by itself when the picker stops asking.
    clock.set(110.0);
    assert!(!hub.info().metering);
    assert_eq!(input.open_count(), 0);
    // And on request.
    hub.set_meter(true);
    hub.set_meter(false);
    assert_eq!(input.open_count(), 0);
}

#[tokio::test]
async fn a_lost_device_is_reported_as_a_short_line() {
    let input = FakeInput::new(&["Mic"]);
    let clock = Arc::new(FakeClock {
        wall: Mutex::new(100.0),
        started: Instant::now(),
    });
    let hub = AudioHub::new(input.clone(), clock);
    hub.set_meter(true);
    let errors = input.errors.lock().unwrap().clone().unwrap();
    errors(AudioError::DeviceLost("Mic".into()));
    assert_eq!(
        hub.info().reason.as_deref(),
        Some("The audio input Mic was lost.")
    );
}

#[tokio::test]
async fn a_build_without_audio_says_why() {
    let clock = Arc::new(FakeClock {
        wall: Mutex::new(100.0),
        started: Instant::now(),
    });
    let hub = AudioHub::new(
        Arc::new(stagepilot_lights::audio::UnavailableInput(
            "No input here.".into(),
        )),
        clock,
    );
    let info = hub.info();
    assert!(!info.available);
    assert_eq!(info.reason.as_deref(), Some("No input here."));
    assert_eq!(hub.set_meter(true).as_deref(), Some("No input here."));
}
