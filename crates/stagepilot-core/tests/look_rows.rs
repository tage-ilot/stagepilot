//! Look rows: one timeline row holding several OSC messages. Old files must load and re-save
//! unchanged, the caps are enforced, and `outgoing` picks the messages or the single address.

use serde_json::json;
use stagepilot_core::config::*;
use uuid::Uuid;

fn lights_with(osc_cues: serde_json::Value) -> LightsSettings {
    serde_json::from_value(json!({
        "enabled": true,
        "cue_maps": {"s": {"song_key": "s", "song_title": "S", "osc_cues": osc_cues}}
    }))
    .unwrap()
}

fn errors_of(mut lights: LightsSettings) -> Vec<String> {
    lights.normalise();
    let mut errors = Vec::new();
    lights.validate(&[], &mut errors);
    errors.into_iter().map(|e| e.msg).collect()
}

fn message(address: &str) -> OscMessage {
    OscMessage {
        address: address.into(),
        args: vec![OscArgument::Float(0.5)],
    }
}

fn look(at: f64, messages: Vec<OscMessage>) -> OscLightingCue {
    OscLightingCue {
        id: Uuid::new_v4(),
        at_seconds: at,
        address: String::new(),
        args: vec![],
        label: String::new(),
        color: None,
        messages,
        source: OscCueSource::Captured,
    }
}

fn with_cues(cues: Vec<OscLightingCue>) -> LightsSettings {
    let mut lights = LightsSettings::default();
    lights.cue_maps.insert(
        "s".into(),
        SongLightingCueMap {
            song_key: "s".into(),
            song_title: "S".into(),
            cues: vec![],
            osc_cues: cues,
        },
    );
    lights
}

#[test]
fn an_old_cue_loads_as_a_manual_row_and_saves_unchanged() {
    let old = json!({
        "id": "6f0e2f0e-7a8e-4c52-9c7d-5f3f0f0f0f01",
        "at_seconds": 12.5,
        "address": "/live/cue/1",
        "args": [{"type": "int", "value": 3}],
        "label": "Verse",
        "color": null
    });
    let lights = lights_with(json!([old.clone()]));
    let cue = &lights.cue_maps["s"].osc_cues[0];
    assert!(cue.messages.is_empty());
    assert_eq!(cue.source, OscCueSource::Manual);
    let saved = serde_json::to_value(&lights.cue_maps["s"].osc_cues[0]).unwrap();
    assert_eq!(
        saved, old,
        "no messages/source keys appear on an untouched cue"
    );
    assert!(errors_of(lights).is_empty());
}

#[test]
fn a_look_row_round_trips_through_json() {
    let row = look(
        3.0,
        vec![
            message("/fixture/MH 1/overrides/dimmer"),
            message("/fixture/MH 2/overrides/dimmer"),
        ],
    );
    let text = serde_json::to_string(&row).unwrap();
    assert!(text.contains("\"source\":\"captured\""));
    let back: OscLightingCue = serde_json::from_str(&text).unwrap();
    assert_eq!(back, row);
}

#[test]
fn a_look_row_sends_its_messages_and_a_plain_row_its_address() {
    let row = look(1.0, vec![message("/a"), message("/b")]);
    let sent: Vec<_> = row.outgoing().into_iter().map(|m| m.address).collect();
    assert_eq!(sent, ["/a", "/b"]);
    let mut plain = look(1.0, vec![]);
    plain.address = "/live/cue/1".into();
    plain.args = vec![OscArgument::Int(2)];
    let out = plain.outgoing();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].address, "/live/cue/1");
    assert_eq!(out[0].args, vec![OscArgument::Int(2)]);
}

#[test]
fn look_message_addresses_follow_the_address_rules() {
    for bad in ["no-slash", "/has space", &format!("/{}", "x".repeat(256))] {
        let errors = errors_of(with_cues(vec![look(1.0, vec![message(bad)])]));
        assert_eq!(errors.len(), 1, "{bad}: {errors:?}");
    }
    assert!(errors_of(with_cues(vec![look(1.0, vec![message("/ok")])])).is_empty());
}

#[test]
fn caps_are_named_and_sized_for_a_400_row_6k_message_song() {
    assert!(MAX_OSC_ROWS_PER_SONG >= 400 * 2);
    assert!(MAX_OSC_MESSAGES_PER_SONG >= 6_000 * 2);
    assert!(MAX_OSC_MESSAGES_PER_ROW >= 106);
}

#[test]
fn too_many_messages_in_one_row_is_refused() {
    let at_cap = look(
        1.0,
        (0..MAX_OSC_MESSAGES_PER_ROW)
            .map(|_| message("/a"))
            .collect(),
    );
    assert!(errors_of(with_cues(vec![at_cap])).is_empty());
    let over = look(
        1.0,
        (0..=MAX_OSC_MESSAGES_PER_ROW)
            .map(|_| message("/a"))
            .collect(),
    );
    assert_eq!(errors_of(with_cues(vec![over])).len(), 1);
}

#[test]
fn too_many_rows_in_one_song_is_refused() {
    let rows = |n: usize| (0..n).map(|i| look(i as f64 / 10.0, vec![])).collect();
    let mut at_cap: Vec<OscLightingCue> = rows(MAX_OSC_ROWS_PER_SONG);
    for row in &mut at_cap {
        row.address = "/a".into();
    }
    assert!(errors_of(with_cues(at_cap)).is_empty());
    let mut over: Vec<OscLightingCue> = rows(MAX_OSC_ROWS_PER_SONG + 1);
    for row in &mut over {
        row.address = "/a".into();
    }
    assert_eq!(errors_of(with_cues(over)).len(), 1);
}

#[test]
fn too_many_messages_in_one_song_is_refused() {
    let per_row = MAX_OSC_MESSAGES_PER_ROW;
    let rows_needed = MAX_OSC_MESSAGES_PER_SONG / per_row + 1;
    let rows: Vec<_> = (0..rows_needed)
        .map(|i| {
            look(
                i as f64 / 10.0,
                (0..per_row).map(|_| message("/a")).collect(),
            )
        })
        .collect();
    assert!(rows.iter().map(|r| r.messages.len()).sum::<usize>() > MAX_OSC_MESSAGES_PER_SONG);
    assert_eq!(errors_of(with_cues(rows)).len(), 1);
}

#[test]
fn a_fixture_address_may_contain_spaces_but_other_addresses_may_not() {
    let ok = errors_of(with_cues(vec![look(
        1.0,
        vec![message("/fixture/Moving Head 1/overrides/dimmer")],
    )]));
    assert!(ok.is_empty(), "{ok:?}");
    let bad = errors_of(with_cues(vec![look(1.0, vec![message("/live/my cue")])]));
    assert_eq!(bad.len(), 1);
    let control = errors_of(with_cues(vec![look(
        1.0,
        vec![message("/fixture/a\nb/overrides/dimmer")],
    )]));
    assert_eq!(control.len(), 1);
}
