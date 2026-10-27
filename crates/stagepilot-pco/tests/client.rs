//! Behaviour tests for the Services API client: ports of `test_planning_center_client.py` and the
//! discovery cases of `test_planning_center_plan_discovery.py` (see docs/archive/rust-migration/phase5-test-map.md).

mod support;

use chrono::NaiveDate;
use serde_json::{json, Value};
use stagepilot_pco::client::{extract_songs, matching_service_times, PcoApi, PcoClient};
use stagepilot_pco::errors::PcoError;
use stagepilot_pco::models::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::*;

fn client(mock: &Mock, settings: PcoSettings) -> PcoClient {
    PcoClient::new(&settings, Some(mock.url.clone())).expect("client builds")
}

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn weekend() -> ServiceType {
    ServiceType {
        id: "42".into(),
        name: "Weekend Services".into(),
        sequence: 1,
        archived: false,
    }
}

// ---- test_planning_center_client.py ---------------------------------------------------------------------

#[tokio::test]
async fn list_service_types_uses_required_contract_and_maps_resources() {
    let mock = Mock::start(|_| {
        Resp::json(
            200,
            json!({"data": [service_type("10", "Weekend", 1), archived_service_type("20", "Legacy", 2)],
                "meta": {"total_count": 2, "count": 2, "next": null}}),
        )
    })
    .await;
    let types = client(&mock, pat_settings())
        .list_service_types()
        .await
        .unwrap();
    assert_eq!(
        types
            .iter()
            .map(|t| (t.id.as_str(), t.name.as_str(), t.sequence))
            .collect::<Vec<_>>(),
        vec![("10", "Weekend", 1), ("20", "Legacy", 2)]
    );
    assert!(!types[0].archived && types[1].archived);
    let requests = mock.requests();
    assert_eq!(requests.len(), 1);
    let r = &requests[0];
    assert_eq!(r.path, "/services/v2/service_types");
    assert_eq!(r.query["order"], "sequence");
    assert_eq!(r.query["per_page"], "100");
    assert_eq!(r.query["offset"], "0");
    assert_eq!(r.header("accept"), Some("application/json"));
    assert!(r
        .header("user-agent")
        .unwrap()
        .starts_with("StagePilot tests"));
    assert_eq!(r.header("x-pco-api-version"), Some("2018-11-01"));
    // base64("test-app-id:test-secret")
    assert_eq!(
        r.header("authorization"),
        Some("Basic dGVzdC1hcHAtaWQ6dGVzdC1zZWNyZXQ=")
    );
    assert!(
        !r.path.contains("test-secret") && r.query.values().all(|v| !v.contains("test-secret"))
    );
}

#[tokio::test]
async fn list_service_types_follows_safe_next_link() {
    let mock = Mock::start(|r| {
        if r.query.get("offset").map(String::as_str).unwrap_or("0") == "0" {
            Resp::json(
                200,
                json!({"data": [service_type("1", "First", 1)],
                    "meta": {"total_count": 2, "count": 1, "next": {"offset": 1}},
                    "links": {"next": "/services/v2/service_types?order=sequence&per_page=100&offset=1"}}),
            )
        } else {
            Resp::json(200, json!({"data": [service_type("2", "Second", 2)],
                "meta": {"total_count": 2, "count": 1, "next": null}, "links": {"next": null}}))
        }
    })
    .await;
    let types = client(&mock, pat_settings())
        .list_service_types()
        .await
        .unwrap();
    assert_eq!(
        types.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        vec!["1", "2"]
    );
    let offsets: Vec<_> = mock
        .requests()
        .iter()
        .map(|r| r.query.get("offset").cloned().unwrap_or("0".into()))
        .collect();
    assert_eq!(offsets, vec!["0", "1"]);
}

