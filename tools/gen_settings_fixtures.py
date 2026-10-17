"""Write synthetic Python-written settings and auth fixtures for the Rust core tests.

Run from backend/:  uv run --frozen python ../tools/gen_settings_fixtures.py
Everything here is synthetic: no real hosts, ids, secrets or names.
"""

from __future__ import annotations

import json
from pathlib import Path

from argon2 import PasswordHasher
from argon2.low_level import Type

from stagepilot.core.settings import PersistentSettings, SettingsFileStore
from stagepilot.services.dashboard_auth import hash_dashboard_pin

OUT = Path(__file__).resolve().parents[1] / "crates" / "stagepilot-core" / "tests" / "fixtures"
OUT.mkdir(parents=True, exist_ok=True)


def write(name: str, payload: object) -> None:
    (OUT / name).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


# 1. A file as the current Python app writes it (store.save), with a PIN hash and edited values.
settings = PersistentSettings(
    timezone="America/Chicago",
    server_port=9123,
    lan_access=True,
    web_dashboard_pin_hash="pbkdf2_sha256$210000$c2FsdHNhbHRzYWx0MTIzNA==$PLACEHOLDER",
)
settings = settings.model_copy(update={"web_dashboard_pin_hash": hash_dashboard_pin("2468")})
store = SettingsFileStore(OUT / "_scratch.json")
store.save(settings)
(OUT / "python_written_v2.json").write_text((OUT / "_scratch.json").read_text(encoding="utf-8"), encoding="utf-8")
(OUT / "_scratch.json").unlink()

# 2. Schema 1 files (no schema_version): untouched defaults, an explicit MIDI choice, and release_channel.
write("python_v1_untouched.json", {
    "integration_modes": {"service_source": "demo", "midi_source": "real", "timer_output": "simulated"},
    "midi": {"enabled": True},
    "timezone": "America/Denver",
    "release_channel": "beta",
})
write("python_v1_midi_chosen.json", {
    "schema_version": 1,
    "integration_modes": {"service_source": "demo", "midi_source": "real", "timer_output": "simulated"},
    "midi": {"enabled": True, "input_name": "Synthetic MIDI Port 1"},
})
write("python_v2_release_channel.json", {"schema_version": 2, "release_channel": "stable", "timezone": "UTC"})
write("python_v2_unknown_key.json", {"schema_version": 2, "bogus_key": 1})

# 3. Hashes made by the Python auth code. The PIN hash is PBKDF2; remote passwords are Argon2id.
hasher = PasswordHasher(time_cost=3, memory_cost=65536, parallelism=1, type=Type.ID)
write("auth_hashes.json", {
    "pin": "2468",
    "pin_hash": hash_dashboard_pin("2468"),
    "argon2_password": "correct horse battery staple",
    "argon2_hash": hasher.hash("correct horse battery staple"),
    "argon2_hash_other": hasher.hash("another synthetic passphrase"),
    "argon2_other_password": "another synthetic passphrase",
})
print("wrote", sorted(p.name for p in OUT.iterdir()))
