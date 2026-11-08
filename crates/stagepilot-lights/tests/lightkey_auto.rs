//! Auto capture/replay: the pure decision, the availability grace (fake clock), the pass verdict
//! and the rules for which capture a song keeps.

use stagepilot_core::config::{
    CaptureLabel, CaptureStatus, LightkeyMode, OscCueSource, OscLightingCue, OscMessage,
    SongLightingCueMap,
};
use stagepilot_lights::lightkey_auto::{
    decide, judge, song_capture, Action, Availability, Coverage, PassVerdict, Reason, SongCapture,
    GRACE_SECONDS,
};
use stagepilot_lights::lightkey_looks::{save_capture, CaptureSave};

fn row(source: OscCueSource) -> OscLightingCue {
    OscLightingCue {
        id: uuid::Uuid::new_v4(),
        at_seconds: 1.0,
        address: "/fixture/P1/overrides/dimmer".into(),
        args: vec![],
        label: "Look".into(),
        color: None,
        messages: vec![OscMessage {
            address: "/fixture/P1/overrides/dimmer".into(),
            args: vec![],
        }],
        source,
        edited: false,
        fade_seconds: 0.0,
    }
}

fn map(rows: Vec<OscLightingCue>, capture: Option<CaptureStatus>) -> SongLightingCueMap {
    SongLightingCueMap {
        song_key: "s".into(),
        song_title: "S".into(),
        cues: vec![],
        osc_cues: rows,
        loudness: None,
        captured_edited: false,
        capture,
        effects: vec![],
    }
}

fn status(complete: bool, coverage: f64) -> CaptureStatus {
    CaptureStatus {
        complete,
        coverage_seconds: coverage,
        recorded_at: "2026-11-02T00:00:00Z".into(),
        project: None,
    }
}

// ---------------------------------------------------------------------------- the decision

#[test]
fn the_decision_table() {
    use Action::{Capture, Replay};
    use Reason::*;
    use SongCapture::{Complete, Incomplete, NotCaptured};
    let table = [
        // mode, connection, song, action, reason
        (
            LightkeyMode::Auto,
            true,
            NotCaptured,
            Capture,
            AutoCapturing,
        ),
        (LightkeyMode::Auto, true, Incomplete, Capture, AutoCapturing),
        (
            LightkeyMode::Auto,
            true,
            Complete,
            Replay,
            AutoFullyCaptured,
        ),
        (
            LightkeyMode::Auto,
            false,
            NotCaptured,
            Replay,
            AutoNoConnection,
        ),
        (
            LightkeyMode::Auto,
            false,
            Incomplete,
            Replay,
            AutoNoConnection,
        ),
        (
            LightkeyMode::Auto,
            false,
            Complete,
            Replay,
            AutoNoConnection,
        ),
        (
            LightkeyMode::Replay,
            true,
            NotCaptured,
            Replay,
            PinnedReplay,
        ),
        (
            LightkeyMode::Replay,
            false,
            NotCaptured,
            Replay,
            PinnedReplay,
        ),
        (LightkeyMode::Replay, true, Complete, Replay, PinnedReplay),
        (LightkeyMode::Record, true, Complete, Capture, PinnedRecord),
        (
            LightkeyMode::Record,
            true,
            NotCaptured,
            Capture,
            PinnedRecord,
        ),
        (
            LightkeyMode::Record,
            false,
            NotCaptured,
            Capture,
            PinnedRecord,
        ),
    ];
    for (mode, connected, song, action, reason) in table {
        let d = decide(mode, connected, song);
        assert_eq!(
            (d.action, d.reason),
            (action, reason),
            "{mode:?} {connected} {song:?}"
        );
    }
}

#[test]
fn reasons_read_as_the_strip_says_them() {
    assert_eq!(Reason::AutoCapturing.text(), "Auto: capturing this song");
    assert_eq!(
        Reason::AutoFullyCaptured.text(),
        "Auto: replaying (fully captured)"
    );
    assert_eq!(
        Reason::AutoNoConnection.text(),
        "Auto: replaying (no connection)"
    );
}

#[test]
fn a_song_is_classified_from_its_cue_map() {
    assert_eq!(song_capture(None), SongCapture::NotCaptured);
    let manual = map(vec![row(OscCueSource::Manual)], None);
    assert_eq!(song_capture(Some(&manual)), SongCapture::NotCaptured); // manual-only still captures
    let old = map(vec![row(OscCueSource::Captured)], None); // an older file: no status
    assert_eq!(song_capture(Some(&old)), SongCapture::Incomplete);
    let partial = map(vec![row(OscCueSource::Captured)], Some(status(false, 50.0)));
    assert_eq!(song_capture(Some(&partial)), SongCapture::Incomplete);
    let full = map(vec![row(OscCueSource::Captured)], Some(status(true, 240.0)));
    assert_eq!(song_capture(Some(&full)), SongCapture::Complete);
    // a status with no captured rows left (they were cleared) is not a capture
    let cleared = map(vec![row(OscCueSource::Manual)], Some(status(true, 240.0)));
    assert_eq!(song_capture(Some(&cleared)), SongCapture::NotCaptured);
}

