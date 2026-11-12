#!/usr/bin/env bash
# Installs the pinned cargo-nextest release (prebuilt binary) into $CARGO_HOME/bin.
#
# No third-party action and no `cargo install` (a source build takes minutes on every runner):
# the archive comes from the official GitHub release and its SHA-256 is checked against the
# digests below before anything is unpacked. To upgrade, change NEXTEST_VERSION and the three
# digests together (GitHub shows each asset's digest on the release page).
#
#   scripts/ci/install_nextest.sh [destination-dir]
set -euo pipefail

NEXTEST_VERSION="0.9.151"

case "$(uname -s)" in
  Linux)
    target="x86_64-unknown-linux-gnu"
    sha256="1d4ce302b820c41f4b89c5fb9bce45f0f0c94882c1cf08fc360caf08be6e7df4"
    exe="cargo-nextest"
    ;;
  Darwin)
    target="universal-apple-darwin"
    sha256="d1d7d025d4bfc9fb154cd76aea3949bd78534829a0589abefa6e5dd4edb135dd"
    exe="cargo-nextest"
    ;;
  MINGW* | MSYS* | CYGWIN*)
    target="x86_64-pc-windows-msvc"
    sha256="e51bdb4b5883df48f335381b287378edb244318fac65ae0adaf6b9e69298246b"
    exe="cargo-nextest.exe"
    ;;
  *)
    echo "install_nextest.sh: unsupported OS $(uname -s)" >&2
    exit 1
    ;;
esac

dest="${1:-${CARGO_HOME:-$HOME/.cargo}/bin}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

archive="cargo-nextest-${NEXTEST_VERSION}-${target}.tar.gz"
url="https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-${NEXTEST_VERSION}/${archive}"

curl --fail --silent --show-error --location --retry 3 --output "$tmp/$archive" "$url"

if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$tmp/$archive" | cut -d' ' -f1)"
else
  actual="$(shasum -a 256 "$tmp/$archive" | cut -d' ' -f1)"
fi
if [ "$actual" != "$sha256" ]; then
  echo "install_nextest.sh: SHA-256 mismatch for $archive" >&2
  echo "  expected $sha256" >&2
  echo "  actual   $actual" >&2
  exit 1
fi

tar -xzf "$tmp/$archive" -C "$tmp" "$exe"
mkdir -p "$dest"
install -m 755 "$tmp/$exe" "$dest/$exe"
"$dest/$exe" nextest --version | head -n 1
