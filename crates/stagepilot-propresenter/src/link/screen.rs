//! The screen side of the ProPresenter Link: ProPresenter's own Search and library list, driven with macOS Accessibility.
//!
//! The scripts are bundled files (`assets/gui/`): one AppleScript per job and one small Swift click program. Values (a song
//! number, a name, a playlist) are only ever passed as arguments to `osascript`, never built into a script or a command line.
//! What was chosen: the click program is run by the Swift interpreter (`xcrun swift click.swift X Y`), so there is no compiled
//! file to keep, sign or trust; it is only needed by `select_library`, which is best effort anyway.
//!
//! Nothing here could be proven without a Mac running ProPresenter: the control names are those seen on ProPresenter 19.0.1
//! (see `docs/propresenter.md`).

use super::{LinkError, SelectOutcome};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const CHECK_ACCESS: &str = include_str!("../../assets/gui/check_access.applescript");
pub const IMPORT_SONG: &str = include_str!("../../assets/gui/import_song.applescript");
pub const SELECT_LIBRARY: &str = include_str!("../../assets/gui/select_library.applescript");
pub const CLICK_SWIFT: &str = include_str!("../../assets/gui/click.swift");

/// Why a song cannot be imported on a computer that is not a Mac.
pub const NOT_A_MAC: &str = "Importing a song needs a Mac; this computer is not one.";
/// Why not, when macOS has not allowed StagePilot to control other programs.
pub const NO_ACCESS: &str =
    "Allow StagePilot in System Settings > Privacy & Security > Accessibility, then try again.";

const OSASCRIPT: &str = "/usr/bin/osascript";
const ACCESS_RECHECK: Duration = Duration::from_secs(30);
const IMPORT_TIMEOUT: Duration = Duration::from_secs(90);

/// What the screen can do. The product uses [`MacScreen`]; tests use a stand-in.
#[async_trait]
pub trait Screen: Send + Sync {
    /// Whether the screen can be driven, and if not why, in plain words.
    async fn access(&self) -> (bool, Option<String>);
    /// Runs the import steps up to and including pressing Import and closing ProPresenter's Search panel.
    async fn import(
        &self,
        multitracks_id: &str,
        name: &str,
        playlist: &str,
    ) -> Result<(), LinkError>;
    /// Best effort selection of a library row.
    async fn select_library(&self, name: &str) -> SelectOutcome;
}

/// The real screen of this computer.
pub struct MacScreen {
    dir: PathBuf,
    access: Mutex<Option<(Instant, bool, Option<String>)>>,
}

impl MacScreen {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            access: Mutex::new(None),
        }
    }

    /// Where the scripts are written for `osascript` to read: a folder only the current user can use.
    pub fn default_dir() -> PathBuf {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map_or_else(std::env::temp_dir, PathBuf::from);
        if cfg!(target_os = "macos") {
            home.join("Library/Application Support/StagePilot/propresenter-link")
        } else {
            home.join(".local/share/stagepilot/propresenter-link")
        }
    }

    fn script(&self, name: &str, text: &str) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let path = self.dir.join(name);
        if std::fs::read(&path).ok().as_deref() != Some(text.as_bytes()) {
            let temp = self.dir.join(format!("{name}.new"));
            std::fs::write(&temp, text)?;
            std::fs::rename(&temp, &path)?;
        }
        Ok(path)
    }

    /// The last line osascript printed.
    async fn run(
        &self,
        name: &str,
        text: &str,
        args: &[&str],
        limit: Duration,
    ) -> Result<String, LinkError> {
        let path = self
            .script(name, text)
            .map_err(|_| LinkError::Io("The screen scripts could not be written.".into()))?;
        let child = tokio::process::Command::new(OSASCRIPT)
            .arg(&path)
            .args(args)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output();
        let output = match tokio::time::timeout(limit, child).await {
            Err(_) => {
                return Err(LinkError::Timeout(
                    "ProPresenter did not answer on screen in time.".into(),
                ))
            }
            Ok(Err(_)) => {
                return Err(LinkError::Io(
                    "The screen scripts could not be started on this computer.".into(),
                ))
            }
            Ok(Ok(output)) => output,
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(line) = stdout.lines().rev().find(|line| !line.trim().is_empty()) {
            return Ok(line.trim().to_string());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if ["assistive", "not allowed", "-25211", "-1719"]
            .iter()
            .any(|needle| stderr.contains(needle))
        {
            return Ok(format!("ERR|check|{NO_ACCESS}"));
        }
        Ok("ERR|run|The screen script stopped without an answer.".to_string())
    }
}

#[async_trait]
impl Screen for MacScreen {
    async fn access(&self) -> (bool, Option<String>) {
        if !cfg!(target_os = "macos") {
            return (false, Some(NOT_A_MAC.to_string()));
        }
        if let Some((at, ok, why)) = self
            .access
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            if at.elapsed() < ACCESS_RECHECK {
                return (ok, why);
            }
        }
        let asked = self
            .run(
                "check_access.applescript",
                CHECK_ACCESS,
                &[],
                Duration::from_secs(10),
            )
            .await;
        let ok = asked.as_ref().is_ok_and(|line| line.starts_with("OK"));
        let why = (!ok).then(|| NO_ACCESS.to_string());
        // A check that got no answer (macOS may be asking the person) is not remembered as "not allowed": it is asked again.
        if !matches!(asked, Err(LinkError::Timeout(_))) {
            *self.access.lock().unwrap_or_else(|p| p.into_inner()) =
                Some((Instant::now(), ok, why.clone()));
        }
        (ok, why)
    }

    async fn import(
        &self,
        multitracks_id: &str,
        name: &str,
        playlist: &str,
    ) -> Result<(), LinkError> {
        let line = self
            .run(
                "import_song.applescript",
                IMPORT_SONG,
                &[multitracks_id, name, playlist],
                IMPORT_TIMEOUT,
            )
            .await?;
        parse_import_answer(&line)
    }

    async fn select_library(&self, name: &str) -> SelectOutcome {
        let untried = |detail: &str| SelectOutcome {
            tried: false,
            detail: Some(detail.to_string()),
        };
        let (ok, why) = self.access().await;
        if !ok {
            return untried(&why.unwrap_or_default());
        }
        let Ok(line) = self
            .run(
                "select_library.applescript",
                SELECT_LIBRARY,
                &[name],
                Duration::from_secs(30),
            )
            .await
        else {
            return untried("The library row could not be looked for.");
        };
        let Some((x, y)) = parse_found(&line) else {
            return untried("The library row was not found on screen.");
        };
        let Ok(click) = self.script("click.swift", CLICK_SWIFT) else {
            return untried("The click could not be prepared.");
        };
        let run = tokio::process::Command::new("/usr/bin/xcrun")
            .arg("swift")
            .arg(click)
            .arg(x.to_string())
            .arg(y.to_string())
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output();
        match tokio::time::timeout(Duration::from_secs(90), run).await {
            Ok(Ok(output)) if output.status.success() => SelectOutcome {
                tried: true,
                detail: None,
            },
            _ => untried("The click could not be made."),
        }
    }
}

