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

The value is a deployment secret: it is supplied as the `TESTING_SOURCES` repository/environment
variable of the deploy workflow, and must never be committed (not in the repository, tests, docs
or commit messages). If the testing network's address changes, the setting must be updated;
until then marking silently stops and new test installations look like ordinary ones.
Existing installations are not changed (nothing stored says which source they came from); they
leave through the 48h sweep.

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
