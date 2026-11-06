#!/usr/bin/env bash
# One command: build v2.42.0 and the current checkout headless, run the upgrade test, write a report.
#
#   scripts/upgrade-test/run.sh [report.md]
#
# Environment (all optional):
#   OLD_TAG          release tag to upgrade from (default v2.42.0)
#   OLD_BIN, NEW_BIN prebuilt stagepilot-headless binaries; skips the matching build
#   TARGET_ROOT      where the two cargo target dirs live (default ~/.cache/stagepilot-upgrade-test)
#   KEEP=1           keep the scratch directories
# Exit code: 0 all checks pass, 1 a check failed, 2 harness/build error. Not run in CI.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(git -C "$here" rev-parse --show-toplevel)"
tag="${OLD_TAG:-v2.42.0}"
target_root="${TARGET_ROOT:-$HOME/.cache/stagepilot-upgrade-test}"
report="${1:-$here/out/REPORT.md}"
mkdir -p "$target_root" "$(dirname "$report")"

if [ -z "${OLD_BIN:-}" ]; then
  wt="$target_root/worktree-$tag"
  if [ ! -d "$wt" ]; then
    git -C "$repo" fetch --quiet --tags origin || true
    git -C "$repo" worktree add --detach "$wt" "$tag"
  fi
  echo "== building $tag ($wt)"
  (cd "$wt" && CARGO_TARGET_DIR="$target_root/target-old" cargo build -p stagepilot-headless)
  OLD_BIN="$target_root/target-old/debug/stagepilot-headless"
fi
if [ -z "${NEW_BIN:-}" ]; then
  echo "== building current checkout ($repo)"
  (cd "$repo" && CARGO_TARGET_DIR="$target_root/target-new" cargo build -p stagepilot-headless)
  NEW_BIN="$target_root/target-new/debug/stagepilot-headless"
fi

old_label="$tag ($(git -C "$repo" rev-parse --short "$tag^{commit}" 2>/dev/null || echo '?'))"
new_label="$(git -C "$repo" rev-parse --abbrev-ref HEAD) $(git -C "$repo" rev-parse --short HEAD)"
args=(--old "$OLD_BIN" --new "$NEW_BIN" --report "$report" --old-label "$old_label" --new-label "$new_label")
[ -n "${KEEP:-}" ] && args+=(--keep)
exec python3 "$here/upgrade_test.py" "${args[@]}"
