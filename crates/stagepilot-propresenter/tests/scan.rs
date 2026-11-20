//! The ProPresenter network scan: what it asks, where, how it ends. A fake network records every probe; one test runs the real
//! probe against a local HTTP server and checks the only request it makes.

use async_trait::async_trait;
use axum::extract::State;
use axum::http::{Method, Uri};
use axum::routing::any;
use axum::Router;
use stagepilot_propresenter::scan::{
    is_private, may_probe, Answer, Ipv4Net, Network, ProbeError, ScanErrorClass, ScanService,
    ScanSnapshot, ScanState, SystemNetwork, LOCAL_NETWORK_SETTINGS_URL, USUAL_PORT,
};
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Log = Arc<Mutex<Vec<(Ipv4Addr, u16)>>>;

struct FakeNet {
    nets: Vec<Ipv4Net>,
    answers: HashMap<(Ipv4Addr, u16), Answer>,
    /// What a host that does not answer does (default: a timeout, like an address nobody has).
    miss: ProbeError,
    errors: HashMap<Ipv4Addr, ProbeError>,
    loopback: bool,
    delay: Duration,
    log: Log,
}

impl FakeNet {
    fn new(nets: &[(&str, u8)]) -> Self {
        FakeNet {
            nets: nets
                .iter()
                .map(|(a, p)| Ipv4Net::new(a.parse().unwrap(), *p).unwrap())
                .collect(),
            answers: HashMap::new(),
            miss: ProbeError::TimedOut,
            errors: HashMap::new(),
            loopback: true,
            delay: Duration::ZERO,
            log: Arc::default(),
        }
    }
    fn answer(mut self, host: &str, port: u16, name: &str) -> Self {
        self.answers.insert(
            (host.parse().unwrap(), port),
            Answer {
                name: Some(name.into()),
                version: "ProPresenter 19.0.1".into(),
            },
        );
        self
    }
}

#[async_trait]
impl Network for FakeNet {
    async fn probe(
        &self,
        host: Ipv4Addr,
        port: u16,
        _timeout: Duration,
    ) -> Result<Option<Answer>, ProbeError> {
        self.log.lock().unwrap().push((host, port));
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        if let Some(answer) = self.answers.get(&(host, port)) {
            return Ok(Some(answer.clone()));
        }
        Err(self
            .errors
            .get(&host)
            .copied()
            .unwrap_or(if host.is_loopback() {
                ProbeError::Refused
            } else {
                self.miss
            }))
    }
    async fn networks(&self) -> Vec<Ipv4Net> {
        self.nets.clone()
    }
    async fn loopback_works(&self) -> bool {
        self.loopback
    }
}

