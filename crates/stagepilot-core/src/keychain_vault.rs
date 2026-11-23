//! One keychain item per process instead of one per secret (macOS).
//!
//! The macOS login keychain asks again, for every item, the first time a new build of the app
//! touches it. StagePilot kept four items under the service `StagePilot` and the desktop shell
//! one per remote-access account, so an update raised six prompts. A [`Vault`] keeps all the
//! secrets of one process in a single keychain item (account `vault`, a JSON object of
//! `{ "<account>": "<secret>" }`), so the next update asks once per process.
//!
//! Rules this module keeps (the tests below prove each one against in-memory fakes):
//! - A vault belongs to ONE process. The backend and the desktop shell use different services, so
//!   they never read-modify-write the same item. Writes inside the process are serialized by the
//!   lock that also guards the cached copy.
//! - The item is read at most once per launch. Our own writes update the cached copy.
//! - Migration is non-destructive. An account the vault lacks is read from its LEGACY per-account
//!   item (the old prompt, once, and only for items that exist), copied into the vault with one
//!   write per pass, and the vault is read back and compared before the copy counts as done.
//!   The legacy items are NEVER deleted here: a delete is one more prompt, and an older app
//!   (a rollback) still reads them. After a rollback, the older app sees the value the legacy
//!   item had when it was migrated, not a token saved later in the vault.
//! - A delete leaves a tombstone (`null`) in the vault. Without it the next launch would find the
//!   account missing, read the untouched legacy item and bring the deleted secret back.
//! - An unparsable vault is never overwritten. Reads fall back to the legacy items, writes and
//!   deletes are refused until the item is fixed, with the same plain error as an unavailable
//!   keychain.
//! - A secret never appears in an error, a log line or a `Debug` output. This module has no
//!   `Debug` for anything that can hold one and no message that quotes the item.
//!
//! The operating-system glue (the real keychain items) exists on macOS only. Everything else is
//! plain code over the [`KeychainBackend`] trait, so Windows and Linux keep their per-account
//! items untouched and the logic is still tested on every platform.

use crate::settings::{KeychainBackend, KeychainUnavailable};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// The keychain account of the vault item.
pub const VAULT_ACCOUNT: &str = "vault";

/// What a vault holds for one account: `Some(secret)`, or `None` for a deliberate delete.
type Entries = BTreeMap<String, Option<String>>;

/// Opens the legacy per-account item of an account (read only here).
pub type LegacyItems = Box<dyn Fn(&str) -> Box<dyn KeychainBackend> + Send + Sync>;

/// Saved secrets by account. The vault implements it; the desktop broker routes through it.
pub trait AccountSecrets: Send + Sync {
    /// `Ok(None)` when nothing is saved; `Err` when the keychain itself is unavailable.
    fn get(&self, account: &str) -> Result<Option<String>, KeychainUnavailable>;
    fn set(&self, account: &str, secret: &str) -> Result<(), KeychainUnavailable>;
    /// Deleting a secret that is not saved is not an error.
    fn delete(&self, account: &str) -> Result<(), KeychainUnavailable>;
}

enum Loaded {
    Entries(Entries),
    /// The item exists but is not a JSON object of strings. Never overwritten.
    Corrupt,
}

struct State {
    loaded: Option<Loaded>,
    /// A migration write failed or did not read back: stop trying (each try can prompt) and keep
    /// answering from the legacy items for the rest of this launch.
    migration_blocked: bool,
}

/// One keychain item holding every secret of one process.
pub struct Vault {
    item: Box<dyn KeychainBackend>,
    legacy: LegacyItems,
    state: Mutex<State>,
}

fn parse(text: &str) -> Loaded {
    // The parser's error is dropped on purpose: it can quote part of the text.
    match serde_json::from_str::<Entries>(text) {
        Ok(entries) => Loaded::Entries(entries),
        Err(_) => Loaded::Corrupt,
    }
}

