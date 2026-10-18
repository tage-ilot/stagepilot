//! Port of `test_playback_arbitration.py` (the arbiter and ArbitratedDispatcher halves; the
//! plugin/HTTP/MIDI-backend halves are phase 3b). Driven by a hand-set clock: no sleeping.

use stagepilot_playback::arbiter::*;
use std::sync::{Arc, Mutex};

fn arbiter(clock: &ManualClock) -> PlaybackSourceArbiter {
    PlaybackSourceArbiter::new(clock.clock())
}

fn stable_api(a: &mut PlaybackSourceArbiter, clock: &ManualClock) {
    for _ in 0..4 {
        a.observe_api(true, true);
        clock.advance(1.0);
    }
}

#[test]
fn single_owner_and_connection() {
    for (api, midi, owner) in [
        (false, false, Source::None),
        (true, false, Source::PlaybackApi),
        (false, true, Source::Midi),
        (true, true, Source::PlaybackApi),
    ] {
        let clock = ManualClock::new();
        let mut a = arbiter(&clock);
        a.midi_connected = midi;
        if api {
            stable_api(&mut a, &clock);
        }
        let r = a.snapshot();
        assert_eq!(r.active_source, owner, "api={api} midi={midi}");
        assert_eq!(r.connected, api || midi);
        assert_eq!(r.playback_api.connected, api);
        assert_eq!(r.midi.connected, midi);
    }
}

#[test]
fn immediate_failover_stable_failback_and_flapping() {
    let clock = ManualClock::new();
    let mut a = arbiter(&clock);
    a.midi_connected = true;
    stable_api(&mut a, &clock);
    assert!(a.allows(Source::PlaybackApi));
    a.observe_api(false, false);
    assert!(a.allows(Source::Midi));
    a.observe_api(true, true);
    assert!(a.allows(Source::Midi));
    clock.advance(1.0);
    a.observe_api(false, false);
    a.observe_api(true, true);
    assert!(a.allows(Source::Midi));
    stable_api(&mut a, &clock);
    assert!(a.allows(Source::PlaybackApi));
    clock.advance(5.0);
    assert!(a.allows(Source::Midi));
    assert!(!a.snapshot().playback_api.connected);
}

#[test]
fn single_heartbeat_is_not_stable_failback_and_midi_loss_does_not_wait() {
    let clock = ManualClock::new();
    let mut a = arbiter(&clock);
    a.midi_connected = true;
    a.observe_api(true, true);
    clock.set(3.0);
    assert!(a.allows(Source::Midi));
    a.midi_connected = false;
    assert!(a.allows(Source::PlaybackApi));
}

#[test]
fn a_gap_over_1_5_seconds_restarts_the_stability_clock() {
    let clock = ManualClock::new();
    let mut a = arbiter(&clock);
    a.midi_connected = true;
    a.observe_api(true, true); // t=0
    clock.set(1.4);
    a.observe_api(true, true); // 1.4 gap is fine: still stable since 0
    clock.set(3.0);
    a.observe_api(true, true); // 1.6 gap: stability restarts at 3.0
    assert!(a.allows(Source::Midi));
    for t in [4.0, 5.0, 5.9] {
        clock.set(t);
        a.observe_api(true, true);
    }
    assert!(a.allows(Source::Midi), "2.9 s of stability is not enough");
    clock.set(6.0);
    a.observe_api(true, true);
    assert!(
        a.allows(Source::PlaybackApi),
        "exactly 3 s stable with a fresh heartbeat wins"
    );
}

#[test]
fn midi_never_owns_when_the_user_chose_the_api() {
    let clock = ManualClock::new();
    let mut a = arbiter(&clock);
    a.midi_allowed = false;
    a.midi_connected = true; // even a forced MIDI signal cannot own in API mode
    assert_eq!(a.snapshot().active_source, Source::None);
    assert!(!a.snapshot().connected);
}

#[test]
fn revision_bumps_only_when_the_owner_changes() {
    let clock = ManualClock::new();
    let mut a = arbiter(&clock);
    let r0 = a.revision;
    a.snapshot();
    a.snapshot();
    assert_eq!(a.revision, r0);
    a.observe_api(true, true);
    assert_eq!(a.revision, r0 + 1);
    a.observe_api(true, true);
    assert_eq!(a.revision, r0 + 1);
    a.observe_api(false, false);
    assert_eq!(a.revision, r0 + 2);
}

#[test]
fn reasons_are_the_python_strings() {
    let clock = ManualClock::new();
    let mut a = arbiter(&clock);
    assert_eq!(a.snapshot().reason, "Disconnected.");
    assert_eq!(a.snapshot().midi.reason, "MIDI is disconnected.");
    a.observe_api(true, true);
    let s = a.snapshot();
    assert_eq!(s.reason, "Connected to Playback.");
    assert_eq!(s.playback_api.reason, "Playback is connected.");
    a.midi_connected = true;
    a.observe_api(false, false);
    assert_eq!(a.snapshot().reason, "Connected through MIDI.");
}

