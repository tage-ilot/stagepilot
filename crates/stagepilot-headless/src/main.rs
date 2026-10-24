//! `stagepilot-headless`: the Rust counterpart of `python -m stagepilot` (`__main__.py` ->
//! `main.run()`).
//!
//! Python takes no command-line flags; everything comes from saved settings and `STAGEPILOT_*`
//! environment variables (`STAGEPILOT_HOST`, `STAGEPILOT_PORT`, `STAGEPILOT_LOG_LEVEL`,
//! `STAGEPILOT_WEB_ROOT`, `STAGEPILOT_DESKTOP_REMOTE_ROOT` + `STAGEPILOT_CLOUDFLARED_BINARY`, ...).
//! The default bind is the saved setting: 127.0.0.1, or 0.0.0.0 with LAN access, on the saved
//! server port. Logs are JSON lines on stdout. Ctrl-C and SIGTERM stop it gracefully.
//!
//! Additions over Python (all optional): `--host H` and `--port N` override the bind address;
//! `--help` and `--version` print and exit.

use stagepilot_server::launch::run_production;
use std::process::ExitCode;

fn usage() -> &'static str {
    "stagepilot-headless [--host HOST] [--port PORT]\n\n\
     Settings and STAGEPILOT_* environment variables decide everything else.\n"
}

#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    Run {
        host: Option<String>,
        port: Option<u16>,
    },
    Print(String),
    Error(String),
}

fn parse(args: &[String]) -> Parsed {
    let (mut host, mut port) = (None, None);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => return Parsed::Print(usage().to_string()),
            "--version" | "-V" => {
                return Parsed::Print(format!(
                    "stagepilot-headless {}\nmidi-backend: {}\n",
                    stagepilot_server::APP_VERSION,
                    stagepilot_midi::backend_name()
                ))
            }
            "--host" => match iter.next() {
                Some(value) => host = Some(value.clone()),
                None => return Parsed::Error("--host needs a value".into()),
            },
            "--port" => match iter.next().map(|v| v.parse::<u16>()) {
                Some(Ok(value)) => port = Some(value),
                _ => return Parsed::Error("--port needs a number from 0 to 65535".into()),
            },
            other => return Parsed::Error(format!("unrecognised argument: {other}")),
        }
    }
    Parsed::Run { host, port }
}

fn init_logging() {
    // STAGEPILOT_LOG_LEVEL wins; otherwise the `log_level` saved in the settings file (m1).
    let level = stagepilot_server::logging::process_startup_filter();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .or_else(|_| tracing_subscriber::EnvFilter::try_new(&level))
        .unwrap_or_else(|_| "info".into());
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .init();
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Parsed::Print(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Parsed::Error(message) => {
            eprintln!("stagepilot-headless: {message}\n{}", usage());
            ExitCode::from(2)
        }
        Parsed::Run { host, port } => {
            init_logging();
            match run_production(host, port).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    tracing::error!(%error, "stagepilot_headless_failed");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_arguments_runs_with_saved_settings() {
        assert_eq!(
            parse(&[]),
            Parsed::Run {
                host: None,
                port: None
            }
        );
    }

    #[test]
    fn host_and_port_override() {
        assert_eq!(
            parse(&args(&["--host", "0.0.0.0", "--port", "9000"])),
            Parsed::Run {
                host: Some("0.0.0.0".into()),
                port: Some(9000)
            }
        );
    }

    #[test]
    fn version_is_the_release_version_not_the_crate_version() {
        let Parsed::Print(text) = parse(&args(&["--version"])) else {
            panic!("--version prints");
        };
        assert!(
            text.starts_with(&format!(
                "stagepilot-headless {}\n",
                stagepilot_server::APP_VERSION
            )),
            "{text}"
        );
        assert!(!text.contains("0.0.0\n"), "{text}");
    }

    #[test]
    fn version_names_the_midi_backend() {
        let Parsed::Print(text) = parse(&args(&["--version"])) else {
            panic!("--version prints");
        };
        // The midi crate's own feature decides (cargo unifies `--features stagepilot-midi/midir-backend`
        // without enabling this crate's feature), so key the expectation on what it reports.
        let expected = if stagepilot_midi::backend_name() == "midir" {
            "midi-backend: midir\n"
        } else {
            "midi-backend: none\n"
        };
        assert!(text.ends_with(expected), "{text}");
    }

    #[test]
    fn bad_arguments_are_errors() {
        assert!(matches!(parse(&args(&["--port", "x"])), Parsed::Error(_)));
        assert!(matches!(parse(&args(&["--host"])), Parsed::Error(_)));
        assert!(matches!(parse(&args(&["--nope"])), Parsed::Error(_)));
        assert!(matches!(parse(&args(&["--help"])), Parsed::Print(_)));
    }
}
