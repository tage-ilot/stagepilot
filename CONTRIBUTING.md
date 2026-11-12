# Contributing to StagePilot

StagePilot is intended for live-production use, so clear failure behavior and
tests matter as much as the happy path. Small, reviewable changes are preferred.

## Set up a development environment

Install a stable Rust toolchain (`rust-toolchain.toml` pins the channel), a current
Node.js LTS release, and npm. The Tauri v2 system prerequisites are needed only for
desktop work. Python 3 is used only by a few build and replay scripts under
`scripts/` and needs no packages.

```sh
cd frontend
npm install
cd ../desktop
npm install
cd ..
```

The server is the Rust workspace under `crates/`. For development, run it headless
with `cargo run -p stagepilot-headless` (demo mode needs no vendor accounts or
production equipment) and the frontend with `npm --prefix frontend run dev`.

## Branches and commits

Create focused branches such as `feat/event-stream`, `fix/song-index-bounds`,
or `docs/plugin-lifecycle`. Use conventional commit messages where practical:

```text
feat(core): add asynchronous event bus
fix(api): reject zero-duration timer actions
test(midi): cover note-on velocity zero
docs: clarify LAN security boundary
```

Do not commit credentials, local configuration, generated dependency folders,
build outputs, or log files.

## Formatting, linting, and tests

Before opening a pull request, run the checks relevant to the changed workspace:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --exclude midi-probe
scripts/contract/run_rust.sh

npm --prefix frontend run lint
npm --prefix frontend test
npm --prefix frontend run typecheck
npm --prefix frontend run build

cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --check
cargo check --manifest-path desktop/src-tauri/Cargo.toml
```

`cargo test` runs the same tests and always works. CI runs them with
[cargo-nextest](https://nexte.st) (`cargo install --locked cargo-nextest`, or
`scripts/ci/install_nextest.sh`), which starts a process per test and overlaps the test binaries:

```sh
cargo nextest run --workspace --exclude midi-probe     # what `cargo test` runs, without doctests
cargo test --workspace --exclude midi-probe --doc      # doctests (nextest does not run them)
cargo nextest run --profile ci --workspace --exclude midi-probe   # as CI: one retry, FLAKY report
```

The heavy timing tests are limited to one at a time by `.config/nextest.toml`. A test must pass
when it runs alone (nextest runs each in its own process): do not rely on another test having
set up shared state first. A test that passes only on the CI retry is reported as FLAKY in the job
summary; fix it, do not ignore it.

Use `cargo fmt` without `--check` to apply formatting. Frontend
formatting follows the scripts and configuration committed in that workspace.
Run `npm --prefix frontend run lint` before pushing frontend changes.

### Tests never talk to production

`.cargo/config.toml` sets `STAGEPILOT_FORBID_PRODUCTION_ORIGINS=1` for `cargo test` and
`cargo nextest`. With it set, the real HTTP transports (`stagepilot-remote`, `stagepilot-pco`, the
diagnostics upload) end the process with `refusing to contact production in a test: <host>` (exit
code 101) instead of quietly contacting the beta control plane, db-ip.com or Planning Center. If
you see it, point the test at a loopback fake: `DesktopOptions { control_plane_origin:
"http://127.0.0.1:9".into(), .. }` or a fake `HttpTransport` (see `tests/checkin.rs`), and a mock
`base_url` for Planning Center clients. A test of the guard wraps the call in `expecting_refusal`
to get the error back. `cargo run` gets the variable too: use
`STAGEPILOT_FORBID_PRODUCTION_ORIGINS=0 cargo run ...` to run the app against production on your
machine. The shipped app never sets it.

### Quick big-song tests (pull requests only)

A handful of tests guard the Lightkey save caps and time budgets with very large songs
(`stagepilot-lights/tests/lightkey_big_songs.rs`, and `big_song_caps.rs` and `cue_map_scale.rs` in
`stagepilot-core/tests`). They take minutes on a CI runner. Setting
`STAGEPILOT_QUICK_SONG_TESTS=1` makes them run a scaled version (a shorter song or fewer fixtures
and addresses, with the caps scaled by the same factor and the same assertions) or print
`skipped (quick mode): <name>` when scaling would stop them testing what they are for.

- Unset (the default), everything runs in full: a plain `cargo test`, `cargo nextest run`, pushes
  to `main`, a manual run of `rust.yml` and every release path.
- `.github/workflows/rust.yml` sets it to `1` only for the `pull_request` event.
- To try the quick version locally: `STAGEPILOT_QUICK_SONG_TESTS=1 cargo test -p stagepilot-lights
  --test lightkey_big_songs`. Run the full ones (variable unset) before a release or when you
  change the fit, the caps or the replay engine.
- A test reads the switch only through `crates/stagepilot-core/tests/support/quick_song_tests.rs`;
  a new heavy test should do the same and say in its file header what its quick version still
  proves.

Tests should describe observable behavior, including bounds, timeouts, malformed
input, reconnects, and failure isolation. Vendor API tests must use mocks and
must never require contributor credentials.

## Pull requests

A pull request should:

- explain the user-visible or architectural outcome;
- stay within one coherent concern;
- include or update tests for behavioral changes;
- update public documentation and the changelog when appropriate;
- identify unverified platform or vendor assumptions;
- pass the Rust, frontend, and desktop checks affected by the change; and
- avoid unrelated formatting or generated-file churn.

Reviewers may ask for reliability evidence beyond unit tests when a change can
affect a live cue, timer, credential, or network boundary.

## Contributing a plugin

Read [docs/plugins.md](docs/plugins.md) first. New plugins belong behind the core
plugin contract, publish and subscribe through typed events, expose health, and
must not import another integration plugin. Include deterministic tests with a
fake client or transport.

Propose event-model changes separately when possible. Event names and payloads
are shared contracts; an integration-specific payload should not be added to a
general domain event merely for one vendor.

## Security reports

Do not open a public issue containing credentials, tokens, private network
details, or an exploitable vulnerability. Until a dedicated private reporting
channel is published, remove all sensitive data from a minimal report and ask a
maintainer for a private contact path.
