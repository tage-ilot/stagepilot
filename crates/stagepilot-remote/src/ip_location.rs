//! Weekly, invisible city lookup for the fleet page.
//!
//! The app asks db-ip.com (free tier, no key) which city its own public address belongs to, keeps
//! only the formatted city text (`"Rancho Mirage, CA"`) in `location.json` and reports it on the
//! normal check-in. The address itself is never parsed out of the response, stored, sent or
//! logged. There is no setting for this; `STAGEPILOT_DISABLE_IP_LOOKUP=1` turns it off.

use std::path::Path;

use serde_json::{json, Value};

use crate::bootstrap::sanitize_location;
use crate::files::{atomic_write, create_private_dir};
use crate::http::{HttpRequest, HttpTransport};

/// db-ip free "who am I" endpoint.
pub const LOOKUP_URL: &str = "https://api.db-ip.com/v2/free/self";
/// Per-request timeout; the lookup runs off the check-in path, but must not linger.
pub const LOOKUP_TIMEOUT_SECS: f64 = 10.0;
/// A stored city older than this is looked up again.
pub const REFRESH_AFTER_SECS: f64 = 7.0 * 24.0 * 3600.0;
/// After a failed lookup, wait at least this long.
pub const FAILURE_BACKOFF_SECS: f64 = 3600.0;
/// Hard cap on failed attempts per UTC day (the backoff already limits this to 24).
pub const MAX_FAILURES_PER_DAY: u32 = 4;
/// Environment variable that disables the lookup when set to `1`.
pub const DISABLE_ENV: &str = "STAGEPILOT_DISABLE_IP_LOOKUP";

/// True when the opt-out value disables the lookup (`1`, `true`, `yes`, any case).
pub fn disabled_by(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes")
    )
}

/// The opt-out as set in this process's environment.
pub fn disabled_in_env() -> bool {
    disabled_by(std::env::var(DISABLE_ENV).ok().as_deref())
}

/// What is persisted in `location.json`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocationState {
    pub location: Option<String>,
    /// Wall-clock seconds of the last successful lookup; 0 for a legacy value.
    pub looked_up_at: f64,
    /// Wall-clock seconds of the last failed attempt; 0 for none.
    pub last_failure_at: f64,
    /// Failed attempts on the UTC day `failure_day`.
    pub failures_today: u32,
    pub failure_day: u64,
}

impl LocationState {
    /// Parse `location.json`. Accepts the current object, the object `#307` wrote
    /// (`{"location": "..."}` with no timestamp, treated as stale) and a bare string.
    pub fn parse(text: &str) -> Self {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return Self::default();
        };
        match &value {
            Value::String(s) => Self {
                location: sanitize_location(Some(s)),
                ..Self::default()
            },
            Value::Object(map) => {
                let num = |key: &str| map.get(key).and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
                Self {
                    location: sanitize_location(map.get("location").and_then(Value::as_str)),
                    looked_up_at: num("looked_up_at"),
                    last_failure_at: num("last_failure_at"),
                    failures_today: num("failures_today") as u32,
                    failure_day: num("failure_day") as u64,
                }
            }
            _ => Self::default(),
        }
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            create_private_dir(parent)?;
        }
        let body = json!({
            "location": self.location,
            "looked_up_at": self.looked_up_at,
            "last_failure_at": self.last_failure_at,
            "failures_today": self.failures_today,
            "failure_day": self.failure_day,
        })
        .to_string();
        atomic_write(path, &body).map_err(|e| std::io::Error::other(e.to_string()))
    }

    /// Whether a lookup should run now.
    pub fn due(&self, now: f64) -> bool {
        let day = (now / 86_400.0) as u64;
        if self.last_failure_at > 0.0 {
            if now - self.last_failure_at < FAILURE_BACKOFF_SECS {
                return false;
            }
            if self.failure_day == day && self.failures_today >= MAX_FAILURES_PER_DAY {
                return false;
            }
        }
        self.location.is_none() || now - self.looked_up_at >= REFRESH_AFTER_SECS
    }

    pub fn record_success(&mut self, location: String, now: f64) {
        self.location = Some(location);
        self.looked_up_at = now;
        self.last_failure_at = 0.0;
        self.failures_today = 0;
    }

    /// Keep the previous city, remember when we failed.
    pub fn record_failure(&mut self, now: f64) {
        let day = (now / 86_400.0) as u64;
        if self.failure_day != day {
            self.failure_day = day;
            self.failures_today = 0;
        }
        self.failures_today += 1;
        self.last_failure_at = now;
    }
}

