# Connector heartbeat stale alert

StagePilot's private Remote connector fails closed when its status heartbeat goes stale
(`crates/stagepilot-remote/src/health.rs`, `check_status()`). On top of that check,
`crates/stagepilot-remote/src/health_alert.rs` can send one outbound webhook POST when the
heartbeat is judged stale, so an operator is notified of a broken tunnel instead of having to
notice it.

## What exists

- `health_alert::check_and_alert` reuses `check_status()` unchanged (same status file, same
  staleness threshold, no new threshold) and POSTs a small JSON payload to an
  operator-configured webhook URL when the check reports unhealthy. With no webhook URL
  configured, alerting is a no-op.
- The payload is generic: `source`, `event`, `state`, `age_seconds`, `detail`. It carries no
  secrets and never echoes the webhook URL back.
- Tests in `crates/stagepilot-remote/tests/feature.rs` cover staleness triggering the alert,
  freshness not triggering it, no webhook configured not calling the sender, and the HTTP call
  against a local fake (no real network call).

## What does not exist

There is no command-line entry point or scheduled runner for this in the Rust workspace. The
earlier Python entry point (`python -m stagepilot.remote_health_alert`) was removed together
with the Python backend (last present at tag `v2.41.1`). Any host that still runs it from an
old checkout keeps working from that checkout, but nothing in this repository invokes the
Rust function periodically. If alerting is wanted again, add a small binary (or a flag on
`stagepilot-headless`) that calls `check_and_alert` with `--status-file`,
`--max-age-seconds` and the webhook URL read from `STAGEPILOT_HEARTBEAT_ALERT_WEBHOOK_URL`
(from an environment file, never the command line, to keep it out of `ps` and shell history),
and run it from a systemd timer or cron.
