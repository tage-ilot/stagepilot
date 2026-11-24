//! The service the operator picked instead of the upcoming one ("the pin").
//!
//! The pin lives in its own small state file next to the settings (`service-pin.json`), NOT in
//! `settings.json`: older builds reject settings keys they do not know, and a pin must never make
//! them refuse the settings. The file is replaced atomically; a damaged file is ignored (logged
//! once) and counts as "no pin".
//!
//! [`PlanPicker`] is what a Services source (Planning Center, Demo) offers the routes: the list of
//! plans (the past 20 days and from today on) and the pin itself. Loading a plan never happens here: the routes publish
//! `ServicePlanSelectionRequested` and the source loads as it always does.

use async_trait::async_trait;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const SOURCE_PLANNING_CENTER: &str = "planning_center";
pub const SOURCE_DEMO: &str = "demo";
/// How far ahead the picker lists plans.
pub const PICKER_DAYS: i64 = 180;
/// How many days back the picker lists plans (to review a service that has gone by).
pub const PICKER_PAST_DAYS: i64 = 20;
/// The most plans from today on the picker lists. The plans of the past days come on top of this,
/// so the upcoming ones are never pushed out.
pub const PICKER_MAX_PLANS: usize = 60;
/// What a plan list is kept for before Planning Center is asked again.
pub const PICKER_CACHE_SECONDS: u64 = 60;
/// Notice texts (one-time, neutral).
pub const NOTICE_PIN_PASSED: &str = "Back to the upcoming service";
pub const NOTICE_PIN_VANISHED: &str =
    "The plan you picked is not in Planning Center any more; back to the upcoming service";

fn one() -> u8 {
    1
}

/// What is saved in `service-pin.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePin {
    #[serde(default = "one")]
    pub schema_version: u8,
    /// Which Services source made the pin (`planning_center` or `demo`); a pin only counts for
    /// the source that made it.
    pub source: String,
    pub plan_id: String,
    pub date: NaiveDate,
    #[serde(default)]
    pub service_type_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// Set when the pin is a service that had already gone by (a review): the pin holds through
    /// this date (the day it was picked) and ends at the next midnight. Older files do not have it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held_until: Option<NaiveDate>,
}

impl ServicePin {
    pub fn new(
        source: &str,
        plan_id: &str,
        date: NaiveDate,
        service_type_id: Option<String>,
        title: Option<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            source: source.to_string(),
            plan_id: plan_id.to_string(),
            date,
            service_type_id,
            title,
            held_until: None,
        }
    }

    /// A pin of a service dated before `today` (a review) is held through `today` and ends at the
    /// next midnight; any other pin is returned as it was.
    pub fn held_for_review(mut self, today: NaiveDate) -> Self {
        if self.date < today {
            self.held_until = Some(today);
        }
        self
    }

    /// A pin holds through the later of its date and `held_until` in the configured time zone
    /// (`today` is that zone's date) and ends when that day has passed.
    pub fn is_expired(&self, today: NaiveDate) -> bool {
        self.held_until
            .map_or(self.date, |held| held.max(self.date))
            < today
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PinStoreError(pub String);

pub trait ServicePinStore: Send + Sync {
    /// `Ok(None)`: no pin. `Err`: the file is there but cannot be read as a pin.
    fn load(&self) -> Result<Option<ServicePin>, PinStoreError>;
    fn save(&self, pin: &ServicePin) -> Result<(), PinStoreError>;
    fn clear(&self) -> Result<(), PinStoreError>;
}

/// `service-pin.json`, replaced atomically (temp file, fsync, rename).
pub struct FileServicePinStore {
    pub path: PathBuf,
}

impl FileServicePinStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl ServicePinStore for FileServicePinStore {
    fn load(&self) -> Result<Option<ServicePin>, PinStoreError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(PinStoreError("The service pin file cannot be read.".into())),
        };
        let invalid = || PinStoreError("The service pin file is damaged.".into());
        let pin: ServicePin = serde_json::from_str(&text).map_err(|_| invalid())?;
        if pin.schema_version != 1 || pin.plan_id.is_empty() {
            return Err(invalid());
        }
        Ok(Some(pin))
    }

    fn save(&self, pin: &ServicePin) -> Result<(), PinStoreError> {
        use std::io::Write;
        let failed = || PinStoreError("The service pin could not be saved.".into());
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|_| failed())?;
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let temporary = parent.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
        let write = || -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            let mut text = serde_json::to_string_pretty(pin).unwrap_or_default();
            text.push('\n');
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, &self.path)
        };
        write().map_err(|_| {
            let _ = std::fs::remove_file(&temporary);
            failed()
        })
    }

    fn clear(&self) -> Result<(), PinStoreError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(PinStoreError(
                "The service pin could not be cleared.".into(),
            )),
        }
    }
}

/// In-memory pin for tests and ephemeral instances.
#[derive(Default)]
pub struct MemoryServicePinStore {
    pin: Mutex<Option<ServicePin>>,
}