#[tokio::test]
async fn list_service_types_rejects_unsafe_next_link() {
    let mock = Mock::start(|_| {
        Resp::json(
            200,
            json!({"data": [service_type("1", "First", 1)],
            "links": {"next": "https://example.com/steal-credentials"}}),
        )
    })
    .await;
    let error = client(&mock, pat_settings())
        .list_service_types()
        .await
        .unwrap_err();
    assert!(
        matches!(&error, PcoError::Response(m) if m.contains("unsafe pagination URL")),
        "{error:?}"
    );
    // The credentials were never sent anywhere but the mock.
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn list_service_types_rejects_pagination_loop() {
    let mock = Mock::start(|r| {
        let self_url = format!(
            "http://{}{}?{}",
            r.header("host").unwrap(),
            r.path,
            r.query
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&")
        );
        Resp::json(
            200,
            json!({"data": [service_type("1", "First", 1)], "links": {"next": self_url}}),
        )
    })
    .await;
    let error = client(&mock, pat_settings())
        .list_service_types()
        .await
        .unwrap_err();
    // The first reply links back to a URL that differs from the first request (params merged), the
    // second reply links to itself: the repeat is caught on the third request at the latest.
    assert!(
        matches!(&error, PcoError::Response(m) if m.contains("repeated pagination request")),
        "{error:?}"
    );
    assert!(mock.requests().len() <= 3);
}

#[tokio::test]
async fn pagination_uses_meta_offset_when_links_are_absent() {
    let mock = Mock::start(|r| {
        if r.query.get("offset").map(String::as_str) == Some("0") {
            Resp::json(
                200,
                json!({"data": [service_type("1", "First", 1)],
                "meta": {"total_count": 2, "count": 1, "next": {"offset": 1}}}),
            )
        } else {
            Resp::json(
                200,
                json!({"data": [service_type("2", "Second", 2)],
                "meta": {"total_count": 2, "count": 1, "next": null}}),
            )
        }
    })
    .await;
    let types = client(&mock, pat_settings())
        .list_service_types()
        .await
        .unwrap();
    assert_eq!(types.len(), 2);
    let offsets: Vec<_> = mock
        .requests()
        .iter()
        .map(|r| r.query["offset"].clone())
        .collect();
    assert_eq!(offsets, vec!["0", "1"]);
}

#[tokio::test]
async fn pagination_rejects_empty_page_before_advertised_total() {
    let mock = Mock::start(|_| {
        Resp::json(
            200,
            json!({"data": [], "meta": {"total_count": 1, "count": 0}}),
        )
    })
    .await;
    let error = client(&mock, pat_settings())
        .list_service_types()
        .await
        .unwrap_err();
    assert!(
        matches!(&error, PcoError::Response(m) if m.contains("non-advancing")),
        "{error:?}"
    );
}

#[tokio::test]
async fn pagination_stops_after_the_page_limit() {
    // Every page is full (100 items would be needed); use the next-link path with a fresh offset
    // each time so only the safety limit can stop it.
    let counter = Arc::new(Mutex::new(0u32));
    let c = counter.clone();
    let mock = Mock::start(move |r| {
        let mut n = c.lock().unwrap();
        *n += 1;
        let _ = r;
        Resp::json(
            200,
            json!({"data": [service_type(&format!("t{n}"), "Type", 1)],
            "links": {"next": format!("/services/v2/service_types?offset={n}")}}),
        )
    })
    .await;
    let error = client(&mock, pat_settings())
        .list_service_types()
        .await
        .unwrap_err();
    assert!(
        matches!(&error, PcoError::Response(m) if m.contains("safety limit")),
        "{error:?}"
    );
    assert_eq!(mock.requests().len(), 100);
}

#[tokio::test]
async fn authentication_and_permission_errors_are_typed_and_never_echo_secrets() {
    for (status, expect_auth) in [(401u16, true), (403, false)] {
        let mock = Mock::start(move |_| {
            Resp::json(
                status,
                json!({"errors": [{"detail": "test-secret must never be echoed"}]}),
            )
        })
        .await;
        let error = client(&mock, pat_settings())
            .list_service_types()
            .await
            .unwrap_err();
        assert_eq!(
            matches!(error, PcoError::Authentication(_)),
            expect_auth,
            "{error:?}"
        );
        assert_eq!(
            matches!(error, PcoError::Permission(_)),
            !expect_auth,
            "{error:?}"
        );
        let text = error.to_string();
        assert!(!text.contains("test-secret") && !text.contains("test-app-id"));
    }
}

#[tokio::test]
async fn rate_limit_preserves_only_safe_retry_delay() {
    let mock = Mock::start(|_| {
        Resp::json(429, json!({"errors": [{"detail": "test-secret"}]})).header("Retry-After", "7")
    })
    .await;
    let error = client(&mock, pat_settings())
        .list_service_types()
        .await
        .unwrap_err();
    assert_eq!(
        error,
        PcoError::RateLimit {
            retry_after_seconds: Some(7)
        }
    );
    assert!(!error.to_string().contains("test-secret"));
    // A non-numeric or negative delay is not a delay.
    for bad in ["soon", "-3", "Wed, 21 Oct 2026 07:28:00 GMT"] {
        let mock =
            Mock::start(move |_| Resp::json(429, json!({})).header("Retry-After", bad)).await;
        let error = client(&mock, pat_settings())
            .list_service_types()
            .await
            .unwrap_err();
        assert_eq!(
            error,
            PcoError::RateLimit {
                retry_after_seconds: None
            },
            "{bad}"
        );
    }
}

#[tokio::test]
async fn other_api_errors_are_sanitized() {
    for status in [400u16, 404, 500, 503] {
        let mock = Mock::start(move |_| {
            Resp::json(status, json!({"errors": [{"detail": "test-secret"}]}))
        })
        .await;
        let error = client(&mock, pat_settings())
            .list_service_types()
            .await
            .unwrap_err();
        assert_eq!(error, PcoError::Api { status });
        assert!(!error.to_string().contains("test-secret"));
    }
}

#[tokio::test]
async fn timeout_is_translated_without_secret_leakage() {
    let mock =
        Mock::start(|_| Resp::json(200, json!({"data": []})).delayed(Duration::from_secs(3))).await;
    let mut settings = pat_settings();
    settings.request_timeout_seconds = 1.0;
    let error = client(&mock, settings)
        .list_service_types()
        .await
        .unwrap_err();
    assert!(matches!(error, PcoError::Timeout(_)), "{error:?}");
    assert!(!format!("{error:?}").contains("test-secret"));
}

#[tokio::test]
async fn transport_error_is_translated() {
    // Nothing listens on port 9 (discard) of loopback.
    let settings = pat_settings();
    let client = PcoClient::new(
        &settings,
        Some(url::Url::parse("http://127.0.0.1:9/").unwrap()),
    )
    .unwrap();
    let error = client.list_service_types().await.unwrap_err();
    assert!(
        matches!(&error, PcoError::Transport(m) if m.contains("Could not connect")),
        "{error:?}"
    );
}

#[tokio::test]
async fn malformed_service_type_responses_are_rejected() {
    let payloads: Vec<Value> = vec![
        json!({}),
        json!({"data": {}}),
        json!({"data": [{"type": "WrongType", "id": "1", "attributes": {"name": "X"}}]}),
        json!({"data": [{"type": "ServiceType", "id": "../unsafe", "attributes": {"name": "X"}}]}),
        json!({"data": [{"type": "ServiceType", "id": "1", "attributes": {}}]}),
        json!({"data": [], "meta": {"total_count": "not-a-number"}}),
    ];
    for payload in payloads {
        let body = payload.clone();
        let mock = Mock::start(move |_| Resp::json(200, body.clone())).await;
        let error = client(&mock, pat_settings())
            .list_service_types()
            .await
            .unwrap_err();
        assert!(
            matches!(&error, PcoError::Response(m) if m.contains("invalid service type response")),
            "{payload}: {error:?}"
        );
    }
}

#[tokio::test]
async fn invalid_json_is_rejected() {
    let mock = Mock::start(|_| Resp::raw(200, "<html>not JSON</html>")).await;
    let error = client(&mock, pat_settings())
        .list_service_types()
        .await
        .unwrap_err();
    assert!(matches!(&error, PcoError::Response(m) if m.contains("invalid service type response")));
}

#[test]
fn client_requires_credentials() {
    for settings in [
        PcoSettings::default(),
        PcoSettings {
            connection_method: stagepilot_core::config::ConnectionMethod::Oauth,
            ..PcoSettings::default()
        },
    ] {
        let error = PcoClient::new(&settings, None).err().unwrap();
        assert!(
            matches!(&error, PcoError::Configuration(m) if m.contains("not configured")),
            "{error:?}"
        );
    }
}

#[tokio::test]
async fn oauth_connection_method_uses_bearer_authentication() {
    let mock = Mock::start(|_| {
        Resp::json(
            200,
            json!({"data": [], "meta": {"total_count": 0, "count": 0, "next": null}}),
        )
    })
    .await;
    assert!(client(&mock, oauth_settings())
        .list_service_types()
        .await
        .unwrap()
        .is_empty());
    let r = &mock.requests()[0];
    assert_eq!(r.header("authorization"), Some("Bearer oauth-access-token"));
    assert_eq!(r.header("x-pco-api-version"), Some("2018-11-01"));
    assert!(!r.path.contains("oauth-access-token"));
}

#[tokio::test]
async fn manual_connection_method_still_uses_basic_authentication() {
    let mock = Mock::start(|_| Resp::json(200, json!({"data": []}))).await;
    client(&mock, pat_settings())
        .list_service_types()
        .await
        .unwrap();
    assert_eq!(
        mock.requests()[0].header("authorization"),
        Some("Basic dGVzdC1hcHAtaWQ6dGVzdC1zZWNyZXQ=")
    );
}

#[test]
fn matching_service_times_uses_dst_aware_zone_for_denver() {
    // 06:30 UTC on 2026-07-12 is Saturday 23:30 in Phoenix (no DST) but Sunday 00:30 in Denver.
    let time = PlanTimeResource {
        kind: "PlanTime".into(),
        id: "pt1".into(),
        attributes: PlanTimeAttributes {
            starts_at: chrono::DateTime::parse_from_rfc3339("2026-07-12T06:30:00Z").unwrap(),
            ends_at: None,
            name: String::new(),
            time_type: "service".into(),
        },
    };
    let sunday = day(2026, 7, 12);
    let denver = matching_service_times(
        std::slice::from_ref(&time),
        sunday,
        sunday,
        chrono_tz::America::Denver,
    );
    let phoenix = matching_service_times(
        std::slice::from_ref(&time),
        sunday,
        sunday,
        chrono_tz::America::Phoenix,
    );
    assert_eq!(denver.len(), 1);
    assert_eq!(phoenix.len(), 0);
}

// ---- plan discovery --------------------------------------------------------------------------------------------

/// A mock Planning Center with per-plan times and items.
async fn discovery(
    plans: Vec<Value>,
    times: Vec<(&'static str, Vec<Value>)>,
    items: Vec<(&'static str, Vec<Value>)>,
) -> Mock {
    Mock::start(move |r| {
        let p = r.path.as_str();
        let plan_id = p.rsplit('/').nth(1).unwrap_or("");
        if p.ends_with("/plans") {
            Resp::json(200, page(plans.clone()))
        } else if p.ends_with("/plan_times") {
            match times.iter().find(|(id, _)| *id == plan_id) {
                Some((_, t)) => Resp::json(200, page(t.clone())),
                None => Resp::json(500, json!({"unexpected": plan_id})),
            }
        } else if p.ends_with("/items") {
            match items.iter().find(|(id, _)| *id == plan_id) {
                Some((_, i)) => Resp::json(200, page(i.clone())),
                None => Resp::json(500, json!({"unexpected": plan_id})),
            }
        } else {
            Resp::json(404, json!({}))
        }
    })
    .await
}

const TZ: &str = "America/Los_Angeles";

#[tokio::test]
async fn no_plans_returns_not_found_without_detail_requests() {
    let mock = discovery(vec![], vec![], vec![]).await;
    let result = client(&mock, pat_settings())
        .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, None, 0)
        .await
        .unwrap();
    assert!(
        matches!(result, PlanDiscoveryResult::NotFound { target_date, .. } if target_date == day(2026, 7, 12))
    );
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn today_plan_wins_when_search_window_contains_future_plan() {
    let mock = discovery(
        vec![plan("today", "Today"), plan("later", "Later")],
        vec![
            (
                "today",
                vec![plan_time("t1", "2026-07-12T17:00:00Z", "service")],
            ),
            (
                "later",
                vec![plan_time("t2", "2026-07-14T17:00:00Z", "service")],
            ),
        ],
        vec![(
            "today",
            vec![item("i1", "song", "Holy", 1, Some(240), Some("s1"))],
        )],
    )
    .await;
    let result = client(&mock, pat_settings())
        .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, None, 3)
        .await
        .unwrap();
    let PlanDiscoveryResult::Loaded {
        candidate, plan, ..
    } = result
    else {
        panic!("{result:?}")
    };
    assert_eq!(
        (candidate.id.as_str(), candidate.target_date),
        ("today", day(2026, 7, 12))
    );
    assert_eq!(plan.songs.len(), 1);
    assert_eq!(plan.service_times, vec!["10:00"]);
}

#[tokio::test]
async fn nearest_upcoming_plan_wins_after_empty_dates() {
    let mock = discovery(
        vec![plan("next", "Next"), plan("later", "Later")],
        vec![
            (
                "next",
                vec![plan_time("t1", "2026-07-13T17:00:00Z", "service")],
            ),
            (
                "later",
                vec![plan_time("t2", "2026-07-14T17:00:00Z", "service")],
            ),
        ],
        vec![(
            "next",
            vec![item("i1", "song", "Holy", 1, Some(240), Some("s1"))],
        )],
    )
    .await;
    let result = client(&mock, pat_settings())
        .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, None, 7)
        .await
        .unwrap();
    let PlanDiscoveryResult::Loaded { candidate, .. } = result else {
        panic!("{result:?}")
    };
    assert_eq!(candidate.id, "next");
    // Only the loaded plan's items were fetched.
    assert!(mock
        .requests()
        .iter()
        .filter(|r| r.path.ends_with("/items"))
        .all(|r| r.path.contains("/plans/next/")));
}

#[tokio::test]
async fn plan_after_configured_lookahead_is_not_considered() {
    let mock = discovery(
        vec![plan("far", "Far")],
        vec![(
            "far",
            vec![plan_time("t1", "2026-07-20T17:00:00Z", "service")],
        )],
        vec![],
    )
    .await;
    let result = client(&mock, pat_settings())
        .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, None, 3)
        .await
        .unwrap();
    assert!(matches!(result, PlanDiscoveryResult::NotFound { .. }));
}

