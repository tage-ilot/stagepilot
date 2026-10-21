//! Port of the Playback half of `test_playback_lengths.py` plus the discovery walk from
//! `plugin.discover_song_order`, against the in-process `SimPlayback` on tokio's PAUSED virtual
//! clock: nothing here sleeps for real, so slow shared runners cannot flake it.

use stagepilot_playback::client::{ClientError, Command, TransportPort};
use stagepilot_playback::discovery::*;
use stagepilot_playback::find::CancelToken;
use stagepilot_playback::lengths::*;
use stagepilot_playback::testkit::SimPlayback;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const SONGS: [i64; 3] = [101, 102, 103];

fn sim(index: usize, lengths: [Option<f64>; 3]) -> Arc<SimPlayback> {
    SimPlayback::new(SONGS.to_vec(), lengths.to_vec(), index)
}

fn opts() -> MeasureOptions {
    MeasureOptions {
        settle: Duration::from_secs(5),
        ..Default::default()
    }
}

async fn measure(s: &Arc<SimPlayback>, o: &MeasureOptions) -> Option<f64> {
    measure_song_length(s.as_ref(), o, &CancelToken::new())
        .await
        .unwrap()
}

#[tokio::test(start_paused = true)]
async fn measures_the_end_and_returns_to_start() {
    let s = sim(0, [Some(279.53), None, None]);
    let _hb = s.spawn_heartbeats(Duration::from_millis(100));
    assert_eq!(measure(&s, &opts()).await, Some(279.53));
    assert_eq!(s.snapshot().position, 0.0);
    assert_eq!(
        s.commands(),
        [
            "transportReturnToStart",
            "waveformSeek",
            "transportReturnToStart"
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_seek_with_no_effect_is_none_and_still_returns_to_start() {
    // test_no_effect_seek_saves_none: the seek never moves the playhead past 0.5 s.
    let s = sim(1, [Some(1.0), None, Some(260.17)]);
    let start = tokio::time::Instant::now();
    assert_eq!(measure(&s, &opts()).await, None);
    assert_eq!(s.snapshot().position, 0.0);
    // The wait for the end position ran out its settle time on the VIRTUAL clock.
    assert!(start.elapsed() >= Duration::from_secs(5));
    assert_eq!(s.commands().last().unwrap(), "transportReturnToStart");
}

#[tokio::test(start_paused = true)]
async fn quick_mode_reads_once_and_precise_mode_needs_a_steady_second_read() {
    let s = sim(0, [Some(200.0), None, None]);
    let q = MeasureOptions {
        mode: Mode::Quick,
        ..opts()
    };
    assert_eq!(measure(&s, &q).await, Some(200.0));
    // Unsteady end: quick still answers, precise refuses.
    let s = sim(0, [Some(200.0), None, None]);
    s.state.lock().unwrap().unsteady = true;
    let _hb = s.spawn_heartbeats(Duration::from_millis(100));
    assert_eq!(
        measure(&s, &opts()).await,
        None,
        "an end that keeps moving is not a length"
    );
    assert_eq!(s.snapshot().position, 0.0);
}

#[tokio::test(start_paused = true)]
async fn precise_waits_for_a_second_fresh_heartbeat() {
    let s = sim(0, [Some(200.0), None, None]);
    // No heartbeat after the first: the confirmation read times out -> None.
    let start = tokio::time::Instant::now();
    assert_eq!(measure(&s, &opts()).await, None);
    assert!(start.elapsed() >= Duration::from_secs(5));
    // With a heartbeat stream the same song measures.
    let s = sim(0, [Some(200.0), None, None]);
    let _hb = s.spawn_heartbeats(Duration::from_millis(100));
    assert_eq!(measure(&s, &opts()).await, Some(200.0));
}

#[tokio::test(start_paused = true)]
async fn refused_while_playing_and_nothing_is_sent() {
    // "commands are refused while playing": measurement does not even start.
    let s = sim(0, [Some(200.0), None, None]);
    s.state.lock().unwrap().playing = true;
    assert_eq!(measure(&s, &opts()).await, None);
    assert!(s.commands().is_empty());
    // And the transport itself refuses the four discovery commands while playing.
    for c in [
        Command::Previous,
        Command::Next,
        Command::SeekEnd,
        Command::ReturnToStart,
    ] {
        assert_eq!(s.send(c).await, Err(ClientError::NotStopped));
    }
    assert!(s.commands().is_empty());
}

#[tokio::test(start_paused = true)]
async fn disconnected_means_no_length_and_no_commands() {
    let s = sim(0, [Some(200.0), None, None]);
    s.disconnect();
    assert_eq!(measure(&s, &opts()).await, None);
    assert!(s.commands().is_empty());
}

#[tokio::test(start_paused = true)]
async fn playback_started_mid_measurement_aborts_and_touches_nothing_more() {
    let s = sim(0, [Some(200.0), None, None]);
    let s2 = s.clone();
    let on_command = Arc::new(AtomicUsize::new(0));
    let c = on_command.clone();
    let o = MeasureOptions {
        on_command: Some(Arc::new(move || {
            // After the first command, someone presses play.
            if c.fetch_add(1, Ordering::SeqCst) == 0 {
                s2.state.lock().unwrap().playing = true;
                s2.beat();
            }
        })),
        ..opts()
    };
    assert_eq!(measure(&s, &o).await, None);
    // Only the first return-to-start was sent: the reset is guarded by "still stopped".
    assert_eq!(s.commands(), ["transportReturnToStart"]);
}

#[tokio::test(start_paused = true)]
async fn song_or_setlist_change_mid_measurement_aborts() {
    for change in ["song", "version", "identity"] {
        let s = sim(0, [Some(200.0), None, None]);
        let s2 = s.clone();
        let n = Arc::new(AtomicUsize::new(0));
        let c = n.clone();
        let o = MeasureOptions {
            on_command: Some(Arc::new(move || {
                if c.fetch_add(1, Ordering::SeqCst) == 0 {
                    let mut st = s2.state.lock().unwrap();
                    match change {
                        "song" => st.index = 2,
                        "version" => st.version += 1,
                        _ => st.setlist_id = Some(999),
                    }
                    drop(st);
                    s2.beat();
                }
            })),
            ..opts()
        };
        assert_eq!(measure(&s, &o).await, None, "{change}");
        assert_eq!(
            s.commands(),
            ["transportReturnToStart"],
            "{change}: nothing sent to the changed selection"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn on_command_counts_every_command() {
    let s = sim(0, [Some(200.0), None, None]);
    let _hb = s.spawn_heartbeats(Duration::from_millis(100));
    let n = Arc::new(AtomicUsize::new(0));
    let c = n.clone();
    let o = MeasureOptions {
        on_command: Some(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })),
        ..opts()
    };
    measure(&s, &o).await;
    assert_eq!(n.load(Ordering::SeqCst), 3);
}

#[tokio::test(start_paused = true)]
async fn cancel_mid_measurement_restores_position_and_reports_cancelled() {
    // test_cancel_mid_measurement_saves_no_partial_lengths
    let s = sim(1, [None, Some(123.0), Some(234.0)]);
    let _hb = s.spawn_heartbeats(Duration::from_millis(100));
    let cancel = CancelToken::new();
    let c2 = cancel.clone();
    let o = MeasureOptions {
        on_command: Some(Arc::new(move || c2.cancel())),
        ..opts()
    };
    let r = measure_song_length(s.as_ref(), &o, &cancel).await;
    assert_eq!(r, Err(Cancelled));
    assert_eq!(s.snapshot().position, 0.0);
    assert_eq!(s.snapshot().index, 1);
    assert_eq!(s.commands().last().unwrap(), "transportReturnToStart");
}

#[tokio::test(start_paused = true)]
async fn restore_position_puts_a_nonzero_original_position_back() {
    let s = sim(0, [Some(200.0), None, None]);
    s.state.lock().unwrap().position = 42.0;
    s.beat();
    let _hb = s.spawn_heartbeats(Duration::from_millis(100));
    let o = MeasureOptions {
        restore_position: true,
        ..opts()
    };
    assert_eq!(measure(&s, &o).await, Some(200.0));
    assert_eq!(s.snapshot().position, 42.0);
    // Without the option the position is left at the start, like Python.
    let s = sim(0, [Some(200.0), None, None]);
    s.state.lock().unwrap().position = 42.0;
    s.beat();
    let _hb = s.spawn_heartbeats(Duration::from_millis(100));
    assert_eq!(measure(&s, &opts()).await, Some(200.0));
    assert_eq!(s.snapshot().position, 0.0);
}

#[test]
fn rounding_matches_python_floor_plus_half() {
    // math.floor(x + 0.5): halves round UP, unlike Rust's round-half-away only for negatives.
    for (x, want) in [
        (0.4, 0),
        (0.5, 1),
        (1.49, 1),
        (279.53, 280),
        (279.5, 280),
        (418.0, 418),
    ] {
        assert_eq!(round_length(x), want, "{x}");
    }
}

// ---- discovery walk ---------------------------------------------------------------------------

async fn discover(s: &Arc<SimPlayback>) -> Result<DiscoveryOutcome, DiscoveryError> {
    // The sim answers each command with a heartbeat and keeps beating, like the real thing.
    let _hb = s.spawn_heartbeats(Duration::from_millis(100));
    discover_song_order(s.as_ref(), &DiscoveryOptions::default()).await
}

#[tokio::test(start_paused = true)]
async fn discovery_returns_order_lengths_and_restores_the_selection() {
    // test_no_effect_seek_saves_none_and_order (the Playback half)
    for start in 0..3 {
        let s = sim(start, [Some(279.53), None, Some(260.17)]);
        let out = discover(&s).await.unwrap();
        assert_eq!(out.order, SONGS);
        assert_eq!(out.lengths, [Some(279.53), None, Some(260.17)]);
        assert!(out.progress >= 3 * SONGS.len(), "{}", out.progress);
        assert_eq!(s.snapshot().index, start, "original selection restored");
        assert_eq!(s.snapshot().position, 0.0);
        assert!(!s.snapshot().playing);
        // Only the four non-transport discovery commands were ever sent.
        for c in s.commands() {
            assert!(
                [
                    "transportPreviousSong",
                    "transportNextSong",
                    "waveformSeek",
                    "transportReturnToStart"
                ]
                .contains(&c.as_str()),
                "unexpected command {c}"
            );
        }
    }
}

#[tokio::test(start_paused = true)]
async fn discovery_never_plays_fades_or_selects() {
    // test_full_seek_scan_sends_no_propresenter_or_lights_commands (Playback half: the scan can
    // not start transport, which is what fires ProPresenter and lighting cues)
    let s = sim(1, [Some(100.0), Some(200.0), Some(300.0)]);
    discover(&s).await.unwrap();
    for c in s.commands() {
        assert!(
            !c.contains("Play")
                && !c.contains("Fade")
                && !c.contains("Select")
                && !c.contains("Pad"),
            "{c}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn discovery_without_measurement_sends_only_navigation() {
    let s = sim(0, [None, None, None]);
    let _hb = s.spawn_heartbeats(Duration::from_millis(100));
    let o = DiscoveryOptions {
        measure: false,
        ..Default::default()
    };
    let out = discover_song_order(s.as_ref(), &o).await.unwrap();
    assert_eq!(out.lengths, [None, None, None]);
    assert!(s
        .commands()
        .iter()
        .all(|c| c.starts_with("transport") && !c.contains("ReturnToStart")));
}

#[tokio::test(start_paused = true)]
async fn playing_disconnected_or_versionless_playback_is_refused_up_front() {
    let s = sim(0, [None, None, None]);
    s.state.lock().unwrap().playing = true;
    s.beat();
    assert_eq!(
        discover_song_order(s.as_ref(), &DiscoveryOptions::default()).await,
        Err(DiscoveryError::Conflict(
            "Stop Playback before discovering song order.".into()
        ))
    );
    let s = sim(0, [None, None, None]);
    s.disconnect();
    assert!(matches!(
        discover_song_order(s.as_ref(), &DiscoveryOptions::default()).await,
        Err(DiscoveryError::Conflict(m)) if m.contains("disconnected")
    ));
    assert!(s.commands().is_empty());
}

#[tokio::test(start_paused = true)]
async fn the_step_limit_is_a_hard_stop() {
    let s = sim(0, [None, None, None]);
    let _hb = s.spawn_heartbeats(Duration::from_millis(100));
    let o = DiscoveryOptions {
        max_steps: 2,
        measure: false,
        ..Default::default()
    };
    let r = discover_song_order(s.as_ref(), &o).await;
    assert!(matches!(r, Err(DiscoveryError::Conflict(m)) if m.contains("200-step safety limit")));
    assert!(s.commands().len() <= 2);
}

#[tokio::test(start_paused = true)]
async fn a_setlist_change_during_discovery_aborts_with_no_result() {
    let s = sim(0, [Some(10.0), Some(20.0), Some(30.0)]);
    let _hb = s.spawn_heartbeats(Duration::from_millis(100));
    let s2 = s.clone();
    let o = DiscoveryOptions::default();
    let bump = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        s2.state.lock().unwrap().version += 1;
        s2.beat();
    });
    let r = discover_song_order(s.as_ref(), &o).await;
    bump.await.unwrap();
    assert!(matches!(r, Err(DiscoveryError::Conflict(m)) if m.contains("setlist changed")));
}

#[tokio::test(start_paused = true)]
async fn a_silent_playback_is_an_error_not_an_end_of_list() {
    // "silence is NOT evidence of an end": no heartbeats after Previous -> conflict.
    let s = sim(1, [None, None, None]);
    s.state.lock().unwrap().deaf = true; // commands accepted, nothing changes, and no heartbeat
    let r = discover_song_order(s.as_ref(), &DiscoveryOptions::default()).await;
    assert!(matches!(r, Err(DiscoveryError::Conflict(m)) if m.contains("heartbeat timed out")));
}

#[tokio::test(start_paused = true)]
async fn the_end_of_the_list_needs_fresh_unchanged_heartbeats() {
    // Single-song setlist: Previous and Next both leave the song unchanged; fresh heartbeats
    // keep coming, so each is a recognised end and the order is just that song.
    let s = SimPlayback::new(vec![101], vec![Some(77.0)], 0);
    let out = discover(&s).await.unwrap();
    assert_eq!(out.order, [101]);
    assert_eq!(out.lengths, [Some(77.0)]);
}

#[tokio::test(start_paused = true)]
async fn cancel_mid_scan_walks_back_without_selecting_or_playing() {
    // test_cancel_mid_measurement_saves_no_partial_lengths (whole-scan form)
    for initial in 0..3usize {
        let s = sim(initial, [None, Some(123.0), Some(234.0)]);
        let _hb = s.spawn_heartbeats(Duration::from_millis(100));
        let cancel = CancelToken::new();
        let c2 = cancel.clone();
        let s2 = s.clone();
        let watcher = tokio::spawn(async move {
            // Cancel as soon as the first seek-to-end has been sent.
            loop {
                if s2.commands().iter().any(|c| c == "waveformSeek") {
                    c2.cancel();
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });
        let o = DiscoveryOptions {
            cancel,
            ..Default::default()
        };
        let r = discover_song_order(s.as_ref(), &o).await;
        watcher.await.unwrap();
        assert_eq!(r, Err(DiscoveryError::Cancelled), "initial={initial}");
        assert_eq!(s.snapshot().position, 0.0, "initial={initial}");
        assert_eq!(
            s.snapshot().index,
            initial,
            "initial={initial}: walked back to the original song"
        );
        assert!(s
            .commands()
            .iter()
            .all(|c| !c.contains("Select") && !c.contains("Play")));
    }
}
