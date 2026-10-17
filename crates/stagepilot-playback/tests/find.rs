//! Port of `test_playback_scan_diagnostics.py` (classification, diagnosed scan, block detection)
//! plus `find_playback` / `scan_candidates` behaviour from `find.py`. Everything runs on tokio's
//! paused virtual clock against a scripted [`Network`]: no sockets, no real waiting.

use stagepilot_playback::find::*;
use stagepilot_playback::normalizer::Heartbeat;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const HB: Heartbeat = Heartbeat {
    song_id: 101,
    position: 0.0,
    playing: false,
    pad: false,
    setlist_version: Some(1),
};

type Script = Arc<dyn Fn(&str) -> Result<Option<Heartbeat>, ProbeError> + Send + Sync>;

#[derive(Clone)]
struct MockNet {
    script: Script,
    nets: Vec<Ipv4Net>,
    gateway: Option<String>,
    loopback_ok: bool,
    connect: LanCheck,
    names: Vec<(String, String)>,
    probe_delay: Duration,
    calls: Arc<Mutex<Vec<String>>>,
    in_flight: Arc<Mutex<(usize, usize)>>,
    network_calls: Arc<Mutex<usize>>,
}

fn net(cidr: &str) -> Ipv4Net {
    let (a, p) = cidr.split_once('/').unwrap();
    Ipv4Net::new(a.parse().unwrap(), p.parse().unwrap()).unwrap()
}

impl MockNet {
    fn new(
        script: impl Fn(&str) -> Result<Option<Heartbeat>, ProbeError> + Send + Sync + 'static,
    ) -> Self {
        MockNet {
            script: Arc::new(script),
            nets: vec![net("192.0.2.0/29")], // 6 hosts
            gateway: Some("192.0.2.1".into()),
            loopback_ok: true,
            connect: LanCheck::Ok,
            names: vec![("192.0.2.2".into(), "en9".into())],
            probe_delay: Duration::ZERO,
            calls: Arc::default(),
            in_flight: Arc::default(),
            network_calls: Arc::default(),
        }
    }
}

impl Network for MockNet {
    async fn probe(
        &self,
        host: &str,
        _port: u16,
        _t: Duration,
    ) -> Result<Option<Heartbeat>, ProbeError> {
        self.calls.lock().unwrap().push(host.to_string());
        {
            let mut f = self.in_flight.lock().unwrap();
            f.0 += 1;
            f.1 = f.1.max(f.0);
        }
        if !self.probe_delay.is_zero() {
            tokio::time::sleep(self.probe_delay).await;
        }
        self.in_flight.lock().unwrap().0 -= 1;
        (self.script)(host)
    }
    async fn networks(&self) -> Vec<Ipv4Net> {
        *self.network_calls.lock().unwrap() += 1;
        self.nets.clone()
    }
    async fn resolve_name(&self, host: &str) -> String {
        host.to_string()
    }
    async fn gateway(&self) -> Option<String> {
        self.gateway.clone()
    }
    async fn selftest_loopback(&self) -> bool {
        self.loopback_ok
    }
    async fn selftest_connect(&self, _h: &str, _p: u16, _t: Duration) -> LanCheck {
        self.connect
    }
    async fn interface_names(&self) -> Vec<(String, String)> {
        self.names.clone()
    }
}

// errno values differ by platform; build them from the table the classifier itself uses.
#[cfg(target_os = "linux")]
mod e {
    pub const EPERM: i32 = 1;
    pub const EACCES: i32 = 13;
    pub const EHOSTUNREACH: i32 = 113;
    pub const ENETUNREACH: i32 = 101;
    pub const EIO: i32 = 5;
}
#[cfg(target_os = "macos")]
mod e {
    pub const EPERM: i32 = 1;
    pub const EACCES: i32 = 13;
    pub const EHOSTUNREACH: i32 = 65;
    pub const ENETUNREACH: i32 = 51;
    pub const EIO: i32 = 5;
}
#[cfg(windows)]
mod e {
    pub const EPERM: i32 = 10013; // WSAEACCES
    pub const EACCES: i32 = 10013;
    pub const EHOSTUNREACH: i32 = 10065;
    pub const ENETUNREACH: i32 = 10051;
    pub const EIO: i32 = 5;
}

fn os(code: i32) -> ProbeError {
    ProbeError::os(code)
}

fn timeout() -> Duration {
    Duration::from_millis(100)
}

async fn scan(n: &MockNet, typed: Option<&str>) -> ScanReport {
    scan_network(n, typed, 8080, true, timeout(), &ScanHooks::default()).await
}

