# Changelog

## [Unreleased]

## [2.41.1] - 2026-10-16

First published release of the single-channel, plain-version line (2.41.0 was tagged but its
build failed validation on the Intel Mac runner, so it was never published and its notes are
included here).

### Fixed

- Test-only: the fake Playback used in the test suite allowed 0.25 s between heartbeats, so a
  slow build machine could drop the connection mid-scan. No application behaviour changed.

## [2.41.0] - 2026-10-16 (not published)

StagePilot moves to one release channel and a simpler version number. This release
rolls up every change made since 1.1.103 (the 1.1.104 pre-releases below), so everyone
on 1.1.103 receives all of them in one update.

### Changed

- One channel: there is no longer a Stable/Beta switch. Every install updates from the
  public GitHub releases of `tage-ilot/stagepilot`. The "Release channel" setting is
  removed from Advanced; a saved value from an older version is ignored and dropped the
  next time settings are saved. Beta installs update to this release automatically.
- Versions are plain `X.Y.Z` from here on (this release is 2.41.0).
- Release tooling now suggests the next patch and refuses anything that is not a plain
  `X.Y.Z` newer than the latest tag.
- The in-app update check uses one command (`check_for_update`) in place of the
  per-channel one.

### Includes

All of the 1.1.104 pre-release work, notably: song arrangement (skip a song on either
side of a Playback pairing without reordering anything), OSC lighting with cue board and
timeline, Connection Access, a restartless Remote Access race fix, the animated loading
logo, playback auto-connect always on, and the reworked design system. See the
`1.1.104-beta.41` notes below for the full list.

## [1.1.104-beta.41] - 2026-10-16

### Added

- Playback song arrangement: Playback songs and service-plan songs show as linked pairs, and you can skip a song on
  either side without reordering anything. Skipping plan song 4 and Playback song 5 makes Playback track 4 start plan
  song 5. Skips are saved with Save configuration; a skipped Playback song never touches the timer.
- Lighting can send OSC (UDP) as well as MIDI. OSC is the main method; MIDI stays
  as the alternate. Only the saved method sends cues, and each method keeps its
  own cue list, so switching never deletes anything.
- Lighting cue board: saved reusable cues you press while a song plays to add a
  cue at the current time, with "Preview only" and "Undo last capture". A visual
  timeline shows the cues on the song, with drag-to-move, zoom, and labels that
  stack so close cues stay readable. The whole Song lighting timeline is one
  collapsible section that remembers whether you left it open.
- Connection Access (Host, Local network, Internet) on the StagePilot panel is
  one switch that saves as soon as you choose it. The dashboard PIN is always
  required (default 1234); Remote access and its people appear under Internet.
- The loading logo glides into the dashboard header, and the dashboard fades in
  once it lands.
- Dashboard widgets can be placed anywhere without overlapping; the spacer tool
  is gone. New users and "Reset layout" get the same arrangement: Service plan
  on the left, and Now playing, Manual controls and the event stream stacked
  on the right.
- The Now playing bar is blue at rest, sweeps to the live colour when the
  countdown starts and keeps a lighter band moving down it, and sweeps back to
  blue when it stops.

### Changed

- Services, Presentation and MIDI connect, switch and change settings without
  restarting StagePilot. Switching a running show's source is refused with a
  clear message until the timer is stopped, and settings fixed by an environment
  variable say so. Concurrent changes are applied one at a time, so the last
  save wins.
- The header status pill is the notification: it grows into an info (blue) or
  error (red) message for up to 5 seconds, then shrinks back. The logo no longer
  moves at any window width.
- Status words, pills and the Manual controls buttons use the bold pixel font;
  the countdown and other numbers use Geist Mono. Manual controls always make
  even rows, and Now playing and Manual controls cannot be resized to cut off
  their content.
- Playback: Playback host sits beside Scan Network, and network scanning and
  connecting automatically are always on. The "Simulated MIDI" option is removed.
- StagePilot panel: the status cards are Version, State revision, API endpoint
  and WebSocket, with Advanced (Timezone first) under them.
- The Presentation Look list reads "Current Look" for "leave the Look alone".
- "Not connected" is now "Disconnected" everywhere.
- After a restart StagePilot retries by itself when it cannot reach the backend.
- Developer guides: DESIGN.md and the style guide were rewritten to match the app.

### Notes

- Versions 37 to 40 were not published (their release builds failed on test-only problems, never on the app), so these notes include them.

## [1.1.104-beta.36] - 2026-10-13

### Changed

- Success is a deeper green (`#31551D` fills, brighter green words) and the
  status dots are gone from the connection cards and from "Ready" in the
  header; the status word on each card is now capitalized and larger. In the
  compact layout the word always stays on one line and never overlaps the icon
  or a neighbouring card (it gets smaller on narrow screens instead of
  wrapping).
- Errors and warnings are now one pure red (`#ff0000`), so they cannot be
  confused with the deep-red primary, Lighting or Presentation colours.
