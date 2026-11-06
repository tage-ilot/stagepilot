//! A credential store that can never hold StagePilot up.
//!
//! The OS keychain (macOS Keychain, Windows Credential Manager, Secret Service) can sit on an
//! approval dialog nobody answers, for example on a locked screen after an update changed the
//! code signature. A native call like that cannot be cancelled safely, so
//! [`GuardedCredentialStore`] runs every call of the wrapped store on a thread of its own and
//! only waits a short, fixed time for the answer:
//!
//! * a read that answers in time behaves exactly like a plain read;
//! * a read that does not answer yet reports "Waiting for keychain access" and stays in flight
//!   (one at a time, never one per caller); its answer is handed to the next caller;
//! * a read that fails reports "Keychain access was refused" and is tried again after
//!   [`GuardTiming::retry_after`], never sooner (every try can raise the system dialog again);
//! * a write that does not finish in time reports it and keeps the value in memory, so a
//!   refreshed token is not lost, and [`CredentialStore::retry`] writes it again later.
//!
//! Nothing here deletes or replaces a stored secret because a read failed.

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::settings::{CredentialAccess, CredentialStore, CredentialStoreError};

/// Shown while a keychain call has not answered yet.
pub const KEYCHAIN_WAITING: &str = "Waiting for keychain access. StagePilot keeps running and connects as soon as access is allowed.";
/// Shown when the keychain refused or failed.
pub const KEYCHAIN_REFUSED: &str = "Keychain access was refused or failed. StagePilot tries again shortly; allow access when the system asks.";
/// Shown when a write did not finish.
pub const KEYCHAIN_WRITE_WAITING: &str = "The keychain did not answer in time, so the change is not saved yet. It stays in use until StagePilot closes and is saved when access is allowed.";
/// Shown when a write failed.
pub const KEYCHAIN_WRITE_FAILED: &str = "The keychain refused the change, so it is not saved yet. It stays in use until StagePilot closes; StagePilot tries to save it again shortly.";

/// How long the guard waits, and how often it tries again.
#[derive(Debug, Clone, Copy)]
pub struct GuardTiming {
    /// How long a caller waits for a read before getting "waiting" (the read carries on).
    pub read_wait: Duration,
    /// How long a caller waits for a write or delete.
    pub write_wait: Duration,
    /// The earliest a failed read is tried again.
    pub retry_after: Duration,
}

impl Default for GuardTiming {
    fn default() -> Self {
        Self {
            read_wait: Duration::from_millis(150),
            write_wait: Duration::from_secs(10),
            retry_after: Duration::from_secs(30),
        }
    }
}

enum ReadSlot {
    Idle,
    Reading {
        gen: u64,
    },
    /// A finished read nobody has taken yet.
    Done(Option<String>),
    Failed {
        at: Instant,
    },
}

struct State {
    read: ReadSlot,
    /// Bumped by every write or delete so an older read never overwrites newer data.
    gen: u64,
    /// A secret that is in use but not (yet) in the keychain.
    unsaved: Option<String>,
    write_busy: bool,
    write_failed: bool,
}

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
}

fn lock(shared: &Shared) -> MutexGuard<'_, State> {
    shared.state.lock().unwrap_or_else(|p| p.into_inner())
}

pub struct GuardedCredentialStore {
    inner: Arc<dyn CredentialStore>,
    shared: Arc<Shared>,
    timing: GuardTiming,
}

impl GuardedCredentialStore {
    pub fn new(inner: Arc<dyn CredentialStore>) -> Self {
        Self::with_timing(inner, GuardTiming::default())
    }

