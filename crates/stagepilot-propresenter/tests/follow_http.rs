//! The follow calls of `HttpApi` against a small stand-in HTTP server: the exact paths ProPresenter's API takes, what a 204 and a 404
//! mean, and what is read from the three "where is it" answers (shapes as the swagger of ProPresenter 19 gives them).

use stagepilot_propresenter::link::api::{
    ActivePlaylist, ApiError, HttpApi, ProPresenterApi, SlidePosition,
};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// Serves `routes` (path to status and body) on a free port, on a thread; every request line is recorded.
fn serve(routes: Vec<(&'static str, u16, &'static str)>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let table: HashMap<&'static str, (u16, &'static str)> =
        routes.into_iter().map(|(p, s, b)| (p, (s, b))).collect();
    let log = seen.clone();
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            let Ok(mut socket) = socket else { break };
            let mut buffer = vec![0u8; 4096];
            let n = socket.read(&mut buffer).unwrap_or(0);
            let text = String::from_utf8_lossy(&buffer[..n]).to_string();
            let line = text.lines().next().unwrap_or_default().to_string();
            log.lock().unwrap().push(line.clone());
            let path = line.split(' ').nth(1).unwrap_or_default().to_string();
            let (status, body) = table.get(path.as_str()).copied().unwrap_or((404, ""));
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes());
        }
    });
    (base, seen)
}

#[tokio::test]
async fn a_playlist_item_and_a_slide_are_triggered_by_get_on_the_documented_paths() {
    let (base, seen) = serve(vec![
        ("/v1/playlist/PL-1/3/trigger", 204, ""),
        ("/v1/presentation/active/17/trigger", 204, ""),
    ]);
    let api = HttpApi::at(&base);
    api.trigger_playlist_item("PL-1", 3).await.unwrap();
    api.trigger_slide(17).await.unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            "GET /v1/playlist/PL-1/3/trigger HTTP/1.1".to_string(),
            "GET /v1/presentation/active/17/trigger HTTP/1.1".to_string()
        ]
    );
    // A number the arrangement does not have is a 404; any other failure is its status.
    assert_eq!(api.trigger_slide(99).await, Err(ApiError::NotFound));
    assert_eq!(
        api.trigger_playlist_item("PL-2", 0).await,
        Err(ApiError::NotFound)
    );
    let (base, _) = serve(vec![("/v1/presentation/active/1/trigger", 500, "")]);
    assert_eq!(
        HttpApi::at(&base).trigger_slide(1).await,
        Err(ApiError::Status(500))
    );
    // Nothing that is not a plain id goes into a path, and nothing is asked for it.
    let before = seen.lock().unwrap().len();
    assert_eq!(
        api.trigger_playlist_item("../x", 0).await,
        Err(ApiError::BadId)
    );
    assert_eq!(seen.lock().unwrap().len(), before);
}

#[tokio::test]
async fn where_it_is_is_read_from_the_three_answers() {
    let (base, _) = serve(vec![
        (
            "/v1/playlist/active",
            200,
            r#"{"presentation":{"playlist":{"uuid":"PL","name":"Sunday","index":1},"item":{"uuid":"IT","name":"Song","index":4}},"announcements":null}"#,
        ),
        (
            "/v1/presentation/active",
            200,
            r#"{"presentation":{"id":{"uuid":"PU","name":"Song","index":0},"groups":[],"has_timeline":false}}"#,
        ),
        (
            "/v1/presentation/slide_index",
            200,
            r#"{"presentation_index":{"index":13,"presentation_id":{"uuid":"PU","name":"Song","index":0}}}"#,
        ),
    ]);
    let api = HttpApi::at(&base);
    assert_eq!(
        api.active_playlist().await.unwrap(),
        Some(ActivePlaylist {
            playlist_uuid: "PL".into(),
            playlist_name: "Sunday".into(),
            item_uuid: "IT".into(),
            item_index: 4
        })
    );
    assert_eq!(
        api.active_presentation().await.unwrap().as_deref(),
        Some("PU")
    );
    assert_eq!(
        api.slide_position().await.unwrap(),
        Some(SlidePosition {
            presentation_uuid: "PU".into(),
            index: 13
        })
    );
}

#[tokio::test]
async fn nothing_active_is_none_not_an_error_and_a_broken_answer_is_unreadable() {
    let (base, _) = serve(vec![
        (
            "/v1/playlist/active",
            200,
            r#"{"presentation":null,"announcements":null}"#,
        ),
        (
            "/v1/presentation/slide_index",
            200,
            r#"{"presentation_index":null}"#,
        ),
        ("/v1/presentation/active", 200, "not json"),
    ]);
    let api = HttpApi::at(&base);
    assert_eq!(api.active_playlist().await.unwrap(), None);
    assert_eq!(api.slide_position().await.unwrap(), None);
    assert_eq!(api.active_presentation().await, Err(ApiError::Unreadable));
    // ProPresenter answers 404 when nothing is active on some versions: the same as none.
    let (base, _) = serve(vec![]);
    let api = HttpApi::at(&base);
    assert_eq!(api.active_presentation().await.unwrap(), None);
    assert_eq!(api.active_playlist().await.unwrap(), None);
    assert_eq!(api.slide_position().await.unwrap(), None);
}

#[tokio::test]
async fn an_unreachable_propresenter_is_no_answer() {
    let api = HttpApi::at("http://127.0.0.1:1");
    assert!(matches!(
        api.trigger_slide(0).await,
        Err(ApiError::NoAnswer(_))
    ));
    assert!(matches!(
        api.slide_position().await,
        Err(ApiError::NoAnswer(_))
    ));
}

#[tokio::test]
async fn a_presentation_that_answers_404_is_not_opened_and_another_error_is_not_a_no() {
    let (base, seen) = serve(vec![
        ("/v1/presentation/AAAA-1", 200, r#"{"presentation":{}}"#),
        ("/v1/presentation/BBBB-2", 500, ""),
    ]);
    let api = HttpApi::at(&base);
    assert_eq!(api.presentation_opened("AAAA-1").await, Ok(true));
    assert_eq!(api.presentation_opened("CCCC-3").await, Ok(false));
    assert_eq!(
        api.presentation_opened("BBBB-2").await,
        Err(ApiError::Status(500))
    );
    assert_eq!(api.presentation_opened("../x").await, Err(ApiError::BadId));
    assert_eq!(seen.lock().unwrap().len(), 3, "a bad id is never sent");
}