#[tokio::test]
async fn plan_window_uses_consecutive_local_midnights_across_dst() {
    // 2026-03-08 is the US spring-forward day: the local day is 23 hours long.
    let mock = discovery(vec![], vec![], vec![]).await;
    client(&mock, pat_settings())
        .load_plan_for_date(&weekend(), day(2026, 3, 8), TZ, None, 0)
        .await
        .unwrap();
    let r = &mock.requests()[0];
    assert_eq!(r.query["filter"], "after,before");
    assert_eq!(r.query["after"], "2026-03-08T07:59:59Z");
    assert_eq!(r.query["before"], "2026-03-09T07:00:00Z");
    assert_eq!(r.query["order"], "sort_date");
}

#[tokio::test]
async fn exact_local_date_matching_converts_utc_service_times() {
    // 03:30 UTC on 2026-07-13 is 20:30 on 2026-07-12 in Los Angeles.
    let mock = discovery(
        vec![plan("p", "Evening")],
        vec![("p", vec![plan_time("t", "2026-07-13T03:30:00Z", "service")])],
        vec![("p", vec![item("i", "song", "Holy", 1, Some(1), None)])],
    )
    .await;
    let result = client(&mock, pat_settings())
        .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, None, 0)
        .await
        .unwrap();
    let PlanDiscoveryResult::Loaded { candidate, .. } = result else {
        panic!("{result:?}")
    };
    assert_eq!(candidate.target_date, day(2026, 7, 12));
    assert_eq!(
        candidate.service_times[0].format("%H:%M").to_string(),
        "20:30"
    );
}