// ---- classify_exception ------------------------------------------------------------------------

#[test]
fn each_error_maps_to_one_error_class() {
    // test_each_exception_maps_to_one_error_class
    let cases: Vec<(ProbeError, ErrorClass)> = vec![
        (os(e::EPERM), ErrorClass::PermissionDenied),
        (os(e::EACCES), ErrorClass::PermissionDenied),
        (
            ProbeError::PermissionDenied("denied".into()),
            ErrorClass::PermissionDenied,
        ),
        (os(e::EHOSTUNREACH), ErrorClass::NoRoute),
        (os(e::ENETUNREACH), ErrorClass::NoRoute),
        (ProbeError::Refused, ErrorClass::Refused),
        (ProbeError::TimedOut, ErrorClass::TimedOut),
        (ProbeError::Handshake("bad".into()), ErrorClass::NotPlayback),
        (ProbeError::Closed("closed".into()), ErrorClass::NotPlayback),
        (ProbeError::Unresolved, ErrorClass::InvalidAddress),
        (ProbeError::InvalidInput, ErrorClass::InvalidAddress),
        (
            ProbeError::Other {
                kind: "RuntimeError".into(),
                message: "boom".into(),
            },
            ErrorClass::UnexpectedError,
        ),
        (os(e::EIO), ErrorClass::UnexpectedError),
    ];
    for (err, expected) in cases {
        assert_eq!(classify_error(&err).0, expected, "{err:?}");
    }
}

#[test]
fn io_errors_classify_like_python_oserror_subclasses() {
    use std::io::{Error, ErrorKind};
    assert_eq!(
        classify_error(&ProbeError::from(Error::from(ErrorKind::ConnectionRefused))).0,
        ErrorClass::Refused
    );
    assert_eq!(
        classify_error(&ProbeError::from(Error::from(ErrorKind::TimedOut))).0,
        ErrorClass::TimedOut
    );
    assert_eq!(
        classify_error(&ProbeError::from(Error::from(ErrorKind::PermissionDenied))).0,
        ErrorClass::PermissionDenied
    );
    assert_eq!(
        classify_error(&ProbeError::from(Error::from_raw_os_error(e::EHOSTUNREACH))).0,
        ErrorClass::NoRoute
    );
    assert_eq!(
        classify_error(&ProbeError::from(Error::from_raw_os_error(e::EPERM))).0,
        ErrorClass::PermissionDenied
    );
}

#[test]
fn not_playback_names_the_failed_step() {
    // test_not_playback_names_the_failed_step
    assert_eq!(
        classify_error(&ProbeError::Handshake("x".into()))
            .2
            .as_deref(),
        Some("handshake")
    );
    assert_eq!(
        classify_error(&ProbeError::Closed("x".into())).2.as_deref(),
        Some("stream")
    );
    let failure = ProbeError::Failure {
        class: ErrorClass::NotPlayback,
        detail: "d".into(),
        step: Some("heartbeat".into()),
    };
    assert_eq!(classify_error(&failure).2.as_deref(), Some("heartbeat"));
}

#[test]
fn details_are_the_plain_python_strings() {
    assert_eq!(
        classify_error(&ProbeError::Refused).1,
        "The computer answered but nothing is listening on the Playback port"
    );
    assert_eq!(
        classify_error(&ProbeError::TimedOut).1,
        "No answer before the time limit"
    );
    assert_eq!(
        classify_error(&os(e::EHOSTUNREACH)).1,
        "No route to that address"
    );
    assert_eq!(
        classify_error(&ProbeError::InvalidInput).1,
        "That is not a valid address"
    );
}

#[tokio::test]
async fn probe_checked_raises_instead_of_returning_none() {
    // test_probe_checked_raises_instead_of_returning_none: a closed port is classified.
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let err = probe_checked("127.0.0.1", port, Duration::from_secs(5))
        .await
        .unwrap_err();
    let (class, _, _) = classify_error(&err);
    // Windows retries a refused loopback connect until the timeout; both are classified.
    assert!(
        matches!(class, ErrorClass::Refused | ErrorClass::TimedOut),
        "{err:?}"
    );
}

#[tokio::test]
async fn probe_checked_rejects_a_bad_typed_host_before_any_io() {
    let err = probe_checked("bad host/x", 8080, timeout())
        .await
        .unwrap_err();
    assert_eq!(classify_error(&err).0, ErrorClass::InvalidAddress);
}

