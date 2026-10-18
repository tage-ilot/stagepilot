//! In-memory MIDI backends: no hardware, no threads. Used by the unit/behaviour tests and by the
//! contract server (`--fake-outputs`).

use crate::output::{MidiOutputBackend, MidiOutputPort};
use crate::{MidiBackend, MidiError, MidiInputPort, MidiMessage, MidiMessageCallback, NoteType};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

// ---- input ------------------------------------------------------------------------------------

pub struct FakeMidiPort {
    pub name: String,
    callback: MidiMessageCallback,
    closed: AtomicBool,
    close_calls: AtomicUsize,
}

impl FakeMidiPort {
    pub fn close_calls(&self) -> usize {
        self.close_calls.load(Ordering::SeqCst)
    }
    /// Simulate the device vanishing: the port reports itself closed without `close()`.
    pub fn mark_closed(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
    pub fn emit(&self, kind: NoteType, channel: u8, note: u8, velocity: u8) {
        (self.callback)(MidiMessage {
            kind,
            channel,
            note,
            velocity,
        });
    }
    pub fn note_on(&self, channel: u8, note: u8, velocity: u8) {
        self.emit(NoteType::NoteOn, channel, note, velocity);
    }
    pub fn note_off(&self, channel: u8, note: u8) {
        self.emit(NoteType::NoteOff, channel, note, 0);
    }
}

impl MidiInputPort for FakeMidiPort {
    fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    fn close(&self) {
        self.close_calls.fetch_add(1, Ordering::SeqCst);
        self.closed.store(true, Ordering::SeqCst);
    }
}

#[derive(Default)]
pub struct FakeMidiBackend {
    pub input_names: Mutex<Vec<String>>,
    pub open_calls: Mutex<Vec<String>>,
    pub ports: Mutex<Vec<Arc<FakeMidiPort>>>,
    list_calls: AtomicUsize,
    fail_listing: AtomicBool,
    /// Delivered from inside `open_input`, i.e. before the plugin has activated the connection.
    pub emit_on_open: Mutex<Option<MidiMessage>>,
}

impl FakeMidiBackend {
    pub fn new(names: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            input_names: Mutex::new(names.iter().map(|n| (*n).to_string()).collect()),
            ..Self::default()
        })
    }
    pub fn set_names(&self, names: &[&str]) {
        *lock(&self.input_names) = names.iter().map(|n| (*n).to_string()).collect();
    }
    pub fn add_name(&self, name: &str) {
        lock(&self.input_names).push(name.to_string());
    }
    pub fn ports(&self) -> Vec<Arc<FakeMidiPort>> {
        lock(&self.ports).clone()
    }
    pub fn open_calls(&self) -> Vec<String> {
        lock(&self.open_calls).clone()
    }
    pub fn list_calls(&self) -> usize {
        self.list_calls.load(Ordering::SeqCst)
    }
    pub fn fail_listing(&self, fail: bool) {
        self.fail_listing.store(fail, Ordering::SeqCst);
    }
}

impl MidiBackend for FakeMidiBackend {
    fn list_input_names(&self) -> Result<Vec<String>, MidiError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_listing.load(Ordering::SeqCst) {
            return Err(MidiError::internal("listing failed"));
        }
        Ok(lock(&self.input_names).clone())
    }

    fn open_input(
        &self,
        name: &str,
        callback: MidiMessageCallback,
    ) -> Result<Arc<dyn MidiInputPort>, MidiError> {
        let matches = lock(&self.input_names)
            .iter()
            .filter(|n| n.as_str() == name)
            .count();
        if matches != 1 {
            return Err(MidiError::public("MIDI input must have one exact match."));
        }
        lock(&self.open_calls).push(name.to_string());
        let early = *lock(&self.emit_on_open);
        let port = Arc::new(FakeMidiPort {
            name: name.to_string(),
            callback,
            closed: AtomicBool::new(false),
            close_calls: AtomicUsize::new(0),
        });
        lock(&self.ports).push(port.clone());
        if let Some(message) = early {
            (port.callback)(message);
        }
        Ok(port)
    }
}

// ---- output -----------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentMessage {
    pub kind: &'static str,
    pub channel: u8,
    pub note: u8,
    pub velocity: u8,
}

#[derive(Default)]
pub struct FakeOutputPort {
    closed: AtomicBool,
    pub messages: Mutex<Vec<SentMessage>>,
    fail_sends: AtomicBool,
}

impl FakeOutputPort {
    pub fn messages(&self) -> Vec<SentMessage> {
        lock(&self.messages).clone()
    }
    pub fn mark_closed(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
    pub fn fail_sends(&self, fail: bool) {
        self.fail_sends.store(fail, Ordering::SeqCst);
    }
}

impl MidiOutputPort for FakeOutputPort {
    fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    fn send_note_on(&self, channel: u8, note: u8, velocity: u8) -> Result<(), MidiError> {
        if self.fail_sends.load(Ordering::SeqCst) {
            return Err(MidiError::public("send failed"));
        }
        lock(&self.messages).push(SentMessage {
            kind: "note_on",
            channel,
            note,
            velocity,
        });
        Ok(())
    }
    fn send_note_off(&self, channel: u8, note: u8) -> Result<(), MidiError> {
        if self.fail_sends.load(Ordering::SeqCst) {
            return Err(MidiError::public("send failed"));
        }
        lock(&self.messages).push(SentMessage {
            kind: "note_off",
            channel,
            note,
            velocity: 0,
        });
        Ok(())
    }
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

#[derive(Default)]
pub struct FakeOutputBackend {
    pub names: Mutex<Vec<String>>,
    pub ports: Mutex<Vec<Arc<FakeOutputPort>>>,
}

impl FakeOutputBackend {
    pub fn new(names: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            names: Mutex::new(names.iter().map(|n| (*n).to_string()).collect()),
            ..Self::default()
        })
    }
    pub fn set_names(&self, names: &[&str]) {
        *lock(&self.names) = names.iter().map(|n| (*n).to_string()).collect();
    }
    pub fn ports(&self) -> Vec<Arc<FakeOutputPort>> {
        lock(&self.ports).clone()
    }
    /// Every message sent on any port, in order.
    pub fn messages(&self) -> Vec<SentMessage> {
        self.ports().iter().flat_map(|p| p.messages()).collect()
    }
}

impl MidiOutputBackend for FakeOutputBackend {
    fn list_output_names(&self) -> Result<Vec<String>, MidiError> {
        Ok(lock(&self.names).clone())
    }
    fn open_output(&self, name: &str) -> Result<Arc<dyn MidiOutputPort>, MidiError> {
        if lock(&self.names)
            .iter()
            .filter(|n| n.as_str() == name)
            .count()
            != 1
        {
            return Err(MidiError::public("Output unavailable."));
        }
        let port = Arc::new(FakeOutputPort::default());
        lock(&self.ports).push(port.clone());
        Ok(port)
    }
}
