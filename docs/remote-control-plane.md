# Remote control plane: installation expiry

Applies to the Cloudflare Worker in `control-plane/` (`stagepilot-beta-control-plane`).

## Expiry rule

An installation expires when `now - createdAt >= 48h` and it is not pinned (`keep`).
The TTL is a constant (`DEFAULT_INSTALLATION_TTL_HOURS = 48`) with an optional Worker var
`INSTALLATION_TTL_HOURS` (must be a number >= 1; invalid values fall back to 48).

"Expire" reuses the admin revoke path (`disable(installation, true)`): the tunnel and DNS record
are deleted if present and `activeInstallations` drops by one. The installation is also set
`archived: true`, so it stays hidden in the admin list. Enrollment limits and quotas are unchanged.

## Pinning

`PATCH /v1/admin/installations/:id/keep` with `{"keep": true|false}` (admin bearer token; same id
format and validation as `/label` and `/archive`; 401 without a token, 400 on a non-boolean).
Pinned installations are never auto-revoked. The admin list includes `keep` and `expiresAt`
(null when pinned or revoked). The admin page has a Pin toggle and shows "expires in Nh" /
"expired". Unpinning an installation older than the TTL makes it eligible on the next sweep.

## Grandfather migration

The first time the sweep runs, every non-revoked installation in phase `provisioned` is marked
`keep: true`. This is recorded under the storage key `sweep:grandfathered` and never repeats, so
installations that provision later are not grandfathered.

## Sweep

A Durable Object alarm on `Registry` runs hourly. The first alarm is armed on the first request
after deploy; every run re-arms the next one, including after an error (no wrangler.toml change).
Per run, oldest first:

- up to 100 expired installations with no tunnel/generation (no provider calls);
- up to 10 expired installations that need Cloudflare calls, using the `recovery` provider lane
  (bounded by `PROVIDER_TOTAL_BUDGET`; if the budget is exhausted the remaining provider-backed
  installs wait until the next run).

A failure on one installation logs a generic message, is skipped, and is retried next hour.
A never-provisioned backlog of ~400 drains in about 4 runs.

After the revoke step the same run also purges offline test installations (next section), up to
100 purges per run, which is in addition to, not instead of, the caps above.

## Testing sources

Installations that enroll from our own testing network are test installations. The deployment
setting `TESTING_SOURCES` (comma-separated, same format and normalisation as
`ENROLLMENT_EXEMPT_SOURCES`: exact IPv4 addresses, IPv6 as a canonical `/64`; invalid entries are
dropped) lists those sources. Unset, empty or all-invalid means the feature does nothing.

When a **new** installation is created (`POST /v1/installations/enroll`, and the authenticated
`reenroll` route) from a listed source it is stored with `testing: true` and `archived: true`.
Only that boolean is stored, never the address. Effects:

- Location shows `Testing` (the admin hint says `testing`), below an operator `locationOverride`
  and above the app-reported city, the network guess and `Test Install`.
- It is hidden by the existing archived behaviour (the admin page hides archived rows unless
  "Show archived" is ticked; the API returns them with `archived: true`).
- It is revoked by the normal 48h expiry sweep, with the usual tunnel/DNS cleanup. It is never
  grandfathered, and `PATCH .../keep` with `{"keep": true}` answers 409 until the operator clears
  `testing` with `PATCH /v1/admin/installations/:id/testing` and `{"testing": false}` (clear-only;
  there is no way to set it by hand, and `archived` is left as is).
- Enrollment quotas and limits are unchanged; a testing source is not exempt unless it is also in
  `ENROLLMENT_EXEMPT_SOURCES`.

The value is a deployment secret: it is supplied as the `TESTING_SOURCES` GitHub **secret** of the
deploy workflow (not a variable: this repository is public, a variable's value is printed in the
public workflow log, a secret's is masked), and must never be committed (not in the repository,
tests, docs or commit messages). If the testing network's address changes, the setting must be updated;
until then marking silently stops and new test installations look like ordinary ones.
Existing installations are not changed (nothing stored says which source they came from); they
leave through the 48h sweep.

## Auto-archive of known test hostnames

CI runners and the AgentHub dev host enroll constantly (428 GitHub-runner installations enrolled in
one burst) and used to stay in the default admin list until archived by hand. An installation now
hides itself when its self-reported `deviceName` is a known test host (`isKnownTestDevice`:
`runnervm*`, the macOS runner UUID names, exactly `agent-hub`, exactly `stagepilot-ci`;
case-insensitive, whole-name match).

- **When:** only at the moment a `deviceName` is first applied or changes (enroll, the anonymous
  check-in replay of `enroll`, `status`, `provision`, `reconcile`). A check-in that repeats the
  stored name does nothing, so existing rows are not touched retroactively.
- **Effect:** sets `archived: true`, nothing else. It never revokes, never sets `testing`, and does
  not touch credentials, access, quotas, `activeInstallations` or the 48h expiry (the sweep still
  revokes these installations at `createdAt + 48h`, exactly like any other unpinned one).