#[tokio::test]
async fn plan_with_only_next_local_day_service_time_is_not_found() {
    let mock = discovery(
        vec![plan("p", "Late")],
        vec![("p", vec![plan_time("t", "2026-07-13T07:30:00Z", "service")])],
        vec![],
    )
    .await;
    let result = client(&mock, pat_settings())
        .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, None, 0)
        .await
        .unwrap();
    assert!(matches!(result, PlanDiscoveryResult::NotFound { .. }));
}

#[tokio::test]
async fn non_service_times_never_qualify_a_plan() {
    let mock = discovery(
        vec![plan("p", "Rehearsal")],
        vec![(
            "p",
            vec![plan_time("t", "2026-07-12T17:00:00Z", "rehearsal")],
        )],
        vec![],
    )
    .await;
    let result = client(&mock, pat_settings())
        .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, None, 0)
        .await
        .unwrap();
    assert!(matches!(result, PlanDiscoveryResult::NotFound { .. }));
}

fn two_plans_same_day() -> (Vec<Value>, Vec<(&'static str, Vec<Value>)>) {
    (
        vec![plan("a", "Early"), plan("b", "Late")],
        vec![
            (
                "a",
                vec![plan_time("t1", "2026-07-12T16:00:00Z", "service")],
            ),
            (
                "b",
                vec![plan_time("t2", "2026-07-12T18:00:00Z", "service")],
            ),
        ],
    )
}

#[tokio::test]
async fn multiple_matching_plans_are_ambiguous_without_item_fetch() {
    let (plans, times) = two_plans_same_day();
    let mock = discovery(plans, times, vec![]).await;
    let result = client(&mock, pat_settings())
        .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, None, 0)
        .await
        .unwrap();
    let PlanDiscoveryResult::Ambiguous {
        candidates,
        target_date,
        ..
    } = result
    else {
        panic!("{result:?}")
    };
    assert_eq!(target_date, day(2026, 7, 12));
    assert_eq!(
        candidates.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert!(mock.requests().iter().all(|r| !r.path.ends_with("/items")));
}

#[tokio::test]
async fn explicit_selection_loads_only_the_matching_selected_plan() {
    let (plans, times) = two_plans_same_day();
    let mock = discovery(
        plans,
        times,
        vec![(
            "b",
            vec![item("i", "song", "Holy", 1, Some(200), Some("s"))],
        )],
    )
    .await;
    let result = client(&mock, pat_settings())
        .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, Some("b"), 0)
        .await
        .unwrap();
    let PlanDiscoveryResult::Loaded { plan, .. } = result else {
        panic!("{result:?}")
    };
    assert_eq!(plan.id, "b");
    assert!(mock
        .requests()
        .iter()
        .filter(|r| r.path.ends_with("/items"))
        .all(|r| r.path.contains("/plans/b/")));
}

