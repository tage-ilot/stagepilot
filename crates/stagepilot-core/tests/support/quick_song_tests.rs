//! The quick-song-tests switch, read by every heavy "big song" test through this one file.
//!
//! `STAGEPILOT_QUICK_SONG_TESTS=1` asks the heavy tests for their quick version: a scaled-down
//! song (shorter, fewer fixtures or addresses, caps scaled by the same factor, the same code path
//! and the same kind of assertion), or a skip that prints `skipped (quick mode): <name>` when
//! scaling would stop the test from testing what it is for. Unset (or any value but `1`) every
//! test runs in full, exactly as it always did: a developer's `cargo test`, pushes to main, a
//! manual dispatch, the release dry run and the release build. Only the pull-request run of
//! `.github/workflows/rust.yml` sets it.
//!
//! The file lives with the `stagepilot-core` test support and is included by the other crate's
//! tests with `#[path]`, so there is one reader of the variable, not one per test file.
#![allow(dead_code)]

use std::io::Write;

/// The name of the environment variable.
pub const SWITCH: &str = "STAGEPILOT_QUICK_SONG_TESTS";

/// True when the quick versions were asked for (the variable is exactly `1`).
pub fn on() -> bool {
    std::env::var(SWITCH).is_ok_and(|value| value == "1")
}

/// `full` normally, `quick` in quick mode: the one place a test chooses its size.
pub fn pick<T>(full: T, quick: T) -> T {
    if on() {
        quick
    } else {
        full
    }
}

/// In quick mode, say so and return true: the test returns at once. In full mode it returns
/// false and prints nothing. The line goes to stderr directly so the test harness's output
/// capture does not hide it from the CI log.
pub fn skip(name: &str) -> bool {
    if !on() {
        return false;
    }
    let _ = writeln!(std::io::stderr(), "skipped (quick mode): {name}");
    true
}