// ---- scan_network ------------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn all_hosts_blocked_while_loopback_works_is_permission_denied() {
    // test_all_hosts_blocked_while_loopback_works_is_permission_denied
    let mut n = MockNet::new(|h| {
        if h == "127.0.0.1" {
            Ok(None)
        } else {
            Err(os(e::EPERM))
        }
    });
    n.connect = LanCheck::Blocked;
    let r = scan(&n, None).await;
    assert_eq!(
        (r.outcome, r.error_class),
        (ScanOutcome::Error, Some(ErrorClass::PermissionDenied))
    );
    assert_eq!(r.loopback_ok, Some(true));
    assert_eq!(r.lan_check, LanCheck::Blocked);
    assert_eq!(r.hosts_probed, 7);
    assert_eq!(r.error_counts[&ErrorClass::PermissionDenied], 6);
    assert_eq!(r.message.as_deref(), Some(SCAN_BLOCK_DETAIL));
}

#[tokio::test(start_paused = true)]
async fn timeouts_everywhere_with_working_network_is_plain_not_found() {
    let n = MockNet::new(|_| Ok(None));
    let r = scan(&n, None).await;
    assert_eq!(r.outcome, ScanOutcome::NotFound);
    assert_eq!(r.error_class, None);
    assert_eq!(r.hosts_probed, 7);
    assert_eq!(r.networks, vec!["192.0.2.0/29"]);
    assert_eq!(r.error_counts.get(&ErrorClass::TimedOut), Some(&6));
    assert_eq!(r.error_counts.len(), 1);
    assert_eq!(r.interfaces, vec!["en9"]);
}

#[tokio::test(start_paused = true)]
async fn all_unexpected_errors_are_reported_with_type_and_message() {
    let n = MockNet::new(|h| {
        if h == "127.0.0.1" {
            Ok(None)
        } else {
            Err(ProbeError::Other {
                kind: "RuntimeError".into(),
                message: "pool exploded".into(),
            })
        }
    });
    let r = scan(&n, None).await;
    assert_eq!(
        (r.outcome, r.error_class),
        (ScanOutcome::Error, Some(ErrorClass::UnexpectedError))
    );
    assert!(r.message.unwrap().contains("RuntimeError: pool exploded"));
    assert!(r.sample_errors.len() <= MAX_LOGGED_ERRORS);
}

#[tokio::test(start_paused = true)]
async fn found_on_lan_and_loopback_first() {
    let n = MockNet::new(|h| Ok((h == "192.0.2.3").then_some(HB)));
    let r = scan(&n, None).await;
    assert_eq!(n.calls.lock().unwrap()[0], "127.0.0.1");
    assert_eq!(
        r.candidates
            .iter()
            .map(|c| c.host.as_str())
            .collect::<Vec<_>>(),
        ["192.0.2.3"]
    );
    assert_eq!(r.outcome, ScanOutcome::Found);
    assert_eq!(r.candidates[0].source, DiscoverySource::Lan);
}

#[tokio::test(start_paused = true)]
async fn loopback_hit_never_scans_the_lan() {
    let n = MockNet::new(|h| Ok((h == "127.0.0.1").then_some(HB)));
    let r = scan(&n, None).await;
    assert_eq!(r.outcome, ScanOutcome::Found);
    assert_eq!(r.candidates[0].source, DiscoverySource::Loopback);
    assert_eq!(r.candidates[0].name, "127.0.0.1");
    assert_eq!(n.calls.lock().unwrap().len(), 1);
    assert_eq!(*n.network_calls.lock().unwrap(), 0);
}

#[tokio::test(start_paused = true)]
async fn no_networks_is_no_route() {
    let mut n = MockNet::new(|_| Ok(None));
    n.nets = vec![];
    let r = scan(&n, None).await;
    assert_eq!(
        (r.outcome, r.error_class),
        (ScanOutcome::Error, Some(ErrorClass::NoRoute))
    );
}

#[tokio::test(start_paused = true)]
async fn typed_host_is_one_direct_probe_with_specific_class_and_no_lan_scan() {
    let n = MockNet::new(|_| Err(ProbeError::Refused));
    let r = scan(&n, Some("192.0.2.50")).await;
    assert_eq!(*n.calls.lock().unwrap(), vec!["192.0.2.50"]);
    assert_eq!(
        *n.network_calls.lock().unwrap(),
        0,
        "a typed address must never enumerate networks"
    );
    assert_eq!(
        (r.outcome, r.error_class),
        (ScanOutcome::Error, Some(ErrorClass::Refused))
    );
    assert_eq!(r.hosts_total, 1);
    assert_eq!(r.typed_host.as_deref(), Some("192.0.2.50"));
}

