#!/usr/bin/env bash
# The release "Validate application" command list: the ONE copy.
#
# Called by release-macos.yml, release-windows.yml, release-validate.yml (the no-tag dry run)
# and scripts/create-release.ps1. Run from the repository root after the dependencies are
# installed (npm ci for frontend and desktop, a stable Rust toolchain).
#
#   scripts/validate_application.sh
#
# On macOS the Help Book must already be generated (npm --prefix desktop run build:help)
# because the Tauri build script checks the bundled resources.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

step() { printf '\n==> %s\n' "$*"; }
run() {
  step "$*"
  "$@"
}

run npm --prefix frontend run lint
run npm --prefix frontend run typecheck
run npm --prefix frontend test -- --run
run npm --prefix frontend run build
# Stages the built frontend assets and cloudflared, which the desktop crate bundles.
run npm --prefix desktop run build:runtime

run cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --check
run cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
run cargo test --manifest-path desktop/src-tauri/Cargo.toml
run npm --prefix desktop run release:test
