//! Port of `backend/tests/test_playback_resume_mapping.py`: Playback pause / resume / seek /
//! reconnect mapping end to end. Fake Playback (`stagepilot_playback::testkit`) -> real client ->
//! mapper -> real state service, through the server runtime in demo mode (demo song 1 lasts 281 s).
//!
//! Python drives time by hand (`tick(n)` emits n one-second heartbeats instantly). Here the fake
//! beats on request, so each step sets the fake's state, sends one heartbeat and waits for the
//! timer to settle. Every wait polls a condition and returns as soon as it holds (15 s ceiling),
//! so nothing depends on a heartbeat margin and a slow runner only makes a test slower.

use serde_json::json;
use stagepilot_core::settings::{RuntimeSettings, SettingsService};
use stagepilot_core::state::{TimerState, TimerStatus};
use stagepilot_playback::plugin::PluginOptions;
use stagepilot_playback::testkit::FakePlayback;
use stagepilot_server::Runtime;
use std::sync::Arc;
use std::time::Duration;

const SONG_ONE: u32 = 281;

struct Rig {
    runtime: Arc<Runtime>,
    fake: FakePlayback,
}

async fn until(what: &str, mut ready: impl FnMut() -> bool) {
    for _ in 0..1500 {
        if ready() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for: {what}");
}

/// Let queued observations drain: nothing in the plugin's queue is observable, so settle on the
/// store revision no longer moving for a few polls.
async fn settle(rt: &Runtime) {
    let mut last = rt.store.snapshot().revision;
    let mut quiet = 0;
    for _ in 0..1500 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let now = rt.store.snapshot().revision;
        quiet = if now == last { quiet + 1 } else { 0 };
        last = now;
        if quiet >= 8 {
            return;
        }
    }
}

impl Rig {
    async fn new() -> Rig {
        let service = SettingsService::ephemeral(RuntimeSettings::demo());
        let options = PluginOptions {
            step_timeout: Duration::from_secs(2),
            ..PluginOptions::default()
        };
        let runtime = Runtime::with_playback_options(Arc::new(service), false, options);
        runtime.start().await;
        let fake = FakePlayback::start().await;
        {
            let mut s = fake.state.lock().unwrap();
            s.songs = vec![101, 102, 103];
            s.song = 101;
            s.length = 400.0;
        }
        fake.auto_beat(Duration::from_millis(100));
        let rig = Rig { runtime, fake };
        let mut persistent = rig.runtime.settings_service.snapshot();
        persistent.playback_api.host = Some("127.0.0.1".into());
        persistent.playback_api.port = rig.fake.port;
        persistent.playback_api.auto_scan = false;
        persistent.integration_modes.midi_source = stagepilot_core::config::MidiSource::PlaybackApi;
        rig.runtime
            .playback
            .save_settings(persistent)
            .await
            .unwrap();
        until("connected", || rig.runtime.playback.connection().connected).await;
        rig.runtime.playback.discover_song_order().await.unwrap();
        until("order valid", || !rig.runtime.playback.mapper_stale()).await;
        assert_eq!(rig.runtime.playback.mapper_view().0, vec![101, 102, 103]);
        rig
    }

    fn timer(&self) -> TimerState {
        self.runtime.store.snapshot().timer
    }

    fn set(&self, f: impl FnOnce(&mut stagepilot_playback::testkit::FakeState)) {
        f(&mut self.fake.state.lock().unwrap());
    }

    /// Select song 1 and start its countdown, as a normal "song started" would.
    async fn begin_song_one(&self) {
        let (code, _) = self.dispatch("start_next").await;
        assert_eq!(code, 200);
        until("timer running", || {
            self.timer().status == TimerStatus::Running
        })
        .await;
    }

