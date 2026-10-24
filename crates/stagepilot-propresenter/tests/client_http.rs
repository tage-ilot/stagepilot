//! Client tests against a local mock HTTP server, using the expectations of
//! `backend/tests/test_propresenter_client.py` (httpx `MockTransport` there, a real loopback
//! server here, so reqwest's wire behaviour, timeouts and error mapping are exercised too).

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::{json, Value};
use stagepilot_core::config::ProPresenterSettings;
use stagepilot_propresenter::client::{HttpProPresenterClient, ProPresenterClient, Tuning};
use stagepilot_propresenter::{PpError, Timer};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Log = Arc<Mutex<Vec<(String, String, Option<Value>)>>>;
type Responder = Arc<dyn Fn(&str, &str, Option<&Value>, usize) -> Response + Send + Sync>;

struct Mock {
    port: u16,
    log: Log,
}

impl Mock {
    fn requests(&self) -> Vec<(String, String, Option<Value>)> {
        self.log.lock().unwrap().clone()
    }
    fn pairs(&self) -> Vec<(String, String)> {
        self.requests()
            .into_iter()
            .map(|(m, p, _)| (m, p))
            .collect()
    }
}

#[derive(Clone)]
struct AppState {
    log: Log,
    responder: Responder,
}

async fn handle(State(state): State<AppState>, method: Method, uri: Uri, body: Bytes) -> Response {
    let parsed: Option<Value> = if body.is_empty() {
        None
    } else {
        serde_json::from_slice(&body).ok()
    };
    let count = {
        let mut log = state.log.lock().unwrap();
        log.push((method.to_string(), uri.path().to_string(), parsed.clone()));
        log.len()
    };
    (state.responder)(method.as_str(), uri.path(), parsed.as_ref(), count)
}

