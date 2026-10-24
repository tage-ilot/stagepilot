//! Async HTTP client for the documented ProPresenter timer API (`client.py`).

use crate::errors::PpError;
use crate::models::{Look, Timer};
use async_trait::async_trait;
use serde_json::Value;
use stagepilot_core::config::ProPresenterSettings;
use std::time::Duration;

/// The verification loops. Python keeps them as module constants (tests patch them).
#[derive(Debug, Clone, Copy)]
pub struct Tuning {
    pub timer_update_attempts: u32,
    pub timer_update_interval: Duration,
    pub reset_command_attempts: u32,
    pub reset_verification_attempts: u32,
    pub reset_verification_interval: Duration,
}

fn install_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // An error means a provider is already installed process-wide, which is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            timer_update_attempts: 30,
            timer_update_interval: Duration::from_millis(200),
            reset_command_attempts: 2,
            reset_verification_attempts: 5,
            reset_verification_interval: Duration::from_millis(100),
        }
    }
}

/// `ProPresenterClientContract`.
#[async_trait]
pub trait ProPresenterClient: Send + Sync {
    async fn close(&self);
    async fn list_timers(&self) -> Result<Vec<Timer>, PpError>;
    async fn find_timer(&self, name: &str) -> Result<Timer, PpError>;
    async fn get_timer(&self, timer_id: &str) -> Result<Timer, PpError>;
    /// `GET /v1/timers/current`: the live state of each timer (`running`, `stopped`,
    /// `overrunning`), which `GET /v1/timers` does not carry. Clients without it report none.
    async fn current_timers(&self) -> Result<Vec<Timer>, PpError> {
        Ok(Vec::new())
    }
    async fn list_looks(&self) -> Result<Vec<Look>, PpError>;
    async fn current_look(&self) -> Result<Look, PpError>;
    async fn trigger_look(&self, look_id: &str) -> Result<(), PpError>;
    async fn stop_timer(&self, timer_id: &str) -> Result<(), PpError>;
    async fn set_timer_duration(
        &self,
        timer: &Timer,
        duration_seconds: i64,
    ) -> Result<Timer, PpError>;
    async fn reset_timer(&self, timer_id: &str) -> Result<(), PpError>;
    async fn start_timer(&self, timer_id: &str) -> Result<(), PpError>;
}

/// `reqwest` implementation. Per-phase timeouts like httpx (connect and read each get
/// `request_timeout_seconds`), no redirects, no TLS (plain local HTTP).
pub struct HttpProPresenterClient {
    http: reqwest::Client,
    base_url: String,
    timeout_seconds: f64,
    tuning: Tuning,
}

/// Python `f"{x:g}"` for the timeout values settings allow (0 < x <= 60).
fn format_g(value: f64) -> String {
    let s = format!("{value}");
    s.strip_suffix(".0").map_or(s.clone(), str::to_string)
}

impl HttpProPresenterClient {
    pub fn new(settings: &ProPresenterSettings) -> Self {
        Self::with_tuning(settings, Tuning::default())
    }

