//! Port of backend/tests/test_remote_control.py, test_remote_files.py.

mod common;

use std::sync::Arc;

use common::{cloudflare_client, control_config, FakeCloudflare};
use serde_json::json;
use stagepilot_remote::control::RemoteControl;
use stagepilot_remote::error::RemoteError;
use stagepilot_remote::files::{
    atomic_write, is_group_or_world_readable, read_desired, ControlConfig, DesiredRemote,
};
use stagepilot_remote::platform::InMemoryLocker;
use stagepilot_remote::provider::CloudflareProvider;

fn setup(root: &std::path::Path) -> (RemoteControl<CloudflareProvider>, FakeCloudflare) {
    let config = control_config(root);
    let fake = FakeCloudflare::default();
    let provider = CloudflareProvider::new(&config, cloudflare_client(&fake));
    (
        RemoteControl::new(config, provider, Arc::new(InMemoryLocker::default())),
        fake,
    )
}

fn again(
    control: &RemoteControl<CloudflareProvider>,
    fake: &FakeCloudflare,
) -> RemoteControl<CloudflareProvider> {
    let provider = CloudflareProvider::new(&control.config, cloudflare_client(fake));
    RemoteControl::new(
        control.config.clone(),
        provider,
        Arc::new(InMemoryLocker::default()),
    )
}

fn counts(fake: &FakeCloudflare) -> (u32, u32) {
    let s = fake.state.lock().unwrap();
    (s.created, s.dns_created)
}

#[test]
fn fresh_enable_retry_disable_reenable_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let (control, fake) = setup(dir.path());
    let first = control.apply("enable").unwrap();
    let desired = read_desired(&control.desired_path);
    assert_eq!(first["phase"], "enabled");
    assert!(desired.enabled);
    assert!(!is_group_or_world_readable(&control.token_path).unwrap());
    assert!(!is_group_or_world_readable(&control.desired_path).unwrap());
    for _ in 0..3 {
        assert_eq!(again(&control, &fake).apply("enable").unwrap(), first);
    }
    assert_eq!(counts(&fake), (1, 1));
    {
        let s = fake.state.lock().unwrap();
        let configuration = &s.configs.values().next().unwrap()["config"];
        assert_eq!(
            configuration["ingress"],
            json!([
                {"hostname": "remote.example.com", "service": "http://127.0.0.1:8766"},
                {"service": "http_status:404"},
            ])
        );
        assert_eq!(configuration["warp-routing"]["enabled"], json!(false));
    }
    assert!(!std::fs::read_to_string(&control.desired_path)
        .unwrap()
        .contains("account-token"));
    assert_eq!(control.apply("disable").unwrap()["phase"], "disabled");
    let disabled = read_desired(&control.desired_path);
    assert!(!disabled.enabled && !control.token_path.exists());
    assert_eq!(disabled.public_origin, "https://remote.example.com");
    assert_eq!(disabled.port, 8766);
    {
        let s = fake.state.lock().unwrap();
        assert!(s.records.is_empty() && s.tunnels.is_empty());
    }
    control.apply("disable").unwrap();
    let second = control.apply("enable").unwrap();
    assert_eq!(second["installation"], first["installation"]);
    assert_ne!(second["tunnel_id"], first["tunnel_id"]);
    assert_ne!(
        read_desired(&control.desired_path).generation,
        desired.generation
    );
}

#[test]
fn lost_create_configure_dns_response_reconciles_without_duplicates() {
    for cut in [1, 2, 3] {
        let dir = tempfile::tempdir().unwrap();
        let (control, fake) = setup(dir.path());
        fake.state.lock().unwrap().lose_after = cut;
        assert!(
            matches!(control.apply("enable"), Err(RemoteError::Provider(_))),
            "cut {cut}"
        );
        assert!(!read_desired(&control.desired_path).enabled);
        assert_eq!(control.apply("enable").unwrap()["phase"], "enabled");
        assert_eq!(counts(&fake), (1, 1), "cut {cut}");
    }
}

#[test]
fn lost_disable_responses_reconcile_and_stay_closed() {
    for cut in [4, 5, 6, 7] {
        let dir = tempfile::tempdir().unwrap();
        let (control, fake) = setup(dir.path());
        control.apply("enable").unwrap();
        fake.state.lock().unwrap().lose_after = cut;
        assert!(control.apply("disable").is_err(), "cut {cut}");
        assert!(!read_desired(&control.desired_path).enabled);
        assert!(!control.token_path.exists());
        match control.apply("enable") {
            Err(RemoteError::Provider(m)) => assert!(m.contains("disable"), "{m}"),
            other => panic!("expected provider error, got {other:?}"),
        }
        assert_eq!(control.apply("disable").unwrap()["phase"], "disabled");
        let s = fake.state.lock().unwrap();
        assert!(s.tunnels.is_empty() && s.records.is_empty(), "cut {cut}");
    }
}

#[test]
fn network_outage_disable_is_local_first() {
    let dir = tempfile::tempdir().unwrap();
    let (control, fake) = setup(dir.path());
    control.apply("enable").unwrap();
    fake.state.lock().unwrap().offline = true;
    assert!(control.apply("disable").is_err());
    assert!(!read_desired(&control.desired_path).enabled);
    assert!(!control.token_path.exists());
    fake.state.lock().unwrap().offline = false;
    control.apply("disable").unwrap();
    let s = fake.state.lock().unwrap();
    assert!(s.tunnels.is_empty() && s.records.is_empty());
}