#[test]
fn labels() {
    let cap = row(OscCueSource::Captured);
    let man = row(OscCueSource::Manual);
    let l = |m: &SongLightingCueMap| m.capture_label();
    assert_eq!(l(&map(vec![], None)), CaptureLabel::NotCaptured);
    assert_eq!(l(&map(vec![man.clone()], None)), CaptureLabel::ManuallyCued);
    assert_eq!(
        l(&map(vec![cap.clone()], Some(status(true, 9.0)))),
        CaptureLabel::FullyCaptured
    );
    assert_eq!(l(&map(vec![cap.clone()], None)), CaptureLabel::Incomplete);
    assert_eq!(
        l(&map(
            vec![cap.clone(), man.clone()],
            Some(status(true, 9.0))
        )),
        CaptureLabel::ManuallyCuedAndFullyCaptured
    );
    assert_eq!(
        l(&map(vec![cap, man], Some(status(false, 5.0)))),
        CaptureLabel::Incomplete
    );
}

// ------------------------------------------------------------------ availability and grace

#[test]
fn a_link_that_was_never_up_is_not_available() {
    let mut a = Availability::default();
    a.observe(0.0, false);
    assert!(!a.usable(0.0, false));
    assert!(!a.usable(500.0, false));
}

#[test]
fn a_drop_is_covered_for_sixty_seconds_when_a_song_starts() {
    let mut a = Availability::default();
    a.observe(100.0, true);
    assert!(a.usable(100.0, true));
    a.observe(110.0, false); // drop starts at 110
    assert!(a.usable(110.0, false));
    assert!(a.usable(110.0 + GRACE_SECONDS - 0.1, false));
    assert!(!a.usable(110.0 + GRACE_SECONDS, false));
}

#[test]
fn coming_back_resets_the_drop() {
    let mut a = Availability::default();
    a.observe(0.0, true);
    a.observe(10.0, false);
    assert_eq!(a.down_since(), Some(10.0));
    a.observe(50.0, true); // back after 40 s
    assert_eq!(a.down_since(), None);
}

// ---------------------------------------------------------------------------- pass verdict

#[test]
fn the_pass_verdict() {
    use PassVerdict::*;
    // (duration, covered in total, has changes)
    let cases = [
        (200.0, 200.0, true, Complete),
        (200.0, 190.0, true, Complete), // exactly 10 s missing
        (200.0, 189.9, true, Partial),  // 10.1 s missing
        (200.0, 190.1, true, Complete),
        (1000.0, 990.0, true, Complete), // 10 s, not 5 %, on a long song
        (1000.0, 950.0, true, Partial),  // 95 % no longer completes
        (200.0, 200.0, false, NoChanges), // a pass with no changes is nothing
        (200.0, 0.0, false, NoChanges),
        (200.0, 120.0, true, Partial), // stopped at 60 %
        (200.0, 0.0, true, Partial),   // a change at 0 s is still saved
        (200.0, 0.5, true, Partial),   // however short
        (200.0, 9.0, true, Partial),
        (8.0, 0.0, true, Partial), // short song: half is the floor
        (8.0, 4.0, true, Complete),
        (200.0, 250.0, true, Complete), // an overrun is clamped, not refused
        (0.0, 30.0, true, UnknownLength),
        (0.0, 0.0, false, UnknownLength),
    ];
    for (duration, covered, changes, want) in cases {
        assert_eq!(
            judge(duration, covered, changes),
            want,
            "{duration} {covered} {changes}"
        );
    }
}

fn covered(spans: &[(f64, f64)]) -> Coverage {
    let mut c = Coverage::new();
    for &(a, b) in spans {
        c.add(a, b);
    }
    c
}

fn verdict(duration: f64, spans: &[(f64, f64)]) -> PassVerdict {
    judge(duration, covered(spans).total(duration), true)
}

#[test]
fn coverage_is_the_union_of_the_recorded_stretches() {
    let mut c = Coverage::new();
    assert!(c.is_empty());
    c.add(10.0, 20.0);
    c.add(15.0, 30.0); // overlap merges
    c.add(30.0, 40.0); // touching merges
    c.add(50.0, 60.0);
    c.add(5.0, 5.0); // empty
    c.add(9.0, 3.0); // reversed
    c.add(f64::NAN, 9.0);
    assert_eq!(c.total(200.0), 40.0);
    c.add(-5.0, 2.0); // a negative start is cut to 0
    assert_eq!(c.total(200.0), 42.0);
    c.add(0.0, 100.0); // re-recording a stretch counts it once
    assert_eq!(c.total(200.0), 100.0);
    assert_eq!(c.total(80.0), 80.0); // past the end is not coverage
    assert_eq!(c.missing(200.0), 100.0);
    assert_eq!(c.missing(50.0), 0.0);
}

