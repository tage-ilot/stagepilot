//! The song order from the Library Link, end to end against fakes only: a fake helper (an axum server on
//! a loopback port) and the fake Playback API (`stagepilot_playback::testkit`). No rig, no real helper.
//!
//! What is proved: the five conditions each failing on their own fall back to today's behaviour and say
//! why in one plain line; everything holding saves the order once (a second look, and a new library
//! revision that changes nothing, save nothing); a person's scan wins until the plan changes; the
//! library order and a real scan of the same setlist are the same song ids; a plan in another order
//! than Playback's saves Playback's order.

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::settings::{RuntimeSettings, SettingsService};
use stagepilot_playback::plugin::PluginOptions;
use stagepilot_playback::testkit::FakePlayback;
use stagepilot_server::playback_library::note_in_use;
use stagepilot_server::playback_order::refresh;
use stagepilot_server::{build_app, AppOptions, Runtime};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;

const STEP: Duration = Duration::from_millis(10);
const LIMIT: usize = 1500; // 15 s

// ---- the fake helper ----------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Helper(Arc<Mutex<Behaviour>>);

#[derive(Default)]
struct Behaviour {
    hits: Vec<String>,
    revision: String,
    generation: u32,
    /// The setlist Playback has open (`/setlists/current`); `None` is a 404.
    current: Option<Value>,
    /// Answer everything with this status instead.
    broken: Option<u16>,
}

fn item(id: i64, order: usize, title: &str, scan_length: f64) -> Value {
    json!({
        "setlist_song_id": id, "order": order, "title": title, "artist": "Artist",
        "key": {"id": 3, "name": "Ab"}, "key_original": {"id": 1, "name": "C"},
        "bpm": 65.0, "bpm_original": 65.0, "length_s": scan_length + 3.0, "scan_length_s": scan_length,
        "arrangement": "Default", "planning_center_song_id": null, "rental_expires_utc": null
    })
}

/// A setlist the way the helper lists it, in Playback's order: (song number, title).
fn setlist(songs: &[(i64, &str)]) -> Value {
    let items: Vec<Value> = songs
        .iter()
        .enumerate()
        .map(|(i, (id, title))| item(*id, i + 1, title, 280.0 + i as f64 * 10.0))
        .collect();
    json!({"setlist_id": 4, "title": "Sunday", "performance_date": "2026-11-22", "items": items})
}

impl Helper {
    fn new(current: Value) -> Self {
        let helper = Self::default();
        {
            let mut b = helper.0.lock().unwrap();
            b.revision = "rev-1".into();
            b.current = Some(current);
        }
        helper
    }

    fn set(&self, f: impl FnOnce(&mut Behaviour)) {
        f(&mut self.0.lock().unwrap());
    }

    /// Playback opens another setlist: the library changed, and its revision says so.
    fn open(&self, current: Value) {
        let mut b = self.0.lock().unwrap();
        b.current = Some(current);
        b.generation += 1;
        b.revision = format!("rev-{}", b.generation);
    }

    fn hits(&self) -> Vec<String> {
        self.0.lock().unwrap().hits.clone()
    }

    async fn serve(&self) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new().fallback(answer).with_state(self.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("127.0.0.1:{port}")
    }
}

