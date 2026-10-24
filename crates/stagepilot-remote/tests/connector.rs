//! Port of backend/tests/test_remote_connector.py and test_remote_quick.py, against the fake
//! process spawner (portable) plus a real-process check on Unix.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use common::*;
use serde_json::Value;
use stagepilot_remote::connector::{supervisor_pass, Connector, TokenProvider};
use stagepilot_remote::feature::RemoteFeature;
use stagepilot_remote::files::{atomic_write, read_desired, DesiredRemote};
use stagepilot_remote::http::{HttpRequest, HttpResponse, HttpTransport, TransportError};
#[cfg(unix)]
use stagepilot_remote::platform::{ProcessSpawner, SpawnSpec, StdSpawner};
use stagepilot_remote::quick::QuickConnector;

fn ready_transport(ready: Arc<AtomicBool>) -> Arc<dyn HttpTransport> {
    Arc::new(FnTransport(move |r: &HttpRequest| {
        if ready.load(Ordering::SeqCst) {
            let status = if r.path() == "/ready" || r.path() == "/api/v1/access" {
                200
            } else {
                404
            };
            Ok(HttpResponse::new(status, ""))
        } else {
            Err(TransportError("refused".into()))
        }
    }))
}

struct Fix {
    dir: tempfile::TempDir,
    tp: TestPlatform,
    ready: Arc<AtomicBool>,
    desired: DesiredRemote,
    connector: Connector,
}

fn fixture() -> Fix {
    let dir = tempfile::tempdir().unwrap();
    let ready = Arc::new(AtomicBool::new(true));
    let tp = test_platform(ready_transport(ready.clone()));
    let binary = dir.path().join("fake-connector");
    std::fs::write(&binary, b"x").unwrap();
    let desired = DesiredRemote {
        enabled: true,
        generation: uuid::Uuid::new_v4().to_string(),
        public_origin: "https://remote.test".into(),
        port: 18900,
    };
    let control = dir.path().join("remote.json");
    atomic_write(&control, &desired.to_json()).unwrap();
    let token = dir.path().join("connector.token");
    atomic_write(&token, "test-run-token").unwrap();
    make_private(&token);
    let connector = Connector::new(
        &binary,
        &control,
        &dir.path().join("status.json"),
        18901,
        None,
        None,
        &tp.platform,
    );
    Fix {
        dir,
        tp,
        ready,
        desired,
        connector,
    }
}

fn status(f: &Fix) -> Value {
    serde_json::from_str(&std::fs::read_to_string(f.dir.path().join("status.json")).unwrap())
        .unwrap()
}

#[test]
fn child_crash_restart_disable_reenable_and_supervisor_restart() {
    let mut f = fixture();
    f.connector.step().unwrap();
    assert_eq!(status(&f)["state"], "connected");
    assert_eq!(f.tp.spawner.specs().len(), 1);
    let first = f.connector.pid().unwrap();
    // Crash the child: the connector waits out its restart backoff, then starts another.
    f.tp.spawner.children()[0].exit(-9);
    f.connector.step().unwrap();
    assert!(!f.connector.has_process());
    assert_eq!(status(&f)["state"], "reconnecting");
    f.tp.clock.advance(1.5);
    f.connector.step().unwrap();
    assert!(f.connector.pid().is_some_and(|p| p != first));
    assert_eq!(status(&f)["state"], "connected");
    // Disable.
    atomic_write(
        &f.dir.path().join("remote.json"),
        &DesiredRemote::default().to_json(),
    )
    .unwrap();
    f.connector.step().unwrap();
    assert!(!f.connector.has_process());
    assert_eq!(status(&f)["state"], "disabled");
    assert!(f.tp.spawner.children()[1].terminated.load(Ordering::SeqCst));
    // Re-enable with a new generation.
    f.desired.generation = uuid::Uuid::new_v4().to_string();
    atomic_write(&f.dir.path().join("remote.json"), &f.desired.to_json()).unwrap();
    f.connector.step().unwrap();
    assert_eq!(status(&f)["state"], "connected");
    f.connector.stop();
    // Supervisor restart: a fresh connector recovers; corrupt state fails closed.
    let mut restarted = Connector::new(
        &f.dir.path().join("fake-connector"),
        &f.dir.path().join("remote.json"),
        &f.dir.path().join("status.json"),
        18901,
        None,
        None,
        &f.tp.platform,
    );
    restarted.step().unwrap();
    assert_eq!(status(&f)["state"], "connected");
    atomic_write(&f.dir.path().join("remote.json"), "corrupt").unwrap();
    restarted.step().unwrap();
    assert!(!restarted.has_process());
    restarted.stop();
    assert!(!std::fs::read_to_string(f.dir.path().join("status.json"))
        .unwrap()
        .contains("test-run-token"));
}

