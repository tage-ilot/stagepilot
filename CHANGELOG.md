# Changelog

## [Unreleased]

### Changed

- **A preset you switch on just before a song no longer keeps the song's first look away from those lights.** StagePilot gave a light to the operator for 30 s whenever it changed in a way the replay had not caused, also when no song was playing. On the 2026-11-08 rig test `clear_all` pressed 16 s before play left 19 lights out of the first look of Ever Be: they did not follow the plan for the first 12 s and the moving heads not for 35 s. Now what is touched while no song is replaying (before the first look, after a song ended) is not a take-over: when a song starts from the top, or play is pressed on a song that is not on (a resume or seek), every older take-over is dropped and the first look goes to every light it covers. A touch during the song, also while it is paused, works as before (the light is yours for 30 s, then handed back; a touch of the last 30 s stays yours after a jump). Nothing is released at a song's end or on stop. A Lightkey preset on a held light still shows nothing until **Release lights**. This is a behaviour decision ("option A", the first look holds the whole rig). See \"The operator always wins, per fixture\" in `docs/lights.md`.

### Fixed

- Jumping into the middle of a newly selected song and pressing play now switches StagePilot to that song. In Playback, selecting a song that is not StagePilot's current song, jumping into it and pressing play sent `song.selected`, `seek`, `song.resumed` (rig test of 2026-11-08: song 30049080 at 232.8 s); a play only counted as a song start below 2 s, and a resume of a song other than the current one was dropped, so StagePilot stayed on the previous song, its timer stood still, the lights did not replay the new song and its position was not followed. That resume is now handled as the song being started at that position (current song, countdown, ProPresenter timer and lights from the jump position, not from 0). A deliberate difference from the Python app. Resumes inside the current song, auto-advance, navigation without play, pause, stop and section loops are unchanged. See "How a song is recognised" in `docs/playback-api.md`.
- **The moving heads no longer freeze for 30 seconds after a song starts.** On the 2026-11-08 rig test (nobody touched anything) some lights were counted as taken by the operator at the start of a replay and then ignored for 30 s: Elohim from the start lost MH1, MH3 and MH4 until +29.6 s, Ever Be from 175 s lost Z2, and Ever Be from the top lost the heads for about 30 s after the first look (MH1-MH4 matched the plan in only 3 to 11 of 24 samples between 8 and 20 s). Three causes, three rules (see \"The operator always wins, per fixture\" in `docs/lights.md`): the relay reports the *old* position once more right after the first send (the first value a property is sent has no earlier sent value, so the relay's last value is now its baseline and a change equal to it within 500 ms is the echo); a head that has to travel more than 5 % of its range takes up to a second and settles with a small overshoot (for 1.5 s after such a send a value in the stretch old to new, widened by the overshoot, is the echo); and an 8-bit pan moves in steps of 2.47 degrees, wider than the 1 degree allowed (angles now match within 0.5 % of the range). A real hand-set move still takes the light on the first change (tested with Z1 pan -20 then +20) and gives it back after 30 quiet seconds. The offline check `lightkey_yield_check::run_start` replays the helper's log around a song start against the engine; the tests replay the three real logs and show that the rc.10 rules yield the lights and the new rules yield none.
- **On a very busy song the moving and chasing lights no longer stop; the show is simplified evenly instead.** StagePilot sends at most 200 packets a second to Lightkey. On the busiest saved song (7 loops, 36 chasing lights, 16 colour fades every 100 ms) the colour fades took all of it and the loops got nothing: they changed about 4 times in 30 s where the recording changed them 500 to 2,300 times. When a song asks for much more than that for more than 2 seconds, the loops now get at least 35% of the packets and the colour fades at most 65%, and every light is served in turn (on that song: every loop light 3.4 to 4 times a second, every colour fade about 4, where the loops had none). The Lighting panel says `This song is very busy: some lights update less often than recorded.` while that happens (new `replay_load` with `busy` and `served_percent` in the lights status). A song that fits (both Sunday songs) plays exactly as before. The 200 packets a second limit is **not** raised: whether Lightkey takes more is not known and needs the rig. See "A very busy song" in `docs/lights.md`.
### Added

- **StagePilot now says on screen when a song's lighting data was set aside.** If a song's lighting recording is damaged when StagePilot starts, only that song is taken out and the rest loads (rc.10), but nothing on screen said so. A calm banner now reads `The lighting data for <song> was damaged and set aside; record it again.` on the main screen and at the top of the Lighting screen, with the song name and a **Got it** button that closes it for the rest of the session (it comes back with the next start, and a different song is a new notice). If the whole settings file could not be used, a second banner says StagePilot started with its default settings (a separate one when the file is from a newer StagePilot). Nothing is shown when there is nothing to say. See "A damaged song never costs you the settings" in `docs/lights.md`.
- **The Lighting card says it is saving a long recording.** A busy 20-minute song takes 35-45 s to fit after it ends and the card showed nothing, so it looked as if the recording was lost. From the moment the recording ends until the looks are fitted and saved the card now reads `Saving the recording of “<song>”…` with a pulsing dot, then the usual `Saved N looks...` message (or `Not saved...` / `Could not save...` as before). New `saving` field on the Lightkey status (`{song, started_at}`, left out when nothing is being saved; a Rust-only extension in the contract).

### Fixed

- **Planning Center no longer stays in error after the keychain prompt is answered.** When the prompt was answered after the first second of the app's life, Planning Center connected and loaded the service plan, but the plugin kept the error from its failed start: `/api/v1/health` stayed `degraded` and `plugins.planning_center.status` stayed `error` (and the plugin's reload and plan-choice requests stayed unsubscribed). When the keychain recovers, the plugin is now started again through the normal plugin start (only that plugin, only if it is still marked failed), so its status goes back to `running`. Seen on the iMac after the rc.10 install.

## [2.43.0-rc.10] - 2026-11-08

### Added

- **Every captured song now saves the starting look of all the lights, house lights included.** A recording used to keep only the lights that changed during the song, so the house and front lights you set with a Lightkey preset before a song were not in it (the saved songs Elohim and Ever Be touched 35 and 32 of the 66 lights) and the replay did not recreate the whole picture. A new recording saves what every light looked like when it began (a light that stays off keeps just its dimmer or colour), and the replay sets all of them at the start of the song, so "Holding N fixtures" is the whole rig and presets cannot change the picture. For the real 66-light rig that is 154 values and the first look goes out in 0.77 s. **At the end of the song nothing changes**: the lights stay exactly as they are and you carry on in Lightkey. Because the whole rig is held, a Lightkey preset switched on a held light has no visible effect until you press **Release lights**; a manual change to the light in Lightkey is still noticed and wins. Songs recorded before this are not touched and still replay as before; the Lighting screen says `Recorded before the starting lights were saved. Record this song again to include the house lights.` (arm Record and play the song once). The capture line reads `Starting look of 66 lights saved.`; if the starting look cannot be read the song is saved anyway and says so. New optional `capture.start_lights` in the saved settings (an older build ignores it). See "The starting look" in `docs/lights.md`.

