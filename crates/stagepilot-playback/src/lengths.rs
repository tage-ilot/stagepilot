//! Playback-facing song-length measurement: mirrors `PlaybackClient.measure_song_length`
//! (client.py) and the length rounding of `planning_center_lengths.preview_plan`.
//!
//! Python semantics kept:
//! * only the selected, STOPPED song is touched; nothing is ever played, faded or selected;
//! * `transportReturnToStart` first (wait for a fresh heartbeat at ~0), then
//!   `waveformSeek(86400)` (wait for a fresh heartbeat past 0.5 s), then a second fresh
//!   heartbeat must agree within 0.01 s ("steady end"), otherwise there is no length;
//! * any change of song / setlist version / setlist id / connection / playing aborts the read;
//! * a failed measurement is `None`, independent of order discovery;
//! * whatever happens, a final `transportReturnToStart` is attempted and, if it cannot be
//!   confirmed, the result is `None`.
//!
//! Added on top (documented in the PR): [`Mode::Quick`] (one read, no steady-end confirmation),
//! and optional restoration of a non-zero original position.
//!
//! All waiting uses `tokio::time`, so tests run on a paused virtual clock.

use crate::client::{ClientError, Command, TransportPort};
use crate::discovery::Cancelled;
use crate::find::CancelToken;
use crate::normalizer::Heartbeat;
use std::time::Duration;
use tokio::time::Instant;

pub const SEEK_END_SECONDS: f64 = 86400.0;
/// Heartbeat position below this counts as "at the start" (Python `< 0.01`).
pub const AT_START: f64 = 0.01;
/// Heartbeat position above this counts as "moved to the end" (Python `> 0.5`).
pub const MOVED_TO_END: f64 = 0.5;
/// Two end reads closer than this are "steady".
pub const STEADY: f64 = 0.01;
/// Default settle time per wait (Python `settle=3.0`; the plugin passes its step timeout, 5 s).
pub const DEFAULT_SETTLE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Python behaviour: seek to the end, then require a second steady end heartbeat.
    #[default]
    Precise,
    /// One end read after the seek; no confirmation read (fewer heartbeats to wait for).
    Quick,
}

#[derive(Clone)]
pub struct MeasureOptions {
    pub settle: Duration,
    pub mode: Mode,
    /// After the final return to start, seek back to a non-zero original position.
    pub restore_position: bool,
    /// Called after every command that was sent (Python `on_command`).
    pub on_command: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl Default for MeasureOptions {
    fn default() -> Self {
        MeasureOptions {
            settle: DEFAULT_SETTLE,
            mode: Mode::Precise,
            restore_position: false,
            on_command: None,
        }
    }
}

/// Why a measurement step stopped. Every variant except `Cancelled` becomes "no length".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepError {
    /// "Playback changed during length measurement" (and the stopped/connected checks).
    Changed,
    Timeout,
    Client(ClientError),
}

impl From<ClientError> for StepError {
    fn from(e: ClientError) -> Self {
        StepError::Client(e)
    }
}

struct Ctx<'a, P: TransportPort> {
    port: &'a P,
    song: Heartbeat,
    setlist_id: Option<crate::normalizer::SetlistId>,
    generation: u64,
    opts: &'a MeasureOptions,
    cancel: &'a CancelToken,
}

