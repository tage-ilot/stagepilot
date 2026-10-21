#!/usr/bin/env python3
"""Semantically validate that Actions jobs use the correct runner policy.

Policy: the repository is public, so GitHub-hosted standard runners are free and
unlimited, and there is no self-hosted runner. A dead or missing self-hosted
runner makes every job that targets it queue forever, so `self-hosted` is
forbidden everywhere. Linux jobs use the pinned hosted image `ubuntu-24.04`;
native Windows and macOS jobs use hosted `windows-*` / `macos-*` labels.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / ".github" / "workflows"
LINUX_LABELS = ["ubuntu-24.04"]
NATIVE_LABEL = re.compile(r"^(?:windows|macos)-", re.IGNORECASE)
REQUIRED_ACTIONS = {
    "actions/checkout",
    "actions/setup-node",
    "astral-sh/setup-uv",
}
EXPECTED_NATIVE = {
    "ci.yml:desktop",
    "ci.yml:desktop-macos-lifecycle",
    "release-macos.yml:build",
    "macos-signing-stability.yml:stability",
    "release-windows.yml:build",
    "rust-installer-ci.yml:build",
}
# Present only on branches that carry the Rust workspace; optional elsewhere.
OPTIONAL_NATIVE = {"rust.yml:rust", "spike-rust.yml:rust"}


def labels(value: object) -> list[str]:
    if isinstance(value, str):
        return [value]
    if isinstance(value, list) and all(isinstance(item, str) for item in value):
        return value
    raise AssertionError(f"runs-on must be a string or string list, got {value!r}")


def main() -> None:
    linux_jobs: list[str] = []
    native_jobs: list[str] = []
    actions: set[str] = set()

    workflow_paths = sorted(WORKFLOWS.glob("*.yml"))
    assert workflow_paths, "no workflows found"

    for workflow_path in workflow_paths:
        document = yaml.safe_load(workflow_path.read_text(encoding="utf-8"))
        assert isinstance(document, dict), f"{workflow_path.name}: workflow must be a mapping"
        jobs = document.get("jobs")
        assert isinstance(jobs, dict) and jobs, f"{workflow_path.name}: jobs must be a mapping"

        for job_id, job in jobs.items():
            assert isinstance(job, dict), f"{workflow_path.name}:{job_id}: job must be a mapping"
            key = f"{workflow_path.name}:{job_id}"
            job_labels = labels(job.get("runs-on"))
            assert "self-hosted" not in job_labels, (
                f"{key}: there is no self-hosted runner; a job that targets one queues forever"
            )

            if key in EXPECTED_NATIVE | OPTIONAL_NATIVE:
                native_jobs.append(key)
                assert any(
                    NATIVE_LABEL.match(label) or "matrix.runner" in label for label in job_labels
                ), f"{key}: native job must use a hosted windows-*/macos-* runner label, got {job_labels}"
            else:
                linux_jobs.append(key)
                assert job_labels == LINUX_LABELS, (
                    f"{key}: Linux job must use exactly {LINUX_LABELS}, got {job_labels}"
                )

            for step in job.get("steps", []):
                if isinstance(step, dict) and isinstance(step.get("uses"), str):
                    actions.add(step["uses"].split("@", 1)[0])

    assert EXPECTED_NATIVE <= set(native_jobs) <= EXPECTED_NATIVE | OPTIONAL_NATIVE, (
        f"native job inventory changed: expected {sorted(EXPECTED_NATIVE)}, got {sorted(native_jobs)}"
    )
    missing_actions = REQUIRED_ACTIONS - actions
    assert not missing_actions, f"required bootstrap actions are no longer allowed/present: {sorted(missing_actions)}"

    print(json.dumps({"linux_jobs": linux_jobs, "native_hosted_jobs": native_jobs}, indent=2))


if __name__ == "__main__":
    main()
