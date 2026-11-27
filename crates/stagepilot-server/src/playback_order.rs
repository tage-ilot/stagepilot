//! The song order from the Library Link: the one place that gathers what the decision reads, asks it
//! (`stagepilot_playback::library_order::decide`) and acts on the answer.
//!
//! Three things start a look, and all three go through the same code (`look`):
//! * the watcher started with the runtime looks every few seconds; a look costs nothing while the Library
//!   Link was never in use or Playback is not the selected source, and the helper itself is asked no more
//!   often than the rest of the Library Link is (see `PlaybackLibrary::open_setlist`);
//! * the connection made when StagePilot starts (`connect_at_start`): with a key paired it asks the helper
//!   at once, fresh, tries again after 2 s, 5 s, 15 s and then every 30 s while the helper is silent, and
//!   decides as soon as the helper has answered once. It never blocks start-up and does nothing at all when
//!   no key was ever paired;
//! * the "Scan Playback Songs" button while the Library Link works (`refresh_from_library`): a fresh read,
//!   and the order is saved even over an older scan, because the press asks for exactly that.
//!
//! The order is saved only when it differs from what is saved (`PlaybackPlugin::apply_library_order`); the
//! plain reason it is not used is handed to the plugin, which shows it in the status. The reason is
//! information only and never touches the verdict.

use crate::playback_library::{effective_settings, note_in_use};
use crate::runtime::Runtime;
use stagepilot_playback::library::PlanTitle;
use stagepilot_playback::library_order::{decide, Evidence, LibraryOrder, Verdict};
use stagepilot_playback::plugin::LibrarySave;
use std::time::Duration;

/// How often the watcher looks.
pub const WATCH_EVERY: Duration = Duration::from_secs(5);

/// The waits between the tries made at start-up while the helper is silent: 2 s, 5 s, 15 s, then 30 s each.
pub const START_RETRY: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(15),
];
pub const START_RETRY_LATER: Duration = Duration::from_secs(30);

/// How long to wait after the `tries`th failed try (1 = the first).
pub fn start_wait(tries: usize) -> Duration {
    START_RETRY
        .get(tries.saturating_sub(1))
        .copied()
        .unwrap_or(START_RETRY_LATER)
}

/// What one look came to.
#[derive(Debug, Clone, PartialEq)]
enum Looked {
    /// The Library Link is not part of this (never used, forgotten, not selected, no answer at all).
    Quiet,
    /// The order is not used, for the reason already handed to the plugin.
    NotUsed,
    /// The order holds; `Some` is what was decided.
    Use(LibraryOrder),
}

/// Gather what the decision reads and decide. `fresh` reads the helper again whatever its throttle says. The
/// reason, when there is one, is handed to the plugin here.
async fn look(rt: &Runtime, fresh: bool, needs_use: bool) -> Looked {
    let settings = effective_settings(rt);
    // Only a Library Link that has been in use is asked from here; the first time the helper answers is the
    // Library Link block's doing (`note_in_use`), never an unprompted look at a computer that may never have
    // had one. A person's press (`needs_use` false) is the one exception: it is the person asking.
    if !rt.playback.wants_library_order() || (needs_use && settings.enabled != Some(true)) {
        rt.playback.set_library_note(None);
        return Looked::Quiet;
    }
    let open = if fresh {
        rt.playback_library.open_setlist_fresh(&settings).await
    } else {
        rt.playback_library.open_setlist(&settings).await
    };
    let plan = rt.store.snapshot().plan;
    let titles: Option<Vec<PlanTitle>> = plan
        .as_ref()
        .map(|plan| rt.playback_library.plan_titles(plan));
    let plan_ids: Vec<String> = plan
        .iter()
        .flat_map(|plan| plan.songs.iter().map(|song| song.id.clone()))
        .collect();
    let (skipped_playback, skipped_plan) = rt.playback.saved_skips();
    let (api_connected, heartbeat, loading_setlist) = rt.playback.library_evidence();
    let verdict = decide(&Evidence {
        link_in_use: open.in_use,
        link_current: open.current,
        current: open.setlist.as_ref(),
        api_connected,
        loading_setlist,
        heartbeat,
        plan: titles.as_deref(),
        plan_ids: &plan_ids,
        skipped_plan: &skipped_plan,
        skipped_playback: &skipped_playback,
    });
    match verdict {
        Verdict::Quiet => {
            rt.playback.set_library_note(None);
            Looked::Quiet
        }
        Verdict::NotUsed(line) => {
            rt.playback.set_library_note(Some(line));
            Looked::NotUsed
        }
        Verdict::Use(order) => {
            rt.playback.set_library_note(None);
            Looked::Use(order)
        }
    }
}