    pub fn with_timing(inner: Arc<dyn CredentialStore>, timing: GuardTiming) -> Self {
        Self {
            inner,
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    read: ReadSlot::Idle,
                    gen: 0,
                    unsaved: None,
                    write_busy: false,
                    write_failed: false,
                }),
                cv: Condvar::new(),
            }),
            timing,
        }
    }

    fn spawn_read(&self, gen: u64) -> bool {
        let inner = self.inner.clone();
        let shared = self.shared.clone();
        std::thread::Builder::new()
            .name("credential-read".into())
            .spawn(move || {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| inner.get_secret()));
                let mut st = lock(&shared);
                if matches!(st.read, ReadSlot::Reading { gen: g } if g == gen) {
                    st.read = match result {
                        Ok(Ok(value)) => ReadSlot::Done(value),
                        _ => ReadSlot::Failed { at: Instant::now() },
                    };
                }
                shared.cv.notify_all();
            })
            .is_ok()
    }

    /// Writes `unsaved` until it is saved or fails; one writer thread at a time.
    fn spawn_writer(&self) -> bool {
        let inner = self.inner.clone();
        let shared = self.shared.clone();
        std::thread::Builder::new()
            .name("credential-write".into())
            .spawn(move || loop {
                let Some(value) = lock(&shared).unsaved.clone() else {
                    let mut st = lock(&shared);
                    st.write_busy = false;
                    shared.cv.notify_all();
                    return;
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    inner.set_secret(&value)
                }));
                let mut st = lock(&shared);
                match result {
                    Ok(Ok(())) => {
                        if st.unsaved.as_deref() == Some(value.as_str()) {
                            st.unsaved = None;
                            st.write_failed = false;
                            st.write_busy = false;
                            st.read = ReadSlot::Idle;
                            shared.cv.notify_all();
                            return;
                        }
                        // A newer value arrived while this one was being written.
                    }
                    _ => {
                        st.write_failed = true;
                        st.write_busy = false;
                        shared.cv.notify_all();
                        return;
                    }
                }
            })
            .is_ok()
    }
}

impl CredentialStore for GuardedCredentialStore {
    fn get_secret(&self) -> Result<Option<String>, CredentialStoreError> {
        let mut st = lock(&self.shared);
        let started = Instant::now();
        loop {
            if let Some(value) = &st.unsaved {
                return Ok(Some(value.clone()));
            }
            match st.read {
                ReadSlot::Done(_) => {
                    if let ReadSlot::Done(value) = std::mem::replace(&mut st.read, ReadSlot::Idle) {
                        return Ok(value);
                    }
                }
                ReadSlot::Failed { at } => {
                    if at.elapsed() < self.timing.retry_after {
                        return Err(CredentialStoreError(KEYCHAIN_REFUSED.into()));
                    }
                    st.read = ReadSlot::Idle;
                }
                ReadSlot::Idle => {
                    let gen = st.gen;
                    st.read = ReadSlot::Reading { gen };
                    if !self.spawn_read(gen) {
                        st.read = ReadSlot::Failed { at: Instant::now() };
                    }
                }
                ReadSlot::Reading { .. } => {
                    let waited = started.elapsed();
                    if waited >= self.timing.read_wait {
                        return Err(CredentialStoreError(KEYCHAIN_WAITING.into()));
                    }
                    st = self
                        .shared
                        .cv
                        .wait_timeout(st, self.timing.read_wait - waited)
                        .unwrap_or_else(|p| p.into_inner())
                        .0;
                }
            }
        }
    }

