//! `test_state_models.py`: the `ServiceLoadState` invariants (the rest of that file is pydantic
//! field validation that Rust types and serde enforce structurally).

use chrono::NaiveDate;
use stagepilot_core::state::{ServiceLoadState, ServiceLoadStatus, ServicePlanCandidate};

fn target() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 12).unwrap()
}

fn candidate(id: &str, date: NaiveDate) -> ServicePlanCandidate {
    ServicePlanCandidate {
        id: id.into(),
        title: format!("Service {id}"),
        service_type_id: "service-type-1".into(),
        service_type_name: "Weekend".into(),
        target_date: date,
        service_times: vec!["09:00".into()],
    }
}

fn state(
    status: ServiceLoadStatus,
    date: Option<NaiveDate>,
    candidates: Vec<ServicePlanCandidate>,
    is_stale: bool,
) -> ServiceLoadState {
    ServiceLoadState {
        status,
        target_date: date,
        candidates,
        is_stale,
        ..ServiceLoadState::default()
    }
}

fn two() -> Vec<ServicePlanCandidate> {
    vec![candidate("plan-1", target()), candidate("plan-2", target())]
}

fn err(s: ServiceLoadState) -> String {
    s.validate().expect_err("must be rejected")
}

use ServiceLoadStatus::*;

#[test]
fn ambiguous_load_requires_at_least_two_candidates() {
    let s = state(
        Ambiguous,
        Some(target()),
        vec![candidate("plan-1", target())],
        false,
    );
    assert!(err(s).contains("requires at least two candidates"));
}

#[test]
fn candidates_are_rejected_outside_an_ambiguous_load() {
    assert!(err(state(Error, Some(target()), two(), false))
        .contains("only valid for an ambiguous load state"));
}

#[test]
fn ambiguous_candidates_must_match_the_target_date() {
    let other = NaiveDate::from_ymd_opt(2026, 7, 13).unwrap();
    let s = state(
        Ambiguous,
        Some(target()),
        vec![candidate("plan-1", target()), candidate("plan-2", other)],
        false,
    );
    assert!(err(s).contains("must match the service-load target date"));
}

#[test]
fn loaded_state_requires_a_date_and_cannot_be_stale_or_ambiguous() {
    let valid = state(Loaded, Some(target()), vec![], false);
    assert!(valid.validate().is_ok());
    assert!(!valid.is_stale && valid.candidates.is_empty());

    assert!(err(state(Loaded, None, vec![], false)).contains("requires a target date"));
    assert!(err(state(Loaded, Some(target()), vec![], true))
        .contains("successfully loaded service cannot retain"));
    assert!(err(state(Loaded, Some(target()), two(), false))
        .contains("only valid for an ambiguous load state"));
}

#[test]
fn every_non_idle_load_requires_a_target_date() {
    for status in [Loading, Loaded, NotFound, Ambiguous, Error] {
        assert!(
            err(state(status, None, vec![], false)).contains("requires a target date"),
            "{status:?}"
        );
    }
}

#[test]
fn idle_load_rejects_a_target_date() {
    assert!(err(state(Idle, Some(target()), vec![], false))
        .contains("idle service-load state cannot have a target date"));
}

#[test]
fn completed_failure_loading_and_ambiguity_can_retain_a_same_day_stale_plan() {
    for status in [NotFound, Error, Loading] {
        let s = state(status, Some(target()), vec![], true);
        assert!(s.validate().is_ok() && s.is_stale, "{status:?}");
    }
    let s = state(Ambiguous, Some(target()), two(), true);
    assert!(s.validate().is_ok() && s.is_stale);
}

#[test]
fn idle_or_successful_load_cannot_be_stale() {
    for status in [Idle, Loaded] {
        let date = if status == Idle { None } else { Some(target()) };
        assert!(
            err(state(status, date, vec![], true))
                .contains("successfully loaded service cannot retain"),
            "{status:?}"
        );
    }
}
