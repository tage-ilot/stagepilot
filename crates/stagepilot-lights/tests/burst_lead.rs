//! Step bursts start before their row so that a big look lands on the beat: the middle of the
//! burst is on the row time, the important values go first, bursts keep their order, nothing is
//! sent for the past or before position 0, an operator take-over skips a value without delaying
//! the others, and the ramps next to a burst are not starved. Fake clock only.

use stagepilot_core::config::{OscArgument, OscCueSource, OscLightingCue, OscMessage};
use stagepilot_lights::lightkey_replay::{
    simulate, Kind, ReplayEngine, SentPacket, FAST_TICK_SECONDS, PACKET_GAP_SECONDS,
    STEP_LEAD_MAX_MS,
};
use uuid::Uuid;

const LEAD: f64 = 0.045;

fn dim(i: usize) -> String {
    format!("/fixture/F{i:03}/overrides/dimmer")
}

fn msg(address: &str, value: f64) -> OscMessage {
    OscMessage {
        address: address.into(),
        args: vec![OscArgument::Float(value)],
    }
}

fn row(at: f64, fade: f64, messages: Vec<OscMessage>) -> OscLightingCue {
    OscLightingCue {
        id: Uuid::new_v4(),
        at_seconds: at,
        address: "/stagepilot/look".into(),
        args: vec![],
        label: String::new(),
        color: None,
        messages,
        source: OscCueSource::Captured,
        edited: false,
        fade_seconds: fade,
    }
}

/// A row that sets the dimmers of fixtures `0..n` to `value`.
fn look(at: f64, n: usize, value: f64) -> OscLightingCue {
    row(at, 0.0, (0..n).map(|i| msg(&dim(i), value)).collect())
}

/// A ramp over the dimmer of `ramp_fixture` from `from` to `to` that starts at `at`.
fn ramp(at: f64, seconds: f64, address: &str, to: f64) -> OscLightingCue {
    row(at, seconds, vec![msg(address, to)])
}

fn no_one(_: f64, _: &str) -> bool {
    false
}

fn run(rows: &[OscLightingCue], from: f64, to: f64) -> Vec<SentPacket> {
    let mut engine = ReplayEngine::new(rows, &[]);
    simulate(&mut engine, from, to, LEAD, &no_one)
}

fn steps_at_row(packets: &[SentPacket], row_time: f64) -> Vec<&SentPacket> {
    packets
        .iter()
        .filter(|p| p.kind == Kind::Step && (p.position - row_time).abs() < 1e-3)
        .collect()
}

fn value(p: &SentPacket) -> f64 {
    match p.message.args[0] {
        OscArgument::Float(x) => x,
        _ => panic!("float expected"),
    }
}

/// Lateness of a packet against the time its row's step is meant to leave (row time minus the
/// look lead), seconds; negative = early.
fn lateness(p: &SentPacket, row_time: f64) -> f64 {
    p.at - (row_time - LEAD)
}

#[test]
fn a_98_value_burst_lands_centred_on_the_row_time() {
    let rows = vec![look(0.0, 98, 0.0), look(2.0, 98, 1.0)];
    let packets = run(&rows, 0.0, 4.0);
    let burst = steps_at_row(&packets, 2.0);
    assert_eq!(burst.len(), 98);
    let late: Vec<f64> = burst.iter().map(|p| lateness(p, 2.0)).collect();
    let (first, last) = (late[0], late[97]);
    // spread over (98 - 1) x 5 ms = 485 ms, the middle on the row time
    assert!(
        (last - first - 0.485).abs() < 0.002,
        "span {}",
        last - first
    );
    assert!(
        ((first + last) / 2.0).abs() < 0.003,
        "centre {} s off",
        (first + last) / 2.0
    );
    // 242 ms early at most, 243 late at most (half of the span each side)
    assert!((-0.2445..=-0.2405).contains(&first), "{first}");
    assert!(last <= 0.2425 + 0.002);
    // spacing never below 5 ms
    for pair in burst.windows(2) {
        assert!(pair[1].at - pair[0].at >= PACKET_GAP_SECONDS - 1e-9);
    }
}