    pub fn with_tuning(settings: &ProPresenterSettings, tuning: Tuning) -> Self {
        let timeout = Duration::from_secs_f64(settings.request_timeout_seconds.max(0.001));
        install_crypto_provider();
        let http = reqwest::Client::builder()
            .connect_timeout(timeout)
            .read_timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .unwrap_or_default();
        Self {
            http,
            base_url: format!("http://{}:{}", settings.host, settings.port),
            timeout_seconds: settings.request_timeout_seconds,
            tuning,
        }
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        json: Option<&Value>,
    ) -> Result<Option<Value>, PpError> {
        let url = format!("{}{}", self.base_url, path);
        let mut request = self.http.request(method, &url);
        if let Some(body) = json {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.to_string());
        }
        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                PpError::Connection(format!(
                    "ProPresenter did not respond within {} seconds.",
                    format_g(self.timeout_seconds)
                ))
            } else if e.is_connect() {
                PpError::Connection(format!(
                    "Could not connect to ProPresenter at {}.",
                    self.base_url
                ))
            } else {
                PpError::Connection("The ProPresenter API request failed.".into())
            }
        })?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let flattened = text.trim().replace(['\r', '\n'], " ");
            let detail = if flattened.is_empty() {
                String::new()
            } else {
                format!(": {}", flattened.chars().take(200).collect::<String>())
            };
            return Err(PpError::Response(format!(
                "ProPresenter returned HTTP {}{}.",
                status.as_u16(),
                detail
            )));
        }
        let bytes = response.bytes().await.map_err(|e| {
            if e.is_timeout() {
                PpError::Connection(format!(
                    "ProPresenter did not respond within {} seconds.",
                    format_g(self.timeout_seconds)
                ))
            } else {
                PpError::Connection("The ProPresenter API request failed.".into())
            }
        })?;
        if bytes.is_empty() {
            return Ok(None);
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| PpError::Response("ProPresenter returned a non-JSON response.".into()))
    }

    async fn get(&self, path: &str) -> Result<Option<Value>, PpError> {
        self.request(reqwest::Method::GET, path, None).await
    }

    async fn timer_operation(&self, timer_id: &str, operation: &str) -> Result<(), PpError> {
        self.get(&format!("/v1/timer/{timer_id}/{operation}"))
            .await
            .map(|_| ())
    }

    fn extract_list(
        payload: Option<Value>,
        keys: [&str; 3],
        what: &str,
    ) -> Result<Vec<Value>, PpError> {
        let objects = |values: &Vec<Value>| -> Vec<Value> {
            values.iter().filter(|v| v.is_object()).cloned().collect()
        };
        match payload {
            Some(Value::Array(values)) => return Ok(objects(&values)),
            Some(Value::Object(map)) => {
                for key in keys {
                    if let Some(Value::Array(values)) = map.get(key) {
                        return Ok(objects(values));
                    }
                }
            }
            _ => {}
        }
        Err(PpError::Response(format!(
            "ProPresenter returned an unexpected {what} response."
        )))
    }

    async fn verify_timer_duration(&self, timer: &Timer, duration: u32) -> Result<Timer, PpError> {
        let mut current: Option<Timer> = None;
        for attempt in 0..self.tuning.timer_update_attempts {
            let read = self.get_timer(&timer.id.uuid).await?;
            let confirmed = read
                .countdown
                .as_ref()
                .is_some_and(|c| c.duration == duration);
            current = Some(read);
            if confirmed {
                if let Some(read) = current {
                    return Ok(read);
                }
            }
            if attempt + 1 < self.tuning.timer_update_attempts {
                tokio::time::sleep(self.tuning.timer_update_interval).await;
            }
        }
        let observed = current
            .as_ref()
            .map_or("no readable timer", Timer::timer_type);
        Err(PpError::Response(format!(
            "ProPresenter did not confirm timer \"{}\" duration as {} seconds; it still reported {}. \
             The timer was not started.",
            timer.id.name, duration, observed
        )))
    }

    fn timer_is_zero(timer: &Timer) -> bool {
        if timer.countdown.as_ref().is_none_or(|c| c.duration != 0) {
            return false;
        }
        match &timer.time {
            None => true,
            Some(time) => {
                let normalised = time.trim();
                !normalised.is_empty() && normalised.chars().all(|c| "0:.- ".contains(c))
            }
        }
    }
}

#[async_trait]
impl ProPresenterClient for HttpProPresenterClient {
    async fn close(&self) {}

    async fn list_timers(&self) -> Result<Vec<Timer>, PpError> {
        let payload = self.get("/v1/timers").await?;
        let raw = Self::extract_list(payload, ["timers", "items", "data"], "timer-list")?;
        raw.iter()
            .map(|value| {
                Timer::from_value(value).ok_or_else(|| {
                    PpError::Response("ProPresenter returned an invalid timer object.".into())
                })
            })
            .collect()
    }

    async fn current_timers(&self) -> Result<Vec<Timer>, PpError> {
        let payload = self.get("/v1/timers/current").await?;
        let raw = Self::extract_list(payload, ["timers", "items", "data"], "timer-list")?;
        Ok(raw.iter().filter_map(Timer::from_value).collect())
    }

