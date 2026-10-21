//! Critical-error alert store with a 15 minute per-alert-key cooldown/dedup engine: port of
//! `services/critical_alerts.py`. The control-plane forwarding sits behind [`AlertSender`]; the
//! enrolment identity it needs (`DesktopBootstrapStore`) belongs to the remote-desktop phase.

use rusqlite::{params, Connection, TransactionBehavior};
use std::path::PathBuf;

use crate::auth::{system_clock, Clock};

pub const COOLDOWN_SECONDS: f64 = 15.0 * 60.0;
pub const STATUS_OPEN: &str = "open";
pub const STATUS_ACKNOWLEDGED: &str = "acknowledged";

#[derive(Debug, Clone, PartialEq)]
pub struct CriticalAlert {
    pub id: i64,
    pub category: String,
    pub subject: String,
    pub message: String,
    pub severity: String,
    pub first_seen: f64,
    pub last_seen: f64,
    pub count: i64,
    pub status: String,
    pub last_notified: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertResult {
    pub alert: CriticalAlert,
    pub should_notify: bool,
}

pub type AlertError = rusqlite::Error;

pub struct CriticalAlertStore {
    pub path: PathBuf,
    clock: Clock,
}

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS critical_alerts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        category TEXT NOT NULL,
        subject TEXT NOT NULL,
        message TEXT NOT NULL,
        severity TEXT NOT NULL CHECK(severity IN ('warning', 'critical')),
        first_seen REAL NOT NULL,
        last_seen REAL NOT NULL,
        count INTEGER NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('open', 'acknowledged')),
        last_notified REAL
    );
    CREATE INDEX IF NOT EXISTS idx_critical_alerts_key
        ON critical_alerts(category, subject, status);
";

const COLUMNS: &str =
    "id, category, subject, message, severity, first_seen, last_seen, count, status, last_notified";

fn row_to_alert(r: &rusqlite::Row<'_>) -> rusqlite::Result<CriticalAlert> {
    Ok(CriticalAlert {
        id: r.get(0)?,
        category: r.get(1)?,
        subject: r.get(2)?,
        message: r.get(3)?,
        severity: r.get(4)?,
        first_seen: r.get(5)?,
        last_seen: r.get(6)?,
        count: r.get(7)?,
        status: r.get(8)?,
        last_notified: r.get(9)?,
    })
}