#[cfg(unix)]
#[test]
fn missing_insecure_token_and_metrics_conflict_fail_closed() {
    let mut f = fixture();
    let token = f.dir.path().join("connector.token");
    make_mode(&token, 0o644);
    f.connector.step().unwrap();
    assert!(!f.connector.has_process());
    make_mode(&token, 0o600);
    // Not retried until the backoff elapses, then a busy metrics port blocks the start.
    f.tp.clock.advance(5.0);
    f.tp.ports.set_busy(18901, true);
    f.connector.step().unwrap();
    assert!(!f.connector.has_process());
    assert_eq!(status(&f)["state"], "reconnecting");
    f.tp.ports.set_busy(18901, false);
    f.tp.clock.advance(10.0);
    f.connector.step().unwrap();
    assert!(f.connector.has_process());
    assert_eq!(status(&f)["state"], "connected");
    // Arguments carry the token file, never the token.
    let spec = &f.tp.spawner.specs()[0];
    assert!(
        spec.args.contains(&"--token-file".to_owned())
            && !spec.args.contains(&"--token".to_owned())
    );
    f.connector.stop();
}

#[test]
fn desktop_token_is_passed_directly_without_creating_a_file() {
    let mut f = fixture();
    let token = f.dir.path().join("connector.token");
    std::fs::remove_file(&token).unwrap();
    let provider: TokenProvider = Arc::new(|| Ok("test-run-token".to_owned()));
    f.connector = Connector::new(
        &f.dir.path().join("fake-connector"),
        &f.dir.path().join("remote.json"),
        &f.dir.path().join("status.json"),
        18901,
        Some(provider),
        None,
        &f.tp.platform,
    );
    f.connector.step().unwrap();
    assert_eq!(status(&f)["state"], "connected");
    assert!(!token.exists());
    let spec = f.tp.spawner.specs()[0].clone();
    // M5: the tunnel token must not be visible in `ps`: never in argv, only in the child's env.
    assert!(
        spec.args.iter().all(|a| !a.contains("test-run-token")),
        "token leaked into argv: {:?}",
        spec.args
    );
    assert!(!spec.args.contains(&"--token".to_owned()));
    assert!(!spec.args.contains(&"--token-file".to_owned()));
    let tunnel_token: Vec<_> = spec
        .env
        .iter()
        .filter(|(k, _)| k == "TUNNEL_TOKEN")
        .collect();
    assert_eq!(tunnel_token.len(), 1);
    assert_eq!(tunnel_token[0].1, "test-run-token");
    // The token is not echoed by the spec's Debug output either.
    assert!(!format!("{spec:?}").contains("test-run-token"));
    assert!(!spec.capture_output);
    f.tp.spawner.children()[0].exit(-9);
    f.connector.step().unwrap();
    f.tp.clock.advance(1.5);
    f.connector.step().unwrap();
    assert_eq!(f.tp.spawner.specs().len(), 2);
    assert!(!token.exists());
    f.connector.stop();
    assert!(!token.exists());
}

#[test]
fn stop_kills_a_child_that_ignores_terminate() {
    let mut f = fixture();
    f.tp.spawner.set_ignore_terminate(true);
    f.connector.step().unwrap();
    f.connector.stop();
    let child = &f.tp.spawner.children()[0];
    assert!(child.terminated.load(Ordering::SeqCst) && child.killed.load(Ordering::SeqCst));
}