#[test]
fn ownership_and_configuration_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let (control, fake) = setup(dir.path());
    fake.state.lock().unwrap().records =
        vec![json!({"name": "remote.example.com", "id": "foreign"})];
    match control.apply("enable") {
        Err(RemoteError::Provider(m)) => assert!(m.contains("owned"), "{m}"),
        other => panic!("{other:?}"),
    }
    {
        let s = fake.state.lock().unwrap();
        assert_eq!(s.created, 0);
        assert_eq!(s.records.len(), 1);
    }
    {
        let mut s = fake.state.lock().unwrap();
        s.records.clear();
        s.bad_readback = true;
    }
    match control.apply("enable") {
        Err(RemoteError::Provider(m)) => assert!(m.contains("read-back"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(!read_desired(&control.desired_path).enabled);
    fake.state.lock().unwrap().bad_readback = false;
    control.apply("enable").unwrap();
    fake.state.lock().unwrap().records[0]["comment"] = json!("foreign");
    match control.apply("disable") {
        Err(RemoteError::Provider(m)) => assert!(m.contains("unowned"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(fake.state.lock().unwrap().records.len(), 1);
    assert!(!read_desired(&control.desired_path).enabled);
}

#[test]
fn parallel_enable_is_serialized() {
    let dir = tempfile::tempdir().unwrap();
    let (control, fake) = setup(dir.path());
    let control = Arc::new(control);
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let control = control.clone();
            std::thread::spawn(move || control.apply("enable").unwrap())
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(results.iter().all(|r| *r == results[0]));
    assert_eq!(counts(&fake), (1, 1));
}

#[test]
fn std_file_lock_serializes_real_files() {
    use stagepilot_remote::platform::{FileLocker, StdFileLocker};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.lock");
    let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let (path, counter) = (path.clone(), counter.clone());
            std::thread::spawn(move || {
                let _g = StdFileLocker.acquire(&path).unwrap();
                let before = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(20));
                assert_eq!(
                    counter.load(std::sync::atomic::Ordering::SeqCst),
                    before + 1
                );
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn binding_change_refused_and_bad_control_file_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let (control, fake) = setup(dir.path());
    control.apply("enable").unwrap();
    let mut changed = control.config.clone();
    changed.hostname = "other.example.com".into();
    match again(&control, &fake).apply("enable") {
        Ok(_) => {}
        Err(e) => panic!("{e:?}"),
    }
    let provider = CloudflareProvider::new(&changed, cloudflare_client(&fake));
    match RemoteControl::new(changed, provider, Arc::new(InMemoryLocker::default())).apply("enable")
    {
        Err(RemoteError::Provider(m)) => assert!(m.contains("Configuration changed"), "{m}"),
        other => panic!("{other:?}"),
    }
    std::fs::write(&control.desired_path, r#"{"enabled":"true"}"#).unwrap();
    assert!(!read_desired(&control.desired_path).enabled);
    let mut bad = control.config.clone();
    bad.remote_port = 8765;
    assert!(bad.validate().is_err());
    let mut bad = control.config.clone();
    bad.api_token_file = bad.installation_dir.join("account-token");
    assert!(bad.validate().is_err());
    let mut relative = control.config.clone();
    relative.control_dir = "relative".into();
    assert!(relative.validate().is_err());
    assert!(ControlConfig::from_json(&serde_json::to_string(&control.config).unwrap()).is_ok());
    assert!(ControlConfig::from_json(r#"{"extra":1}"#).is_err());
}

#[test]
fn corrupt_state_still_closes_on_disable() {
    let dir = tempfile::tempdir().unwrap();
    let (control, _fake) = setup(dir.path());
    control.apply("enable").unwrap();
    std::fs::write(control.config.control_dir.join("state.json"), "not json").unwrap();
    assert!(control.apply("disable").is_err());
    let desired = read_desired(&control.desired_path);
    assert_eq!(desired, DesiredRemote::default());
    assert!(!control.token_path.exists());
}

// ---- test_remote_files.py

#[test]
fn private_file_is_not_reported_as_exposed() {
    let dir = tempfile::tempdir().unwrap();
    let secret = dir.path().join("connector.token");
    atomic_write(&secret, "spi_test.token").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert!(!is_group_or_world_readable(&secret).unwrap());
}

#[cfg(unix)]
#[test]
fn group_or_world_readable_file_is_reported_as_exposed() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let secret = dir.path().join("connector.token");
    atomic_write(&secret, "spi_test.token").unwrap();
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(is_group_or_world_readable(&secret).unwrap());
}

#[cfg(windows)]
#[test]
fn windows_synthesized_mode_never_reports_exposure() {
    let dir = tempfile::tempdir().unwrap();
    let secret = dir.path().join("connector.token");
    atomic_write(&secret, "spi_test.token").unwrap();
    assert!(!is_group_or_world_readable(&secret).unwrap());
}

#[test]
fn desired_remote_validation() {
    assert!(DesiredRemote::from_json(
        r#"{"enabled":true,"generation":"x","public_origin":"https://a.b"}"#
    )
    .is_err());
    let ok = format!(
        r#"{{"enabled":true,"generation":"{}","public_origin":"https://a.example.com","port":8766}}"#,
        uuid::Uuid::new_v4()
    );
    assert!(DesiredRemote::from_json(&ok).is_ok());
    for origin in [
        "http://a.example.com",
        "https://a.example.com/x",
        "https://u@a.example.com",
        "https://a.example.com?x=1",
    ] {
        let text = format!(
            r#"{{"enabled":true,"generation":"{}","public_origin":"{origin}"}}"#,
            uuid::Uuid::new_v4()
        );
        assert!(DesiredRemote::from_json(&text).is_err(), "{origin}");
    }
    assert!(DesiredRemote::from_json(r#"{"port":80}"#).is_err());
    assert!(DesiredRemote::from_json(r#"{"unknown":1}"#).is_err());
}
