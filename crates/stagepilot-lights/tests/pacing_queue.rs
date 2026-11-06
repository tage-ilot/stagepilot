//! The pacing queue of the smooth replay: steps jump ahead of ramps and effects, a waiting ramp
//! or effect value is replaced by a newer one for the same address, order inside a class is
//! first in first out, and the 5 ms spacing (200 packets a second) holds. Fake clock only.

use stagepilot_core::config::{OscArgument, OscMessage};
use stagepilot_lights::lightkey_replay::{Kind, PacingQueue, PACKET_GAP_SECONDS};

fn msg(address: &str, value: f64) -> OscMessage {
    OscMessage {
        address: address.into(),
        args: vec![OscArgument::Float(value)],
    }
}

fn addr(i: usize) -> String {
    format!("/fixture/F{i}/overrides/dimmer")
}

fn value(m: &OscMessage) -> f64 {
    match m.args[0] {
        OscArgument::Float(x) => x,
        _ => panic!("float expected"),
    }
}

/// Drain on a fake clock from `from`, one packet per slot; returns (time, message, kind).
fn drain(q: &mut PacingQueue, from: f64) -> Vec<(f64, OscMessage, Kind)> {
    let mut out = Vec::new();
    let mut now = from;
    while !q.is_empty() {
        now = now.max(q.next_slot(now));
        let item = q.pop_due(now).expect("due at its slot");
        out.push((now, item.message, item.kind));
    }
    out
}

#[test]
fn step_behind_100_effect_values_goes_out_within_the_bound() {
    let mut q = PacingQueue::new();
    for i in 0..100 {
        q.push(msg(&addr(i), 0.5), Kind::Effect, 0.0, false);
    }
    q.push(
        msg("/fixture/STEP/overrides/dimmer", 1.0),
        Kind::Step,
        0.0,
        false,
    );
    let sent = drain(&mut q, 0.0);
    assert_eq!(sent.len(), 101);
    assert_eq!(sent[0].2, Kind::Step, "the step leaves first");
    assert!(sent[0].0 <= 0.005, "step waited {} s", sent[0].0);
}

#[test]
fn step_arriving_while_effects_are_draining_waits_at_most_one_slot() {
    let mut q = PacingQueue::new();
    for i in 0..100 {
        q.push(msg(&addr(i), 0.5), Kind::Effect, 0.0, false);
    }
    // 20 packets leave, then a step arrives at t = 0.1 s.
    let mut now: f64 = 0.0;
    for _ in 0..20 {
        now = now.max(q.next_slot(now));
        q.pop_due(now).unwrap();
    }
    let arrive = now + 0.001;
    q.push(
        msg("/fixture/STEP/overrides/dimmer", 1.0),
        Kind::Step,
        arrive,
        false,
    );
    let slot = q.next_slot(arrive);
    assert!(slot - arrive <= PACKET_GAP_SECONDS + 1e-9);
    let first = q.pop_due(slot).unwrap();
    assert_eq!(first.kind, Kind::Step);
}

#[test]
fn priority_is_steps_then_ramps_then_effects() {
    let mut q = PacingQueue::new();
    q.push(
        msg("/fixture/E/overrides/dimmer", 0.1),
        Kind::Effect,
        0.0,
        false,
    );
    q.push(
        msg("/fixture/R/overrides/dimmer", 0.2),
        Kind::Ramp,
        0.0,
        false,
    );
    q.push(
        msg("/fixture/S/overrides/dimmer", 0.3),
        Kind::Step,
        0.0,
        false,
    );
    let kinds: Vec<Kind> = drain(&mut q, 0.0).iter().map(|s| s.2).collect();
    assert_eq!(kinds, vec![Kind::Step, Kind::Ramp, Kind::Effect]);
}

#[test]
fn stale_ramp_value_is_dropped_when_a_newer_one_arrives() {
    let mut q = PacingQueue::new();
    q.push(msg(&addr(1), 0.10), Kind::Ramp, 0.0, false);
    q.push(msg(&addr(2), 0.20), Kind::Ramp, 0.0, false);
    q.push(msg(&addr(1), 0.15), Kind::Ramp, 0.05, false);
    assert_eq!(q.len(), 2);
    let sent = drain(&mut q, 0.05);
    let first: Vec<f64> = sent
        .iter()
        .filter(|s| s.1.address == addr(1))
        .map(|s| value(&s.1))
        .collect();
    assert_eq!(
        first,
        vec![0.15],
        "only the newest value of the address is sent"
    );
}

#[test]
fn stale_effect_value_is_replaced_by_a_newer_ramp_or_step_for_the_address() {
    let mut q = PacingQueue::new();
    q.push(msg(&addr(1), 0.1), Kind::Effect, 0.0, false);
    q.push(msg(&addr(1), 0.9), Kind::Step, 0.01, false);
    q.push(msg(&addr(2), 0.1), Kind::Effect, 0.0, false);
    q.push(msg(&addr(2), 0.8), Kind::Ramp, 0.01, false);
    let sent = drain(&mut q, 0.01);
    assert_eq!(sent.len(), 2);
    assert_eq!(value(&sent[0].1), 0.9);
    assert_eq!(value(&sent[1].1), 0.8);
}

