# Private-beta Remote control plane

## Scope and completion boundary

`control-plane/` is the minimum Cloudflare Worker control plane for explicitly
enrolled private-beta installations. It is not signup, an invite system, billing,
organization management, a relay, or a dashboard API. The Worker and its single
Durable Object are the only account-management tier; no owner's home server is in
the path.

This repository contains a locally verified implementation and manual deployment
workflow. It has not been deployed by this milestone. A real deployment requires
an approved Cloudflare account, zone, Worker route, Durable Object migration, and
secrets. Do not claim live multi-installation acceptance until the operations gate
at the end of this document has been run against two disposable enrolled installs.

## Trust boundaries

```text
Private beta administrator
  ADMIN_API_TOKEN -> authenticated aggregate metrics/revocation endpoints

Cloudflare Worker + singleton Registry Durable Object
  server-only account/zone IDs, CLOUDFLARE_API_TOKEN,
  ADMIN_API_TOKEN, INSTALLATION_SIGNING_KEY (GITHUB_RELEASE_TOKEN optional,
  absent while the release broker is deferred)
  -> exact named tunnel/configuration/DNS lifecycle

Enrolled StagePilot installation
  random installation ID + installation credential only
  -> authenticated provision/disable/reconcile/status
  <- stable generated hostname + that tunnel's run credential only
  -> existing remote_beta_control writes private connector.token + remote.json

cloudflared -> dedicated 127.0.0.1 Remote listener -> existing Remote auth/runtime
```

The Worker never returns its account token, admin token, or signing key. It does
not store cloudflared run tokens: it retrieves the exact tunnel token only after
route read-back and returns it to that authenticated installation. Installation
credentials are deterministic HMAC capabilities bound to random 128-bit IDs; only
the server-side signing key can derive them. The public enrollment endpoint accepts
only an app-generated nonce and creates the random identity and hostname itself.
Possession of the private beta is sufficient: there is no account, GitHub check,
OAuth, invite code, administrator-created bundle, or private bundle delivery.

No CORS policy is emitted. CORS is not authentication, and browsers are not a
supported provisioning client. Request bodies are capped at 4 KiB, all state and
mutation responses are `no-store`, and public errors omit provider bodies and
resource details.

## Durable identity and ownership

The singleton `Registry` Durable Object serializes enrollment, resource mutation,
and the global provider budget. Its storage contains installation metadata,
desired lifecycle state, aggregate counters, and enrollment nonce mappings. A
nonce of 8–128 safe characters
returns the same random ID, generated hostname, and installation credential when
replayed without consuming quota. Different nonces produce different identities
and credentials. Raw source addresses are never stored: IPv4 is canonicalized,
IPv6 is canonicalized to /64, and the result is HMAC-hashed with server-only key
material. Source quota records expire after 24 hours and are bounded by the
installation ceiling.

Hostnames are generated as:

```text
sp-<32 lowercase hex installation ID>.<REMOTE_HOST_SUFFIX>
```

## Finite beta guardrails

The measured beta defaults are intentionally generous for ordinary UI polling and
reconnect behavior:

- 3 new installations per canonical source IPv4 or IPv6 /64 per 24 hours;
- 500 active installations globally (`BETA_INSTALLATION_LIMIT`), with new
  enrollment controlled independently by `ENROLLMENT_ENABLED`;
- 120 authenticated status requests per installation per 60 seconds;
- 20 authenticated lifecycle mutations per installation per 60 seconds;
- a 30-second in-memory reconcile/token cache after confirmed provider read-back;
- 600 serialized Cloudflare API calls per 5 minutes, with normal provision work
  stopped at 480 so 120 calls remain for disable, revoke, and recovery.

Quota and capacity denials return a sanitized 429 or 503 with `Retry-After`. They
create no installation, Durable Object identity, tunnel, DNS record, or provider
resource. The desktop retries bounded transient responses with `Retry-After`,
exponential backoff, and jitter. Admin metrics expose only active/enrollment and
denial totals; raw addresses, keyed hashes, IDs, and credentials are omitted.

The zone has exactly one StagePilot-owned Free-plan `http_ratelimit` rule: 60
requests per source IP and Cloudflare colo per 10 seconds, followed by a 10-second
block. Its
expression is `(http.host wildcard "sp-*.illuminary.studio")`, so unrelated
`illuminary.studio` hosts and other ruleset phases are untouched. A WebSocket
connection contributes its HTTP upgrade/reconnect request, not each WSS message.
Managed DDoS protection remains in Cloudflare's separate managed phases.

Clients cannot submit a hostname. Each tunnel name includes both the installation
ID and a client-generated UUID generation. DNS ownership requires an exact CNAME,
exact generated hostname, exact tunnel target, proxied state, and exact tunnel
name in the record comment. Ambiguous, foreign, locally managed, or malformed
resources fail closed and are never adopted or deleted.

