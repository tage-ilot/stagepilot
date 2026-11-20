//! What `GET /api/v1/propresenter-link/status` says: how StagePilot reaches the ProPresenter computer and whether that works.
//! The helper's key is never part of it.

use serde::Serialize;
use stagepilot_core::helper_update::HelperUpdateStatus;
use ts_rs::TS;

/// How the ProPresenter computer is reached. The rule: the saved ProPresenter host is this computer (a loopback address or one of
/// this computer's own addresses): `local`; any other computer: `helper`. There is no switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LinkMode {
    /// StagePilot runs on the ProPresenter computer: the files are used directly and no helper is needed.
    Local,
    /// ProPresenter is on another computer: the ProPresenter Link helper there is used.
    Helper,
}

/// The ProPresenter Link's state. The helper states are those of the Playback Library Link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LinkState {
    /// Local: ProPresenter's folder is here. Helper: it answers to this StagePilot's key and is the bundled version or newer.
    Ready,
    /// Nothing answers at the ProPresenter computer and no setup was ever started (or no ProPresenter computer is chosen).
    NotInstalled,
    /// It answers and works but is older than the one bundled in StagePilot.
    Outdated,
    /// It answers but refuses this StagePilot (no key, a different key, or this computer is not the one it answers).
    NeedsCommand,
    /// A setup was started and it does not answer now (or Local: the folder is not there).
    NotAnswering,
    /// The first look is being made.
    Checking,
    /// Local: macOS did not let the look through in time, most likely because it is asking the person for permission (Documents
    /// folder, Automation, Accessibility). The next look tries again.
    Waiting,
}

impl LinkState {
    /// The helper can be used (files, index, import) in these.
    pub fn is_usable(self) -> bool {
        matches!(self, Self::Ready | Self::Outdated)
    }
}

/// What the status says while macOS may be asking the person for permission on this computer.
pub const WAITING_FOR_MACOS: &str =
    "macOS may be asking for permission on this computer. Answer the prompt, then press Connect again.";

/// The one plain sentence for a state.
pub fn state_words(mode: Option<LinkMode>, state: LinkState, version: Option<&str>) -> String {
    let version = version
        .filter(|v| !v.is_empty())
        .map_or(String::new(), |v| format!(" (version {v})"));
    match (mode, state) {
        (None, _) => "Choose the ProPresenter computer first.".to_string(),
        (Some(LinkMode::Local), LinkState::Waiting) => WAITING_FOR_MACOS.to_string(),
        (Some(LinkMode::Local), LinkState::Ready) => {
            "ProPresenter is on this computer, so no setup is needed.".to_string()
        }
        (Some(LinkMode::Local), _) => {
            "ProPresenter's folder was not found on this computer. Open ProPresenter once, or give its folder in the settings."
                .to_string()
        }
        (_, LinkState::NotInstalled) => {
            "The Presenter Link is not installed on the ProPresenter computer.".to_string()
        }
        (_, LinkState::NeedsCommand) => {
            "The Presenter Link does not know this StagePilot yet. Run the setup command again."
                .to_string()
        }
        (_, LinkState::Outdated) => format!(
            "The Presenter Link{version} can be updated. It still works; run the setup command to update it."
        ),
        (_, LinkState::Ready) => format!("The Presenter Link{version} is ready."),
        (_, LinkState::NotAnswering) => "The Presenter Link did not answer.".to_string(),
        (_, LinkState::Checking) => "Checking the Presenter Link…".to_string(),
        (_, LinkState::Waiting) => "Waiting for the ProPresenter computer to answer.".to_string(),
    }
}

/// The body of `GET /api/v1/propresenter-link/status`.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct LinkStatus {
    /// `null` while no ProPresenter computer is chosen (nothing is asked then).
    pub mode: Option<LinkMode>,
    pub state: LinkState,
    /// One plain sentence for the operator, never empty.
    pub message: String,
    /// The helper's version, when it answered.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub version: Option<String>,
    /// The version bundled in this StagePilot (what the setup command installs).
    pub bundled_version: String,
    /// A song can be imported through ProPresenter's Search from there.
    pub can_import: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub import_blocker: Option<String>,
    /// The library folders in ProPresenter's folder.
    pub libraries: Vec<String>,
    /// The library named `Playback` is there.
    pub playback_library_exists: bool,
    /// The saved ProPresenter computer, as typed (helper mode only).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub computer_address: Option<String>,
    /// ProPresenter's folder on that computer.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub data_folder: Option<String>,
    /// A setup session has made a key (helper mode).
    pub paired: bool,
    /// The helper can install an update that StagePilot sends.
    pub updater: bool,
    /// The answer shown is the last good one: the computer did not answer just now.
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub last_update_error: Option<String>,
    /// What StagePilot is doing about an older helper (left out when there is nothing to say).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<HelperUpdateStatus>")]
    pub update: Option<HelperUpdateStatus>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_one_plain_sentence() {
        for mode in [None, Some(LinkMode::Local), Some(LinkMode::Helper)] {
            for state in [
                LinkState::Ready,
                LinkState::NotInstalled,
                LinkState::Outdated,
                LinkState::NeedsCommand,
                LinkState::NotAnswering,
                LinkState::Checking,
                LinkState::Waiting,
            ] {
                let words = state_words(mode, state, Some("0.1.0"));
                assert!(
                    !words.is_empty() && !words.contains('\n'),
                    "{mode:?} {state:?}"
                );
            }
        }
        assert_eq!(
            state_words(Some(LinkMode::Helper), LinkState::Ready, Some("0.1.0")),
            "The Presenter Link (version 0.1.0) is ready."
        );
        assert_eq!(
            state_words(Some(LinkMode::Helper), LinkState::Ready, None),
            "The Presenter Link is ready."
        );
        assert_eq!(
            serde_json::to_value(LinkState::NeedsCommand).unwrap(),
            serde_json::json!("needs_command")
        );
        assert_eq!(
            serde_json::to_value(LinkState::Waiting).unwrap(),
            serde_json::json!("waiting")
        );
        assert_eq!(
            state_words(Some(LinkMode::Local), LinkState::Waiting, None),
            "macOS may be asking for permission on this computer. Answer the prompt, then press Connect again."
        );
    }
}
