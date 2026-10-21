//! Command wire shapes, asserted against the live rig capture (`fixtures/real/rig-commands.jsonl`,
//! recorded 2026-10-17 on Playback 8.5.4; see docs/rust-migration/captures.md).
//!
//! The capture holds every command the test script SENT (`kind: sent`) and what a second,
//! listening connection RECEIVED from Playback (`kind: frame`). Playback relays commands it
//! understands to other clients unchanged, so for each sent command there must be an identical
//! relayed frame, and the Rust client's payload for the same command must equal both.

use serde_json::Value;
use stagepilot_playback::client::Command;

const CAPTURE: &str = include_str!("fixtures/real/rig-commands.jsonl");

/// The Rust `Command` that corresponds to a sent message, built only from the message's values.
fn command_for(msg: &Value) -> Command {
    let (kind, body) = msg.as_object().unwrap().iter().next().unwrap();
    match kind.as_str() {
        "transportPlay" => Command::Play(body["playing"].as_bool().unwrap()),
        "transportPad" => Command::Pad(body["playing"].as_bool().unwrap()),
        "transportFade" => Command::Fade(body["direction"].as_u64().unwrap() as u8),
        "waveformSeek" => Command::Seek(body["sequenceTime"].as_f64().unwrap()),
        "waveformDoubleTap" => Command::SectionJump(body["setlistSongSectionID"].as_i64().unwrap()),
        "waveformLoop" => Command::SectionLoop {
            section_id: body["setlistSongSectionID"].as_i64().unwrap(),
            active: body["active"].as_bool().unwrap(),
        },
        "mixerInfiniteLoop" => Command::InfiniteLoop(body["active"].as_bool().unwrap()),
        "mixerLoop" => Command::LoopOnce(body["active"].as_bool().unwrap()),
        "mixerMuteMIDI" => Command::MuteMidi(body["active"].as_bool().unwrap()),
        "setlistSelectSong" => Command::SelectSong(body["setlistSongID"].as_i64().unwrap()),
        "transportNextSong" => Command::Next,
        "transportPreviousSong" => Command::Previous,
        "transportReturnToStart" => Command::ReturnToStart,
        other => panic!("capture contains a command the client has no variant for: {other}"),
    }
}

#[test]
fn every_sent_command_matches_the_rust_payload_and_the_relayed_frame() {
    let mut sent = Vec::new();
    let mut relayed = Vec::new();
    for line in CAPTURE.lines() {
        let rec: Value = serde_json::from_str(line).unwrap();
        match rec["kind"].as_str().unwrap() {
            "sent" => sent.push(rec["msg"].clone()),
            "frame" if rec["msg"].get("heartbeat").is_none() => relayed.push(rec["msg"].clone()),
            _ => {}
        }
    }
    assert!(
        sent.len() >= 20,
        "capture too thin: {} commands",
        sent.len()
    );
    let mut kinds = std::collections::BTreeSet::new();
    for (i, msg) in sent.iter().enumerate() {
        let payload: Value = serde_json::from_str(&command_for(msg).payload()).unwrap();
        assert_eq!(
            &payload, msg,
            "command {i}: Rust payload differs from what was sent"
        );
        assert_eq!(
            &relayed[i], msg,
            "command {i}: Playback relayed something different from what was sent"
        );
        kinds.insert(msg.as_object().unwrap().keys().next().unwrap().clone());
    }
    assert_eq!(
        sent.len(),
        relayed.len(),
        "every sent command must be relayed once"
    );
    for required in [
        "transportPlay",
        "waveformSeek",
        "waveformDoubleTap",
        "setlistSelectSong",
        "transportPad",
        "transportFade",
        "waveformLoop",
        "mixerLoop",
        "mixerInfiniteLoop",
        "mixerMuteMIDI",
    ] {
        assert!(kinds.contains(required), "capture lacks {required}");
    }
}

/// Exact bytes the Rust client puts on the wire (key order and spacing), for the seven commands
/// phase 0 inferred plus the loop/jump variants. Playback does not care about spacing, but the
/// Python client (`playback_api.client`, compact separators) and this client agree byte for byte.
#[test]
fn exact_wire_bytes() {
    let cases: [(Command, &str); 12] = [
        (Command::Play(true), r#"{"transportPlay":{"playing":true}}"#),
        (
            Command::Play(false),
            r#"{"transportPlay":{"playing":false}}"#,
        ),
        (
            Command::SelectSong(29552198),
            r#"{"setlistSelectSong":{"setlistSongID":29552198}}"#,
        ),
        (Command::Pad(false), r#"{"transportPad":{"playing":false}}"#),
        (Command::Fade(1), r#"{"transportFade":{"direction":1}}"#),
        (Command::Fade(0), r#"{"transportFade":{"direction":0}}"#),
        (
            Command::SectionJump(0),
            r#"{"waveformDoubleTap":{"setlistSongSectionID":0}}"#,
        ),
        (
            Command::SectionLoop {
                section_id: 0,
                active: true,
            },
            r#"{"waveformLoop":{"setlistSongSectionID":0,"active":true}}"#,
        ),
        (
            Command::InfiniteLoop(true),
            r#"{"mixerInfiniteLoop":{"active":true}}"#,
        ),
        (
            Command::LoopOnce(false),
            r#"{"mixerLoop":{"active":false}}"#,
        ),
        (
            Command::MuteMidi(true),
            r#"{"mixerMuteMIDI":{"active":true}}"#,
        ),
        (
            Command::Seek(20.0),
            r#"{"waveformSeek":{"sequenceTime":20.0}}"#,
        ),
    ];
    for (cmd, wire) in cases {
        assert_eq!(cmd.payload(), wire);
    }
}