#[tokio::test(start_paused = true)]
async fn typed_host_success_and_invalid() {
    let ok = scan(&MockNet::new(|_| Ok(Some(HB))), Some("192.0.2.50")).await;
    assert_eq!(ok.outcome, ScanOutcome::Found);
    assert_eq!(ok.candidates[0].source, DiscoverySource::Manual);
    let n = MockNet::new(|_| Ok(Some(HB)));
    let bad = scan(&n, Some("bad host/x")).await;
    assert_eq!(
        (bad.outcome, bad.error_class),
        (ScanOutcome::Error, Some(ErrorClass::InvalidAddress))
    );
    assert!(
        n.calls.lock().unwrap().is_empty(),
        "an invalid host is never probed"
    );
}

#[tokio::test(start_paused = true)]
async fn cancel_stops_scan() {
    let n = MockNet::new(|_| Ok(None));
    let hooks = ScanHooks::default();
    hooks.cancel.cancel();
    let r = scan_network(&n, None, 8080, true, timeout(), &hooks).await;
    assert_eq!(r.outcome, ScanOutcome::Cancelled);
}

#[tokio::test(start_paused = true)]
async fn cancel_between_chunks_ends_the_scan_early() {
    // 3 x /24 = 762 hosts = 12 chunks of 64; cancel after the first chunk completes.
    let mut n = MockNet::new(|_| Ok(None));
    n.nets = vec![
        net("192.0.2.0/24"),
        net("198.51.100.0/24"),
        net("203.0.113.0/24"),
    ];
    let hooks = ScanHooks::default();
    let token = hooks.cancel.clone();
    let hooks = ScanHooks {
        progress: Some(Arc::new(move |_, _| token.cancel())),
        ..hooks
    };
    let r = scan_network(&n, None, 8080, true, timeout(), &hooks).await;
    assert_eq!(r.outcome, ScanOutcome::Cancelled);
    assert_eq!(r.hosts_probed, 1 + 64);
}

#[tokio::test(start_paused = true)]
async fn progress_and_phase_callbacks_report_real_counts() {
    let n = MockNet::new(|_| Ok(None));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let phases = Arc::new(Mutex::new(Vec::new()));
    let (s, p) = (seen.clone(), phases.clone());
    let hooks = ScanHooks {
        cancel: CancelToken::new(),
        progress: Some(Arc::new(move |c, t| s.lock().unwrap().push((c, t)))),
        on_phase: Some(Arc::new(move |name, _, _| {
            p.lock().unwrap().push(name.to_string())
        })),
    };
    scan_network(&n, None, 8080, true, timeout(), &hooks).await;
    assert_eq!(*seen.lock().unwrap().last().unwrap(), (6, 6));
    assert_eq!(phases.lock().unwrap()[0], "This computer");
}

// ---- block-signature classification ------------------------------------------------------------

fn big() -> Vec<Ipv4Net> {
    vec![net("192.0.2.0/24"), net("198.51.100.0/24")] // 508 hosts
}

fn beta29(host: &str) -> Result<Option<Heartbeat>, ProbeError> {
    if host == "127.0.0.1" {
        return Ok(None);
    }
    if matches!(host, "192.0.2.5" | "192.0.2.6" | "198.51.100.5") {
        return Err(ProbeError::Refused); // this Mac's own addresses
    }
    if host == "192.0.2.9" {
        return Err(ProbeError::Handshake("not playback".into()));
    }
    let last: u32 = host.rsplit('.').next().unwrap().parse().unwrap();
    Err(os(if last != 0 { e::EHOSTUNREACH } else { e::EPERM }))
}

fn beta29_net() -> MockNet {
    let mut n = MockNet::new(beta29);
    n.nets = big();
    n.connect = LanCheck::Unknown;
    n.gateway = None;
    n.names = vec![
        ("192.0.2.5".into(), "en0".into()),
        ("192.0.2.6".into(), "en1".into()),
        ("198.51.100.5".into(), "en2".into()),
    ];
    n
}

#[tokio::test(start_paused = true)]
async fn beta29_numbers_are_permission_denied_even_with_lan_check_unknown() {
    let r = scan(&beta29_net(), None).await;
    assert_eq!(r.error_counts[&ErrorClass::NoRoute], 504);
    assert_eq!(r.error_counts[&ErrorClass::Refused], 3);
    assert_eq!(
        (r.outcome, r.error_class),
        (ScanOutcome::Error, Some(ErrorClass::PermissionDenied))
    );
    assert_eq!(r.classification, Some(ErrorClass::PermissionDenied));
    assert_eq!(r.classification_inputs["others_timed_out"], 0);
}

