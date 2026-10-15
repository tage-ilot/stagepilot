# Planning Center OAuth 2.0 sign-in — scoping & design

Status: **SCOPING ONLY — not approved for implementation.** This document
answers the 8 scoping questions from the kanban task. No code from this
task should ship; a follow-up implementation task should be filed only
after the operator reviews this doc.

## 0. Today vs. proposed

Today: user creates a Personal Access Token (App ID + Secret) in their own
Planning Center developer account, pastes both into
`PlanningCenterSetupPanel.tsx`. Stored via `keyring` in
`backend/src/stagepilot/core/settings.py` (`KEYRING_SERVICE = "StagePilot"`,
`KEYRING_ACCOUNT = "planning-center-secret"`), used for HTTP Basic Auth
in `backend/src/stagepilot/plugins/planning_center/client.py`.

Proposed: add "Sign in with Planning Center" using the standard OAuth 2.0
Authorization Code + PKCE flow, alongside (not replacing) the existing
manual method.

## 1. One-time app registration (OPERATOR ACTION REQUIRED)

Registering a StagePilot OAuth application in Planning Center's Developer
Center is a one-time, product-wide action, not a per-installation one. It
must be done by a human with an Organization Administrator role on the
Planning Center account used to register it (PCO restricts OAuth app
creation to org admins as of March 2023). This is **not something an
autonomous agent should or can do** — it requires the operator's own (or
a dedicated StagePilot-controlled) Planning Center org identity, and the
resulting `client_id` (and `client_secret`, see §2) become a
product-wide constant baked into config/build, analogous to how any
other third-party API key for the product itself would be provisioned.

Operator action item: register the app at
`https://api.planningcenteronline.com/oauth/applications` (or the
Developer Center UI), set the redirect URI pattern to
`http://127.0.0.1:*/callback` (PCO supports loopback wildcard redirect
URIs for installed apps — verify at registration time; if a wildcard port
isn't accepted, a fixed local port or a chain of pre-registered ports may
be needed instead, see §3), and hand the resulting `client_id`/
`client_secret` to whoever builds the feature via a secure channel (not
committed to git, not pasted into chat).

## 2. Where client_id / client_secret live — the PKCE caveat

The task brief assumed PKCE alone might let StagePilot avoid a
client_secret entirely, as a generic "public client" pattern. **This is
not true for Planning Center specifically.** PCO's own authentication
docs show the token-exchange call for the PKCE flow still requires
`client_secret` as a form parameter alongside `code_verifier`:

```
curl -X POST https://api.planningcenteronline.com/oauth/token \
     -F grant_type=authorization_code \
     -F code=... -F code_verifier=... \
     -F client_id=... -F client_secret=... \
     -F redirect_uri=...
```

