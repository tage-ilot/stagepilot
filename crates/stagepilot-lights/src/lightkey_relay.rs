//! Client for the `lightkey-api` OSC relay: subscribe, keep the subscription alive, measure the
//! clock offset, decode the stream into live fixture state. The rules are in
//! `crate::lightkey_fixtures`; this file is the UDP shell around them.
//!
//! The relay sends to the address that subscribed, over plain unicast UDP, so it works across the
//! wired router (multicast sACN does not). The socket is `connect`ed to the relay, so the kernel
//! drops datagrams from any other source.

use crate::lightkey::{FixtureLink, FixtureStatus};
use crate::lightkey_fixtures::{
    clock_sample, parse_packet, parse_pong, ClockEstimate, FixtureCommand, FixtureState, Joined,
    PacketKind, StreamTracker, RELAY_VERSION,
};
use rosc::{encoder, OscMessage, OscPacket, OscType};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::UdpSocket;

/// Renew the subscription this often. The relay drops a silent subscriber after 15 s.
pub const SUBSCRIBE_SECONDS: f64 = 5.0;
/// Subscribe this often while the relay has not answered yet (it may not be up).
pub const RETRY_SECONDS: f64 = 1.0;
/// A clock-ping burst this often, and at the start of every session.
pub const PING_REFRESH_SECONDS: f64 = 120.0;
const PING_BURST: u32 = 12;
const PING_GAP_SECONDS: f64 = 0.03;
/// No packet for this long while subscribed means the stream is lost. The relay says hello every
/// 2 s even when nothing changes.
pub const SILENCE_SECONDS: f64 = 6.0;
const TICK: Duration = Duration::from_millis(30);

/// Called with `(local_unix_seconds, changed_commands)` for every fixture change after the first
/// keyframe. `local_unix_seconds` is the DMX-change time on THIS computer's clock.
pub type FixtureSink = Arc<dyn Fn(f64, Vec<FixtureCommand>) + Send + Sync>;

#[derive(Debug)]
struct Shared {
    state: FixtureState,
    link: FixtureLink,
    detail: Option<String>,
    seen: bool,
    have_keyframe: bool,
    fixtures: u32,
    tracker: StreamTracker,
    clock: ClockEstimate,
    /// `agent_clock - our_clock`, seconds.
    offset: Option<f64>,
    last_rx: f64,
}

impl Shared {
    fn new() -> Self {
        Self {
            state: FixtureState::new(),
            link: FixtureLink::Waiting,
            detail: None,
            seen: false,
            have_keyframe: false,
            fixtures: 0,
            tracker: StreamTracker::new(),
            clock: ClockEstimate::new(),
            offset: None,
            last_rx: 0.0,
        }
    }
}

/// What the link reads. Cheap to clone.
#[derive(Clone)]
pub struct RelayClient {
    shared: Arc<Mutex<Shared>>,
}

impl Default for RelayClient {
    fn default() -> Self {
        Self::new()
    }
}

fn lock(m: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// `host:port` of the relay for a Lightkey API address (`http://10.30.0.211:8770`). The relay
/// listens on the same computer as the API.
pub fn relay_target(api_url: &str, port: u16) -> Option<String> {
    let rest = api_url.split_once("://").map_or(api_url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split_once(']').map(|(h, _)| h)?,
        None => authority.rsplit_once(':').map_or(authority, |(h, _)| h),
    };
    if host.is_empty() {
        return None;
    }
    Some(if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    })
}

fn message(addr: &str, args: Vec<OscType>) -> Vec<u8> {
    encoder::encode(&OscPacket::Message(OscMessage {
        addr: addr.into(),
        args,
    }))
    .unwrap_or_default()
}

