//! The Rust mirror must agree with the Python API schema frozen in `contract/openapi.json`:
//! same field names, same required/optional split. (Types are checked by the serde round trip.)

use serde_json::Value;
use stagepilot_core::api::{MidiInputResponse, MidiInputsResponse};
use std::collections::BTreeSet;

fn schema(name: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let doc: Value = serde_json::from_str(include_str!("../../../contract/openapi.json")).unwrap();
    let s = &doc["components"]["schemas"][name];
    let props = s["properties"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let required = s["required"]
        .as_array()
        .map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
        .unwrap_or_default();
    (props, required)
}

fn rust_fields(sample: &Value) -> BTreeSet<String> {
    sample.as_object().unwrap().keys().cloned().collect()
}

#[test]
fn midi_inputs_response_fields_match_openapi() {
    let sample = MidiInputsResponse {
        enabled: false,
        channel: 1,
        note: 112,
        configured_input_name: None,
        selected_input_name: None,
        inputs: vec![],
        mappings: Default::default(),
    };
    let (props, required) = schema("MidiInputsResponse");
    let json = serde_json::to_value(&sample).unwrap();
    assert_eq!(rust_fields(&json), props);
    // Optional on the Python side = has a default = `#[serde(default)]` here: deserialising
    // with only the required fields must work.
    let mut minimal = serde_json::Map::new();
    for k in &required {
        minimal.insert(k.clone(), json[k].clone());
    }
    let back: MidiInputsResponse = serde_json::from_value(Value::Object(minimal)).unwrap();
    assert_eq!(back, sample);
}

#[test]
fn midi_input_response_fields_match_openapi() {
    let sample = MidiInputResponse {
        id: "a".repeat(64),
        name: "Port".into(),
        ambiguous: false,
        selected: true,
        connected: true,
    };
    let (props, required) = schema("MidiInputResponse");
    assert_eq!(rust_fields(&serde_json::to_value(&sample).unwrap()), props);
    assert_eq!(
        required, props,
        "every MidiInputResponse field is required in Python"
    );
}

#[test]
fn cue_names_match_python_enum() {
    let doc: Value = serde_json::from_str(include_str!("../../../contract/openapi.json")).unwrap();
    let py: BTreeSet<String> = doc["components"]["schemas"]["MidiCueName"]["enum"]
        .as_array()
        .expect("openapi has no MidiCueName enum")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    use stagepilot_core::api::MidiCueName::*;
    let rust: BTreeSet<String> = [
        StartNext,
        RestartCurrent,
        Previous,
        Next,
        ReloadPlan,
        StopTimer,
    ]
    .iter()
    .map(|c| {
        serde_json::to_value(c)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string()
    })
    .collect();
    assert_eq!(rust, py);
}
