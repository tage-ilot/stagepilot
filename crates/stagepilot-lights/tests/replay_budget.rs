//! The replay's send budget on a very busy song (F-H1).
//!
//! `MAX_MESSAGES_PER_SECOND` (2,500) is the most the engine sends. On a song busier than that,
//! ramps (colour rows) used to take all of it and the loops (effects) got nothing. Now, once a
//! class has been starved for more than two seconds, the budget is shared: ramps at most 65%,
//! effects at least 35%, and every address is served in turn. A song that fits is not busy
//! (golden test with the two real songs of the 2026-11-08 service).

use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{
    EffectMember, EffectStep, LightingEffect, LightingMethod, LightkeySettings, LightsSettings,
    OscArgument, OscCueSource, OscLightingCue, OscMessage, SongLightingCueMap,
};
use stagepilot_core::events::{new_event, EventPayload, EventType, TimerPayload};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::Song;
use stagepilot_lights::lightkey_link::{ArmedStore, LinkOptions, MemoryArmedStore};
use stagepilot_lights::lightkey_replay::{
    simulate_with_load, Kind, LoadReport, ReplayEngine, ReplayLoad, SentPacket,
    MAX_MESSAGES_PER_SECOND,
};
use stagepilot_lights::{LightsOptions, LightsPlugin};
use stagepilot_osc::{OscBackend, OscOutput, OutputError};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

const LEAD: f64 = 0.045;