#[test]
fn spawn_failure_backs_off_and_unhealthy_edge_is_reconnecting() {
    let mut f = fixture();
    f.tp.spawner.fail_next("exec format error");
    f.connector.step().unwrap();
    assert_eq!(status(&f)["state"], "reconnecting");
    assert!(status(&f)["pid"].is_null());
    f.connector.step().unwrap(); // still inside the backoff window
    assert_eq!(f.tp.spawner.specs().len(), 0);
    f.tp.clock.advance(2.5);
    f.ready.store(false, Ordering::SeqCst);
    f.connector.step().unwrap();
    assert_eq!(f.tp.spawner.specs().len(), 1);
    assert_eq!(status(&f)["state"], "reconnecting");
    assert!(status(&f)["pid"].is_number());
}

#[test]
fn child_environment_is_minimal() {
    let mut f = fixture();
    f.connector.step().unwrap();
    let env = f.tp.spawner.specs()[0].env.clone();
    let keys: Vec<_> = env.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["PATH", "HOME"]);
}

#[cfg(unix)]
#[test]
fn std_spawner_terminates_a_real_child() {
    let mut child = StdSpawner
        .spawn(&SpawnSpec {
            program: "/bin/sleep".into(),
            args: vec!["60".into()],
            env: vec![("PATH".into(), "/bin:/usr/bin".into())],
            capture_output: false,
        })
        .unwrap();
    assert!(child.pid() > 0);
    assert!(matches!(child.try_wait(), Ok(None)));
    stagepilot_remote::connector::stop_child(&mut child);
    assert!(matches!(child.try_wait(), Ok(Some(_))));
}

#[cfg(unix)]
#[test]
fn std_spawner_reports_a_missing_binary_as_an_error() {
    let result = StdSpawner.spawn(&SpawnSpec {
        program: "/definitely/not/here".into(),
        args: vec![],
        env: vec![],
        capture_output: false,
    });
    assert!(result.is_err());
}

#[test]
fn supervisor_pass_publishes_named_and_off_status() {
    let mut f = fixture();
    let ready = Arc::new(AtomicBool::new(true));
    let tp = test_platform(ready_transport(ready));
    let control = f.dir.path().join("remote.json");
    let feature = RemoteFeature::new(
        &control,
        18900,
        tp.platform.locker.clone(),
        tp.platform.clock.clone(),
    );
    let mut quick = QuickConnector::new(
        &f.dir.path().join("fake-connector"),
        feature.clone(),
        18901,
        &tp.platform,
    );
    let status_file = f.dir.path().join("status.json");
    // No intent file yet: only the "off" product status is published.
    supervisor_pass(
        &mut f.connector,
        &mut quick,
        &feature,
        &f.dir.path().join("fake-connector"),
        &control,
        &status_file,
        tp.clock.as_ref(),
    );
    let access: Value =
        serde_json::from_str(&std::fs::read_to_string(&feature.status_path).unwrap()).unwrap();
    assert_eq!(access["state"], "off");
    // Managed (named) enable: the product status follows the connector heartbeat.
    feature
        .set_managed_enabled(true, &f.desired.generation)
        .unwrap();
    supervisor_pass(
        &mut f.connector,
        &mut quick,
        &feature,
        &f.dir.path().join("fake-connector"),
        &control,
        &status_file,
        tp.clock.as_ref(),
    );
    let access: Value =
        serde_json::from_str(&std::fs::read_to_string(&feature.status_path).unwrap()).unwrap();
    assert_eq!(access["state"], "connected");
    assert_eq!(access["url"], "https://remote.test");
    assert_eq!(access["temporary_url"], false);
    f.connector.stop();
}

// ---------------------------------------------------------------- quick (test_remote_quick.py)

fn quick_fixture(
    binary_exists: bool,
) -> (
    tempfile::TempDir,
    TestPlatform,
    RemoteFeature,
    QuickConnector,
) {
    let dir = tempfile::tempdir().unwrap();
    let tp = test_platform(ready_transport(Arc::new(AtomicBool::new(false))));
    let binary = dir.path().join("quick-substitute");
    if binary_exists {
        std::fs::write(&binary, b"#!/bin/sh\n").unwrap();
        make_executable(&binary);
    }
    let feature = RemoteFeature::new(
        &dir.path().join("remote.json"),
        18902,
        tp.platform.locker.clone(),
        tp.platform.clock.clone(),
    );
    let quick = QuickConnector::new(&binary, feature.clone(), 18903, &tp.platform);
    (dir, tp, feature, quick)
}