    async fn find_timer(&self, name: &str) -> Result<Timer, PpError> {
        let wanted = name.trim().to_lowercase();
        let mut matches: Vec<Timer> = self
            .list_timers()
            .await?
            .into_iter()
            .filter(|t| t.id.name.trim().to_lowercase() == wanted)
            .collect();
        match matches.len() {
            0 => Err(PpError::TimerNotFound(format!(
                "ProPresenter timer \"{name}\" was not found."
            ))),
            1 => Ok(matches.remove(0)),
            _ => Err(PpError::TimerNotFound(format!(
                "Multiple ProPresenter timers are named \"{name}\"."
            ))),
        }
    }

    /// Read one timer directly so write verification does not use a stale list.
    async fn get_timer(&self, timer_id: &str) -> Result<Timer, PpError> {
        let payload = self.get(&format!("/v1/timer/{timer_id}")).await?;
        payload.as_ref().and_then(Timer::from_value).ok_or_else(|| {
            PpError::Response("ProPresenter returned an invalid timer object.".into())
        })
    }

    async fn list_looks(&self) -> Result<Vec<Look>, PpError> {
        let payload = self.get("/v1/looks").await?;
        let raw = Self::extract_list(payload, ["looks", "items", "data"], "looks-list")?;
        raw.iter()
            .map(|value| {
                Look::from_value(value).ok_or_else(|| {
                    PpError::Response("ProPresenter returned an invalid Look object.".into())
                })
            })
            .collect()
    }

    async fn current_look(&self) -> Result<Look, PpError> {
        let payload = self.get("/v1/look/current").await?;
        payload.as_ref().and_then(Look::from_value).ok_or_else(|| {
            PpError::Response("ProPresenter returned an invalid current Look object.".into())
        })
    }

    async fn trigger_look(&self, look_id: &str) -> Result<(), PpError> {
        if look_id.trim().is_empty() {
            return Err(PpError::Value("Look ID cannot be empty.".into()));
        }
        self.get(&format!("/v1/look/{look_id}/trigger"))
            .await
            .map(|_| ())
    }

    async fn stop_timer(&self, timer_id: &str) -> Result<(), PpError> {
        self.timer_operation(timer_id, "stop").await
    }

    async fn set_timer_duration(
        &self,
        timer: &Timer,
        duration_seconds: i64,
    ) -> Result<Timer, PpError> {
        let duration = u32::try_from(duration_seconds)
            .map_err(|_| PpError::Value("Timer duration must not be negative.".into()))?;
        // ProPresenter exposes an atomic endpoint for replacing the timer definition and applying
        // Reset. On macOS, a standalone PUT followed by GET /reset can acknowledge both requests
        // while retaining the previous elapsed/countdown-to-time variant.
        self.request(
            reqwest::Method::PUT,
            &format!("/v1/timer/{}/reset", timer.id.uuid),
            Some(&timer.update_payload(duration)),
        )
        .await?;
        self.verify_timer_duration(timer, duration).await
    }

    async fn reset_timer(&self, timer_id: &str) -> Result<(), PpError> {
        let mut current: Option<Timer> = None;
        for command_attempt in 0..self.tuning.reset_command_attempts {
            self.timer_operation(timer_id, "reset").await?;
            for verification in 0..self.tuning.reset_verification_attempts {
                let read = self.get_timer(timer_id).await?;
                let zero = Self::timer_is_zero(&read);
                current = Some(read);
                if zero {
                    return Ok(());
                }
                if verification + 1 < self.tuning.reset_verification_attempts {
                    tokio::time::sleep(self.tuning.reset_verification_interval).await;
                }
            }
            let _ = command_attempt;
        }
        let observed = match &current {
            None => "no readable timer".to_string(),
            Some(timer) => timer.time.clone().unwrap_or_else(|| "None".into()),
        };
        Err(PpError::Response(format!(
            "ProPresenter did not confirm that the timer reset to 0:00; it still reported {observed}."
        )))
    }

    async fn start_timer(&self, timer_id: &str) -> Result<(), PpError> {
        self.timer_operation(timer_id, "start").await
    }
}
