"""Pair Playback songs with service-plan songs when either side has songs to skip.

Neither list is ever reordered. The Nth song that is not skipped on one side pairs with the
Nth that is not skipped on the other, so a crossed or out-of-order pairing cannot exist.
"""

from collections.abc import Collection, Sequence


def pair_songs(
    plan_song_ids: Sequence[str],
    song_order: Sequence[int],
    skipped_plan_song_ids: Collection[str] = (),
    skipped_song_ids: Collection[int] = (),
) -> list[tuple[int, int]]:
    """Return (plan index, Playback index) for every linked pair, both zero-based."""
    plan_side = [
        i for i, song_id in enumerate(plan_song_ids) if song_id not in skipped_plan_song_ids
    ]
    playback_side = [i for i, song_id in enumerate(song_order) if song_id not in skipped_song_ids]
    return list(zip(plan_side, playback_side, strict=False))


def plan_index_for(
    song_id: int,
    plan_song_ids: Sequence[str] | None,
    song_order: Sequence[int],
    skipped_plan_song_ids: Collection[str] = (),
    skipped_song_ids: Collection[int] = (),
) -> int | None:
    """The plan song a Playback song starts, or None when it must not drive the timer.

    None for a skipped Playback song, a song with no partner, an unknown song, and any arrangement
    that cannot be trusted: skips are saved but the loaded plan is missing or no longer contains
    every skipped plan song (the plan changed underneath the arrangement).
    """
    if song_id in skipped_song_ids or song_id not in song_order:
        return None
    if not skipped_plan_song_ids and not skipped_song_ids:
        return song_order.index(song_id)  # The original one-to-one behaviour, unchanged.
    if plan_song_ids is None or not set(skipped_plan_song_ids) <= set(plan_song_ids):
        return None
    for plan_index, playback_index in pair_songs(
        plan_song_ids, song_order, skipped_plan_song_ids, skipped_song_ids
    ):
        if song_order[playback_index] == song_id:
            return plan_index
    return None


def align_lengths(
    plan_song_ids: Sequence[str],
    song_order: Sequence[int],
    lengths: Sequence[float | None],
    skipped_plan_song_ids: Collection[str] = (),
    skipped_song_ids: Collection[int] = (),
) -> list[float | None]:
    """Playback lengths laid out by plan song: None where a plan song has no linked track."""
    aligned: list[float | None] = [None] * len(plan_song_ids)
    for plan_index, playback_index in pair_songs(
        plan_song_ids, song_order, skipped_plan_song_ids, skipped_song_ids
    ):
        if playback_index < len(lengths):
            aligned[plan_index] = lengths[playback_index]
    return aligned
