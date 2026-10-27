# Security model

StagePilot will handle vendor credentials and control production systems. The
current foundation minimizes its network and desktop permissions, but it has not
completed a production security review.

## Current defaults

- The server binds to `127.0.0.1:8765` unless LAN dashboard access is explicitly enabled.
- Browser dashboard access requires a PIN by default; replace the initial
  `1234` PIN before enabling LAN access.
- Demo mode is enabled and needs no external credentials.
- Local environment and configuration files are ignored by Git.
- The Tauri window receives only `core:default`; shell execution and broad file
  system capabilities are not granted.
- The Tauri content security policy limits connections to the local server and
  local Vite development server.
- The server runs inside the desktop process (no separate backend executable), so the
  shell supervises it directly; see `desktop-packaging.md`.

Loopback binding reduces exposure but is not authentication. Another process
running as the same user may still be able to call a local service.

## Credential rules

- Never commit real credentials, paste them into issues, or use them in tests.
- Never put secrets in URLs, exception messages, event payloads, public state,
  WebSocket messages, analytics, or diagnostic exports.
- Accept a secret in a dedicated write-only input path and return only whether
  it is configured.
- Redact by field allow-list before structured logging; do not rely solely on
  matching known key names after serialization.
- Use the operating system's credential store when persistent secret storage is
  implemented. A plain JSON settings file is not an acceptable final store.
- Rotate a credential immediately if it may have entered Git history or logs.

The tracked `.env.example` contains empty placeholders only. An untracked `.env`
is convenient for development, not a production secret vault.

## Remote access

Remote and LAN access are disabled by default. The general settings panel offers
an explicit LAN mode for trusted private production networks. Browser access is
protected by an HTTP-only session cookie after PIN verification; API and
WebSocket requests are rejected until authentication succeeds. The PIN is
stored as a salted PBKDF2 hash and is never returned by the settings API.
Protection can be disabled explicitly.

LAN mode still has no TLS, so every device on that network must be treated as
trusted and the PIN must not be reused for another account or service. Do not
enable it on public Wi-Fi or expose its port to the internet.

Internet-facing access is unsupported. Before listening beyond a trusted LAN,
the project needs an explicit threat model and tested controls for:

- user and device authentication;
- authorization for control actions versus read-only state;
- TLS termination and certificate management;
- strict allowed origins and WebSocket origin checks;
- rate limits and bounded request sizes;
- firewall and network-segmentation guidance; and
- log, error, and settings response review for sensitive data.

CORS controls which browser origins may read responses; it does not protect an
API from non-browser clients and must not be presented as access control.

## External data and control actions

Planning Center titles, song names, MIDI labels, ProPresenter timer names, and
plugin messages are untrusted data. Validate lengths and types, render text
without HTML injection, and avoid interpolating values into shell commands or
paths.

Control actions should be typed, authenticated when remote access exists, safe
at index boundaries, and protected against accidental duplicate delivery. A
zero-duration song must never start a timer. Network calls require timeouts and
bounded retries so a malicious or broken peer cannot exhaust workers forever.

## Desktop packaging

Release packaging is described in `desktop-packaging.md` and `macos-adhoc-signing.md`;
it must keep defining and testing:

- server lifecycle supervision;
- per-platform code signing and notarization;
- update signature verification and rollback behavior;
- minimum Tauri capability permissions;
- a production content security policy without development origins; and
- clean failure behavior for port conflicts and stale child processes.

Do not add Tauri shell or unrestricted process permissions merely to make a
development command convenient. Introduce the narrowest permission with a clear
call site and review it as a security-sensitive change.

## Reporting a vulnerability

Do not publish exploit details or secrets in a public issue. Until a dedicated
private reporting address is documented, submit a redacted request asking a
maintainer for a private contact channel.