impl ServicePinStore for MemoryServicePinStore {
    fn load(&self) -> Result<Option<ServicePin>, PinStoreError> {
        Ok(self.pin.lock().unwrap_or_else(|p| p.into_inner()).clone())
    }
    fn save(&self, pin: &ServicePin) -> Result<(), PinStoreError> {
        *self.pin.lock().unwrap_or_else(|p| p.into_inner()) = Some(pin.clone());
        Ok(())
    }
    fn clear(&self) -> Result<(), PinStoreError> {
        *self.pin.lock().unwrap_or_else(|p| p.into_inner()) = None;
        Ok(())
    }
}

/// `STAGEPILOT_SERVICE_PIN_PATH`, else `service-pin.json` beside the settings file.
pub fn default_service_pin_path(environ: &BTreeMap<String, String>) -> PathBuf {
    if let Some(value) = environ
        .get("STAGEPILOT_SERVICE_PIN_PATH")
        .filter(|v| !v.is_empty())
    {
        return PathBuf::from(value);
    }
    crate::settings::default_settings_path(environ).with_file_name("service-pin.json")
}

struct Held {
    loaded: bool,
    pin: Option<ServicePin>,
    warned: bool,
}

/// The one place the running app reads and writes the pin: reads the file once (a damaged file
/// is logged once and ignored), keeps the answer in memory, writes through before it changes.
pub struct PinKeeper {
    store: Arc<dyn ServicePinStore>,
    held: Mutex<Held>,
}

impl PinKeeper {
    pub fn new(store: Arc<dyn ServicePinStore>) -> Arc<Self> {
        Arc::new(Self {
            store,
            held: Mutex::new(Held {
                loaded: false,
                pin: None,
                warned: false,
            }),
        })
    }

    pub fn in_memory() -> Arc<Self> {
        Self::new(Arc::new(MemoryServicePinStore::default()))
    }

    fn with_held<T>(&self, f: impl FnOnce(&mut Held) -> T) -> T {
        let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
        if !held.loaded {
            held.loaded = true;
            match self.store.load() {
                Ok(pin) => held.pin = pin,
                Err(error) => {
                    held.pin = None;
                    if !held.warned {
                        held.warned = true;
                        tracing::warn!(reason = %error, "service_pin_ignored");
                    }
                }
            }
        }
        f(&mut held)
    }

    /// The saved pin if `source` made it.
    pub fn get(&self, source: &str) -> Option<ServicePin> {
        self.with_held(|h| h.pin.clone().filter(|p| p.source == source))
    }

    /// Replace the pin. The file is written first; on failure nothing changes.
    pub fn set(&self, pin: ServicePin) -> Result<(), String> {
        self.with_held(|h| {
            self.store.save(&pin).map_err(|e| e.0)?;
            h.pin = Some(pin);
            Ok(())
        })
    }

    pub fn clear(&self) -> Result<(), String> {
        self.with_held(|h| {
            self.store.clear().map_err(|e| e.0)?;
            h.pin = None;
            Ok(())
        })
    }

    /// Put back what `get` returned before a change (a pin made by this source or none).
    pub fn restore(&self, previous: Option<ServicePin>) -> Result<(), String> {
        match previous {
            Some(pin) => self.set(pin),
            None => self.clear(),
        }
    }
}

/// One plan in the picker's list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpcomingPlan {
    pub id: String,
    pub date: NaiveDate,
    pub title: String,
    pub service_type_id: String,
    pub service_type_name: String,
}

/// `GET /api/v1/planning-center/plans/upcoming`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpcomingPlans {
    pub plans: Vec<UpcomingPlan>,
    pub upcoming_id: Option<String>,
    pub pinned_id: Option<String>,
    pub pinned_date: Option<NaiveDate>,
}

/// What a Services source offers the picker routes.
#[async_trait]
pub trait PlanPicker: Send + Sync {
    /// Plans from `PICKER_PAST_DAYS` days ago through `PICKER_DAYS` days ahead, by date: at most
    /// `PICKER_MAX_PLANS` from today on, and the past ones on top of them. `Err` is a message for the operator; the loaded plan is never touched by a failed list.
    async fn upcoming_plans(&self) -> Result<UpcomingPlans, String>;
    /// Today's date in the configured time zone.
    fn today(&self) -> NaiveDate;
    /// The pin this source holds (expired or not).
    fn pin(&self) -> Option<ServicePin>;
    fn set_pin(&self, plan: &UpcomingPlan) -> Result<(), String>;
    fn clear_pin(&self) -> Result<(), String>;
    fn restore_pin(&self, previous: Option<ServicePin>) -> Result<(), String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    fn pin() -> ServicePin {
        ServicePin::new(
            SOURCE_PLANNING_CENTER,
            "plan-9",
            date(29),
            Some("42".into()),
            Some("Advent".into()),
        )
    }

    #[test]
    fn a_pin_holds_through_its_date_and_ends_after_it() {
        let p = pin();
        assert!(!p.is_expired(date(28)));
        assert!(!p.is_expired(date(29)));
        assert!(p.is_expired(date(30)));
    }

