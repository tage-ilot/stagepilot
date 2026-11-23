//! The pacing queue of the smooth replay: steps jump ahead of ramps and effects, a waiting ramp
//! or effect value is replaced by a newer one for the same address, order inside a class is
//! first in first out. What a tick has due leaves in OSC bundles of at most 16 messages, planned
//! by `plan_bundles` (order kept, never two values of one address in a bundle).

use stagepilot_core::config::{OscArgument, OscMessage};
use stagepilot_lights::lightkey_replay::{plan_bundles, Kind, PacingQueue};

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

/// Drain in sending order; returns (0, message, kind).
fn drain(q: &mut PacingQueue, from: f64) -> Vec<(f64, OscMessage, Kind)> {
    let mut out = Vec::new();
    while let Some(item) = q.pop() {
        out.push((from, item.message, item.kind));
    }
    out
}

#[test]
fn step_behind_100_effect_values_goes_out_first() {
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
}

#[test]
fn step_arriving_while_effects_are_waiting_goes_next() {
    let mut q = PacingQueue::new();
    for i in 0..100 {
        q.push(msg(&addr(i), 0.5), Kind::Effect, 0.0, false);
    }
    // 20 messages leave, then a step arrives.
    for _ in 0..20 {
        q.pop().unwrap();
    }
    q.push(
        msg("/fixture/STEP/overrides/dimmer", 1.0),
        Kind::Step,
        0.1,
        false,
    );
    let first = q.pop().unwrap();
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
fn ramps_keep_a_quarter_of_the_places_while_steps_drain() {
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
    // every RAMP_SHARE_EVERY-th place is a ramp value while there are ramp values waiting
    for (n, k) in kinds
        .iter()
        .take(10 * RAMP_SHARE_EVERY as usize)
        .enumerate()
    {
        assert_eq!(
            *k == Kind::Ramp,
            (n + 1) % RAMP_SHARE_EVERY as usize == 0,
            "place {n}"
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
fn waiting_messages_can_be_dropped() {
    let mut q = PacingQueue::new();
    for i in 0..10 {
        q.push(msg(&addr(i), i as f64), Kind::Step, 0.0, false);
    }
    assert_eq!(q.drop_where(&|m| m.address == addr(3)), 1);
    let sent = drain(&mut q, 0.0);
    assert_eq!(sent.len(), 9);
    assert!(sent.iter().all(|s| s.1.address != addr(3)));
}

fn addrs(bundles: &[Vec<OscMessage>]) -> Vec<Vec<String>> {
    bundles
        .iter()
        .map(|b| b.iter().map(|m| m.address.clone()).collect())
        .collect()
}

fn plan(messages: Vec<OscMessage>) -> Vec<Vec<OscMessage>> {
    plan_bundles(messages, |m| &m.address, 16)
}

#[test]
fn bundles_hold_at_most_16_messages_and_keep_the_order() {
    let all: Vec<OscMessage> = (0..100)
        .map(|i| msg(&format!("/fixture/F{i}/overrides/dimmer"), i as f64))
        .collect();
    let bundles = plan(all.clone());
    assert_eq!(bundles.len(), 7, "100 messages = 6 full bundles and 4");
    assert!(bundles.iter().all(|b| !b.is_empty() && b.len() <= 16));
    let flat: Vec<OscMessage> = bundles.into_iter().flatten().collect();
    assert_eq!(flat, all);
}

#[test]
fn a_bundle_never_holds_two_values_of_one_address() {
    let bundles = plan(vec![
        msg(&addr(1), 0.0),
        msg(&addr(2), 0.0),
        msg(&addr(1), 1.0),
        msg(&addr(3), 1.0),
    ]);
    assert_eq!(
        addrs(&bundles),
        vec![vec![addr(1), addr(2)], vec![addr(1), addr(3)]]
    );
    assert_eq!(
        value(&bundles[1][0]),
        1.0,
        "the later value is in the later bundle"
    );
}

#[test]
fn one_fixtures_messages_are_not_split_when_they_fit() {
    // 15 single messages, then a fixture with 3 properties: it starts the next bundle.
    let mut all: Vec<OscMessage> = (0..15).map(|i| msg(&addr(i), 0.0)).collect();
    for property in ["dimmer", "red", "green"] {
        all.push(msg(&format!("/fixture/WASH/overrides/{property}"), 1.0));
    }
    let bundles = plan(all);
    assert_eq!(bundles.len(), 2);
    assert_eq!(bundles[0].len(), 15);
    assert_eq!(bundles[1].len(), 3);
    assert!(bundles[1]
        .iter()
        .all(|m| m.address.starts_with("/fixture/WASH/")));
}

#[test]
fn a_lone_message_is_a_bundle_of_one_and_nothing_gives_nothing() {
    assert_eq!(plan(vec![msg(&addr(1), 0.0)]).len(), 1);
    assert!(plan(Vec::new()).is_empty());
}
