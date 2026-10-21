//! Concurrency-safe observable application state store: port of `core/state.py`.
//!
//! One mutex owns the state. Every mutation bumps `revision`, stamps `updated_at` and publishes
//! the new snapshot on a `watch` channel. A `watch` receiver only ever sees the latest value, which
//! is the same "queue of size one, drop the oldest" behaviour as the Python store: a slow observer
//! (a stalled WebSocket) never blocks writers or other observers.

use crate::state::ApplicationState;
use chrono::Utc;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

#[derive(Clone)]
pub struct StateStore {
    inner: Arc<Inner>,
}

struct Inner {
    state: Mutex<ApplicationState>,
    tx: watch::Sender<Arc<ApplicationState>>,
}

impl Default for StateStore {
    fn default() -> Self {
        Self::new(ApplicationState::default())
    }
}

impl StateStore {
    pub fn new(initial: ApplicationState) -> Self {
        let (tx, _rx) = watch::channel(Arc::new(initial.clone()));
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(initial),
                tx,
            }),
        }
    }

    pub fn snapshot(&self) -> ApplicationState {
        self.lock().clone()
    }

    /// Apply `mutation`, bump the revision, stamp `updated_at`, notify observers, return the
    /// resulting snapshot.
    pub fn mutate<F: FnOnce(&mut ApplicationState)>(&self, mutation: F) -> ApplicationState {
        let mut guard = self.lock();
        mutation(&mut guard);
        guard.revision += 1;
        guard.updated_at = Utc::now();
        let snapshot = guard.clone();
        // Sent while still holding the lock so observers see strictly increasing revisions.
        self.inner.tx.send_replace(Arc::new(snapshot.clone()));
        snapshot
    }

    /// Observe snapshots. The receiver always holds the latest one.
    pub fn subscribe(&self) -> watch::Receiver<Arc<ApplicationState>> {
        self.inner.tx.subscribe()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ApplicationState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