#[test]
fn the_lead_is_capped_at_step_lead_max() {
    // 300 values: (300 - 1) x 5 ms / 2 = 747 ms would be the centred lead
    let rows = vec![look(0.0, 300, 0.0), look(5.0, 300, 1.0)];
    let packets = run(&rows, 0.0, 9.0);
    let burst = steps_at_row(&packets, 5.0);
    assert_eq!(burst.len(), 300);
    let earliest = burst
        .iter()
        .map(|p| lateness(p, 5.0))
        .fold(f64::INFINITY, f64::min);
    assert!(
        earliest >= -STEP_LEAD_MAX_MS / 1000.0 - 0.002,
        "starts {earliest} s before the row"
    );
    assert!(earliest <= -STEP_LEAD_MAX_MS / 1000.0 + 0.002, "{earliest}");
}

#[test]
fn the_lead_can_be_turned_off_and_changed_per_engine() {
    let rows = vec![look(0.0, 98, 0.0), look(2.0, 98, 1.0)];
    let mut off = ReplayEngine::new(&rows, &[]).with_step_lead_max_ms(0.0);
    let packets = simulate(&mut off, 0.0, 4.0, LEAD, &no_one);
    let first = steps_at_row(&packets, 2.0)
        .iter()
        .map(|p| lateness(p, 2.0))
        .fold(f64::INFINITY, f64::min);
    assert!(first >= -0.001, "no lead: first value at {first}");
    let mut short = ReplayEngine::new(&rows, &[]).with_step_lead_max_ms(100.0);
    let packets = simulate(&mut short, 0.0, 4.0, LEAD, &no_one);
    let first = steps_at_row(&packets, 2.0)
        .iter()
        .map(|p| lateness(p, 2.0))
        .fold(f64::INFINITY, f64::min);
    assert!((first + 0.1).abs() < 0.002, "100 ms lead: first at {first}");
}

#[test]
fn a_burst_of_up_to_40_ms_keeps_its_exact_timing_and_a_longer_one_is_moved() {
    // 9 values take 8 x 5 = 40 ms: exact. 10 values take 45 ms: moved.
    for (n, moved) in [(1usize, false), (5, false), (9, false), (10, true)] {
        let rows = vec![look(0.0, n, 0.0), look(2.0, n, 1.0)];
        let packets = run(&rows, 0.0, 4.0);
        let burst = steps_at_row(&packets, 2.0);
        assert_eq!(burst.len(), n);
        let first = lateness(burst[0], 2.0);
        if moved {
            assert!(first < -0.015, "{n} values: first value at {first}");
        } else {
            assert!(
                first.abs() < 0.001,
                "{n} values start on the row time, not at {first}"
            );
        }
    }
}

#[test]
fn the_small_burst_is_the_same_as_without_the_lead() {
    let rows = vec![look(0.0, 9, 0.0), look(2.0, 9, 1.0), look(3.0, 9, 0.5)];
    let with = run(&rows, 0.0, 5.0);
    let mut off = ReplayEngine::new(&rows, &[]).with_step_lead_max_ms(0.0);
    let without = simulate(&mut off, 0.0, 5.0, LEAD, &no_one);
    assert_eq!(with.len(), without.len());
    for (a, b) in with.iter().zip(&without) {
        assert!((a.at - b.at).abs() < 1e-9);
        assert_eq!(a.message, b.message);
    }
}

