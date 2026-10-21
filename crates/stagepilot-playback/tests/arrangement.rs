//! Port of the pure-function half of `test_playback_arrangement.py` (pairing around skipped songs
//! and length alignment). The settings/HTTP/mapper tests in that file are phase 3b.

use stagepilot_playback::arrangement::*;

// The owner's example: five plan songs, five Playback songs; plan song 4 is acoustic (no track)
// and Playback song 5 is background music that is not in the service.
const PLAN: [&str; 5] = ["a", "b", "c", "d", "e"];
const ORDER: [i64; 5] = [900, 17, 42, 55, 70];

#[test]
fn without_skips_nothing_changes() {
    assert_eq!(
        pair_songs(&PLAN, &ORDER, &[], &[]),
        [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4)]
    );
    let got: Vec<_> = ORDER
        .iter()
        .map(|s| plan_index_for(*s, Some(&PLAN), &ORDER, &[], &[]))
        .collect();
    assert_eq!(got, [Some(0), Some(1), Some(2), Some(3), Some(4)]);
}

#[test]
fn the_owners_example_pairs_around_both_skips() {
    assert_eq!(
        pair_songs(&PLAN, &ORDER, &["d"], &[70]),
        [(0, 0), (1, 1), (2, 2), (4, 3)]
    );
    assert_eq!(
        plan_index_for(55, Some(&PLAN), &ORDER, &["d"], &[70]),
        Some(4)
    );
    assert_eq!(plan_index_for(70, Some(&PLAN), &ORDER, &["d"], &[70]), None);
}

#[test]
fn pairs_never_cross_or_reorder() {
    let plan_skips: [&[&str]; 4] = [&[], &["a"], &["b", "d"], &["c"]];
    let pb_skips: [&[i64]; 4] = [&[], &[900], &[17, 55], &[70]];
    for ps in plan_skips {
        for bs in pb_skips {
            let pairs = pair_songs(&PLAN, &ORDER, ps, bs);
            assert!(
                pairs.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1),
                "{ps:?} {bs:?}"
            );
        }
    }
}

#[test]
fn a_song_with_no_partner_drives_nothing() {
    assert_eq!(plan_index_for(70, Some(&PLAN), &ORDER, &["a"], &[]), None);
    assert_eq!(
        plan_index_for(900, Some(&PLAN), &ORDER, &["a"], &[]),
        Some(1)
    );
}

#[test]
fn unknown_song_and_missing_plan_drive_nothing() {
    assert_eq!(plan_index_for(1, Some(&PLAN), &ORDER, &["d"], &[70]), None);
    assert_eq!(plan_index_for(900, None, &ORDER, &["d"], &[70]), None);
}

#[test]
fn a_plan_that_lost_a_skipped_song_is_not_trusted() {
    assert_eq!(
        plan_index_for(55, Some(&["a", "b", "c", "e"]), &ORDER, &["d"], &[70]),
        None
    );
}

#[test]
fn without_skips_the_one_to_one_rule_needs_no_plan() {
    assert_eq!(plan_index_for(42, None, &ORDER, &[], &[]), Some(2));
}

#[test]
fn lengths_are_laid_out_by_plan_song() {
    let lengths = [
        Some(418.0),
        Some(312.0),
        Some(535.0),
        Some(298.0),
        Some(606.0),
    ];
    assert_eq!(
        align_lengths(&PLAN, &ORDER, &lengths, &["d"], &[70]),
        [Some(418.0), Some(312.0), Some(535.0), None, Some(298.0)]
    );
    assert_eq!(align_lengths(&PLAN, &ORDER, &lengths, &[], &[]), lengths);
    // Fewer measured lengths than songs leaves the rest unmeasured.
    assert_eq!(
        align_lengths(&PLAN, &ORDER, &lengths[..2], &[], &[]),
        [Some(418.0), Some(312.0), None, None, None]
    );
}
