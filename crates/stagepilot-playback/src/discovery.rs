//! Song-order discovery with length measurement: mirrors `PlaybackInputPlugin.discover_song_order`
//! and `_step` in `plugin.py` (the navigation/measurement core only; settings persistence,
//! mapper installation and HTTP status belong to the plugin glue).
//!
//! Python semantics kept: walk `Previous` to the first song, then for each song measure its
//! length and step `Next`; the end of the list is recognised by a step that does not change the
//! song while fresh heartbeats keep arriving (silence is NOT evidence of an end); revisiting a
//! song is a wrap and aborts; 200 steps is the safety limit; the original selection is restored
//! with `Previous` steps; any playing / disconnected / setlist-version change aborts with a
//! [`DiscoveryError`] and no result. A cancelled run makes a bounded best-effort walk back to
//! the original song and never selects or plays.

use crate::client::{Command, TransportPort};
use crate::find::CancelToken;
use crate::lengths::{measure_song_length, MeasureOptions};
use crate::normalizer::Heartbeat;
use std::time::Duration;
use tokio::time::Instant;

pub const DEFAULT_STEP_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_MAX_STEPS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryError {
    Conflict(String),
    Cancelled,
}

fn conflict<T>(msg: &str) -> Result<T, DiscoveryError> {
    Err(DiscoveryError::Conflict(msg.to_string()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveryOutcome {
    pub order: Vec<i64>,
    pub lengths: Vec<Option<f64>>,
    pub version: i64,
    /// Navigation commands plus measurement commands sent (Python `progress`).
    pub progress: usize,
}

#[derive(Clone)]
pub struct DiscoveryOptions {
    pub step_timeout: Duration,
    pub max_steps: usize,
    pub measure: bool,
    pub cancel: CancelToken,
    /// Live copy of the navigation + measurement command count (Python `progress`).
    pub progress: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    /// Live index of the song being measured (Python `discovery_song`).
    pub song: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        DiscoveryOptions {
            step_timeout: DEFAULT_STEP_TIMEOUT,
            max_steps: DEFAULT_MAX_STEPS,
            measure: true,
            cancel: CancelToken::new(),
            progress: None,
            song: None,
        }
    }
}

struct Walk<'a, P: TransportPort> {
    port: &'a P,
    opts: &'a DiscoveryOptions,
    steps: usize,
    progress: usize,
}

impl<P: TransportPort> Walk<'_, P> {
    fn publish_progress(&self) {
        if let Some(p) = &self.opts.progress {
            p.store(self.progress, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Python `_check_discovery`.
    fn check(&self, version: Option<i64>) -> Result<Heartbeat, DiscoveryError> {
        let status = self.port.status();
        let Some(hb) = status.heartbeat.filter(|_| status.connected) else {
            return conflict(
                "Playback is disconnected. Enable remote connections and connect first.",
            );
        };
        if hb.playing {
            return conflict("Stop Playback before discovering song order.");
        }
        let Some(v) = hb.setlist_version else {
            return conflict("Playback has not supplied a valid setlist version.");
        };
        if version.is_some_and(|expected| expected != v) {
            return conflict("Playback setlist changed during discovery; try again.");
        }
        Ok(hb)
    }

    /// Python `_step`: send Previous/Next and wait for the song to change.
    async fn step(&mut self, next: bool, version: i64) -> Result<i64, DiscoveryError> {
        let before = self.check(Some(version))?.song_id;
        if self.steps >= self.opts.max_steps {
            return conflict("Song order discovery exceeded the 200-step safety limit.");
        }
        let serial = self.port.heartbeat_serial();
        let command = if next {
            Command::Next
        } else {
            Command::Previous
        };
        self.port
            .send(command)
            .await
            .map_err(|e| DiscoveryError::Conflict(e.to_string()))?;
        self.progress += 1;
        self.publish_progress();
        self.steps += 1;
        let deadline = Instant::now() + self.opts.step_timeout;
        let mut rx = self.port.subscribe();
        loop {
            let current = self.check(Some(version))?;
            if current.song_id != before {
                return Ok(current.song_id);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                // At a non-wrapping end Playback keeps sending fresh, unchanged heartbeats.
                if self.port.heartbeat_serial() >= serial + 2
                    && Instant::now().saturating_duration_since(self.port.last_heartbeat_at())
                        < self.opts.step_timeout / 2
                {
                    return Ok(before);
                }
                return conflict("Playback heartbeat timed out during song order discovery.");
            }
            rx.borrow_and_update();
            let _ =
                tokio::time::timeout(remaining.min(Duration::from_millis(100)), rx.changed()).await;
        }
    }
}

pub async fn discover_song_order<P: TransportPort>(
    port: &P,
    opts: &DiscoveryOptions,
) -> Result<DiscoveryOutcome, DiscoveryError> {
    let mut walk = Walk {
        port,
        opts,
        steps: 0,
        progress: 0,
    };
    let start = walk.check(None)?;
    let original = start.song_id;
    let version = start.setlist_version.expect("checked by check()");
    let mut order: Vec<i64> = Vec::new();
    let result = run(&mut walk, original, version, &mut order).await;
    match result {
        Err(DiscoveryError::Cancelled) => {
            // Bounded, best-effort walk back with Previous/Next; never select or play.
            let _ = restore_after_cancel(&mut walk, original, version, &order).await;
            Err(DiscoveryError::Cancelled)
        }
        other => other,
    }
}

async fn run<P: TransportPort>(
    walk: &mut Walk<'_, P>,
    original: i64,
    version: i64,
    order: &mut Vec<i64>,
) -> Result<DiscoveryOutcome, DiscoveryError> {
    let cancelled = |w: &Walk<'_, P>| w.opts.cancel.is_cancelled();
    let mut current = original;
    let mut visited = vec![current];
    loop {
        if cancelled(walk) {
            return Err(DiscoveryError::Cancelled);
        }
        let previous = walk.step(false, version).await?;
        if previous == current {
            break;
        }
        if visited.contains(&previous) {
            return conflict("Playback navigation wrapped; order discovery aborted.");
        }
        visited.push(previous);
        current = previous;
    }
    order.push(current);
    let mut lengths: Vec<Option<f64>> = Vec::new();
    loop {
        if cancelled(walk) {
            return Err(DiscoveryError::Cancelled);
        }
        if let Some(song) = &walk.opts.song {
            song.store(order.len(), std::sync::atomic::Ordering::SeqCst);
        }
        let length = if walk.opts.measure {
            let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let counter = count.clone();
            let live = walk.opts.progress.clone();
            let base = walk.progress;
            let m = MeasureOptions {
                settle: walk.opts.step_timeout,
                on_command: Some(std::sync::Arc::new(move || {
                    let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    if let Some(p) = &live {
                        p.store(base + n, std::sync::atomic::Ordering::SeqCst);
                    }
                })),
                ..Default::default()
            };
            let r = measure_song_length(walk.port, &m, &walk.opts.cancel).await;
            walk.progress += count.load(std::sync::atomic::Ordering::SeqCst);
            match r {
                Ok(v) => v,
                Err(Cancelled) => return Err(DiscoveryError::Cancelled),
            }
        } else {
            None
        };
        lengths.push(length);
        walk.check(Some(version))?;
        let following = walk.step(true, version).await?;
        if following == current {
            break;
        }
        if order.contains(&following) {
            return conflict("Playback navigation wrapped; order discovery aborted.");
        }
        order.push(following);
        current = following;
    }
    let Some(at) = order.iter().position(|s| *s == original) else {
        return conflict("Original Playback selection was lost during discovery.");
    };
    let expected_back: Vec<i64> = order[at..order.len() - 1].iter().rev().copied().collect();
    for expected in expected_back {
        if walk.step(false, version).await? != expected {
            return conflict("Could not restore the original Playback selection.");
        }
    }
    walk.check(Some(version))?;
    Ok(DiscoveryOutcome {
        order: order.clone(),
        lengths,
        version,
        progress: walk.progress,
    })
}

async fn restore_after_cancel<P: TransportPort>(
    walk: &mut Walk<'_, P>,
    original: i64,
    version: i64,
    order: &[i64],
) -> Result<(), DiscoveryError> {
    // The cancel token stays set, so the walk uses a fresh option set for the steps.
    let selected = walk.check(Some(version))?.song_id;
    let go_previous = match (
        order.iter().position(|s| *s == original),
        order.iter().position(|s| *s == selected),
    ) {
        (Some(o), Some(s)) => s > o,
        _ => false,
    };
    for _ in 0..walk.opts.max_steps {
        let now = walk.check(Some(version))?.song_id;
        if now == original {
            break;
        }
        if walk.step(!go_previous, version).await? == now {
            break;
        }
    }
    Ok(())
}
