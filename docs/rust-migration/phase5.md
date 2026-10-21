# Rust migration, phase 5: Planning Center

Ports the Planning Center integration from the Python backend to Rust.

## What moved

- Crate `stagepilot-pco`: Services API client (`client.rs`), OAuth sign-in with PKCE and the
  control-plane relay (`oauth.rs`), plan-loading plugin (`plugin.rs`), setup service (`setup.rs`),
  plan cache (`plan_cache.rs`), song-length write/undo (`lengths.rs`), env overrides (`settings.rs`).
- Server: `/api/v1/planning-center/*` routes in `stagepilot-server/src/pco_routes.rs`; live apply of
  the service source from `PUT /settings`; startup refresh and the proactive refresh loop.

## Compatibility

- Keyring account `planning-center-oauth` and its JSON blob, and the plan cache file name/shape, are
  identical to Python's, so either backend can read what the other wrote.
- Error `Display` text equals Python's messages (routes return it as the HTTP `detail`).
- Only `invalid_grant`-class errors set `needs_reconnect`; transient failures never do.
- `client_secret` never reaches the machine; token calls go through the control plane.
- Secrets use types with redacted `Debug`.

## Documented differences

- One total deadline per request (Python has per-phase timeouts).
- Pagination links must share the base URL's origin (Python checked scheme and host only).
- Redirects are never followed.
- Unparsable `STAGEPILOT_PCO_*` values are ignored (Python refuses to start).
- reqwest uses rustls with the `ring` provider, so CI needs no cmake/NASM.

## Verification

- `scripts/contract/mock_pco.py` is a deterministic mock of the Services API and the control plane.
  The `pco` contract profile (96 steps, `pco_steps` in `contract/transcripts.json`) runs against
  both backends; the mock's request log is compared too. `run_baseline.sh` and `run_rust.sh` run it.
- Rust tests: `crates/stagepilot-pco/tests/{client,oauth}.rs`.
- Nothing contacts the real Planning Center or control plane.