#[test]
fn gaps_anywhere_are_fine_when_at_most_ten_seconds_in_total_are_missing() {
    use PassVerdict::*;
    let d = 200.0;
    assert_eq!(verdict(d, &[(0.0, 200.0)]), Complete); // no gaps
    assert_eq!(
        verdict(d, &[(0.0, 60.0), (64.0, 120.0), (125.0, 200.0)]),
        Complete
    ); // 4 s + 5 s
    assert_eq!(verdict(d, &[(0.0, 100.0), (110.0, 200.0)]), Complete); // exactly 10 s
    assert_eq!(verdict(d, &[(0.0, 100.0), (110.1, 200.0)]), Partial); // 10.1 s
    assert_eq!(verdict(d, &[(0.0, 50.0), (80.0, 200.0)]), Partial); // seek forward 30 s
    assert_eq!(verdict(d, &[(0.0, 50.0), (58.0, 200.0)]), Complete); // seek forward 8 s
                                                                     // seek back 20 s at 120 and record on to the end: the union counts once
    assert_eq!(verdict(d, &[(0.0, 120.0), (100.0, 200.0)]), Complete);
    assert_eq!(covered(&[(0.0, 120.0), (100.0, 200.0)]).total(d), 200.0);
    // a link drop of 5, 20 and 90 s mid-song: the pass goes on and is judged by the total
    assert_eq!(verdict(d, &[(0.0, 80.0), (85.0, 200.0)]), Complete);
    assert_eq!(verdict(d, &[(0.0, 80.0), (100.0, 200.0)]), Partial);
    assert_eq!(verdict(d, &[(0.0, 80.0), (170.0, 200.0)]), Partial);
    assert_eq!(verdict(300.0, &[(0.0, 80.0), (170.0, 300.0)]), Partial); // 90 s of 300
                                                                         // a late start of 6 s and 14 s
    assert_eq!(verdict(d, &[(6.0, 200.0)]), Complete);
    assert_eq!(verdict(d, &[(14.0, 200.0)]), Partial);
    // unknown length is unchanged
    assert_eq!(judge(0.0, 30.0, true), UnknownLength);
}

// ---------------------------------------------------------------- which capture a song keeps

fn rows() -> Vec<OscLightingCue> {
    vec![row(OscCueSource::Captured)]
}

fn saved(result: CaptureSave) -> SongLightingCueMap {
    match result {
        CaptureSave::Saved(map) => map,
        other => panic!("expected a save, got {other:?}"),
    }
}

#[test]
fn a_complete_pass_replaces_captured_rows_and_keeps_manual_ones() {
    let existing = map(
        vec![row(OscCueSource::Captured), row(OscCueSource::Manual)],
        Some(status(false, 30.0)),
    );
    let out = saved(save_capture(
        Some(&existing),
        "s",
        "S",
        rows(),
        status(true, 200.0),
    ));
    assert_eq!(out.capture.as_ref().map(|c| c.complete), Some(true));
    assert_eq!(
        out.osc_cues
            .iter()
            .filter(|r| r.source == OscCueSource::Manual)
            .count(),
        1
    );
}

#[test]
fn a_complete_capture_survives_a_later_partial() {
    let existing = map(rows(), Some(status(true, 200.0)));
    assert!(matches!(
        save_capture(Some(&existing), "s", "S", rows(), status(false, 150.0)),
        CaptureSave::Kept
    ));
}

#[test]
fn a_complete_pass_replaces_an_older_complete_one() {
    let existing = map(rows(), Some(status(true, 190.0)));
    let out = saved(save_capture(
        Some(&existing),
        "s",
        "S",
        rows(),
        status(true, 200.0),
    ));
    assert_eq!(out.capture.unwrap().coverage_seconds, 200.0);
}

#[test]
fn a_partial_replaces_a_partial_only_if_it_covers_as_much() {
    let existing = map(rows(), Some(status(false, 100.0)));
    assert!(matches!(
        save_capture(Some(&existing), "s", "S", rows(), status(false, 99.0)),
        CaptureSave::Kept
    ));
    let equal = saved(save_capture(
        Some(&existing),
        "s",
        "S",
        rows(),
        status(false, 100.0),
    ));
    assert_eq!(equal.capture.unwrap().coverage_seconds, 100.0);
    let more = saved(save_capture(
        Some(&existing),
        "s",
        "S",
        rows(),
        status(false, 120.0),
    ));
    assert_eq!(more.capture.unwrap().coverage_seconds, 120.0);
}

#[test]
fn captured_rows_without_a_status_are_replaced_by_any_pass() {
    let existing = map(rows(), None);
    let out = saved(save_capture(
        Some(&existing),
        "s",
        "S",
        rows(),
        status(false, 25.0),
    ));
    assert_eq!(out.capture.unwrap().coverage_seconds, 25.0);
}

#[test]
fn a_partial_on_a_manual_only_song_is_saved() {
    let existing = map(vec![row(OscCueSource::Manual)], None);
    let out = saved(save_capture(
        Some(&existing),
        "s",
        "S",
        rows(),
        status(false, 25.0),
    ));
    assert_eq!(out.capture_label(), CaptureLabel::Incomplete);
}