#[test]
fn dimmer_and_colour_go_before_other_levels_and_angles_in_a_fixed_order() {
    let mut messages = Vec::new();
    for i in 0..4 {
        messages.push(OscMessage {
            address: format!("/fixture/F{i}/overrides/pan"),
            args: vec![OscArgument::Float(10.0 + i as f64)],
        });
        messages.push(OscMessage {
            address: format!("/fixture/F{i}/overrides/zoom"),
            args: vec![OscArgument::Float(0.2)],
        });
        messages.push(OscMessage {
            address: format!("/fixture/F{i}/overrides/color"),
            args: vec![
                OscArgument::Float(0.1),
                OscArgument::Float(0.2),
                OscArgument::Float(0.3),
            ],
        });
        messages.push(msg(&dim(i), 1.0));
    }
    let base: Vec<OscMessage> = messages
        .iter()
        .map(|m| OscMessage {
            address: m.address.clone(),
            args: m.args.iter().map(|_| OscArgument::Float(0.0)).collect(),
        })
        .collect();
    let rows = vec![row(0.0, 0.0, base), row(2.0, 0.0, messages)];
    let packets = run(&rows, 0.0, 4.0);
    let burst = steps_at_row(&packets, 2.0);
    assert_eq!(burst.len(), 16);
    let class = |p: &SentPacket| {
        let a = &p.message.address;
        if a.ends_with("dimmer") {
            0
        } else if a.ends_with("color") {
            1
        } else if a.ends_with("zoom") {
            2
        } else {
            3
        }
    };
    let classes: Vec<i32> = burst.iter().map(|p| class(p)).collect();
    let mut sorted = classes.clone();
    sorted.sort();
    assert_eq!(classes, sorted, "dimmer, colour, other levels, then angles");
    // inside a class by address, so the order does not change between runs
    let again = run(&rows, 0.0, 4.0);
    let a: Vec<&str> = burst.iter().map(|p| p.message.address.as_str()).collect();
    let b: Vec<&str> = steps_at_row(&again, 2.0)
        .iter()
        .map(|p| p.message.address.as_str())
        .collect();
    assert_eq!(a, b);
}

#[test]
fn two_bursts_close_together_keep_their_order() {
    // 60 values each, 0.2 s apart: each takes 295 ms, so they cannot both be centred
    let rows = vec![
        look(0.0, 60, 0.0),
        look(2.0, 60, 1.0),
        look(2.2, 60, 0.5),
        look(2.4, 60, 0.25),
    ];
    let packets = run(&rows, 0.0, 5.0);
    let steps: Vec<&SentPacket> = packets
        .iter()
        .filter(|p| p.kind == Kind::Step && p.position > 1.0)
        .collect();
    assert_eq!(steps.len(), 180);
    // sent in row order: all values of row 1, then row 2, then row 3
    let rows_seen: Vec<f64> = steps.iter().map(|p| p.position).collect();
    let mut sorted = rows_seen.clone();
    sorted.sort_by(f64::total_cmp);
    assert_eq!(
        rows_seen, sorted,
        "a later row never overtakes an earlier one"
    );
    // 5 ms apart at least, all through
    for pair in steps.windows(2) {
        assert!(pair[1].at - pair[0].at >= PACKET_GAP_SECONDS - 1e-9);
    }
    // each fixture ends on the last row's value, and every value is sent at most as late as the
    // queue demands: the first row is centred, the later ones follow it
    let last_value = |i: usize| {
        steps
            .iter()
            .rev()
            .find(|p| p.message.address == dim(i))
            .map(|p| value(p))
    };
    for i in 0..60 {
        assert_eq!(last_value(i), Some(0.25));
    }
    let first_row: Vec<f64> = steps_at_row(&packets, 2.0)
        .iter()
        .map(|p| lateness(p, 2.0))
        .collect();
    let centre = (first_row[0] + first_row[59]) / 2.0;
    assert!(centre.abs() < 0.004, "the first burst is centred: {centre}");
}

