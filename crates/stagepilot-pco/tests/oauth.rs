//! Behaviour tests for the OAuth sign-in: port of `test_planning_center_oauth.py`.

mod support;

use serde_json::{json, Value};
use stagepilot_core::config::ConnectionMethod;
use stagepilot_core::settings::{CredentialStore, MemoryCredentialStore};
use stagepilot_pco::oauth::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::*;

const CLIENT_ID: &str = "test-client-id";

fn token_payload(access: &str, refresh: &str, expires_in: i64) -> Value {
    json!({"access_token": access, "refresh_token": refresh, "expires_in": expires_in,
        "scope": "services", "token_type": "Bearer"})
}

struct Rig {
    oauth: Arc<OAuthService>,
    store: OAuthTokenStore,
    mock: Mock,
    clock: Arc<Mutex<f64>>,
}

async fn rig(handler: impl Fn(&Req) -> Resp + Send + Sync + 'static) -> Rig {
    let mock = Mock::start(handler).await;
    let store = OAuthTokenStore::new(Arc::new(MemoryCredentialStore::default()));
    let clock = Arc::new(Mutex::new(1_000_000.0f64));
    let c = clock.clone();
    let oauth = Arc::new(OAuthService::with_clock(
        CLIENT_ID,
        store.clone(),
        ControlPlaneClient::for_test_origin(&mock.origin).unwrap(),
        Arc::new(move || *c.lock().unwrap()),
    ));
    Rig {
        oauth,
        store,
        mock,
        clock,
    }
}

fn flow_then(token: Value) -> impl Fn(&Req) -> Resp + Send + Sync + 'static {
    move |r| {
        if r.path.ends_with("/flow") {
            Resp::json(201, json!({"flow_id": "aabbccdd", "ticket": "ticket-1"}))
        } else {
            Resp::json(200, token.clone())
        }
    }
}

fn tokens(access: &str, refresh: &str, expires_at: f64) -> OAuthTokens {
    OAuthTokens {
        access_token: access.into(),
        refresh_token: refresh.into(),
        expires_at,
        scope: "services".into(),
        needs_reconnect: false,
    }
}

#[test]
fn pkce_verifier_and_s256_challenge_match_rfc7636() {
    let verifier = generate_code_verifier();
    assert!((43..=128).contains(&verifier.len()));
    assert!(verifier
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    assert_ne!(verifier, generate_code_verifier());
    // RFC 7636 appendix B test vector.
    assert_eq!(
        code_challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn state_embeds_the_flow_id_and_is_unguessable() {
    let a = build_state("aabbccdd");
    let b = build_state("aabbccdd");
    assert_ne!(a, b);
    assert_eq!(flow_id_from_state(&a).as_deref(), Some("aabbccdd"));
    assert_eq!(flow_id_from_state("nodot"), None);
    assert_eq!(flow_id_from_state(".tail"), None);
    assert_eq!(flow_id_from_state("head."), None);
}

#[test]
fn authorize_url_requests_only_the_services_scope_and_s256() {
    let url = url::Url::parse(&build_authorize_url("cid", "st ate.x", "chal")).unwrap();
    assert_eq!(
        url.origin().ascii_serialization(),
        "https://api.planningcenteronline.com"
    );
    assert_eq!(url.path(), "/oauth/authorize");
    let query: std::collections::BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["scope"], "services");
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["state"], "st ate.x");
    // `redirect_uri` is added by the desktop side, which knows the free loopback port.
    assert!(!query.contains_key("redirect_uri"));
}

#[tokio::test]
async fn sign_in_exchanges_the_code_and_stores_the_token_blob() {
    let r = rig(flow_then(token_payload("access-1", "refresh-1", 7200))).await;
    let (url, state) = r.oauth.start().await.unwrap();
    assert_eq!(flow_id_from_state(&state).as_deref(), Some("aabbccdd"));
    assert!(url.contains("code_challenge="));
    let tokens = r
        .oauth
        .complete(&state, "the-code", "http://127.0.0.1:52847/callback")
        .await
        .unwrap();
    assert_eq!(tokens.access_token, "access-1");
    let stored = r.store.load().unwrap().unwrap();
    assert_eq!(stored.refresh_token, "refresh-1");
    assert!(!stored.needs_reconnect);
    assert!(stored.expires_in(*r.clock.lock().unwrap()) > 7000.0);
    let exchange = r.mock.requests().last().unwrap().json();
    assert_eq!(exchange["ticket"], "ticket-1");
    assert_eq!(exchange["redirect_uri"], "http://127.0.0.1:52847/callback");
    assert!(exchange["code_verifier"].is_string());
    // The verifier sent is the one whose challenge was put in the authorize URL.
    let challenge = url::Url::parse(&url)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "code_challenge")
        .unwrap()
        .1
        .into_owned();
    assert_eq!(
        code_challenge_for(exchange["code_verifier"].as_str().unwrap()),
        challenge
    );
}

