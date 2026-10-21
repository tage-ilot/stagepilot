//! Stress shape from the earlier Python stress test, run through the Rust client against the
//! in-process fake Playback: many song changes, play/pause cycles and seeks.
//!
//! Prints one JSON line: latency ranges (command sent -> event seen, command sent -> heartbeat
//! state changed) and the number of poll failures (disconnects / last_error). The fake answers
//! instantly, so this isolates the client's own overhead; real Playback adds its heartbeat delay
//! (up to 1 s) on top of the heartbeat path, which is why `fast_transport` exists.
//! Usage: stress [rounds]
use stagepilot_playback::client::{Command, ConnectionOptions, PlaybackClient};
use stagepilot_playback::normalizer::EventType;
use stagepilot_playback::testkit::FakePlayback;
use std::time::{Duration, Instant};

fn range(v: &mut [f64]) -> serde_json::Value {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    serde_json::json!({
        "n": n,
        "min_ms": v[0], "median_ms": v[n / 2], "p95_ms": v[(n * 95 / 100).min(n - 1)], "max_ms": v[n - 1],
    })
}

async fn until(mut f: impl FnMut() -> bool) -> f64 {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(3), "timed out");
        tokio::time::sleep(Duration::from_micros(100)).await;
    }
    t0.elapsed().as_secs_f64() * 1000.0
}

#[tokio::main]
async fn main() {
    let rounds: usize = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let fake = FakePlayback::start().await;
    let mut c = PlaybackClient::start(
        ConnectionOptions {
            port: fake.port,
            host_override: Some("127.0.0.1".into()),
            history_capacity: 1_000_000,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    until(|| c.status().connected).await;
    let (mut play_ev, mut pause_ev, mut first_play_ev, mut seek_ev, mut select_hb) =
        (vec![], vec![], vec![], vec![], vec![]);
    let songs = [101i64, 102, 103];
    for i in 0..rounds {
        // Song change (stopped), then the first play after it.
        let song = songs[(i + 1) % 3];
        c.send(Command::SelectSong(song)).await.unwrap();
        select_hb.push(until(|| c.status().heartbeat.is_some_and(|h| h.song_id == song)).await);
        let seen = c.events().len();
        c.send(Command::Play(true)).await.unwrap();
        first_play_ev.push(
            until(|| {
                c.events()[seen.min(c.events().len())..]
                    .iter()
                    .any(|e| matches!(e.kind, EventType::SongStarted | EventType::SongResumed))
            })
            .await,
        );
        // Play/pause cycles and seeks while on this song.
        for _ in 0..3 {
            let seen = c.events().len();
            c.send(Command::Play(false)).await.unwrap();
            pause_ev.push(
                until(|| {
                    c.events()[seen.min(c.events().len())..]
                        .iter()
                        .any(|e| e.kind == EventType::SongPaused)
                })
                .await,
            );
            let seen = c.events().len();
            c.send(Command::Play(true)).await.unwrap();
            play_ev.push(
                until(|| {
                    c.events()[seen.min(c.events().len())..].iter().any(|e| {
                        e.kind == EventType::SongResumed || e.kind == EventType::SongStarted
                    })
                })
                .await,
            );
            let seen = c.events().len();
            c.send(Command::Seek(30.0 + i as f64)).await.unwrap();
            seek_ev.push(
                until(|| {
                    c.events()[seen.min(c.events().len())..]
                        .iter()
                        .any(|e| e.kind == EventType::Seek)
                })
                .await,
            );
        }
        c.send(Command::Play(false)).await.unwrap();
        until(|| c.status().heartbeat.is_some_and(|h| !h.playing)).await;
    }
    let st = c.status();
    println!(
        "{}",
        serde_json::json!({
            "rounds": rounds,
            "commands_sent": fake.received.lock().unwrap().len(),
            "connections": *fake.connections.lock().unwrap(),
            "poll_failures": u32::from(!st.connected) + u32::from(st.last_error.is_some()),
            "select_song_to_heartbeat": range(&mut select_hb),
            "first_play_after_song_change_to_event": range(&mut first_play_ev),
            "play_to_event": range(&mut play_ev),
            "pause_to_event": range(&mut pause_ev),
            "seek_to_event": range(&mut seek_ev),
        })
    );
    c.stop().await;
}
