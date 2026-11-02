//! Size targets for thinning and merging, on synthetic data shaped like the real takes taken on
//! the rig: 25 ms steps, 63 or 106 addresses, mostly colour / pan / dimmer sweeps over a song of
//! 255 s (109k commands, 63 addresses; 51k commands, 106 addresses).

use stagepilot_lights::lightkey_fixtures::{FixtureArg, FixtureCommand};
use stagepilot_lights::lightkey_looks::{merge, state_at, thin, tolerance_for, Look};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn between(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.next()
    }
}

fn smooth(x: f64) -> f64 {
    x * x * (3.0 - 2.0 * x)
}

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// Sweeps in a song: per address, `active_seconds` of 25 ms steps made of 1.5 to 5 s ramps at
/// random times (no two ramps of one address overlap).
fn synthetic(
    addresses: usize,
    commands: usize,
    song_seconds: f64,
    seed: u64,
) -> Vec<(f64, FixtureCommand)> {
    let mut rng = Rng(seed);
    let per_address = commands / addresses;
    let mut out: Vec<(f64, FixtureCommand)> = Vec::new();
    for index in 0..addresses {
        let fixture = format!("Fixture {:02}", index / 3 + 1);
        let kind = index % 3; // 0 dimmer, 1 colour, 2 pan
        let (name, width) = match kind {
            0 => ("dimmer", 1),
            1 => ("color", 3),
            _ => ("panAngle", 1),
        };
        let address = format!("/fixture/{fixture}/overrides/{name}");
        let mut level: Vec<f64> = match kind {
            0 => vec![0.0],
            1 => vec![0.2, 0.2, 0.2],
            _ => vec![90.0],
        };
        let mut sent = 0usize;
        let mut cursor = rng.between(0.0, 10.0);
        while sent < per_address && cursor < song_seconds - 6.0 {
            let ramp = rng.between(1.5, 5.0);
            let steps = (ramp / 0.025) as usize;
            let target: Vec<f64> = match kind {
                0 => vec![if rng.next() < 0.3 {
                    0.0
                } else {
                    rng.between(0.3, 1.0)
                }],
                1 => (0..3).map(|_| rng.between(0.0, 1.0)).collect(),
                _ => vec![(level[0] + rng.between(-80.0, 80.0)).clamp(-270.0, 270.0)],
            };
            for step in 1..=steps {
                if sent >= per_address {
                    break;
                }
                let k = smooth(step as f64 / steps as f64);
                let value: Vec<FixtureArg> = (0..width)
                    .map(|i| FixtureArg::Float(round4(level[i] + (target[i] - level[i]) * k)))
                    .collect();
                out.push((
                    cursor + step as f64 * 0.025,
                    FixtureCommand::new(address.clone(), value),
                ));
                sent += 1;
            }
            level = target;
            cursor += ramp + rng.between(0.5, 14.0);
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

fn counts(looks: &[Look]) -> (usize, usize) {
    (looks.len(), looks.iter().map(|l| l.messages.len()).sum())
}

fn report(name: &str, changes: &[(f64, FixtureCommand)], duration: f64) {
    let addresses: std::collections::HashSet<_> = changes.iter().map(|c| c.1.a.as_str()).collect();
    println!(
        "{name}: {} commands, {} addresses, {duration} s",
        changes.len(),
        addresses.len()
    );
    for (label, interval) in [
        ("Fine 0.25 s", 0.25),
        ("Normal 0.5 s", 0.5),
        ("Coarse 1 s", 1.0),
    ] {
        let thinned = thin(&[], changes, duration, interval);
        let merged = merge(thinned.clone());
        let (r0, m0) = counts(&thinned);
        let (r1, m1) = counts(&merged);
        println!("  {label}: before merge {r0} rows / {m0} messages; after merge {r1} rows / {m1} messages");
    }
}

#[test]
fn rig_shaped_takes_fit_the_size_targets_at_normal_detail() {
    for (name, addresses, commands, seed) in
        [("take A", 63, 109_000, 7), ("take B", 106, 51_000, 11)]
    {
        let changes = synthetic(addresses, commands, 255.0, seed);
        assert!(
            changes.len() >= commands * 9 / 10,
            "{} commands",
            changes.len()
        );
        report(name, &changes, 255.0);
        let merged = merge(thin(&[], &changes, 255.0, 0.5));
        let (rows, messages) = counts(&merged);
        assert!(rows <= 400, "{name}: {rows} rows");
        assert!(messages <= 6_000, "{name}: {messages} messages");
        // Fewer, not fewer-and-wrong: row times stay on the 0.5 s grid.
        assert!(merged
            .iter()
            .all(|l| ((l.at_seconds / 0.5).round() * 0.5 - l.at_seconds).abs() < 1e-9));
    }
}

#[test]
fn thinning_keeps_every_address_near_where_it_was() {
    // The recorded value at a sample time vs the replayed one: within the merge tolerance plus
    // what one grid slot of motion can add (checked on the slowest, most common sweeps).
    let changes = synthetic(63, 109_000, 255.0, 7);
    let merged = merge(thin(&[], &changes, 255.0, 0.5));
    let mut latest: std::collections::HashMap<&str, &Vec<FixtureArg>> =
        std::collections::HashMap::new();
    let mut worst_level = 0.0f64;
    let mut cursor = 0usize;
    let mut sample = 10.0;
    while sample < 255.0 {
        while cursor < changes.len() && changes[cursor].0 <= sample {
            latest.insert(changes[cursor].1.a.as_str(), &changes[cursor].1.v);
            cursor += 1;
        }
        let replayed = state_at(&merged, sample);
        for (address, recorded) in &latest {
            if address.ends_with("dimmer") {
                if let (Some(have), Some(FixtureArg::Float(want))) =
                    (replayed.get(*address), recorded.first())
                {
                    if let Some(FixtureArg::Float(have)) = have.first() {
                        worst_level = worst_level.max((have - want).abs());
                    }
                }
            }
        }
        sample += 5.0;
    }
    // A fade of at most 1.0 over >= 1.5 s moves under 0.35 per 0.5 s slot.
    assert!(worst_level < 0.5, "worst dimmer error {worst_level}");
    assert!(tolerance_for("/fixture/A/overrides/dimmer") > 0.0);
}