#[tokio::test]
async fn the_stored_blob_is_the_json_python_reads_and_nothing_else_is_written() {
    let credentials = Arc::new(MemoryCredentialStore::default());
    let mock = Mock::start(flow_then(token_payload("access-1", "refresh-1", 7200))).await;
    let store = OAuthTokenStore::new(credentials.clone());
    let oauth = OAuthService::new(
        CLIENT_ID,
        store,
        ControlPlaneClient::for_test_origin(&mock.origin).unwrap(),
    );
    let (_, state) = oauth.start().await.unwrap();
    oauth
        .complete(&state, "c", "http://127.0.0.1:1/cb")
        .await
        .unwrap();
    let blob: Value = serde_json::from_str(&credentials.get_secret().unwrap().unwrap()).unwrap();
    let mut keys: Vec<_> = blob.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "access_token",
            "expires_at",
            "needs_reconnect",
            "refresh_token",
            "scope"
        ]
    );
    // A blob Python wrote (pydantic `model_dump_json`) loads.
    let python = r#"{"access_token":"a","refresh_token":"r","expires_at":1792300000.5,"scope":"services","needs_reconnect":false}"#;
    let store = OAuthTokenStore::new(Arc::new(MemoryCredentialStore::new(Some(python.into()))));
    assert_eq!(store.load().unwrap().unwrap().access_token, "a");
}

#[tokio::test]
async fn a_mismatched_state_is_rejected_before_any_token_exchange() {
    let r = rig(flow_then(token_payload("access-1", "refresh-1", 7200))).await;
    r.oauth.start().await.unwrap();
    let error = r
        .oauth
        .complete("wrong.state", "code", "http://127.0.0.1:1/cb")
        .await
        .unwrap_err();
    let CompleteError::OAuth(error) = error else {
        panic!()
    };
    assert!(error.permanent);
    assert!(error.message.contains("could not be verified"));
    assert!(r.mock.requests().iter().all(|q| q.path.ends_with("/flow")));
    assert!(r.store.load().unwrap().is_none());
}

#[tokio::test]
async fn a_sign_in_expires_and_cannot_be_completed_twice() {
    let r = rig(flow_then(token_payload("access-1", "refresh-1", 7200))).await;
    let (_, state) = r.oauth.start().await.unwrap();
    *r.clock.lock().unwrap() += FLOW_TTL_SECONDS + 1.0;
    let CompleteError::OAuth(error) = r
        .oauth
        .complete(&state, "c", "http://127.0.0.1:1/cb")
        .await
        .unwrap_err()
    else {
        panic!()
    };
    assert!(error.permanent && error.message.contains("expired"));
    // A fresh one completes once and the flow is then consumed.
    let (_, state) = r.oauth.start().await.unwrap();
    r.oauth
        .complete(&state, "c", "http://127.0.0.1:1/cb")
        .await
        .unwrap();
    assert!(r
        .oauth
        .complete(&state, "c", "http://127.0.0.1:1/cb")
        .await
        .is_err());
    // No sign-in started at all.
    let none = rig(flow_then(token_payload("a", "r", 7200))).await;
    assert!(none.oauth.complete("x.y", "c", "u").await.is_err());
}