    #[test]
    fn a_review_pin_of_a_past_plan_holds_through_the_day_it_was_picked() {
        let review = ServicePin::new(SOURCE_PLANNING_CENTER, "plan-1", date(10), None, None)
            .held_for_review(date(23));
        assert_eq!(review.held_until, Some(date(23)));
        assert!(!review.is_expired(date(23)));
        assert!(
            review.is_expired(date(24)),
            "the next local midnight drops it"
        );
        // A pin of today or later is not a review and expires the day after its date.
        let today = pin().held_for_review(date(23));
        assert_eq!(today.held_until, None);
        let same_day = ServicePin::new(SOURCE_PLANNING_CENTER, "p", date(23), None, None)
            .held_for_review(date(23));
        assert_eq!(same_day.held_until, None);
        assert!(!same_day.is_expired(date(23)) && same_day.is_expired(date(24)));
    }

    #[test]
    fn a_pin_file_from_before_review_pins_still_loads_and_expires_as_before() {
        let dir = tempfile_dir("old-file");
        let path = dir.join("service-pin.json");
        std::fs::write(
            &path,
            r#"{"schema_version":1,"source":"planning_center","plan_id":"plan-9","date":"2026-11-29"}"#,
        )
        .unwrap();
        let loaded = FileServicePinStore::new(&path).load().unwrap().unwrap();
        assert_eq!(loaded.held_until, None);
        assert!(!loaded.is_expired(date(29)));
        assert!(loaded.is_expired(date(30)));
        // A pin without the field is saved without it, so an older build reads it too.
        FileServicePinStore::new(&path).save(&loaded).unwrap();
        assert!(!std::fs::read_to_string(&path)
            .unwrap()
            .contains("held_until"));
    }

    #[test]
    fn a_review_pin_round_trips_through_the_file() {
        let dir = tempfile_dir("review");
        let store = FileServicePinStore::new(dir.join("service-pin.json"));
        let review = pin().held_for_review(date(30));
        store.save(&review).unwrap();
        assert_eq!(store.load().unwrap(), Some(review));
    }

    #[test]
    fn the_file_store_round_trips_and_clears() {
        let dir = tempfile_dir("round-trip");
        let store = FileServicePinStore::new(dir.join("service-pin.json"));
        assert_eq!(store.load().unwrap(), None);
        store.save(&pin()).unwrap();
        assert_eq!(store.load().unwrap(), Some(pin()));
        // No temporary file is left behind.
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["service-pin.json".to_string()]);
        store.clear().unwrap();
        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn a_damaged_file_is_ignored_and_counts_as_no_pin() {
        let dir = tempfile_dir("damaged");
        let path = dir.join("service-pin.json");
        for text in [
            "{not json",
            "{}",
            r#"{"source":"demo","plan_id":"","date":"2026-11-29"}"#,
        ] {
            std::fs::write(&path, text).unwrap();
            let keeper = PinKeeper::new(Arc::new(FileServicePinStore::new(&path)));
            assert_eq!(keeper.get(SOURCE_PLANNING_CENTER), None, "{text}");
            assert_eq!(keeper.get(SOURCE_DEMO), None, "{text}");
        }
        // A new pin replaces the damaged file.
        let keeper = PinKeeper::new(Arc::new(FileServicePinStore::new(&path)));
        keeper.set(pin()).unwrap();
        let again = PinKeeper::new(Arc::new(FileServicePinStore::new(&path)));
        assert_eq!(again.get(SOURCE_PLANNING_CENTER), Some(pin()));
    }

    #[test]
    fn a_pin_only_counts_for_the_source_that_made_it() {
        let keeper = PinKeeper::in_memory();
        keeper.set(pin()).unwrap();
        assert_eq!(keeper.get(SOURCE_PLANNING_CENTER), Some(pin()));
        assert_eq!(keeper.get(SOURCE_DEMO), None);
    }

    #[test]
    fn restore_puts_back_the_previous_pin_or_none() {
        let keeper = PinKeeper::in_memory();
        keeper.restore(None).unwrap();
        assert_eq!(keeper.get(SOURCE_PLANNING_CENTER), None);
        keeper.set(pin()).unwrap();
        keeper.restore(None).unwrap();
        assert_eq!(keeper.get(SOURCE_PLANNING_CENTER), None);
        keeper.restore(Some(pin())).unwrap();
        assert_eq!(keeper.get(SOURCE_PLANNING_CENTER), Some(pin()));
    }

    #[test]
    fn the_default_path_sits_beside_the_settings_file() {
        let mut env = BTreeMap::new();
        env.insert(
            "STAGEPILOT_SETTINGS_PATH".to_string(),
            "/data/sp/settings.json".to_string(),
        );
        assert_eq!(
            default_service_pin_path(&env),
            PathBuf::from("/data/sp/service-pin.json")
        );
        env.insert(
            "STAGEPILOT_SERVICE_PIN_PATH".to_string(),
            "/elsewhere/pin.json".to_string(),
        );
        assert_eq!(
            default_service_pin_path(&env),
            PathBuf::from("/elsewhere/pin.json")
        );
    }

    fn tempfile_dir(_name: &str) -> PathBuf {
        tempfile::tempdir().unwrap().keep()
    }
}
