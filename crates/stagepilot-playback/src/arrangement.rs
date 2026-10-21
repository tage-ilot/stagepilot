//! Pair Playback songs with service-plan songs when either side has songs to skip:
//! mirrors `playback_api/arrangement.py`.
//!
//! Neither list is ever reordered. The Nth song that is not skipped on one side pairs with the
//! Nth that is not skipped on the other, so a crossed or out-of-order pairing cannot exist.

/// `(plan index, Playback index)` for every linked pair, both zero-based.
pub fn pair_songs(
    plan_song_ids: &[&str],
    song_order: &[i64],
    skipped_plan_song_ids: &[&str],
    skipped_song_ids: &[i64],
) -> Vec<(usize, usize)> {
    let plan_side = plan_song_ids
        .iter()
        .enumerate()
        .filter(|(_, id)| !skipped_plan_song_ids.contains(id))
        .map(|(i, _)| i);
    let playback_side = song_order
        .iter()
        .enumerate()
        .filter(|(_, id)| !skipped_song_ids.contains(id))
        .map(|(i, _)| i);
    plan_side.zip(playback_side).collect()
}

/// The plan song a Playback song starts, or `None` when it must not drive the timer: a skipped
/// Playback song, a song with no partner, an unknown song, and any arrangement that cannot be
/// trusted (skips are saved but the loaded plan is missing or no longer contains every skipped
/// plan song).
pub fn plan_index_for(
    song_id: i64,
    plan_song_ids: Option<&[&str]>,
    song_order: &[i64],
    skipped_plan_song_ids: &[&str],
    skipped_song_ids: &[i64],
) -> Option<usize> {
    if skipped_song_ids.contains(&song_id) {
        return None;
    }
    let position = song_order.iter().position(|id| *id == song_id)?;
    if skipped_plan_song_ids.is_empty() && skipped_song_ids.is_empty() {
        return Some(position); // The original one-to-one behaviour, unchanged.
    }
    let plan = plan_song_ids?;
    if !skipped_plan_song_ids.iter().all(|s| plan.contains(s)) {
        return None;
    }
    pair_songs(plan, song_order, skipped_plan_song_ids, skipped_song_ids)
        .into_iter()
        .find(|(_, playback_index)| song_order[*playback_index] == song_id)
        .map(|(plan_index, _)| plan_index)
}

/// Playback lengths laid out by plan song: `None` where a plan song has no linked track.
pub fn align_lengths(
    plan_song_ids: &[&str],
    song_order: &[i64],
    lengths: &[Option<f64>],
    skipped_plan_song_ids: &[&str],
    skipped_song_ids: &[i64],
) -> Vec<Option<f64>> {
    let mut aligned = vec![None; plan_song_ids.len()];
    for (plan_index, playback_index) in pair_songs(
        plan_song_ids,
        song_order,
        skipped_plan_song_ids,
        skipped_song_ids,
    ) {
        if let Some(l) = lengths.get(playback_index) {
            aligned[plan_index] = *l;
        }
    }
    aligned
}