#[tokio::test]
async fn refresh_overwrites_the_stored_refresh_token_every_time() {
    let n = Arc::new(Mutex::new(0));
    let c = n.clone();
    let r = rig(move |req| {
        if req.path.ends_with("/flow") {
            return Resp::json(201, json!({"flow_id": "aabbccdd", "ticket": "t"}));
        }
        let mut n = c.lock().unwrap();
        *n += 1;
        Resp::json(
            200,
            token_payload(&format!("access-{n}"), &format!("refresh-{n}"), 7200),
        )
    })
    .await;
    r.store
        .save(&tokens("old", "refresh-0", 1_000_060.0))
        .unwrap();
    r.oauth.refresh_now(None).await.unwrap();
    assert_eq!(r.store.load().unwrap().unwrap().refresh_token, "refresh-1");
    r.oauth.refresh_now(None).await.unwrap();
    assert_eq!(r.store.load().unwrap().unwrap().refresh_token, "refresh-2");
    // Each refresh sent the refresh token that was stored at the time.
    let sent: Vec<_> = r
        .mock
        .requests()
        .iter()
        .filter(|q| q.path.ends_with("/refresh"))
        .map(|q| q.json()["refresh_token"].clone())
        .collect();
    assert_eq!(sent, vec![json!("refresh-0"), json!("refresh-1")]);
}

#[tokio::test]
async fn a_revoked_refresh_token_sets_the_explicit_needs_reconnect_state() {
    let r = rig(|req| {
        if req.path.ends_with("/flow") {
            Resp::json(201, json!({"flow_id": "aabbccdd", "ticket": "t"}))
        } else {
            Resp::json(400, json!({"error": "invalid_grant"}))
        }
    })
    .await;
    r.store.save(&tokens("a", "r", 1_000_060.0)).unwrap();
    let CompleteError::OAuth(error) = r.oauth.refresh_now(None).await.unwrap_err() else {
        panic!()
    };
    assert!(error.permanent);
    let status = r.oauth.status(ConnectionMethod::Oauth);
    assert!(status.needs_reconnect && !status.connected);
    // The dead connection refuses to hand out a token without calling the control plane again.
    let before = r.mock.requests().len();
    let CompleteError::OAuth(error) = r.oauth.valid_access_token().await.unwrap_err() else {
        panic!()
    };
    assert!(error.permanent);
    assert_eq!(r.mock.requests().len(), before);
}

#[tokio::test]
async fn permanent_codes_are_exactly_invalid_grant_unauthorized_client_and_invalid_client() {
    for (code, permanent) in [
        ("invalid_grant", true),
        ("unauthorized_client", true),
        ("invalid_client", true),
        ("server_error", false),
        ("slow_down", false),
    ] {
        let r = rig(move |req| {
            if req.path.ends_with("/flow") {
                Resp::json(201, json!({"flow_id": "f", "ticket": "t"}))
            } else {
                Resp::json(400, json!({"error": code}))
            }
        })
        .await;
        r.store.save(&tokens("a", "r", 1_000_060.0)).unwrap();
        let CompleteError::OAuth(error) = r.oauth.refresh_now(None).await.unwrap_err() else {
            panic!()
        };
        assert_eq!(error.permanent, permanent, "{code}");
        assert_eq!(
            r.store.load().unwrap().unwrap().needs_reconnect,
            permanent,
            "{code}"
        );
    }
}

#[tokio::test]
async fn a_transient_failure_never_marks_the_connection_as_needing_reconnect() {
    for status in [429u16, 500, 502, 503] {
        let r = rig(move |req| {
            if req.path.ends_with("/flow") {
                Resp::json(201, json!({"flow_id": "f", "ticket": "t"}))
            } else {
                Resp::json(status, json!({"error": "overloaded"}))
            }
        })
        .await;
        r.store.save(&tokens("a", "r", 1_000_060.0)).unwrap();
        let CompleteError::OAuth(error) = r.oauth.refresh_now(None).await.unwrap_err() else {
            panic!()
        };
        assert!(!error.permanent, "{status}");
        if status == 429 {
            assert!(error.message.contains("busy"));
        }
        let stored = r.store.load().unwrap().unwrap();
        assert!(!stored.needs_reconnect);
        assert_eq!(
            stored.refresh_token, "r",
            "a failed refresh leaves the tokens alone"
        );
    }
}