### Fixed

- **A fast move of the lights' own replay is no longer taken for the operator.** On the real rig a replay of "Elohim" swept the four moving heads (pan 112 to -23 degrees in 0.15 s); the DMX showed positions between the replay's sends, StagePilot read them as someone moving the lights, dropped the four heads (\"4 yielded to the operator\") and, because the take-back needs 30 quiet seconds, left them frozen across a jump of the song. A value that lies between the previous value the replay sent and the one it sent last (within about 200 ms of that send) now counts as the replay's own. A real fader or override move to any other value still makes the operator win. See \"The operator always wins, per fixture\" in `docs/lights.md`. `cargo run --release -p stagepilot-lights --example false_yield_report -- settings.json ring.json` reproduces it offline from a recorded DMX log.
- **Pause, jump, play now follows the jump.** When you paused Playback, jumped to another place in the song and pressed play straight away, StagePilot started the countdown from the place where you paused, not the place you jumped to. The countdown, the ProPresenter timer and the lights then ran minutes off for the rest of the song (found on the rig: 119 s ahead of the music). The resume now uses the jump position, however fast play follows the jump. As a safety net, if Playback keeps reporting a position more than 3 s from where StagePilot's timer is, for two reports in a row while the song is playing, StagePilot moves the timer to Playback's position and logs `playback_position_resync` with both positions. Normal drift under a second never triggers it, and neither does a section loop or section jump in Playback: those stay monitor-only and never move the timer (the net is switched off for that song until the next start, resume, seek or song change).
- **A damaged lighting recording can no longer reset all of StagePilot's settings.** After the 2026-11-08 service the iMac saved one loop (song "Ever Be") whose phase offset (8.025 s) was longer than the loop (8.003 s). The settings check refused the whole file at the next start and every setting (Planning Center, MIDI, ProPresenter, Playback, Lighting, PIN, LAN access, every song's recording) would have gone back to the defaults. Fixed in three places: the recorder now stores every offset inside its loop (the same phase: 8.025 s becomes 0.022 s); no song is saved unless it passes the same check as the settings file (a harmless out-of-range offset is repaired and logged, anything else is refused with a plain message and the raw recording is kept); and when the settings file is read, a problem inside one song's lighting data is repaired or, if it cannot be, only that song is set aside (`settings.json.song-<key>-<time>.json`, newest 5 kept) while everything else loads. The notice reads `The lighting data for <song> was damaged and set aside; record it again.` (`settings_notice` in `GET /api/v1/settings`). Problems elsewhere in the file behave as before. See "A damaged song never costs you the settings" in `docs/lights.md`. An rc.9 file already damaged this way loads as soon as it is opened by this build.
- **A recorded song is never thrown away for size.** On 2026-11-08 a 336 s, 66-fixture song needed about 11,400 rows and was discarded ("the song has too many rows") with its recording. The caps are now 50,000 rows, 600,000 messages, 1,000 effects and 250,000 loop steps (were 1,000 / 15,000 / 200 / 20,000), and a song that still does not fit goes down a ladder that always ends with a saved capture: exact fit, looser tolerance (x2 to x32, smallest kept), thinning of steps closer than 0.1 to 8 s, and a trim of the least important changes. The status says what happened (`The song was very busy, so it was simplified: kept N of M changes.`) and the capture records it (`capture.simplified`, optional; an older build ignores it). Loop detection no longer disappears at a looser tolerance (it made the song bigger: 2,916 rows at x4, 13,483 at x8).
- **A very busy song no longer freezes the Lighting screens.** A song at the new limits (up to 50,000 rows) used to make the timeline and the cue list crawl: with 5,000 rows opening the card took 18 s and every tick of the follow-along 10 s. The timeline now draws at most one bar per pixel (it says `Drawing N of M cues` when it does; nothing is removed or changed), the cue list shows one page of 40, and the work that walks every row is done once instead of ten times a second. Opening a 50,000-row song took 0.5 s and a follow-along tick 34 ms in the same test.
- **The capture line says when a song was simplified** (`Simplified: 6,200 of 11,400 changes kept.`) and counts of 1,000 or more have thousands separators.
- **The raw recording of every finished song is kept** in `lightkey-recordings/` next to `settings.json` (newest 5, at most 300 MB), so a song that fails can be fitted again offline with `cargo run --example refit_recording`. A real storage failure now says where it is.
- **Recording a very long or very busy song cannot grow without bound**: past 1,000,000 held changes the recorder thins what it holds instead of stopping.
- Saving and loading a song at the new caps: the settings file is written streamed and one line per timeline row (85 MB, was 216 MB for the same song), loading no longer copies the whole file three times (5.3 s to 2.4 s for a song at the caps), and a replay effect no longer scans its whole cycle on every tick or copies its cycle for every member.

### Changed

- The cue-map `PUT` accepts bodies up to 120 MB (was 8 MB) so a song at the caps can be saved from the page.
- Dev and test builds optimise the `stagepilot-lights` crate (`opt-level = 2`); the song fit takes minutes unoptimised.

## [2.43.0-rc.9] - 2026-11-08

Release candidate 9 (pre-release; the in-app updater does not offer it). Everything in rc.8 applies. Adds the automatic search for the Lighting computer (Lightkey only).

### Added

- **StagePilot finds the Lighting computer by itself.** While nothing is connected (and only then: never while the link is up, never with Lighting off or set to MIDI), StagePilot looks for the Lightkey helper on this computer and on the private networks it can reach, then connects. The first look is about 5 s after start or after a setting change, then every 60 s, and at once on **Search again**. It asks each address one question (`GET /health`, 600 ms, at most 64 at a time, nothing else sent, private addresses only: `127.0.0.1`, the networks this computer is attached to, and the /24 around the saved Lighting address, the saved Replay host and the last found computer, at most 1,100 addresses). It works with helpers already installed (0.4.x and 0.5.0). With an Automatic address (none typed) exactly one found computer is connected to and remembered (`Found a Lighting computer at <ip> and connected.`); several are listed to choose from; with a typed address that is down, a found computer is only offered (`Use <ip>` / `Keep waiting`), never switched to. A found computer fills the Replay host only when it is blank, otherwise `Capture is from <ip>; Replay sends to <host>.`. See "Finding the Lighting computer" in `docs/lights.md`. New: `lightkey.found_url` (optional, empty by default), `discovery` on `GET /api/v1/lights/lightkey`, and `POST /api/v1/lights/lightkey/discovery/{scan,use,dismiss}`.

