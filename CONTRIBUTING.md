# Contributing to StagePilot

StagePilot is intended for live-production use, so clear failure behavior and
tests matter as much as the happy path. Small, reviewable changes are preferred.

## Set up a development environment

Install Python 3.12+, uv, a current Node.js LTS release, and npm. Rust stable and
the Tauri v2 system prerequisites are needed only for desktop work.

```sh
uv sync --project backend --extra dev
cd frontend
npm install
cd ../desktop
npm install
cd ..
```

Run the backend and frontend in separate terminals using the commands in
[README.md](README.md). Demo mode should remain usable without vendor accounts or
production equipment.

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
cd backend
uv run ruff format --check .
uv run ruff check .
uv run mypy
uv run pytest
cd ..

npm --prefix frontend run lint
npm --prefix frontend test
npm --prefix frontend run typecheck
npm --prefix frontend run build

cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --check
cargo check --manifest-path desktop/src-tauri/Cargo.toml
```

Use `ruff format` or `cargo fmt` without `--check` to apply formatting. Frontend
formatting follows the scripts and configuration committed in that workspace.
Run `npm --prefix frontend run lint` and `npx --yes @google/design.md@0.4.0 lint DESIGN.md` before pushing frontend changes.

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
- pass backend, frontend, and desktop checks affected by the change; and
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