#[cfg(unix)]
#[test]
fn quick_child_lifecycle_without_account_token() {
    let (_dir, tp, feature, mut quick) = quick_fixture(true);
    tp.spawner
        .set_output(b"INF registered\nhttps://substitute.trycloudflare.com\n");
    feature.set_enabled(true).unwrap();
    quick.step().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while quick.url().is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    quick.step().unwrap();
    assert!(read_desired(&feature.control).enabled);
    assert_eq!(feature.status()["state"], "reconnecting"); // No real edge/ingress health.
    assert!(feature.status()["url"].is_null());
    let spec = tp.spawner.specs()[0].clone();
    assert!(
        spec.args.contains(&"--url".to_owned()) && !spec.args.contains(&"--token-file".to_owned())
    );
    assert!(spec.capture_output);
    let first = quick.has_process();
    assert!(first);
    tp.spawner.children()[0].exit(-9);
    quick.step().unwrap();
    assert!(!read_desired(&feature.control).enabled);
    tp.clock.advance(1.5);
    quick.step().unwrap();
    assert_eq!(tp.spawner.specs().len(), 2);
    feature.set_enabled(false).unwrap();
    quick.step().unwrap();
    assert!(!quick.has_process());
    assert!(!read_desired(&feature.control).enabled);
    assert_eq!(feature.status()["state"], "off");
    quick.stop();
}

#[cfg(unix)]
#[test]
fn quick_connected_requires_edge_and_local_ingress_health() {
    let dir = tempfile::tempdir().unwrap();
    let ready = Arc::new(AtomicBool::new(true));
    let tp = test_platform(ready_transport(ready.clone()));
    let binary = dir.path().join("quick-substitute");
    std::fs::write(&binary, b"#!/bin/sh\n").unwrap();
    make_executable(&binary);
    tp.spawner
        .set_output(b"https://substitute.trycloudflare.com\n");
    let feature = RemoteFeature::new(
        &dir.path().join("remote.json"),
        18902,
        tp.platform.locker.clone(),
        tp.platform.clock.clone(),
    );
    let mut quick = QuickConnector::new(&binary, feature.clone(), 18903, &tp.platform);
    feature.set_enabled(true).unwrap();
    quick.step().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while quick.url().is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    quick.step().unwrap();
    let published: Value =
        serde_json::from_str(&std::fs::read_to_string(&feature.status_path).unwrap()).unwrap();
    assert_eq!(published["state"], "connected");
    assert_eq!(published["url"], "https://substitute.trycloudflare.com");
    let desired = read_desired(&feature.control);
    assert!(
        desired.enabled
            && desired.public_origin == "https://substitute.trycloudflare.com"
            && desired.port == 18902
    );
    quick.stop();
}

#[cfg(unix)]
#[test]
fn quick_rate_limit_stops_and_reports_error() {
    let (_dir, tp, feature, mut quick) = quick_fixture(true);
    tp.spawner
        .set_output(b"ERR failed to request quick Tunnel: status 429 Too Many Requests\n");
    feature.set_enabled(true).unwrap();
    quick.step().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        quick.step().unwrap();
        if feature.status_path.exists() && !quick.has_process() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "rate limit never observed"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let published: Value =
        serde_json::from_str(&std::fs::read_to_string(&feature.status_path).unwrap()).unwrap();
    assert_eq!(published["state"], "error");
    assert!(!read_desired(&feature.control).enabled);
    quick.stop();
}

#[test]
fn missing_binary_is_safe_product_error() {
    let (dir, _tp, feature, mut quick) = quick_fixture(false);
    feature.set_enabled(true).unwrap();
    quick.step().unwrap();
    let status = feature.status();
    assert_eq!(status["state"], "error");
    assert!(status["url"].is_null());
    assert!(!status
        .values()
        .any(|v| v.to_string().contains(dir.path().to_str().unwrap())));
    assert!(!read_desired(&feature.control).enabled);
}