### Downgrade note

- `lightkey.found_url` is a new optional key in the saved Lighting settings. An rc.7 or rc.8 build ignores it (its Lightkey settings do not reject unknown keys) and drops it the next time it saves settings; Automatic then goes back to the Replay host on port 8770, so after a downgrade type the Lighting address once.

## [2.43.0-rc.8] - 2026-11-07

Release candidate 8 (pre-release; the in-app updater does not offer it). Everything in rc.7 applies. Found on the real rig by setting up the Lighting Mac from scratch with the Lighting section's command.

### Changed

- **The Lighting card says Connected when capture and replay are both live.** It used to say Ready until the first cue was sent, even with everything up. It now reads **Connected** while the Lightkey helper is connected (capture) and the OSC address is open (replay), and **Ready** only while the replay address is available but the helper is not connected yet. Sending a cue no longer changes the word. It goes back to Ready if the helper drops, and to Connected when it returns.
- **The setup command is labelled.** The Lighting section's install box now introduces the command with "Capture Script for Lighting Computer (beta, only for Lightkey):", so it is clear the capture helper works with Lightkey only.

### Fixed

- **A Lighting Mac set up without cue names now shows as Connected.** StagePilot checked the helper through its cue-name feed (`/log`), which only exists when the optional cue module is installed. A helper installed the way the Lighting section installs it has no such feed, so StagePilot reported "Cannot reach the Lightkey API" even though the helper was running, and the setup box never finished. When the feed is missing, StagePilot now reads the helper's `/health` instead: Connected when the helper is up, "Lightkey is not running" when Lightkey is closed. Cue names still appear when the cue module is installed.

## [2.43.0-rc.7] - 2026-11-07

Release candidate 7 (pre-release; the in-app updater does not offer it). Everything in rc.6 applies. Found on the real rig: a brand-new Lighting Mac set up from the Lighting section did not connect when it reaches StagePilot through a different network than the one StagePilot talks to it on.

### Fixed

- **The Lighting setup command now opens the right network interface.** The Lighting Mac used to allow only the interface that routes back to StagePilot, so a Mac with a wired lighting network plus a second network refused StagePilot ("Lightkey API refused this address"). The command now also passes the saved Lighting address when it is a local IP, and the helper opens the interface that has it. Bundles helper 0.5.0 with `install.sh --allow-address`.
- **A changed show says what changed.** When the fixtures change during a recording but the project name and fixture count stay the same, the notice reads "the fixtures in the Lightkey project changed during the recording (Show, 66 fixtures)" instead of repeating the same words on both sides of an arrow.
- **Removing the helper forgets its folder answers.** `uninstall.sh --purge` resets the Documents and Downloads answers while the helper app still exists, so macOS asks again after a reinstall. The setup note also says the Lighting Mac must be unlocked to see the questions.

## [2.43.0-rc.6] - 2026-11-07

Release candidate 6 (pre-release; the in-app updater does not offer it). Everything in rc.5 applies. This candidate sets up and follows the Lighting computer: the Lighting section can install the Lightkey helper for you, shows which Lightkey project is open, warns when a song was captured under another project, and can re-read the fixtures. It bundles helper 0.5.0, which notices a changed project by itself and starts through a small signed "Lightkey API" app so macOS can ask once for local-network, Documents and Downloads access (a plain background python3 is refused without any question). A Lightkey re-save that only reorders a fixture's channels is no longer mistaken for a changed show. Update the helper on the Lighting Mac to 0.5.0 (the new box in the Lighting section does it); a 0.4.x helper keeps working but cannot re-read fixtures. It also replaces the manual Location setting with a weekly automatic city lookup.

### Added

