//! Port of `test_planning_center_lengths_retry.py`: repeated and uncertain length writes keep one
//! restore point, reconcile after a timeout, never overwrite a newer operator edit, and survive a
//! restart. Runs the real `LengthService` + `PcoClient` against the in-process mock server and a
//! real `StateService` (the reload action mutates the store, like the Python rig's dispatcher).

mod support;

use chrono::{NaiveDate, Utc};
use serde_json::json;
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::ConnectionMethod;
use stagepilot_core::events::EventType;
use stagepilot_core::settings::{
    MemoryCredentialStore, MemorySettingsStore, SettingsFileStore, SettingsService,
};
use stagepilot_core::state::{ApplicationState, ConnectionStatus, ServicePlan, Song};
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
use stagepilot_pco::client::default_factory;
use stagepilot_pco::lengths::{LengthService, ReloadOutcome};
use stagepilot_pco::models::PcoSettings;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::*;

#[derive(Clone, Copy, Debug)]
enum Failure {
    Timeout,
    Status(u16),
}

type Remote = Arc<Mutex<BTreeMap<String, i64>>>;
type Failures = Arc<Mutex<BTreeMap<String, Failure>>>;

struct Rig {
    settings: Arc<SettingsService>,
    state: StateStore,
    dispatcher: StateService,
    service: LengthService,
    plan: ServicePlan,
    remote: Remote,
    failures: Failures,
    mock: Mock,
}

fn settings_service(path: Option<&Path>) -> Arc<SettingsService> {
    let store: Box<dyn stagepilot_core::settings::SettingsStore> = match path {
        Some(p) => Box::new(SettingsFileStore::new(p)),
        None => Box::new(MemorySettingsStore::new(None)),
    };
    let service = SettingsService::new(
        store,
        Box::new(MemoryCredentialStore::default()),
        BTreeMap::new(),
    );
    service.load();
    Arc::new(service)
}

fn pco_settings() -> PcoSettings {
    PcoSettings {
        connection_method: ConnectionMethod::Manual,
        // A "timeout" failure answers slower than this.
        request_timeout_seconds: 0.3,
        ..pat_settings()
    }
}

fn build_service(
    mock: &Mock,
    settings: Arc<SettingsService>,
    state: StateStore,
    dispatcher: StateService,
) -> LengthService {
    LengthService::new(
        settings,
        state,
        dispatcher,
        default_factory(Some(mock.url.clone())),
        Arc::new(pco_settings),
    )
    .with_reload_timeout(Duration::from_secs(3))
}

impl Rig {
    async fn new(path: Option<&Path>, count: usize) -> Rig {
        let settings = settings_service(path);
        let plan = ServicePlan {
            id: "plan".into(),
            title: "Sunday".into(),
            date: NaiveDate::from_ymd_opt(2026, 7, 12).unwrap(),
            service_type: "Weekend".into(),
            service_type_id: Some("type".into()),
            service_times: vec![],
            duration_source: "planning_center".into(),
            songs: (0..count)
                .map(|i| {
                    Song::new(
                        &i.to_string(),
                        &format!("Song {i}"),
                        Some(100),
                        i as u32 + 1,
                    )
                })
                .collect(),
        };
        let state = StateStore::new(ApplicationState {
            plan: Some(plan.clone()),
            planning_center_status: ConnectionStatus::Connected,
            ..ApplicationState::default()
        });
        let remote: Remote = Arc::new(Mutex::new(
            (0..count).map(|i| (i.to_string(), 100)).collect(),
        ));
        let failures: Failures = Arc::default();

        let (r, f, s) = (remote.clone(), failures.clone(), settings.clone());
        let mock = Mock::start(move |req| {
            let prefix = "/services/v2/service_types/type/plans/plan/items/";
            assert!(req.path.starts_with(prefix), "{}", req.path);
            let id = req.path[prefix.len()..].to_string();
            if req.method == "PATCH" {
                // The write-ahead record is already durably saved before any request.
                assert!(
                    s.snapshot()
                        .planning_center_length_undo
                        .iter()
                        .any(|rec| rec.item_id == id),
                    "no write-ahead record before PATCH of {id}"
                );
                match f.lock().unwrap().get(&id).copied() {
                    Some(Failure::Timeout) => {
                        return Resp::json(200, json!({})).delayed(Duration::from_secs(2))
                    }
                    Some(Failure::Status(code)) => {
                        return Resp::json(code, json!({})).header("Retry-After", "0")
                    }
                    None => {}
                }
                let body = req.json();
                assert_eq!(body["data"]["type"], "Item");
                let attributes = body["data"]["attributes"].as_object().unwrap();
                assert_eq!(attributes.len(), 1, "only the length may be written");
                r.lock()
                    .unwrap()
                    .insert(id.clone(), attributes["length"].as_i64().unwrap());
            }
            let length = r.lock().unwrap()[&id];
            let n: i64 = id.parse().unwrap();
            Resp::json(
                200,
                json!({"data": item(&id, "song", &format!("Song {id}"), n, Some(length), None)}),
            )
        })
        .await;

        let bus = EventBus::new();
        let dispatcher = StateService::new(bus.clone(), state.clone(), 50, 50);
        dispatcher.start();
        Self::wire_reload(&bus, &state, &remote);
        let service = build_service(&mock, settings.clone(), state.clone(), dispatcher.clone());
        Rig {
            settings,
            state,
            dispatcher,
            service,
            plan,
            remote,
            failures,
            mock,
        }
    }