impl Vault {
    /// `item` is the vault item itself (read and write only), `legacy` opens the old
    /// per-account items.
    pub fn new(item: Box<dyn KeychainBackend>, legacy: LegacyItems) -> Self {
        Self {
            item,
            legacy,
            state: Mutex::new(State {
                loaded: None,
                migration_blocked: false,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Reads the item the first time only. A failed read is not remembered (as before, it is
    /// tried again later); an unparsable item is, so it is not asked for again and again.
    fn ensure_loaded(&self, state: &mut State) -> Result<(), KeychainUnavailable> {
        if state.loaded.is_none() {
            state.loaded = Some(match self.item.read()? {
                None => Loaded::Entries(Entries::new()),
                Some(text) => parse(&text),
            });
        }
        Ok(())
    }

    /// Writes `entries` as the whole vault, reading it back to compare. Returns whether the
    /// write is confirmed.
    fn write_entries(&self, entries: &Entries) -> Result<String, KeychainUnavailable> {
        let text = serde_json::to_string(entries).map_err(|_| KeychainUnavailable)?;
        self.item.write(&text)?;
        Ok(text)
    }

    /// The secrets of several accounts in one pass. Accounts the vault lacks are read from their
    /// legacy items and written into the vault with a single write for the whole pass. The answer
    /// for each account is independent: one unavailable legacy item does not hide the others.
    pub fn get_many(&self, accounts: &[&str]) -> Vec<Result<Option<String>, KeychainUnavailable>> {
        let mut state = self.lock();
        if let Err(error) = self.ensure_loaded(&mut state) {
            return accounts.iter().map(|_| Err(error)).collect();
        }
        let blocked = state.migration_blocked;
        let (answers, migrated) = match state.loaded.as_ref() {
            Some(Loaded::Entries(entries)) => {
                let mut answers = Vec::with_capacity(accounts.len());
                let mut migrated = Entries::new();
                for account in accounts {
                    match entries.get(*account) {
                        Some(known) => answers.push(Ok(known.clone())),
                        None => {
                            let legacy = (self.legacy)(account).read();
                            if let Ok(Some(secret)) = &legacy {
                                migrated.insert((*account).to_string(), Some(secret.clone()));
                            }
                            answers.push(legacy);
                        }
                    }
                }
                (answers, migrated)
            }
            // Unparsable vault: answer from the legacy items and touch nothing.
            _ => (
                accounts
                    .iter()
                    .map(|account| (self.legacy)(account).read())
                    .collect(),
                Entries::new(),
            ),
        };
        if migrated.is_empty() || blocked {
            return answers;
        }
        if let Some(Loaded::Entries(entries)) = state.loaded.as_ref() {
            let mut next = entries.clone();
            next.extend(migrated);
            // Only a write that reads back identical counts as migrated; anything else leaves
            // the cached copy as it was and stops further tries in this launch.
            let confirmed = match self.write_entries(&next) {
                Ok(text) => matches!(self.item.read(), Ok(Some(back)) if back == text),
                Err(_) => false,
            };
            if confirmed {
                state.loaded = Some(Loaded::Entries(next));
            } else {
                state.migration_blocked = true;
            }
        }
        answers
    }

    fn change(&self, account: &str, value: Option<String>) -> Result<(), KeychainUnavailable> {
        let mut state = self.lock();
        self.ensure_loaded(&mut state)?;
        let Some(Loaded::Entries(entries)) = state.loaded.as_ref() else {
            // An unparsable vault is not overwritten.
            return Err(KeychainUnavailable);
        };
        if value.is_none() && matches!(entries.get(account), Some(None)) {
            return Ok(());
        }
        let mut next = entries.clone();
        next.insert(account.to_string(), value);
        // A failed write leaves the old vault in the keychain and in the cached copy.
        self.write_entries(&next)?;
        state.loaded = Some(Loaded::Entries(next));
        Ok(())
    }
}

impl AccountSecrets for Vault {
    fn get(&self, account: &str) -> Result<Option<String>, KeychainUnavailable> {
        self.get_many(&[account])
            .pop()
            .unwrap_or(Err(KeychainUnavailable))
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), KeychainUnavailable> {
        self.change(account, Some(secret.to_string()))
    }
    fn delete(&self, account: &str) -> Result<(), KeychainUnavailable> {
        self.change(account, None)
    }
}

/// One account seen through a shared vault, as a [`KeychainBackend`] for `KeyringCredentialStore`.
pub struct VaultAccount {
    vault: Arc<Vault>,
    account: String,
}

impl VaultAccount {
    pub fn new(vault: Arc<Vault>, account: &str) -> Self {
        Self {
            vault,
            account: account.to_string(),
        }
    }
}

impl KeychainBackend for VaultAccount {
    fn read(&self) -> Result<Option<String>, KeychainUnavailable> {
        self.vault.get(&self.account)
    }
    fn write(&self, secret: &str) -> Result<(), KeychainUnavailable> {
        self.vault.set(&self.account, secret)
    }
    fn delete(&self) -> Result<(), KeychainUnavailable> {
        self.vault.delete(&self.account)
    }
}

/// The real keychain, macOS only.
#[cfg(target_os = "macos")]
mod os {
    use super::*;
    use std::sync::OnceLock;

    /// The vault item of one service.
    struct OsItem {
        service: String,
    }

    impl KeychainBackend for OsItem {
        fn read(&self) -> Result<Option<String>, KeychainUnavailable> {
            match keyring::Entry::new(&self.service, VAULT_ACCOUNT).and_then(|e| e.get_password()) {
                Ok(text) => Ok(Some(text)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(_) => Err(KeychainUnavailable),
            }
        }
        fn write(&self, secret: &str) -> Result<(), KeychainUnavailable> {
            keyring::Entry::new(&self.service, VAULT_ACCOUNT)
                .and_then(|e| e.set_password(secret))
                .map_err(|_| KeychainUnavailable)
        }
        fn delete(&self) -> Result<(), KeychainUnavailable> {
            Err(KeychainUnavailable)
        }
    }

    /// A legacy per-account item (read only: legacy items are never written or deleted).
    struct LegacyItem {
        service: String,
        account: String,
    }

    impl KeychainBackend for LegacyItem {
        fn read(&self) -> Result<Option<String>, KeychainUnavailable> {
            match keyring::Entry::new(&self.service, &self.account).and_then(|e| e.get_password()) {
                Ok(text) => Ok(Some(text)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(_) => Err(KeychainUnavailable),
            }
        }
        fn write(&self, _: &str) -> Result<(), KeychainUnavailable> {
            Err(KeychainUnavailable)
        }
        fn delete(&self) -> Result<(), KeychainUnavailable> {
            Err(KeychainUnavailable)
        }
    }

    /// The process's vault for `service`; one per service, shared by every store of the process.
    /// It holds the cached item for as long as the stores' own caches do (the whole launch).
    pub fn vault_for_service(service: &str) -> Arc<Vault> {
        static VAULTS: OnceLock<Mutex<BTreeMap<String, Arc<Vault>>>> = OnceLock::new();
        let mut vaults = VAULTS
            .get_or_init(|| Mutex::new(BTreeMap::new()))
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        Arc::clone(vaults.entry(service.to_string()).or_insert_with(|| {
            let legacy_service = service.to_string();
            Arc::new(Vault::new(
                Box::new(OsItem {
                    service: service.to_string(),
                }),
                Box::new(move |account| {
                    Box::new(LegacyItem {
                        service: legacy_service.clone(),
                        account: account.to_string(),
                    })
                }),
            ))
        }))
    }
}

#[cfg(target_os = "macos")]
pub use os::vault_for_service;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// The vault item: counts calls, can be told to fail writes.
    #[derive(Clone, Default)]
    struct FakeItem {
        text: Arc<Mutex<Option<String>>>,
        reads: Arc<AtomicUsize>,
        writes: Arc<AtomicUsize>,
        fail_reads: Arc<AtomicBool>,
        fail_writes: Arc<AtomicBool>,
        /// Reads after a write answer something else (a write that did not land).
        stale_readback: Arc<AtomicBool>,
    }

    impl FakeItem {
        fn with(text: &str) -> Self {
            let item = Self::default();
            *item.text.lock().unwrap() = Some(text.to_string());
            item
        }
        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
        fn writes(&self) -> usize {
            self.writes.load(Ordering::SeqCst)
        }
        fn json(&self) -> Option<serde_json::Value> {
            self.text
                .lock()
                .unwrap()
                .as_ref()
                .map(|t| serde_json::from_str(t).unwrap())
        }
    }

    impl KeychainBackend for FakeItem {
        fn read(&self) -> Result<Option<String>, KeychainUnavailable> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if self.fail_reads.load(Ordering::SeqCst) {
                return Err(KeychainUnavailable);
            }
            if self.stale_readback.load(Ordering::SeqCst) && self.writes() > 0 {
                return Ok(Some("{}".into()));
            }
            Ok(self.text.lock().unwrap().clone())
        }
        fn write(&self, secret: &str) -> Result<(), KeychainUnavailable> {
            if self.fail_writes.load(Ordering::SeqCst) {
                return Err(KeychainUnavailable);
            }
            self.writes.fetch_add(1, Ordering::SeqCst);
            *self.text.lock().unwrap() = Some(secret.to_string());
            Ok(())
        }
        fn delete(&self) -> Result<(), KeychainUnavailable> {
            Err(KeychainUnavailable)
        }
    }

    /// The legacy per-account items: counts reads per account, records any write or delete.
    #[derive(Clone, Default)]
    struct FakeLegacy {
        items: Arc<Mutex<BTreeMap<String, String>>>,
        reads: Arc<Mutex<Vec<String>>>,
        changed: Arc<AtomicUsize>,
    }

    impl FakeLegacy {
        fn with(pairs: &[(&str, &str)]) -> Self {
            let legacy = Self::default();
            for (account, secret) in pairs {
                legacy
                    .items
                    .lock()
                    .unwrap()
                    .insert((*account).into(), (*secret).into());
            }
            legacy
        }
        fn reads(&self) -> usize {
            self.reads.lock().unwrap().len()
        }
        fn untouched(&self) -> bool {
            self.changed.load(Ordering::SeqCst) == 0
        }
        fn factory(&self) -> LegacyItems {
            let legacy = self.clone();
            Box::new(move |account| {
                Box::new(LegacyAccount {
                    legacy: legacy.clone(),
                    account: account.to_string(),
                })
            })
        }
    }

    struct LegacyAccount {
        legacy: FakeLegacy,
        account: String,
    }

    impl KeychainBackend for LegacyAccount {
        fn read(&self) -> Result<Option<String>, KeychainUnavailable> {
            self.legacy.reads.lock().unwrap().push(self.account.clone());
            Ok(self
                .legacy
                .items
                .lock()
                .unwrap()
                .get(&self.account)
                .cloned())
        }
        fn write(&self, _: &str) -> Result<(), KeychainUnavailable> {
            self.legacy.changed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn delete(&self) -> Result<(), KeychainUnavailable> {
            self.legacy.changed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn vault(item: &FakeItem, legacy: &FakeLegacy) -> Vault {
        Vault::new(Box::new(item.clone()), legacy.factory())
    }

    const THREE: [(&str, &str); 3] = [
        ("alpha", "legacy-a"),
        ("beta", "legacy-b"),
        ("gamma", "legacy-c"),
    ];

    #[test]
    fn first_launch_in_one_pass_reads_each_legacy_item_once_and_writes_the_vault_once() {
        let item = FakeItem::default();
        let legacy = FakeLegacy::with(&THREE);
        let v = vault(&item, &legacy);
        let got = v.get_many(&["alpha", "beta", "gamma"]);
        let got: Vec<_> = got.into_iter().map(|r| r.unwrap()).collect();
        assert_eq!(
            got,
            vec![
                Some("legacy-a".to_string()),
                Some("legacy-b".to_string()),
                Some("legacy-c".to_string())
            ]
        );
        assert_eq!(legacy.reads(), 3, "each legacy item once");
        assert_eq!(item.writes(), 1, "one write for the whole pass");
        // one read to find the vault empty, one read back to compare
        assert_eq!(item.reads(), 2);
        assert!(
            legacy.untouched(),
            "legacy items are never written or deleted"
        );
        assert_eq!(
            item.json().unwrap(),
            serde_json::json!({"alpha": "legacy-a", "beta": "legacy-b", "gamma": "legacy-c"})
        );
    }

    #[test]
    fn second_launch_reads_only_the_vault() {
        let item = FakeItem::default();
        let legacy = FakeLegacy::with(&THREE);
        vault(&item, &legacy).get_many(&["alpha", "beta", "gamma"]);
        let (reads_before, writes_before, legacy_before) =
            (item.reads(), item.writes(), legacy.reads());

        // a new process: a new Vault over the same keychain
        let second = vault(&item, &legacy);
        for _ in 0..3 {
            for account in ["alpha", "beta", "gamma"] {
                assert!(second.get(account).unwrap().is_some());
            }
        }
        assert_eq!(
            item.reads() - reads_before,
            1,
            "the vault is read once per launch"
        );
        assert_eq!(item.writes(), writes_before, "nothing written");
        assert_eq!(
            legacy.reads(),
            legacy_before,
            "no legacy item is read again"
        );
    }

    #[test]
    fn stores_that_ask_one_at_a_time_migrate_each_once() {
        // How the app asks: each store reads its own account when it first needs it.
        let item = FakeItem::default();
        let legacy = FakeLegacy::with(&THREE);
        let v = Arc::new(vault(&item, &legacy));
        for account in ["alpha", "beta", "gamma"] {
            let store = crate::settings::KeyringCredentialStore::with_backend(Box::new(
                VaultAccount::new(Arc::clone(&v), account),
            ));
            use crate::settings::CredentialStore;
            assert!(store.get_secret().unwrap().is_some());
            assert!(store.get_secret().unwrap().is_some());
        }
        assert_eq!(legacy.reads(), 3);
        assert_eq!(item.writes(), 3, "one write per account asked, once ever");
        assert!(legacy.untouched());
        // and the next launch needs only the vault
        let before = (item.reads(), legacy.reads());
        let next = vault(&item, &legacy);
        for account in ["alpha", "beta", "gamma"] {
            assert!(next.get(account).unwrap().is_some());
        }
        assert_eq!((item.reads() - before.0, legacy.reads()), (1, before.1));
    }

    #[test]
    fn an_account_with_no_legacy_item_writes_nothing() {
        let item = FakeItem::default();
        let legacy = FakeLegacy::default();
        let v = vault(&item, &legacy);
        assert_eq!(v.get("alpha").unwrap(), None);
        assert_eq!(item.writes(), 0);
        assert_eq!(item.reads(), 1);
    }

    #[test]
    fn a_new_token_goes_to_the_vault_only() {
        let item = FakeItem::default();
        let legacy = FakeLegacy::with(&[("alpha", "old-legacy")]);
        let v = vault(&item, &legacy);
        v.set("alpha", "new-token").unwrap();
        assert_eq!(v.get("alpha").unwrap().as_deref(), Some("new-token"));
        assert_eq!(item.writes(), 1);
        assert_eq!(item.reads(), 1, "only the first load");
        assert_eq!(legacy.reads(), 0, "legacy not even read");
        assert!(legacy.untouched());
        assert_eq!(
            item.json().unwrap(),
            serde_json::json!({"alpha": "new-token"})
        );
        // a later launch sees the new token, not the legacy one
        assert_eq!(
            vault(&item, &legacy).get("alpha").unwrap().as_deref(),
            Some("new-token")
        );
    }

    #[test]
    fn a_new_token_keeps_other_accounts_that_are_not_migrated_yet() {
        let item = FakeItem::default();
        let legacy = FakeLegacy::with(&[("alpha", "a"), ("beta", "b")]);
        let v = vault(&item, &legacy);
        v.set("alpha", "new").unwrap();
        // beta was never read: it still migrates from the legacy item, nothing is lost
        assert_eq!(v.get("beta").unwrap().as_deref(), Some("b"));
        assert_eq!(
            item.json().unwrap(),
            serde_json::json!({"alpha": "new", "beta": "b"})
        );
    }

    #[test]
    fn delete_goes_to_the_vault_and_the_legacy_copy_does_not_come_back() {
        let item = FakeItem::default();
        let legacy = FakeLegacy::with(&[("alpha", "old")]);
        let v = vault(&item, &legacy);
        assert_eq!(v.get("alpha").unwrap().as_deref(), Some("old"));
        v.delete("alpha").unwrap();
        assert_eq!(v.get("alpha").unwrap(), None);
        assert!(legacy.untouched(), "legacy items are never deleted");
        // next launch: the legacy item still exists, the vault says deleted
        let legacy_reads = legacy.reads();
        let next = vault(&item, &legacy);
        assert_eq!(next.get("alpha").unwrap(), None);
        assert_eq!(
            legacy.reads(),
            legacy_reads,
            "deleted means not asked again"
        );
        // deleting twice, or something never saved, is fine
        v.delete("alpha").unwrap();
        v.delete("never-saved").unwrap();
        // saving again brings it back
        next.set("alpha", "again").unwrap();
        assert_eq!(next.get("alpha").unwrap().as_deref(), Some("again"));
    }

    #[test]
    fn deleting_the_only_secret_leaves_a_vault_item_with_no_secret_in_it() {
        let item = FakeItem::default();
        let legacy = FakeLegacy::default();
        let v = vault(&item, &legacy);
        v.set("alpha", "token").unwrap();
        v.delete("alpha").unwrap();
        assert_eq!(item.json().unwrap(), serde_json::json!({"alpha": null}));
        assert!(!item
            .text
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .contains("token"));
        assert_eq!(v.get("alpha").unwrap(), None);
    }

    #[test]
    fn a_corrupt_vault_is_never_overwritten_and_legacy_still_reads() {
        for corrupt in [
            "not json",
            "[\"a\"]",
            "{\"alpha\": 5}",
            "",
            "{\"alpha\": \"x\"",
        ] {
            let item = FakeItem::with(corrupt);
            let legacy = FakeLegacy::with(&[("alpha", "legacy-a")]);
            let v = vault(&item, &legacy);
            assert_eq!(v.get("alpha").unwrap().as_deref(), Some("legacy-a"));
            assert!(v.set("alpha", "new").is_err());
            assert!(v.delete("alpha").is_err());
            assert_eq!(item.writes(), 0, "nothing written over the corrupt item");
            assert_eq!(item.text.lock().unwrap().as_deref(), Some(corrupt));
            assert_eq!(item.reads(), 1, "and it is not asked for again");
        }
    }

    #[test]
    fn a_write_that_fails_keeps_the_old_vault() {
        let item = FakeItem::default();
        let legacy = FakeLegacy::default();
        let v = vault(&item, &legacy);
        v.set("alpha", "one").unwrap();
        let before = item.text.lock().unwrap().clone();
        item.fail_writes.store(true, Ordering::SeqCst);
        assert!(v.set("alpha", "two").is_err());
        assert!(v.set("beta", "other").is_err());
        assert!(v.delete("alpha").is_err());
        assert_eq!(
            *item.text.lock().unwrap(),
            before,
            "the keychain is unchanged"
        );
        assert_eq!(
            v.get("alpha").unwrap().as_deref(),
            Some("one"),
            "so is the copy in memory"
        );
        assert_eq!(v.get("beta").unwrap(), None);
        // and it works again once the keychain does
        item.fail_writes.store(false, Ordering::SeqCst);
        v.set("alpha", "two").unwrap();
        assert_eq!(v.get("alpha").unwrap().as_deref(), Some("two"));
    }

    #[test]
    fn a_migration_write_that_fails_still_answers_from_legacy_and_does_not_retry() {
        let item = FakeItem::default();
        item.fail_writes.store(true, Ordering::SeqCst);
        let legacy = FakeLegacy::with(&[("alpha", "a"), ("beta", "b")]);
        let v = vault(&item, &legacy);
        assert_eq!(v.get("alpha").unwrap().as_deref(), Some("a"));
        assert_eq!(v.get("beta").unwrap().as_deref(), Some("b"));
        assert_eq!(item.writes(), 0);
        assert!(legacy.untouched());
    }

    #[test]
    fn a_migration_that_does_not_read_back_is_not_trusted() {
        let item = FakeItem::default();
        item.stale_readback.store(true, Ordering::SeqCst);
        let legacy = FakeLegacy::with(&[("alpha", "a"), ("beta", "b")]);
        let v = vault(&item, &legacy);
        assert_eq!(v.get("alpha").unwrap().as_deref(), Some("a"));
        let writes = item.writes();
        assert_eq!(v.get("beta").unwrap().as_deref(), Some("b"));
        assert_eq!(
            item.writes(),
            writes,
            "no further migration writes this launch"
        );
        assert!(legacy.untouched());
    }

    #[test]
    fn an_unavailable_vault_item_is_an_error_and_is_tried_again_later() {
        let item = FakeItem::default();
        item.fail_reads.store(true, Ordering::SeqCst);
        let legacy = FakeLegacy::with(&[("alpha", "a")]);
        let v = vault(&item, &legacy);
        assert!(v.get("alpha").is_err());
        assert!(v.set("alpha", "x").is_err());
        assert!(v.delete("alpha").is_err());
        assert_eq!(item.writes(), 0);
        assert_eq!(
            legacy.reads(),
            0,
            "no legacy read while the vault cannot be told"
        );
        item.fail_reads.store(false, Ordering::SeqCst);
        assert_eq!(v.get("alpha").unwrap().as_deref(), Some("a"));
    }

    #[test]
    fn a_missing_or_empty_vault_with_legacy_items_migrates_and_loses_nothing() {
        for start in [None, Some("{}")] {
            let item = match start {
                Some(text) => FakeItem::with(text),
                None => FakeItem::default(),
            };
            let legacy = FakeLegacy::with(&THREE);
            let v = vault(&item, &legacy);
            assert_eq!(v.get("beta").unwrap().as_deref(), Some("legacy-b"));
            assert_eq!(
                item.json().unwrap(),
                serde_json::json!({"beta": "legacy-b"})
            );
        }
    }

    #[test]
    fn two_threads_writing_lose_nothing() {
        let item = FakeItem::default();
        let legacy = FakeLegacy::default();
        let v = Arc::new(vault(&item, &legacy));
        let handles: Vec<_> = (0..2)
            .map(|t| {
                let v = Arc::clone(&v);
                std::thread::spawn(move || {
                    for i in 0..25 {
                        v.set(&format!("acct-{t}-{i}"), &format!("v-{t}-{i}"))
                            .unwrap();
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let json = item.json().unwrap();
        assert_eq!(json.as_object().unwrap().len(), 50);
        // a fresh process sees all 50
        let next = vault(&item, &legacy);
        for t in 0..2 {
            for i in 0..25 {
                assert_eq!(
                    next.get(&format!("acct-{t}-{i}")).unwrap(),
                    Some(format!("v-{t}-{i}"))
                );
            }
        }
    }

    /// macOS only, and touches no keychain item: opening a vault reads nothing.
    #[cfg(target_os = "macos")]
    #[test]
    fn every_store_of_a_service_shares_one_vault() {
        let a = vault_for_service("StagePilot-vault-share-test");
        let b = vault_for_service("StagePilot-vault-share-test");
        let other = vault_for_service("StagePilot-vault-share-test-other");
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!Arc::ptr_eq(&a, &other));
    }
}