#[cfg(unix)]
#[test]
fn quick_spawn_failure_reports_error_and_backs_off() {
    let (_dir, tp, feature, mut quick) = quick_fixture(true);
    feature.set_enabled(true).unwrap();
    tp.spawner.fail_next("boom");
    quick.step().unwrap();
    assert_eq!(feature.status()["state"], "error");
    assert!(!quick.has_process());
    quick.step().unwrap();
    assert_eq!(tp.spawner.specs().len(), 0); // inside the backoff window
    tp.clock.advance(2.5);
    quick.step().unwrap();
    assert_eq!(tp.spawner.specs().len(), 1);
    quick.stop();
}

// ---- B2: a connector left behind by an unclean exit --------------------------------------------

const ORPHAN_PID: u32 = 9001;

fn pidfile(f: &Fix) -> std::path::PathBuf {
    f.dir.path().join("connector.pid")
}

/// Write the pidfile an earlier run would have left, and register that process with the fake OS.
fn leave_orphan(
    f: &Fix,
    running_exe: Option<&std::path::Path>,
    recorded_start: Option<u64>,
    running_start: Option<u64>,
    generation: &str,
) {
    use stagepilot_remote::orphan::{write_pidfile, PidRecord};
    write_pidfile(
        &pidfile(f),
        &PidRecord {
            pid: ORPHAN_PID,
            started_at: recorded_start,
            exe: f
                .dir
                .path()
                .join("fake-connector")
                .to_string_lossy()
                .into_owned(),
            generation: generation.to_owned(),
            metrics_port: 18901,
        },
    );
    if let Some(exe) = running_exe {
        f.tp.inspector.add(ORPHAN_PID, Some(exe), running_start);
    }
}

fn bundled(f: &Fix) -> std::path::PathBuf {
    f.dir.path().join("fake-connector")
}

#[test]
fn a_stale_connector_is_terminated_and_a_fresh_one_starts() {
    let mut f = fixture();
    f.ready.store(false, Ordering::SeqCst); // the orphan's tunnel is not connected
    let exe = bundled(&f);
    leave_orphan(&f, Some(&exe), Some(777), Some(777), "an-old-generation");
    f.connector.step().unwrap();
    assert_eq!(*f.tp.inspector.terminated.lock().unwrap(), vec![ORPHAN_PID]);
    assert!(!f.tp.inspector.is_running(ORPHAN_PID));
    assert_eq!(
        f.tp.spawner.specs().len(),
        1,
        "a fresh connector was started"
    );
    // The pidfile now names the new child.
    let record = stagepilot_remote::orphan::read_pidfile(&pidfile(&f)).unwrap();
    assert_eq!(
        record.pid,
        f.tp.spawner.children()[0].pid.load(Ordering::SeqCst) as u32
    );
    assert_eq!(
        *f.tp.inspector.bound_to_parent.lock().unwrap(),
        vec![record.pid]
    );
    f.connector.stop();
    assert!(!pidfile(&f).exists(), "stop removes the pidfile");
}

#[test]
fn a_stale_connector_that_ignores_terminate_is_killed() {
    let mut f = fixture();
    f.ready.store(false, Ordering::SeqCst);
    let exe = bundled(&f);
    leave_orphan(&f, Some(&exe), Some(1), Some(1), "old");
    f.tp.inspector.stubborn.lock().unwrap().push(ORPHAN_PID);
    f.connector.step().unwrap();
    assert_eq!(*f.tp.inspector.killed.lock().unwrap(), vec![ORPHAN_PID]);
    assert_eq!(f.tp.spawner.specs().len(), 1);
}

#[test]
fn a_connected_connector_for_the_same_remote_is_adopted() {
    let mut f = fixture();
    let exe = bundled(&f);
    let generation = f.desired.generation.clone();
    leave_orphan(&f, Some(&exe), Some(5), Some(5), &generation);
    f.connector.step().unwrap();
    assert!(f.tp.spawner.specs().is_empty(), "nothing new was spawned");
    assert!(f.tp.inspector.terminated.lock().unwrap().is_empty());
    assert_eq!(status(&f)["state"], "connected");
    assert_eq!(status(&f)["pid"], ORPHAN_PID);
    // Stopping the supervisor stops the adopted connector too and clears the pidfile.
    f.connector.stop();
    assert_eq!(*f.tp.inspector.terminated.lock().unwrap(), vec![ORPHAN_PID]);
    assert!(!pidfile(&f).exists());
}