    /// The Python rig's `reload`: the plan is re-read from "Planning Center" into the store.
    fn wire_reload(bus: &EventBus, state: &StateStore, remote: &Remote) {
        let (state, remote) = (state.clone(), remote.clone());
        let _ = bus.subscribe(Some(EventType::ServiceReloadRequested), move |_| {
            let (state, remote) = (state.clone(), remote.clone());
            async move {
                state.mutate(|s| {
                    if let Some(plan) = s.plan.as_mut() {
                        for song in &mut plan.songs {
                            song.duration_seconds = Some(remote.lock().unwrap()[&song.id] as u32);
                        }
                    }
                    s.last_successful_plan_reload_at = Some(Utc::now());
                });
                Ok(())
            }
        });
    }

    fn remote(&self, id: &str) -> i64 {
        self.remote.lock().unwrap()[id]
    }

    fn set_remote(&self, id: &str, value: i64) {
        self.remote.lock().unwrap().insert(id.into(), value);
    }

    fn fail(&self, id: &str, failure: Failure) {
        self.failures.lock().unwrap().insert(id.into(), failure);
    }

    fn patches(&self) -> usize {
        self.mock
            .requests()
            .iter()
            .filter(|r| r.method == "PATCH")
            .count()
    }

    async fn update(&self, plan: &ServicePlan, lengths: &[Option<f64>]) {
        let preview = self.service.preview_plan(plan, lengths, "scan").unwrap();
        self.service.confirm(&preview.token, "scan").await.unwrap();
    }

    /// `fresh_update`: preview from the remote's current value, so a stale plan never hides a write.
    async fn fresh_update(&self, length: i64) {
        let mut plan = self.plan.clone();
        plan.songs[0].duration_seconds = Some(self.remote("0") as u32);
        self.update(&plan, &[Some(length as f64)]).await;
    }
}

#[tokio::test]
async fn multiple_updates_restart_restore_original() {
    for failure in [
        None,
        Some(Failure::Timeout),
        Some(Failure::Status(500)),
        Some(Failure::Status(403)),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut rig = Rig::new(Some(&path), 1).await;
        rig.fresh_update(101).await;
        if let Some(failure) = failure {
            rig.fail("0", failure);
            rig.fresh_update(102).await;
            assert_eq!(rig.remote("0"), 101, "{failure:?}");
            rig.failures.lock().unwrap().clear();
        }
        rig.fresh_update(102).await;
        assert_eq!(rig.remote("0"), 102, "{failure:?}");
        let records = rig.settings.snapshot().planning_center_length_undo;
        assert_eq!(records.len(), 1, "{failure:?}");
        assert_eq!((records[0].old_length, records[0].new_length), (100, 102));

        // Restart: a new settings service over the same file, a new length service.
        let restarted = settings_service(Some(&path));
        rig.settings = restarted.clone();
        rig.service = build_service(
            &rig.mock,
            restarted,
            rig.state.clone(),
            rig.dispatcher.clone(),
        );
        rig.service.restore().await;
        assert_eq!(rig.remote("0"), 100, "{failure:?}");
        assert!(!rig.service.undo_available(), "{failure:?}");
        assert_eq!(
            rig.service.result().reload,
            ReloadOutcome::Verified,
            "{failure:?}"
        );
    }
}

#[tokio::test]
async fn uncertain_second_write_reconciles_for_retry_or_restore() {
    for committed in [false, true] {
        for retry in [false, true] {
            let rig = Rig::new(None, 1).await;
            rig.fresh_update(101).await;
            rig.fail("0", Failure::Timeout);
            rig.fresh_update(102).await;
            if committed {
                // A timeout cannot tell us whether the server applied the request.
                rig.set_remote("0", 102);
            }
            rig.failures.lock().unwrap().clear();
            if retry {
                rig.fresh_update(103).await;
                assert_eq!(rig.remote("0"), 103, "committed={committed}");
            }
            rig.service.restore().await;
            assert_eq!(rig.remote("0"), 100, "committed={committed} retry={retry}");
            assert!(
                !rig.service.undo_available(),
                "committed={committed} retry={retry}"
            );
        }
    }
}

#[tokio::test]
async fn newer_operator_edit_never_overwritten_even_with_fresh_preview() {
    for uncertain in [false, true] {
        let rig = Rig::new(None, 1).await;
        rig.fresh_update(101).await;
        if uncertain {
            rig.fail("0", Failure::Timeout);
            rig.fresh_update(102).await;
            rig.failures.lock().unwrap().clear();
        }
        rig.set_remote("0", 999);
        let before = rig.patches();
        rig.fresh_update(103).await;
        assert!(
            rig.service.result().message.contains("Someone changed"),
            "{}",
            rig.service.result().message
        );
        assert_eq!(rig.patches(), before, "uncertain={uncertain}");
        rig.service.restore().await;
        assert_eq!(rig.remote("0"), 999, "uncertain={uncertain}");
        assert!(!rig.service.undo_available(), "uncertain={uncertain}");
    }
}

#[tokio::test]
async fn conflict_after_second_preview_preserves_original_undo() {
    let rig = Rig::new(None, 1).await;
    rig.fresh_update(101).await;
    let plan = rig.state.snapshot().plan.unwrap();
    let preview = rig
        .service
        .preview_plan(&plan, &[Some(102.0)], "scan")
        .unwrap();
    rig.set_remote("0", 999);
    rig.service.confirm(&preview.token, "scan").await.unwrap();
    let record = rig.settings.snapshot().planning_center_length_undo[0].clone();
    assert_eq!((record.old_length, record.new_length), (100, 101));
    assert_eq!(rig.remote("0"), 999);
}