async fn answer(State(helper): State<Helper>, uri: Uri) -> Response {
    let mut b = helper.0.lock().unwrap();
    b.hits.push(uri.path().to_string());
    if let Some(status) = b.broken {
        return (StatusCode::from_u16(status).unwrap(), "{\"error\": \"no\"}").into_response();
    }
    let json_ok = |v: Value| {
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            v.to_string(),
        )
            .into_response()
    };
    match uri.path() {
        "/health" => json_ok(json!({
            "ok": true, "version": "0.1.1", "database": "ok", "detail": "", "songs": 12,
            "setlists": 3, "library_revision": b.revision
        })),
        "/setlists/current" => match &b.current {
            Some(setlist) => json_ok(setlist.clone()),
            None => (StatusCode::NOT_FOUND, "{\"error\": \"none open\"}").into_response(),
        },
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

// ---- the rig --------------------------------------------------------------------------------------

struct Rig {
    router: Router,
    runtime: Arc<Runtime>,
    fake: FakePlayback,
    helper: Helper,
}

/// Playback's setlist in the demo plan's titles: the demo plan is Battle Belongs, Holy Forever, Gratitude,
/// Firm Foundation. `songs` are the helper's setlist; the fake Playback holds the same numbers.
async fn rig(songs: &[(i64, &str)]) -> Rig {
    let service = SettingsService::ephemeral(RuntimeSettings::demo());
    let runtime =
        Runtime::with_playback_options(Arc::new(service), false, PluginOptions::default());
    runtime.start().await;
    let router = build_app(runtime.clone(), AppOptions { web_root: None });
    let fake = FakePlayback::start().await;
    fake.auto_beat(Duration::from_millis(100));
    {
        let mut s = fake.state.lock().unwrap();
        s.songs = songs.iter().map(|(id, _)| *id).collect();
        s.song = songs[0].0;
    }
    let helper = Helper::new(setlist(songs));
    let address = helper.serve().await;
    let r = Rig {
        router,
        runtime,
        fake,
        helper,
    };
    let (code, _) = r
        .call(
            "PUT",
            "/api/v1/playback-api/settings",
            Some(
                json!({"enabled": true, "host": "127.0.0.1", "port": r.fake.port,
                "auto_scan": false, "fast_transport": true}),
            ),
        )
        .await;
    assert_eq!(code, StatusCode::OK);
    r.until("connected", |s| s["connected"] == true).await;
    for _ in 0..LIMIT {
        if r.runtime.playback.library_evidence().1.is_some() {
            break;
        }
        tokio::time::sleep(STEP).await;
    }
    let (code, _) = r
        .call(
            "PUT",
            "/api/v1/playback-library/settings",
            Some(json!({ "address": address })),
        )
        .await;
    assert_eq!(code, StatusCode::OK);
    r
}

impl Rig {
    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let builder = Request::builder().method(method).uri(path);
        let mut request = match body {
            Some(b) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        request.extensions_mut().insert(ConnectInfo(
            "127.0.0.1:50000".parse::<SocketAddr>().unwrap(),
        ));
        let response = self.router.clone().oneshot(request).await.unwrap();
        let (parts, body) = response.into_parts();
        let bytes = body.collect().await.unwrap().to_bytes();
        (
            parts.status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn status(&self) -> Value {
        self.call("GET", "/api/v1/playback-api/status", None)
            .await
            .1
    }

    async fn until(&self, what: &str, mut ready: impl FnMut(&Value) -> bool) -> Value {
        for _ in 0..LIMIT {
            let status = self.status().await;
            if ready(&status) {
                return status;
            }
            tokio::time::sleep(STEP).await;
        }
        panic!("timed out waiting for: {what}");
    }

    /// The Library Link has been seen working, which the Library Link block of the window does the first
    /// time it opens (`note_in_use`).
    async fn library_in_use(&self) {
        let (code, status) = self
            .call("GET", "/api/v1/playback-library/status?check=1", None)
            .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(status["active"], true, "{status}");
        note_in_use(&self.runtime, true).await;
    }

    /// One look of the watcher, as much later as the helper's own throttle needs (30 s).
    async fn look(&self) -> bool {
        self.runtime
            .playback_library
            .advance_clock(Duration::from_secs(31));
        refresh(&self.runtime).await
    }

    fn set_plan(&self, id: &str, titles: &[&str]) {
        self.runtime.store.mutate(|state| {
            let plan = state.plan.as_mut().expect("the demo plan");
            plan.id = id.into();
            plan.songs = titles
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    stagepilot_core::state::Song::new(
                        &format!("{id}-{i}"),
                        t,
                        Some(300),
                        i as u32 + 1,
                    )
                })
                .collect();
        });
    }

    async fn scan(&self) -> Value {
        let (code, status) = self
            .call(
                "POST",
                "/api/v1/playback-api/discover-song-order",
                Some(json!({"confirm": true})),
            )
            .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(status["discovery"], "done", "{status}");
        status
    }
}

const DEMO: [&str; 4] = [
    "Battle Belongs",
    "Holy Forever",
    "Gratitude",
    "Firm Foundation",
];

const IN_ORDER: [(i64, &str); 4] = [
    (101, "Battle Belongs"),
    (102, "Holy Forever"),
    (103, "Gratitude"),
    (104, "Firm Foundation"),
];

fn note(status: &Value) -> String {
    status["order_library_note"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

// ---- tests ----------------------------------------------------------------------------------------

#[tokio::test]
async fn with_no_library_link_in_use_nothing_changes_and_nothing_is_asked() {
    let r = rig(&IN_ORDER).await;
    // The Library Link has never been seen working: the watcher does not go and look.
    let asked = r.helper.hits().len();
    assert!(!r.look().await);
    assert_eq!(r.helper.hits().len(), asked, "{:?}", r.helper.hits());
    let status = r.status().await;
    assert_eq!(status["order_source"], "none");
    assert_eq!(status["order_mode"], "none");
    assert_eq!(status["song_order"], json!([]));
    assert_eq!(status["order_library_note"], Value::Null);
    r.runtime.stop().await;
}

#[tokio::test]
async fn a_plan_song_that_is_missing_or_uncertain_or_twice_or_a_repeated_number_each_fall_back_with_the_reason(
) {
    let r = rig(&IN_ORDER).await;
    r.library_in_use().await;

    // 4. A plan song Playback does not have.
    r.set_plan(
        "plan-a",
        &[
            "Battle Belongs",
            "Faithfulness",
            "Gratitude",
            "Firm Foundation",
        ],
    );
    assert!(!r.look().await);
    let status = r.status().await;
    assert_eq!(
        note(&status),
        "Not using the Library Link's order: Faithfulness is not in Playback's setlist."
    );
    assert_eq!(status["order_source"], "none");
    assert_eq!(status["song_order"], json!([]));

    // 4. A plan song that is only alike a Playback song.
    r.set_plan(
        "plan-a",
        &["Battle Belongs", "Holy", "Gratitude", "Firm Foundation"],
    );
    r.helper.open(setlist(&[
        (101, "Battle Belongs"),
        (102, "Holy Forever Reprise Live"),
        (103, "Gratitude"),
        (104, "Firm Foundation"),
    ]));
    assert!(!r.look().await);
    let line = note(&r.status().await);
    assert!(
        line.starts_with("Not using the Library Link's order: Holy may be")
            && line.contains("only alike"),
        "{line}"
    );

    // 4. A plan song twice in the plan.
    r.set_plan(
        "plan-a",
        &[
            "Battle Belongs",
            "Gratitude",
            "Gratitude",
            "Firm Foundation",
        ],
    );
    r.helper.open(setlist(&IN_ORDER));
    assert!(!r.look().await);
    assert!(note(&r.status().await).contains("Gratitude is in the plan more than once"));

    // 3. The song Playback has selected is not in the setlist the Library Link says is open.
    r.set_plan("plan-a", &DEMO);
    r.fake.state.lock().unwrap().song = 999;
    for _ in 0..LIMIT {
        if r.runtime
            .playback
            .library_evidence()
            .1
            .is_some_and(|h| h.song_id == 999)
        {
            break;
        }
        tokio::time::sleep(STEP).await;
    }
    assert!(!r.look().await);
    assert!(
        note(&r.status().await).contains("is not in the setlist the Library Link says is open"),
        "{}",
        note(&r.status().await)
    );
    r.fake.state.lock().unwrap().song = 101;

    // 5. One number for two songs.
    r.helper.open(setlist(&[
        (101, "Battle Belongs"),
        (102, "Holy Forever"),
        (102, "Gratitude"),
        (104, "Firm Foundation"),
    ]));
    for _ in 0..LIMIT {
        if r.runtime
            .playback
            .library_evidence()
            .1
            .is_some_and(|h| h.song_id == 101)
        {
            break;
        }
        tokio::time::sleep(STEP).await;
    }
    assert!(!r.look().await);
    assert!(
        note(&r.status().await).contains("Gratitude is in Playback's setlist more than once"),
        "{}",
        note(&r.status().await)
    );

    // Not one of the five held: nothing was saved, and the verdict of the header is not part of this.
    let status = r.status().await;
    assert_eq!(status["order_source"], "none");
    assert_eq!(status["song_order"], json!([]));
    r.runtime.stop().await;
}

#[tokio::test]
async fn an_answer_that_is_only_the_last_good_copy_is_not_used() {
    let r = rig(&IN_ORDER).await;
    r.library_in_use().await;
    // Read once while another condition fails, so there is a last good copy.
    r.set_plan("plan-a", &["Battle Belongs", "Faithfulness"]);
    assert!(!r.look().await);
    assert!(note(&r.status().await).contains("Faithfulness is not in Playback's setlist"));
    // The helper goes silent; the copy stands in for it for a minute, and says so.
    r.helper.set(|b| b.broken = Some(500));
    r.set_plan("plan-a", &DEMO);
    assert!(!r.look().await);
    let status = r.status().await;
    assert_eq!(
        note(&status),
        "Not using the Library Link's order: The Library Link did not answer just now."
    );
    assert_eq!(status["order_source"], "none");
    // Nothing else in the status says a thing is wrong because of it.
    assert_eq!(status["connected"], true);
    r.runtime.stop().await;
}

#[tokio::test]
async fn playback_not_connected_falls_back_with_the_reason() {
    let r = rig(&IN_ORDER).await;
    r.library_in_use().await;
    let Rig {
        router,
        runtime,
        fake,
        helper,
    } = r;
    let (_port, _state) = fake.go_offline().await;
    for _ in 0..LIMIT {
        if !runtime.playback.library_evidence().0 {
            break;
        }
        tokio::time::sleep(STEP).await;
    }
    runtime
        .playback_library
        .advance_clock(Duration::from_secs(31));
    assert!(!refresh(&runtime).await);
    assert_eq!(
        runtime.playback.library_note().as_deref(),
        Some("Not using the Library Link's order: Playback is not connected.")
    );
    drop((router, helper));
    runtime.stop().await;
}

#[tokio::test]
async fn everything_holding_saves_the_order_once_with_the_library_lengths_and_a_scans_fields() {
    let r = rig(&IN_ORDER).await;
    r.library_in_use().await;
    assert!(r.look().await, "saved the first time every condition holds");
    let first = r.status().await;
    assert_eq!(first["order_source"], "library");
    assert_eq!(first["order_mode"], "scanned");
    assert_eq!(first["song_order"], json!([101, 102, 103, 104]));
    assert_eq!(first["song_count"], 4);
    assert_eq!(first["order_library_note"], Value::Null);
    // The helper's `scan_length_s` of each song, the countdown length of the existing rule.
    assert_eq!(first["song_lengths"], json!([280.0, 290.0, 300.0, 310.0]));
    assert!(first["captured_at"].as_str().unwrap().ends_with('Z'));
    assert!(first["captured_version"].is_i64());
    r.until("the order is valid", |s| s["stale"] == false).await;

    // A second look, and a library revision that changes nothing, save nothing.
    assert!(!r.look().await);
    r.helper.set(|b| b.revision = "rev-2".into());
    assert!(!r.look().await);
    let again = r.status().await;
    assert_eq!(again["captured_at"], first["captured_at"]);

    // A revision that changes a length saves again, still from the library.
    let mut list = setlist(&IN_ORDER);
    list["items"][3]["scan_length_s"] = json!(308.0);
    r.helper.open(list);
    assert!(r.look().await);
    let status = r.status().await;
    assert_eq!(status["song_lengths"], json!([280.0, 290.0, 300.0, 308.0]));
    assert_eq!(status["order_source"], "library");

    // The API's setlist version changes: saved again with the new version.
    {
        let mut s = r.fake.state.lock().unwrap();
        s.version += 1;
    }
    let version = status["captured_version"].as_i64().unwrap();
    r.until("the new version is seen", |s| s["stale"] == true)
        .await;
    for _ in 0..LIMIT {
        if r.runtime
            .playback
            .library_evidence()
            .1
            .is_some_and(|h| h.setlist_version == Some(version + 1))
        {
            break;
        }
        tokio::time::sleep(STEP).await;
    }
    assert!(r.look().await);
    let status = r.status().await;
    assert_eq!(status["captured_version"], version + 1);
    r.until("valid again", |s| s["stale"] == false).await;
    // Only the helper's own health and open setlist were ever asked, never the setlist the window matches.
    assert!(r
        .helper
        .hits()
        .iter()
        .all(|h| h == "/health" || h == "/setlists/current"));
    r.runtime.stop().await;
}

#[tokio::test]
async fn a_scan_after_a_library_order_wins_until_the_plan_changes_and_gives_the_same_ids() {
    let r = rig(&IN_ORDER).await;
    r.library_in_use().await;
    assert!(r.look().await);
    let from_library = r.status().await;
    assert_eq!(from_library["order_source"], "library");

    // A person scans: the order source is the scan, and the library does not replace it.
    let scanned = r.scan().await;
    assert_eq!(scanned["order_source"], "scan");
    assert_eq!(scanned["song_order"], from_library["song_order"]);
    assert_eq!(
        scanned["captured_version"],
        from_library["captured_version"]
    );
    r.until("valid", |s| s["stale"] == false).await;
    assert!(!r.look().await);
    assert!(!r.look().await);
    assert_eq!(r.status().await["order_source"], "scan");

    // The plan changes: the library order returns.
    r.set_plan("plan-b", &DEMO);
    assert!(r.look().await);
    let back = r.status().await;
    assert_eq!(back["order_source"], "library");
    assert_eq!(back["song_order"], scanned["song_order"]);
    assert_eq!(back["plan_changed"], false);
    r.runtime.stop().await;
}

#[tokio::test]
async fn a_song_only_playback_has_at_the_end_stays_in_the_order_like_in_a_scan() {
    let songs = [
        (101, "Battle Belongs"),
        (102, "Holy Forever"),
        (103, "Gratitude"),
        (104, "Firm Foundation"),
        (105, "Walk-in Music"),
    ];
    let r = rig(&songs).await;
    r.library_in_use().await;
    assert!(r.look().await);
    let status = r.status().await;
    assert_eq!(status["order_source"], "library");
    assert_eq!(status["song_order"], json!([101, 102, 103, 104, 105]));
    assert_eq!(status["order_library_note"], Value::Null);
    // The same ids a real scan of the same setlist finds.
    let scanned = r.scan().await;
    assert_eq!(scanned["song_order"], status["song_order"]);
    r.runtime.stop().await;
}

#[tokio::test]
async fn a_plan_in_another_order_than_playbacks_is_not_used_because_the_countdown_pairs_by_place() {
    // Plan #1 (Battle Belongs) is Playback #2. Every plan song pairs by title, but the countdown starts the
    // Nth plan song with the Nth Playback song, so the library's order would start the wrong timer. Nothing
    // is saved; the learned order and the scan button stay what they are.
    let songs = [
        (101, "Holy Forever"),
        (102, "Battle Belongs"),
        (103, "Gratitude"),
        (104, "Firm Foundation"),
    ];
    let r = rig(&songs).await;
    r.library_in_use().await;
    assert!(!r.look().await);
    let status = r.status().await;
    assert_eq!(
        note(&status),
        "Not using the Library Link's order: Battle Belongs is not in the same place in the plan as in Playback's setlist."
    );
    assert_eq!(status["order_source"], "none");
    assert_eq!(status["song_order"], json!([]));
    // A person who scans gets Playback's order as always.
    let scanned = r.scan().await;
    assert_eq!(scanned["song_order"], json!([101, 102, 103, 104]));
    assert_eq!(scanned["order_source"], "scan");
    r.runtime.stop().await;
}
