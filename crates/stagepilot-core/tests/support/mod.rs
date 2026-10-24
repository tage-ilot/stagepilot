//! Log capture for tests. One global subscriber records every event with its thread; a test reads
//! only the events its own thread logged after `Capture::start()`. (A per-test `set_default`
//! subscriber is racy: tracing caches "no one is interested" per call site, so an event first hit
//! by another test thread can be lost.)
#![allow(dead_code)]

use std::sync::{Mutex, Once};
use std::thread::ThreadId;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Metadata, Subscriber};

static EVENTS: Mutex<Vec<(ThreadId, Level, String)>> = Mutex::new(Vec::new());
static INSTALL: Once = Once::new();

struct Text(String);
impl Visit for Text {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!("{}={:?} ", field.name(), value));
    }
}

struct Global;
impl Subscriber for Global {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut text = Text(String::new());
        event.record(&mut text);
        EVENTS.lock().unwrap_or_else(|p| p.into_inner()).push((
            std::thread::current().id(),
            *event.metadata().level(),
            text.0,
        ));
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

pub struct Capture {
    thread: ThreadId,
    from: usize,
}

impl Capture {
    pub fn start() -> Self {
        INSTALL.call_once(|| {
            let _ = tracing::subscriber::set_global_default(Global);
        });
        let events = EVENTS.lock().unwrap_or_else(|p| p.into_inner());
        Capture {
            thread: std::thread::current().id(),
            from: events.len(),
        }
    }

    pub fn at(&self, level: Level) -> Vec<String> {
        EVENTS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .skip(self.from)
            .filter(|(t, l, _)| *t == self.thread && *l == level)
            .map(|(_, _, text)| text.clone())
            .collect()
    }
}