async fn serve(responder: Responder) -> Mock {
    let log: Log = Arc::default();
    let app = Router::new().fallback(handle).with_state(AppState {
        log: log.clone(),
        responder,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Mock { port, log }
}

fn json_response(value: Value) -> Response {
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        value.to_string(),
    )
        .into_response()
}

fn no_content() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

fn timer_payload(name: &str, uuid: &str, index: u32, duration: u32) -> Value {
    json!({
        "id": {"uuid": uuid, "name": name, "index": index},
        "allows_overrun": false,
        "countdown": {"duration": duration},
        "state": "stopped",
        "time": "00:01:00",
    })
}

fn client(mock: &Mock, tuning: Tuning) -> HttpProPresenterClient {
    let settings = ProPresenterSettings {
        enabled: true,
        host: "127.0.0.1".into(),
        port: mock.port,
        ..ProPresenterSettings::default()
    };
    HttpProPresenterClient::with_tuning(&settings, tuning)
}

fn fast() -> Tuning {
    Tuning {
        timer_update_interval: Duration::from_millis(1),
        reset_verification_interval: Duration::from_millis(1),
        ..Tuning::default()
    }
}

fn parse_timer(value: Value) -> Timer {
    Timer::from_value(&value).unwrap()
}

#[tokio::test]
async fn client_preserves_timer_identity_when_updating_duration() {
    let saved = Arc::new(Mutex::new(60u32));
    let responder: Responder = {
        let saved = saved.clone();
        Arc::new(move |method, path, body, _| match (method, path) {
            ("GET", "/v1/timers") => json_response(json!([timer_payload(
                "Song Countdown",
                "timer-uuid",
                0,
                *saved.lock().unwrap()
            )])),
            ("GET", "/v1/timer/timer-uuid") => json_response(timer_payload(
                "Song Countdown",
                "timer-uuid",
                0,
                *saved.lock().unwrap(),
            )),
            ("PUT", "/v1/timer/timer-uuid/reset") => {
                *saved.lock().unwrap() =
                    body.unwrap()["countdown"]["duration"].as_u64().unwrap() as u32;
                no_content()
            }
            _ => json_response(json!({})),
        })
    };
    let mock = serve(responder).await;
    let c = client(&mock, fast());
    let timer = c.find_timer("song countdown").await.unwrap();
    let updated = c.set_timer_duration(&timer, 336).await.unwrap();
    assert_eq!(updated.countdown.unwrap().duration, 336);
    let requests = mock.requests();
    let n = requests.len();
    assert_eq!(
        requests[n - 2],
        (
            "PUT".to_string(),
            "/v1/timer/timer-uuid/reset".to_string(),
            Some(json!({
                "id": {"uuid": "timer-uuid", "name": "Song Countdown", "index": 0},
                "allows_overrun": false,
                "countdown": {"duration": 336}
            }))
        )
    );
    assert_eq!(
        (requests[n - 1].0.as_str(), requests[n - 1].1.as_str()),
        ("GET", "/v1/timer/timer-uuid")
    );
}

#[tokio::test]
async fn client_can_set_countdown_duration_to_zero_for_position_reset() {
    let saved = Arc::new(Mutex::new(60u32));
    let responder: Responder = {
        let saved = saved.clone();
        Arc::new(move |method, _path, body, _| {
            if method == "PUT" {
                *saved.lock().unwrap() =
                    body.unwrap()["countdown"]["duration"].as_u64().unwrap() as u32;
                return no_content();
            }
            json_response(timer_payload(
                "Song Countdown",
                "timer-uuid",
                0,
                *saved.lock().unwrap(),
            ))
        })
    };
    let mock = serve(responder).await;
    let c = client(&mock, fast());
    let timer = parse_timer(timer_payload("Song Countdown", "timer-uuid", 0, 60));
    let updated = c.set_timer_duration(&timer, 0).await.unwrap();
    assert_eq!(updated.countdown.unwrap().duration, 0);
    assert_eq!(
        mock.requests(),
        [
            (
                "PUT".to_string(),
                "/v1/timer/timer-uuid/reset".to_string(),
                Some(json!({
                    "id": {"uuid": "timer-uuid", "name": "Song Countdown", "index": 0},
                    "allows_overrun": false,
                    "countdown": {"duration": 0}
                }))
            ),
            ("GET".to_string(), "/v1/timer/timer-uuid".to_string(), None),
        ]
    );
}

#[tokio::test]
async fn client_repeats_reset_when_timer_display_is_not_zero() {
    let resets = Arc::new(Mutex::new(0usize));
    let responder: Responder = {
        let resets = resets.clone();
        Arc::new(move |_method, path, _body, _| {
            if path.ends_with("/reset") {
                *resets.lock().unwrap() += 1;
                return no_content();
            }
            let time = if *resets.lock().unwrap() < 2 {
                "00:00:12"
            } else {
                "00:00:00"
            };
            let mut payload = timer_payload("Song Countdown", "timer-uuid", 0, 0);
            payload["time"] = json!(time);
            json_response(payload)
        })
    };
    let mock = serve(responder).await;
    let c = client(
        &mock,
        Tuning {
            reset_verification_attempts: 1,
            ..fast()
        },
    );
    c.reset_timer("timer-uuid").await.unwrap();
    assert_eq!(*resets.lock().unwrap(), 2);
}

#[tokio::test]
async fn reset_gives_up_after_two_commands_with_the_observed_time() {
    let mock = serve(Arc::new(|_m, path, _b, _| {
        if path.ends_with("/reset") {
            return no_content();
        }
        let mut payload = timer_payload("Song Countdown", "timer-uuid", 0, 0);
        payload["time"] = json!("00:00:12");
        json_response(payload)
    }))
    .await;
    let c = client(
        &mock,
        Tuning {
            reset_verification_attempts: 2,
            ..fast()
        },
    );
    let error = c.reset_timer("timer-uuid").await.unwrap_err();
    assert_eq!(
        error,
        PpError::Response(
            "ProPresenter did not confirm that the timer reset to 0:00; it still reported 00:00:12.".into()
        )
    );
    let resets = mock
        .pairs()
        .iter()
        .filter(|(_, p)| p.ends_with("/reset"))
        .count();
    assert_eq!(resets, 2);
}

#[tokio::test]
async fn client_waits_for_timer_duration_to_be_visible_before_returning() {
    let reads = Arc::new(Mutex::new(0usize));
    let responder: Responder = {
        let reads = reads.clone();
        Arc::new(move |method, _path, _body, _| {
            if method == "PUT" {
                return no_content();
            }
            let mut r = reads.lock().unwrap();
            *r += 1;
            json_response(timer_payload(
                "Song Countdown",
                "timer-uuid",
                0,
                if *r < 3 { 60 } else { 281 },
            ))
        })
    };
    let mock = serve(responder).await;
    let c = client(&mock, fast());
    let timer = parse_timer(timer_payload("Song Countdown", "timer-uuid", 0, 60));
    let updated = c.set_timer_duration(&timer, 281).await.unwrap();
    assert_eq!(updated.countdown.unwrap().duration, 281);
    assert_eq!(*reads.lock().unwrap(), 3);
}

#[tokio::test]
async fn client_converts_an_elapsed_timer_to_countdown_when_setting_duration() {
    let mock = serve(Arc::new(|method, _p, _b, _| {
        if method == "PUT" {
            no_content()
        } else {
            json_response(timer_payload("Song Countdown", "timer-uuid", 0, 253))
        }
    }))
    .await;
    let c = client(&mock, fast());
    let elapsed = parse_timer(json!({
        "id": {"uuid": "timer-uuid", "name": "Song Countdown", "index": 0},
        "allows_overrun": false,
        "elapsed": {"start_time": 0}
    }));
    let converted = c.set_timer_duration(&elapsed, 253).await.unwrap();
    assert_eq!(converted.countdown.unwrap().duration, 253);
    let puts: Vec<_> = mock
        .requests()
        .into_iter()
        .filter(|(m, _, _)| m == "PUT")
        .collect();
    assert_eq!(puts.len(), 1);
    assert_eq!(puts[0].1, "/v1/timer/timer-uuid/reset");
    assert_eq!(
        puts[0].2,
        Some(json!({
            "id": {"uuid": "timer-uuid", "name": "Song Countdown", "index": 0},
            "allows_overrun": false,
            "countdown": {"duration": 253}
        }))
    );
}

#[tokio::test]
async fn client_rejects_unconfirmed_timer_duration() {
    let mock = serve(Arc::new(|method, _p, _b, _| {
        if method == "PUT" {
            return no_content();
        }
        json_response(json!({
            "id": {"uuid": "timer-uuid", "name": "Song Countdown", "index": 0},
            "allows_overrun": false,
            "count_down_to_time": {"time_of_day": 3600, "period": "pm"}
        }))
    }))
    .await;
    let c = client(
        &mock,
        Tuning {
            timer_update_attempts: 1,
            ..fast()
        },
    );
    let timer = parse_timer(timer_payload("Song Countdown", "timer-uuid", 0, 0));
    let error = c.set_timer_duration(&timer, 281).await.unwrap_err();
    assert!(
        matches!(&error, PpError::Response(m) if m.contains("still reported Countdown to Time")),
        "{error}"
    );
    assert_eq!(
        error.message(),
        "ProPresenter did not confirm timer \"Song Countdown\" duration as 281 seconds; it still reported Countdown to Time. The timer was not started."
    );
}

#[tokio::test]
async fn client_rejects_missing_timer_and_accepts_other_timer_types() {
    let calls = Arc::new(Mutex::new(0usize));
    let responder: Responder = {
        let calls = calls.clone();
        Arc::new(move |_m, _p, _b, _| {
            let mut c = calls.lock().unwrap();
            *c += 1;
            if *c == 1 {
                json_response(json!([timer_payload("Other Timer", "timer-uuid", 0, 60)]))
            } else {
                json_response(json!([{
                    "id": {"uuid": "elapsed-uuid", "name": "Song Countdown", "index": 0},
                    "allows_overrun": false,
                    "elapsed": {"start_time": 0}
                }]))
            }
        })
    };
    let mock = serve(responder).await;
    let c = client(&mock, fast());
    let error = c.find_timer("Song Countdown").await.unwrap_err();
    assert_eq!(
        error,
        PpError::TimerNotFound("ProPresenter timer \"Song Countdown\" was not found.".into())
    );
    assert!(error.is_propresenter_error());
    let convertible = c.find_timer("Song Countdown").await.unwrap();
    assert!(convertible.countdown.is_none());
}

#[tokio::test]
async fn duplicate_timer_names_are_refused() {
    let mock = serve(Arc::new(|_m, _p, _b, _| {
        json_response(json!([
            timer_payload("Song Countdown", "a", 0, 1),
            timer_payload("  song countdown ", "b", 1, 1)
        ]))
    }))
    .await;
    let error = client(&mock, fast())
        .find_timer("Song Countdown")
        .await
        .unwrap_err();
    assert_eq!(
        error,
        PpError::TimerNotFound("Multiple ProPresenter timers are named \"Song Countdown\".".into())
    );
}

#[tokio::test]
async fn client_lists_and_triggers_audience_looks() {
    let mock = serve(Arc::new(|_m, path, _b, _| match path {
        "/v1/looks" => {
            json_response(json!([{"id": {"uuid": "look-saved", "name": "Worship", "index": 0}}]))
        }
        "/v1/look/current" => json_response(
            json!({"id": {"uuid": "unique-live-look", "name": "Worship", "index": 0}}),
        ),
        _ => no_content(),
    }))
    .await;
    let c = client(&mock, fast());
    let looks = c.list_looks().await.unwrap();
    c.trigger_look("look-saved").await.unwrap();
    let current = c.current_look().await.unwrap();
    assert_eq!(looks.len(), 1);
    assert_eq!(looks[0].id.uuid, "look-saved");
    assert_eq!(current.id.uuid, "unique-live-look");
    assert_eq!(
        mock.pairs(),
        [
            ("GET".to_string(), "/v1/looks".to_string()),
            ("GET".to_string(), "/v1/look/look-saved/trigger".to_string()),
            ("GET".to_string(), "/v1/look/current".to_string()),
        ]
    );
}

#[tokio::test]
async fn empty_look_id_and_negative_duration_are_value_errors_with_no_traffic() {
    let mock = serve(Arc::new(|_m, _p, _b, _| no_content())).await;
    let c = client(&mock, fast());
    assert_eq!(
        c.trigger_look("  ").await.unwrap_err(),
        PpError::Value("Look ID cannot be empty.".into())
    );
    let timer = parse_timer(timer_payload("T", "u", 0, 1));
    assert_eq!(
        c.set_timer_duration(&timer, -1).await.unwrap_err(),
        PpError::Value("Timer duration must not be negative.".into())
    );
    assert!(mock.requests().is_empty());
}

// ---- error handling, timeouts (httpx -> reqwest parity) ----------------------------------------

#[tokio::test]
async fn http_errors_carry_the_status_and_up_to_200_flattened_characters() {
    let body = format!("line one\r\nline two {}", "x".repeat(300));
    let mock = serve(Arc::new(move |_m, _p, _b, _| {
        (StatusCode::NOT_FOUND, body.clone()).into_response()
    }))
    .await;
    let error = client(&mock, fast()).stop_timer("t").await.unwrap_err();
    let PpError::Response(message) = error else {
        panic!("expected a response error");
    };
    assert!(
        message.starts_with("ProPresenter returned HTTP 404: line one  line two "),
        "{message}"
    );
    assert!(message.ends_with("."));
    let detail = message
        .strip_prefix("ProPresenter returned HTTP 404: ")
        .and_then(|m| m.strip_suffix('.'))
        .unwrap();
    assert_eq!(detail.chars().count(), 200);
}

#[tokio::test]
async fn http_error_without_a_body_has_no_detail() {
    let mock = serve(Arc::new(|_m, _p, _b, _| {
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }))
    .await;
    let error = client(&mock, fast()).stop_timer("t").await.unwrap_err();
    assert_eq!(
        error,
        PpError::Response("ProPresenter returned HTTP 500.".into())
    );
}

#[tokio::test]
async fn a_non_json_success_body_is_a_response_error() {
    let mock = serve(Arc::new(|_m, _p, _b, _| {
        (StatusCode::OK, "not json").into_response()
    }))
    .await;
    let error = client(&mock, fast()).list_timers().await.unwrap_err();
    assert_eq!(
        error,
        PpError::Response("ProPresenter returned a non-JSON response.".into())
    );
}

#[tokio::test]
async fn unexpected_list_shapes_and_invalid_objects_are_response_errors() {
    let mock = serve(Arc::new(|_m, path, _b, _| match path {
        "/v1/timers" => json_response(json!({"unexpected": 1})),
        "/v1/looks" => json_response(json!([{"nonsense": true}])),
        _ => json_response(json!({"id": "bare string"})),
    }))
    .await;
    let c = client(&mock, fast());
    assert_eq!(
        c.list_timers().await.unwrap_err(),
        PpError::Response("ProPresenter returned an unexpected timer-list response.".into())
    );
    assert_eq!(
        c.list_looks().await.unwrap_err(),
        PpError::Response("ProPresenter returned an invalid Look object.".into())
    );
    assert_eq!(
        c.current_look().await.unwrap_err(),
        PpError::Response("ProPresenter returned an invalid current Look object.".into())
    );
    assert_eq!(
        c.get_timer("x").await.unwrap_err(),
        PpError::Response("ProPresenter returned an invalid timer object.".into())
    );
}

#[tokio::test]
async fn wrapped_list_payloads_are_accepted_under_timers_items_or_data() {
    for key in ["timers", "items", "data"] {
        let mock = serve(Arc::new(move |_m, _p, _b, _| {
            json_response(json!({ key: [timer_payload("Song Countdown", "u", 0, 1), 7, "x"] }))
        }))
        .await;
        let timers = client(&mock, fast()).list_timers().await.unwrap();
        assert_eq!(timers.len(), 1, "{key}");
    }
}

#[tokio::test]
async fn nothing_listening_is_a_connection_error_naming_the_base_url() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let settings = ProPresenterSettings {
        host: "127.0.0.1".into(),
        port,
        ..ProPresenterSettings::default()
    };
    let error = HttpProPresenterClient::new(&settings)
        .list_timers()
        .await
        .unwrap_err();
    assert_eq!(
        error,
        PpError::Connection(format!(
            "Could not connect to ProPresenter at http://127.0.0.1:{port}."
        ))
    );
    assert!(error.is_connection());
}

