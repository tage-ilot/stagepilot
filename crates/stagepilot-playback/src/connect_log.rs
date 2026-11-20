//! What the connection retry says in the log. The retry itself (every `reconnect_delay`, one second) is not touched here.
//!
//! A Playback computer that is off the network fails the same way every second. One WARN line per attempt filled the
//! rotating `stagepilot-backend.log` files with the same line (489 lines in a few minutes). So, for one streak of failed
//! attempts (no valid heartbeat in between):
//!
//! * the first failure, and every change of reason (the address, the port or the error class), is a WARN line as before;
//! * a repeat of the same reason is a WARN line at most once a minute, with the number of attempts so far;
//! * the first valid heartbeat after a streak is one INFO line saying how many attempts it took.
//!
//! The time in "since 12:01:04" is the clock the log's own timestamps use (UTC).

use crate::find::ErrorClass;
use std::time::Duration;
use tokio::time::Instant;

/// A repeat of the same reason is logged at most this often.
pub(crate) const REPEAT_EVERY: Duration = Duration::from_secs(60);

/// One failed attempt, as the log says it.
pub(crate) struct Attempt<'a> {
    pub host: &'a str,
    pub port: u16,
    pub class: ErrorClass,
    pub step: Option<&'a str>,
    pub retry_in_ms: u64,
    pub detail: &'a str,
}

struct Streak {
    host: String,
    port: u16,
    class: ErrorClass,
    /// Time of day of the first failure of the streak.
    since: String,
    attempts: u64,
    last_logged: Instant,
}

/// The failed attempts since the last valid heartbeat.
#[derive(Default)]
pub(crate) struct ConnectLog {
    streak: Option<Streak>,
}

fn time_of_day() -> String {
    chrono::Utc::now().format("%H:%M:%S").to_string()
}

impl ConnectLog {
    /// A failed attempt. Says it in the log, or does not.
    pub(crate) fn failed(&mut self, now: Instant, attempt: &Attempt<'_>) {
        self.failed_since(now, attempt, time_of_day);
    }

    fn failed_since(
        &mut self,
        now: Instant,
        attempt: &Attempt<'_>,
        clock: impl FnOnce() -> String,
    ) {
        let Attempt {
            host,
            port,
            class,
            step,
            retry_in_ms,
            detail,
        } = *attempt;
        if let Some(streak) = self
            .streak
            .as_mut()
            .filter(|s| s.host == host && s.port == port && s.class == class)
        {
            streak.attempts += 1;
            if now.saturating_duration_since(streak.last_logged) >= REPEAT_EVERY {
                streak.last_logged = now;
                tracing::warn!(
                    target: "playback",
                    host = %host,
                    port = port,
                    class = ?class,
                    step = ?step,
                    retry_in_ms = retry_in_ms,
                    "playback_connect_failed: {detail} - still failing ({} attempts since {}, same reason)",
                    streak.attempts,
                    streak.since
                );
            }
            return;
        }
        self.streak = Some(Streak {
            host: host.to_string(),
            port,
            class,
            since: clock(),
            attempts: 1,
            last_logged: now,
        });
        tracing::warn!(
            target: "playback",
            host = %host,
            port = port,
            class = ?class,
            step = ?step,
            retry_in_ms = retry_in_ms,
            "playback_connect_failed: {detail}"
        );
    }

    /// The first valid heartbeat: a streak, if there was one, is over.
    pub(crate) fn connected(&mut self, host: &str, port: u16) {
        let Some(streak) = self.streak.take() else {
            return;
        };
        tracing::info!(
            target: "playback",
            host = %host,
            port = port,
            "playback_connect_recovered: connected again after {} failed attempt{} since {}",
            streak.attempts,
            if streak.attempts == 1 { "" } else { "s" },
            streak.since
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{ConnectionOptions, PlaybackClient};
    use crate::testkit::FakePlayback;
    use std::fmt::Write as _;
    use std::sync::{Arc, Mutex};
    use tracing::{Level, Subscriber};

    /// Every event the code under test logs: its level and its text (message first, then `name=value` for each field).
    #[derive(Clone, Default)]
    struct Lines(Arc<Mutex<Vec<(Level, String)>>>);

    impl Lines {
        fn of(&self, level: Level) -> Vec<String> {
            let all = self.0.lock().unwrap();
            all.iter()
                .filter(|(l, _)| *l == level)
                .map(|(_, text)| text.clone())
                .collect()
        }
    }

    struct Text(String);
    impl tracing::field::Visit for Text {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                self.0 = format!("{value:?}{}", self.0);
            } else {
                let _ = write!(self.0, " {}={value:?}", field.name());
            }
        }
    }

    impl Subscriber for Lines {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            let mut text = Text(String::new());
            event.record(&mut text);
            self.0
                .lock()
                .unwrap()
                .push((*event.metadata().level(), text.0));
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    fn attempt(class: ErrorClass) -> Attempt<'static> {
        Attempt {
            host: "10.30.0.141",
            port: 8080,
            class,
            step: Some("connect"),
            retry_in_ms: 1000,
            detail: "No answer before the time limit",
        }
    }

