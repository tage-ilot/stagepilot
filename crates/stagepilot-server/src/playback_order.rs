//! The song order from the Library Link: the one place that gathers what the decision reads, asks it
//! (`stagepilot_playback::library_order::decide`) and acts on the answer.
//!
//! Nothing here is a button. A watcher started with the runtime looks every few seconds; a look costs
//! nothing while the Library Link was never in use or Playback is not the selected source, and the
//! helper itself is asked no more often than the rest of the Library Link is (see
//! `PlaybackLibrary::open_setlist`). The order is saved only when it differs from what is saved
//! (`PlaybackPlugin::apply_library_order`); the plain reason it is not used is handed to the plugin,
//! which shows it in the status. The reason is information only and never touches the verdict.

use crate::playback_library::{effective_settings, plan_titles};
use crate::runtime::Runtime;
use stagepilot_playback::library::PlanTitle;
use stagepilot_playback::library_order::{decide, Evidence, Verdict};
use std::time::Duration;

/// How often the watcher looks.
pub const WATCH_EVERY: Duration = Duration::from_secs(5);

/// One look: decide, say why not when not, and save the order when it is new. Returns whether
/// something was saved.
pub async fn refresh(rt: &Runtime) -> bool {
    let settings = effective_settings(rt);
    // Only a Library Link that has been in use is asked from here; the first time the helper answers
    // is the Library Link block's doing (`note_in_use`), never an unprompted look at a computer that
    // may never have had one.
    if !rt.playback.wants_library_order() || settings.enabled != Some(true) {
        rt.playback.set_library_note(None);
        return false;
    }
    let open = rt.playback_library.open_setlist(&settings).await;
    let plan = rt.store.snapshot().plan;
    let titles: Option<Vec<PlanTitle>> = plan.as_ref().map(plan_titles);
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
            false
        }
        Verdict::NotUsed(line) => {
            rt.playback.set_library_note(Some(line));
            false
        }
        Verdict::Use(order) => {
            rt.playback.set_library_note(None);
            match rt.playback.apply_library_order(&order).await {
                Ok(saved) => saved,
                Err(error) => {
                    tracing::warn!(error = %error, "playback_library_order_not_saved");
                    false
                }
            }
        }
    }
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