#[tokio::test]
async fn a_server_that_never_answers_times_out_with_the_configured_seconds() {
    let mock = serve(Arc::new(|_m, _p, _b, _| no_content())).await;
    // A listener that accepts and says nothing.
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = silent.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = silent.accept().await {
            held.push(socket);
        }
    });
    drop(mock);
    let settings = ProPresenterSettings {
        host: "127.0.0.1".into(),
        port,
        request_timeout_seconds: 0.3,
        ..ProPresenterSettings::default()
    };
    let started = std::time::Instant::now();
    let error = HttpProPresenterClient::new(&settings)
        .list_timers()
        .await
        .unwrap_err();
    assert_eq!(
        error,
        PpError::Connection("ProPresenter did not respond within 0.3 seconds.".into())
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn timer_operations_use_get_on_the_documented_paths() {
    let mock = serve(Arc::new(|_m, _p, _b, _| no_content())).await;
    let c = client(&mock, fast());
    c.stop_timer("u").await.unwrap();
    c.start_timer("u").await.unwrap();
    assert_eq!(
        mock.pairs(),
        [
            ("GET".to_string(), "/v1/timer/u/stop".to_string()),
            ("GET".to_string(), "/v1/timer/u/start".to_string()),
        ]
    );
}

#[tokio::test]
async fn current_timers_reads_the_state_of_each_running_timer() {
    let responder: Responder = Arc::new(|method, path, _, _| match (method, path) {
        ("GET", "/v1/timers/current") => json_response(json!([
            {"id": {"uuid": "timer-uuid", "name": "Song Countdown", "index": 0},
             "time": "00:04:40", "state": "overrunning"},
            {"id": {"uuid": "other", "name": "Message Timer", "index": 1},
             "time": "00:00:00", "state": "stopped"},
        ])),
        _ => StatusCode::NOT_FOUND.into_response(),
    });
    let mock = serve(responder).await;
    let timers = client(&mock, fast()).current_timers().await.unwrap();
    let states: Vec<(String, Option<String>)> = timers
        .into_iter()
        .map(|t| (t.id.uuid, t.state))
        .collect();
    assert_eq!(
        states,
        [
            ("timer-uuid".to_string(), Some("overrunning".to_string())),
            ("other".to_string(), Some("stopped".to_string())),
        ]
    );
}