#[tokio::test(start_paused = true)]
async fn real_miss_with_timeouts_stays_not_found() {
    let mut n = MockNet::new(|_| Ok(None));
    n.nets = big();
    let r = scan(&n, None).await;
    assert_eq!((r.outcome, r.error_class), (ScanOutcome::NotFound, None));
    assert_eq!(r.classification, None);
}

#[tokio::test(start_paused = true)]
async fn mixed_timeouts_and_no_route_is_not_a_block() {
    let mut n = MockNet::new(|h| {
        if h.ends_with(".7") || h == "127.0.0.1" {
            Ok(None)
        } else {
            Err(os(e::EHOSTUNREACH))
        }
    });
    n.nets = big();
    n.names = vec![];
    let r = scan(&n, None).await;
    assert_eq!((r.error_class, r.outcome), (None, ScanOutcome::NotFound));
}

#[tokio::test(start_paused = true)]
async fn refused_only_is_not_a_block() {
    let n = MockNet::new(|h| {
        if h == "127.0.0.1" {
            Ok(None)
        } else {
            Err(ProbeError::Refused)
        }
    });
    let r = scan(&n, None).await;
    assert_eq!(r.outcome, ScanOutcome::NotFound);
    assert_eq!(r.classification, None);
}

#[tokio::test(start_paused = true)]
async fn typed_host_with_gateway_also_failing_is_permission_denied() {
    let mut n = MockNet::new(|_| Err(os(e::EHOSTUNREACH)));
    n.connect = LanCheck::Blocked;
    let r = scan(&n, Some("192.0.2.50")).await;
    assert_eq!(
        (r.outcome, r.error_class),
        (ScanOutcome::Error, Some(ErrorClass::PermissionDenied))
    );
    assert_eq!(r.classification, Some(ErrorClass::PermissionDenied));
    assert_eq!(r.lan_check, LanCheck::Blocked);
    assert!(r
        .message
        .unwrap()
        .contains("macOS is blocking StagePilot's local-network access"));
}

#[tokio::test(start_paused = true)]
async fn typed_host_no_route_but_gateway_fine_stays_no_route() {
    let n = MockNet::new(|_| Err(os(e::EHOSTUNREACH)));
    let r = scan(&n, Some("192.0.2.50")).await;
    assert_eq!(r.error_class, Some(ErrorClass::NoRoute));
    assert_eq!(r.classification, None);
}

#[tokio::test(start_paused = true)]
async fn typed_host_permission_denied_with_working_loopback_is_the_block_message() {
    // Python: error_class == permission_denied alone is enough once loopback works.
    let n = MockNet::new(|_| Err(os(e::EPERM)));
    let r = scan(&n, Some("192.0.2.50")).await;
    assert_eq!(r.error_class, Some(ErrorClass::PermissionDenied));
    assert_eq!(r.classification, Some(ErrorClass::PermissionDenied));
    assert_eq!(r.message.as_deref(), Some(SCAN_BLOCK_DETAIL));
}

#[tokio::test(start_paused = true)]
async fn typed_host_timed_out_is_timed_out() {
    let mut n = MockNet::new(|_| Err(ProbeError::TimedOut));
    n.connect = LanCheck::Blocked;
    let r = scan(&n, Some("192.0.2.50")).await;
    assert_eq!(r.error_class, Some(ErrorClass::TimedOut));
    assert_eq!(r.classification, None);
}

#[tokio::test(start_paused = true)]
async fn slow_typed_failure_with_blocked_gateway_is_not_called_a_block() {
    // The fast-fail rule: a no_route that took longer than 1 s is a real unreachable host.
    let mut n = MockNet::new(|_| Err(os(e::EHOSTUNREACH)));
    n.connect = LanCheck::Blocked;
    n.probe_delay = Duration::from_secs(3);
    let r = scan(&n, Some("192.0.2.50")).await;
    assert_eq!(r.error_class, Some(ErrorClass::NoRoute));
    assert_eq!(r.classification, None);
}