/// `OK|<name>.pro` is success; `ERR|<step>|<why>` names the step that stopped.
pub fn parse_import_answer(line: &str) -> Result<(), LinkError> {
    if line.starts_with("OK|") || line == "OK" {
        return Ok(());
    }
    if let Some(rest) = line.strip_prefix("ERR|") {
        let (step, message) = rest.split_once('|').unwrap_or((rest, ""));
        if step == "check" {
            return Err(LinkError::CannotImport(NO_ACCESS.to_string()));
        }
        return Err(LinkError::Screen {
            step: step.chars().take(80).collect(),
            message: message.chars().take(300).collect(),
        });
    }
    Err(LinkError::Screen {
        step: "answer".into(),
        message: "The screen script gave an answer that was not understood.".into(),
    })
}

/// `FOUND|x|y` as two whole numbers.
pub fn parse_found(line: &str) -> Option<(i64, i64)> {
    let mut parts = line.split('|');
    (parts.next()? == "FOUND").then_some(())?;
    let x = parts.next()?.parse().ok()?;
    let y = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((x, y))
}

/// `true` when `dir` holds exactly the bundled text for a script (tests).
pub fn script_is_current(dir: &Path, name: &str, text: &str) -> bool {
    std::fs::read(dir.join(name)).ok().as_deref() == Some(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read_by_their_first_word() {
        assert!(parse_import_answer("OK|Song.pro").is_ok());
        assert_eq!(
            parse_import_answer("ERR|type the song number|The Search panel has no text field."),
            Err(LinkError::Screen {
                step: "type the song number".into(),
                message: "The Search panel has no text field.".into()
            })
        );
        assert_eq!(
            parse_import_answer("ERR|check|x"),
            Err(LinkError::CannotImport(NO_ACCESS.into()))
        );
        assert!(parse_import_answer("whatever").is_err());
        assert_eq!(parse_found("FOUND|120|340"), Some((120, 340)));
        for bad in ["FOUND|1", "FOUND|a|2", "ERR|x|y", "FOUND|1|2|3", ""] {
            assert_eq!(parse_found(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_bundled_scripts_say_what_they_touch() {
        // The import script presses only the controls it names, and says so in its header.
        assert!(IMPORT_SONG.contains("It touches only:"));
        for step in [
            "open the Search panel",
            "choose the MultiTracks source",
            "type the song number",
            "read the search result",
            "open the Import dialog",
            "fill in the Import dialog",
            "press Import",
            "close the Search panel",
        ] {
            assert!(IMPORT_SONG.contains(step), "{step}");
        }
        assert!(IMPORT_SONG.contains("not exactly one"));
        assert!(IMPORT_SONG.contains("button \"Cancel\" of dlg"));
        // Values come in as arguments only; no shell is built.
        for text in [IMPORT_SONG, SELECT_LIBRARY, CHECK_ACCESS] {
            assert!(!text.contains("do shell script"));
        }
    }

    #[tokio::test]
    async fn a_computer_that_is_not_a_mac_cannot_import() {
        if cfg!(target_os = "macos") {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let screen = MacScreen::new(dir.path().join("scripts"));
        assert_eq!(screen.access().await, (false, Some(NOT_A_MAC.to_string())));
        let outcome = screen.select_library("Playback").await;
        assert!(!outcome.tried);
    }

    #[test]
    fn a_script_is_written_once_and_only_rewritten_when_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        let screen = MacScreen::new(dir.path().join("scripts"));
        let path = screen.script("a.applescript", "one").unwrap();
        assert!(script_is_current(
            &dir.path().join("scripts"),
            "a.applescript",
            "one"
        ));
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        screen.script("a.applescript", "one").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before
        );
        screen.script("a.applescript", "two").unwrap();
        assert!(script_is_current(
            &dir.path().join("scripts"),
            "a.applescript",
            "two"
        ));
    }
}
