//! Authentication: port of `services/dashboard_auth.py` (browser-dashboard PIN and sessions) and
//! `services/remote_auth.py` (remote users, Argon2 passwords, sessions, throttling).
//!
//! * Dashboard PIN hashes are `pbkdf2_sha256$ITER$salt$digest` (urlsafe base64), byte-compatible
//!   with the Python app. An unset hash means the default PIN `1234`.
//! * Remote passwords are Argon2id PHC strings (`m=65536,t=3,p=1`), verified against hashes written
//!   by `argon2-cffi` and vice versa.
//! * Every secret comparison is constant time (`subtle`). The Python dashboard PIN has **no**
//!   lockout; the only lockout in the app is the remote login throttle ([`RemoteStore`]).

use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, PasswordHash, Version};
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use hmac::{Hmac, Mac};
use rand::RngCore;
use rusqlite::{params, Connection, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;

pub const DEFAULT_DASHBOARD_PIN: &str = "1234";
pub const PIN_ITERATIONS: u32 = 210_000;
pub const SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);
pub const SESSION_SECONDS: f64 = 12.0 * 60.0 * 60.0;

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

fn token_urlsafe(nbytes: usize) -> String {
    let mut bytes = vec![0u8; nbytes];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

// ---- dashboard PIN ----------------------------------------------------------------------------

fn pbkdf2_digest(pin: &str, salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(pin.as_bytes(), salt, iterations, &mut out);
    out
}

pub fn hash_dashboard_pin(pin: &str) -> String {
    let mut salt = [0u8; 16];
    rand::rng().fill_bytes(&mut salt);
    let digest = pbkdf2_digest(pin, &salt, PIN_ITERATIONS);
    format!(
        "pbkdf2_sha256${PIN_ITERATIONS}${}${}",
        URL_SAFE.encode(salt),
        URL_SAFE.encode(digest)
    )
}

/// Decode urlsafe base64 the way Python's `urlsafe_b64decode` does for well-formed input.
fn decode_urlsafe(value: &str) -> Option<Vec<u8>> {
    URL_SAFE
        .decode(value)
        .ok()
        .or_else(|| URL_SAFE_NO_PAD.decode(value.trim_end_matches('=')).ok())
}

pub fn verify_dashboard_pin(pin: &str, encoded: Option<&str>) -> bool {
    let Some(encoded) = encoded else {
        return constant_time_eq(pin, DEFAULT_DASHBOARD_PIN);
    };
    let mut parts = encoded.splitn(4, '$');
    let (Some(algorithm), Some(iterations), Some(salt), Some(expected)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    if algorithm != "pbkdf2_sha256" {
        return false;
    }
    let (Ok(iterations), Some(salt)) = (iterations.parse::<u32>(), decode_urlsafe(salt)) else {
        return false;
    };
    if iterations == 0 {
        return false;
    }
    let digest = URL_SAFE.encode(pbkdf2_digest(pin, &salt, iterations));
    constant_time_eq(&digest, expected)
}

/// Short-lived browser sessions, in memory. Tokens are 32 random bytes (urlsafe base64).
#[derive(Default)]
pub struct DashboardSessionStore {
    sessions: Mutex<HashMap<String, Instant>>,
}

impl DashboardSessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&self) -> String {
        let token = token_urlsafe(32);
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(token.clone(), Instant::now() + SESSION_TTL);
        token
    }

    pub fn create_with_ttl(&self, ttl: Duration) -> String {
        let token = token_urlsafe(32);
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(token.clone(), Instant::now() + ttl);
        token
    }

    pub fn valid(&self, token: Option<&str>) -> bool {
        let Some(token) = token.filter(|t| !t.is_empty()) else {
            return false;
        };
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        // Compare against every stored token in constant time, so lookup timing does not reveal
        // how much of a guessed token matched.
        let mut found: Option<(String, Instant)> = None;
        for (stored, expires) in sessions.iter() {
            if constant_time_eq(stored, token) {
                found = Some((stored.clone(), *expires));
            }
        }
        match found {
            None => false,
            Some((stored, expires)) => {
                if expires <= Instant::now() {
                    sessions.remove(&stored);
                    false
                } else {
                    true
                }
            }
        }
    }

    pub fn clear(&self) {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }
}

// ---- remote auth ------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RemoteRole {
    Viewer,
    Operator,
}

