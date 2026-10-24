//! M1: the Planning Center user agent names the real application version.

use stagepilot_core::settings::APP_VERSION;
use stagepilot_pco::models::{default_user_agent, PcoSettings};

#[test]
fn the_default_user_agent_carries_the_build_version() {
    let agent = default_user_agent();
    assert!(
        agent.starts_with(&format!("StagePilot/{APP_VERSION} (")),
        "{agent}"
    );
    assert!(!agent.contains("2.41.1"), "{agent}");
    assert_eq!(PcoSettings::default().user_agent, agent);
}
