//! Test-run guard: with `STAGEPILOT_FORBID_PRODUCTION_ORIGINS=1` (set by `.cargo/config.toml` for
//! every test run) a request to a production service this crate talks to is refused with an error
//! that names the host. The shipped app never sets the variable, so nothing changes for it.
//! `stagepilot-remote` has the same check for its own destinations.

use std::sync::OnceLock;

use url::Url;

use crate::client::API_BASE_URL;
use crate::oauth::{AUTHORIZE_ENDPOINT, DEFAULT_CONTROL_PLANE_ORIGIN};

const FORBID_ENV: &str = "STAGEPILOT_FORBID_PRODUCTION_ORIGINS";
const PRODUCTION_URLS: [&str; 3] = [
    API_BASE_URL,
    AUTHORIZE_ENDPOINT,
    DEFAULT_CONTROL_PLANE_ORIGIN,
];

thread_local! {
    static EXPECT_REFUSAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `f` with this thread expecting [`refuse_production`] to refuse (a test of the guard
/// itself). Anywhere else a refusal ends the process.
pub fn expecting_refusal<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            EXPECT_REFUSAL.set(self.0);
        }
    }
    let _restore = Restore(EXPECT_REFUSAL.replace(true));
    f()
}

/// `Err(message)` when the guard is on and `url` is a production service; `Ok` otherwise (the
/// variable is read once per process). Callers turn errors into a failed or retried request and a
/// test would pass without noticing, so a refusal also ends the process: the message goes to the
/// real stderr and the exit code is 101. Only [`expecting_refusal`] gets the error back.
pub fn refuse_production(url: &Url) -> Result<(), String> {
    static FORBID: OnceLock<bool> = OnceLock::new();
    if *FORBID.get_or_init(|| std::env::var(FORBID_ENV).is_ok_and(|v| v == "1")) {
        if let Err(message) = refuse_host(url) {
            if !EXPECT_REFUSAL.get() {
                use std::io::Write;
                let _ = writeln!(std::io::stderr(), "\n{message}\n");
                std::process::exit(101);
            }
            return Err(message);
        }
    }
    Ok(())
}

fn refuse_host(url: &Url) -> Result<(), String> {
    let host = url.host_str().unwrap_or_default().trim_end_matches('.');
    let production = PRODUCTION_URLS.iter().any(|known| {
        Url::parse(known)
            .ok()
            .is_some_and(|known| known.host_str() == Some(host))
    });
    if production {
        return Err(format!(
            "refusing to contact production in a test: {host} ({FORBID_ENV}=1); point the test \
             at a loopback fake (see CONTRIBUTING.md)"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(url: &str) -> Option<String> {
        refuse_host(&Url::parse(url).unwrap()).err()
    }

    #[test]
    fn production_hosts_are_refused_by_name_and_others_are_not() {
        for (url, host) in [
            (API_BASE_URL, "api.planningcenteronline.com"),
            (
                "https://API.planningcenteronline.com./services/v2/x",
                "api.planningcenteronline.com",
            ),
            (AUTHORIZE_ENDPOINT, "api.planningcenteronline.com"),
            (
                DEFAULT_CONTROL_PLANE_ORIGIN,
                "stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev",
            ),
        ] {
            let message = refused(url).unwrap_or_else(|| panic!("{url} must be refused"));
            assert!(
                message.starts_with("refusing to contact production in a test: "),
                "{message}"
            );
            assert!(message.contains(host), "{message}");
        }
        for url in [
            "http://127.0.0.1:9/",
            "http://localhost:8080/services",
            "https://example.com/",
            "https://planningcenteronline.com.example.org/",
        ] {
            assert_eq!(refused(url), None, "{url}");
        }
    }

    #[test]
    fn the_test_runner_turns_the_guard_on() {
        assert_eq!(std::env::var(FORBID_ENV).as_deref(), Ok("1"));
        let url = Url::parse(API_BASE_URL).unwrap();
        let message = expecting_refusal(|| refuse_production(&url)).unwrap_err();
        assert!(message.starts_with("refusing to contact production in a test: "));
    }
}