impl RelayClient {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared::new())),
        }
    }

    /// The current fixture state (empty until the first keyframe).
    pub fn state(&self) -> FixtureState {
        lock(&self.shared).state.clone()
    }

    /// True once a keyframe arrived and packets are still coming.
    pub fn is_live(&self) -> bool {
        let s = lock(&self.shared);
        s.link == FixtureLink::Live && s.have_keyframe
    }

    /// `None` until the relay has answered once.
    pub fn status(&self, recording: bool) -> Option<FixtureStatus> {
        let s = lock(&self.shared);
        s.seen.then(|| FixtureStatus {
            link: s.link,
            detail: s.detail.clone(),
            fixtures: s.fixtures,
            lost_packets: u32::try_from(s.tracker.gaps()).unwrap_or(u32::MAX),
            recording,
        })
    }

    /// Forget the stream (settings changed, or the session ended).
    pub fn reset(&self) {
        *lock(&self.shared) = Shared::new();
    }

    fn off(&self) {
        let mut s = lock(&self.shared);
        s.link = FixtureLink::Waiting;
        s.detail = None;
    }

    /// Run one session against `target` until `keep_going` says to stop. Returns when the
    /// target changed, the link was stopped, or the socket failed.
    pub async fn session(
        &self,
        target: &str,
        wall: impl Fn() -> f64,
        mono: impl Fn() -> f64,
        keep_going: impl Fn() -> bool,
        sink: FixtureSink,
    ) {
        self.reset();
        let Ok(mut addrs) = tokio::net::lookup_host(target).await else {
            self.fail("Cannot find the Lightkey API host.");
            tokio::time::sleep(Duration::from_secs(2)).await;
            return;
        };
        let Some(addr) = addrs.next() else {
            self.fail("Cannot find the Lightkey API host.");
            tokio::time::sleep(Duration::from_secs(2)).await;
            return;
        };
        let bind = if addr.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let socket = match UdpSocket::bind(bind).await {
            Ok(s) => s,
            Err(_) => {
                self.fail("Cannot open the fixture stream.");
                tokio::time::sleep(Duration::from_secs(2)).await;
                return;
            }
        };
        if socket.connect(addr).await.is_err() {
            self.fail("Cannot open the fixture stream.");
            tokio::time::sleep(Duration::from_secs(2)).await;
            return;
        }
        let mut buffer = vec![0u8; 2048];
        let mut next_subscribe = 0.0_f64;
        let mut next_burst = 0.0_f64;
        let mut pings_left = 0u32;
        let mut next_ping = 0.0_f64;
        let mut ticker = tokio::time::interval(TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            if !keep_going() {
                let _ = socket.send(&message("/relay/unsubscribe", vec![])).await;
                self.off();
                return;
            }
            tokio::select! {
                _ = ticker.tick() => {
                    let now = mono();
                    if now >= next_subscribe {
                        // Until the relay has answered, knock every second; then renew calmly.
                        let answered = lock(&self.shared).have_keyframe;
                        next_subscribe = now + if answered { SUBSCRIBE_SECONDS } else { RETRY_SECONDS };
                        let _ = socket.send(&message("/relay/subscribe", vec![])).await;
                    }
                    if now >= next_burst {
                        next_burst = now + PING_REFRESH_SECONDS;
                        pings_left = PING_BURST;
                        lock(&self.shared).clock.reset();
                    }
                    if pings_left > 0 && now >= next_ping {
                        pings_left -= 1;
                        next_ping = now + PING_GAP_SECONDS;
                        let _ = socket.send(&message("/relay/ping", vec![OscType::Double(wall())])).await;
                    }
                    self.check_silence(now);
                }
                received = socket.recv(&mut buffer) => {
                    match received {
                        Ok(n) => self.on_datagram(&buffer[..n], wall(), mono(), &socket, &sink).await,
                        // The relay is not listening: the OS reports it on the next receive.
                        Err(_) => {
                            self.fail("The Lightkey API relay is not reachable.");
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                    }
                }
            }
        }
    }

    fn fail(&self, detail: &str) {
        let mut s = lock(&self.shared);
        s.link = FixtureLink::Lost;
        s.detail = Some(detail.into());
        s.seen = true;
    }

    fn check_silence(&self, now: f64) {
        let mut s = lock(&self.shared);
        if s.seen && s.link == FixtureLink::Live && now - s.last_rx > SILENCE_SECONDS {
            s.link = FixtureLink::Lost;
            s.detail = Some("No fixture data from the Lightkey API.".into());
        }
    }

    async fn on_datagram(
        &self,
        data: &[u8],
        wall: f64,
        mono: f64,
        socket: &UdpSocket,
        sink: &FixtureSink,
    ) {
        if let Some((t1, t2, t3)) = parse_pong(data) {
            let (round_trip, offset) = clock_sample(t1, t2, t3, wall);
            let mut s = lock(&self.shared);
            if s.clock.add(round_trip, offset) {
                s.offset = Some(offset);
            }
            return;
        }
        let Ok(packet) = parse_packet(data) else {
            return; // not for us; the port is on the network
        };
        let (events, want_keyframe) = {
            let mut s = lock(&self.shared);
            s.seen = true;
            s.last_rx = mono;
            if s.link != FixtureLink::Live || s.detail.is_some() {
                s.link = if s.have_keyframe {
                    FixtureLink::Live
                } else {
                    FixtureLink::Waiting
                };
                s.detail = None;
            }
            if let Some(hello) = &packet.hello {
                if hello.version != RELAY_VERSION {
                    s.link = FixtureLink::Lost;
                    s.detail = Some(format!(
                        "Lightkey API relay version {} is not supported.",
                        hello.version
                    ));
                    return;
                }
                s.fixtures = u32::try_from(hello.fixtures.max(0)).unwrap_or(u32::MAX);
                if s.tracker.on_hello(hello) {
                    s.state = FixtureState::new();
                    s.have_keyframe = false;
                    s.link = FixtureLink::Waiting;
                }
                // Hellos are numbered like any packet: a gap seen here asks for a keyframe too.
                let (_, lost) = s.tracker.on_packet(&packet);
                (Vec::new(), lost)
            } else {
                let (joined, lost) = s.tracker.on_packet(&packet);
                let mut events: Vec<(f64, Vec<FixtureCommand>)> = Vec::new();
                if let Joined::Complete {
                    kind,
                    time,
                    commands,
                } = joined
                {
                    let local = time - s.offset.unwrap_or(0.0);
                    match kind {
                        PacketKind::Keyframe => {
                            let first = !s.have_keyframe;
                            let changed = s.state.replace_with(&commands);
                            s.have_keyframe = true;
                            s.link = FixtureLink::Live;
                            // After the first keyframe, anything a keyframe changes was missed
                            // (a lost packet): hand it on, late, rather than lose it.
                            if !first && !changed.is_empty() {
                                events.push((local, changed));
                            }
                        }
                        PacketKind::Event => {
                            let changed = s.state.apply_all(&commands);
                            if s.have_keyframe && !changed.is_empty() {
                                events.push((local, changed));
                            }
                        }
                        PacketKind::Hello => {}
                    }
                }
                (events, lost)
            }
        };
        for (local, changed) in events {
            sink(local, changed);
        }
        if want_keyframe {
            let _ = socket.send(&message("/relay/keyframe", vec![])).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_from_api_url() {
        assert_eq!(
            relay_target("http://10.30.0.211:8770", 21700).as_deref(),
            Some("10.30.0.211:21700")
        );
        assert_eq!(
            relay_target("lights.local:8770", 21700).as_deref(),
            Some("lights.local:21700")
        );
        assert_eq!(
            relay_target("http://[fe80::1]:8770/x", 21700).as_deref(),
            Some("[fe80::1]:21700")
        );
        assert_eq!(relay_target("http://:8770", 21700), None);
        assert_eq!(relay_target("", 21700), None);
    }
}