#[tokio::test(start_paused = true)]
async fn summary_records_classification_inputs_without_addresses() {
    let mut n = beta29_net();
    n.names = vec![("192.0.2.5".into(), "en0".into())];
    let r = scan(&n, None).await;
    let summary = r.summary();
    assert_eq!(summary["classification"], "permission_denied");
    assert!(!summary["classification_inputs"]
        .to_string()
        .contains("192.0.2"));
    assert_eq!(summary["outcome"], "error");
    assert_eq!(summary["error_class"], "permission_denied");
}

#[test]
fn classify_scan_block_thresholds() {
    use std::collections::BTreeMap;
    let counts =
        |d: usize, t: usize| BTreeMap::from([(ErrorClass::NoRoute, d), (ErrorClass::TimedOut, t)]);
    assert!(classify_scan_block(&counts(100, 0), Some(true)).0);
    assert!(
        !classify_scan_block(&counts(100, 1), Some(true)).0,
        "one timeout means a real network"
    );
    assert!(!classify_scan_block(&counts(100, 0), Some(false)).0);
    assert!(!classify_scan_block(&counts(100, 0), None).0);
    assert!(!classify_scan_block(&BTreeMap::new(), Some(true)).0);
    // 95% denied with the rest refused is still a block (share >= 0.95).
    let mixed = BTreeMap::from([(ErrorClass::NoRoute, 95), (ErrorClass::Refused, 5)]);
    assert!(classify_scan_block(&mixed, Some(true)).0);
    let mixed = BTreeMap::from([(ErrorClass::NoRoute, 94), (ErrorClass::Refused, 6)]);
    assert!(!classify_scan_block(&mixed, Some(true)).0);
}

// ---- saved_address_blocked ---------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn saved_address_blocked_needs_loopback_and_a_blocked_gateway() {
    let mut n = MockNet::new(|_| Ok(None));
    n.connect = LanCheck::Blocked;
    assert!(saved_address_blocked(&n).await);
    n.loopback_ok = false;
    assert!(
        !saved_address_blocked(&n).await,
        "no loopback: sockets are broken, not blocked"
    );
    n.loopback_ok = true;
    n.gateway = None;
    assert!(
        !saved_address_blocked(&n).await,
        "no gateway: cannot prove a block"
    );
    n.gateway = Some("192.0.2.1".into());
    n.connect = LanCheck::Ok;
    assert!(!saved_address_blocked(&n).await);
    n.connect = LanCheck::Unknown;
    assert!(!saved_address_blocked(&n).await);
}

#[test]
fn lan_check_refused_counts_as_ok() {
    assert_eq!(lan_check_from(Ok(())), LanCheck::Ok);
    assert_eq!(lan_check_from(Err(ProbeError::Refused)), LanCheck::Ok);
    assert_eq!(lan_check_from(Err(os(e::EHOSTUNREACH))), LanCheck::Blocked);
    assert_eq!(lan_check_from(Err(os(e::EPERM))), LanCheck::Blocked);
    assert_eq!(lan_check_from(Err(ProbeError::TimedOut)), LanCheck::Unknown);
}

// ---- find_playback / scan_candidates -----------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn manual_override_never_falls_back() {
    let n = MockNet::new(|h| Ok((h == "127.0.0.1").then_some(HB)));
    let found = find_playback(
        &n,
        Some("192.0.2.50"),
        8080,
        true,
        timeout(),
        &CancelToken::new(),
    )
    .await;
    assert!(
        found.is_none(),
        "a typed host that does not answer must not fall back to loopback"
    );
    assert_eq!(*n.calls.lock().unwrap(), vec!["192.0.2.50"]);
}

#[tokio::test(start_paused = true)]
async fn manual_override_hit() {
    let n = MockNet::new(|_| Ok(Some(HB)));
    let r = find_playback(
        &n,
        Some("192.0.2.50"),
        8080,
        true,
        timeout(),
        &CancelToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        (r.host.as_str(), r.source, r.port),
        ("192.0.2.50", DiscoverySource::Manual, 8080)
    );
}

