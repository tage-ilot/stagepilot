//! Test-build guard (cargo feature `forbid-production`, off by default; this crate's own
//! dev-dependency on itself turns it on for its tests and `stagepilot-server`'s dev-dependency for
//! the server's). With it a request to a production service this crate talks to is refused with an
//! error that names the host. Without it [`refuse_production`] is a no-op and none of the guard is
//! compiled in, so the shipped app contains no such code. `stagepilot-remote` has the same check
//! for its own destinations.

#[cfg(not(feature = "forbid-production"))]
#[inline(always)]
pub fn refuse_production(_url: &url::Url) -> Result<(), String> {
    Ok(())
}

#[cfg(feature = "forbid-production")]
pub use guarded::{expecting_refusal, refuse_production};

#[cfg(feature = "forbid-production")]
mod guarded {
    use url::Url;

    use crate::client::API_BASE_URL;
    use crate::oauth::{AUTHORIZE_ENDPOINT, DEFAULT_CONTROL_PLANE_ORIGIN};

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

    /// `Err(message)` when `url` is a production service. Callers turn errors into a failed or
    /// retried request and a test would pass without noticing, so a refusal also ends the
    /// process: the message goes to the real stderr and the exit code is 101. Only
    /// [`expecting_refusal`] gets the error back.
    pub fn refuse_production(url: &Url) -> Result<(), String> {
        if let Err(message) = refuse_host(url) {
            if !EXPECT_REFUSAL.get() {
                use std::io::Write;
                let _ = writeln!(std::io::stderr(), "\n{message}\n");
                std::process::exit(101);
            }
            return Err(message);
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
                "refusing to contact production in a test: {host}; point the test at a \
                 loopback fake (see CONTRIBUTING.md)"
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
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_guard_is_compiled_into_test_builds() {
        // Fails if this crate's self dev-dependency (which turns the feature on) is lost.
        let compiled_in = cfg!(feature = "forbid-production");
        assert!(
            compiled_in,
            "feature forbid-production is not enabled for this test build"
        );
    }
}
