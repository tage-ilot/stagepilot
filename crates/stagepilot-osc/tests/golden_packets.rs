//! Byte-for-byte equality with python-osc for every message the Python lights plugin can send.
//! `tests/fixtures/osc_golden.json` is produced by `tools/rust-output/gen_osc_golden.py`, which
//! sends each case through Python's `UdpOscOutputBackend` to a loopback socket and records the
//! datagram it receives.

use serde_json::Value;
use stagepilot_core::config::OscArgument;
use stagepilot_osc::{encode_message, OscBackend, UdpOscBackend};
use std::net::UdpSocket;
use std::time::Duration;

fn golden() -> Vec<(String, Vec<OscArgument>, Vec<u8>)> {
    let text = include_str!("fixtures/osc_golden.json");
    let cases: Vec<Value> = serde_json::from_str(text).expect("golden json");
    cases
        .into_iter()
        .map(|case| {
            let address = case["address"].as_str().unwrap().to_string();
            let args = serde_json::from_value(case["args"].clone()).expect("typed args");
            let bytes = hex::decode(case["hex"].as_str().unwrap()).unwrap();
            (address, args, bytes)
        })
        .collect()
}

#[test]
fn encoding_equals_python_osc_for_every_golden_case() {
    let cases = golden();
    assert!(cases.len() >= 60, "golden set unexpectedly small");
    for (address, args, expected) in &cases {
        let got = encode_message(address, args).expect("encodes");
        assert_eq!(
            hex::encode(&got),
            hex::encode(expected),
            "address {address:?} args {args:?}"
        );
    }
}

#[test]
fn golden_set_covers_every_argument_type_and_the_sixteen_argument_limit() {
    let cases = golden();
    let has = |f: &dyn Fn(&OscArgument) -> bool| cases.iter().any(|(_, a, _)| a.iter().any(f));
    assert!(has(&|a| matches!(a, OscArgument::Int(_))));
    assert!(has(&|a| matches!(a, OscArgument::Float(_))));
    assert!(has(&|a| matches!(a, OscArgument::String(_))));
    assert!(has(&|a| matches!(a, OscArgument::Bool(true))));
    assert!(has(&|a| matches!(a, OscArgument::Bool(false))));
    assert!(cases.iter().any(|(_, a, _)| a.len() == 16));
    assert!(cases.iter().any(|(_, a, _)| a.is_empty()));
}

#[test]
fn real_udp_output_delivers_the_golden_datagrams() {
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let port = receiver.local_addr().unwrap().port();
    let output = UdpOscBackend.open_output("127.0.0.1", port).expect("opens");
    let mut buffer = vec![0u8; 65535];
    for (address, args, expected) in golden() {
        output.send_message(&address, &args).expect("sends");
        let (n, _) = receiver.recv_from(&mut buffer).expect("datagram");
        assert_eq!(
            hex::encode(&buffer[..n]),
            hex::encode(&expected),
            "address {address:?}"
        );
    }
    output.close();
    assert!(output.closed());
}

#[test]
fn send_after_close_is_a_public_error() {
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    let output = UdpOscBackend
        .open_output("127.0.0.1", receiver.local_addr().unwrap().port())
        .unwrap();
    output.close();
    let error = output.send_message("/x", &[]).unwrap_err();
    assert!(error.public);
    assert_eq!(error.message, "The OSC output is closed.");
}

#[test]
fn unknown_host_is_a_clear_error() {
    let error = UdpOscBackend
        .open_output("no-such-host.invalid", 21600)
        .err()
        .expect("must fail");
    assert!(error.public);
    assert!(error.message.contains("could not be found"), "{error}");
    assert_eq!(
        error.message,
        "The OSC host \"no-such-host.invalid\" could not be found."
    );
}

#[test]
fn overflowing_float_is_refused_and_wide_ints_are_int64() {
    // Beyond 31 bits python-osc sends an int64 (`h`), as the golden file proves for -2**31.
    assert!(encode_message("/x", &[OscArgument::Int(2_147_483_648)]).is_ok());
    assert!(encode_message("/x", &[OscArgument::Float(1e300)]).is_err());
    assert!(encode_message("/x", &[OscArgument::Float(f64::from(f32::MAX))]).is_ok());
}