async fn finished(service: &ScanService) -> ScanSnapshot {
    for _ in 0..600 {
        let snapshot = service.snapshot();
        if snapshot.state != ScanState::Scanning {
            return snapshot;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the scan did not finish");
}

#[tokio::test]
async fn this_computer_answers_first_and_nothing_else_is_asked() {
    let net = FakeNet::new(&[("192.168.4.21", 24)]).answer("127.0.0.1", 1025, "This Mac");
    let log = net.log.clone();
    let service = ScanService::new(Arc::new(net));
    service.start("192.168.4.40", 1025);
    let done = finished(&service).await;
    assert_eq!(done.state, ScanState::Found);
    assert_eq!(done.candidates.len(), 1);
    assert_eq!(done.candidates[0].host, "127.0.0.1");
    assert_eq!(done.candidates[0].name, "This Mac");
    assert_eq!(done.candidates[0].version, "ProPresenter 19.0.1");
    assert_eq!(*log.lock().unwrap(), vec![(Ipv4Addr::LOCALHOST, 1025)]);
}

#[tokio::test]
async fn it_looks_only_at_private_addresses_and_this_computer() {
    // The saved host is a public address and one interface is public: neither is ever asked.
    let net = FakeNet::new(&[("192.168.4.21", 24), ("8.8.8.8", 24), ("172.16.9.4", 24)]).answer(
        "172.16.9.77",
        1025,
        "Stage Mac",
    );
    let log = net.log.clone();
    let service = ScanService::new(Arc::new(net));
    service.start("203.0.113.9", 1025);
    let done = finished(&service).await;
    assert_eq!(done.state, ScanState::Found);
    let asked = log.lock().unwrap().clone();
    assert!(!asked.is_empty());
    assert!(
        asked.iter().all(|(host, _)| may_probe(*host)),
        "a public address was asked: {asked:?}"
    );
    assert!(asked
        .iter()
        .filter(|(h, _)| !h.is_loopback())
        .all(|(h, _)| is_private(*h)));
    assert_eq!(done.candidates[0].host, "172.16.9.77");
    assert_eq!(done.networks, ["192.168.4.0/24", "172.16.9.0/24"]);
    // 254 + 254 addresses and this computer.
    assert_eq!(done.hosts_total, 509);
    assert_eq!(done.hosts_probed, 509);
    assert_eq!(done.current, done.total);
}

#[tokio::test]
async fn the_saved_hosts_own_network_is_looked_at_too() {
    let net = FakeNet::new(&[("192.168.4.21", 24)]).answer("10.20.30.40", 1025, "Far Mac");
    let service = ScanService::new(Arc::new(net));
    service.start("10.20.30.5", 1025);
    let done = finished(&service).await;
    assert_eq!(done.candidates.len(), 1);
    assert_eq!(done.candidates[0].host, "10.20.30.40");
    assert_eq!(done.networks, ["192.168.4.0/24", "10.20.30.0/24"]);
}

#[tokio::test]
async fn the_saved_port_is_tried_first_then_1025() {
    let net = FakeNet::new(&[("192.168.4.21", 24)])
        .answer("192.168.4.50", 1025, "Usual")
        .answer("192.168.4.60", 11025, "Saved");
    // These hosts refuse (something is there, not on that port): the next port gets its turn.
    let mut net = net;
    net.errors
        .insert("192.168.4.50".parse().unwrap(), ProbeError::Refused);
    net.errors
        .insert("192.168.4.60".parse().unwrap(), ProbeError::Refused);
    let log = net.log.clone();
    let service = ScanService::new(Arc::new(net));
    service.start("192.168.4.60", 11025);
    let done = finished(&service).await;
    let found: Vec<(&str, u16)> = done
        .candidates
        .iter()
        .map(|c| (c.host.as_str(), c.port))
        .collect();
    assert_eq!(found, [("192.168.4.50", 1025), ("192.168.4.60", 11025)]);
    let asked = log.lock().unwrap().clone();
    let order: Vec<u16> = asked
        .iter()
        .filter(|(h, _)| *h == "192.168.4.50".parse::<Ipv4Addr>().unwrap())
        .map(|(_, p)| *p)
        .collect();
    assert_eq!(order, [11025, USUAL_PORT]);
    // A host that did not answer at all is not asked twice.
    let silent = asked
        .iter()
        .filter(|(h, _)| *h == "192.168.4.99".parse::<Ipv4Addr>().unwrap())
        .count();
    assert_eq!(silent, 1);
}

#[tokio::test]
async fn nothing_found_says_so_in_one_sentence() {
    let service = ScanService::new(Arc::new(FakeNet::new(&[("192.168.4.21", 24)])));
    service.start("", 1025);
    let done = finished(&service).await;
    assert_eq!(done.state, ScanState::NotFound);
    assert!(done.candidates.is_empty());
    assert_eq!(done.error_class, None);
    assert_eq!(done.settings_url, None);
    assert_eq!(
        done.reason.as_deref(),
        Some("No ProPresenter answered on this network.")
    );
    assert!(done.details.as_deref().unwrap().contains("not_found"));
}

#[tokio::test]
async fn macos_blocking_the_local_network_is_told_apart_from_nobody_there() {
    let mut net = FakeNet::new(&[("192.168.4.21", 24)]);
    net.miss = ProbeError::Denied;
    let service = ScanService::new(Arc::new(net));
    service.start("", 1025);
    let blocked = finished(&service).await;
    assert_eq!(blocked.state, ScanState::NotFound);
    assert_eq!(blocked.error_class, Some(ScanErrorClass::PermissionDenied));
    assert_eq!(
        blocked.settings_url.as_deref(),
        Some(LOCAL_NETWORK_SETTINGS_URL)
    );
    assert!(blocked.reason.as_deref().unwrap().contains("macOS"));

    // The same failures with this computer unable to talk to itself, or with one address merely timing out: not "blocked".
    let mut broken = FakeNet::new(&[("192.168.4.21", 24)]);
    broken.miss = ProbeError::Denied;
    broken.loopback = false;
    let service = ScanService::new(Arc::new(broken));
    service.start("", 1025);
    assert_eq!(finished(&service).await.error_class, None);

    let mut mixed = FakeNet::new(&[("192.168.4.21", 24)]);
    mixed.miss = ProbeError::Denied;
    mixed
        .errors
        .insert("192.168.4.9".parse().unwrap(), ProbeError::TimedOut);
    let service = ScanService::new(Arc::new(mixed));
    service.start("", 1025);
    assert_eq!(finished(&service).await.error_class, None);
}

#[tokio::test]
async fn no_network_at_all_is_its_own_plain_sentence() {
    let service = ScanService::new(Arc::new(FakeNet::new(&[])));
    service.start("", 1025);
    let done = finished(&service).await;
    assert_eq!(done.state, ScanState::NotFound);
    assert_eq!(done.error_class, Some(ScanErrorClass::NoRoute));
    assert_eq!(
        done.reason.as_deref(),
        Some("No usable network was found on this computer.")
    );
}

#[tokio::test]
async fn progress_counts_up_and_cancel_stops_the_scan() {
    let mut net = FakeNet::new(&[("192.168.4.21", 24)]);
    net.delay = Duration::from_millis(200);
    let log = net.log.clone();
    let service = ScanService::new(Arc::new(net));
    service.start("", 1025);
    // A second press does not start a second scan.
    service.start("", 1025);
    let mut seen = 0;
    for _ in 0..200 {
        let now = service.snapshot();
        assert_eq!(now.state, ScanState::Scanning);
        seen = seen.max(now.current);
        if now.current >= 64 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(seen >= 64, "progress never moved: {seen}");
    let running = service.snapshot();
    assert_eq!(running.total, 255);
    assert_eq!(running.phase.as_deref(), Some("Looking for ProPresenter…"));
    service.cancel().await;
    let stopped = service.snapshot();
    assert_eq!(stopped.state, ScanState::Idle);
    assert_eq!(stopped.reason.as_deref(), Some("Scan cancelled."));
    let after = log.lock().unwrap().len();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        log.lock().unwrap().len(),
        after,
        "probes went on after the cancel"
    );
    assert!(after < 255 * 2);
    // Cancelling again, with nothing running, changes nothing.
    service.cancel().await;
    assert_eq!(service.snapshot().state, ScanState::Idle);
}

#[tokio::test]
async fn the_real_probe_makes_one_get_version_request_and_nothing_else() {
    type Seen = Arc<Mutex<Vec<(String, String)>>>;
    let seen: Seen = Arc::default();
    async fn handler(
        State(seen): State<Seen>,
        method: Method,
        uri: Uri,
    ) -> axum::response::Response {
        seen.lock()
            .unwrap()
            .push((method.to_string(), uri.path().to_string()));
        axum::response::IntoResponse::into_response(axum::Json(serde_json_value()))
    }
    fn serde_json_value() -> serde_json::Value {
        serde_json::json!({"name": "Fake ProPresenter", "host_description": "ProPresenter 19.0.1", "api_version": "v1"})
    }
    let app = Router::new()
        .route("/{*rest}", any(handler))
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let service = ScanService::new(Arc::new(SystemNetwork::default()));
    service.start("127.0.0.1", port);
    let done = finished(&service).await;
    assert_eq!(done.state, ScanState::Found);
    assert_eq!(done.candidates[0].port, port);
    assert_eq!(done.candidates[0].name, "Fake ProPresenter");
    assert_eq!(done.candidates[0].version, "ProPresenter 19.0.1");
    assert_eq!(
        *seen.lock().unwrap(),
        vec![("GET".to_string(), "/version".to_string())]
    );

    // A public address is refused by the probe itself, whoever asks.
    let refused = SystemNetwork::default()
        .probe("8.8.8.8".parse().unwrap(), 1025, Duration::from_millis(200))
        .await;
    assert_eq!(refused, Err(ProbeError::Other));
}

#[tokio::test]
async fn something_else_answering_is_not_propresenter() {
    let app = Router::new().route("/{*rest}", any(|| async { "<html>router</html>" }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let answer = SystemNetwork::default()
        .probe(Ipv4Addr::LOCALHOST, port, Duration::from_secs(2))
        .await;
    assert_eq!(answer, Ok(None));
    // Nothing listens on a port nobody holds: refused, not "found".
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let answer = SystemNetwork::default()
        .probe(Ipv4Addr::LOCALHOST, closed, Duration::from_secs(2))
        .await;
    // Windows does not answer a closed loopback port at once: it retries the connection until the time limit.
    if cfg!(windows) {
        assert!(
            matches!(answer, Err(ProbeError::Refused | ProbeError::TimedOut)),
            "{answer:?}"
        );
    } else {
        assert_eq!(answer, Err(ProbeError::Refused));
    }
}