/// One lookup through `transport`. `None` on any failure (network, non-2xx incl. 429, bad JSON,
/// error body, no city). Nothing from the response except the formatted city is returned.
pub fn lookup_city(transport: &dyn HttpTransport) -> Option<String> {
    let request = HttpRequest {
        method: "GET".into(),
        url: LOOKUP_URL.into(),
        headers: vec![("Accept".into(), "application/json".into())],
        timeout_secs: LOOKUP_TIMEOUT_SECS,
        ..HttpRequest::default()
    };
    let response = transport.send(&request).ok()?;
    if !response.is_success() {
        return None;
    }
    parse_response(&response.body)
}

/// Turn a db-ip body into `"City, ST"` / `"City, Country"`.
pub fn parse_response(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let map = value.as_object()?;
    if map.contains_key("errorCode") {
        return None;
    }
    let text = |key: &str| {
        map.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let city = text("city")?;
    let country_code = text("countryCode").map(str::to_ascii_uppercase);
    let formatted = if country_code.as_deref() == Some("US") {
        let state = text("stateProv")?;
        format!("{city}, {}", us_state_abbreviation(state).unwrap_or(state))
    } else {
        let country = text("countryName").or(country_code.as_deref())?;
        format!("{city}, {country}")
    };
    sanitize_location(Some(&formatted))
}

const US_STATES: [(&str, &str); 51] = [
    ("Alabama", "AL"),
    ("Alaska", "AK"),
    ("Arizona", "AZ"),
    ("Arkansas", "AR"),
    ("California", "CA"),
    ("Colorado", "CO"),
    ("Connecticut", "CT"),
    ("Delaware", "DE"),
    ("District of Columbia", "DC"),
    ("Florida", "FL"),
    ("Georgia", "GA"),
    ("Hawaii", "HI"),
    ("Idaho", "ID"),
    ("Illinois", "IL"),
    ("Indiana", "IN"),
    ("Iowa", "IA"),
    ("Kansas", "KS"),
    ("Kentucky", "KY"),
    ("Louisiana", "LA"),
    ("Maine", "ME"),
    ("Maryland", "MD"),
    ("Massachusetts", "MA"),
    ("Michigan", "MI"),
    ("Minnesota", "MN"),
    ("Mississippi", "MS"),
    ("Missouri", "MO"),
    ("Montana", "MT"),
    ("Nebraska", "NE"),
    ("Nevada", "NV"),
    ("New Hampshire", "NH"),
    ("New Jersey", "NJ"),
    ("New Mexico", "NM"),
    ("New York", "NY"),
    ("North Carolina", "NC"),
    ("North Dakota", "ND"),
    ("Ohio", "OH"),
    ("Oklahoma", "OK"),
    ("Oregon", "OR"),
    ("Pennsylvania", "PA"),
    ("Rhode Island", "RI"),
    ("South Carolina", "SC"),
    ("South Dakota", "SD"),
    ("Tennessee", "TN"),
    ("Texas", "TX"),
    ("Utah", "UT"),
    ("Vermont", "VT"),
    ("Virginia", "VA"),
    ("Washington", "WA"),
    ("West Virginia", "WV"),
    ("Wisconsin", "WI"),
    ("Wyoming", "WY"),
];

/// Postal abbreviation for a US state name (case-insensitive); `None` if unknown.
pub fn us_state_abbreviation(name: &str) -> Option<&'static str> {
    let name = name.trim();
    US_STATES
        .iter()
        .find(|(full, _)| full.eq_ignore_ascii_case(name))
        .map(|(_, abbr)| *abbr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_fifty_states_and_dc() {
        assert_eq!(US_STATES.len(), 51);
        assert_eq!(us_state_abbreviation("california"), Some("CA"));
        assert_eq!(us_state_abbreviation("District of Columbia"), Some("DC"));
        assert_eq!(us_state_abbreviation("Narnia"), None);
        let mut abbrs: Vec<_> = US_STATES.iter().map(|(_, a)| *a).collect();
        abbrs.sort_unstable();
        abbrs.dedup();
        assert_eq!(abbrs.len(), 51);
    }

    #[test]
    fn parses_us_non_us_and_failures() {
        let us = r#"{"ipAddress":"203.0.113.9","continentCode":"NA","countryCode":"US","countryName":"United States","stateProv":"California","city":"Rancho Mirage"}"#;
        assert_eq!(parse_response(us).as_deref(), Some("Rancho Mirage, CA"));
        let unknown_state = r#"{"countryCode":"US","stateProv":"Puerto Rico Zone","city":"X"}"#;
        assert_eq!(
            parse_response(unknown_state).as_deref(),
            Some("X, Puerto Rico Zone")
        );
        let de =
            r#"{"countryCode":"DE","countryName":"Germany","stateProv":"Berlin","city":"Berlin"}"#;
        assert_eq!(parse_response(de).as_deref(), Some("Berlin, Germany"));
        for bad in [
            r#"{"countryCode":"US","stateProv":"Texas"}"#,
            r#"{"countryCode":"US","stateProv":"Texas","city":"  "}"#,
            r#"{"errorCode":"QUOTA_EXCEEDED","error":"over quota","city":"Austin"}"#,
            r#"{"countryCode":"US","city":"Austin"}"#,
            "not json",
            "[]",
            "",
        ] {
            assert_eq!(parse_response(bad), None, "{bad}");
        }
        let long = format!(
            r#"{{"countryCode":"DE","countryName":"G","city":"{}"}}"#,
            "a".repeat(90)
        );
        assert_eq!(parse_response(&long), None);
    }

    #[test]
    fn response_ip_never_leaks_into_result() {
        let body = r#"{"ipAddress":"198.51.100.77","countryCode":"FR","countryName":"France","city":"Paris"}"#;
        let city = parse_response(body).unwrap();
        assert!(!city.contains("198.51"));
    }

    #[test]
    fn staleness_rule() {
        let now = 10_000_000.0;
        let none = LocationState::default();
        assert!(none.due(now));
        let mut fresh = LocationState::default();
        fresh.record_success("A, CA".into(), now - 6.0 * 86_400.0);
        assert!(!fresh.due(now));
        let mut old = LocationState::default();
        old.record_success("A, CA".into(), now - 7.0 * 86_400.0 - 1.0);
        assert!(old.due(now));
        // A legacy value (no timestamp) is stale but still the value to send.
        let legacy = LocationState::parse(r#"{"location":"Pasadena, CA"}"#);
        assert_eq!(legacy.location.as_deref(), Some("Pasadena, CA"));
        assert!(legacy.due(now));
    }

    #[test]
    fn failure_backoff_and_daily_cap() {
        let mut s = LocationState::default();
        s.record_success("A, CA".into(), 0.0);
        let t0 = 30.0 * 86_400.0;
        assert!(s.due(t0));
        s.record_failure(t0);
        assert_eq!(s.location.as_deref(), Some("A, CA"));
        assert!(!s.due(t0 + 3599.0));
        assert!(s.due(t0 + 3600.0));
        for i in 1..MAX_FAILURES_PER_DAY {
            s.record_failure(t0 + f64::from(i) * 3600.0);
        }
        assert_eq!(s.failures_today, MAX_FAILURES_PER_DAY);
        assert!(!s.due(t0 + 10.0 * 3600.0), "capped for the rest of the day");
        assert!(s.due(t0 + 86_400.0), "next day allowed again");
    }

    #[test]
    fn legacy_and_roundtrip_files() {
        assert_eq!(
            LocationState::parse("\"Austin, TX\"").location.as_deref(),
            Some("Austin, TX")
        );
        assert_eq!(LocationState::parse("garbage"), LocationState::default());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remote").join("location.json");
        let mut s = LocationState::default();
        s.record_success("Denver, CO".into(), 123.0);
        s.save(&path).unwrap();
        assert_eq!(LocationState::load(&path), s);
    }

    #[test]
    fn opt_out_values() {
        assert!(disabled_by(Some("1")));
        assert!(disabled_by(Some(" TRUE ")));
        assert!(!disabled_by(Some("0")));
        assert!(!disabled_by(Some("")));
        assert!(!disabled_by(None));
    }
}