- **The Lighting section shows the Lightkey project.** `Project: Show.lightkeyproj · 66 fixtures` sits under the connection pill (`Project not reported yet` until the helper says). StagePilot also reads the helper's `GET /health` on connect and about every 30 s for its version, fixture map and project-watch state; a helper that reports no version is older than 0.5.0 and gets a `Helper update available` line (capture and replay keep working as before).
- **Read fixtures** button in the Lightkey strip (`POST /api/v1/lights/lightkey/read-fixtures`, which calls the helper's `POST /fixtures/refresh`): re-reads the open project when fixtures were added to it, and shows `Read 66 fixtures from Show.lightkeyproj`, `No change: 66 fixtures` or the helper's plain-language reason. Disabled when not connected, and for a helper older than 0.5.0 (`Update the helper to use this.`).
- **A capture remembers its project** (`capture.project`, optional). When a song was captured under another project than the one open now, the song's lighting timeline and the strip say `Captured with X. Lightkey has Y open.` It is a notice only: replay is never held, skipped or stopped. Older captures have no project and never warn.
- **A project change during a recording discards it** (`Not saved: the Lightkey project or its fixtures changed during the recording (X -> Y). It captures again next time.`) instead of saving a mix of two fixture maps. A hello sent again with the same map id is not a change.
- **Set up the Lighting computer from the Lighting section.** While no Lighting computer is connected, the Lightkey strip shows a box with one copy-ready command (`curl -fsS http://<this computer>:<port>/lightkey-helper/<token>/install.sh | sh`). Pasted into Terminal on the Lighting Mac it installs the Lightkey helper from StagePilot itself (no internet, GitHub or SSH), checks every file against checksums, and the strip turns to Connected by itself. A connected helper older than the bundled one gets the same box as **Update the helper**. The command is LAN only, expires after 30 minutes, serves at most 30 requests per token and is never reachable through Remote access. The helper files (v0.4.3 for now) are compiled into the app (`scripts/vendor-lightkey-helper.mjs <tag>` refreshes them; re-vendor with the tested release before shipping). New routes: `POST`/`DELETE /api/v1/lights/lightkey/helper-install`.

### Changed

- **Location is automatic.** The Location setting and "Detect my location" are gone. Once a week the app asks db-ip.com for the city of its internet address (only the city is kept and reported; the address itself is never stored or sent). It runs in the background, never delays a check-in, and a failure keeps the old value. Set `STAGEPILOT_DISABLE_IP_LOOKUP=1` to turn it off. The control-plane admin override still wins. Details in `docs/configuration.md` and `docs/remote-control-plane.md`.

### Compatibility

- Downgrading to rc.5 or earlier: a capture saved by this build carries an extra `project` key. No setting on that path rejects unknown keys, so the older build loads it and ignores it, and drops it the next time it saves that song. Nothing else changes in the saved data.

## [2.43.0-rc.5] - 2026-11-06

Release candidate 5 (pre-release; the in-app updater does not offer it). Everything in rc.4 applies. This candidate fixes one problem found while testing rc.4 on the rig: saving a setting from a StagePilot window that was already open could put an old copy of the lighting data back and erase Lightkey captures. Refresh or restart an open StagePilot window before saving settings if it is still running rc.4 or older.

### Fixed

- Saving a setting from a page that was already open no longer erases Lightkey captures or other saved lighting data.

## [2.43.0-rc.4] - 2026-11-06

Release candidate 4 (pre-release; the in-app updater does not offer it). Fixes from the rc.3 rig test: Lightkey captures are always saved, are "fully captured" when the total recorded time is within 10 s of the song (gaps allowed), and no longer drift after a link drop; the countdown and Elapsed time are accurate after pauses and seeks; the Event Stream is no longer flooded; and a pending keychain prompt can no longer hold StagePilot up. From rc.3: old `lightkey-takes.json` captures are not replayed (record those songs again), and do not edit Lighting cues in v2.42.0 after trying this version.

### Changed

- **Lightkey capture is complete when the total recorded time is within 10 s of the song, gaps allowed anywhere.** Before, the capture had to cover the song from the start with no seek, no restart and no link drop of a minute or more. Now the stretches of song time really recorded (timer running, link and fixture stream live) are tracked, and a pass is complete when no more than 10 s of the song in total are missing, wherever they are: link drops of any length, small seeks, pauses, restarts and a late start are just gaps (shorter than 20 s songs still need half). Seeking no longer discards a capture or a loudness take; seeking back records the stretch again and the latest recording wins. Each stretch after a gap starts with a snapshot of every fixture property, so replay never carries a stale value across it. `coverage_seconds` is now the total recorded seconds. Messages: `Saved N looks to X's timeline (6 s of the song were not captured).` and `Incomplete capture: X of Y seconds recorded; it captures again next time.` Docs: `docs/lights.md`.

### Fixed

- **Lightkey captures made after a link drop were stored about 6.6 s early.** Cause: every 120 s the relay clock estimate was cleared when a ping burst started, so a burst sent while the Lightkey agent was frozen (answers sat in its buffer and were stamped when it woke) became the offset, wrong by half the stall, until the next burst. The estimate is now the smallest-round-trip sample of a 10-minute window and a burst never clears it; a poor estimate (no answer, or round trip over 0.2 s) repeats the burst after 3 s; a later estimate that moves the clock by more than 50 ms re-dates the changes recorded in between; every clock sample is logged (`lightkey_clock_offset`) and the HTTP and relay offsets are cross-checked. Reproduced with a frozen fake relay (test fails on rc.3); the 20 s mid-song freeze on the rig itself was not re-run.
- **Lightkey captures are always saved and finish by themselves.** A capture used to stay "recording" (and unsaved) until the next song started when the song ended without a stop event (the ProPresenter timer deliberately runs on into overrun). It now finishes and saves when the song clock passes the scheduled length plus 5 s, and also when StagePilot shuts down mid-pass (saved as incomplete, wait capped at 3 s). Every capture with at least one fixture change is saved, however short (the "too short" refusal is gone). A capture is "Fully captured" only when it reached within 10 s of the song's end (was 95 %); the rest are saved as "Incomplete capture" and are captured again next time. Songs with an unknown length are still not saved.
- The Event Stream is no longer flooded by ProPresenter connection events (one every 10 s pushed real events out within two minutes). Plugins publish a connection event only when the status or detail changes, and the event list keeps one `connection.changed` entry per source for connected and one for not-connected statuses (the newest replaces the older, moving to the top), so the last time an integration was connected stays visible. Dashboard connection entries no longer age out after 5 minutes.
- Now playing card: **Elapsed time** is now the song position after a pause and resume or a seek (it was counted from the last resume, so 01:09 elapsed with 03:24 remaining on a 05:36 song). Elapsed and remaining come from one source, so they always add up to the song length, also when stopped, after a seek and past the end (remaining holds 00:00).
- The song countdown no longer runs about 0.57 s behind Playback. It started when the song-started event was handled, after the first playing heartbeat had already reported a position; it is now anchored to Playback's reported position and the heartbeat's receive time, for song start, resume and seek. Docs: `docs/playback-api.md`.
- A keychain (or Credential Manager) prompt that is pending, refused or fails can no longer hold StagePilot up. On the macOS rig, rc.3 over 2.42.0 never opened its port while nobody answered the keychain approval dialog (the screen was locked). Every credential read and write now runs on a thread of its own and is waited for only 150 ms at startup (writes: 10 s on a request that is already off the main path). The server, the dashboard, Playback, ProPresenter, MIDI and lights come up at once. Only Planning Center says what is wrong (\"Waiting for keychain access\" or \"Keychain access was refused or failed\" in its status), asks again every 30 s, also when you sign in or save again, and connects by itself as soon as the keychain answers, with no restart. A stored secret is never replaced or wiped because a read failed, and a token that could not be saved yet stays in use for the session and is saved again later. **After an update macOS may ask for keychain access again; StagePilot keeps running while it waits.** The Remote credential broker serves each request on its own thread (one waiting dialog no longer stops the next), and the Remote status answers within 3 s even while the dialog waits. The microphone for the Lighting loudness feature behaves the same: an input open that never returns, is refused or fails never delays a song start beyond the documented 1.5 s and Record keeps capturing lights (new test).


## [2.43.0-rc.3] - 2026-11-05

Release candidate 3 (rc.1 stopped on a stray build-directory link committed by mistake; rc.2 stopped on a slow-runner test timeout on macOS Intel; neither was published). Pre-release; the in-app updater does not offer it). Smooth Lightkey capture and replay, a new Lighting screen, config panels that emerge from their card, and countdowns that work before the song order scan. Read before installing:

- **Old Lightkey takes are not replayed.** Captures stored only in `lightkey-takes.json` are ignored (the file is left alone). Record those songs again. Cue maps saved in settings load and replay as before.
- **Do not edit Lighting cues in v2.42.0 after trying this version.** v2.42.0 starts fine on a file saved here and keeps the new fields, but deleting or reordering rows there can attach ramp or message data to the wrong row.
- The passive learned song order is on by default (Playback API settings has the switch). It stays idle while a scanned order exists.
- Known limits for this candidate: the first look of a song can still leave 290 to 485 ms after its time (it cannot be sent before the song starts); later looks are within about 45 ms. Real Lightkey behaviour with the new replay is being checked on the rig.

### Changed

- Lighting UI for smooth capture: the **Detail** control (Fine / Normal / Coarse), its tooltip and the **Simplify** action are gone (a recording is fitted, not thinned; the API still accepts `detail_level` and ignores it, the UI never sends it). The timeline draws a **ramp** (a row with `fade_seconds`) as a bar from its start to start + duration (at least 4 px) with a tooltip of fixtures and from → to values, and a song's **effect loops** as striped bands (`4.0 s loop x 16 fixtures`, on at most three lanes) that open to period, window, fixtures with phase offsets, properties and a sparkline of one cycle. The legend has a new **Effect loop** type. The cue rows show a `+3.2 s` duration for ramps, edit it (0 to 60 s, inside the song), list each loop as one read-only expandable row, and ask \"Remove this loop?\" before removing one (new optional `remove_effect_ids` on the cue-map save; Clear song also clears its loops). Merge is disabled, with the reason, for a ramp with an instant row and other combinations that would change meaning. The strip reads \"N captured items (R ramps/steps, E loops)\". Fixes from PR #296: the state line joins statements with ` · ` and ends each with a full stop, the three strip actions wrap as one group so `How to find a cue's address` is never alone on a row, and Connect / Check again is a neutral outlined primary instead of the crimson of a selected segment.
- Lighting (Lightkey smooth replay): **step bursts start before their row so a big look lands on the beat**. A burst that would take longer than 40 ms at the 5 ms spacing (10 or more values) is started `(n - 1) x 5 ms / 2` early, at most `STEP_LEAD_MAX_MS` = 300 ms, so its middle lands on the row time (dimmer, colour, other levels, angles, in that fixed order; never before the song position of the tick or the previous burst, a small row right before a big burst goes with it); single steps and small bursts keep exact timing. The fixtures are held at the row's values until the row time, an operator take-over drops the waiting value without a hole in the pacing, and running ramps keep every 4th slot while steps drain (a burst's steps are no longer charged to the token bucket) and compete by their distance in tolerances instead of DMX steps. On the two real takes the step lateness against the row (without the first look of the song, which cannot start early) goes from (lead off) median 11 / 10 ms, p99 102 / 80 ms, max 117 / 95 ms to median 2 / 5 ms, p99 35 ms, max 45 / 40 ms; the largest dimmer deviation outside effects on Able goes from 0.181 to 0.057. `replay_budget` prints the lateness. Not verified on a rig: whether fixtures visibly change early.
- Lighting (Lightkey smooth replay): a **pacing queue** between the engine's ticks and the 5 ms spacing. Steps leave first, then ramp values, then effect values, oldest first inside a class; a ramp or effect value still waiting when a newer one for the same address arrives is replaced by it (a stale value is never sent after a newer one); the 200 packets a second cap and the 5 ms spacing are unchanged. The replay tick no longer waits for the queue to drain. On the two real takes the wait a step gets beyond the spacing of the steps of its own tick is at most 6 / 20 ms; what is left (up to 290 / 485 ms) is a look change of 59 / 98 values that the 5 ms spacing itself needs 59 / 98 slots for. A zoom change that the old queue left unsent for 11 s on one take is now sent. `replay_budget` also prints the step burst sizes and the wait added by the queue. Docs: `docs/lights.md`.
- Lighting (Lightkey replay): **smooth replay**. A song with ramps (`fade_seconds`) or effects is now played by an interpolation engine instead of one packet per row: the value of every fixture property at the song position is a pure function of the rows and effects, so seek, resume and re-sync just evaluate at the new position, and pause, stop and the end hold the last look (nothing is cleared). One scheduler tick (no task per fixture, nothing while idle) runs up to 20 Hz while something ramps, sends an address only when its value moved by at least one DMX step (1/255 for levels and colours, 16-bit for angles; a value still moving is sent half a step higher because Lightkey truncates), keeps the total at 200 packets a second with the existing 5 ms pacing, lowers the effect rate first (down to 8 Hz) when more is due and never delays a step. One OSC message per address, no bundles or patterns. Effects play from their cycle with per-member offsets by exact period arithmetic (no drift), and the first and last 0.5 s of an effect blend with the rows around it so there is no jump at the seam. The operator wins per fixture with interpolated values (the echo rule sees every send; a fixture taken over is not driven and not cleared, and is taken back after 30 s at the interpolated value for that moment). Songs captured before this change (no fades, no effects) play exactly as before. New `replay_budget` example reports packets per second, the share sent at the lowered rate, the deviation from the raw motion and the CPU time on a simulated clock. Real Lightkey playback is not verified (rig validation is a separate step). Docs: `docs/lights.md`.
- Lighting (Lightkey capture): a recorded song is now fitted with straight-line **ramps** and detected **effect loops** instead of being thinned by the Detail setting. Per address the recording becomes piecewise-linear ramps within 3 % (dimmer, colour per channel, levels) or 1.5 degrees (angles), at most 60 s each; a silence of more than 0.5 s is held and then stepped, never stretched into a ramp; options, wheels and text step. The ramps and steps of one fixture that start together become one row with the new optional `fade_seconds` (0 or absent = instant, so older maps load and play unchanged). A repeating motion (a pan sweep, a colour pulse, a dimmer chase, with per-fixture phase offsets) is found by period search and stored once as an entry of the song's new `effects` list (`start_seconds`, `end_seconds`, `period_seconds`, `members` with `offset_seconds`, one `cycle` of ramp steps); it is accepted only when it rebuilds the recording within twice the tolerance for at least 98 % of each member's points, otherwise that window stays ordinary rows. On the two real jpl-graphics takes this is 546 and 312 items (rows plus loops) instead of 439 and 387 thinned rows that lost the motion, and 257 KB / 139 KB of JSON against 1.1 MB / 432 KB. A new `smooth_report` example prints the item counts, JSON size, reconstruction error and every detected effect for a takes file. The Detail setting (`lights.lightkey.detail`, `detail_level`) is still accepted and stored but no longer changes the result, and the status messages no longer mention Detail or spacing; a song too large for the caps is fitted again at twice the tolerance and says so. Editing: saving the cue map from the timeline keeps the song's effects (a row edit, delete or merge never changes an effect); a new recording replaces the captured rows and captured effects. Docs: `docs/lights.md`.
- Configuration panels now emerge from their connection card (500 ms; a 120 ms fade with reduced motion): the panel scales out of the card's width and down from its bottom edge, the dashboard below slides by the same amount so the gap never changes, and the content fades in late. Only `transform` and `opacity` animate (no height, grid-row or clip-path animation), so the dashboard is no longer laid out again on every frame; this replaces the earlier height and clip-path funnel. A small notch on the panel's top border, in the connection colour and the panel's exact fill, points at the card and rises out of the panel's edge as the panel finishes opening (it sinks back into it at the start of a close); when another card is picked while a panel is open, the notch glides along the edge to it instead of the panel closing and reopening. Interrupting an open or close turns it round from where it is. Focus returns to the card on close. Dev hook: `localStorage.setItem("spGenieMs","6")` makes the animation six times slower for inspection (a value of 50 or more is read as milliseconds).
- Configuration panels: the notch now has exactly the panel's colour on every panel (measured, at most 0.8/255 per channel after a first open, a card swap, a close and re-open, a window resize and at the end of an open; was up to 4/255 here and up to 13/255 on a bright backdrop). The panels are drawn with the opaque panel colour instead of 94% translucent, because the translucent fill showed the page's backdrop and shadows, which differ under the notch; the look changes by a few levels (the faint red of the backdrop no longer tints the left panels). The notch's diagonal edges no longer overshoot the border at the base corners. The collapse now ends smoothly: it accelerates into the card, ends at zero height and fades out over its last stretch, the content below ends exactly at its closed position and the panel unmounts the moment the animation ends, so no thin strip hangs under the card and pops away.
- Lighting: the **sACN DMX Live Link** block now follows the approved design. One bordered block with the title and a single **state pill** (`Connected · 66 fixtures`, `Not connected`, `Connecting…`) beside it, one wrapping field row (host, OSC UDP, API, a never-dimmed Connect or Check again, and **Mode** and **Detail** as labelled toggles at the right; Detail is shown in Auto and Record only and carries its explanation as a tooltip), and a quiet strip with one line of state text, Release lights, **Send a test cue** (▸ closed, ▾ open) and **How to find a cue's address**. The block is about half as tall at 1440 px. Each status fact is now said once: the plain "Not connected" text, the separate "Lightkey API did not answer." line (now the pill's tooltip and part of the state line), the duplicate "Ready, delivery not confirmed..." footer and the Detail paragraph are gone, the "OSC ready" note is one short neutral info line instead of a pink banner, and the truncated cue-address help is fully readable behind its disclosure. Behaviour, saved settings and the Auto, Replay and Record logic are unchanged.
- Configuration panels now open and close with a short genie-style animation: the panel grows out of its connection card while the dashboard below moves down with it, and closing runs in reverse (300 ms; a 100 ms fade with reduced motion; plain vertical emerge on phones). The panel visibly comes out of, and goes back into, its own card (a funnel starting at the card's width, lifted to meet the card's bottom edge). Focus returns to the card on close. Dev hook: `localStorage.setItem("spGenieMs","3000")` slows the animation for inspection.

### Added

- Lightkey **Auto mode** (per song): the Lighting link now has Auto | Replay | Record (`LightkeySettings.mode`, default Auto; old files without `mode` load as Auto and `replay` stays the master replay switch). In Auto, when a song's timer starts the link captures songs that are not fully captured, replays fully captured songs, and replays when there is no lighting connection (link connected, Lightkey running, fixture stream live, Lighting on; a 60 s grace applies). The decision never changes mid-song. A pass is **fully captured** at 95 % coverage with no playhead move, no link drop of 60 s or more and at least one change; partial passes (10 % or 10 s, with a change) are saved marked incomplete and never replace a complete capture. Songs show Fully captured / Incomplete capture / Manually cued / Manually cued & fully captured / Not captured. Status reports the mode and the Auto reason.
- Lighting screen, round 2 (part B): the OSC connection is now one titled **sACN DMX Live Link** line (status, one shared host or IP box, small OSC UDP and API port boxes, Connect, Replay | Record, and Detail beside it only while Record is chosen); a saved API address on a different host shows a notice with **Use the OSC host** and is never rewritten silently. The test cue moved under it as a collapsed **Send a test cue**. A **Songs in this service** strip shows one chip per plan song (captured coloured with count, loudness glyph and a clear x; not captured greyed and dashed; click to select, green dot for the live song). The timeline section is a one-row 34 px toolbar (Song, Add cue, Clear song..., Merge rows, Simplify, Undo, Zoom with Fit, Height S / M / Auto), the Cue board folds away, and the timeline height follows the stacked label rows in Auto (200 to 420 px) with the waveform and playhead kept below the labels.
- Config panels: the **Playback API | MIDI** and **OSC | MIDI** toggle now sits in the middle of the panel header, between the title and the status pill and close button, in the same place on the Playback API, Playback MIDI and Lighting screens (when the panel is at least 912 px wide; narrower it stays under the title).
- Playback: the **Song order** section can be folded, remembers that on this device, and shows a one-line summary and the scan button even when folded. The button is now always **Scan Playback Songs** (it was "Set up song order" / "Set up again").
- Lighting: **Clear**, **Clear all** and **Clear loudness** now ask in a proper dialog that says the cues will be permanently erased and cannot be undone, with counts of what goes. Clear loudness used to ask nothing.
- Lighting timeline: **colour-coded cue types and a legend filter.** Markers and cue-list rows now show what a cue is: captured, edited or manual look, Lightkey cue on (solid) or off (hollow), fixture message, other OSC (dashed), and on the MIDI timeline one colour per note letter (sharps striped with a ♯). Each type also has a glyph box and a spoken name, so colour is never the only signal. The legend above the timeline hides or shows a type on the timeline and in the list (view-only, kept for the session, "N types hidden" and Show all); a selected marker is never hidden. The pad colour is now a small square after the label instead of the bar colour. Mixed clusters get a neutral chip that names the counts.
- Lighting: **room loudness behind the timeline.** While Record is chosen and a song runs, StagePilot measures how loud the room is (0.25 s windows, -60 to 0 dBFS) and draws it as a faint filled envelope behind the look rows, scaled to the song's own loudest moment and zooming with the timeline ("Loudness: peak at 02:10" for screen readers). Choose the audio input (built-in microphone by default, or an audio interface) in the Lightkey strip with a live level meter, and clear a song's loudness with **Clear loudness**. Only the numbers are saved: audio is never stored or sent. Recording a song again averages the loudness together (at most eight recordings, so it can still adapt). Where audio is unavailable (Docker or headless, no input, permission refused) the picker is hidden with one line saying why, and the lights capture is never affected. macOS asks for microphone permission once (`NSMicrophoneUsageDescription`; see `docs/lights.md`).
- Playback: countdowns now work right after Playback connects, before you scan the song order. StagePilot learns which Playback song is which plan song as they play (labelled "Learning the order as songs play"), and a scan always replaces it. The Playback panel shows the order status, a hint when a song is not placed or looks wrong, and a switch to turn learning off.
- Lighting timeline: captured lights show as **Look** rows ("Look - 12 fixtures"), drawn in a different colour from rows you made. Open a row to edit or delete single messages (same value editors as a single cue), add a message, move the row (0.1 s), or remove a fixture from the whole song. Rows you change, or delete, are marked "edited", and Record asks before it replaces a song's captured looks, including your edits. **Merge rows** (tick rows, Shift-click markers, drag across the timeline, or long-press on touch) and **Simplify this song** (the backend thins and merges the rows, with a "412 rows → 190" preview) cut the number of items, each with one-step Undo. Crowded markers collapse into a numbered cluster that zooms in when clicked; the cue list shows 40 rows per page. The Lightkey strip says "N captured looks on this song", the take list is gone, and a Detail setting (Fine / Normal / Coarse) appears while Record is chosen.
- Lightkey replay: the operator wins per fixture. A change made in Lightkey on a fixture the replay holds (not the replay's own echo) makes replay stop sending to that fixture without a clear; replay takes it back after 30 seconds without a further operator change, or at the next resume if the song is paused. The echo is matched by value (within 2 %, 1 degree for angles) inside about 200 ms, and only properties the replay itself sent are judged, so Lightkey's own running cues never count as the operator. The Lightkey strip shows "Holding N fixtures" and "M yielded to the operator" and has a **Release lights** button (`POST /api/v1/lights/lightkey/release`) that gives every held fixture, and every fixture a saved look names, back to Lightkey, so it also works after a restart. Pressed while a song plays it is momentary. A cue activated in Lightkey on a held fixture is probably not visible in DMX; use Release lights.
- Cue-map safety: a cue-map save may carry the song's `revision`; an older one is refused with 409 so an edit on a stale list cannot overwrite a recording that just finished. The manual cue-map save and the Lighting settings save now take the same lock as the recording and capture saves. The cue-map route accepts up to 8 MB. A look message may carry at most 8 arguments and a text argument at most 256 characters.

### Fixed

- Playback no longer shows Disconnected and reconnects every few seconds while it is still loading a setlist. Playback sends heartbeats without a song during that time; they now count as "connected, no song yet", and Discover Song Order says "Playback is still loading the setlist." until a song is loaded.
- Lightkey replay: pausing, stopping or finishing a song no longer hands the lights back to Lightkey's cues. The last look is held; resuming or seeking re-syncs the fixtures. They are released only when Record is armed or replay or Lighting is switched off, the app shuts down or is reconfigured, a new song starts that does not use them, or you press Release lights. A seek back to before a fixture's first row gives that fixture back.
- Lightkey replay: a new song's first look is no longer delayed by releasing the previous song's unused fixtures, and one failed send (a dropped packet, a reconnect) is retried once and skipped instead of silencing the rest of the song.
- Lightkey recording: a pause, even late in the song, no longer ends the recording and saves a partial take over good rows; only the song's end, a new song or Record off saves it. A take that ran past the song's scheduled end no longer loses its whole fixture track ("A fixture change can hold at most 2048 commands"): the last value of each fixture after the end is folded into the final row, an oversize change is split, and one bad change never discards a take.
- Lighting: the timeline follows only the song that is playing, so a song ending no longer drops your hand pick; rows you are typing are kept when the settings refresh or the follow moves; the Lightkey strip says "Replay is off" (with a button) when the saved setting is off.
- Loudness: opening the audio input (which can wait for the macOS microphone dialog) no longer blocks the song start, the picker or other requests.
- Opening the Lighting or MIDI configuration panel no longer makes the page lay out and repaint on every frame (about 1.2 layouts per frame, now about 0.3 like the other panels). Two causes: the song-scan progress bar kept a frame loop running even when no scan was in progress, and the Lighting panel's content fade stayed attached to a section that was replaced a moment after the panel opened.

### Changed

- Lightkey Detail: the status carries `detail_level` and the settings route takes `detail_level` (what the Lighting screen sends; `detail` still works). A recording too large for the chosen Detail is still saved at twice the spacing, and the message now says so ("Saved 88 looks to <song>'s timeline (reduced detail, 1 s)."). New route `POST /api/v1/lights/lightkey/simplify` runs the thin + merge step on a song's captured looks (preview, then apply); cue maps carry `captured_edited` so a deleted captured row still counts as an edit.
- Lightkey recording now saves to the song's timeline instead of a separate take. When a song ends (or Record is turned off with at least half recorded) the fixture changes are thinned to the Detail setting (Fine 0.25 s, Normal 0.5 s, Coarse 1 s) and merged into a few look rows on that grid, saved as the song's captured rows (manual rows stay), and the raw data is discarded. A rig-shaped take (109k commands, 63 addresses) becomes about 357 rows and 5,900 messages at Normal. The takes list, its Clear buttons, the takes route and `lightkey-takes.json` are gone; an old takes file is ignored and left alone. Status now reads `Recording <song>.`, `Saved N looks to <song>'s timeline.` and `Not saved: only X of Y seconds recorded.`
- Lightkey captures hold fixture data only. Cues are no longer recorded or replayed, so the song's cue state no longer overrides the fixtures. Earlier captures still load (their cue data is ignored), and a song without a fixture capture shows "no fixture capture" and replays nothing.
- Lighting: the song timeline now follows the live song (or the last one played); a hand pick holds until the next song starts, with a "Follow live" button. The two Lightkey switches are now one Replay | Record toggle.
- Look rows carry the placeholder address `/stagepilot/look` (their messages are what is sent), so an older StagePilot never sends a fixture address without its value. Do not downgrade with look rows saved.
- Developer-facing only, no change to the app: the retired Python backend (`backend/`), its CI jobs, the `python`/`rust` installer variants and the Python fixture generators are removed. Every build is the in-process Rust app. The 2.41.1 sources remain at the `v2.41.1` tag, and the migration notes moved to `docs/archive/`.
- Remote Access: if you have turned it on and it stays unreachable for 2 minutes, the system check lists "Remote access offline" and the header pill says so instead of "Ready". It does not block starting a service, appears only while the problem lasts, and never shows when Remote Access is off.
- A lights test no longer races the plugin recording its last cue on slow runners.

## [2.42.0] - 2026-10-25

StagePilot 2.42.0 is the first release that runs on the new Rust backend. The backend is now part of the app itself: there is no separate Python program to start, update or sign, and the app is ready within about a second of opening it. Your settings, Planning Center sign-in, Playback song order and Lightkey takes carry over unchanged, and the in-app updater offers this update to earlier versions.

### Changed

- The backend is built into the app. Opening StagePilot starts everything, including the remote-access tunnel.
- The saved log level is honoured. The log records when each output (lighting, MIDI, ProPresenter, Lightkey, Playback, Planning Center) connects or disconnects, and old backend logs are pruned so they stay under 100 MB.
- Lighting over OSC reads "Ready, delivery not confirmed" until the first cue is sent. MIDI ports are listed in the Lighting setup even when OSC is the chosen method.

### Fixed

- Planning Center: the plan keeps loading past the sign-in token's two-hour life. The app uses the refreshed token on every request and signs in again once if Planning Center rejects it.
- Planning Center: valid plans with empty title or length fields no longer log a warning at every launch, and a failed start is recorded once.
- Planning Center lengths: the lengths dialog works (categories, preview, confirm and restore).
- Remote access: the remote sign-in page loads again. After the app is killed or crashes, the tunnel reconnects at the next launch instead of staying on "reconnecting". The tunnel token is no longer visible in the process list, and the bundled cloudflared is updated to 2026.10.3.
- Lighting: with Record from Lightkey and Replay captured lights both on, each status line shows once and replay reads "Replay paused while recording." Old status messages clear after 10 seconds or when the state changes. Record from Lightkey stays armed across a restart, and replay spreads its OSC messages out instead of sending a burst.
- ProPresenter: timers report their running state, look changes are confirmed by position and name or id, and an unknown look is no longer saved.
- Playback: after scanning a new setlist, skipped songs that are no longer in it are dropped. Saving the song arrangement works again, and the settings file is no longer treated as damaged at the next launch.
- Settings: unknown settings are kept instead of resetting the file. A damaged or newer settings file is copied aside as `settings.json.corrupt-<time>`, reported in the app, and never overwritten.
- The app, the health report and the Planning Center user agent show the real version, and health no longer always reads "degraded".
- Playback: a network outage and a refused connection show different messages.

### Security

- The dashboard PIN locks out after repeated wrong tries from one address, and failed tries are logged. Changing requests from another website origin are refused, and responses carry standard browser security headers.
- Health details (plugin states and errors) need a signed-in session; the public health check shows only status and version.

### Known limits

- The Windows installer is not code-signed, so Windows SmartScreen shows a warning the first time. The macOS apps are signed with the project's own certificate and are not notarized.
- The default dashboard PIN is still 1234. Change it in the dashboard settings.

## [2.42.0-rc.5] - 2026-10-25

Release candidate. This is the first build whose backend is the Rust server linked into the app (no bundled Python). It is published as a GitHub pre-release, is never offered to installed apps by the updater, and the stable line stays on v2.41.1.

### Fixed

- Planning Center: the plan no longer stops loading about two hours after launch. The app now uses the refreshed sign-in token on every request, and signs in again once if Planning Center rejects it.
- Planning Center: valid plans with empty title or length fields no longer log a warning at every launch, and a failed start is recorded once.
- Planning Center lengths: the lengths dialog works again (categories, preview, confirm and restore).
- Remote access: the remote sign-in page loads again; it returned an error instead of the sign-in form.
- Remote access: after the app is killed or crashes, the tunnel reconnects on the next launch instead of staying on "reconnecting". The tunnel token is no longer visible in the process list, and the bundled cloudflared is updated to 2026.10.3.
- Lighting: with Record from Lightkey and Replay captured lights both on, each status line shows once and replay reads "Replay paused while recording." Old status messages clear after 10 seconds or when the state changes.
- Lighting: Record from Lightkey stays armed across a restart, and replay spreads its OSC messages out instead of sending a burst. Lighting OSC shows "Ready, delivery not confirmed" until a cue is sent.
- ProPresenter: timers report their running state, look changes are confirmed by index and name or id, and an unknown look is no longer saved.
- Settings: unknown settings are kept instead of resetting the file, a damaged or newer settings file is copied aside as `settings.json.corrupt-<time>` and reported in the app, and the file is never overwritten when it cannot be read.
- The app, health report and Planning Center user agent now show the real version instead of 2.41.1, and health no longer always reads "degraded".
- Playback: a network outage and a refused connection show different messages.

### Security

- The dashboard PIN now locks out after repeated wrong tries, and failed tries are logged. Changing requests from another website origin are refused, and responses carry standard browser security headers.
- Health details (plugin states and errors) need a signed-in session; the public health check shows only status and version.

### Changed

- The saved log level is honoured. The app logs a line when each output connects or disconnects, and old backend logs are pruned to stay under 100 MB.

## [2.42.0-rc.4] - 2026-10-23

Release candidate. This is the first build whose backend is the Rust server linked into the app (no bundled Python). It is published as a GitHub pre-release, is never offered to installed apps by the updater, and the stable line stays on v2.41.1.

### Fixed

- Release validation: the contract test helper is loaded by path so mypy agrees when run from the repository root (the previous release candidate, v2.42.0-rc.1, failed validation on that and was never published).

## [2.42.0-rc.3] - 2026-10-23

Release candidate. This is the first build whose backend is the Rust server linked into the app (no bundled Python). It is published as a GitHub pre-release, is never offered to installed apps by the updater, and the stable line stays on v2.41.1.

### Added

- Lighting: the Lightkey live link also captures color, intensity and mover movement. With
  `lightkey-api` 0.4 or newer it subscribes to the agent's OSC fixture relay, records each fixture
  change with its song time next to the cue changes, and replays them in Lightkey's own OSC form.
  Replay clears every fixture it held when the song ends or stops. Older agents keep working with
  cue capture only. Rust only: the Python backend is not changed. See `docs/lights.md`.

## [2.42.0-rc.2] - 2026-10-22

Release candidate. This is the first build whose backend is the Rust server linked into the app (no bundled Python). It is published as a GitHub pre-release, is never offered to installed apps by the updater, and the stable line stays on v2.41.1.

### Fixed

- Release validation: the contract test helper is loaded by path so mypy agrees when run from the repository root (the previous release candidate, v2.42.0-rc.1, failed validation on that and was never published).

## [2.42.0-rc.1] - 2026-10-22

Release candidate. This is the first build whose backend is the Rust server linked into the app (no bundled Python). It is published as a GitHub pre-release, is never offered to installed apps by the updater, and the stable line stays on v2.41.1.

### Added

- Release candidates: a `vX.Y.Z-rc.N` tag builds the Rust (in-process) installers, with no Python
  sidecar, and publishes them as a GitHub pre-release that is never "Latest" and so never reaches
  the in-app updater. Plain `X.Y.Z` tags are unchanged. A manual **Rust installer CI** workflow
  builds the same installers without tagging or secrets. `create-release.ps1` accepts `X.Y.Z-rc.N`.
- Lighting: a small Lightkey live link. Connect to the `lightkey-api` agent on the Lightkey
  Mac, record which cues Lightkey turns on and off during a song's countdown, and replay them
  with that song later. Replay is on by default, has its own switch, pauses while recording and
  leaves any cue the Lightkey operator changes alone for 3 seconds. OSC only. See
  `docs/lights.md`.

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