#[tokio::test]
async fn network_failure_is_transient() {
    let store = OAuthTokenStore::new(Arc::new(MemoryCredentialStore::default()));
    store.save(&tokens("a", "r", 1.0)).unwrap();
    let oauth = OAuthService::new(
        CLIENT_ID,
        store.clone(),
        ControlPlaneClient::for_test_origin("http://127.0.0.1:9").unwrap(),
    );
    let CompleteError::OAuth(error) = oauth.refresh_now(None).await.unwrap_err() else {
        panic!()
    };
    assert!(!error.permanent);
    assert!(error.message.contains("Could not reach"));
    assert!(!store.load().unwrap().unwrap().needs_reconnect);
}

#[tokio::test]
async fn invalid_control_plane_replies_are_transient_and_never_store_anything() {
    for body in [
        json!([1]),
        json!({"flow_id": "", "ticket": "t"}),
        json!({"flow_id": "f"}),
    ] {
        let r = rig(move |_| Resp::json(201, body.clone())).await;
        let error = r.oauth.start().await.unwrap_err();
        assert!(!error.permanent && error.message.contains("invalid response"));
    }
    for token in [
        json!({"access_token": "a"}),
        json!({"access_token": "a", "refresh_token": "r", "expires_in": "soon"}),
        json!({"access_token": "", "refresh_token": "r", "expires_in": 5}),
    ] {
        let r = rig(flow_then(token)).await;
        let (_, state) = r.oauth.start().await.unwrap();
        let CompleteError::OAuth(error) = r.oauth.complete(&state, "c", "u").await.unwrap_err()
        else {
            panic!()
        };
        assert!(!error.permanent);
        assert!(r.store.load().unwrap().is_none());
    }
    let r = rig(|_| Resp::raw(200, "not json")).await;
    assert!(r.oauth.start().await.is_err());
}

#[test]
fn refresh_is_due_ten_to_fifteen_minutes_before_the_two_hour_expiry() {
    let now = 1_000_000.0;
    assert!(!tokens("a", "r", now + 7200.0).needs_refresh(now));
    assert!(!tokens("a", "r", now + 901.0).needs_refresh(now));
    assert!(tokens("a", "r", now + 900.0).needs_refresh(now));
    assert!(tokens("a", "r", now - 5.0).needs_refresh(now));
}

#[tokio::test]
async fn valid_access_token_refreshes_when_due_and_refuses_a_dead_connection() {
    let r = rig(flow_then(token_payload(
        "fresh-access",
        "fresh-refresh",
        7200,
    )))
    .await;
    r.store.save(&tokens("stale", "r", 1_000_060.0)).unwrap();
    assert_eq!(r.oauth.valid_access_token().await.unwrap(), "fresh-access");
    // Not due: handed out without any call.
    let before = r.mock.requests().len();
    assert_eq!(r.oauth.valid_access_token().await.unwrap(), "fresh-access");
    assert_eq!(r.mock.requests().len(), before);
    r.store
        .save(&OAuthTokens {
            needs_reconnect: true,
            ..tokens("a", "r", 1_007_200.0)
        })
        .unwrap();
    let CompleteError::OAuth(error) = r.oauth.valid_access_token().await.unwrap_err() else {
        panic!()
    };
    assert!(error.permanent);
    // Disconnected altogether.
    r.store.clear().unwrap();
    let CompleteError::OAuth(error) = r.oauth.valid_access_token().await.unwrap_err() else {
        panic!()
    };
    assert!(error.permanent && error.message.contains("disconnected"));
}

#[tokio::test]
async fn proactive_loop_refreshes_a_due_token_then_stops_cleanly() {
    let r = rig(flow_then(token_payload(
        "looped-access",
        "looped-refresh",
        7200,
    )))
    .await;
    r.store.save(&tokens("a", "r", 1_000_060.0)).unwrap();
    let (stop, rx) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(proactive_refresh_loop(
        r.oauth.clone(),
        rx,
        Duration::from_millis(10),
    ));
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if r.store
            .load()
            .unwrap()
            .is_some_and(|t| t.access_token == "looped-access")
        {
            break;
        }
    }
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        r.store.load().unwrap().unwrap().access_token,
        "looped-access"
    );
}