(source: https://api.planningcenteronline.com/docs/overview/authentication#pkce
— confirmed directly from PCO's docs, not assumed)

So PCO does not offer a genuinely public/secret-less client type; every
registered app is effectively "confidential" and PKCE only adds
authorization-code-interception protection, it does not remove the
secret requirement. This changes the recommendation:

**Recommendation: the client_secret must not be embedded in the desktop
app.** A secret shipped in a distributed desktop binary (even
obfuscated) is recoverable by any user and would let anyone impersonate
the StagePilot OAuth app. The token-exchange step (the only step that
needs the secret) should be proxied through a small StagePilot-operated
server endpoint, similar in spirit to how Cloudflare credentials are
never shipped client-side today. Concretely:
- Desktop app performs steps 1–2 of the flow itself (open browser to
  `/oauth/authorize` with `client_id` + PKCE `code_challenge` — the
  `client_id` alone is not sensitive and can ship in the app).
- Desktop app receives the `code` on its local loopback listener, then
  POSTs `{code, code_verifier, redirect_uri}` to a new StagePilot backend
  endpoint (e.g. `POST /oauth/planning-center/exchange` on the existing
  control-plane service used for Remote Access), which holds the
  `client_secret` server-side and performs the actual `/oauth/token`
  call, returning only the resulting access/refresh tokens to the
  desktop app.
- The same proxy endpoint is reused for token refresh
  (`grant_type=refresh_token`), so the client_secret never needs to
  leave the server at any point in the token lifecycle.

This does mean the feature has a genuine backend/control-plane
dependency, not just desktop + local Planning Center API calls — this is
the single biggest scope driver identified in this doc (see §8).

## 3. Local redirect/callback design

Model directly on `desktop/src-tauri/src/native_credentials.rs`
(`NativeCredentialBroker`): bind `TcpListener::bind("127.0.0.1:0")` for
an OS-assigned ephemeral port, run a lightweight non-blocking accept loop
on a background thread, shut down via an `AtomicBool` + `Drop` exactly
like the existing broker.

OAuth-specific additions on top of that pattern:
- Generate PKCE `code_verifier`/`code_challenge` (S256) and a random
  `state` value per sign-in attempt before opening the browser.
- The loopback server accepts exactly one GET request to `/callback`,
  validates `state` matches what was generated (CSRF protection), reads
  `code` from the query string, and immediately closes/stops listening
  after that first request — single-use, matching the existing broker's
  "authorization"-token single-purpose design.
- Serve a plain static "you can close this tab" HTML response on the
  callback request itself; do not rely on the browser tab for anything
  further.
- Timeout: if no callback arrives within a bounded window (e.g. 5
  minutes), stop the listener and surface "sign-in was not completed" in
  the UI rather than hanging indefinitely; also stop listening
  immediately if the user cancels from the StagePilot UI.
- Open the browser via `tauri_plugin_opener` (already a dependency —
  confirmed at `desktop/src-tauri/src/lib.rs:987`,
  `.plugin(tauri_plugin_opener::init())`), which opens the user's
  default system browser, not an embedded webview. This matches the
  explicit recommendation against embedded webviews for OAuth (PCO's
  consent page, cookies/session, and any 2FA the user's PCO account uses
  are all safer and more standards-compliant in a real browser). I
  confirmed `tauri_plugin_opener` is already wired into the app; I have
  not written or run a spike opening PCO's actual authorize URL through
  it in this scoping task, so full end-to-end confirmation of the
  browser-open path still belongs in the implementation task, not
  claimed as proven here.
- redirect_uri registered with PCO should be the loopback origin +
  `/callback`; if PCO's registration UI cannot accept a wildcard port,
  fall back to a small fixed set of pre-registered ports the app tries
  in sequence (standard pattern used by e.g. `gcloud auth login`,
  GitHub CLI's `gh auth login`).

## 4. Token storage

Reuse the existing `keyring`-backed pattern in
`backend/src/stagepilot/core/settings.py` — same `KEYRING_SERVICE`, a new
`KEYRING_ACCOUNT` value for OAuth tokens (or a small JSON blob under one
account containing access token, refresh token, and expiry) instead of
the current single PAT secret. This is sufficient for the secret
material itself.

OAuth-specific addition: **store the access token's expiry timestamp
alongside the tokens** (not just the opaque tokens), so refresh can be
proactive (see §5) rather than only reactive on a 401. This is new
compared to the current PAT flow, which has no expiry. The non-secret
`PersistentPlanningCenterSettings` model (settings.json) is the right
place for a `connection_method: "oauth" | "manual"` marker and non-secret
metadata (e.g. connected-account display name for UI), keeping the
sensitive token blob itself in the keyring only, matching the existing
split between `PersistentPlanningCenterSettings` and the keyring secret.

## 5. Background refresh design

- Proactive refresh ~10–15 minutes before the 2-hour access-token expiry
  (checked against the stored expiry from §4), run from existing
  backend startup/scheduler machinery rather than a new standalone
  timer if one already exists for comparable periodic tasks.
- Fallback reactive refresh on any 401 from the Planning Center client,
  in case the proactive path was missed (e.g. app was asleep/closed).
- Every successful refresh returns a **new** refresh token per PCO's
  docs — the stored refresh token must be overwritten on every refresh,
  not reused, or subsequent refreshes will fail once the old one is
  invalidated.
- Failure handling (the refresh token itself is expired/revoked, e.g.
  90+ days unused, or the user revoked access in PCO): this must produce
  a specific, actionable UI state in `PlanningCenterSetupPanel.tsx` —
  "Your Planning Center connection has expired, sign in again" with the
  sign-in button immediately actionable, not a generic error banner or a
  silent retry loop. This directly echoes the lesson from
  t_460cf8dd/t_7bdec765 (recent credential-revoked banner-flash and
  stuck-disabled bugs in Remote Access) — OAuth failure states need the
  same care given to those, including avoiding any transient
  false-negative banner flash from a single failed check before a retry.

## 6. Migration path (non-breaking)

- Keep the manual App ID/Secret (PAT) method fully available and
  functional — no forced migration, no deprecation warning on existing
  working connections. It also remains useful for scripting/self-hosted/
  power users, mirroring PCO's own PAT-vs-OAuth distinction.
- Add OAuth as the new **default/recommended** path presented first in
  the UI (§7), but do not touch `PersistentPlanningCenterSettings` or
  delete/rotate any existing keyring secret for users already connected
  via PAT.
- `connection_method` marker (§4) lets the backend and UI both know
  which auth path is active per installation without guessing from the
  shape of the stored secret.

## 7. UI/UX

Stays inside the existing `PlanningCenterSetupPanel.tsx` widget — no new
standalone settings page, per standing rule.

Proposed layout:
- Primary, prominent "Sign in with Planning Center" button at the top of
  the panel when disconnected (or when the OAuth connection has expired
  per §5).
- Once connected via OAuth: show connected-account/org display name (if
  available from PCO's API) and a "Disconnect" action, no App ID/Secret
  fields visible.
- The current App ID/Secret fields move under a collapsed "Advanced /
  manual setup" disclosure, unchanged in behavior, for existing users
  and power users. If already connected via that method, that section
  stays expanded by default showing the current state rather than
  hidden behind the disclosure (don't hide an active configuration from
  its owner).

## 8. Effort estimate

Comparable in complexity to the existing Remote Access credential-broker
+ reactivate/reenroll machinery already in this codebase — i.e.
**non-trivial, multi-file, not a quick afternoon change.** Concretely
touches:
- New Rust: PKCE/state generation, loopback OAuth listener (extending or
  paralleling `native_credentials.rs`), Tauri command(s) to
  start/cancel sign-in and expose the browser-open call.
- New backend: token-exchange/refresh proxy endpoint holding
  `client_secret` server-side (new control-plane surface, not just
  desktop+PCO — this is the part with no existing analog in the
  codebase and the main driver of scope), keyring storage changes for
  OAuth token blob + expiry, proactive refresh scheduling, PCO client
  changes to use Bearer tokens instead of Basic Auth when
  `connection_method == "oauth"`.
- New frontend: sign-in button + connected-state UI in
  `PlanningCenterSetupPanel.tsx`, collapsed manual section, expiry/
  re-approval error states.
- Regression tests needed specifically for: refresh-failure/revoked-token
  UX (no silent failure, no banner flash), state/CSRF validation
  rejecting mismatched callbacks, listener timeout/cleanup on abandoned
  browser tab, and non-breaking behavior for existing PAT-connected
  installations.

Realistic sizing: a genuinely new backend control-plane endpoint plus
desktop-side OAuth plumbing plus UI work with careful failure-state
design is multiple work-days of focused effort, not a single-session
task — closer to the scope of the Remote Access broker + reenroll work
than to a typical bug-fix task in the recent history on this board.

## Approved implementation decisions (supersede the open items above)

Status: **APPROVED FOR IMPLEMENTATION.** The operator has registered the
OAuth app, provisioned `PLANNING_CENTER_CLIENT_ID`/`_CLIENT_SECRET` as
`stagepilot-control-plane` Worker secrets, and registered exactly 4 fixed
loopback redirect URIs (PCO requires an exact port match; no wildcard
support): `http://127.0.0.1:52847/callback` through `...52850/callback`.
The desktop listener tries these 4 ports in order, first free wins.

1. Reuses the existing `stagepilot-beta-control-plane` Worker/Durable
   Object (`control-plane/src/index.ts`), not a new service.
2. New routes `POST /v1/planning-center/oauth/token` and
   `POST /v1/planning-center/oauth/refresh` on the existing `Registry`
   DO `handle()` dispatcher.
3. **Route auth/abuse-control decision**: these routes are reached by
   "any legitimate desktop instance mid-OAuth-flow," which is a broader
   set than "installations that have ever enrolled for Remote Access"
   (Remote Access enrollment is opt-in and most Planning Center users
   will never touch it — see `remote_desktop.py`). Requiring an existing
   `isInstallation` credential would therefore lock out most users. The
   session's Remote Access `isInstallation` bearer pattern is not
   reusable here.

   Instead: a per-flow HMAC "flow ticket", keyed with the existing
   `INSTALLATION_SIGNING_KEY` secret, exactly parallel to how
   `credential()` already derives per-installation HMAC tokens from that
   same key. When the desktop app calls the *Python backend's* new
   `/api/v1/planning-center/oauth/start`, the backend mints
   `flow_ticket = base64url(HMAC-SHA256(INSTALLATION_SIGNING_KEY-equivalent... )`
   — concretely, the backend does NOT hold the Worker's signing key, so
   instead the Worker itself mints the ticket: add a lightweight
   `POST /v1/planning-center/oauth/flow` (no auth required, IP rate
   limited like anonymous `enroll()`) that returns `{flow_id, ticket}`
   where `ticket = HMAC(INSTALLATION_SIGNING_KEY, "pco-oauth-flow:" +
   flow_id)`, bound to the `state` value the backend generates. The
   backend calls this once when starting a sign-in, embeds `flow_id` in
   `state`, and presents `ticket` on the later `/oauth/token` /
   `/oauth/refresh` calls. The Worker verifies `ticket` matches
   `flow_id` before relaying to PCO. This is not proof the caller is a
   *known* installation, but it does mean the caller must have gone
   through the Worker's own IP-rate-limited flow-issuance step first —
   the same anti-abuse posture as anonymous enrollment — so the token
   route can never be hit "cold" as an open OAuth relay. Combine with
   the existing per-source `RateWindow` rate limiting (reuse
   `takeInstallationRate`-style windows keyed by source IP instead of
   installation id) on all 3 new routes.
4. Redirect URI approach: fixed 4-port set (§3), confirmed above, no
   wildcard fallback needed.
