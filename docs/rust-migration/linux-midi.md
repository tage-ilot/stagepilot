# Linux MIDI (midir) build requirements

`midir` 0.11 talks to ALSA on Linux through the `alsa` / `alsa-sys` crates. `alsa-sys` finds the library with
`pkg-config`, so a Linux build of any crate that depends on `midir` (today `crates/midi-probe`, later `stagepilot-midi`) needs:

| Distro | Packages |
|---|---|
| Debian / Ubuntu | `libasound2-dev` and `pkg-config` (runtime: `libasound2`) |
| Fedora / RHEL | `alsa-lib-devel` and `pkgconf-pkg-config` |
| Arch | `alsa-lib` and `pkgconf` |
| Alpine | `alsa-lib-dev` and `pkgconf` |

A plain `gcc` toolchain is also needed to build `alsa-sys`' C shim (the runner has `build-essential`).
Windows (WinMM) and macOS (CoreMIDI) need nothing extra.

## State of the self-hosted runner (`[self-hosted, stagepilot-linux]`, which is this CT120 host)

Checked 2026-10-17, nothing installed or changed:

- `libasound2t64` (runtime library) and `libasound2-data` are present.
- `libasound2-dev` (headers, `alsa.pc`) is **not** installed and `/usr/include/alsa` does not exist.
- `pkg-config` is **not** installed.

So `cargo build -p midi-probe` fails on the runner with `The system library 'alsa' required by crate 'alsa-sys' was not found`
(reproduced locally on the same host: `pkg-config` missing). The Linux job in `.github/workflows/rust.yml` therefore builds and tests the
workspace with `--exclude midi-probe`; the probe still runs on Windows and both macOS runners.

## Decision for the owner

Before phase 4 (MIDI) lands on Linux CI, one of:

1. Install `libasound2-dev pkg-config` on the runner (one `apt-get`, needs root; not done here), or
2. keep MIDI crates out of the Linux job and rely on the three native runners, or
3. gate `midir` behind a Cargo feature (`alsa-backend`), off by default on Linux CI.

Headless Linux users (the browser/headless use case in the plan) would need `libasound2` at runtime only; a static-linking option does not exist
in `midir` today.
