//! t4: no `/api/v1` route may answer with a hard-coded 503 "not ported yet" stub.
//!
//! In rc.4 the Planning Center length routes always answered 503 although the service behind them
//! existed, and no test noticed because the stub was also the documented "not connected" answer.
//! This test scans the route sources. A 503 with a fixed message is only allowed when it is a
//! real condition check, and each one must be listed here so a new one is a conscious decision.

use std::fs;
use std::path::Path;

/// Wording a stub uses. None of it may appear in the server's route code.
const STUB_PHRASES: &[&str] = &[
    "not ported",
    "not available in this build",
    "not implemented",
    "pc_unavailable",
    "is a stub",
    "stubbed",
];

/// Fixed-message 503s that are real conditions (the condition is named next to each one).
const ALLOWED_FIXED_503: &[&str] = &[
    // pco_routes.rs: the build has no Planning Center OAuth client id.
    "This build of StagePilot cannot sign in to Planning Center.",
    // pco_routes.rs: loading the selected plan failed.
    "Planning Center could not process the plan selection.",
];

fn sources() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<_> = fs::read_dir(&dir)
        .expect("src directory")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let text = fs::read_to_string(&p).expect("readable source");
            (p.file_name().unwrap().to_string_lossy().into_owned(), text)
        })
        .collect()
}

/// Collapse whitespace so a call split over several lines is one string.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn no_route_source_carries_stub_wording() {
    for (name, text) in sources() {
        let lower = text.to_lowercase();
        for phrase in STUB_PHRASES {
            assert!(
                !lower.contains(phrase),
                "{name} contains the stub wording {phrase:?}: wire the real service instead"
            );
        }
    }
}

#[test]
fn every_fixed_message_503_is_a_listed_condition() {
    for (name, text) in sources() {
        let flat = squash(&text);
        for call in ["http_error(503, \"", "http_error( 503, \""] {
            let mut rest = flat.as_str();
            while let Some(at) = rest.find(call) {
                let after = &rest[at + call.len()..];
                let message = after.split('"').next().unwrap_or_default();
                assert!(
                    ALLOWED_FIXED_503.contains(&message),
                    "{name}: a route answers a fixed 503 {message:?}. If it is a real condition, \
                     list it in ALLOWED_FIXED_503; if it stands in for an unported feature, wire \
                     the feature"
                );
                rest = after;
            }
        }
    }
}