impl RemoteRole {
    pub fn as_str(self) -> &'static str {
        match self {
            RemoteRole::Viewer => "Viewer",
            RemoteRole::Operator => "Operator",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "Viewer" => Some(RemoteRole::Viewer),
            "Operator" => Some(RemoteRole::Operator),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{detail}")]
pub struct RemoteAuthError {
    pub status: u16,
    pub detail: String,
}

impl RemoteAuthError {
    fn new(status: u16, detail: &str) -> Self {
        Self {
            status,
            detail: detail.to_string(),
        }
    }
    fn storage(error: rusqlite::Error) -> Self {
        tracing::warn!(%error, "remote_store_failed");
        Self::new(503, "Remote storage unavailable.")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteSession {
    pub user_id: String,
    pub email: String,
    pub role: RemoteRole,
    pub expires_at: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteUser {
    pub id: String,
    pub email: String,
    pub role: String,
    pub enabled: bool,
}

pub fn digest(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

pub fn csrf_token(token: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(token.as_bytes()).expect("HMAC accepts any key length");
    mac.update(b"stagepilot-remote-csrf-v1");
    hex::encode(mac.finalize().into_bytes())
}

pub fn normalize_email(value: &str) -> Result<String, RemoteAuthError> {
    let value = value.trim().to_lowercase();
    let bad = || RemoteAuthError::new(422, "A valid email address is required.");
    if value.chars().count() > 254
        || value.matches('@').count() != 1
        || value.chars().any(char::is_whitespace)
    {
        return Err(bad());
    }
    let (local, domain) = value.split_once('@').ok_or_else(bad)?;
    if local.is_empty() || domain.is_empty() || !domain.contains('.') || domain.starts_with('.') {
        return Err(bad());
    }
    Ok(value)
}

fn argon2_hasher() -> Argon2<'static> {
    // argon2-cffi PasswordHasher(time_cost=3, memory_cost=65536, parallelism=1, type=ID).
    let params = Params::new(65536, 3, 1, Some(32)).expect("valid Argon2 parameters");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Hash with the app's Argon2id parameters, producing a standard PHC string.
pub fn hash_password(password: &str) -> Result<String, RemoteAuthError> {
    argon2_hasher()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|_| RemoteAuthError::new(503, "Remote storage unavailable."))
}

/// Verify against any Argon2 PHC string (the parameters are read from the hash itself).
pub fn verify_password(password: &str, encoded: &str) -> bool {
    let Ok(hash) = PasswordHash::new(encoded) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &hash)
        .is_ok()
}

/// `check_needs_rehash`: true when the stored hash does not use the app's current parameters.
pub fn needs_rehash(encoded: &str) -> bool {
    let current = "$argon2id$v=19$m=65536,t=3,p=1$";
    !encoded.starts_with(current)
}

pub type Clock = Box<dyn Fn() -> f64 + Send + Sync>;

pub fn system_clock() -> Clock {
    Box::new(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    })
}

/// SQLite transactions enforce last-Operator protection across connections. Construction does
/// no I/O. Session tokens are hashed at rest; user changes revoke all their sessions. Methods are
/// blocking: call them from `spawn_blocking`, never from the async request path.
pub struct RemoteStore {
    pub path: PathBuf,
    clock: Clock,
}

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS users (
        id TEXT PRIMARY KEY, email TEXT UNIQUE NOT NULL,
        password TEXT NOT NULL,
        role TEXT NOT NULL CHECK(role IN ('Viewer', 'Operator')),
        enabled INTEGER NOT NULL CHECK(enabled IN (0,1))
    );
    CREATE TABLE IF NOT EXISTS sessions (
        token TEXT PRIMARY KEY, user_id TEXT NOT NULL
        REFERENCES users(id) ON DELETE CASCADE, expires REAL NOT NULL
    );
    CREATE TABLE IF NOT EXISTS attempts (
        key TEXT PRIMARY KEY, start REAL NOT NULL, count INTEGER NOT NULL
    );
";

impl RemoteStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self::with_clock(path, system_clock())
    }

    pub fn with_clock(path: impl Into<PathBuf>, clock: Clock) -> Self {
        Self {
            path: path.into(),
            clock,
        }
    }

    fn now(&self) -> f64 {
        (self.clock)()
    }

    fn open(&self) -> Result<Connection, RemoteAuthError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| RemoteAuthError::new(503, "Remote storage unavailable."))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }
        // Create the database privately before SQLite opens it (no credential bytes yet).
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .mode(0o600)
                .open(&self.path);
        }
        let conn = Connection::open(&self.path).map_err(RemoteAuthError::storage)?;
        conn.busy_timeout(Duration::from_secs(2))
            .map_err(RemoteAuthError::storage)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(RemoteAuthError::storage)?;
        conn.execute_batch(SCHEMA)
            .map_err(RemoteAuthError::storage)?;
        Ok(conn)
    }

    /// Run `f` in a `BEGIN IMMEDIATE` transaction: commit on `Ok`, roll back on `Err`.
    fn with_tx<T>(
        &self,
        f: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, RemoteAuthError>,
    ) -> Result<T, RemoteAuthError> {
        let mut conn = self.open()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(RemoteAuthError::storage)?;
        let value = f(&tx)?;
        tx.commit().map_err(RemoteAuthError::storage)?;
        Ok(value)
    }

    pub fn installation_generation(&self, generation: &str) -> Result<(), RemoteAuthError> {
        self.with_tx(|db| {
            db.execute(
                "CREATE TABLE IF NOT EXISTS remote_generation (id INTEGER PRIMARY KEY, value TEXT)",
                [],
            )
            .map_err(RemoteAuthError::storage)?;
            let current: Option<String> = db
                .query_row("SELECT value FROM remote_generation WHERE id=1", [], |r| {
                    r.get(0)
                })
                .ok();
            if current.as_deref() != Some(generation) {
                db.execute("DELETE FROM sessions", [])
                    .map_err(RemoteAuthError::storage)?;
                db.execute(
                    "INSERT OR REPLACE INTO remote_generation VALUES (1, ?)",
                    params![generation],
                )
                .map_err(RemoteAuthError::storage)?;
            }
            Ok(())
        })
    }

    pub fn users(&self) -> Result<Vec<RemoteUser>, RemoteAuthError> {
        self.with_tx(|db| {
            let mut stmt = db
                .prepare("SELECT id, email, role, enabled FROM users ORDER BY email")
                .map_err(RemoteAuthError::storage)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(RemoteUser {
                        id: r.get(0)?,
                        email: r.get(1)?,
                        role: r.get(2)?,
                        enabled: r.get::<_, i64>(3)? != 0,
                    })
                })
                .map_err(RemoteAuthError::storage)?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(RemoteAuthError::storage)
        })
    }

    pub fn bootstrap(&self, email: &str, password: &str) -> Result<RemoteUser, RemoteAuthError> {
        self.create_user(email, password, RemoteRole::Operator, true)
    }

    pub fn create_user(
        &self,
        email: &str,
        password: &str,
        role: RemoteRole,
        first_only: bool,
    ) -> Result<RemoteUser, RemoteAuthError> {
        let email = normalize_email(email)?;
        let length = password.chars().count();
        if !(12..=1024).contains(&length) {
            return Err(RemoteAuthError::new(
                422,
                "Passwords must contain 12 to 1024 characters.",
            ));
        }
        let encoded = hash_password(password)?;
        self.with_tx(|db| {
            let any: bool = db.query_row("SELECT 1 FROM users", [], |_| Ok(())).is_ok();
            if first_only && any {
                return Err(RemoteAuthError::new(
                    409,
                    "The first Operator already exists.",
                ));
            }
            if !any && role != RemoteRole::Operator {
                return Err(RemoteAuthError::new(
                    409,
                    "The first remote user must be an Operator.",
                ));
            }
            let user_id = token_urlsafe(18);
            match db.execute(
                "INSERT INTO users VALUES (?, ?, ?, ?, 1)",
                params![user_id, email, encoded, role.as_str()],
            ) {
                Ok(_) => {}
                Err(rusqlite::Error::SqliteFailure(e, _))
                    if e.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    return Err(RemoteAuthError::new(
                        409,
                        "That email is already registered.",
                    ));
                }
                Err(e) => return Err(RemoteAuthError::storage(e)),
            }
            Ok(RemoteUser {
                id: user_id,
                email,
                role: role.as_str().to_string(),
                enabled: true,
            })
        })
    }

    pub fn update_user(
        &self,
        user_id: &str,
        role: Option<RemoteRole>,
        enabled: Option<bool>,
        password: Option<&str>,
        delete: bool,
    ) -> Result<(), RemoteAuthError> {
        let encoded = match password {
            Some(p) => {
                if !(12..=1024).contains(&p.chars().count()) {
                    return Err(RemoteAuthError::new(
                        422,
                        "Passwords must contain 12 to 1024 characters.",
                    ));
                }
                Some(hash_password(p)?)
            }
            None => None,
        };
        self.with_tx(|db| {
            let row: Option<(String, i64, String)> = db
                .query_row(
                    "SELECT role, enabled, password FROM users WHERE id=?",
                    params![user_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .ok();
            let Some((row_role, row_enabled, row_password)) = row else {
                return Err(RemoteAuthError::new(404, "Remote user not found."));
            };
            if row_enabled != 0
                && row_role == "Operator"
                && (delete || enabled == Some(false) || role == Some(RemoteRole::Viewer))
            {
                let operators: i64 = db
                    .query_row(
                        "SELECT count(*) FROM users WHERE enabled=1 AND role='Operator'",
                        [],
                        |r| r.get(0),
                    )
                    .map_err(RemoteAuthError::storage)?;
                if operators <= 1 {
                    return Err(RemoteAuthError::new(
                        409,
                        "The last enabled Operator cannot be removed.",
                    ));
                }
            }
            db.execute("DELETE FROM sessions WHERE user_id=?", params![user_id])
                .map_err(RemoteAuthError::storage)?;
            if delete {
                db.execute("DELETE FROM users WHERE id=?", params![user_id])
                    .map_err(RemoteAuthError::storage)?;
            } else {
                db.execute(
                    "UPDATE users SET role=?, enabled=?, password=? WHERE id=?",
                    params![
                        role.map(|r| r.as_str().to_string()).unwrap_or(row_role),
                        enabled.map(i64::from).unwrap_or(row_enabled),
                        encoded.unwrap_or(row_password),
                        user_id
                    ],
                )
                .map_err(RemoteAuthError::storage)?;
            }
            Ok(())
        })
    }

    /// Persist an attempt before verification (unknown users included). 5 per email and 30 global
    /// per 300 s window; both counters are incremented together.
    fn throttle(&self, email: &str) -> Result<(), RemoteAuthError> {
        let now = self.now();
        self.with_tx(|db| {
            db.execute("DELETE FROM attempts WHERE start<=?", params![now - 300.0])
                .map_err(RemoteAuthError::storage)?;
            let email_key = format!("email:{}", digest(email));
            let keys = [("global".to_string(), 30i64), (email_key, 5i64)];
            for (key, limit) in &keys {
                let count: Option<i64> = db
                    .query_row("SELECT count FROM attempts WHERE key=?", params![key], |r| r.get(0))
                    .ok();
                if count.is_some_and(|c| c >= *limit) {
                    return Err(RemoteAuthError::new(
                        429,
                        "Too many login attempts. Try again later.",
                    ));
                }
            }
            for (key, _) in &keys {
                db.execute(
                    "INSERT INTO attempts VALUES (?, ?, 1) ON CONFLICT(key) DO UPDATE SET count=count+1",
                    params![key, now],
                )
                .map_err(RemoteAuthError::storage)?;
            }
            Ok(())
        })
    }

    pub fn login(
        &self,
        email: &str,
        password: &str,
        previous: Option<&str>,
    ) -> Result<String, RemoteAuthError> {
        let email = email.trim().to_lowercase();
        // The throttle commits on its own, so a failed password still counts.
        self.throttle(&email)?;
        let incorrect = || RemoteAuthError::new(401, "Incorrect email or password.");
        let user: Option<(String, String, i64)> = {
            let conn = self.open()?;
            let result = conn
                .query_row(
                    "SELECT id, password, enabled FROM users WHERE email=?",
                    params![email],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .ok();
            result
        };
        let Some((user_id, stored, enabled)) = user else {
            // Same Argon2 cost, no user enumeration through the cheap missing-user path.
            let _ = hash_password(password);
            return Err(incorrect());
        };
        if !verify_password(password, &stored) {
            return Err(incorrect());
        }
        if enabled == 0 {
            return Err(incorrect());
        }
        let rehash = if needs_rehash(&stored) {
            Some(hash_password(password)?)
        } else {
            None
        };
        let now = self.now();
        let token = token_urlsafe(32);
        self.with_tx(|db| {
            if let Some(new_hash) = &rehash {
                db.execute(
                    "UPDATE users SET password=? WHERE id=?",
                    params![new_hash, user_id],
                )
                .map_err(RemoteAuthError::storage)?;
            }
            db.execute("DELETE FROM sessions WHERE expires<=?", params![now])
                .map_err(RemoteAuthError::storage)?;
            if let Some(prev) = previous.filter(|p| !p.is_empty()) {
                db.execute("DELETE FROM sessions WHERE token=?", params![digest(prev)])
                    .map_err(RemoteAuthError::storage)?;
            }
            // Bound session growth per account without limiting concurrent users.
            db.execute(
                "DELETE FROM sessions WHERE user_id=? AND token NOT IN \
                 (SELECT token FROM sessions WHERE user_id=? ORDER BY expires DESC LIMIT 9)",
                params![user_id, user_id],
            )
            .map_err(RemoteAuthError::storage)?;
            db.execute(
                "INSERT INTO sessions VALUES (?, ?, ?)",
                params![digest(&token), user_id, now + SESSION_SECONDS],
            )
            .map_err(RemoteAuthError::storage)?;
            Ok(())
        })?;
        Ok(token)
    }

    pub fn session(&self, token: Option<&str>) -> Result<Option<RemoteSession>, RemoteAuthError> {
        let Some(token) = token.filter(|t| !t.is_empty() && t.len() <= 128) else {
            return Ok(None);
        };
        let now = self.now();
        self.with_tx(|db| {
            let row: Option<(String, String, String, f64)> = db
                .query_row(
                    "SELECT users.id, users.email, users.role, sessions.expires FROM sessions \
                     JOIN users ON users.id=sessions.user_id \
                     WHERE token=? AND expires>? AND enabled=1",
                    params![digest(token), now],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .ok();
            Ok(row.and_then(|(user_id, email, role, expires_at)| {
                RemoteRole::parse(&role).map(|role| RemoteSession {
                    user_id,
                    email,
                    role,
                    expires_at,
                })
            }))
        })
    }

    /// Clear the login throttle counters (operator-queued `rate_limit_reset`). Idempotent; never
    /// touches users or sessions.
    pub fn reset_login_attempts(&self) -> Result<(), RemoteAuthError> {
        self.with_tx(|db| {
            db.execute("DELETE FROM attempts", [])
                .map_err(RemoteAuthError::storage)?;
            Ok(())
        })
    }

    pub fn revoke(&self, token: &str, all_sessions: bool) -> Result<(), RemoteAuthError> {
        self.with_tx(|db| {
            if all_sessions {
                db.execute(
                    "DELETE FROM sessions WHERE user_id IN (SELECT user_id FROM sessions WHERE token=?)",
                    params![digest(token)],
                )
            } else {
                db.execute("DELETE FROM sessions WHERE token=?", params![digest(token)])
            }
            .map_err(RemoteAuthError::storage)?;
            Ok(())
        })
    }

    /// Idempotency: claim a request key. `Ok(Some((status, body)))` replays a finished action.
    pub fn claim_action(
        &self,
        key: &str,
        fingerprint: &str,
        expires: f64,
    ) -> Result<Option<(u16, Vec<u8>)>, RemoteAuthError> {
        let now = self.now();
        self.with_tx(|db| {
            db.execute(
                "CREATE TABLE IF NOT EXISTS remote_actions \
                 (key TEXT PRIMARY KEY, fingerprint TEXT, expires REAL, status INTEGER, response BLOB)",
                [],
            )
            .map_err(RemoteAuthError::storage)?;
            db.execute("DELETE FROM remote_actions WHERE expires<=?", params![now])
                .map_err(RemoteAuthError::storage)?;
            let row: Option<(String, Option<i64>, Option<Vec<u8>>)> = db
                .query_row(
                    "SELECT fingerprint, status, response FROM remote_actions WHERE key=?",
                    params![key],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .ok();
            if let Some((stored, status, response)) = row {
                if stored != fingerprint {
                    return Err(RemoteAuthError::new(
                        409,
                        "This request identifier belongs to another action.",
                    ));
                }
                let Some(status) = status else {
                    return Err(RemoteAuthError::new(
                        409,
                        "Action outcome is pending or uncertain. Check live state.",
                    ));
                };
                return Ok(Some((status as u16, response.unwrap_or_default())));
            }
            let count: i64 = db
                .query_row("SELECT count(*) FROM remote_actions", [], |r| r.get(0))
                .map_err(RemoteAuthError::storage)?;
            if count >= 2048 {
                return Err(RemoteAuthError::new(
                    429,
                    "Remote action capacity reached. Try again later.",
                ));
            }
            db.execute(
                "INSERT INTO remote_actions VALUES (?, ?, ?, NULL, NULL)",
                params![key, fingerprint, expires],
            )
            .map_err(RemoteAuthError::storage)?;
            Ok(None)
        })
    }

    pub fn finish_action(
        &self,
        key: &str,
        status: u16,
        response: &[u8],
    ) -> Result<(), RemoteAuthError> {
        if response.len() > 262_144 {
            return Err(RemoteAuthError::new(
                409,
                "Action completed. Check live state before continuing.",
            ));
        }
        self.with_tx(|db| {
            db.execute(
                "UPDATE remote_actions SET status=?, response=? WHERE key=?",
                params![status as i64, response, key],
            )
            .map_err(RemoteAuthError::storage)?;
            Ok(())
        })
    }
}
