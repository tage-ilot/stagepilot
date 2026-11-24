//! Patches of a loop (`LightingEffect::patches`): the file format and its rules. Old files
//! without the key read and write unchanged; a patch must name a member and a property of its
//! effect, lie inside the effect's window, and stay inside the caps.

use stagepilot_core::config::*;
use uuid::Uuid;

fn effect() -> LightingEffect {
    LightingEffect {
        id: Uuid::new_v4(),
        label: "Loop - 2 fixtures, 4 s".into(),
        start_seconds: 10.0,
        end_seconds: 100.0,
        period_seconds: 4.0,
        members: vec![
            EffectMember {
                fixture: "A".into(),
                offset_seconds: 0.0,
            },
            EffectMember {
                fixture: "B".into(),
                offset_seconds: 1.0,
            },
        ],
        cycle: vec![
            EffectStep {
                at_seconds: 0.0,
                property: "dimmer".into(),
                value: vec![1.0],
                fade_seconds: 2.0,
            },
            EffectStep {
                at_seconds: 2.0,
                property: "dimmer".into(),
                value: vec![0.0],
                fade_seconds: 2.0,
            },
        ],
        patches: Vec::new(),
        source: OscCueSource::Captured,
        edited: false,
    }
}

fn patch() -> EffectPatch {
    EffectPatch {
        fixture: "A".into(),
        property: "dimmer".into(),
        from_seconds: 20.0,
        to_seconds: 24.0,
        steps: vec![
            EffectPatchStep {
                at_seconds: 0.0,
                value: vec![1.0],
                fade_seconds: 0.5,
            },
            EffectPatchStep {
                at_seconds: 3.0,
                value: vec![0.0],
                fade_seconds: 1.0,
            },
        ],
    }
}

fn map_with(effect: LightingEffect) -> SongLightingCueMap {
    SongLightingCueMap {
        song_key: "s".into(),
        song_title: "S".into(),
        cues: vec![],
        osc_cues: vec![],
        loudness: None,
        captured_edited: false,
        capture: None,
        effects: vec![effect],
    }
}

fn errors_of(map: SongLightingCueMap) -> Vec<String> {
    let mut lights = LightsSettings::default();
    lights.cue_maps.insert(map.song_key.clone(), map);
    lights.normalise();
    let mut errors = Vec::new();
    lights.validate(&[], &mut errors);
    errors.into_iter().map(|e| e.msg).collect()
}

#[test]
fn an_effect_without_patches_reads_and_writes_exactly_as_before() {
    let text = serde_json::to_string(&effect()).expect("write");
    assert!(!text.contains("patches"), "{text}");
    let read: LightingEffect = serde_json::from_str(&text).expect("read");
    assert!(read.patches.is_empty());
    assert_eq!(read.cycle.len(), 2);
}

#[test]
fn patches_survive_a_round_trip_and_an_older_reader_would_ignore_them() {
    let mut e = effect();
    e.patches.push(patch());
    let text = serde_json::to_string(&e).expect("write");
    assert!(text.contains("\"patches\""), "{text}");
    let read: LightingEffect = serde_json::from_str(&text).expect("read");
    assert_eq!(read, e);
    // a build from before patches reads the same file with the key unknown: it must not refuse it
    let mut value: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert!(value
        .as_object_mut()
        .expect("object")
        .contains_key("patches"));
    value.as_object_mut().expect("object").remove("patches");
    let plain: LightingEffect = serde_json::from_value(value).expect("a loop without patches");
    assert_eq!(plain.cycle, e.cycle);
}

#[test]
fn a_good_patch_is_accepted() {
    let mut e = effect();
    e.patches.push(patch());
    let errors = errors_of(map_with(e));
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn a_patch_must_name_a_member_and_a_property_of_its_loop() {
    let mut e = effect();
    let mut stranger = patch();
    stranger.fixture = "Z".into();
    e.patches.push(stranger);
    let mut unknown = patch();
    unknown.property = "color".into();
    e.patches.push(unknown);
    let errors = errors_of(map_with(e));
    assert_eq!(errors.len(), 2, "{errors:?}");
}

#[test]
fn a_patch_must_lie_inside_the_window_and_have_steps_with_finite_values() {
    let mut e = effect();
    let mut outside = patch();
    outside.to_seconds = 101.0;
    e.patches.push(outside);
    let mut backwards = patch();
    backwards.from_seconds = 30.0;
    backwards.to_seconds = 25.0;
    e.patches.push(backwards);
    let mut empty = patch();
    empty.steps.clear();
    e.patches.push(empty);
    let mut nan = patch();
    nan.steps[0].value = vec![f64::NAN];
    e.patches.push(nan);
    let mut no_values = patch();
    no_values.steps[0].value = vec![];
    e.patches.push(no_values);
    let errors = errors_of(map_with(e));
    assert!(errors.len() >= 5, "{errors:?}");
}

#[test]
fn the_patch_steps_of_a_song_are_capped() {
    let mut e = effect();
    let mut big = patch();
    big.steps = (0..MAX_EFFECT_PATCH_STEPS_PER_PATCH)
        .map(|i| EffectPatchStep {
            at_seconds: i as f64 * 0.001,
            value: vec![0.5],
            fade_seconds: 0.0,
        })
        .collect();
    // exactly at the per-patch cap: accepted
    e.patches.push(big.clone());
    assert!(errors_of(map_with(e.clone())).is_empty());
    // one step over: refused
    let mut over = big.clone();
    over.steps.push(EffectPatchStep {
        at_seconds: 3.9,
        value: vec![0.5],
        fade_seconds: 0.0,
    });
    e.patches = vec![over];
    assert_eq!(errors_of(map_with(e.clone())).len(), 1);
    // many patches whose steps add up past the per-song cap: refused
    let per_effect = MAX_EFFECT_PATCH_STEPS.div_ceil(MAX_EFFECT_PATCH_STEPS_PER_PATCH) + 1;
    let mut effects = Vec::new();
    for _ in 0..per_effect {
        let mut one = effect();
        one.patches = vec![big.clone()];
        effects.push(one);
    }
    let mut map = map_with(effect());
    map.effects = effects;
    assert!(!errors_of(map).is_empty());
}

#[test]
fn patch_times_are_rounded_to_milliseconds_when_the_song_is_normalised() {
    let mut e = effect();
    let mut p = patch();
    p.from_seconds = 20.000_4;
    p.steps[0].fade_seconds = 0.500_49;
    e.patches.push(p);
    let mut lights = LightsSettings::default();
    let map = map_with(e);
    lights.cue_maps.insert(map.song_key.clone(), map);
    lights.normalise();
    let patch = &lights.cue_maps["s"].effects[0].patches[0];
    assert_eq!(patch.from_seconds, 20.0);
    assert_eq!(patch.steps[0].fade_seconds, 0.5);
}