#[tokio::test]
async fn proactive_loop_survives_a_permanent_failure_without_raising() {
    let r = rig(|req| {
        if req.path.ends_with("/flow") {
            Resp::json(201, json!({"flow_id": "f", "ticket": "t"}))
        } else {
            Resp::json(400, json!({"error": "invalid_grant"}))
        }
    })
    .await;
    r.store.save(&tokens("a", "r", 1_000_060.0)).unwrap();
    let (stop, rx) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(proactive_refresh_loop(
        r.oauth.clone(),
        rx,
        Duration::from_millis(10),
    ));
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if r.store.load().unwrap().is_some_and(|t| t.needs_reconnect) {
            break;
        }
    }
    // Once flagged, the loop stops trying: no further refresh requests arrive.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let refreshes = r
        .mock
        .requests()
        .iter()
        .filter(|q| q.path.ends_with("/refresh"))
        .count();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        r.mock
            .requests()
            .iter()
            .filter(|q| q.path.ends_with("/refresh"))
            .count(),
        refreshes
    );
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert!(r.store.load().unwrap().unwrap().needs_reconnect);
}

#[test]
fn an_untrusted_control_plane_origin_is_refused() {
    assert!(ControlPlaneClient::with_origin("https://example.com").is_err());
    assert!(ControlPlaneClient::with_origin("http://127.0.0.1:1").is_err());
    assert!(ControlPlaneClient::with_origin(DEFAULT_CONTROL_PLANE_ORIGIN).is_ok());
}

#[test]
fn a_corrupt_token_blob_reads_as_not_connected() {
    for blob in [
        "not json at all",
        "{}",
        r#"{"access_token":"","refresh_token":"r","expires_at":1}"#,
        "[1,2]",
    ] {
        let store = OAuthTokenStore::new(Arc::new(MemoryCredentialStore::new(Some(blob.into()))));
        assert!(store.load().unwrap().is_none(), "{blob}");
    }
}

#[tokio::test]
async fn disconnect_clears_the_stored_tokens() {
    let r = rig(|_| Resp::json(201, json!({"flow_id": "f", "ticket": "t"}))).await;
    r.store.save(&tokens("a", "r", 1_000_060.0)).unwrap();
    r.oauth.disconnect().unwrap();
    assert!(r.store.load().unwrap().is_none());
    assert!(!r.oauth.status(ConnectionMethod::Manual).connected);
}

#[tokio::test]
async fn sign_in_is_unavailable_without_a_client_id() {
    let mock = Mock::start(|_| Resp::json(201, json!({}))).await;
    let oauth = OAuthService::new(
        "",
        OAuthTokenStore::new(Arc::new(MemoryCredentialStore::default())),
        ControlPlaneClient::for_test_origin(&mock.origin).unwrap(),
    );
    assert!(!oauth.configured());
    let error = oauth.start().await.unwrap_err();
    assert!(error.message.contains("cannot sign in"));
    assert!(mock.requests().is_empty());
}

#[test]
fn tokens_never_appear_in_debug_output() {
    let t = tokens("secret-access", "secret-refresh", 1.0);
    let text = format!("{t:?}");
    assert!(
        !text.contains("secret-access") && !text.contains("secret-refresh"),
        "{text}"
    );
    let flow = PendingFlow {
        state: "s.s".into(),
        code_verifier: "verifier-secret".into(),
        flow_id: "f".into(),
        ticket: "ticket-secret".into(),
        started_at: 0.0,
    };
    let text = format!("{flow:?}");
    assert!(
        !text.contains("verifier-secret")
            && !text.contains("ticket-secret")
            && !text.contains("s.s"),
        "{text}"
    );
}

#[test]
fn quote_plus_matches_python() {
    assert_eq!(
        quote_plus("a b/c?d=e&f~g_h.i-j"),
        "a+b%2Fc%3Fd%3De%26f~g_h.i-j"
    );
    assert_eq!(quote_plus("é"), "%C3%A9");
}
