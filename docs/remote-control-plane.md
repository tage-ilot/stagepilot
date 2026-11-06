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