#[tokio::test(start_paused = true)]
async fn find_prefers_loopback_then_lan_and_honours_auto_scan() {
    let n = MockNet::new(|h| Ok((h == "127.0.0.1" || h == "192.0.2.4").then_some(HB)));
    let r = find_playback(&n, None, 8080, true, timeout(), &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(r.source, DiscoverySource::Loopback);
    let n = MockNet::new(|h| Ok((h == "192.0.2.4").then_some(HB)));
    assert!(
        find_playback(&n, None, 8080, false, timeout(), &CancelToken::new())
            .await
            .is_none()
    );
    let r = find_playback(&n, None, 8080, true, timeout(), &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(
        (r.host.as_str(), r.source),
        ("192.0.2.4", DiscoverySource::Lan)
    );
}

#[tokio::test(start_paused = true)]
async fn probe_errors_are_misses_in_find_playback() {
    let n = MockNet::new(|h| {
        if h == "192.0.2.3" {
            Ok(Some(HB))
        } else {
            Err(os(e::EHOSTUNREACH))
        }
    });
    let r = find_playback(&n, None, 8080, true, timeout(), &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(r.host, "192.0.2.3");
}

#[tokio::test(start_paused = true)]
async fn cancelled_find_probes_nothing() {
    let n = MockNet::new(|_| Ok(Some(HB)));
    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(find_playback(&n, None, 8080, true, timeout(), &cancel)
        .await
        .is_none());
    assert!(n.calls.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn at_most_32_probes_in_flight_and_waves_follow_the_virtual_clock() {
    // 254 hosts, 3 s per probe, 32 at a time: ceil(254 / 32) = 8 waves = 24 s of VIRTUAL time.
    let mut n = MockNet::new(|_| Ok(None));
    n.nets = vec![net("192.0.2.0/24")];
    n.probe_delay = Duration::from_secs(3);
    let start = tokio::time::Instant::now();
    let found = find_playback(
        &n,
        None,
        8080,
        true,
        Duration::from_secs(3),
        &CancelToken::new(),
    )
    .await;
    assert!(found.is_none());
    assert_eq!(n.in_flight.lock().unwrap().1, MAX_WORKERS);
    let elapsed = start.elapsed();
    // 1 loopback probe + 8 waves of 3 s.
    assert_eq!(elapsed, Duration::from_secs(3 * (1 + 8)));
}

#[tokio::test(start_paused = true)]
async fn scan_candidates_reports_progress_in_chunks_of_64() {
    let mut n = MockNet::new(|h| Ok((h == "192.0.2.200").then_some(HB)));
    n.nets = vec![net("192.0.2.0/24")];
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    let found = scan_candidates(
        &n,
        None,
        8080,
        true,
        timeout(),
        &CancelToken::new(),
        Some(Arc::new(move |c, t| s.lock().unwrap().push((c, t)))),
    )
    .await;
    assert_eq!(
        found.iter().map(|c| c.host.as_str()).collect::<Vec<_>>(),
        ["192.0.2.200"]
    );
    assert_eq!(
        *seen.lock().unwrap(),
        vec![(64, 254), (128, 254), (192, 254), (254, 254)]
    );
}

#[tokio::test(start_paused = true)]
async fn scan_candidates_manual_and_loopback() {
    let n = MockNet::new(|h| Ok((h == "127.0.0.1").then_some(HB)));
    let r = scan_candidates(&n, None, 8080, true, timeout(), &CancelToken::new(), None).await;
    assert_eq!(
        (r[0].name.as_str(), r[0].source),
        ("127.0.0.1", DiscoverySource::Loopback)
    );
    let r = scan_candidates(
        &n,
        Some("192.0.2.9"),
        8080,
        true,
        timeout(),
        &CancelToken::new(),
        None,
    )
    .await;
    assert!(r.is_empty());
}

// ---- interface and network parsing -------------------------------------------------------------

const IP_O: &str = "\
1: lo    inet 127.0.0.1/8 scope host lo\\       valid_lft forever
2: eth0    inet 192.168.4.21/22 brd 192.168.7.255 scope global eth0\\       valid_lft forever
3: wg0    inet 169.254.9.9/16 scope global wg0
";

const IFCONFIG_MAC: &str = "\
lo0: flags=8049<UP,LOOPBACK,RUNNING,MULTICAST> mtu 16384
\tinet 127.0.0.1 netmask 0xff000000
en0: flags=8863<UP,BROADCAST,SMART,RUNNING,SIMPLEX,MULTICAST> mtu 1500
\tether aa:bb:cc:dd:ee:ff
\tinet 192.168.1.20 netmask 0xffffff00 broadcast 192.168.1.255
utun3: flags=8051<UP,POINTOPOINT,RUNNING,MULTICAST> mtu 1380
\tinet 10.8.0.2 --> 10.8.0.1 netmask 0xffffffff
";

#[test]
fn parses_ip_and_ifconfig_output() {
    let ip = parse_interfaces(IP_O);
    assert!(ip.contains(&("192.168.4.21".to_string(), 22)));
    let mac = parse_interfaces(IFCONFIG_MAC);
    assert!(mac.contains(&("192.168.1.20".to_string(), 24)));
    assert!(mac.contains(&("10.8.0.2".to_string(), 32)));
}

#[test]
fn usable_networks_never_exceed_a_24_and_skip_unusable_addresses() {
    let nets = usable_networks(&parse_interfaces(IP_O));
    // /22 is narrowed to the attached /24; loopback and link-local are skipped.
    assert_eq!(
        nets.iter().map(|n| n.to_string()).collect::<Vec<_>>(),
        ["192.168.4.0/24"]
    );
    let nets = usable_networks(&parse_interfaces(IFCONFIG_MAC));
    // /32 point-to-point tunnel is skipped (prefix must be < 31).
    assert_eq!(
        nets.iter().map(|n| n.to_string()).collect::<Vec<_>>(),
        ["192.168.1.0/24"]
    );
    // A narrower subnet is kept as is, duplicates collapse.
    let nets = usable_networks(&[
        ("192.0.2.9".into(), 29),
        ("192.0.2.10".into(), 29),
        ("224.0.0.1".into(), 24),
        ("0.0.0.0".into(), 24),
    ]);
    assert_eq!(
        nets.iter().map(|n| n.to_string()).collect::<Vec<_>>(),
        ["192.0.2.8/29"]
    );
}

#[test]
fn network_hosts_exclude_network_and_broadcast() {
    let hosts = net("192.0.2.0/29").hosts();
    assert_eq!(hosts.len(), 6);
    assert_eq!(hosts[0], "192.0.2.1".parse::<Ipv4Addr>().unwrap());
    assert_eq!(hosts[5], "192.0.2.6".parse::<Ipv4Addr>().unwrap());
    assert_eq!(net("10.1.2.3/24").hosts().len(), 254);
    assert!(net("192.0.2.0/24").contains("192.0.2.77".parse().unwrap()));
    assert!(!net("192.0.2.0/24").contains("192.0.3.1".parse().unwrap()));
}

#[test]
fn interface_names_from_ifconfig_and_ip() {
    let mac: HashMap<_, _> = interface_names(IFCONFIG_MAC).into_iter().collect();
    assert_eq!(mac["192.168.1.20"], "en0");
    assert_eq!(mac["127.0.0.1"], "lo0");
    let ip: HashMap<_, _> = interface_names(IP_O).into_iter().collect();
    assert_eq!(ip["192.168.4.21"], "eth0");
}

#[test]
fn typed_host_validation() {
    assert!(valid_typed_host("192.0.2.10"));
    assert!(valid_typed_host("stage-mac.local"));
    for bad in ["", "a b", "a/b", "a?b", "a#b", "a\\b", "a\nb", "a\rb"] {
        assert!(!valid_typed_host(bad), "{bad:?}");
    }
    assert!(!valid_typed_host(&"a".repeat(254)));
    assert!(valid_typed_host(&"a".repeat(253)));
}

// ---- real sockets: the production probe against the in-process fake ----------------------------

#[tokio::test]
async fn real_probe_finds_the_fake_playback_and_sends_nothing() {
    use stagepilot_playback::testkit::FakePlayback;
    let fake = FakePlayback::start().await;
    let hb = probe_checked("127.0.0.1", fake.port, Duration::from_secs(20))
        .await
        .unwrap();
    assert_eq!((hb.song_id, hb.playing), (101, false));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        fake.received.lock().unwrap().is_empty(),
        "discovery must never send a command"
    );
}

#[tokio::test]
async fn real_probe_rejects_a_server_that_does_not_negotiate_pr_protocol() {
    use stagepilot_playback::testkit::FakePlayback;
    let fake = FakePlayback::start().await;
    *fake.protocol.lock().unwrap() = Some("other");
    let err = probe_checked("127.0.0.1", fake.port, Duration::from_secs(20))
        .await
        .unwrap_err();
    assert_eq!(classify_error(&err).0, ErrorClass::NotPlayback, "{err:?}");
}

#[tokio::test]
async fn real_probe_with_a_silent_server_reports_no_heartbeat() {
    use stagepilot_playback::testkit::FakePlayback;
    let fake = FakePlayback::start().await;
    fake.stall(true);
    // Generous: the bound is only an upper limit for a slow runner; the probe returns at its own
    // (short) deadline.
    let err = probe_checked("127.0.0.1", fake.port, Duration::from_millis(500))
        .await
        .unwrap_err();
    let (class, _, step) = classify_error(&err);
    assert_eq!(
        (class, step.as_deref()),
        (ErrorClass::NotPlayback, Some("heartbeat"))
    );
}