#[tokio::test]
async fn explicit_selection_must_match_a_current_day_candidate() {
    let (plans, times) = two_plans_same_day();
    let mock = discovery(plans, times, vec![]).await;
    let error = client(&mock, pat_settings())
        .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, Some("zzz"), 0)
        .await
        .unwrap_err();
    assert!(matches!(error, PcoError::PlanSelection(_)), "{error:?}");
    // An unsafe selected id is refused before any request.
    let error = client(&mock, pat_settings())
        .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, Some("../x"), 0)
        .await
        .unwrap_err();
    assert!(matches!(error, PcoError::Configuration(_)), "{error:?}");
}

#[tokio::test]
async fn song_extraction_orders_linked_and_generic_items_and_reports_skips() {
    let items = vec![(
        "p",
        vec![
            item("h", "header", "Welcome", 1, None, None),
            item("s2", "song", "  Holy Forever  ", 3, Some(336), Some("7002")),
            item("s1", "song", "Amazing Grace", 2, Some(240), Some("7001")),
            item("m", "media", "Video", 4, Some(90), None),
            item("g", "item", "Prayer", 5, Some(120), None),
            item("n", "song", "   ", 6, Some(60), Some("7003")),
            item("lin", "item", "Offering Song", 7, Some(150), Some("7004")),
        ],
    )];
    let mock = discovery(
        vec![plan("p", "Sunday")],
        vec![("p", vec![plan_time("t", "2026-07-12T17:00:00Z", "service")])],
        items,
    )
    .await;
    let result = client(&mock, pat_settings())
        .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, None, 0)
        .await
        .unwrap();
    let PlanDiscoveryResult::Loaded {
        plan,
        skipped_items,
        ..
    } = result
    else {
        panic!("{result:?}")
    };
    let songs: Vec<_> = plan
        .songs
        .iter()
        .map(|s| (s.order, s.id.as_str(), s.title.as_str(), s.is_generic))
        .collect();
    assert_eq!(
        songs,
        vec![
            (1, "s1", "Amazing Grace", false),
            (2, "s2", "Holy Forever", false),
            (3, "lin", "Offering Song", false),
        ]
    );
    // `Prayer` is neither a song type nor linked to a song, so it is skipped as `not_song`.
    assert!(skipped_items
        .iter()
        .any(|i| i.item_id == "g" && i.reason.as_str() == "not_song"));
    assert_eq!(skipped_items.len(), 4);
}

