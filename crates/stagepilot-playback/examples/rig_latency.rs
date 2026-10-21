//! Rig latency probe. Usage:
//!   rig_latency <host> <port> listen <seconds>     read-only: heartbeat cadence, no commands
//!   rig_latency <host> <port> pass                 ONE controlled pass (see below)
//!
//! Safety (owner authorisation 2026-10-17, see the PR): at most 5 commands per second (a 250 ms
//! gap is enforced between ANY two sends), no loops, no retries. The pass:
//!   1 Fade(out) + MuteMidi(true)        silence and no MIDI before anything moves
//!   2 Play, Pause                       (start #1)  latency of play and pause
//!   3 Seek(10), Seek(0)                 seek while paused
//!   4 Next/Previous, Play, Pause        (start #2)  first play after a song change
//!   5 SelectSong(original), Seek(0)     restore the song
//!   6 MuteMidi(false), Fade(in)         restore; pad and loops are never touched
//! Refuses to run if Playback is not stopped at the start. If any step fails it jumps straight
//! to the restore commands. Prints one JSON object.

use stagepilot_playback::client::{Command, ConnectionOptions, Observer, PlaybackClient};
use stagepilot_playback::normalizer::Heartbeat;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MIN_GAP: Duration = Duration::from_millis(250);

#[derive(Clone, Default)]
struct Feed(Arc<Mutex<Vec<(Instant, Heartbeat)>>>);

struct Rig {
    client: PlaybackClient,
    feed: Feed,
    last_send: Instant,
    sent: Vec<String>,
}

impl Rig {
    fn latest(&self) -> Option<Heartbeat> {
        self.client.status().heartbeat
    }

    /// Send one command (rate limited) and return the time it was sent.
    async fn send(&mut self, c: Command) -> Result<Instant, String> {
        let wait = MIN_GAP.saturating_sub(self.last_send.elapsed());
        tokio::time::sleep(wait).await;
        let at = Instant::now();
        self.client.send(c).await.map_err(|e| e.to_string())?;
        self.last_send = Instant::now();
        self.sent.push(c.payload());
        Ok(at)
    }

    /// Seconds from `sent` until the first heartbeat published after it satisfies `pred`.
    async fn latency(&self, sent: Instant, pred: impl Fn(&Heartbeat) -> bool) -> Option<f64> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some((t, _)) = self
                .feed
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|(t, h)| *t >= sent && pred(h))
            {
                return Some(t.duration_since(sent).as_secs_f64());
            }
            if Instant::now() > deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }
}