#[test]
fn an_orphan_that_is_not_connected_is_not_adopted() {
    let mut f = fixture();
    f.ready.store(false, Ordering::SeqCst);
    let exe = bundled(&f);
    let generation = f.desired.generation.clone();
    leave_orphan(&f, Some(&exe), Some(5), Some(5), &generation);
    f.connector.step().unwrap();
    assert_eq!(*f.tp.inspector.terminated.lock().unwrap(), vec![ORPHAN_PID]);
    assert_eq!(f.tp.spawner.specs().len(), 1);
}

#[test]
fn a_pidfile_for_a_dead_process_is_discarded_without_signalling_anything() {
    let mut f = fixture();
    leave_orphan(&f, None, Some(5), None, "old");
    f.connector.step().unwrap();
    assert!(f.tp.inspector.terminated.lock().unwrap().is_empty());
    assert!(f.tp.inspector.killed.lock().unwrap().is_empty());
    assert_eq!(f.tp.spawner.specs().len(), 1);
}

#[test]
fn a_recycled_pid_running_another_program_is_left_alone() {
    let mut f = fixture();
    let other = std::path::Path::new("/usr/bin/some-other-program").to_path_buf();
    leave_orphan(&f, Some(&other), Some(5), Some(99), "old");
    f.connector.step().unwrap();
    assert!(f.tp.inspector.terminated.lock().unwrap().is_empty());
    assert!(f.tp.inspector.killed.lock().unwrap().is_empty());
    assert!(
        f.tp.inspector.is_running(ORPHAN_PID),
        "the stranger still runs"
    );
    assert_eq!(f.tp.spawner.specs().len(), 1);
}

#[test]
fn a_recycled_pid_running_the_same_binary_for_someone_else_is_left_alone() {
    // Same program, different start time: another StagePilot's connector reusing the pid.
    let mut f = fixture();
    let exe = bundled(&f);
    leave_orphan(&f, Some(&exe), Some(5), Some(6), "old");
    f.connector.step().unwrap();
    assert!(f.tp.inspector.terminated.lock().unwrap().is_empty());
    assert!(f.tp.inspector.is_running(ORPHAN_PID));
    assert_eq!(f.tp.spawner.specs().len(), 1);
}

#[test]
fn an_unreadable_pidfile_is_ignored() {
    let mut f = fixture();
    std::fs::write(pidfile(&f), b"not json").unwrap();
    f.connector.step().unwrap();
    assert_eq!(f.tp.spawner.specs().len(), 1);
}

#[cfg(unix)]
#[test]
fn a_child_exits_when_its_parent_is_killed() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    // `parent` stands for StagePilot, `child` for cloudflared.
    let mut parent = Command::new("sleep")
        .arg("60")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = Command::new("sleep")
        .arg("60")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    stagepilot_remote::orphan::spawn_parent_watch(parent.id(), child.id());
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        child.try_wait().unwrap().is_none(),
        "the child lives while the parent does"
    );
    parent.kill().unwrap(); // SIGKILL: no cleanup runs
    parent.wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the child outlived its parent");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(unix)]
#[test]
fn the_system_inspector_recognises_a_real_process_and_a_dead_one() {
    use stagepilot_remote::orphan::{ProcessInspector, SystemInspector};
    use std::process::{Command, Stdio};
    let mut child = Command::new("sleep")
        .arg("60")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let info = SystemInspector.info(child.id()).expect("running");
    let exe = info.exe.expect("exe readable");
    assert!(exe.to_string_lossy().contains("sleep"), "{exe:?}");
    assert!(info.started_at.is_some());
    // The same process reports the same start marker twice.
    assert_eq!(
        SystemInspector.info(child.id()).unwrap().started_at,
        info.started_at
    );
    SystemInspector.terminate(child.id());
    child.wait().unwrap();
    assert!(SystemInspector.info(child.id()).is_none());
}
