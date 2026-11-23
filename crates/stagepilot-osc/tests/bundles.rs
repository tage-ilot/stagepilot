//! OSC bundles: several messages in one UDP packet with the "immediately" time tag, which is how
//! the lights replay sends what a tick has due. The encoder is checked against the OSC 1.0 layout
//! byte for byte and against `rosc`'s decoder; the real UDP output delivers one datagram.

use rosc::{decoder, OscPacket, OscType};
use stagepilot_core::config::OscArgument;
use stagepilot_osc::fake::FakeOscBackend;
use stagepilot_osc::{encode_bundle, encode_message, OscBackend, UdpOscBackend};
use std::net::UdpSocket;
use std::time::Duration;

fn messages() -> Vec<(String, Vec<OscArgument>)> {
    vec![
        (
            "/fixture/PN1/overrides/dimmer".into(),
            vec![OscArgument::Float(0.5)],
        ),
        (
            "/fixture/PN2/overrides/color".into(),
            vec![
                OscArgument::Float(0.1),
                OscArgument::Float(0.2),
                OscArgument::Float(0.3),
            ],
        ),
        ("/fixture/PN3/overrides/clear".into(), vec![]),
    ]
}

#[test]
fn a_bundle_is_the_osc_layout_byte_for_byte() {
    let messages = messages();
    let bytes = encode_bundle(&messages).unwrap();
    // "#bundle\0", then the time tag 0 / 1 (immediately)
    assert_eq!(&bytes[..8], b"#bundle\0");
    assert_eq!(&bytes[8..16], &[0, 0, 0, 0, 0, 0, 0, 1]);
    // then every message with its size (4 bytes, big endian), the messages as `encode_message`
    let mut at = 16;
    for (address, args) in &messages {
        let want = encode_message(address, args).unwrap();
        let size = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        assert_eq!(size, want.len());
        assert_eq!(&bytes[at + 4..at + 4 + size], want.as_slice());
        at += 4 + size;
    }
    assert_eq!(at, bytes.len(), "nothing after the last message");
}

#[test]
fn a_bundle_round_trips_through_the_osc_decoder_in_order() {
    let messages = messages();
    let bytes = encode_bundle(&messages).unwrap();
    let (rest, packet) = decoder::decode_udp(&bytes).unwrap();
    assert!(rest.is_empty());
    let OscPacket::Bundle(bundle) = packet else {
        panic!("a bundle was expected");
    };
    assert_eq!((bundle.timetag.seconds, bundle.timetag.fractional), (0, 1));
    assert_eq!(bundle.content.len(), 3);
    for (packet, (address, args)) in bundle.content.iter().zip(&messages) {
        let OscPacket::Message(m) = packet else {
            panic!("a message was expected");
        };
        assert_eq!(&m.addr, address);
        assert_eq!(m.args.len(), args.len());
        for (got, want) in m.args.iter().zip(args) {
            match (got, want) {
                (OscType::Float(x), OscArgument::Float(y)) => assert_eq!(*x, *y as f32),
                other => panic!("{other:?}"),
            }
        }
    }
}

#[test]
fn sixteen_messages_of_a_colour_each_fit_one_datagram() {
    let sixteen: Vec<_> = (0..16)
        .map(|i| {
            (
                format!("/fixture/PN{i:02}/overrides/color"),
                vec![
                    OscArgument::Float(0.1),
                    OscArgument::Float(0.2),
                    OscArgument::Float(0.3),
                ],
            )
        })
        .collect();
    let bytes = encode_bundle(&sixteen).unwrap();
    assert!(bytes.len() < 1400, "{} bytes", bytes.len());
}

#[test]
fn real_udp_output_sends_a_bundle_as_one_datagram_and_one_message_as_a_plain_message() {
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let port = receiver.local_addr().unwrap().port();
    let output = UdpOscBackend.open_output("127.0.0.1", port).unwrap();
    let mut buffer = vec![0u8; 65535];

    let messages = messages();
    output.send_bundle(&messages).unwrap();
    let (n, _) = receiver.recv_from(&mut buffer).unwrap();
    assert_eq!(&buffer[..n], encode_bundle(&messages).unwrap().as_slice());

    output.send_bundle(&messages[..1]).unwrap();
    let (n, _) = receiver.recv_from(&mut buffer).unwrap();
    assert_eq!(
        &buffer[..n],
        encode_message(&messages[0].0, &messages[0].1)
            .unwrap()
            .as_slice(),
        "one message is not wrapped in a bundle"
    );

    output.close();
    let error = output.send_bundle(&messages).unwrap_err();
    assert!(error.public);
}

#[test]
fn the_fake_output_records_a_bundle_as_its_messages_in_order() {
    let backend = FakeOscBackend::default();
    let output = backend.open_output("127.0.0.1", 21600).unwrap();
    let messages = messages();
    output.send_bundle(&messages).unwrap();
    assert_eq!(backend.sent(), messages);
    let fake = &backend.outputs()[0];
    assert_eq!(fake.packets.lock().unwrap().len(), 1, "one packet");
}