    fn set_secret(&self, secret: &str) -> Result<(), CredentialStoreError> {
        let mut st = lock(&self.shared);
        st.gen += 1;
        st.unsaved = Some(secret.to_string());
        st.read = ReadSlot::Idle;
        if !st.write_busy {
            st.write_busy = true;
            st.write_failed = false;
            if !self.spawn_writer() {
                st.write_busy = false;
                st.write_failed = true;
            }
        }
        let started = Instant::now();
        loop {
            if st.unsaved.is_none() {
                return Ok(());
            }
            if !st.write_busy {
                return Err(CredentialStoreError(KEYCHAIN_WRITE_FAILED.into()));
            }
            let waited = started.elapsed();
            if waited >= self.timing.write_wait {
                return Err(CredentialStoreError(KEYCHAIN_WRITE_WAITING.into()));
            }
            st = self
                .shared
                .cv
                .wait_timeout(st, self.timing.write_wait - waited)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }

    fn remove_secret(&self) -> Result<(), CredentialStoreError> {
        {
            let mut st = lock(&self.shared);
            st.gen += 1;
            st.unsaved = None;
            st.read = ReadSlot::Idle;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let inner = self.inner.clone();
        let spawned = std::thread::Builder::new()
            .name("credential-remove".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    inner.remove_secret()
                }));
                let _ = tx.send(result);
            })
            .is_ok();
        if !spawned {
            return Err(CredentialStoreError(KEYCHAIN_REFUSED.into()));
        }
        match rx.recv_timeout(self.timing.write_wait) {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(CredentialStoreError(KEYCHAIN_REFUSED.into())),
            Err(_) => Err(CredentialStoreError(KEYCHAIN_WAITING.into())),
        }
    }

    fn access(&self) -> CredentialAccess {
        let st = lock(&self.shared);
        if st.unsaved.is_some() {
            return if st.write_busy {
                CredentialAccess::Waiting
            } else {
                CredentialAccess::Refused
            };
        }
        match st.read {
            ReadSlot::Reading { .. } => CredentialAccess::Waiting,
            ReadSlot::Failed { .. } => CredentialAccess::Refused,
            ReadSlot::Idle | ReadSlot::Done(_) => CredentialAccess::Ready,
        }
    }

    fn retry(&self) {
        let mut st = lock(&self.shared);
        if matches!(st.read, ReadSlot::Failed { .. }) {
            st.read = ReadSlot::Idle;
        }
        if st.unsaved.is_some() && !st.write_busy {
            st.write_busy = true;
            st.write_failed = false;
            if !self.spawn_writer() {
                st.write_busy = false;
                st.write_failed = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A store whose behaviour the test changes while it runs.
    #[derive(Default)]
    struct Fake {
        mode: Mutex<&'static str>,
        released: Mutex<bool>,
        cv: Condvar,
        value: Mutex<Option<String>>,
        reads: AtomicUsize,
    }
    impl Fake {
        fn set_mode(&self, mode: &'static str) {
            *self.mode.lock().unwrap() = mode;
        }
        fn release(&self) {
            *self.released.lock().unwrap() = true;
            self.cv.notify_all();
        }
        fn act(&self) -> Result<(), CredentialStoreError> {
            let mode = *self.mode.lock().unwrap();
            match mode {
                "block" => {
                    let mut released = self.released.lock().unwrap();
                    while !*released {
                        released = self.cv.wait(released).unwrap();
                    }
                    Ok(())
                }
                "slow" => {
                    std::thread::sleep(Duration::from_millis(400));
                    Ok(())
                }
                "error" => Err(CredentialStoreError("denied".into())),
                _ => Ok(()),
            }
        }
    }
    impl CredentialStore for Fake {
        fn get_secret(&self) -> Result<Option<String>, CredentialStoreError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.act()?;
            Ok(self.value.lock().unwrap().clone())
        }
        fn set_secret(&self, secret: &str) -> Result<(), CredentialStoreError> {
            self.act()?;
            *self.value.lock().unwrap() = Some(secret.into());
            Ok(())
        }
        fn remove_secret(&self) -> Result<(), CredentialStoreError> {
            self.act()?;
            *self.value.lock().unwrap() = None;
            Ok(())
        }
    }

    fn timing() -> GuardTiming {
        GuardTiming {
            read_wait: Duration::from_millis(50),
            write_wait: Duration::from_millis(300),
            retry_after: Duration::from_millis(100),
        }
    }

    fn guarded(fake: &Arc<Fake>) -> GuardedCredentialStore {
        GuardedCredentialStore::with_timing(fake.clone(), timing())
    }

    #[test]
    fn a_normal_read_is_returned_at_once() {
        let fake = Arc::new(Fake::default());
        *fake.value.lock().unwrap() = Some("s".into());
        let store = guarded(&fake);
        assert_eq!(store.get_secret().unwrap().as_deref(), Some("s"));
        assert_eq!(store.access(), CredentialAccess::Ready);
    }

    #[test]
    fn a_missing_secret_is_none_not_an_error() {
        let fake = Arc::new(Fake::default());
        assert_eq!(guarded(&fake).get_secret().unwrap(), None);
    }

    #[test]
    fn a_read_that_never_returns_gives_waiting_and_starts_only_one_thread() {
        let fake = Arc::new(Fake::default());
        fake.set_mode("block");
        let store = guarded(&fake);
        let started = Instant::now();
        for _ in 0..5 {
            let error = store.get_secret().unwrap_err();
            assert_eq!(error.0, KEYCHAIN_WAITING);
        }
        // Each call waits 50 ms by design; a slow CI runner (macOS Intel) stretches that several times.
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(store.access(), CredentialAccess::Waiting);
        assert_eq!(fake.reads.load(Ordering::SeqCst), 1);
        fake.release();
    }

    #[test]
    fn a_pending_read_is_picked_up_when_it_finishes() {
        let fake = Arc::new(Fake::default());
        *fake.value.lock().unwrap() = Some("late".into());
        fake.set_mode("slow");
        let store = guarded(&fake);
        assert_eq!(store.get_secret().unwrap_err().0, KEYCHAIN_WAITING);
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(store.get_secret().unwrap().as_deref(), Some("late"));
    }

    #[test]
    fn a_refused_read_reports_it_and_is_retried_only_after_the_pause() {
        let fake = Arc::new(Fake::default());
        fake.set_mode("error");
        let store = guarded(&fake);
        assert_eq!(store.get_secret().unwrap_err().0, KEYCHAIN_REFUSED);
        assert_eq!(store.get_secret().unwrap_err().0, KEYCHAIN_REFUSED);
        assert_eq!(
            fake.reads.load(Ordering::SeqCst),
            1,
            "no retry inside the pause"
        );
        assert_eq!(store.access(), CredentialAccess::Refused);
        // The keychain starts answering: the explicit retry (or the pause) lets it through.
        fake.set_mode("ok");
        *fake.value.lock().unwrap() = Some("back".into());
        store.retry();
        assert_eq!(store.get_secret().unwrap().as_deref(), Some("back"));
        assert_eq!(store.access(), CredentialAccess::Ready);
    }

    #[test]
    fn a_write_that_never_returns_is_reported_and_kept_in_memory() {
        let fake = Arc::new(Fake::default());
        fake.set_mode("block");
        let store = guarded(&fake);
        let started = Instant::now();
        let error = store.set_secret("token").unwrap_err();
        assert_eq!(error.0, KEYCHAIN_WRITE_WAITING);
        assert!(started.elapsed() < Duration::from_secs(3));
        // The token is still in use, and a second save does not pile up another writer.
        assert_eq!(store.get_secret().unwrap().as_deref(), Some("token"));
        assert!(store.set_secret("token2").is_err());
        assert_eq!(store.get_secret().unwrap().as_deref(), Some("token2"));
        assert_eq!(store.access(), CredentialAccess::Waiting);
        // The prompt is answered: the newest value is saved.
        fake.set_mode("ok");
        fake.release();
        let end = Instant::now() + Duration::from_secs(3);
        while store.access() != CredentialAccess::Ready && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(fake.value.lock().unwrap().as_deref(), Some("token2"));
    }

    #[test]
    fn a_refused_write_is_reported_kept_and_saved_by_retry() {
        let fake = Arc::new(Fake::default());
        fake.set_mode("error");
        let store = guarded(&fake);
        assert_eq!(store.set_secret("t").unwrap_err().0, KEYCHAIN_WRITE_FAILED);
        assert_eq!(store.get_secret().unwrap().as_deref(), Some("t"));
        assert_eq!(store.access(), CredentialAccess::Refused);
        fake.set_mode("ok");
        store.retry();
        let end = Instant::now() + Duration::from_secs(3);
        while store.access() != CredentialAccess::Ready && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(fake.value.lock().unwrap().as_deref(), Some("t"));
    }

    #[test]
    fn a_good_write_and_remove_pass_through() {
        let fake = Arc::new(Fake::default());
        let store = guarded(&fake);
        store.set_secret("a").unwrap();
        assert_eq!(store.get_secret().unwrap().as_deref(), Some("a"));
        store.remove_secret().unwrap();
        assert_eq!(store.get_secret().unwrap(), None);
    }

    #[test]
    fn a_remove_that_never_returns_reports_waiting() {
        let fake = Arc::new(Fake::default());
        fake.set_mode("block");
        let store = guarded(&fake);
        assert_eq!(store.remove_secret().unwrap_err().0, KEYCHAIN_WAITING);
        fake.release();
    }
}