impl<P: TransportPort> Ctx<'_, P> {
    /// The latest heartbeat, or `Changed` when anything about the selection moved.
    fn current(&self) -> Result<Heartbeat, StepError> {
        let status = self.port.status();
        match status.heartbeat {
            Some(hb)
                if status.connected
                    && self.port.generation() == self.generation
                    && !hb.playing
                    && hb.song_id == self.song.song_id
                    && hb.setlist_version == self.song.setlist_version
                    && status.setlist_id == self.setlist_id =>
            {
                Ok(hb)
            }
            _ => Err(StepError::Changed),
        }
    }

    /// Wait for a heartbeat NEWER than `serial` that satisfies `predicate`.
    async fn wait(
        &self,
        serial: u64,
        predicate: impl Fn(&Heartbeat) -> bool,
    ) -> Result<Result<Heartbeat, StepError>, Cancelled> {
        let mut rx = self.port.subscribe();
        let deadline = Instant::now() + self.opts.settle;
        loop {
            if self.cancel.is_cancelled() {
                return Err(Cancelled);
            }
            rx.borrow_and_update();
            match self.current() {
                Err(e) => return Ok(Err(e)),
                Ok(hb) if self.port.heartbeat_serial() > serial && predicate(&hb) => {
                    return Ok(Ok(hb))
                }
                Ok(_) => {}
            }
            // Wake on a new heartbeat, on the deadline, or (polled) on cancellation.
            let step =
                Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now()));
            if step.is_zero() {
                return Ok(Err(StepError::Timeout));
            }
            let _ = tokio::time::timeout(step, rx.changed()).await;
        }
    }

    async fn send(&self, command: Command) -> Result<u64, StepError> {
        self.current()?;
        let serial = self.port.heartbeat_serial();
        self.port.send(command).await?;
        if let Some(f) = &self.opts.on_command {
            f();
        }
        Ok(serial)
    }

    async fn at_start(&self) -> Result<Result<Heartbeat, StepError>, Cancelled> {
        let serial = match self.send(Command::ReturnToStart).await {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        self.wait(serial, |hb| hb.position < AT_START).await
    }
}

/// Measure the selected stopped song. `Ok(None)` is "could not measure" (Python `None`);
/// `Err(Cancelled)` only when `cancel` fired. In both cases the song is returned to the start
/// (or its original position) whenever the connection still allows it.
pub async fn measure_song_length<P: TransportPort>(
    port: &P,
    opts: &MeasureOptions,
    cancel: &CancelToken,
) -> Result<Option<f64>, Cancelled> {
    let initial = port.status();
    let Some(song) = initial.heartbeat else {
        return Ok(None);
    };
    if song.playing || !initial.connected {
        return Ok(None);
    }
    let ctx = Ctx {
        port,
        song,
        setlist_id: initial.setlist_id.clone(),
        generation: port.generation(),
        opts,
        cancel,
    };

    let mut result: Option<f64> = None;
    let mut cancelled = false;
    let measured: Result<Result<f64, StepError>, Cancelled> = async {
        if let Err(e) = ctx.at_start().await? {
            return Ok(Err(e));
        }
        let serial = match ctx.send(Command::SeekEnd).await {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let first = match ctx.wait(serial, |hb| hb.position > MOVED_TO_END).await? {
            Ok(h) => h,
            Err(e) => return Ok(Err(e)),
        };
        if opts.mode == Mode::Quick {
            return Ok(Ok(first.position));
        }
        let serial = port.heartbeat_serial();
        let second = match ctx.wait(serial, |hb| hb.position > MOVED_TO_END).await? {
            Ok(h) => h,
            Err(e) => return Ok(Err(e)),
        };
        Ok(if (first.position - second.position).abs() < STEADY {
            Ok(second.position)
        } else {
            Err(StepError::Changed)
        })
    }
    .await;
    match measured {
        Ok(Ok(v)) => result = Some(v),
        Ok(Err(_)) => {}
        Err(Cancelled) => cancelled = true,
    }

    // Always reset, even after a failure or cancellation (guarded by `current()`, so a changed
    // or playing selection is never touched). A cancelled token is ignored for the reset.
    let never = CancelToken::new();
    let reset_ctx = Ctx {
        cancel: &never,
        ..ctx
    };
    let reset_ok = matches!(reset_ctx.at_start().await, Ok(Ok(_)));
    if !reset_ok {
        result = None;
    } else if opts.restore_position && song.position >= AT_START {
        // Extension: put a non-zero original position back (Python leaves it at 0).
        let restored = async {
            let serial = reset_ctx.send(Command::Seek(song.position)).await?;
            reset_ctx
                .wait(serial, |hb| (hb.position - song.position).abs() < 0.5)
                .await
                .unwrap_or(Err(StepError::Timeout))
        }
        .await;
        if restored.is_err() {
            result = None;
        }
    }
    if cancelled {
        Err(Cancelled)
    } else {
        Ok(result)
    }
}

/// `math.floor(measured + 0.5)`: the whole second written to Planning Center.
pub fn round_length(measured: f64) -> i64 {
    (measured + 0.5).floor() as i64
}