#[test]
fn extract_songs_skip_reasons_match_python() {
    let make = |id: &str, kind: &str, title: &str, seq: i64, song: Option<&str>| {
        serde_json::from_value::<ItemResource>(item(id, kind, title, seq, Some(60), song)).unwrap()
    };
    let items = vec![
        make("h", "header", "Welcome", 1, None),
        make("m", "media", "Video", 2, None),
        make("g", "item", "Prayer", 3, None),
        make("e", "song", "   ", 4, Some("1")),
        make("ok", "song", "Real", 5, None),
    ];
    let (songs, skipped) = extract_songs(&items);
    assert_eq!(
        songs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        vec!["ok"]
    );
    assert!(
        songs[0].is_generic,
        "a song-typed item without a linked song is generic"
    );
    assert_eq!(
        skipped
            .iter()
            .map(|s| (s.item_id.as_str(), s.reason.as_str(), s.title.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("h", "header", "Welcome"),
            ("m", "media", "Video"),
            ("g", "not_song", "Prayer"),
            ("e", "missing_title", "(untitled)"),
        ]
    );
}

#[tokio::test]
async fn item_pagination_stays_on_item_endpoint_and_sorts_across_pages() {
    let mock = Mock::start(|r| {
        let p = r.path.as_str();
        if p.ends_with("/plans") {
            Resp::json(200, page(vec![plan("p", "Sunday")]))
        } else if p.ends_with("/plan_times") {
            Resp::json(
                200,
                page(vec![plan_time("t", "2026-07-12T17:00:00Z", "service")]),
            )
        } else if r.query.get("offset").map(String::as_str) == Some("0") {
            Resp::json(
                200,
                json!({"data": [item("late", "song", "Second", 9, Some(100), None)],
                "meta": {"total_count": 2, "next": {"offset": 1}}}),
            )
        } else {
            Resp::json(
                200,
                json!({"data": [item("early", "song", "First", 1, Some(100), None)],
                "meta": {"total_count": 2}}),
            )
        }
    })
    .await;
    let result = client(&mock, pat_settings())
        .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, None, 0)
        .await
        .unwrap();
    let PlanDiscoveryResult::Loaded { plan, .. } = result else {
        panic!("{result:?}")
    };
    assert_eq!(
        plan.songs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        vec!["early", "late"]
    );
    let item_requests: Vec<_> = mock
        .requests()
        .into_iter()
        .filter(|r| r.path.ends_with("/items"))
        .collect();
    assert_eq!(item_requests.len(), 2);
    assert!(item_requests.iter().all(|r| r.query["include"] == "song"));
}

#[tokio::test]
async fn malformed_and_duplicate_discovery_resources_are_rejected() {
    let bad_plans = vec![
        vec![json!({"type": "Plan", "id": "../x", "attributes": {"title": "x"}})],
        vec![json!({"type": "Nope", "id": "1", "attributes": {"title": "x"}})],
        vec![plan("1", "a"), plan("1", "b")], // duplicate ids
    ];
    for plans in bad_plans {
        let mock = discovery(plans, vec![], vec![]).await;
        let error = client(&mock, pat_settings())
            .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, None, 0)
            .await
            .unwrap_err();
        assert!(matches!(error, PcoError::Response(_)), "{error:?}");
    }
    // A bad identifier is rejected before any request.
    let mock = discovery(vec![], vec![], vec![]).await;
    let bad_type = ServiceType {
        id: "../x".into(),
        ..weekend()
    };
    let error = client(&mock, pat_settings())
        .load_plan_for_date(&bad_type, day(2026, 7, 12), TZ, None, 0)
        .await
        .unwrap_err();
    assert!(matches!(error, PcoError::Configuration(_)));
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn lookahead_and_timezone_are_validated() {
    let mock = discovery(vec![], vec![], vec![]).await;
    let c = client(&mock, pat_settings());
    let error = c
        .load_plan_for_date(&weekend(), day(2026, 7, 12), TZ, None, 366)
        .await
        .unwrap_err();
    assert!(matches!(&error, PcoError::Configuration(m) if m.contains("between 0 and 365")));
    let error = c
        .load_plan_for_date(&weekend(), day(2026, 7, 12), "Not/AZone", None, 0)
        .await
        .unwrap_err();
    assert!(matches!(&error, PcoError::Configuration(m) if m.contains("timezone is invalid")));
}

