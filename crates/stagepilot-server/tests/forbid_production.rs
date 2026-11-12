//! The server's tests get the test-build guard from this crate's dev-dependencies on
//! `stagepilot-remote` and `stagepilot-pco` (feature `forbid-production`). Both names below exist
//! only with the feature, so a build that loses a dev-dependency fails to compile this file, and
//! the calls prove the guard refuses the production hosts the server's managers would reach.

use stagepilot_remote::bootstrap::DEFAULT_CONTROL_PLANE_ORIGIN;

#[test]
fn the_server_tests_have_the_guard_from_both_crates() {
    let remote = stagepilot_remote::http::expecting_refusal(|| {
        stagepilot_remote::http::refuse_production(DEFAULT_CONTROL_PLANE_ORIGIN)
    })
    .unwrap_err();
    assert!(remote
        .0
        .starts_with("refusing to contact production in a test: "));
    let url = reqwest::Url::parse(stagepilot_pco::client::API_BASE_URL).unwrap();
    let pco = stagepilot_pco::production_guard::expecting_refusal(|| {
        stagepilot_pco::production_guard::refuse_production(&url)
    })
    .unwrap_err();
    assert!(pco.starts_with("refusing to contact production in a test: "));
}