// ---- ArbitratedDispatcher ----------------------------------------------------------------------

#[derive(Clone, Default)]
struct Recorder {
    calls: Arc<Mutex<Vec<String>>>,
    accept: Arc<Mutex<bool>>,
}

impl Recorder {
    fn accepting() -> Self {
        Recorder {
            calls: Arc::default(),
            accept: Arc::new(Mutex::new(true)),
        }
    }
    fn outcome(&self) -> ActionOutcome {
        let accepted = *self.accept.lock().unwrap();
        ActionOutcome::new(accepted, if accepted { "accepted" } else { "no plan" })
    }
    fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl ActionDispatcher for Recorder {
    async fn dispatch(&self, action: ActionName, source: &str) -> ActionOutcome {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{}@{source}", action.as_str()));
        self.outcome()
    }
    async fn dispatch_song_position(&self, position: i64, source: &str) -> ActionOutcome {
        self.calls
            .lock()
            .unwrap()
            .push(format!("position {position}@{source}"));
        self.outcome()
    }
    async fn timer(&self, call: TimerCall, source: &str) -> ActionOutcome {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{call:?}@{source}"));
        self.outcome()
    }
}

struct State(Option<i64>);
impl StateView for State {
    async fn current_song_index(&self) -> Option<i64> {
        self.0
    }
}

fn dispatcher(
    index: Option<i64>,
) -> (
    ArbitratedDispatcher<Recorder, State>,
    Arc<Mutex<PlaybackSourceArbiter>>,
    ManualClock,
    Recorder,
) {
    let clock = ManualClock::new();
    let arb = Arc::new(Mutex::new(arbiter(&clock)));
    let rec = Recorder::accepting();
    let d = ArbitratedDispatcher::new(arb.clone(), rec.clone(), State(index));
    (d, arb, clock, rec)
}

#[tokio::test]
async fn cross_source_dedupe_and_expiry() {
    // test_cross_source_dedupe_and_expiry[position|restart|stop|start_next]
    for action in ["position", "restart", "stop", "start_next"] {
        let (d, arb, clock, rec) = dispatcher(Some(0));
        arb.lock().unwrap().observe_api(true, true);
        let api = match action {
            "position" | "start_next" => d.dispatch_song_position(1, "playback_api").await,
            "stop" => d.dispatch(ActionName::StopTimer, "playback_api").await,
            _ => d.dispatch(ActionName::RestartCurrent, "playback_api").await,
        };
        assert!(api.accepted, "{action}");
        {
            let mut a = arb.lock().unwrap();
            a.midi_connected = true;
            a.observe_api(false, false);
        }
        let midi = match action {
            "position" => d.dispatch_song_position(1, "midi_playback").await,
            "restart" => {
                d.dispatch(ActionName::RestartCurrent, "midi_playback")
                    .await
            }
            "stop" => d.dispatch(ActionName::StopTimer, "midi_playback").await,
            _ => d.dispatch(ActionName::StartNext, "midi_playback").await,
        };
        assert!(
            !midi.accepted && midi.message.contains("duplicate cross-source"),
            "{action}: {midi:?}"
        );
        clock.advance(2.0);
        assert!(
            d.dispatch_song_position(1, "midi_playback").await.accepted,
            "{action}"
        );
        assert_eq!(rec.count(), 2);
    }
}

#[tokio::test]
async fn rejected_action_does_not_poison_failover() {
    let (d, arb, _clock, rec) = dispatcher(Some(0));
    arb.lock().unwrap().observe_api(true, true);
    *rec.accept.lock().unwrap() = false;
    d.dispatch_song_position(1, "playback_api").await;
    {
        let mut a = arb.lock().unwrap();
        a.midi_connected = true;
        a.observe_api(false, false);
    }
    *rec.accept.lock().unwrap() = true;
    assert!(d.dispatch_song_position(1, "midi_playback").await.accepted);
}

#[tokio::test]
async fn api_mode_midi_events_are_never_accepted() {
    // test_api_mode_midi_events_are_never_accepted
    let (d, arb, _clock, rec) = dispatcher(Some(0));
    {
        let mut a = arb.lock().unwrap();
        a.midi_allowed = false;
        a.midi_connected = true;
    }
    let r = d.dispatch_song_position(1, "midi_playback").await;
    assert!(
        !r.accepted && r.message.contains("not the selected"),
        "{r:?}"
    );
    assert_eq!(rec.count(), 0);
}

#[tokio::test]
async fn api_down_with_midi_not_allowed_ignores_api_actions_without_midi_fallback() {
    // test_api_mode_playback_down_stays_not_connected_without_midi (arbiter half)
    let (d, arb, _clock, rec) = dispatcher(Some(0));
    arb.lock().unwrap().midi_allowed = false;
    let r = d.dispatch(ActionName::StopTimer, "playback_api").await;
    assert!(!r.accepted);
    assert_eq!(r.message, "ignored: MIDI active");
    assert_eq!(rec.count(), 0);
}

#[tokio::test]
async fn midi_mode_midi_works_and_api_is_ignored() {
    // test_midi_mode_midi_works_and_api_is_ignored (arbiter half)
    let (d, arb, _clock, rec) = dispatcher(Some(0));
    {
        let mut a = arb.lock().unwrap();
        a.midi_connected = true;
    }
    let api = d.dispatch(ActionName::StopTimer, "playback_api").await;
    assert_eq!(
        (api.accepted, api.message.as_str()),
        (false, "ignored: MIDI active")
    );
    assert!(
        d.dispatch(ActionName::StopTimer, "midi_playback")
            .await
            .accepted
    );
    assert_eq!(rec.count(), 1);
}

#[tokio::test]
async fn api_owner_ignores_midi_with_the_python_message() {
    let (d, arb, _clock, _rec) = dispatcher(Some(0));
    arb.lock().unwrap().observe_api(true, true);
    let r = d.dispatch(ActionName::StopTimer, "midi_playback").await;
    assert_eq!(r.message, "ignored: Playback API active");
}

#[tokio::test]
async fn midi_simulation_bypasses_the_arbiter_when_the_api_does_not_own() {
    let (d, _arb, _clock, rec) = dispatcher(Some(0));
    assert!(
        d.dispatch(ActionName::StopTimer, "midi.simulation")
            .await
            .accepted
    );
    assert!(d.seek_timer(5.0, "midi.simulation").await.accepted);
    assert_eq!(
        rec.calls.lock().unwrap().clone(),
        ["stop_timer@midi.simulation", "Seek(5.0)@midi.simulation"]
    );
}

#[tokio::test]
async fn suppressed_during_song_order_discovery() {
    let (mut d, arb, _clock, rec) = dispatcher(Some(0));
    arb.lock().unwrap().observe_api(true, true);
    d.suppressed = Box::new(|| true);
    let r = d.dispatch(ActionName::StopTimer, "playback_api").await;
    assert_eq!(r.message, "ignored: song order discovery active");
    assert_eq!(rec.count(), 0);
}

#[tokio::test]
async fn song_order_change_between_events_drops_a_start() {
    let (mut d, arb, _clock, rec) = dispatcher(Some(0));
    arb.lock().unwrap().observe_api(true, true);
    let revision = Arc::new(Mutex::new(0i64));
    let r2 = revision.clone();
    d.api_mapping_revision = Box::new(move || {
        let mut r = r2.lock().unwrap();
        *r += 1; // changes between the first and second read
        *r
    });
    let r = d.dispatch(ActionName::StartNext, "playback_api").await;
    assert_eq!(r.message, "ignored: Playback song order changed");
    assert_eq!(rec.count(), 0);
    // Other actions are unaffected by the mapping revision.
    assert!(
        d.dispatch(ActionName::StopTimer, "playback_api")
            .await
            .accepted
    );
}

#[tokio::test]
async fn start_next_key_is_the_next_song_position() {
    // From song index 2, start_next means "start position 4" and a MIDI start of 4 is a duplicate.
    let (d, arb, _clock, _rec) = dispatcher(Some(2));
    arb.lock().unwrap().observe_api(true, true);
    assert!(
        d.dispatch(ActionName::StartNext, "playback_api")
            .await
            .accepted
    );
    {
        let mut a = arb.lock().unwrap();
        a.midi_connected = true;
        a.observe_api(false, false);
    }
    let r = d.dispatch_song_position(4, "midi_playback").await;
    assert!(!r.accepted && r.message.contains("duplicate"));
}

#[tokio::test]
async fn start_next_is_blocked_by_any_recent_start_from_the_other_source() {
    // The extra `START_NEXT` rule: any other-source "start" within 2 s suppresses it.
    let (d, arb, _clock, _rec) = dispatcher(None);
    arb.lock().unwrap().observe_api(true, true);
    assert!(d.dispatch_song_position(7, "playback_api").await.accepted);
    {
        let mut a = arb.lock().unwrap();
        a.midi_connected = true;
        a.observe_api(false, false);
    }
    let r = d.dispatch(ActionName::StartNext, "midi_playback").await;
    assert!(
        !r.accepted && r.message.contains("duplicate cross-source"),
        "{r:?}"
    );
}

#[tokio::test]
async fn timer_calls_forward_their_position() {
    let (d, arb, _clock, rec) = dispatcher(Some(0));
    arb.lock().unwrap().observe_api(true, true);
    assert!(d.resume_timer_at(12.5, "playback_api").await.accepted);
    assert!(d.stop_timer_at(30.0, "playback_api").await.accepted);
    assert_eq!(
        rec.calls.lock().unwrap().clone(),
        ["Resume(12.5)@playback_api", "Stop(30.0)@playback_api"]
    );
}