/// One look: decide, say why not when not, and save the order when it is new. Returns whether
/// something was saved.
pub async fn refresh(rt: &Runtime) -> bool {
    save_if_used(rt, look(rt, false, true).await).await
}

async fn save_if_used(rt: &Runtime, looked: Looked) -> bool {
    let Looked::Use(order) = looked else {
        return false;
    };
    match rt.playback.apply_library_order(&order).await {
        Ok(saved) => saved,
        Err(error) => {
            tracing::warn!(error = %error, "playback_library_order_not_saved");
            false
        }
    }
}

/// How a press on "Scan Playback Songs" with a working Library Link ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pressed {
    /// The order is the Library Link's now (saved, or it already was exactly this).
    Read,
    /// The Library Link is not usable here (no helper, no key, silent, not selected): the caller says so and
    /// the button steps through Playback as it always did.
    Unavailable,
    /// A condition failed; the plain line is in the status (`order_library_note`).
    NotUsed,
    /// The order holds but could not be saved just now (an update is running, or Playback changed its setlist).
    Busy,
}

/// The "Scan Playback Songs" press when the Library Link works: read the helper fresh, decide with the same
/// five conditions, save the order as the library's (over an older scan too). Playback's transport is never
/// touched: nothing is selected, sought or played.
pub async fn refresh_from_library(rt: &Runtime) -> Pressed {
    // The press is the person asking: it needs a paired key and a helper that answers, not an earlier
    // "in use" mark.
    if !rt.playback_library.is_paired() {
        return Pressed::Unavailable;
    }
    let looked = look(rt, true, false).await;
    // The helper answered with a key: it has been in use (what a window's status check would have noted).
    note_in_use(rt, !matches!(looked, Looked::Quiet)).await;
    match looked {
        Looked::Quiet => Pressed::Unavailable,
        Looked::NotUsed => Pressed::NotUsed,
        Looked::Use(order) => match rt.playback.apply_library_order_on_request(&order).await {
            Ok(LibrarySave::Written | LibrarySave::AlreadySaved) => Pressed::Read,
            Ok(LibrarySave::Declined) => Pressed::Busy,
            Err(error) => {
                tracing::warn!(error = %error, "playback_library_order_not_saved");
                Pressed::Busy
            }
        },
    }
}

/// The connection made when the runtime starts, with no window open and without waiting for the first look of
/// the watcher. One task, never awaited by start-up. With no key paired the Library Link was never set up
/// and nothing is asked. Otherwise: read the helper fresh (health, then the open setlist and its songs); while
/// it is silent try again after 2 s, 5 s, 15 s and then every 30 s; once it has answered, decide and save the
/// order when the five conditions hold, and leave the rest to the watcher.
pub fn connect_at_start(rt: std::sync::Weak<Runtime>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tries = 0usize;
        loop {
            {
                let Some(rt) = rt.upgrade() else { return };
                // Never set up (or forgotten): nothing is asked, ever, from here.
                if !rt.playback_library.is_paired() {
                    return;
                }
                if connect_once(&rt).await {
                    return;
                }
            }
            tries += 1;
            tokio::time::sleep(start_wait(tries)).await;
        }
    })
}

/// One try. `true` when the helper answered (whatever the decision was).
async fn connect_once(rt: &Runtime) -> bool {
    let settings = effective_settings(rt);
    let open = rt.playback_library.open_setlist_fresh(&settings).await;
    if !open.in_use {
        return false;
    }
    // The helper answers with this key: from now on the Library Link is in use, and the watcher looks.
    note_in_use(rt, true).await;
    // The same look as every other, with the helper's answer just read (not read twice: it is held).
    let looked = look(rt, false, true).await;
    save_if_used(rt, looked).await;
    true
}

/// The watcher: one task for the runtime's life.
pub fn watch(rt: std::sync::Weak<Runtime>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(WATCH_EVERY).await;
            let Some(rt) = rt.upgrade() else { return };
            refresh(&rt).await;
        }
    })
}
