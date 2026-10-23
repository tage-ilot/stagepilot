#!/usr/bin/env bash
# The release "Validate application" command list: the ONE copy.
#
# Called by release-macos.yml, release-windows.yml, release-validate.yml (the no-tag dry run)
# and scripts/create-release.ps1. Run from the repository root after the dependencies are
# installed (uv sync, npm ci for frontend and desktop, a stable Rust toolchain).
#
#   scripts/validate_application.sh python   # X.Y.Z: Python sidecar installer
#   scripts/validate_application.sh rust      # X.Y.Z-rc.N: Rust in-process installer, no sidecar
#
# On macOS the Help Book must already be generated (npm --prefix desktop run build:help)
# because the Tauri build script checks the bundled resources.
set -euo pipefail

VARIANT="${1:-}"
case "$VARIANT" in
  python | rust) ;;
  *)
    echo "usage: $0 python|rust" >&2
    exit 2
    ;;
esac
cd "$(dirname "${BASH_SOURCE[0]}")/.."

step() { printf '\n==> %s\n' "$*"; }
run() {
  step "$*"
  "$@"
}

run uv run --project backend ruff format --check backend/src backend/tests
run uv run --project backend ruff check backend/src backend/tests
run uv run --project backend python -m mypy --config-file backend/pyproject.toml backend/src backend/tests
run uv run --project backend python -m pytest -c backend/pyproject.toml backend/tests
run npm --prefix frontend run lint
run npm --prefix frontend run typecheck
run npm --prefix frontend test -- --run
run npm --prefix frontend run build

if [[ "$VARIANT" == "rust" ]]; then
  # No PyInstaller sidecar: only the frontend assets and cloudflared are staged.
  run npm --prefix desktop run build:runtime:rust
  # Tauri auto-merges tauri.macos.conf.json, which declares the sidecar as externalBin, into
  # every cargo build of the desktop crate. The rust installers null it for `tauri build`
  # (tauri.rust.*.conf.json); the same override is passed to the cargo steps through
  # TAURI_CONFIG (a JSON merge patch that tauri-build reads) so they check what ships.
  export TAURI_CONFIG='{"bundle":{"externalBin":null}}'
else
  run npm --prefix desktop run build:runtime
fi

run cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --check
run cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
run cargo test --manifest-path desktop/src-tauri/Cargo.toml
if [[ "$VARIANT" == "rust" ]]; then
  run cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets --features in-process -- -D warnings
  run cargo test --manifest-path desktop/src-tauri/Cargo.toml --features in-process
fi
unset TAURI_CONFIG
run npm --prefix desktop run release:test