#[test]
fn a_seek_or_resume_never_sends_values_for_rows_in_the_past() {
    let rows = vec![
        look(0.0, 60, 0.0),
        look(1.0, 60, 0.2),
        look(2.0, 60, 0.4),
        look(3.0, 60, 0.6),
    ];
    // resume at 1.95: the burst of row 3.0 may start early, the one of 2.0 would have started
    // at 1.85 and starts now instead
    let from = 1.95;
    let packets = run(&rows, from, 4.0);
    let first_position = from + LEAD;
    for p in &packets {
        assert!(
            p.position >= first_position - 1e-9,
            "a value for position {} was sent after resuming at {first_position}",
            p.position
        );
        assert!(p.at >= from - 1e-9);
    }
    // the first packets are the look at the resume position (row 1.0's values) then row 2.0's
    // burst; row 1.0 itself is not replayed as a burst
    let row2 = steps_at_row(&packets, 2.0);
    assert_eq!(row2.len(), 60);
    assert!(
        row2[0].at >= from - 1e-9,
        "starts no earlier than the resume"
    );
}

#[test]
fn a_position_that_goes_back_forgets_what_was_sent_early() {
    let rows = vec![look(0.0, 60, 0.0), look(2.0, 60, 1.0)];
    let mut engine = ReplayEngine::new(&rows, &[]);
    let none = |_: &str| false;
    // forward to just before the burst's start: the burst goes out early, fixtures held
    let mut now = 0.0;
    let mut sent_early = false;
    while now < 1.9 {
        let t = engine.tick(now, now + LEAD, &none);
        sent_early |= t.positions.iter().any(|p| (*p - 2.0).abs() < 1e-3);
        now += t.wake_in.unwrap_or(0.05).max(0.001);
    }
    assert!(sent_early, "the burst for 2.0 started before 1.9");
    // seek back to 0.5: the engine must send the values at 0.5 (0.0), not hold the burst's
    let t = engine.tick(now + 0.01, 0.5, &none);
    assert_eq!(t.messages.len(), 60, "the look at the new position");
    assert!(t
        .messages
        .iter()
        .all(|m| m.args == vec![OscArgument::Float(0.0)]));
    assert!(t.positions.iter().all(|p| (*p - 0.5).abs() < 1e-9));
}

#[test]
fn an_early_burst_never_starts_before_position_zero() {
    // the row is at 0.1 s: centred it would start 142 ms before the song
    let rows = vec![look(0.0, 3, 0.0), look(0.1, 98, 1.0)];
    let packets = run(&rows, 0.0, 2.0);
    assert!(packets.iter().all(|p| p.at >= 0.0));
    assert!(packets
        .iter()
        .all(|p| p.position >= LEAD - 1e-9 || p.position == 0.1));
    let burst = steps_at_row(&packets, 0.1);
    assert_eq!(burst.len(), 98);
    assert!(burst[0].at >= 0.0);
}

#[test]
fn a_fixture_taken_over_mid_burst_is_skipped_and_the_others_do_not_wait_for_it() {
    let rows = vec![look(0.0, 98, 0.0), look(2.0, 98, 1.0)];
    let free = run(&rows, 0.0, 4.0);
    // fixture 50 is taken over 100 ms before the row time: its value was still waiting
    let taken = dim(50);
    let mut engine = ReplayEngine::new(&rows, &[]);
    let with = simulate(&mut engine, 0.0, 4.0, LEAD, &|now, a| {
        now >= 1.9 && a == taken
    });
    let sent_after = |packets: &[SentPacket]| {
        packets
            .iter()
            .filter(|p| p.message.address == taken && p.at >= 1.9)
            .count()
    };
    assert!(sent_after(&free) > 0, "without the take-over it would go");
    assert_eq!(
        sent_after(&with),
        0,
        "not sent to a fixture the operator has"
    );
    // every other value goes out no later than without the take-over
    for p in free.iter().filter(|p| p.message.address != taken) {
        let q = with
            .iter()
            .find(|q| q.message == p.message && (q.position - p.position).abs() < 1e-9)
            .expect("still sent");
        assert!(q.at <= p.at + 1e-9, "delayed: {} vs {}", q.at, p.at);
    }
    // and the ones behind the skipped value move up
    let behind_free = free.iter().find(|p| p.message.address == dim(60)).unwrap();
    let behind = with.iter().find(|p| p.message.address == dim(60)).unwrap();
    assert!(behind.at <= behind_free.at + 1e-9);
}