- Playback is a clearer blue.
- Inside a connection's configuration panel its own colour is the only accent:
  Save configuration, Scan and Connect buttons, the "Changes saved" bar and
  connected messages all use it. Errors stay red and dialogs keep the app
  colours.
- The Backend card no longer keeps its hover look after it is clicked.
- Previous and Next now say what they did: `"Song" is up next. Press Start
  next to play it.`

### Fixed

- Previous only selects the earlier song and starts nothing, but the service
  plan then marked it "Current · live". "Current · live" now follows the song
  that is really playing and "Up next" follows the song Start next will play,
  in the service plan and in Now playing.
- Reloading the service plan after Previous silently changed which song Start
  next would play; it now keeps the same song.
- Playback: a notice, and no stale Planning Center length update, when
  Playback's song lengths are outdated or already applied.

### Added

- Developers: an optional dev-only visual text and colour editor for the web
  frontend (`docs/visual-editor.md`). It is not part of the packaged app.

### Notes

- Remote Access recovery of an older stuck installation still needs the
  control plane to be deployed; that has not been done.
- Not yet tested against a live Playback by hand: play, pause, seek and resume.

## [1.1.104-beta.35] - 2026-10-13

### Added

- Playback: pausing, seeking and resuming now carry through. A stop (from
  Playback, StagePilot or MIDI) keeps the remaining time and the song position
  instead of blanking the countdown; pressing play again, or seeking to a new
  spot (playing or stopped), continues the countdown from the real Playback
  position. ProPresenter holds on stop and restarts at the remaining seconds;
  Lighting sends nothing while stopped and, after a resume or seek, only the
  cues still ahead (never a burst of earlier ones). A seek to the start followed
  by play still counts as a fresh start. After a reconnect mid-song the timer
  re-aligns only if Playback is on StagePilot's current song.
- The dashboard shows the frozen remaining time while stopped.

### Changed

- Playback reconnects faster: the last working address is re-checked about
  once a second, with the full network scan only occasionally. The status shows
  the true state ("Looking for Playback...", "Connected").
- StagePilot no longer switches to MIDI on its own when Playback drops. MIDI
  runs only when it is the selected connection method.
- Colours: deep red primary (`#c7231d`), per-connection colours (Services green,
  Presentation orange, Playback slate, Lighting pink-red, StagePilot red), a
  teal accent for Up next and information, warning orange `#e34e1f`. The open
  status card now wears its own connection's colour. Inter is replaced by Geist.
- "Lights" is now "Lighting"; "Basic" is now "General configuration".
- Remote Access: an installation stuck on an older setup is now recovered by the
  control plane (this part takes effect when the control plane is deployed), and
  the app explains the situation instead of a generic error.

### Notes

- beta.34 was tagged but never published (a new Playback reconnect test raced on
  the Windows build); beta.35 carries the same changes plus the test fix.
- Not yet tested against a live Playback by hand: play, pause, seek and resume,
  Remote Connections off and on, and a dropped network are the checks to run.
- Timing is accurate to about a second (Playback reports once a second).

## [1.1.104-beta.33] - 2026-10-12

### Changed

- Frontend facelift: one design system (`DESIGN.md`) drives the whole interface.
  Inter is bundled and used everywhere; shared Button, Field, Select, Panel,
  Badge, Disclosure, Save bar and Dialog components replace per-screen styling;
  every panel, the header and status cards, live controls, dialogs and the
  startup screens use them. The loading bar keeps its pixel stripes. Buttons are
  44px on touch devices; the global corner radius is 8px.
- Settings panels use "Configuration" and "Service plan", and "Note" replaces
  "pulse" (for example "Note length (ms)", "Test cue"). Lights shows
  "MIDI Output to Lighting Controller".
- Playback: a Connection method choice (Playback API main, MIDI alternate)
  replaces the alternate-connection row; "Other computer..." replaces
  "Connect a specific computer".
- Every configuration panel has one always-visible Save configuration button.
  Fields that could interrupt a running show (connection method, ports, MIDI
  output, channel and note length, PIN, network access, timezone) are held with a
  "Not saved yet" marker until you save. Send test cue uses the saved output and
  is disabled, with a reason, while a new output is unsaved.
- Clearer offline and API error messages, safer repeated clicks on Save, and
  better handling of long names, long song lists and 320px screens.

### Notes

- Visual and interface release. No backend behavior or API changes.
- beta.32 was tagged but never published (its Windows build failed a line-ending
  check in the build tooling); beta.33 carries the same changes plus that fix.
- Real-device touch, screen-reader and 200% browser zoom checks were emulated
  only; hardware acceptance is not implied.
- Startup and error screen text over the background photo can be low contrast at
  its brightest spot; left as is by owner decision.

## [1.1.104-beta.31] - 2026-10-08

### Fixed

- Playback play, pause and stop update promptly; selecting another song while
  one plays now stops the timer. Saved song order checks setlist id and version.
- Platform-conditional desktop bindings pass strict Linux Rust checks without
  changing native credential or startup behavior.

### Added

- Discover Song Order silently reads each song's length (no audio played),
  reports a Playback/plan song-count mismatch, and shows scan progress.