- **Never for:** a pinned (`keep`) installation, one that has ever provisioned (`everProvisioned`
  or phase `provisioned`), or one that is turning remote access on (`desiredEnabled`). The rule
  only ever sets `archived`; it never clears it, so a later change back to a real host name leaves
  the row archived (use Unarchive).
- **Operator override guard:** the admin archive route (`PATCH .../archive`, either Archive or
  Unarchive) stores `archivedByOperator: true`. From then on the rule never changes `archived`
  for that installation, so an operator's Unarchive is not undone by the next check-in, even if
  the name changes to another test host. The flag is internal and is not returned by the API.
- `deviceName` stays untrusted and display-only: it is never used for auth, access or quotas, and
  a client can only ever hide its own row, not gain anything.
- The admin page's **Active today / Total** stat no longer counts archived installations as
  active today (Total is unchanged), and the Installations heading shows how many rows are
  shown versus archived (for example `1 shown, 464 archived`) whenever any are archived.

Rollback: redeploy the previous Worker version. Rows already archived keep `archived: true` and a
possible `archivedByOperator: true`, both harmless to older versions (unknown fields are ignored).

## Permanent removal of offline test installations

> **This deletes data permanently.** A purged installation cannot be restored. Rolling back the
> Worker version does not bring the records back; only a Durable Object backup would.

Revoking and archiving a test installation left its record, alerts, approvals and diagnostics in
storage forever. The hourly sweep now deletes them completely.

**Identified as a test install:** `installation.testing === true` (enrolled from `TESTING_SOURCES`)
or `isKnownTestDevice(deviceName)` (`runnervm*`, the macOS runner UUID names, exactly `agent-hub`,
exactly `stagepilot-ci`).

**Not online for 48 hours:** `lastSeen(inst) = lastSeenAt ?? updatedAt ?? createdAt`, and
`now - lastSeen >= TEST_PURGE_OFFLINE_HOURS` (48). Optional Worker var `INSTALLATION_TEST_PURGE_HOURS`
(a number >= 1; invalid values fall back to 48).

**`lastSeenAt`:** an ISO time set whenever an installation proves its credential: the status
check-in, provision, reconcile, alerts, approvals, diagnostics, pending-action ack, reenroll,
reactivate, and the anonymous check-in replay of `enroll` (a request from a revoked installation
that still presents its credential also counts: it is online). It is written at most once per 10
minutes per installation (`LAST_SEEN_WRITE_INTERVAL_MS`), so storage writes do not multiply. A wrong
credential never sets it. Unlike `updatedAt` it is not bumped by admin actions (label, pin, archive).
Installations that have not checked in since this field was introduced fall back to `updatedAt`,
which is conservative (admin actions only delay a purge). It is returned by
`GET /v1/admin/installations` as `lastSeenAt` (null when never recorded); the admin page shows
"last seen" from `lastSeenAt || updatedAt`.

**Purge rule.** In the sweep, after the revoke step, a test installation is purged when ALL hold:

- it is not pinned (`keep`);
- it was never provisioned (`everProvisioned` is not set) and its phase is not `provisioned`;
- `lastSeen` is at least the purge window ago.

Whether it was already revoked or archived does not matter, so the existing revoked test rows drain
too. The conditions are judged on the state at the start of the run, so the revoke done by the same
run (which bumps `updatedAt` and resets the phase) neither delays nor enables a purge. The 48h
from-creation revoke rule is unchanged. Real installations are never matched: pinned, provisioned or
ever-provisioned installs and any install whose name is not a known test name and that was not
enrolled from a testing source are always kept, however long they are offline. `deviceName` is
self-reported and still never affects authentication; the only thing a real installation that
spoofs a test hostname can do is get **itself** purged, and only if it is unpinned and has never
provisioned.

**What is deleted** (per installation id, children first, the `installation:<id>` record last, in
`storage.delete` batches of at most 128 keys):

- `installation:<id>`
- `alert:<id>:*`
- `approval:<id>:*`
- `pending-action:<id>:*`
- `diagnostics-meta:<id>:*` and the matching R2 objects in `DIAGNOSTICS` (each meta record's `key`,
  the same way the diagnostics expiry does it); `diagnosticsBytesUsed` is reduced by their size
- the `enrollment:<nonce>` idempotency records that point at the id
- the in-memory token cache entry

If the installation still has provider resources (`generation` or `tunnelId`) the existing revoke
path runs first on the `recovery` lane and the record is deleted only after it succeeds. Any failure
(provider, R2, storage) logs a generic message, leaves the record, and is retried next hour.

**Counters.** `activeInstallations` drops by one only when the installation was still counted as
active (never twice, never below 0). `enrollments` is a historic total and is not changed. The stats
object gains an optional `purgedInstallations` total (stored stats without it stay valid).

**Limits.** At most 100 purges per run (oldest `lastSeen` first), of which at most 10 may need
Cloudflare calls, on top of the revoke caps. One failing installation does not stop the run and the
alarm is re-armed in `finally`.

