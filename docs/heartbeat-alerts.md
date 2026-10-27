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

## Running it

`stagepilot-remote-health-alert` (built from `crates/stagepilot-remote`, `cargo build --release -p
stagepilot-remote --bin stagepilot-remote-health-alert`) runs one check and exits. It is not
shipped in the desktop installers; build it where the alert should run (for example the host that
keeps the connector status file).

    STAGEPILOT_HEARTBEAT_ALERT_WEBHOOK_URL=<url> \
      stagepilot-remote-health-alert --status-file /abs/path/connector-status.json --max-age-seconds 10

- Put the webhook URL in an environment file (systemd `EnvironmentFile=`), not on the command line:
  the optional `--webhook-url` flag wins when both are set but shows up in `ps` and shell history.
- It prints one JSON line (`healthy`, `state`, `age_seconds`, `detail`, `alerted`, plus
  `alert_error` when the webhook could not be reached). The webhook URL is never printed.
- Exit codes: 0 healthy, 1 stale, stopped, invalid status file or undeliverable alert, 2 bad
  arguments. Run it from a systemd timer or cron; a non-zero exit marks the unit failed.
- With no webhook configured it still checks and exits 1 when unhealthy, but sends nothing.

The earlier Python entry point (`python -m stagepilot.remote_health_alert`, last present at tag
`v2.41.1`) took the same options and printed the same JSON line.