fn json_f(v: Option<f64>) -> String {
    v.map_or("null".into(), |x| format!("{x:.3}"))
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).expect("host").clone();
    let port: u16 = args.get(2).expect("port").parse().expect("port");
    let mode = args.get(3).map(String::as_str).unwrap_or("listen");
    let feed = Feed::default();
    let f2 = feed.clone();
    let observer: Observer = Arc::new(move |status, _| {
        if let Some(h) = status.heartbeat {
            let mut v = f2.0.lock().unwrap();
            v.push((Instant::now(), h));
            if v.len() > 5000 {
                v.drain(..2500);
            }
        }
    });
    let opts = ConnectionOptions {
        host_override: Some(host),
        port,
        ..Default::default()
    };
    let client = PlaybackClient::start(opts, Some(observer)).expect("start");
    let t0 = Instant::now();
    while !client.status().connected {
        if t0.elapsed() > Duration::from_secs(20) {
            println!("{{\"error\":\"not connected in 20 s\"}}");
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let connect_s = t0.elapsed().as_secs_f64();
    let start = client.status().heartbeat.expect("heartbeat");
    let mut rig = Rig {
        client,
        feed: feed.clone(),
        last_send: Instant::now() - MIN_GAP,
        sent: vec![],
    };

    if mode == "listen" {
        let secs: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(10);
        tokio::time::sleep(Duration::from_secs(secs)).await;
        {
            let v = feed.0.lock().unwrap();
            let gaps: Vec<f64> = v
                .windows(2)
                .map(|w| w[1].0.duration_since(w[0].0).as_secs_f64())
                .collect();
            let mean = gaps.iter().sum::<f64>() / gaps.len().max(1) as f64;
            let max = gaps.iter().cloned().fold(0.0, f64::max);
            println!(
            "{{\"mode\":\"listen\",\"connect_s\":{connect_s:.3},\"heartbeats\":{},\"mean_gap_s\":{mean:.4},\"max_gap_s\":{max:.4},\"song\":{},\"position\":{:.2},\"playing\":{},\"pad\":{},\"commands_sent\":0}}",
            v.len(), start.song_id, start.position, start.playing, start.pad
        );
        }
        rig.client.stop().await;
        return;
    }

    if start.playing {
        println!("{{\"error\":\"Playback is playing; refusing to run\"}}");
        rig.client.stop().await;
        return;
    }
    let orig = start;
    let mut r: Vec<(&str, Option<f64>)> = Vec::new();
    let mut starts = 0;
    let mut failed: Option<String> = None;

    macro_rules! step {
        ($name:expr, $cmd:expr, $pred:expr) => {
            if failed.is_none() {
                match rig.send($cmd).await {
                    Ok(at) => {
                        let l = rig.latency(at, $pred).await;
                        if l.is_none() {
                            failed = Some(format!("no heartbeat change after {}", $name));
                        }
                        r.push(($name, l));
                    }
                    Err(e) => failed = Some(format!("{}: {e}", $name)),
                }
            }
        };
    }

    // 1 silence first (fade has no heartbeat field, so just allow it to settle)
    for c in [Command::Fade(1), Command::MuteMidi(true)] {
        if let Err(e) = rig.send(c).await {
            failed = Some(e);
        }
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    // 2 play / pause on the original song
    if failed.is_none() {
        starts += 1;
    }
    step!("play", Command::Play(true), |h: &Heartbeat| h.playing);
    tokio::time::sleep(Duration::from_millis(600)).await;
    step!("pause", Command::Play(false), |h: &Heartbeat| !h.playing);
    // 3 seek while paused
    step!("seek_10", Command::Seek(10.0), |h: &Heartbeat| (h.position
        - 10.0)
        .abs()
        < 1.0);
    step!("seek_0", Command::Seek(0.0), |h: &Heartbeat| h.position
        < 0.5);
    // 4 song change, then first play after the change
    let mut changed_song = None;
    if failed.is_none() {
        let o = orig.song_id;
        match rig.send(Command::Next).await {
            Ok(at) => {
                let l = rig.latency(at, move |h| h.song_id != o).await;
                changed_song = rig.latest().map(|h| h.song_id).filter(|s| *s != o);
                if changed_song.is_none() {
                    // at the end of the list: go the other way instead
                    match rig.send(Command::Previous).await {
                        Ok(at2) => {
                            let l2 = rig.latency(at2, move |h| h.song_id != o).await;
                            changed_song = rig.latest().map(|h| h.song_id).filter(|s| *s != o);
                            r.push(("navigate_previous", l2));
                        }
                        Err(e) => failed = Some(e),
                    }
                } else {
                    r.push(("navigate_next", l));
                }
            }
            Err(e) => failed = Some(e),
        }
    }
    if changed_song.is_some() && failed.is_none() {
        starts += 1;
        step!(
            "first_play_after_song_change",
            Command::Play(true),
            |h: &Heartbeat| h.playing
        );
        tokio::time::sleep(Duration::from_millis(600)).await;
        step!("pause_2", Command::Play(false), |h: &Heartbeat| !h.playing);
    }
    // 5 restore the song (also measures select)
    let o = orig.song_id;
    if rig.latest().is_some_and(|h| h.song_id != o) {
        match rig.send(Command::SelectSong(o)).await {
            Ok(at) => {
                let l = rig.latency(at, move |h| h.song_id == o).await;
                r.push(("select_original", l));
            }
            Err(e) => failed = Some(e),
        }
    }
    // 6 restore (always attempted, even after a failure; never play)
    if rig.latest().is_some_and(|h| h.playing) {
        let _ = rig.send(Command::Play(false)).await;
    }
    let _ = rig.send(Command::Seek(0.0)).await;
    let _ = rig.send(Command::MuteMidi(false)).await;
    let _ = rig.send(Command::Fade(0)).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let end = rig.latest();
    let restored = end.is_some_and(|h| {
        h.song_id == orig.song_id && !h.playing && h.pad == orig.pad && h.position < 0.5
    });
    let lat: Vec<String> = r
        .iter()
        .map(|(n, l)| format!("\"{n}\":{}", json_f(*l)))
        .collect();
    println!(
        "{{\"mode\":\"pass\",\"connect_s\":{connect_s:.3},\"latency_s\":{{{}}},\"song_starts\":{starts},\"commands_sent\":{},\"commands\":{:?},\"failed\":{},\"restored\":{restored},\"start_state\":{{\"song\":{},\"position\":{:.2},\"playing\":{},\"pad\":{}}},\"end_state\":{{\"song\":{},\"position\":{:.2},\"playing\":{},\"pad\":{}}}}}",
        lat.join(","),
        rig.sent.len(),
        rig.sent,
        failed.map_or("null".into(), |f| format!("{f:?}")),
        orig.song_id, orig.position, orig.playing, orig.pad,
        end.map_or(0, |h| h.song_id), end.map_or(0.0, |h| h.position),
        end.is_some_and(|h| h.playing), end.is_some_and(|h| h.pad),
    );
    rig.client.stop().await;
}