#[test]
fn two_steps_for_one_address_are_both_sent_in_order() {
    let mut q = PacingQueue::new();
    q.push(msg(&addr(1), 0.0), Kind::Step, 0.0, false);
    q.push(msg(&addr(1), 1.0), Kind::Step, 0.0, false);
    let values: Vec<f64> = drain(&mut q, 0.0).iter().map(|s| value(&s.1)).collect();
    assert_eq!(values, vec![0.0, 1.0]);
}

#[test]
fn a_step_is_never_followed_by_an_older_ramp_value_of_its_address() {
    let mut q = PacingQueue::new();
    q.push(msg(&addr(1), 0.4), Kind::Ramp, 0.0, false);
    q.push(msg(&addr(1), 1.0), Kind::Step, 0.01, false);
    let values: Vec<f64> = drain(&mut q, 0.01).iter().map(|s| value(&s.1)).collect();
    assert_eq!(values, vec![1.0]);
}

#[test]
fn order_inside_a_class_is_first_in_first_out() {
    let mut q = PacingQueue::new();
    for i in 0..30 {
        q.push(msg(&addr(i), i as f64), Kind::Step, 0.0, false);
    }
    for i in 30..60 {
        q.push(msg(&addr(i), i as f64), Kind::Effect, 0.0, false);
    }
    let order: Vec<usize> = drain(&mut q, 0.0)
        .iter()
        .map(|s| value(&s.1) as usize)
        .collect();
    assert_eq!(order, (0..60).collect::<Vec<_>>());
}

#[test]
fn a_replaced_value_keeps_the_place_of_the_one_it_replaces() {
    let mut q = PacingQueue::new();
    q.push(msg(&addr(1), 1.0), Kind::Effect, 0.0, false);
    q.push(msg(&addr(2), 2.0), Kind::Effect, 0.0, false);
    q.push(msg(&addr(1), 3.0), Kind::Effect, 0.05, false);
    let order: Vec<f64> = drain(&mut q, 0.05).iter().map(|s| value(&s.1)).collect();
    assert_eq!(order, vec![3.0, 2.0]);
}

#[test]
fn the_cap_of_200_packets_a_second_holds() {
    let mut q = PacingQueue::new();
    for i in 0..1000 {
        q.push(msg(&addr(i), 0.5), Kind::Effect, 0.0, false);
    }
    let sent = drain(&mut q, 0.0);
    assert_eq!(sent.len(), 1000);
    for pair in sent.windows(2) {
        assert!(
            pair[1].0 - pair[0].0 >= PACKET_GAP_SECONDS - 1e-9,
            "packets {} and {} closer than 5 ms",
            pair[0].0,
            pair[1].0
        );
    }
    let in_first_second = sent.iter().filter(|s| s.0 < 1.0).count();
    assert!(in_first_second <= 200, "{in_first_second} packets in 1 s");
}

#[test]
fn nothing_is_due_before_its_slot() {
    let mut q = PacingQueue::new();
    q.push(msg(&addr(1), 0.0), Kind::Step, 0.0, false);
    q.push(msg(&addr(2), 0.0), Kind::Step, 0.0, false);
    assert!(q.pop_due(0.0).is_some());
    assert!(q.pop_due(0.001).is_none());
    assert!(q.pop_due(0.005).is_some());
}

#[test]
fn ramps_keep_a_quarter_of_the_slots_while_steps_drain() {
    use stagepilot_lights::lightkey_replay::RAMP_SHARE_EVERY;
    let mut q = PacingQueue::new();
    for i in 0..60 {
        q.push(msg(&addr(i), 1.0), Kind::Step, 0.0, false);
    }
    for i in 100..110 {
        q.push(msg(&addr(i), 0.5), Kind::Ramp, 0.0, false);
    }
    let sent = drain(&mut q, 0.0);
    let kinds: Vec<Kind> = sent.iter().map(|s| s.2).collect();
    // every RAMP_SHARE_EVERY-th slot is a ramp value while there are ramp values waiting
    for (n, k) in kinds
        .iter()
        .take(10 * RAMP_SHARE_EVERY as usize)
        .enumerate()
    {
        assert_eq!(
            *k == Kind::Ramp,
            (n + 1) % RAMP_SHARE_EVERY as usize == 0,
            "slot {n}"
        );
    }
    // the steps still leave in order and all of them go
    let steps: Vec<usize> = sent
        .iter()
        .filter(|s| s.2 == Kind::Step)
        .map(|s| value(&s.1) as usize)
        .collect();
    assert_eq!(steps.len(), 60);
    // the first step is not held up by a ramp
    assert_eq!(kinds[0], Kind::Step);
}

#[test]
fn waiting_messages_can_be_dropped_without_taking_a_slot() {
    let mut q = PacingQueue::new();
    for i in 0..10 {
        q.push(msg(&addr(i), i as f64), Kind::Step, 0.0, false);
    }
    assert_eq!(q.drop_where(&|m| m.address == addr(3)), 1);
    let sent = drain(&mut q, 0.0);
    assert_eq!(sent.len(), 9);
    // no hole where it was
    assert!((sent[8].0 - 8.0 * PACKET_GAP_SECONDS).abs() < 1e-9);
}