- Update Planning Center previews changes and requires confirmation before
  writing only song lengths. It checks conflicts and retains undo; nothing
  runs automatically, and a running timer is never changed.

### Release-scoped risk acceptance

- Hunter waived offline updater-key backup/recovery attestation for this private
  beta only and accepts the recovery risk. This does not prove backups exist
  or change future release policy. Signing-secret presence, the configured
  public key and new published updater signatures still require verification.
- Real-rig scan/Update Planning Center preview and macOS Keychain/permission
  persistence across this update remain UNVERIFIED; hardware acceptance is
  not implied by automated tests or signed builds.

## [1.1.104-beta.30] - 2026-10-07

### Fixed

- Scan Network, a typed address and a saved address now say plainly "macOS is blocking
  StagePilot's local-network access" (numbered steps, Open Local Network settings, Copy
  diagnostic details) when every other address fails instantly while loopback works.

### Changed

- macOS builds are signed with a stable StagePilot identity so Local Network and Keychain
  approvals survive updates. After this update macOS asks once more: click Allow (Local
  Network), Always Allow (Keychain), and approve any App Management prompt. See
  docs/macos-adhoc-signing.md. This is not Apple notarization.

## [1.1.104-beta.29] - 2026-10-07

beta.28 was tagged but never published (its Windows build failed one test that
was fixed in #87); this is the next unused number with identical product changes.

### Fixed

- Scan Network no longer reports every failure as "Couldn't find Playback". Scan
  errors are classified (`permission_denied`, `no_route`, `refused`, `timed_out`,
  `not_playback`, `invalid_address`, `unexpected_error`), shown with one concrete
  next step, and logged. Each scan self-tests loopback and the default gateway so
  "this app is blocked from the local network" is told apart from "nothing is
  answering". A typed address is one direct probe with its own specific result
  and never runs a network scan.
- The scan shows its real phase, progress (checked N of M), networks and elapsed
  time, and reports how many addresses were actually probed.
- Fewer macOS password prompts: the Keychain is not read at all when Planning
  Center is not configured, and is read at most once per launch otherwise.

### Added

- Playback scan diagnostics: one INFO log line per scan (trigger, networks,
  interfaces, per-class error counts, outcome, duration), connect/reconnect and
  settings-migration decisions, and the sidecar's executable/signing identifier at
  startup. "Copy diagnostic details" on a failed scan, and the last scan plus
  recent Playback log lines in "Send logs to developer".
- "Open Local Network settings" link when macOS blocks LAN access.

### One-time prompts after this update

macOS may ask for your login password once for each StagePilot Keychain item
(Planning Center) and may re-ask for Local Network access, because each update is
a new ad-hoc-signed program. Click **Always Allow**. If Scan Network says macOS
blocked StagePilot, turn on every StagePilot entry in System Settings > Privacy &
Security > Local Network and reopen StagePilot. These prompts cannot be removed
without a paid Apple Developer ID; this release removes every avoidable Keychain
read. The signing identifier is unchanged, so in-app updates from beta.27 are
unaffected.

### Known issue

- On macOS the packaged backend can be denied Local Network access even though the
  app is enabled under Privacy & Security > Local Network (see
  docs/playback-api.md, "Local Network access on macOS"). StagePilot now says so
  and tells you what to turn on. The code-signing identifier is intentionally not
  changed in this release (see docs/macos-adhoc-signing.md, "Keychain and Local
  Network prompts").

## [1.1.104-beta.27] - 2026-10-06

### Changed

- Playback connection window redesigned: one-click Scan Network, a single unified
  connection status banner, a song-order card, and a Save settings footer with
  a discard confirmation. Defaults for a fresh install now start on the Playback
  connection.
- The collapsed MIDI section is now labelled "Alternate connection: MIDI
  settings" (previously "Advanced"); it still starts collapsed and is not
  persisted open. Docs and tests updated to the new name.

### Fixed

- Settings migration: existing settings files are upgraded once and
  idempotently, keeping all saved MIDI values (fix carried in this release's
  Playback UX work, PR #85).
- beta.24 and beta.25 were tagged but never published, and beta.26 is the
  latest published release; this is the next unused number.

## [1.1.104-beta.26] - 2026-10-05

### Changed

- Playback API is now the default Playback connection (settings schema 2).
  MIDI (native, network, gateway) moved under a collapsed Advanced section and
  is unchanged as the alternative. Unified status/readiness is consistent across
  the config panel, dashboard and checklist; when both are connected the
  Playback API wins and MIDI is used only when the API is not connected.
- Settings migration: existing settings files are upgraded once and
  idempotently; all saved MIDI values are kept and the default source flips to
  Playback API.
- Docs: operator hardware acceptance documentation (#84). Planning Center
  all-service-types diagnostics and regression test aligned with the released
  failure isolation (#81); no production behavior change.

### Fixed

- Playback integration tests were timing-sensitive on slow CI runners (beta.24 and
  beta.25 were tagged but their builds failed validation and were never
  published); widened fake-endpoint timeouts and slowed the fake heartbeat,
  which starved the fake server on the Windows event loop. No production code change.

### Added

- Playback API input is the schema-2 default, with heartbeat-verified local-first
  discovery, manual-address override, reconnect, bounded event monitoring and
  live source switching. Existing native/network MIDI remains the unchanged
  alternative and all saved MIDI settings survive migration.
- Explicit confirmed Discover Song Order walks only Previous/Next while Playback
  is stopped, suppresses Playback-triggered actions, restores the original
  selection, and atomically saves completed order/version/time. Version changes
  and unknown IDs disarm starts until rediscovery; reconnect revalidates safely.
  No Playback play/pause/select/seek or general control routes are exposed.

## [1.1.104-beta.23] - 2026-10-03

### Fixed

- "All service types" could flip the Planning Center connection to ERROR
  even when most configured service types loaded successfully. Root cause:
  one service type returning a plan response shape that failed internal
  validation aborted the entire multi-service-type lookup, discarding every
  other service type's already-fetched valid plan data. Each service type's
  lookup is now isolated — a single bad response is logged and skipped, and
  the nearest valid plan from the remaining healthy service types still
  loads. Only surfaces an error if every configured service type fails.
- Planning Center failure logs now record the exception type, HTTP status
  (when applicable), and the affected service type ID(s) alongside the
  existing sanitized error marker, without logging anything that could
  identify a specific Planning Center account or organization. Makes future
  "Send logs to developer" bundles actually diagnosable instead of a single
  uninformative warning line.

## [1.1.104-beta.22] - 2026-10-03

### Fixed

- "Copy Log" and "Send logs to developer" now cap each log file to the
  newest 100 lines: a backend under load repeating the same handful of
  log lines (e.g. HTTP requests) previously returned thousands of
  near-duplicate lines, unreadable, and could push the diagnostic upload
  past its size limit once JSON-escaped.
- Fixed a reload loop: a transient Planning Center failure (timeout,
  rate limit, momentary API hiccup) right after saving settings — most
  visibly after selecting "All service types" — used to be treated the
  same as genuinely invalid settings, forcing a full app reload that hit
  the same transient error again, looping indefinitely. Transient
  failures are now accepted like any successful save; the live
  connection status still reflects the real error, and the existing
  background refresh retries on its own.

## [1.1.104-beta.21] - 2026-10-02

### Added

- A "Copy Log" button next to "Send all logs to developer" in Settings →
  Backend: copies the current backend log straight to the clipboard, fully
  offline (no network call, no upload rate limit, no Remote Access
  requirement).

### Fixed

- "Send logs to developer" and the equivalent clipboard-copy command were
  always empty for a normally-running (non-crashed) backend — the
  overwhelming common case for both actions. Both now correctly read the
  active backend log.

## [1.1.104-beta.20] - 2026-10-02

### Added

- Fleet admin panel: per-installation list with name, phase, location, and
  last-seen time; operator-settable labels; archive/unarchive (hides an
  installation from the default view without affecting its access, unlike
  revoke); and removal.
- Installations now self-report their OS device hostname on an always-on
  lightweight check-in, independent of whether Remote Access is enabled —
  improves fleet visibility and tunnel-access approval, and auto-fills an
  installation's admin-panel label the first time it's empty. Self-reported
  and display-only; never used for authentication or installation matching.
- Admin panel password-based login (a short memorable password, padded
  client-side to meet the bearer-token length floor) alongside the existing
  long-token login.
- "Send logs to developer" diagnostic bundle upload: a manual action
  surfaced from crash/error dialogs and the backend settings panel.
  Rate-limited per-installation (6 uploads/hour) and client-side
  (1/minute), backed by Cloudflare R2 with an account-wide 8 GiB hard
  storage cap and 30-day automatic expiry — both enforced in Worker code
  rather than relying on an R2 platform quota, which does not exist; R2
  only bills past its free tier.

### Fixed

- Mobile Remote Access loading screen: replaced an oversized fixed-width
  wordmark with a responsive circular spinner matching the desktop loading
  bar's style, removing horizontal overflow/forced pinch-zoom on phone and
  tablet.
- Service Plan hover preview: added an item-title heading and a 0.5s
  hover-intent delay before the preview appears, with no re-delay when
  moving between items.
- Planning Center OAuth: the dashboard no longer shows a stale
  "connection: error" after a successful sign-in — service types and the
  day's plan now load automatically without manual reconnect steps.

## [1.1.104-beta.18] - 2026-09-29

### Added

- Fleet alerting infrastructure, end to end. The backend now has a critical
  alert store that deduplicates repeated alerts and holds a 15-minute cooldown
  per alert key, so a persistent fault notifies once instead of flooding
  (#57), plus a sliding-window rate-anomaly watcher with per-event-type
  thresholds that raises an alert when an event type spikes abnormally (#58).
  Critical alerts are forwarded from the installation to the fleet control
  plane with serialized timestamps (#61).
- Fleet control plane alert handling: an ingestion endpoint that accepts
  installation alerts, plus admin endpoints to list and acknowledge them
  (#60), and admin actions to reset an installation's rate limit and to
  request approval from it (#62).
- A static, StagePilot-styled fleet admin panel served by the control plane
  for reviewing alerts and driving those admin actions (#63).
- A fleet city list derived from edge geolocation at the control-plane
  boundary. Raw client IP addresses are never stored or returned — only the
  coarse city/region derived at the edge (#66). Alerts in both the admin list
  and the admin panel now show which installation location they came from
  (#67).

### Changed

- Service Plan hover-to-preview now shows a title heading on the preview
  card, and the hover-intent delay moved to the widget level at 0.5s, so
  sweeping the pointer across several items no longer flickers a preview open
  for each one (#59).

### Fixed

- Planning Center OAuth: completing the OAuth connect flow no longer leaves
  the dashboard stuck in `connection:error` until the user manually walked
  through the reconnect steps. The connection is now activated as part of the
  OAuth callback, across all Planning Center service types (#56).
- The loading bar's blue edge mark is replaced with a thin line in the
  outline color, matching the rest of the loading treatment instead of
  introducing an off-palette blue (#64).
- The mobile-responsive loading-screen fix now also applies to the web
  dashboard access gate and the PIN gate, which were still rendering the
  oversized desktop loading screen on phones and tablets (#65).

## [1.1.104-beta.17] - 2026-09-29

### Fixed

- Merged all 9 pending PRs (#44-#52) into a single integration branch and
  resolved two cross-PR contract breaks that no individual PR's CI could see:
  the Planning Center client Protocol widened by #49 to include
  `resolve_selected_plan` is now satisfied by every test double (#45, #50);
  the Timezone select re-keyed from offset-minutes to IANA zones by #47 now
  has its Baker Island (`Etc/GMT+12`, UTC-12) coverage from #49 asserting the
  correct IANA-keyed value instead of the superseded offset value.

## [1.1.104-beta.16] - 2026-09-27

### Fixed

- The Planning Center status indicators in the dashboard header and the
  Services widget now correctly show "connected" when signed in with
  Planning Center OAuth, not only when a manual API key is configured.
- Fixed a stale "Planning Center returned an invalid plan response" error
  that could linger in the Recent Event Stream after a newer request had
  already superseded it.
- The Recent Event Stream now keeps informational (non-error) entries
  visible for about 5 minutes before they disappear (previously 2 minutes),
  while any active error continues to stay visible until it's resolved.
- Fixed dashboard widget layouts (custom sizes/positions) not surviving an
  app restart: a delayed startup re-fit pass was overwriting the restored
  layout shortly after launch.
- Fixed Planning Center OAuth sign-in not staying connected across an app
  restart: the app now restores and refreshes the saved OAuth token before
  starting the Planning Center connection, instead of after.

## [1.1.104-beta.15] - 2026-09-26

### Changed

- The Planning Center settings panel now clearly separates settings that work
  with either Planning Center sign-in or a manual connection from settings
  that only apply to a manual API key. The manual-only section is now
  collapsed by default and labeled "Manual API Connection". Several buttons
  ("Load Service Types", "Save Settings", and related actions) previously
  required an Application ID even when signed in with Planning Center; they
  now work correctly for OAuth-connected users with no manual credentials
  entered.
- Service Type now has an "All" option that searches every active service
  type and uses the single nearest upcoming plan across all of them, instead
  of requiring one specific service type to be selected.
- Timezone is now a dropdown with one entry per UTC offset (labeled by its
  most populous city, e.g. "New York (UTC-5)") instead of requiring exact
  IANA timezone syntax. It defaults to your system's timezone and stores the
  real, DST-aware zone underneath the fixed label.

## [1.1.104-beta.14] - 2026-09-25

### Fixed

- Planning Center OAuth `client_id` was resolving empty in release builds due
  to a GitHub Actions environment-secret scoping mismatch (beta.13
  regression) — sign-in should now actually work. Release builds now fail
  loudly instead of silently shipping broken sign-in if this ever regresses
  again.

## [1.1.104-beta.13] - 2026-09-25

### Fixed

- Planning Center "Sign in with Planning Center" now actually works in
  packaged builds. The OAuth `client_id` was never wired into the real
  release pipeline, so every shipped build (including beta.12) failed
  every sign-in attempt with "This build of StagePilot cannot sign in to
  Planning Center." The client_id is now baked into the desktop binary at
  compile time and threaded through to the backend sidecar, sourced from
  the same `PLANNING_CENTER_CLIENT_ID` repo secret the control-plane
  deploy already uses.

## [1.1.104-beta.12] - 2026-09-24

### Added

- Planning Center OAuth 2.0 sign-in: "Sign in with Planning Center" is now
  the default connection method in Planning Center settings, using an
  in-app browser handshake (desktop PKCE loopback listener + control-plane
  token exchange/refresh). The existing manual App ID/Secret method is
  kept as an Advanced/fallback option — nothing is forced to migrate.
  Includes a connected/disconnected state indicator and a "Reconnect to
  Planning Center" prompt if access is denied, cancelled, or later revoked
  from Planning Center's side. (PRs #29, #30, #31)

### Fixed

- Eliminate a flaky race in the pco_oauth end-to-end callback test: replace
  a fixed 50ms pre-connect sleep with a retry-connect loop and widen the
  callback wait timeout, removing a test-only timing race (no production
  behavior change). (PR #32)
- Eliminate a further structural race in the callback wait() polling loop
  that could still time out on macOS CI runners even with a connection
  already queued (PR #33).
- Fix a real production bug: `CallbackServer::wait()` detached its accept
  thread and leaked the bound TCP listener whenever a Planning Center
  sign-in attempt timed out or was abandoned. Since only 4 loopback ports
  are registered with Planning Center, repeated timed-out sign-in attempts
  by a real user could exhaust all four and break sign-in until the app
  was restarted. Now the listener is properly shut down and joined before
  `wait()` returns. This was also the root cause of the macOS CI flake
  (leaked listeners from prior runs contending for the same 4-port pool).
  (PR #34)

## [1.1.104-beta.11] - 2026-09-23

### Fixed

- Restore the desktop window's maximum size cap (1700x2560) so it matches
  the background image dimensions again; verified against the actual
  measured asset and native window-manager resize/maximize clamping.
- Make a fresh install's dashboard layout match what Reset Layout produces,
  instead of using a different static sizing pass on first load.
- Auto-fit dashboard widget content height on mobile / Remote Access (the
  same React app on narrow viewports), replacing the old static row clamp.
- Allow enabling Remote Access with zero Operators configured.

### Added

- Recent Event Stream: add a hide/show toggle scoped to edit-layout mode
  (persisted, defaults to visible on Reset Layout) and 120-second
  auto-expiry for non-error events.

## [1.1.104-beta.4] - 2026-09-19

### Fixed

- Fix the Regenerate Remote link button so it correctly regenerates the
  Remote link instead of breaking Remote access.
- Add a loading state to the Regenerate Remote link button while the
  request is in flight.
- Add a regression test covering dashboard widget layout persistence.

## [1.1.104-beta.3] - 2026-09-18

### Changed

- Move the Release Channel row and add a Remote Access checkbox with an
  animated in-widget expand in Backend Settings.
- Set the dashboard widget layout default.
- Rename "Open Remote" to "Regenerate Remote link".

### Fixed

- Fix narrow-window layout by setting `tauri.conf.json` minWidth=640 /
  minHeight=480 and removing the max window-size constraints.

## [1.1.104-beta.2] - 2026-09-18

### Fixed

- Add a release-channel (STABLE / BETA) toggle to the frontend updater path.
- Use a 16-hex-character `randomHex(8)` label in the control plane instead of
  a shorter/weaker identifier.
- Retain the enrollment nonce as the durable installation identity across
  `finish_revoke()` so re-enabling after a disable does not mint a new
  identity.
- Move the Remote Access settings entry point inside the Backend Settings
  widget.

## [Unreleased]

### Added

- Register a searchable native macOS Help Book generated from StagePilot's
  Markdown documentation so the system Help menu can return useful local
  setup, operation, and troubleshooting topics.

## [1.1.41] - 2026-07-29

### Fixed

- Add a browser-only PIN gate before the dashboard loading screen, with
  password protection enabled by default and configurable from StagePilot
  Backend without changing the desktop startup experience.
- Report ProPresenter as disconnected, rather than errored, while the
  application is closed or unreachable, while retaining error status for
  confirmed API and configuration failures.
- Continuously monitor ProPresenter, Playback MIDI input, and Lights MIDI
  output availability so status cards reflect closed applications, detached
  devices, and subsequent reconnections without manual refreshes.
- Keep only the newest unresolved error pinned in Recent Event Stream and
  automatically remove its pin when the affected integration recovers.
- Keep the Lights configuration header synchronized with the Lights dashboard
  status instead of showing a separate loading state.
- Move readiness details into a translucent header-status hover/focus panel,
  show disconnected checks neutrally, and keep unconfigured Lights optional
  until a MIDI output has been selected.
- Double the Service Plan widget's default desktop and tablet height so more of
  the service order is visible without scrolling.
- Keep readiness and Planning Center PAT help popovers open while their trigger
  or panel is hovered, open them immediately on click, and use a short,
  discoverable hover delay.

### Changed

- Replace the standalone readiness widget with connection details available
  from the header system status while keeping optional Lights configuration
  from blocking overall readiness.
- Enlarge connection-icon backgrounds without enlarging their icons.
- Place both the Edit Layout button and its editing toolbar below the dashboard.
- Add concise Personal Access Token setup guidance and a direct Planning Center
  help link beside Application ID.

## [1.1.35] - 2026-07-29

### Fixed

- Fixed the packaged backend startup failure on Intel macOS by explicitly
  disabling Hardened Runtime library validation for the ad-hoc release flavor.
- Added final `.app`, DMG, and updater-archive backend launch verification for
  both Intel and Apple Silicon release jobs.
- Preserved specific packaged-backend exit diagnostics so a later generic
  startup timeout cannot hide a macOS signing rejection.
- Replaced the startup bar's fixed 58% backend target with smooth,
  milestone-bounded progress that continues through long healthy starts,
  freezes on confirmed failure, and resumes without moving backward on retry.
- Prevented demo plans and simulated MIDI or timer behavior from reporting
  Planning Center, MIDI, or ProPresenter as connected, and made production
  readiness distinguish real plans, real connections, and timer discovery.

### Changed

- Made the macOS 12.0 deployment target explicit throughout release builds and
  added final Mach-O deployment-target checks.
- Added bounded backend-log rotation and actionable startup recovery controls.

## [1.1.32] - 2026-07-29

- Added production-desktop-only GitHub Release update checks.
- Added explicit update confirmation, signed download/install progress,
  automatic relaunch, and post-update status beside the existing logo.
- Added window size, position, maximized, and fullscreen restoration without
  restoring a minimized window.
- Added coordinated updater archives, signatures, and `latest.json` automation
  for Windows x64, Intel macOS, and Apple Silicon macOS.
- Replaced fixed dashboard snap slots with a responsive GridStack layout that
  supports explicit edit mode, two-dimensional dragging, constrained resizing,
  automatic compaction, intentional spacers, keyboard ordering, responsive
  desktop/tablet/mobile layouts, and v1 layout migration.

All notable changes to StagePilot will be documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
intends to use [Semantic Versioning](https://semver.org/) once releases begin.

## [1.0.0] - 2026-07-19

### Added

- Production-ready Windows and macOS desktop packaging.
- Persistent Planning Center, MIDI, ProPresenter, lighting, and backend configuration.
- Secure Planning Center credential storage and automatic service-plan loading.
- Playback MIDI automation, synchronized ProPresenter countdowns, and lighting cue timelines.
- Persistent drag-and-snap dashboard layouts with accessible movement controls.

### Reliability

- Added cached service-plan fallback, connection recovery, readiness checks, structured diagnostics,
  and verified ProPresenter timer configuration across repeated cues.

## [0.9.14] - 2026-07-19

- Fixed ProPresenter timer-type conversion by atomically configuring and resetting timers.
- Added pointer-based dashboard widget dragging and made Manual Controls movable.
- Enlarged the dashboard logo without changing header height and refined loading-screen spacing.

## [0.9.13] - 2026-07-19

- Added movable, snapping dashboard widgets with persistent layouts and accessible controls.
- Improved ProPresenter Look confirmation, settings messaging, and timer behavior.
- Disabled the first-launch setup overlay while preserving its implementation.
- Refined the application background placement and loading-screen branding.

## [0.9.12] - 2026-07-19

### Fixed

- Configure ProPresenter timer type and duration before issuing Reset, preventing
  the previous song duration from appearing before the next countdown starts.
- Explicitly reset the ProPresenter timer after Reset Position changes its
  duration to `0:00`.

## [0.9.11] - 2026-07-19

### Fixed

- Configure and reset ProPresenter timers atomically before duration verification,
  allowing repeated song cues to start reliably.
- Allow Countdown to Time and Elapsed Time timers to be selected and convert
  them to Countdown Timers when a song-start cue arrives.
- Preserve packaged backend diagnostics in the macOS application log directory.

## [0.9.10] - 2026-07-19

### Fixed

- Retry loading general settings only after the packaged backend is reachable,
  preventing a stale macOS WebKit `Load failed.` message.
- Verify ProPresenter duration changes through the documented single-timer API
  and allow slower Intel Mac updates without starting an unconfirmed timer.

## [0.9.9] - 2026-07-19

### Fixed

- Preserve completed first-launch onboarding when a stale settings snapshot is
  saved by another configuration panel.
- Verify ProPresenter countdown durations before resetting and starting timers,
  including delayed updates observed on Intel macOS.
- Terminate nested PyInstaller backend processes when StagePilot exits or
  restarts on macOS.
- Explicitly create, show, focus, and restore the StagePilot window when the app
  launches or is reopened from the macOS Dock.

### Changed

- Added macOS desktop lifecycle compilation and regression tests to CI.

## [0.9.8] - 2026-07-18

### Added

- Select and apply a saved ProPresenter Look when ProPresenter settings are saved.
- Standalone MultiTracks MCP cue utility with dry-run planning, guarded writes,
  verification, secure authentication, and reporting.

### Fixed

- Restart the managed desktop backend automatically after MIDI or Planning
  Center settings enable a previously inactive integration.
- Fully quit the managed backend with the desktop application and pass the
  saved-settings path explicitly to packaged backend processes on macOS.

## [0.9.7] - 2026-07-15

### Added

- Initial monorepo foundation for the FastAPI backend, React dashboard, and
  Tauri desktop shell.
- Typed asynchronous event bus, observable application state, isolated plugin
  manager, structured logging, and graceful application lifecycle.
- Demo service workflow with safe song navigation, timer simulation, REST
  actions, health/state APIs, and full-state WebSocket updates.
- Dark, responsive live-production dashboard with connection status, ordered
  songs, current timer state, readiness checks, manual controls, and events.
- Core, plugin, API, and WebSocket tests plus backend and frontend quality
  tooling.
- Shared architecture, contribution, configuration, security, plugin, and
  milestone documentation.
- Safe example environment configuration and repository ignore rules.
- Secret-aware Planning Center PAT configuration with validated credentials,
  IANA time zone, request timeout, and identifying user-agent settings.
- Typed asynchronous Planning Center service-type client with Basic Auth,
  version pinning, safe pagination, timeout handling, sanitized errors, and
  mocked contract tests.
- Today-first Planning Center plan discovery with timezone-aware service-time
  matching and a configurable nearest-upcoming fallback window that defaults to
  30 days. Discovery excludes past and no-service-time plans and preserves
  explicit ambiguity when multiple plans share the selected date.
- Ordered linked and generic Planning Center song parsing with scheduled
  durations, source song IDs, and visible skipped-item reasons.
- Production Planning Center plugin startup and reload orchestration with
  current-or-upcoming last-known-good plans, connection and discovery state,
  explicit dashboard plan selection, skipped-item visibility, live health,
  single-flight refreshes, date-rollover cleanup, and demo-mode isolation.
- First backend MIDI Playback slice with disabled-by-default Mido/RtMidi input
  discovery, environment-configured port, channel, and six cue mappings, bounded
  ordered dispatch, duplicate protection, reconnect handling, safe port metadata,
  session-only API and dashboard input selection, disconnect and refresh
  controls, and manual cue simulation through the hardware action path.
- Bounded live MIDI note monitor with port, channel, note, velocity, and
  accepted-or-ignored diagnostics in the production dashboard.
- Vitest and React Testing Library coverage for dashboard plan ambiguity,
  pending selection, revision-safe live state, stale readiness, and skipped-item
  warnings.
- Persistent v0.5 setup with validated atomic settings, Windows Credential
  Manager PAT storage, independent backend integration modes, service-type
  onboarding, preference-aware plan selection, and a non-secret
  last-known-good service cache.
- Six-step first-launch checklist plus editable general, advanced MIDI, and
  ProPresenter configuration. Production panels activate real integrations when
  saved and no longer expose demo or simulated choices.
- Auto-closing first-launch completion feedback with manual dismissal, plus a
  full MIDI 0–127 note-name dropdown using Playback's octave convention.
- Live MIDI cue-filter reconfiguration so saved note, channel, velocity, and
  debounce changes affect the running Playback input without a restart.
- A persistent **Lights** connection and lighting configuration panel with
  macOS MIDI-output discovery, Lightkey-compatible Note On/Off test pulses, and
  per-song elapsed-time cue maps keyed by stable Planning Center song IDs.
- A monotonic backend lighting scheduler that starts from the confirmed shared
  countdown event, cancels safely on stop/restart, and prevents old timelines
  from firing after a song change.
- Live remaining and elapsed song clocks in the dashboard, derived from the
  same timer start and scheduled duration used by ProPresenter.
- ProPresenter-aligned countdown rounding and start timestamps, plus a Reset
  Position sequence that stops the configured timer and resets its duration to
  zero in both StagePilot and ProPresenter.
- A reserved header notification queue with content-width confirmation and
  error highlights. Concurrent action and service-state messages display in
  order for up to six seconds without shifting dashboard controls.
- Planning Center non-song items interleaved into the service-plan display by
  their original sequence, with compact header separators and subdued ordinary
  items that show Planning Center descriptions and scheduled durations while
  remaining excluded from song controls.
- A bundled 1700-by-2560 film-flare application background that scrolls from
  the top without scaling, plus matching desktop maximum window dimensions.
- A simplified StagePilot header wordmark using a locally bundled Instrument
  Serif font across browser and desktop builds.
- Cohesive, higher-contrast setup panels with a shared Production setup header,
  brighter supporting text, compact connection-status badge, and restrained
  per-integration button accents.
- A darker translucent Now Playing card, a larger outlined StagePilot wordmark,
  and right-aligned header notifications beside the system status.
- Dark translucent connection-card hover states with brighter text, semantic
  manual-control button accents, and green/red event-stream severity styling.
- Outline-first manual controls that reveal their semantic colors on hover and
  press, including a green Restart Current action and dark-green Now Playing label.
- A locally bundled StagePilot header font and a two-message notification queue
  that discards older messages when newer notifications arrive.
- Dark-orange Now Playing labeling and deeper READY-green highlighting for the
  current song row, subtitle, and numbered icon.
- A reproducible PyInstaller backend sidecar, Tauri startup/readiness/port-conflict
  supervisor, owned-process-tree shutdown, NSIS release configuration, desktop
  connection-status bridge, and CI-built Windows installer artifact.

[Unreleased]: https://github.com/huntrw6/stage-pilot/compare/v0.9.9...HEAD
[0.9.9]: https://github.com/huntrw6/stage-pilot/releases/tag/v0.9.9
[0.9.8]: https://github.com/huntrw6/stage-pilot/releases/tag/v0.9.8
[0.9.7]: https://github.com/huntrw6/stage-pilot/releases/tag/v0.9.7