Stale older generation: a record and tunnel left by an earlier generation of the
SAME installation are replaced (record, then connections, then tunnel, each read
back) on provision, reconcile and disable only when all hold: proxied CNAME on
this installation's exact hostname; comment `stagepilot-<this installation
id>-<UUID>` with a different generation; content exactly `<tunnelId>.cfargotunnel.com`
for the single cloudflare-managed tunnel of exactly that name; and zero live
connections. Anything else still fails closed with `hostname ownership conflict`.

Provision first journals `desiredEnabled=true` and `enabling`, then reconciles the
exact tunnel name before any create. It writes and reads back the dedicated
loopback ingress plus deny-all catch-all, creates and reads back the exact DNS
record, and only then retrieves the tunnel run token. Lost write responses leave
the operation incomplete; a later authenticated `reconcile` discovers the exact
resource and continues without a duplicate.

Disable journals `desiredEnabled=false` and `revoking` before cloud mutation. The
installation client independently disables its local desired policy and removes
its connector token before making the request. The Worker deletes only the exact
owned DNS record, applies deny-all ingress, disconnects connectors, deletes the
tunnel, reads back absence, and then records `disabled`. A failed operation stays
`revoking`; retry `disable` or `reconcile`.

Administrative revoke follows the same cleanup but permanently marks the
installation revoked. Its credential then receives 401 for every installation
route. Re-enabling after ordinary disable requires a new generation, which causes
the existing StagePilot Remote listener/auth integration to invalidate old Remote
sessions. Viewer/Operator, CSRF, session cookies, LAN-PIN rejection, HTTPS/WSS
provenance, dedicated-loopback ingress, and production-runtime isolation remain in
the existing backend and are not reimplemented by this service.

## API

All routes except health and enrollment require an authorization bearer.

| Method and path | Principal | Behavior |
| --- | --- | --- |
| `GET /health` | public | Secret-free liveness only |
| `POST /v1/installations/enroll` | public beta app | Idempotent transparent enrollment; body `{nonce}` |
| `GET /v1/admin/metrics` | administrator | Sanitized aggregate counters only |
| `POST /v1/admin/installations/:id/revoke` | administrator | Permanent credential and resource revocation |
| `GET /v1/installations/:id/status` | matching installation | Sanitized lifecycle state plus any operator-queued `pendingActions` |
| `POST /v1/installations/:id/rate-limit/reset` | administrator | Queues a `rate_limit_reset` pending action for that one installation |
| `POST /v1/installations/:id/pending-actions/:actionId/ack` | matching installation | Clears a pending action after it was applied locally |
| `POST /v1/installations/:id/approval-requests` | matching installation | Raises an operator sign-off request; body `{reason}` |
| `GET /v1/installations/:id/approval-requests` | matching installation | Polls its own requests for approved/denied outcomes |
| `GET /v1/admin/approval-requests` | administrator | Lists requests (`?status=pending` by default) |
| `POST /v1/admin/approval-requests/:requestId/decide` | administrator | Final approve/deny; body `{approved}` |
| `POST /v1/installations/:id/provision` | matching installation | Idempotent exact-generation provision; body `{generation}` |
| `POST /v1/installations/:id/disable` | matching installation | Fail-closed cleanup |
| `POST /v1/installations/:id/reconcile` | matching installation | Resume persisted desired lifecycle |
| `POST /v1/installations/:id/revoke` | matching installation | Permanent self-revocation and cleanup |
| `GET /v1/releases/latest.json` | public beta app | Allowlisted latest signed updater metadata |
| `GET /v1/releases/vVERSION/ASSET` | public beta app | Exact immutable allowlisted updater payload only |

## Installation-side operation

On first local enable, the packaged desktop persists a random enrollment nonce and
keeps it as the durable installation identity across the installation's lifetime
(it is not a secret and the control plane never returns it), then calls the
fixed HTTPS enrollment origin, validates the returned identity, hostname, and
credential binding, and stores only the unique installation credential in
Windows Credential Manager or macOS Keychain. It persists only the origin,
installation ID, hostname, port, and nonce as non-secret recovery metadata.
Lost responses replay the same nonce. Disabling Remote Access genuinely revokes
the tunnel, DNS record, and installation credential, but retains the nonce
locally so a later re-enable replays it and the control plane reprovisions the
SAME hostname under a fresh generation/credential instead of minting a new
installation. Later lifecycle requests read the credential through the
authenticated native Tauri broker; the frontend never receives it.

This exact identity-preservation mechanism (the nonce kept as durable identity,
never cleared on disable/revoke) is also what lets an in-place app update and a
regenerated tunnel link ("Regenerate Remote link", see `reenroll()`) keep the
same installationId/credential/hostname across restarts and across a brand-new
generation respectively -- both reuse this same bootstrap persistence
(`backend/src/stagepilot/remote_bootstrap.py`), not a separate mechanism.

## Always-on lightweight check-in

Independent of whether Remote Access has ever been turned on,
`DesktopRemoteManager.run()` now performs a lightweight check-in once at
startup and then every several hours for the life of the process
(`DesktopRemoteManager.lightweight_checkin()`). This:

- reuses the existing anonymous `POST /v1/installations/enroll` nonce-based
  flow (`DesktopBootstrapStore.ensure_enrolled()`) to establish/confirm this
  installation's identity;
- reuses the existing authenticated `GET /v1/installations/:id/status` route
  to send a minimal "I exist" signal plus the self-reported `deviceName` (see
  below).

It deliberately does **not** call `provision`/`reconcile`/`disable`/`revoke`,
so it never creates or touches a Cloudflare tunnel or DNS record. Remote
Access's own gating is completely unchanged: the tunnel/DNS/dashboard stay
exactly as gated as before, behind `feature.intent().enabled`. The only new
behavior is that every installation now reports its existence and
`deviceName` to the control plane from first launch, with no opt-in -- an
explicit product/privacy decision; see the code comment at the check-in call
site and `docs/private-beta-enrollment-and-guardrails.md` for the data this
sends. This should be reflected in user-facing privacy policy/release notes
documentation (not handled by this change).

## deviceName

Every enroll/status/provision/reconcile request may optionally carry
`deviceName`, the installation's self-reported hostname (Python
`platform.node()`, validated/truncated to 1-253 chars with control characters
stripped before it is ever sent). It is **untrusted and display-only**: the
control plane never uses it for authentication, installation matching, or any
other security decision, and an installation can claim any value. It exists
purely so the fleet admin panel can show a human-meaningful hint next to the
generated hostname/label. An installation's `label` auto-fills from
`deviceName` only the very first time one is received while no label has ever
been set; once an operator sets any label, this auto-fill never overwrites it
again. `deviceName` is fully optional and backward compatible -- an
installation that never sends it keeps working exactly as before.

For the current backend-only validation path, create a private
`BetaControlConfig` JSON outside the connector export with the exact HTTPS Worker
origin, enrolled ID/hostname, absolute credential/state/export paths, and the
dedicated Remote port. Store the installation credential in the configured
credential file at mode 0600. The credential and state directory must not be
under the connector export.

Run from the installed backend environment:

```sh
python -m stagepilot.remote_beta_control enable --config /absolute/private/beta-control.json
python -m stagepilot.remote_beta_control status --config /absolute/private/beta-control.json
python -m stagepilot.remote_beta_control reconcile --config /absolute/private/beta-control.json
python -m stagepilot.remote_beta_control disable --config /absolute/private/beta-control.json
```

The client uses a local `flock` and fsynced state. Enable exports only
`remote.json` and the installation's cloudflared run token. Disable closes local
Remote and deletes that token before contacting the Worker. Never put the
installation credential in `remote.json`, `connector.token`, an argument, a log,
or the frontend.

## Deployment and secret rotation

Deployment is intentionally manual through `.github/workflows/deploy-control-plane.yml`
and the protected `stagepilot-control-plane` GitHub environment. A push does not
publish the Worker. Restrict deployment to the `main` branch and configure required
reviewers when the repository plan supports them. The current private-beta repository
plan does not support environment reviewers, so its compensating controls are a
private repository, a manual-only `workflow_dispatch`, environment-scoped secrets,
and an exact `main`-only deployment policy. Do not add a push trigger or broaden the
deployment branch while that limitation exists. Set these environment values (not
repository placeholders):

- variables: `CLOUDFLARE_ACCOUNT_ID`, `CLOUDFLARE_ZONE_ID`,
  `REMOTE_HOST_SUFFIX`, `REMOTE_PORT`, `ENROLLMENT_ENABLED`,
  `BETA_INSTALLATION_LIMIT`, `BETA_RELEASE_VERSIONS`,
  `BETA_LATEST_RELEASE_VERSION`;
- environment secrets: `CLOUDFLARE_API_TOKEN`, `ADMIN_API_TOKEN`,
  `INSTALLATION_SIGNING_KEY`. The release broker/in-app updater are deferred
  for this beta (see `docs/native-completion-runbook.md`), so no
  `STAGEPILOT_RELEASE_TOKEN` Actions secret is set and the Worker's optional
  `GITHUB_RELEASE_TOKEN` binding is intentionally absent; the deploy workflow
  no longer maps or requires it.

The account and zone IDs are 32 lowercase hexadecimal characters. The suffix is
the DNS suffix under which generated installation hostnames may be created. The
dedicated port is 1024-65535 and must not be local port 8765. The provider token
must have Worker Scripts deployment for the target account plus Account
Cloudflare Tunnel Edit and Zone DNS Edit for only the chosen zone. The independent
WAF operator must have Zone WAF Edit only for the chosen zone. The admin token and signing key are independent random values of at least 32 bytes.
If the release broker is ever promoted out of deferral, its GitHub token must be
read-only for `tage-ilot/stagepilot-beta` contents/releases, never returned to
clients, and never reused for repository writes.

Dispatch the workflow manually and approve the protected environment when an
environment reviewer is configured. It runs
tests and TypeScript build, validates every value without printing secrets, builds
a Wrangler dry-run preview, supplies all nine vars on the command line, installs
all three Worker runtime secrets through Wrangler, deploys the tracked Durable
Object migration/binding, then reads `wrangler secret list --format json` and
fails unless all three secret names are present. No secret value is printed. Read
back the deployed Worker version, `REGISTRY` binding, nine vars, secret names,
custom HTTPS origin, and `/health` before enrollment. Configure the custom Worker
hostname narrowly in Cloudflare if it is not already attached; do not alter
unrelated DNS records.

An independently authorized zone-WAF operator runs
`node control-plane/scripts/waf-rate-limit.mjs apply`; the narrower Worker/tunnel
token intentionally cannot edit WAF. The helper creates or replaces only the rule
with ref `stagepilot_remote_beta_rate_limit_v1`, refuses to overwrite any unrelated
rate rule, and reads back its ruleset ID, rule ID, expression, threshold, period,
and mitigation duration. To roll back the edge rule, delete that exact rule/ruleset
in
the `http_ratelimit` phase through the Cloudflare dashboard/API, verify the phase
has no StagePilot rule, and leave managed DDoS and every other phase unchanged.
For emergency enrollment rollback, set `ENROLLMENT_ENABLED=false` and redeploy;
existing authenticated installations and revocation remain available.

For provider/admin rotation, replace the matching protected environment secret,
manually rerun and approve the workflow, verify the secret-name read-back and live
health, and only then retire the old value. Update the administrator token file
out of band after admin rotation. Provider rotation does not invalidate
installation credentials or current connectors, but reconciliation fails until a
working runtime token is restored. Administrative revoke is the normal
single-site response to compromise.

Rotating `INSTALLATION_SIGNING_KEY` immediately invalidates every installation
credential. Inventory and disable/revoke affected installations first, preserve
the old key only in the approved recovery vault, deploy the new key, and have
each affected desktop transparently re-enroll before retiring the old key.

For code rollback, select the last known-good Cloudflare Worker version and keep
the current Durable Object binding and schema; migrations are forward-only and a
code rollback is not a storage rollback. Reapply/read back the four environment
vars and three secret names, then verify `/health` and one authenticated redacted
status request. Restore an old runtime secret only when it is known not to be
compromised. If schema compatibility is uncertain, stop enrollment and resource
mutation rather than running two registry writers or guessing at stored state.

Back up Durable Object storage through an approved Cloudflare export/backup
procedure before migration. Do not copy its state into a second active writer.
Provider resources remain recoverable by exact generated names, but loss of the
registry or signing key is an administrative incident, not permission to adopt or
delete ambiguous resources.

## Verification evidence and remaining live gate

Local control-plane validation passes 13 tests: six Worker tests cover
unauthenticated allocation rejection, missing-runtime-secret failure, idempotent
enrollment, distinct identities/credentials, two simultaneously provisioned
isolated tunnels and DNS routes, cross-credential denial, independent
disable/revoke, restart reconciliation after a lost create response, and
foreign-route rejection; seven operations tests cover deployment wiring and
validation, secret-name read-back, strict bundle schema and permissions, redacted
output, idempotent recovery, and per-installation binding. TypeScript strict
checking and `npm audit` pass. Backend targeted validation passes the installation
client plus existing Remote control/lifecycle tests, Ruff, and strict mypy.

Before declaring the Cloudflare milestone live, enroll two disposable
installations and verify all of the following by exact API read-back and the
existing browser proof:

- distinct installation IDs, credentials, hostnames, tunnel IDs, DNS ownership,
  connector tokens, identity databases, and Remote sessions;
- each stable hostname passes HTTPS, authenticated WSS/reconnect,
  Viewer/Operator authorization, CSRF, anonymous/LAN-PIN rejection, and logout;
- connector-only restart preserves the installation and active generation;
- disabling installation A closes its local listener first, removes only A's DNS
  and tunnel, rejects A's old sessions/token, and leaves B fully connected;
- permanent revoke leaves A unauthorized and restart-safe while B remains intact;
- provider outage/lost response stays fail-closed and authenticated reconciliation
  finishes without duplicate or cross-owned resources.

Record redacted resource IDs and timestamps in `remote-provisioning.md`. Never
record any account, admin, signing, installation, session, or tunnel token.