    async fn dispatch(&self, action: &str) -> (u16, serde_json::Value) {
        use axum::body::Body;
        use axum::extract::ConnectInfo;
        use axum::http::Request;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let router = stagepilot_server::build_app(
            self.runtime.clone(),
            stagepilot_server::AppOptions { web_root: None },
        );
        let mut request = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/actions/{action}"))
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(
            "127.0.0.1:1".parse::<std::net::SocketAddr>().unwrap(),
        ));
        let response = router.oneshot(request).await.unwrap();
        let code = response.status().as_u16();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (code, serde_json::from_slice(&bytes).unwrap_or(json!(null)))
    }

    /// `play_ten_seconds`: the song starts on Playback and has played ten seconds.
    async fn play_ten_seconds(&self) {
        self.begin_song_one().await;
        self.set(|s| {
            s.playing = true;
            s.position = 10.0;
        });
        self.fake.beat();
        settle(&self.runtime).await;
    }

    async fn pump(&self) {
        self.fake.beat();
        settle(&self.runtime).await;
    }

    async fn reconnected(&self, connections: u32) {
        until("reconnected", || {
            *self.fake.connections.lock().unwrap() >= connections
                && self.runtime.playback.connection().connected
        })
        .await;
        settle(&self.runtime).await;
    }
}

#[tokio::test]
async fn play_pause_play() {
    let r = Rig::new().await;
    r.play_ten_seconds().await;
    assert_eq!(r.timer().status, TimerStatus::Running);
    r.set(|s| s.playing = false);
    r.pump().await;
    let stopped = r.timer();
    assert_eq!(stopped.status, TimerStatus::Stopped);
    assert_eq!(stopped.position_seconds, Some(10.0));
    assert_eq!(stopped.remaining_seconds, Some(SONG_ONE - 10));
    r.set(|s| s.playing = true);
    r.pump().await;
    let running = r.timer();
    assert_eq!(running.status, TimerStatus::Running);
    assert_eq!(running.duration_seconds, Some(SONG_ONE - 10));
    assert_eq!(running.position_seconds, Some(10.0));
    r.runtime.stop().await;
}