impl CriticalAlertStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self::with_clock(path, system_clock())
    }

    pub fn with_clock(path: impl Into<PathBuf>, clock: Clock) -> Self {
        Self {
            path: path.into(),
            clock,
        }
    }

    fn open(&self) -> rusqlite::Result<Connection> {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(&self.path)?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        conn.execute_batch(SCHEMA)?;
        Ok(conn)
    }

    fn tx<T>(
        &self,
        f: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    /// Record an occurrence and decide whether it should notify now.
    ///
    /// * no open row for `(category, subject)`: insert a fresh row, notify;
    /// * open row and the cooldown has elapsed since it last notified: bump the counter, notify;
    /// * open row within the cooldown: bump the counter, do not notify.
    pub fn record(
        &self,
        category: &str,
        subject: &str,
        message: &str,
        severity: &str,
    ) -> rusqlite::Result<AlertResult> {
        let now = (self.clock)();
        self.tx(|db| {
            let existing: Option<(i64, Option<f64>)> = db
                .query_row(
                    "SELECT id, last_notified FROM critical_alerts \
                     WHERE category=? AND subject=? AND status=?",
                    params![category, subject, STATUS_OPEN],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok();
            let Some((id, last_notified)) = existing else {
                db.execute(
                    "INSERT INTO critical_alerts \
                     (category, subject, message, severity, first_seen, last_seen, \
                     count, status, last_notified) VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?)",
                    params![
                        category,
                        subject,
                        message,
                        severity,
                        now,
                        now,
                        STATUS_OPEN,
                        now
                    ],
                )?;
                let id = db.last_insert_rowid();
                let alert = db.query_row(
                    &format!("SELECT {COLUMNS} FROM critical_alerts WHERE id=?"),
                    params![id],
                    row_to_alert,
                )?;
                return Ok(AlertResult {
                    alert,
                    should_notify: true,
                });
            };
            let should_notify = last_notified.is_none_or(|n| now - n >= COOLDOWN_SECONDS);
            if should_notify {
                db.execute(
                    "UPDATE critical_alerts SET count=count+1, last_seen=?, last_notified=?, \
                     message=?, severity=? WHERE id=?",
                    params![now, now, message, severity, id],
                )?;
            } else {
                db.execute(
                    "UPDATE critical_alerts SET count=count+1, last_seen=?, message=?, \
                     severity=? WHERE id=?",
                    params![now, message, severity, id],
                )?;
            }
            let alert = db.query_row(
                &format!("SELECT {COLUMNS} FROM critical_alerts WHERE id=?"),
                params![id],
                row_to_alert,
            )?;
            Ok(AlertResult {
                alert,
                should_notify,
            })
        })
    }

    pub fn list_open(&self) -> rusqlite::Result<Vec<CriticalAlert>> {
        self.tx(|db| {
            let mut stmt = db.prepare(&format!(
                "SELECT {COLUMNS} FROM critical_alerts WHERE status=? ORDER BY last_seen DESC"
            ))?;
            let rows = stmt.query_map(params![STATUS_OPEN], row_to_alert)?;
            rows.collect()
        })
    }

    pub fn get(&self, alert_id: i64) -> rusqlite::Result<Option<CriticalAlert>> {
        self.tx(|db| {
            match db.query_row(
                &format!("SELECT {COLUMNS} FROM critical_alerts WHERE id=?"),
                params![alert_id],
                row_to_alert,
            ) {
                Ok(a) => Ok(Some(a)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e),
            }
        })
    }

    pub fn acknowledge(&self, alert_id: i64) -> rusqlite::Result<Option<CriticalAlert>> {
        self.tx(|db| {
            db.execute(
                "UPDATE critical_alerts SET status=? WHERE id=?",
                params![STATUS_ACKNOWLEDGED, alert_id],
            )?;
            match db.query_row(
                &format!("SELECT {COLUMNS} FROM critical_alerts WHERE id=?"),
                params![alert_id],
                row_to_alert,
            ) {
                Ok(a) => Ok(Some(a)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e),
            }
        })
    }
}

/// `datetime.fromtimestamp(ts, UTC).isoformat()` with `+00:00` replaced by `Z`.
pub fn utc_isoformat(timestamp: f64) -> String {
    use chrono::{DateTime, SecondsFormat, Utc};
    let micros = (timestamp * 1e6).round() as i64;
    let dt: DateTime<Utc> = DateTime::from_timestamp_micros(micros).unwrap_or(DateTime::UNIX_EPOCH);
    if micros % 1_000_000 == 0 {
        dt.to_rfc3339_opts(SecondsFormat::Secs, true)
    } else {
        dt.to_rfc3339_opts(SecondsFormat::Micros, true)
    }
}

/// Payload accepted by the control-plane installation alert endpoint.
pub fn build_control_plane_payload(alert: &CriticalAlert) -> serde_json::Value {
    serde_json::json!({
        "category": alert.category,
        "subject": alert.subject,
        "message": alert.message,
        "severity": alert.severity,
        "first_seen": utc_isoformat(alert.first_seen),
        "last_seen": utc_isoformat(alert.last_seen),
        "count": alert.count,
    })
}

pub type AlertSender<'a> = &'a dyn Fn(&serde_json::Value) -> bool;

/// Record an alert and forward fresh notifications. Cooldown/dedup always runs locally; with no
/// sender (no enrolled control-plane identity) forwarding is skipped without affecting the caller.
pub fn alert_critical(
    store: &CriticalAlertStore,
    category: &str,
    subject: &str,
    message: &str,
    severity: &str,
    sender: Option<AlertSender<'_>>,
) -> rusqlite::Result<AlertResult> {
    let result = store.record(category, subject, message, severity)?;
    if result.should_notify {
        if let Some(send) = sender {
            send(&build_control_plane_payload(&result.alert));
        }
    }
    Ok(result)
}