// ---- all service types ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn all_service_types_isolates_one_failing_service_type() {
    let mock = Mock::start(|r| {
        let p = r.path.as_str();
        if p.contains("/service_types/bad/") {
            Resp::json(500, json!({}))
        } else if p.ends_with("/service_types/good/plans") {
            Resp::json(200, page(vec![plan("p", "Good")]))
        } else if p.ends_with("/plan_times") {
            Resp::json(
                200,
                page(vec![plan_time("t", "2026-07-12T17:00:00Z", "service")]),
            )
        } else if p.ends_with("/items") {
            Resp::json(
                200,
                page(vec![item("i", "song", "Holy", 1, Some(10), None)]),
            )
        } else {
            Resp::json(404, json!({}))
        }
    })
    .await;
    let types = vec![
        ServiceType {
            id: "bad".into(),
            name: "Bad".into(),
            sequence: 0,
            archived: false,
        },
        ServiceType {
            id: "good".into(),
            name: "Good".into(),
            sequence: 1,
            archived: false,
        },
    ];
    let result = client(&mock, pat_settings())
        .load_plan_for_service_types(&types, day(2026, 7, 12), TZ, None, 0)
        .await
        .unwrap();
    assert!(
        matches!(result, PlanDiscoveryResult::Loaded { ref plan, .. } if plan.service_type == "Good")
    );
}

#[tokio::test]
async fn all_service_types_raises_the_first_failure_when_every_type_fails() {
    let mock = Mock::start(|r| {
        if r.path.contains("/service_types/a/") {
            Resp::json(403, json!({}))
        } else {
            Resp::json(500, json!({}))
        }
    })
    .await;
    let types = vec![
        ServiceType {
            id: "a".into(),
            name: "A".into(),
            sequence: 0,
            archived: false,
        },
        ServiceType {
            id: "b".into(),
            name: "B".into(),
            sequence: 1,
            archived: false,
        },
    ];
    let error = client(&mock, pat_settings())
        .load_plan_for_service_types(&types, day(2026, 7, 12), TZ, None, 0)
        .await
        .unwrap_err();
    assert!(matches!(error, PcoError::Permission(_)), "{error:?}");
}

#[tokio::test]
async fn all_service_types_surfaces_cross_type_ties_as_candidates() {
    let mock = Mock::start(|r| {
        let p = r.path.as_str();
        if p.ends_with("/plans") {
            let id = if p.contains("/service_types/one/") {
                "p1"
            } else {
                "p2"
            };
            Resp::json(200, page(vec![plan(id, id)]))
        } else if p.ends_with("/plan_times") {
            Resp::json(
                200,
                page(vec![plan_time("t", "2026-07-12T17:00:00Z", "service")]),
            )
        } else {
            Resp::json(200, page(vec![]))
        }
    })
    .await;
    let types = vec![
        ServiceType {
            id: "one".into(),
            name: "One".into(),
            sequence: 0,
            archived: false,
        },
        ServiceType {
            id: "two".into(),
            name: "Two".into(),
            sequence: 1,
            archived: false,
        },
    ];
    let c = client(&mock, pat_settings());
    let result = c
        .load_plan_for_service_types(&types, day(2026, 7, 12), TZ, None, 0)
        .await
        .unwrap();
    let PlanDiscoveryResult::Ambiguous {
        candidates,
        service_type,
        ..
    } = result
    else {
        panic!("{result:?}")
    };
    assert!(service_type.is_none());
    // Same start time: ordering falls back to the plan id.
    assert_eq!(
        candidates.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["p1", "p2"]
    );
    let resolved = c
        .resolve_selected_plan(&candidates, &types, day(2026, 7, 12), "p2")
        .await
        .unwrap();
    assert!(
        matches!(resolved, PlanDiscoveryResult::Loaded { ref candidate, .. } if candidate.service_type_id == "two")
    );
}

// ---- single-item reads and writes ---------------------------------------------------------------------------------------

fn item_doc(id: &str, length: i64) -> Value {
    json!({"data": item(id, "song", "Song", 1, Some(length), Some("s"))})
}