**A purged installation calling back** is indistinguishable from an id that never existed (401
`unauthorized` on installation routes, 404 on admin routes). Enrollment: `enrollment:<nonce>` maps
a nonce to an installation id. The purge removes those records, and even a leftover one is harmless:
enroll finds no installation behind it and mints a brand-new complete installation with a new id,
so a purged record can never reappear half-formed.

Rollback: redeploy the previous Worker version. Already purged records are gone for good;
`lastSeenAt` is an unknown field to the older version and is ignored.

## Locations, Test Install, and the manual override

### App-reported location

The network guess below is often wrong (a Rancho Mirage studio shows as Los Angeles), so the app
reports its own city. About once a week (on the first check-in after start when nothing is stored
or the stored city is older than 7 days) the app calls `GET https://api.db-ip.com/v2/free/self`
(db-ip free API, no key), formats the answer as `City, ST` (US) or `City, Country`, and stores
`{location, looked_up_at}` in `location.json` beside the installation identity. The always-on
check-in (`GET /v1/installations/:id/status`, also `provision`/`reconcile`) sends the stored value
as a `location` query parameter (JSON body on the POST routes), next to `deviceName`. There is no
user-facing setting. `STAGEPILOT_DISABLE_IP_LOOKUP=1` turns the lookup off.

A failed lookup (network error, HTTP 429, bad JSON, no city) keeps the previous value and is not
retried for at least an hour (at most 4 failed attempts per day). It runs on its own task and
never delays or fails the check-in.

The control plane validates the value (string, trimmed, at most 80 characters, no control
characters; bad values are ignored), stores it as `reportedLocation`, and treats it as
**untrusted and display-only**, exactly like `deviceName`. A request with no `location` never
erases it; an empty `location` clears it. The admin list exposes `reportedLocation` and a
`locationSource` (`admin`, `app`, `network`, `none`); the page shows a small hint next to each
location (`IP lookup by app`, `network guess`, `set by admin`).

Privacy: the app and the control plane **never store, log or send the raw IP address**; only the
final city text is reported. db-ip itself sees the address, as any web service you call does. The
admin override beats everything. Accuracy is city/metro level: IP lookups can be off by roughly
5-30 miles. Apps that predate this keep using the network guess until a release containing the
lookup is installed; use the manual override meanwhile. The free API allows about 1,000 requests
per day per address, far above one a week. Credit: IP Geolocation by [DB-IP](https://db-ip.com).

### Network guess and manual override

The admin page's **Locations** list (API route `/v1/admin/cities`, response key `cities`, kept for
compatibility) groups non-revoked installations by their effective location. The same value is
the `location` shown on each installation row and on alerts.

Automatic location comes from Cloudflare edge geolocation (`request.cf` city/region/country).
That reflects the registered area of the network address the request came from (often the ISP's
hub or a VPN/carrier exit), **not** the physical site, so it can be wrong. StagePilot still never
records or uses the raw IP address.

Effective location, in order:

1. `locationOverride`, if set by the operator;
2. `reportedLocation`, the city the app itself reports (see below);
3. geolocation city, then region, then country;
4. otherwise `Test Install` only if the installation is not pinned **and** its self-reported
   `deviceName` is a known test host (case-insensitive, whole-name match): a GitHub-hosted Linux
   runner (`^runnervm[a-z0-9]+$`), a GitHub-hosted macOS runner
   (`^[a-z]{3}\d{2}-[a-z]{2}\d+-<uuid>-<12 hex>\.local$`), exactly `agent-hub`, or exactly
   `stagepilot-ci`. Everything else with no location, including pinned installs and installs
   with no `deviceName`, is `Unknown`. `deviceName` is untrusted and this is display-only.

`PATCH /v1/admin/installations/:id/location` with `{"location": "Pasadena"}` (admin bearer token;
same id format as `/label`). The value is trimmed, at most 80 characters, with no control
characters (400 otherwise; 404 for an unknown id). An empty string clears the override. The
override is display-only: never used for auth or any security decision, and geolocation refreshes
never overwrite it. The admin list exposes it as `locationOverride`, and the page has an inline
"Set location" editor next to the name editor.

### Purge test installations now (operator action)

`POST /v1/admin/installations/purge-test` with `{"confirm": true}` (admin token) deletes test installations immediately instead of waiting for the 48h-offline rule. Eligibility is the same as the sweep minus the time condition: a test installation (testing network or known test hostname) that is not pinned, was never provisioned and is not currently provisioned. Deletion goes through the same path as the sweep (provider teardown first, then children, counters and the record last). At most 250 per call; the reply is `{purged, failed, remaining}`, so call again while `remaining` is above 0. Deleted records cannot be restored.

### Legacy nameless test installs and the active counter

Installations from before device names were reported have no `deviceName`. One that is revoked and was never provisioned counts as a test installation (it is dead weight from the earliest test runs), so the 48h-offline sweep and the purge-now action delete it. A live, pinned or ever-provisioned installation is never matched by this. The sweep and the purge-now action also recount `activeInstallations` from the stored records, so a drifted counter (which the 500 cap and the admin "Total" read) corrects itself within an hour.