#[test]
fn ramps_next_to_a_burst_are_not_starved() {
    // ten ramps run for 8 s while a 98 value look changes at 4 s
    let mut rows = vec![look(0.0, 98, 0.0)];
    let ramp_addresses: Vec<String> = (200..210).map(dim).collect();
    for a in &ramp_addresses {
        rows.push(row(0.0, 0.0, vec![msg(a, 0.0)]));
        rows.push(ramp(1.0, 8.0, a, 1.0));
    }
    rows.push(look(4.0, 98, 1.0));
    rows.sort_by(|a, b| a.at_seconds.total_cmp(&b.at_seconds));
    let with = run(&rows, 0.0, 10.0);
    let mut off = ReplayEngine::new(&rows, &[]).with_step_lead_max_ms(0.0);
    let without = simulate(&mut off, 0.0, 10.0, LEAD, &no_one);

    // longest time any running ramp goes without a new value, around the burst
    let longest_gap = |packets: &[SentPacket]| {
        let mut worst: f64 = 0.0;
        for a in &ramp_addresses {
            let times: Vec<f64> = packets
                .iter()
                .filter(|p| &p.message.address == a && p.kind == Kind::Ramp)
                .map(|p| p.at)
                .filter(|t| (3.3..5.0).contains(t))
                .collect();
            for w in times.windows(2) {
                worst = worst.max(w[1] - w[0]);
            }
        }
        worst
    };
    let (g_with, g_without) = (longest_gap(&with), longest_gap(&without));
    assert!(
        g_with <= g_without + 0.01,
        "a ramp went {g_with:.3} s without a value with the lead, {g_without:.3} s without"
    );
    assert!(
        g_with <= 0.5,
        "no ramp is starved for more than half a second: {g_with:.3}"
    );
    // the 200 packets a second cap holds with the lead on
    let busiest = |packets: &[SentPacket]| {
        (0..10)
            .map(|s| {
                packets
                    .iter()
                    .filter(|p| p.at >= s as f64 && p.at < s as f64 + 1.0)
                    .count()
            })
            .max()
            .unwrap()
    };
    assert!(busiest(&with) <= 200);
    let _ = FAST_TICK_SECONDS;
}

#[test]
fn a_dimmer_ramp_is_not_starved_by_fast_angle_ramps() {
    // 30 pans move 40 degrees in 2 s (about 1000 DMX steps a second each) next to a slow dimmer
    // ramp: the budget cannot carry all of them at 20 Hz. Ramps are served by their distance in
    // tolerances, so the dimmer (8 DMX steps of tolerance) is not left behind by the angles (184
    // DMX steps of tolerance): it gets a value every few hundred ms, not none
    let pan = |i: usize| format!("/fixture/P{i:02}/overrides/panAngle");
    let mut start: Vec<OscMessage> = (0..30).map(|i| msg(&pan(i), 0.0)).collect();
    start.push(msg(&dim(0), 0.0));
    let mut ramps: Vec<OscMessage> = (0..30).map(|i| msg(&pan(i), 40.0)).collect();
    ramps.push(msg(&dim(0), 0.3));
    let rows = vec![row(0.0, 0.0, start), row(1.0, 2.0, ramps)];
    let packets = run(&rows, 0.0, 5.0);
    let times: Vec<f64> = packets
        .iter()
        .filter(|p| p.message.address == dim(0) && p.kind == Kind::Ramp)
        .map(|p| p.at)
        .collect();
    let worst = times.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max);
    assert!(
        times.len() >= 4,
        "{} dimmer values at {times:?}",
        times.len()
    );
    assert!(worst < 0.5, "the dimmer waited {worst} s for a value");
}