#[tokio::test]
async fn item_requests_validate_identifiers_and_lengths_before_any_request() {
    let mock = Mock::start(|_| Resp::json(200, item_doc("i", 1))).await;
    let c = client(&mock, pat_settings());
    let allow = || true;
    for (st, p, i) in [("../x", "p", "i"), ("t", "a b", "i"), ("t", "p", "")] {
        assert!(matches!(
            c.get_plan_item(st, p, i).await.unwrap_err(),
            PcoError::Configuration(_)
        ));
    }
    for length in [-1, 86_401] {
        let error = c
            .update_plan_item_length("t", "p", "i", length, Some(1), &allow)
            .await
            .unwrap_err();
        assert!(matches!(&error, PcoError::Configuration(m) if m == "Invalid plan item length."));
    }
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn patch_sends_the_item_attributes_body_and_checks_the_returned_item() {
    let mock =
        Mock::start(|r| Resp::json(200, item_doc(r.path.rsplit('/').next().unwrap(), 300))).await;
    let c = client(&mock, pat_settings());
    let updated = c
        .update_plan_item_length("t", "p", "i", 300, Some(240), &|| true)
        .await
        .unwrap();
    assert_eq!(updated.attributes.length, Some(300));
    let r = &mock.requests()[0];
    assert_eq!(
        (r.method.as_str(), r.path.as_str()),
        ("PATCH", "/services/v2/service_types/t/plans/p/items/i")
    );
    assert_eq!(
        r.json(),
        json!({"data": {"type": "Item", "attributes": {"length": 300}}})
    );
    // A response for a different item is rejected.
    let wrong = Mock::start(|_| Resp::json(200, item_doc("other", 300))).await;
    let error = client(&wrong, pat_settings())
        .get_plan_item("t", "p", "i")
        .await
        .unwrap_err();
    assert!(matches!(&error, PcoError::Response(m) if m.contains("invalid item response")));
}

#[tokio::test]
async fn patch_is_refused_while_playback_runs() {
    let mock = Mock::start(|_| Resp::json(200, item_doc("i", 1))).await;
    let error = client(&mock, pat_settings())
        .update_plan_item_length("t", "p", "i", 10, Some(1), &|| false)
        .await
        .unwrap_err();
    assert!(matches!(&error, PcoError::Configuration(m) if m.contains("Playback started")));
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn write_401_and_403_are_permission_errors_with_the_write_message() {
    for status in [401u16, 403] {
        let mock = Mock::start(move |_| Resp::json(status, json!({}))).await;
        let error = client(&mock, pat_settings())
            .update_plan_item_length("t", "p", "i", 10, Some(1), &|| true)
            .await
            .unwrap_err();
        assert!(
            matches!(&error, PcoError::Permission(m) if m.contains("can't edit this plan")),
            "{error:?}"
        );
    }
}

#[tokio::test(start_paused = false)]
async fn rate_limited_write_waits_once_rechecks_the_item_then_retries() {
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let c2 = calls.clone();
    let mock = Mock::start(move |r| {
        let mut calls = c2.lock().unwrap();
        calls.push(r.method.clone());
        match (
            r.method.as_str(),
            calls.iter().filter(|m| *m == "PATCH").count(),
        ) {
            ("PATCH", 1) => Resp::json(429, json!({})).header("Retry-After", "0"),
            ("GET", _) => Resp::json(200, item_doc("i", 240)),
            _ => Resp::json(200, item_doc("i", 300)),
        }
    })
    .await;
    let updated = client(&mock, pat_settings())
        .update_plan_item_length("t", "p", "i", 300, Some(240), &|| true)
        .await
        .unwrap();
    assert_eq!(updated.attributes.length, Some(300));
    assert_eq!(calls.lock().unwrap().clone(), vec!["PATCH", "GET", "PATCH"]);
}

#[tokio::test]
async fn rate_limited_write_aborts_when_someone_else_changed_the_item() {
    let mock = Mock::start(|r| match r.method.as_str() {
        "PATCH" => Resp::json(429, json!({})).header("Retry-After", "0"),
        _ => Resp::json(200, item_doc("i", 999)), // not the expected 240
    })
    .await;
    let error = client(&mock, pat_settings())
        .update_plan_item_length("t", "p", "i", 300, Some(240), &|| true)
        .await
        .unwrap_err();
    assert!(matches!(error, PcoError::LengthConflict(_)), "{error:?}");
    assert_eq!(
        mock.requests()
            .iter()
            .filter(|r| r.method == "PATCH")
            .count(),
        1
    );
}

#[tokio::test]
async fn second_429_or_missing_retry_after_raises_the_rate_limit_error() {
    let mock = Mock::start(|_| Resp::json(429, json!({})).header("Retry-After", "0")).await;
    let error = client(&mock, pat_settings())
        .get_plan_item("t", "p", "i")
        .await
        .unwrap_err();
    // GET: the first 429 retries once (attempt 0, delay known), the second raises.
    assert_eq!(
        error,
        PcoError::RateLimit {
            retry_after_seconds: Some(0)
        }
    );
    assert_eq!(mock.requests().len(), 2);
    let none = Mock::start(|_| Resp::json(429, json!({}))).await;
    let error = client(&none, pat_settings())
        .update_plan_item_length("t", "p", "i", 10, Some(1), &|| true)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        PcoError::RateLimit {
            retry_after_seconds: None
        }
    );
    assert_eq!(none.requests().len(), 1);
}

#[tokio::test]
async fn write_timeout_says_the_change_may_not_be_confirmed() {
    let mock =
        Mock::start(|_| Resp::json(200, item_doc("i", 1)).delayed(Duration::from_secs(3))).await;
    let mut settings = pat_settings();
    settings.request_timeout_seconds = 1.0;
    let error = client(&mock, settings)
        .update_plan_item_length("t", "p", "i", 10, Some(1), &|| true)
        .await
        .unwrap_err();
    assert!(
        matches!(&error, PcoError::Timeout(m) if m.contains("may not be confirmed")),
        "{error:?}"
    );
}

#[tokio::test]
async fn redirects_are_never_followed_so_credentials_cannot_be_forwarded() {
    let mock =
        Mock::start(|_| Resp::json(302, json!({})).header("Location", "https://example.com/steal"))
            .await;
    let error = client(&mock, pat_settings())
        .list_service_types()
        .await
        .unwrap_err();
    assert_eq!(error, PcoError::Api { status: 302 });
}

#[test]
fn secrets_never_appear_in_debug_output() {
    let settings = pat_settings();
    let text = format!("{settings:?}");
    assert!(
        !text.contains("test-secret") && !text.contains("test-app-id"),
        "{text}"
    );
    let oauth = format!("{:?}", oauth_settings());
    assert!(!oauth.contains("oauth-access-token"), "{oauth}");
}