#[tokio::test]
async fn pause_seek_forward_play() {
    let r = Rig::new().await;
    r.play_ten_seconds().await;
    r.set(|s| {
        s.playing = false;
    });
    r.pump().await;
    r.set(|s| s.position = 100.0);
    r.fake
        .send_raw(r#"{"waveformSeek":{"sequenceTime":100.0}}"#);
    r.pump().await;
    let held = r.timer();
    assert_eq!(
        held.status,
        TimerStatus::Stopped,
        "a seek while stopped never runs"
    );
    assert_eq!(held.position_seconds, Some(100.0));
    assert_eq!(held.remaining_seconds, Some(SONG_ONE - 100));
    r.set(|s| s.playing = true);
    r.pump().await;
    let running = r.timer();
    assert_eq!(running.status, TimerStatus::Running);
    assert_eq!(running.duration_seconds, Some(SONG_ONE - 100));
    r.runtime.stop().await;
}

#[tokio::test]
async fn seek_while_playing_keeps_running() {
    let r = Rig::new().await;
    r.play_ten_seconds().await;
    r.set(|s| s.position = 200.0);
    r.fake
        .send_raw(r#"{"waveformSeek":{"sequenceTime":200.0}}"#);
    r.pump().await;
    let running = r.timer();
    assert_eq!(running.status, TimerStatus::Running);
    assert_eq!(running.duration_seconds, Some(SONG_ONE - 200));
    assert_eq!(running.position_seconds, Some(200.0));
    r.runtime.stop().await;
}

#[tokio::test]
async fn seek_past_end_stops_with_zero_remaining() {
    let r = Rig::new().await;
    r.play_ten_seconds().await;
    r.set(|s| s.position = 300.0);
    r.fake
        .send_raw(r#"{"waveformSeek":{"sequenceTime":300.0}}"#);
    r.pump().await;
    let done = r.timer();
    assert_eq!(done.status, TimerStatus::Stopped);
    assert_eq!(done.remaining_seconds, Some(0));
    r.runtime.stop().await;
}

#[tokio::test]
async fn drop_and_return_on_the_same_song_aligns_the_timer() {
    let r = Rig::new().await;
    r.play_ten_seconds().await;
    let before = *r.fake.connections.lock().unwrap();
    r.set(|s| s.position = 15.0); // Playback kept playing while StagePilot was disconnected
    r.fake.drop_connection();
    r.reconnected(before + 1).await;
    r.pump().await;
    let running = r.timer();
    assert_eq!(running.status, TimerStatus::Running);
    assert_eq!(
        running.position_seconds,
        Some(15.0),
        "aligned at the first heartbeat back"
    );
    assert_eq!(running.duration_seconds, Some(SONG_ONE - 15));
    assert!(
        !r.runtime.playback.mapper_stale(),
        "reconnect alone never invalidates the order"
    );
    r.runtime.stop().await;
}

#[tokio::test]
async fn return_on_a_different_song_is_ignored() {
    let r = Rig::new().await;
    r.play_ten_seconds().await;
    let before = r.timer();
    let connections = *r.fake.connections.lock().unwrap();
    r.set(|s| {
        s.song = 102;
        s.position = 30.0;
        s.playing = true;
    });
    r.fake.drop_connection();
    r.reconnected(connections + 1).await;
    r.pump().await;
    let after = r.timer();
    assert_eq!(after.status, before.status);
    assert_eq!(after.position_seconds, before.position_seconds);
    assert_eq!(after.duration_seconds, before.duration_seconds);
    r.runtime.stop().await;
}

#[tokio::test]
async fn stale_order_blocks_resume_and_seek_but_pause_still_stops() {
    let r = Rig::new().await;
    r.play_ten_seconds().await;
    r.set(|s| s.playing = false);
    r.pump().await;
    let held = r.timer();
    r.set(|s| s.version = 2); // the setlist was edited: the saved order is no longer valid
    r.pump().await;
    assert!(r.runtime.playback.mapper_stale());
    r.set(|s| {
        s.position = 100.0;
        s.playing = true;
    });
    r.fake
        .send_raw(r#"{"waveformSeek":{"sequenceTime":100.0}}"#);
    r.pump().await;
    let after = r.timer();
    assert_eq!(after.status, held.status);
    assert_eq!(after.position_seconds, held.position_seconds);
    // Pause still requests a stop even though the order is stale.
    r.set(|s| s.playing = false);
    r.pump().await;
    assert_eq!(
        r.runtime.store.snapshot().last_action.as_deref(),
        Some("stop_timer")
    );
    r.runtime.stop().await;
}

#[tokio::test]
async fn stall_leaves_the_timer_alone_then_realigns() {
    let r = Rig::new().await;
    r.play_ten_seconds().await;
    let before = r.timer();
    let connections = *r.fake.connections.lock().unwrap();
    r.fake.stall(true);
    until("disconnected by the stall", || {
        !r.runtime.playback.connection().connected
    })
    .await;
    settle(&r.runtime).await;
    let during = r.timer();
    assert_eq!(
        during.status, before.status,
        "a stall alone never changes the countdown"
    );
    assert_eq!(during.position_seconds, before.position_seconds);
    r.set(|s| s.position = 40.0);
    r.fake.stall(false);
    r.reconnected(connections + 1).await;
    r.pump().await;
    let running = r.timer();
    assert_eq!(running.status, TimerStatus::Running);
    assert_eq!(running.position_seconds, Some(40.0));
    r.runtime.stop().await;
}

#[tokio::test]
async fn a_heartbeat_without_a_song_id_does_not_map() {
    let r = Rig::new().await;
    r.play_ten_seconds().await;
    let before = r.timer();
    r.fake.send_raw(
        r#"{"heartbeat":{"stateData":{"sequenceTime":50,"sequencerPlayState":{"playing":{}},"padPlayerPlayState":{"stopped":{}},"setlistCloudVersion":1}}}"#,
    );
    r.pump().await;
    let timer = r.timer();
    assert_eq!(timer.status, TimerStatus::Running);
    assert_eq!(
        timer.position_seconds, before.position_seconds,
        "position 50 in the bad frame never mapped"
    );
    assert!(!r.runtime.playback.mapper_stale());
    r.runtime.stop().await;
}

#[tokio::test]
async fn connecting_while_not_playing_does_nothing() {
    let r = Rig::new().await;
    r.begin_song_one().await;
    let before = r.timer();
    assert_eq!(before.status, TimerStatus::Running);
    let connections = *r.fake.connections.lock().unwrap();
    r.set(|s| s.playing = false);
    r.fake.drop_connection();
    r.reconnected(connections + 1).await;
    r.pump().await;
    let after = r.timer();
    assert!(matches!(
        after.status,
        TimerStatus::Running | TimerStatus::Stopped
    ));
    r.runtime.stop().await;
}
