//! OSC output over UDP: port of `plugins/lights/osc.py` (python-osc's `SimpleUDPClient`).
//!
//! The wire encoding is `rosc`'s; `tests/golden_packets.rs` proves it is byte-for-byte what
//! python-osc sends for every message the Python lights plugin can produce. OSC over UDP is
//! one-shot: `open_output` only creates and resolves the socket, send errors are the only feedback.

use rosc::{encoder, OscMessage, OscPacket, OscType};
use stagepilot_core::config::OscArgument;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::{Arc, Mutex};

/// An output failure. `public` mirrors Python's `ValueError`/`RuntimeError` split: only those
/// messages are shown to the operator, everything else collapses to the caller's fallback text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputError {
    pub public: bool,
    pub message: String,
}

impl OutputError {
    pub fn public(message: impl Into<String>) -> Self {
        Self {
            public: true,
            message: message.into(),
        }
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            public: false,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for OutputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for OutputError {}

/// `OscOutputContract`.
pub trait OscOutput: Send + Sync {
    fn closed(&self) -> bool;
    fn send_message(&self, address: &str, args: &[OscArgument]) -> Result<(), OutputError>;
    fn close(&self);
}

/// `OscOutputBackendContract`.
pub trait OscBackend: Send + Sync {
    fn open_output(&self, host: &str, port: u16) -> Result<Arc<dyn OscOutput>, OutputError>;
}

/// One typed argument as python-osc puts it on the wire: `int` is `i`, `float` is `f` (32-bit
/// big endian), `string` is `s`, `bool` is `T` / `F` (no payload).
pub fn to_osc_type(argument: &OscArgument) -> Result<OscType, OutputError> {
    Ok(match argument {
        // python-osc picks `h` (int64) when `int.bit_length() > 31`, which includes -2**31.
        OscArgument::Int(v) => match i32::try_from(*v) {
            Ok(small) if small != i32::MIN => OscType::Int(small),
            _ => OscType::Long(*v),
        },
        OscArgument::Float(v) => {
            let narrowed = *v as f32;
            // struct.pack(">f") raises OverflowError for a finite double that does not fit.
            if v.is_finite() && !narrowed.is_finite() {
                return Err(OutputError::internal(
                    "OSC float argument is outside 32 bits.",
                ));
            }
            OscType::Float(narrowed)
        }
        OscArgument::String(v) => OscType::String(v.clone()),
        OscArgument::Bool(v) => OscType::Bool(*v),
    })
}

/// The datagram `SimpleUDPClient.send_message(address, values)` sends.
pub fn encode_message(address: &str, args: &[OscArgument]) -> Result<Vec<u8>, OutputError> {
    let args = args
        .iter()
        .map(to_osc_type)
        .collect::<Result<Vec<_>, _>>()?;
    encoder::encode(&OscPacket::Message(OscMessage {
        addr: address.to_string(),
        args,
    }))
    .map_err(|e| OutputError::internal(format!("OSC encoding failed: {e}")))
}

/// Real UDP output.
pub struct UdpOscOutput {
    socket: Mutex<Option<UdpSocket>>,
    target_text: String,
    target: SocketAddr,
}

impl UdpOscOutput {
    fn socket(&self) -> std::sync::MutexGuard<'_, Option<UdpSocket>> {
        self.socket.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl OscOutput for UdpOscOutput {
    fn closed(&self) -> bool {
        self.socket().is_none()
    }

    fn send_message(&self, address: &str, args: &[OscArgument]) -> Result<(), OutputError> {
        let guard = self.socket();
        let Some(socket) = guard.as_ref() else {
            return Err(OutputError::public("The OSC output is closed."));
        };
        let packet = encode_message(address, args)?;
        socket.send_to(&packet, self.target).map_err(|e| {
            OutputError::public(format!("OSC send to {} failed: {}.", self.target_text, e))
        })?;
        Ok(())
    }

    fn close(&self) {
        self.socket().take();
    }
}

/// Create UDP OSC outputs. The host is resolved here so a bad host fails on save.
#[derive(Debug, Default, Clone, Copy)]
pub struct UdpOscBackend;

impl OscBackend for UdpOscBackend {
    fn open_output(&self, host: &str, port: u16) -> Result<Arc<dyn OscOutput>, OutputError> {
        let target_text = format!("{host}:{port}");
        let not_found =
            || OutputError::public(format!("The OSC host \"{host}\" could not be found."));
        let target = (host, port)
            .to_socket_addrs()
            .map_err(|_| not_found())?
            .next()
            .ok_or_else(not_found)?;
        let bind = if target.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        };
        let socket = UdpSocket::bind(bind).map_err(|e| {
            OutputError::public(format!("OSC output to {target_text} failed: {e}."))
        })?;
        Ok(Arc::new(UdpOscOutput {
            socket: Mutex::new(Some(socket)),
            target_text,
            target,
        }))
    }
}

pub mod fake {
    //! In-memory OSC backend for tests (no sockets).

    use super::*;

    /// What a send recorded: the address and the argument values, as the Python fake does.
    pub type Sent = (String, Vec<OscArgument>);

    #[derive(Default)]
    pub struct FakeOscOutput {
        closed: Mutex<bool>,
        pub sent: Mutex<Vec<Sent>>,
        /// Datagrams exactly as UDP would carry them.
        pub packets: Mutex<Vec<Vec<u8>>>,
        pub fail_sends: Mutex<Option<String>>,
    }

    fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        m.lock().unwrap_or_else(|p| p.into_inner())
    }

    impl FakeOscOutput {
        pub fn sent(&self) -> Vec<Sent> {
            lock(&self.sent).clone()
        }
        pub fn set_closed(&self) {
            *lock(&self.closed) = true;
        }
    }

    impl OscOutput for FakeOscOutput {
        fn closed(&self) -> bool {
            *lock(&self.closed)
        }
        fn send_message(&self, address: &str, args: &[OscArgument]) -> Result<(), OutputError> {
            if let Some(message) = lock(&self.fail_sends).clone() {
                return Err(OutputError::public(message));
            }
            lock(&self.packets).push(encode_message(address, args)?);
            lock(&self.sent).push((address.to_string(), args.to_vec()));
            Ok(())
        }
        fn close(&self) {
            *lock(&self.closed) = true;
        }
    }

    #[derive(Default)]
    pub struct FakeOscBackend {
        pub outputs: Mutex<Vec<Arc<FakeOscOutput>>>,
        pub opened: Mutex<Vec<(String, u16)>>,
        pub open_error: Mutex<Option<OutputError>>,
    }

    impl FakeOscBackend {
        pub fn sent(&self) -> Vec<Sent> {
            lock(&self.outputs).iter().flat_map(|o| o.sent()).collect()
        }
        pub fn opened(&self) -> Vec<(String, u16)> {
            lock(&self.opened).clone()
        }
        pub fn outputs(&self) -> Vec<Arc<FakeOscOutput>> {
            lock(&self.outputs).clone()
        }
        pub fn fail_open(&self, error: OutputError) {
            *lock(&self.open_error) = Some(error);
        }
    }

    impl OscBackend for FakeOscBackend {
        fn open_output(&self, host: &str, port: u16) -> Result<Arc<dyn OscOutput>, OutputError> {
            if let Some(error) = lock(&self.open_error).clone() {
                return Err(error);
            }
            lock(&self.opened).push((host.to_string(), port));
            let output = Arc::new(FakeOscOutput::default());
            lock(&self.outputs).push(output.clone());
            Ok(output)
        }
    }
}
