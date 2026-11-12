#!/usr/bin/env bash
# Runs the workspace tests with cargo-nextest (CI profile) and reports them in the job summary.
#
#   scripts/ci/run_nextest.sh [cargo nextest run arguments, e.g. --workspace --features a,b]
#
# - Prints the number of tests nextest ran, so a platform that runs a different number than
#   `cargo test` did is visible (nextest does not run doctests; the workflow runs those next).
# - A test that fails and then passes on the retry (profile.ci in .config/nextest.toml has
#   retries = 1) is FLAKY. It does not fail the job, but it is never silent: the job summary
#   starts with a warning and lists each flaky test, and the run emits a ::warning:: annotation.
# - Exits with nextest's own exit code.
set -uo pipefail

summary="${GITHUB_STEP_SUMMARY:-/dev/null}"
log="$(mktemp)"
trap 'rm -f "$log"' EXIT

start=$(date +%s)
cargo nextest run --profile ci --color never "$@" 2>&1 | tee "$log"
status=${PIPESTATUS[0]}
seconds=$(($(date +%s) - start))

# "Summary [ 235.682s] 1515 tests run: 1515 passed (1 flaky), 0 skipped"
line="$(grep -E '^[[:space:]]*Summary \[' "$log" | tail -n 1 | sed -E 's/^[[:space:]]+//')"
count="$(printf '%s\n' "$line" | sed -nE 's/.*\] ([0-9]+) tests? run.*/\1/p')"
# "FLAKY 2/2 [   1.2s] crate::binary test_name" (final listing; deduplicated)
flaky="$(grep -E '^[[:space:]]*FLAKY ' "$log" | sed -E 's/^[[:space:]]+//' | sort -u)"

{
  if [ -n "$flaky" ]; then
    echo "### :warning: FLAKY tests passed only on the retry"
    echo
    echo "These tests failed once and passed on the retry. The job is green, but each one is a"
    echo "real flake to fix; do not ignore this list."
    echo
    echo '```'
    printf '%s\n' "$flaky"
    echo '```'
    echo
  fi
  echo "nextest: ${line:-no summary line (the run did not finish)}"
  echo
  echo "nextest_tests_run=${count:-unknown} test_seconds=${seconds} runner=${RUNNER_OS:-local}"
} >>"$summary"

if [ -n "$flaky" ]; then
  while IFS= read -r test; do
    echo "::warning title=FLAKY test (passed only on retry)::${test}"
  done <<<"$flaky"
fi

echo "nextest_tests_run=${count:-unknown} test_seconds=${seconds} exit=${status}"
exit "$status"