fn fixture_map(name: &str) -> SongLightingCueMap {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// FNV-1a over every packet: when it leaves, the song position it is for, why, where, what.
fn digest(packets: &[SentPacket]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in packets {
        let line = format!(
            "{:.6}|{:.6}|{:?}|{}|{:?}|{}\n",
            p.at, p.position, p.kind, p.message.address, p.message.args, p.reduced
        );
        for b in line.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

#[derive(Debug, PartialEq, Eq)]
struct Counts {
    total: usize,
    step: usize,
    ramp: usize,
    effect: usize,
    slowed: usize,
}

fn counts(packets: &[SentPacket]) -> Counts {
    let n = |k: Kind| packets.iter().filter(|p| p.kind == k).count();
    Counts {
        total: packets.len(),
        step: n(Kind::Step),
        ramp: n(Kind::Ramp),
        effect: n(Kind::Effect),
        slowed: packets.iter().filter(|p| p.reduced).count(),
    }
}

fn run(
    rows: &[OscLightingCue],
    effects: &[LightingEffect],
    from: f64,
    to: f64,
) -> (Vec<SentPacket>, LoadReport) {
    let mut engine = ReplayEngine::new(rows, effects);
    simulate_with_load(&mut engine, from, to, LEAD, &|_, _| false)
}

// ------------------------------------------------------------------------------ R4: no change

/// The two real songs of the 2026-11-08 service (trimmed copies of the saved cue maps: rows and
/// effects only), the whole song, no operator. The numbers are those of the 2,500 messages a
/// second budget and the 40 Hz tick (they were 11,075 and 14,822 messages at 200 a second and
/// 20 Hz, with 1,854 and 1,399 effect values slowed down; now none is slowed); the digest is of
/// every message in order, so a single message moved, added or dropped changes it.
#[test]
fn songs_that_fit_are_not_busy_and_play_as_pinned() {
    let cases = [
        (
            "real_song_ever_be.json",
            320.0,
            Counts {
                total: 23_922,
                step: 217,
                ramp: 5_606,
                effect: 18_099,
                slowed: 0,
            },
            0xb0dd_c3df_ccc7_f302_u64,
        ),
        (
            "real_song_elohim.json",
            450.0,
            Counts {
                total: 29_771,
                step: 665,
                ramp: 14_978,
                effect: 14_128,
                slowed: 0,
            },
            0x809f_f42b_a4ec_a728_u64,
        ),
    ];
    for (file, seconds, want, want_digest) in cases {
        let map = fixture_map(file);
        let (packets, load) = run(&map.osc_cues, &map.effects, 0.0, seconds);
        assert_eq!(counts(&packets), want, "{file}");
        assert_eq!(digest(&packets), want_digest, "{file}: the packet order");
        assert_eq!(load.busy_seconds, 0.0, "{file} is never shown as busy");
        assert!(!load.last.busy, "{file}");
        println!(
            "{file}: {:?}, digest {:016x}, lowest served {}%",
            counts(&packets),
            digest(&packets),
            load.min_served_percent
        );
    }
}

// ------------------------------------------------------------------------- the busy song

fn msg(address: String, args: Vec<f64>) -> OscMessage {
    OscMessage {
        address,
        args: args.into_iter().map(OscArgument::Float).collect(),
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

fn loop_effect(
    period: f64,
    end: f64,
    members: impl Iterator<Item = (String, f64)>,
) -> LightingEffect {
    LightingEffect {
        id: Uuid::new_v4(),
        label: "loop".into(),
        start_seconds: 0.0,
        end_seconds: end,
        period_seconds: period,
        members: members
            .map(|(fixture, offset_seconds)| EffectMember {
                fixture,
                offset_seconds,
            })
            .collect(),
        cycle: vec![
            EffectStep {
                at_seconds: 0.0,
                property: "dimmer".into(),
                value: vec![1.0],
                fade_seconds: period / 2.0,
            },
            EffectStep {
                at_seconds: period / 2.0,
                property: "dimmer".into(),
                value: vec![0.0],
                fade_seconds: period / 2.0,
            },
        ],
        source: OscCueSource::Captured,
        edited: false,
    }
}

struct Busy {
    rows: Vec<OscLightingCue>,
    effects: Vec<LightingEffect>,
    ramp_addresses: Vec<String>,
    loop_addresses: Vec<String>,
}

/// A song busier than the budget: `ramp_fixtures` colour ramps changing every 100 ms,
/// `loop_fixtures` loops with periods of 3, 5, 9 and 15 s, and a first look of 66 fixtures.
fn busy_song(ramp_fixtures: usize, loop_fixtures: usize, seconds: f64) -> Busy {
    let mut rows = Vec::new();
    // The first look: 66 fixtures.
    rows.push(row(
        0.0,
        0.0,
        (1..=66)
            .map(|n| msg(format!("/fixture/F{n}/overrides/dimmer"), vec![0.5]))
            .collect(),
    ));
    // Colour ramps: every 100 ms every fixture gets a new colour to fade to.
    if ramp_fixtures > 0 {
        let steps = (seconds * 10.0) as usize;
        for k in 0..steps {
            let messages = (1..=ramp_fixtures)
                .map(|n| {
                    let phase = k as f64 * 0.9 + n as f64 * 0.7;
                    msg(
                        format!("/fixture/P{n}/overrides/color"),
                        vec![
                            0.5 + 0.5 * phase.sin(),
                            0.5 + 0.5 * (phase * 1.3).cos(),
                            0.5 + 0.5 * (phase * 0.7).sin(),
                        ],
                    )
                })
                .collect();
            rows.push(row(k as f64 * 0.1, 0.1, messages));
        }
    }
    // Loops at 3, 5, 9 and 15 s, the fixtures dealt out over them.
    let periods = [3.0, 5.0, 9.0, 15.0];
    let mut effects = Vec::new();
    if loop_fixtures > 0 {
        for (i, period) in periods.iter().enumerate() {
            let mine: Vec<usize> = (1..=loop_fixtures)
                .filter(|n| (n - 1) % periods.len() == i)
                .collect();
            let count = mine.len() as f64;
            effects.push(loop_effect(
                *period,
                seconds + 10.0,
                mine.iter()
                    .enumerate()
                    .map(|(j, n)| (format!("L{n}"), period * j as f64 / count)),
            ));
        }
    }
    Busy {
        rows,
        effects,
        ramp_addresses: (1..=ramp_fixtures)
            .map(|n| format!("/fixture/P{n}/overrides/color"))
            .collect(),
        loop_addresses: (1..=loop_fixtures)
            .map(|n| format!("/fixture/L{n}/overrides/dimmer"))
            .collect(),
    }
}

/// When the packets of `kind` that left in `[from, to)` left, per address.
fn per_address(
    packets: &[SentPacket],
    kind: Kind,
    from: f64,
    to: f64,
) -> BTreeMap<String, Vec<f64>> {
    let mut out: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for p in packets
        .iter()
        .filter(|p| p.kind == kind && p.at >= from && p.at < to)
    {
        out.entry(p.message.address.clone()).or_default().push(p.at);
    }
    out
}

/// R1, R2, R5: 16 colour ramps every 100 ms, 32 loop addresses at 3 to 15 s, a 66-fixture first
/// look.
#[test]
fn a_very_busy_song_gives_its_loops_a_fair_share_and_serves_every_address() {
    let song = busy_song(80, 160, 75.0);
    let (packets, load) = run(&song.rows, &song.effects, 0.0, 75.0);
    let total = counts(&packets);
    println!("busy song, 0-75 s: {total:?}, {load:?}");

    // The window: 60 s after the few seconds the engine takes to see that the song is busy.
    let (from, to) = (5.0, 65.0);
    let ramps = per_address(&packets, Kind::Ramp, from, to);
    let loops = per_address(&packets, Kind::Effect, from, to);
    let ramp_packets: usize = ramps.values().map(Vec::len).sum();
    let loop_packets: usize = loops.values().map(Vec::len).sum();
    let share = loop_packets as f64 / (ramp_packets + loop_packets) as f64;
    println!(
        "window {from}-{to} s: ramp {ramp_packets}, effect {loop_packets} = {:.1}% effects",
        share * 100.0
    );

    // The budget holds: 2,500 messages a second, steps included.
    let window_total = packets.iter().filter(|p| p.at >= from && p.at < to).count();
    assert!(
        window_total as f64 <= MAX_MESSAGES_PER_SECOND * (to - from) + 20.0,
        "{window_total} packets in {} s",
        to - from
    );
    // R1: effects at least 35% of what ramps and effects sent, ramps at most 65% ...
    // (35% is the guaranteed floor; a point of tolerance for the rounding of the average)
    assert!(share >= 0.34, "effects {:.1}%", share * 100.0);
    // ... when the loops do not need more: here they need 160 of 240 addresses' turns, so the
    // ramps are well under their 65%.
    assert!(
        share <= 0.75,
        "ramps keep a share too: {:.1}%",
        share * 100.0
    );

    // R2: every loop address at least 3 times a second, every moving ramp address about 4.
    for address in &song.loop_addresses {
        let rate = loops.get(address).map_or(0, Vec::len) as f64 / (to - from);
        assert!(rate >= 3.0, "{address}: {rate:.2} packets a second");
    }
    for address in &song.ramp_addresses {
        let rate = ramps.get(address).map_or(0, Vec::len) as f64 / (to - from);
        assert!(rate >= 3.9, "{address}: {rate:.2} packets a second");
    }
    // ... in turn: nobody waits much more than half a second.
    for (address, times) in ramps.iter().chain(loops.iter()) {
        let worst = times.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max);
        assert!(worst < 0.75, "{address}: waited {worst:.2} s once");
    }

    // R3: the engine says so.
    assert!(load.last.busy, "{:?}", load.last);
    assert!(load.busy_seconds > 60.0, "{}", load.busy_seconds);
    assert!(load.last.served_percent < 100);
    // The first look and the steps are never held back.
    assert!(total.step >= 66);
}

/// What the busy song asks for, against what the budget gives.
#[test]
fn the_busy_song_asks_for_far_more_than_the_budget_gives() {
    let song = busy_song(80, 160, 20.0);
    // 80 ramps and 160 loops at 40 Hz each.
    let wanted = (80 + 160) as f64 * 40.0;
    assert!(wanted > 3.0 * MAX_MESSAGES_PER_SECOND);
    let (_, load) = run(&song.rows, &song.effects, 0.0, 20.0);
    assert!(load.last.busy);
    assert!(
        load.last.served_percent <= 80,
        "{}% served",
        load.last.served_percent
    );
}

/// R1 with many more ramps than loops: ramps may not take more than 65%, and not one address
/// is left out.
#[test]
fn ramps_are_held_to_their_share_while_loops_wait() {
    let song = busy_song(320, 160, 40.0);
    let (packets, load) = run(&song.rows, &song.effects, 0.0, 40.0);
    let (from, to) = (6.0, 36.0);
    let ramps = per_address(&packets, Kind::Ramp, from, to);
    let loops = per_address(&packets, Kind::Effect, from, to);
    let r: usize = ramps.values().map(Vec::len).sum();
    let e: usize = loops.values().map(Vec::len).sum();
    let ramp_share = r as f64 / (r + e) as f64;
    println!(
        "320 ramps, 160 loops: ramps {r}, effects {e} = {:.1}% ramps",
        ramp_share * 100.0
    );
    assert!(ramp_share <= 0.67, "ramps took {:.1}%", ramp_share * 100.0);
    assert!(ramp_share >= 0.45, "ramps were left too little");
    for address in &song.loop_addresses {
        assert!(loops.get(address).map_or(0, Vec::len) > 30, "{address}");
    }
    for address in &song.ramp_addresses {
        assert!(ramps.get(address).map_or(0, Vec::len) > 30, "{address}");
    }
    assert!(load.last.busy);
}

/// A busy tick never puts more than 16 messages in one packet, never two values of one address
/// in one packet, and the second-by-second total stays inside the budget.
#[test]
fn a_busy_song_sends_bundles_of_at_most_16_inside_the_budget() {
    use std::collections::{BTreeMap, BTreeSet};
    let song = busy_song(80, 160, 30.0);
    let (packets, _) = run(&song.rows, &song.effects, 0.0, 30.0);
    let mut by_packet: BTreeMap<usize, Vec<&SentPacket>> = BTreeMap::new();
    for p in &packets {
        by_packet.entry(p.packet).or_default().push(p);
    }
    let biggest = by_packet.values().map(Vec::len).max().unwrap();
    assert_eq!(biggest, 16, "busy ticks fill the bundles");
    for (n, group) in &by_packet {
        assert!(group.len() <= 16, "packet {n}: {} messages", group.len());
        let addresses: BTreeSet<&str> = group.iter().map(|p| p.message.address.as_str()).collect();
        assert_eq!(addresses.len(), group.len(), "packet {n}: an address twice");
        assert!(
            group.iter().all(|p| (p.at - group[0].at).abs() < 1e-9),
            "packet {n}: one bundle leaves at one time"
        );
    }
    // the messages go out in the order the queue gave (bundles in order, `packet` never goes back)
    assert!(packets.windows(2).all(|w| w[1].packet >= w[0].packet));
    // the budget: a token bucket of 125 on top of 2,500 a second; the steps (the first look and
    // the like) are on top of it, a few hundred in this song
    let steps = counts(&packets).step as f64;
    let burst = MAX_MESSAGES_PER_SECOND / 20.0;
    for second in 1..29 {
        let n = packets
            .iter()
            .filter(|p| p.at >= second as f64 && p.at < second as f64 + 1.0)
            .count() as f64;
        assert!(
            n <= MAX_MESSAGES_PER_SECOND + burst + steps,
            "{n} messages in second {second}"
        );
    }
    let mean = packets.len() as f64 / 30.0;
    println!(
        "busy song: {} messages in {} packets, {mean:.0} a second, largest packet {biggest}",
        packets.len(),
        by_packet.len()
    );
}

/// "When only one class has demand it may use everything, as today."
#[test]
fn a_song_of_ramps_only_still_uses_the_whole_budget() {
    let song = busy_song(400, 0, 20.0);
    assert!(song.effects.is_empty());
    let (packets, _) = run(&song.rows, &song.effects, 0.0, 20.0);
    let c = counts(&packets);
    assert_eq!(c.effect, 0);
    let ramps_per_second = c.ramp as f64 / 20.0;
    assert!(
        ramps_per_second >= 0.9 * MAX_MESSAGES_PER_SECOND,
        "{ramps_per_second} ramp messages a second"
    );
}

/// R3: a song that fits is not busy, and says it is served in full.
#[test]
fn a_song_that_fits_is_not_busy() {
    let song = busy_song(2, 4, 20.0);
    let (packets, load) = run(&song.rows, &song.effects, 0.0, 20.0);
    assert!(counts(&packets).effect > 0);
    assert_eq!(load.busy_seconds, 0.0);
    assert!(!load.last.busy);
    assert!(load.last.served_percent >= 90, "{:?}", load.last);
}

// --------------------------------------------------------------------- the lights status

type Log = Arc<Mutex<Vec<String>>>;
struct CountOutput(Log);
impl OscOutput for CountOutput {
    fn closed(&self) -> bool {
        false
    }
    fn send_message(&self, address: &str, _: &[OscArgument]) -> Result<(), OutputError> {
        self.0.lock().unwrap().push(address.into());
        Ok(())
    }
    fn close(&self) {}
}
struct CountBackend(Log);
impl OscBackend for CountBackend {
    fn open_output(&self, _: &str, _: u16) -> Result<Arc<dyn OscOutput>, OutputError> {
        Ok(Arc::new(CountOutput(self.0.clone())))
    }
}

fn song(key: &str) -> Song {
    Song {
        id: format!("item-{key}"),
        title: key.into(),
        duration_seconds: Some(300),
        order: 1,
        service_sequence: None,
        is_generic: false,
        source_song_id: Some(key.into()),
        description: None,
        formatted_duration: None,
        key_name: None,
    }
}

/// Play a song through the plugin on tokio's paused clock and return the lights status'
/// `replay_load` after `seconds`, and how many packets were sent.
async fn status_after(
    rows: Vec<OscLightingCue>,
    effects: Vec<LightingEffect>,
    seconds: f64,
) -> (Option<ReplayLoad>, usize) {
    let mut cue_maps = BTreeMap::new();
    cue_maps.insert(
        "s".to_string(),
        SongLightingCueMap {
            song_key: "s".into(),
            song_title: "s".into(),
            cues: vec![],
            osc_cues: rows,
            loudness: None,
            captured_edited: false,
            capture: None,
            effects,
        },
    );
    let lights = LightsSettings {
        enabled: true,
        method: LightingMethod::Osc,
        osc_host: "127.0.0.1".into(),
        osc_port: 9000,
        cue_maps,
        lightkey: LightkeySettings {
            api_url: "http://127.0.0.1:9".into(),
            replay: true,
            ..LightkeySettings::default()
        },
        ..LightsSettings::default()
    };
    let bus = EventBus::new();
    let log: Log = Arc::default();
    let mut options = LightsOptions::new(Arc::new(|| {
        Err(stagepilot_midi::MidiError::public("no MIDI in this test"))
    }));
    let backend_log = log.clone();
    options.osc_backend_factory = Arc::new(move || Arc::new(CountBackend(backend_log.clone())));
    options.lightkey = LinkOptions {
        armed_store: Arc::new(MemoryArmedStore::new()) as Arc<dyn ArmedStore>,
        ..LinkOptions::default()
    };
    let plugin = LightsPlugin::new(bus.clone(), lights, options).unwrap();
    plugin.start().await.unwrap();
    bus.publish(new_event(
        EventType::SongStarted,
        "t",
        EventPayload::Song {
            song: song("s"),
            index: 0,
            position_seconds: None,
            observed_at: None,
        },
    ))
    .await;
    bus.publish(new_event(
        EventType::TimerStarted,
        "t",
        EventPayload::Timer(TimerPayload {
            duration_seconds: 300,
            song: Some(song("s")),
            running: true,
            ..Default::default()
        }),
    ))
    .await;
    tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
    let status = plugin.snapshot(false).await;
    let sent = log.lock().unwrap().len();
    (status.replay_load, sent)
}

#[tokio::test(start_paused = true)]
async fn the_lights_status_says_busy_only_while_a_busy_song_plays() {
    let busy = busy_song(80, 160, 30.0);
    let (load, sent) = status_after(busy.rows, busy.effects, 10.0).await;
    let load = load.expect("a replay that is running reports its load");
    println!("busy song through the plugin: {sent} packets in 10 s, {load:?}");
    assert!(load.busy, "{load:?}");
    assert!(load.served_percent < 100);
    // 2,500 a second at most, whatever the song asks for
    assert!(sent <= 26_000, "{sent}");

    let light = busy_song(2, 4, 30.0);
    let (load, _) = status_after(light.rows, light.effects, 10.0).await;
    let load = load.expect("a replay that is running reports its load");
    assert!(!load.busy, "{load:?}");
    assert!(load.served_percent >= 90, "{load:?}");

    // nothing playing: nothing to say (the field is left out of the status)
    let (load, _) = status_after(vec![], vec![], 1.0).await;
    assert_eq!(load, None);
}
