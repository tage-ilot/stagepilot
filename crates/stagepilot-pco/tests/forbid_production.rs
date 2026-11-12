//! The test-run guard on the real Planning Center clients (see `production_guard`): the default
//! API base and the control-plane origin are refused before any socket is opened.

mod support;

use stagepilot_pco::client::{PcoApi, PcoClient};
use stagepilot_pco::errors::PcoError;
use stagepilot_pco::oauth::ControlPlaneClient;
use stagepilot_pco::production_guard::expecting_refusal;
use support::pat_settings;

const PREFIX: &str = "refusing to contact production in a test: ";

fn runtime() -> tokio::runtime::Runtime {
    // One thread: the future runs on the test thread, where `expecting_refusal` applies.
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn the_control_plane_client_refuses_the_production_origin() {
    let client = ControlPlaneClient::new().unwrap();
    let error = expecting_refusal(|| runtime().block_on(client.start_flow())).unwrap_err();
    assert!(error.message.starts_with(PREFIX), "{}", error.message);
    assert!(
        error
            .message
            .contains("stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev"),
        "{}",
        error.message
    );
}

#[test]
fn the_services_client_refuses_the_default_api_base() {
    let client = PcoClient::new(&pat_settings(), None).unwrap();
    let error = expecting_refusal(|| runtime().block_on(client.list_service_types())).unwrap_err();
    let PcoError::Configuration(message) = &error else {
        panic!("{error:?}");
    };
    assert!(message.starts_with(PREFIX), "{message}");
    assert!(
        message.contains("api.planningcenteronline.com"),
        "{message}"
    );
}