    /// The retry's rhythm: one attempt a second, in virtual time.
    async fn attempts(log: &mut ConnectLog, count: u32, class: ErrorClass) {
        for _ in 0..count {
            log.failed_since(Instant::now(), &attempt(class), || "12:01:04".into());
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn two_minutes_of_the_same_failure_are_two_or_three_lines_not_one_hundred_and_twenty() {
        let lines = Lines::default();
        let _guard = tracing::subscriber::set_default(lines.clone());
        let mut log = ConnectLog::default();
        attempts(&mut log, 120, ErrorClass::TimedOut).await;
        let warns = lines.of(Level::WARN);
        assert!(warns.len() <= 3, "{} lines: {warns:#?}", warns.len());
        assert!(warns.len() >= 2, "a long streak still says it is going on");
        // The first line is the one of today, with the same fields.
        assert!(warns[0].starts_with("playback_connect_failed: No answer before the time limit"));
        assert!(
            warns[0].contains("host=10.30.0.141") && warns[0].contains("port=8080"),
            "{}",
            warns[0]
        );
        assert!(
            warns[0].contains("class=TimedOut") && warns[0].contains("step=Some(\"connect\")"),
            "{}",
            warns[0]
        );
        assert!(warns[0].contains("retry_in_ms=1000"), "{}", warns[0]);
        assert!(!warns[0].contains("still failing"));
        // The repeat counts the attempts and says since when.
        assert!(
            warns[1].contains("still failing (61 attempts since 12:01:04, same reason)"),
            "{}",
            warns[1]
        );
        assert!(warns[1].contains("class=TimedOut"), "{}", warns[1]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_reason_is_said_at_once_and_starts_its_own_count() {
        let lines = Lines::default();
        let _guard = tracing::subscriber::set_default(lines.clone());
        let mut log = ConnectLog::default();
        attempts(&mut log, 5, ErrorClass::NoRoute).await;
        assert_eq!(lines.of(Level::WARN).len(), 1);
        attempts(&mut log, 5, ErrorClass::Refused).await;
        let warns = lines.of(Level::WARN);
        assert_eq!(warns.len(), 2, "{warns:#?}");
        assert!(warns[1].contains("class=Refused"), "{}", warns[1]);
        // Back to the first reason: that is a change too.
        attempts(&mut log, 1, ErrorClass::NoRoute).await;
        assert_eq!(lines.of(Level::WARN).len(), 3);
        // Another address with the same class is another reason.
        let other = Attempt {
            host: "10.30.0.142",
            ..attempt(ErrorClass::NoRoute)
        };
        log.failed(Instant::now(), &other);
        let warns = lines.of(Level::WARN);
        assert_eq!(warns.len(), 4);
        assert!(warns[3].contains("host=10.30.0.142"), "{}", warns[3]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_connection_after_a_streak_is_one_info_line_and_a_quiet_start_says_nothing() {
        let lines = Lines::default();
        let _guard = tracing::subscriber::set_default(lines.clone());
        let mut log = ConnectLog::default();
        log.connected("10.30.0.141", 8080);
        assert!(lines.of(Level::INFO).is_empty(), "nothing failed before");
        attempts(&mut log, 90, ErrorClass::TimedOut).await;
        log.connected("10.30.0.141", 8080);
        let info = lines.of(Level::INFO);
        assert_eq!(info.len(), 1, "{info:#?}");
        assert!(
            info[0].starts_with("playback_connect_recovered: connected again after 90 failed attempts since 12:01:04"),
            "{}",
            info[0]
        );
        assert!(info[0].contains("host=10.30.0.141") && info[0].contains("port=8080"));
        // The streak is over: a second failure is a first failure again, and a second connection says nothing.
        log.connected("10.30.0.141", 8080);
        assert_eq!(lines.of(Level::INFO).len(), 1);
        let before = lines.of(Level::WARN).len();
        attempts(&mut log, 1, ErrorClass::TimedOut).await;
        assert_eq!(lines.of(Level::WARN).len(), before + 1);
        log.connected("10.30.0.141", 8080);
        assert!(lines.of(Level::INFO)[1].contains("after 1 failed attempt since"));
    }

    /// The real retry loop against an address nobody listens on, then against a Playback that comes up on it.
    #[tokio::test]
    async fn the_real_retry_logs_one_line_for_a_hundred_refusals_and_one_when_playback_comes_up() {
        let lines = Lines::default();
        let _guard = tracing::subscriber::set_default(lines.clone());
        let port = {
            let spare = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            spare.local_addr().unwrap().port()
        };
        let mut client = PlaybackClient::start(
            ConnectionOptions {
                port,
                host_override: Some("127.0.0.1".into()),
                heartbeat_timeout: Duration::from_secs(2),
                reconnect_delay: Duration::from_millis(10),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        // The first refusal comes at once on Linux and macOS and after about a second on Windows: wait for it, then let many more
        // attempts (10 ms apart where the refusal is immediate) go by.
        for _ in 0..3000 {
            if !lines.of(Level::WARN).is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
        let warns = lines.of(Level::WARN);
        assert_eq!(warns.len(), 1, "{warns:#?}");
        assert!(
            // Linux and macOS refuse a closed port at once; Windows retries first and reports no answer.
            (warns[0].contains("class=Refused") || warns[0].contains("class=TimedOut"))
                && warns[0].contains("host=127.0.0.1"),
            "{}",
            warns[0]
        );
        assert!(lines
            .of(Level::INFO)
            .iter()
            .all(|l| !l.contains("recovered")));
        let fake = FakePlayback::start_on(port, None).await;
        fake.auto_beat(Duration::from_millis(50));
        for _ in 0..600 {
            if client.status().connected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(client.status().connected, "Playback came up");
        let recovered: Vec<String> = lines
            .of(Level::INFO)
            .into_iter()
            .filter(|l| l.starts_with("playback_connect_recovered"))
            .collect();
        assert_eq!(recovered.len(), 1, "{recovered:#?}");
        assert!(
            recovered[0].contains("failed attempt") && recovered[0].contains(" since "),
            "{}",
            recovered[0]
        );
        client.stop().await;
    }
}
