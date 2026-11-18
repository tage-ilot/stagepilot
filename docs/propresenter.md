# ProPresenter integration

StagePilot controls one reusable ProPresenter timer through ProPresenter's local HTTP API. When a song cue arrives, StagePilot stops the timer, configures it as a Countdown Timer with the song duration, resets it, verifies the saved duration, and then starts it.

## Required ProPresenter setup

1. Open ProPresenter **Settings → Network** and enable the API.
2. Note the configured API port. StagePilot defaults to `1025` but does not assume that value.
3. Create a countdown timer named `Song Countdown`, or select another detected countdown timer in StagePilot.
4. Link the timer to the desired ProPresenter stage layout.

## StagePilot configuration

The production setup panel persistently stores:

- Host
- Port
- Timer name
- Request timeout
- Connection test
- Timer rediscovery

Save the connection settings in the panel. Saving automatically enables real
ProPresenter output and connects right away, with no restart; no simulated
output choice is shown in the production panel. Switching between simulated and
real output is refused while the timer is running. Environment overrides remain available for
development or unattended startup:

```dotenv
STAGEPILOT_TIMER_OUTPUT=propresenter
STAGEPILOT_PROPRESENTER_HOST=127.0.0.1
STAGEPILOT_PROPRESENTER_PORT=1025
STAGEPILOT_PROPRESENTER_TIMER_NAME=Song Countdown
STAGEPILOT_PROPRESENTER_TIMEOUT_SECONDS=3
STAGEPILOT_PROPRESENTER_RECONNECT_INITIAL_SECONDS=1
STAGEPILOT_PROPRESENTER_RECONNECT_MAX_SECONDS=30
STAGEPILOT_PROPRESENTER_HEALTH_CHECK_SECONDS=10
```

## Connection status

ProPresenter is checked every few seconds (`STAGEPILOT_PROPRESENTER_HEALTH_CHECK_SECONDS`
while a timer is running, the reconnect interval otherwise). A connection event is published
only when the status or its detail changes; an unchanged check just refreshes "last checked".
In the event stream (`recent_events`, also in `GET /api/v1/state`) the newest `connection.changed`
entry per source replaces the older one, separately for connected and not-connected
statuses, so the last time ProPresenter was properly connected stays visible after a
disconnect.

## ProPresenter Link

The ProPresenter Link is how StagePilot reaches the **files and the screen of the ProPresenter computer**: it reads the
song files (to find which MultiTracks song each one is), adds new files to a library named `Playback`, and, on a Mac, drives
ProPresenter's own Search window to import a song. What it is used for is [Adding Playback songs to a playlist](#adding-playback-songs-to-a-playlist)
below. It is optional: with no ProPresenter Link StagePilot works as before.

**Which connection (no switch, the saved ProPresenter computer decides).** The link is used while the ProPresenter output is
in use and a ProPresenter host is saved (`propresenter.host`). If that host is this computer (`127.0.0.1`, `localhost`, `::1`,
or one of this computer's own addresses) the link is **Local**: StagePilot reads and writes the ProPresenter folder itself, and
there is nothing to install, no key and no setup. Any other address is **Helper**: a small signed program on the
ProPresenter computer (the ProPresenter Link, port 8790) does it. While no ProPresenter host is saved nothing is asked, ever.

**Folder.** `~/Documents/ProPresenter` by default; `propresenter.link.data_folder` (a full path, optional, nothing secret) points
elsewhere. Every path is relative to it: `..`, absolute paths and links leading out of the folder are refused. Only `*.pro`
files are listed, and a file may be up to 40 MB. Files are only ever **created** (never replaced, renamed or deleted), only
in the library `Playback` (a library folder of another name can be created, never `Songs`), through a temporary file, fsync,
rename and a SHA-256 check. A song's MultiTracks number is read from its file by a small streaming reader (no whole-file load,
cached by path, size and time). The screen steps (check access, import a song through Search, select a library row) are
scripts bundled with StagePilot (`crates/stagepilot-propresenter/assets/gui`); values are passed to them as arguments, never
built into script text. Import needs a Mac with Accessibility permission for the program that runs the scripts, and says in
plain words when it cannot.

**Status.** `GET /api/v1/propresenter-link/status[?check=1]` answers
`{mode: "local"|"helper"|null, state, message, version?, bundled_version, can_import, import_blocker?, libraries, playback_library_exists, data_folder?, computer_address?, paired, updater, stale, update?}`.
`state` is `ready`, `not_installed`, `outdated` (works, an update is available), `needs_command` (the helper does not know this
StagePilot's key: run the setup command again), `not_answering` or `checking`. It is read when a ProPresenter host is saved and
at most every 30 seconds while a window asks (the Check button waits 2 seconds, 10 after a refusal); a helper that goes quiet
keeps its last good answer for a minute, marked `stale`. The state never changes the header's Ready / Check system verdict.

**Setup (Helper only).** `POST /api/v1/propresenter-link/helper-install` opens a 30-minute session and answers a command
to paste into Terminal on the ProPresenter computer; `DELETE` closes it. The command fetches `/propresenter-helper/<token>/install.sh`
and `/propresenter-helper/<token>/files/<name>` (a 128-bit token, private network sources only, never through the remote tunnel);
the installer checks every file against `SHA256SUMS`, installs a LaunchAgent (`io.stagepilot.propresenter-link`, in
`~/Library/Application Support/ProPresenterLink`), reads the key from a private temporary file (never a command line), and
answers only the StagePilot computer's address it was given. The session ends when the helper answers a signed request. StagePilot
makes the key once, keeps it in the credential store under its own name, and never shows or logs it. When the saved host is this
computer the route answers `409` with `code: "not_needed"` and one plain sentence.

**Helper requests.** Every request except the bare `GET /health` carries `X-PPL-Time` and `X-PPL-Sig`
(hex HMAC-SHA256 of `METHOD\n<path+query>\n<time>`, and for `PUT`/`POST` also `\n<SHA-256 of the body>`), within 120 seconds;
ten bad signatures from one address lock it for a minute (`429`). Endpoints: `GET /health`, `GET /index`,
`GET /libraries/<name>/files`, `GET /libraries/<name>/files/<file>`, `PUT /libraries/Playback/files/<file>` (create-only,
`X-Sha256`), `POST /libraries/<name>`, `POST /import {id, name, playlist}`, `POST /select-library {name}`, `POST /update`.
The helper updates itself like the Library Link's and the Lightkey helper's: StagePilot sends the signed bundle once nothing
plays and no timer runs (`helper_auto_update` turns it off). This helper is a copy of the Library Link's design with its own
code and key; the two helpers are independent. Without a Mac or the Accessibility permission, the files still work and
`can_import` is `false`.

To try it without ProPresenter: `python3 tools/fake_propresenter_link.py --port 18790 --key-file key.txt --allow-client 127.0.0.1 --allow-loopback`
runs the real helper on a temporary folder with the screen replaced; `tools/fake_propresenter_api.py` stands in for ProPresenter's
own HTTP API (libraries and playlists) for the later arrangement work.

## Adding Playback songs to a playlist

StagePilot can put the songs Playback plays into one ProPresenter playlist, in setlist order, at a place you choose, each one
**with the arrangement and the key Playback plays it in**. It needs the Playback Library Link (for the songs, their sections,
their keys and their MultiTracks numbers), the ProPresenter Link (for the files; **Ready**) and ProPresenter's own network
setting (its API, port 1025). Nothing happens unless a person asks for a **preview** and then presses the button for exactly
what the preview showed.

**What pressing Add songs does, in the window's words:** *This imports songs, arrangements and keys into <playlist>. The
playlist's contents are replaced with what the preview shows.* The playlist is written whole, so what is in it afterwards is what
the preview showed; that is expected and the window does not warn about it. *Nothing in ProPresenter changes until you press Add
songs.* stays as it was.

**What happens, song by song.**

1. The song is found in ProPresenter by its **MultiTracks number** (Playback's `content_id`, which is also the number inside the
   song file ProPresenter has), never by its title.
2. When ProPresenter does not have it, it is **imported through ProPresenter's own Search** (Mac with Accessibility permission;
   the song is named after its title without dots and slashes, because ProPresenter cuts a name at the first dot). The import is
   asked not to touch any playlist; StagePilot does the playlist itself. After it, StagePilot checks that the new file really is
   the song that was asked for; if not it stops with `wrong_song` and adds nothing for that song. **Nothing is typed blind:** the
   screen script checks, before every key it sends, that ProPresenter is the front app and that the field meant to get the key
   has the keyboard; after typing the song number it reads the Search field back and after typing the name it reads the Name
   field back, and each must hold exactly what was typed (the Import dialog's other fields must still hold what they held, and
   its Library must still read Songs). Anything else stops the script with `ERR|type the song number|...` (at most 40 characters
   of what the field held), Cancel is pressed, the panel is closed and nothing more is typed. StagePilot also reads the playlist
   before and after the import step: the song is imported with Playlist "None", so its item count must be the same, and a
   difference is listed in the result.
3. A **copy** is made in the library named **Playback** (made when it is not there): `<Song> (n).pro`, with the lowest `n` from 1
   that no file in any library has, so a name is never reused (ProPresenter ignores a file replaced under the same name). The
   copy keeps **all of the song's own arrangements** (the people running ProPresenter can still change the section order by
   hand), adds one named `Playback` made from the sections Playback plays (repeats stay separate entries; "Count Off" is the
   song's blank-named group) and selects it. A section is matched to a group first by laying the names alongside the song's own
   `Full` arrangement, then by the one group with that name; several groups of one name when they say the same thing.
   **A section no group has blocks that song. A section that fits more than one group (which Interlude is repeated, which blank
   "Count Off" group) is guessed and the song goes on**: the Nth use of a name (counting every use in Playback's list, from the
   first) takes the Nth group of that name, taken in the order the song's own `Full` arrangement first plays them when `Full`
   plays every one of them, else in the song's group order. When Playback uses the name more often than the song has groups:
   with exactly **one** use too many the extra use takes the **last** group again (A, B, B); with **two or more** too many the
   groups go **round and round** (A, B, A, B, A, like "Bridge 1, Bridge 2, Bridge 1, Bridge 2"). Every guess is listed, with the
   rule that picked it (`guessed`, `guesses` in the preview and in the result) as *Interlude (3rd use) uses the 2nd "Interlude"
   group: check it in ProPresenter.* and, after the import, as a short *Check in ProPresenter:* note under the song. A guess
   never blocks, needs no tick and does not turn **Add songs** off; if it is wrong the person fixes the arrangement in
   ProPresenter by hand. Every id inside the copy is new (consistently), its dates are now, its name is unchanged and everything StagePilot does not know about in the
   file is copied as it was, byte for byte.
4. If a copy that **already plays what Playback plays now, in the same key** is in the Playback library, it is used and nothing
   new is made. When several files in `Songs` carry the number, the one with a `Full` arrangement whose groups match is used;
   when more than one qualifies the song is `needs_choice` and is never guessed.
5. **Screen steps never run while ProPresenter is showing something.** Before any step that types or clicks in ProPresenter (the
   import through Search, selecting the Playback library) and again right before each one, StagePilot asks ProPresenter's
   `GET /v1/status/layers`. If `slide`, `media`, `announcements`, `props` or `messages` is on, nothing is typed or clicked: the
   job stops with `live_on_screen` (*ProPresenter is showing something on screen (slides). Clear it or wait, then press Try
   again.*, naming the layers), the playlist is not written, files already made are kept and **Try again** is safe. A camera
   input (`video_input`) and `audio` are ignored (a camera is often on). `GET /v1/presentation/active` is not used: it stays set
   after a Clear. An answer that cannot be read counts as live, and the message says so. The check is in the engine, so the same
   code protects this computer and another computer's helper.
6. ProPresenter loads a new file **lazily**: StagePilot waits up to 30 seconds (looking every 2) for the copy to get an id in
   ProPresenter's Playback library, selects that library on screen once after 10 seconds (Mac), and if the id is still missing
   answers `needs_library_open`: **Open the Playback library in ProPresenter once, then press Try again.** Nothing is written to
   the playlist in that case. Try again is a new preview and apply: files made before are reused, so it is safe to repeat.
7. Then the playlist is written **once**, as a whole: every item that was there is sent back exactly as it was read (id, name,
   kind, hidden and PCO flags, header colour, duration), the new songs are inserted at the place, the indexes renumbered. A
   song item names the Playback-library entry (its id twice, its name without ` - [ Playback ]`) and `Playback` as the
   arrangement. About 10 seconds later the playlist is read back and every difference from the plan (order, names, kinds,
   durations, the arrangement of each new song) is listed. A song whose copy is already in the playlist is not added twice.

**Where.** `after_header: "<name>"` is the end of that header's section (before the next header, or the end of the list); `end`
is after the last item. A header that does not exist, or that appears twice, blocks the change.

**The key.** Playback's key is written into the copy (`music.user` and its text) as ordinary data. **ProPresenter does not show
a key for these songs, so nobody should expect to see it there**; StagePilot shows Playback's key in its own window. A key
whose text StagePilot does not know (`Eb`, `F#`, `Bb`, `Am`, `C♯ minor` are known) leaves the song's own key alone and the
preview says so.

**What it never does.** It never deletes, renames or overwrites a file or a playlist item; it never edits a file in `Songs`
(only ProPresenter's own import adds one there); it touches only the Playback library folder, the Songs library (through
ProPresenter's import) and **one** named playlist. It does not delete playlists (ProPresenter has no such call). When the
playlist write is refused (a `404` makes ProPresenter drop the whole write), nothing in the playlist changed and the answer
says so and names the backup.

**Safety conditions** (checked again when the change is applied): the ProPresenter Link is Ready; no song is playing in
StagePilot (Playback says playing, or StagePilot's countdown runs), checked before anything starts and again before every song
and before the playlist is written (`409 song_playing`) and, before any screen step, nothing is shown on screen (`live_on_screen`, above); ProPresenter's API answers (its version from `GET /version` is part of
the answer); the preview is still true (`409 plan_changed` when ProPresenter, the playlist or Playback's arrangement changed
since); every song that cannot be added was left out by name (`skip`); only one job runs at a time (`409 busy`). Before the
playlist is touched it is saved as JSON in `propresenter-playlist-backups/<playlist>-<time>.json` beside the settings file; the
newest 20 are kept.

**Routes** (`/api/v1/propresenter-link/`, behind the same dashboard PIN, origin check and remote-access rules as every other
write route; a remote viewer cannot use them; ids are numbers or plain text, never a path; unknown fields are refused; errors are
`{detail, code}`):

- `GET playlists` → `{playlists: [{id, name, folder?, headers: [...]}]}`, folders flattened, kept for 10 seconds.
- `POST add-songs/preview {setlist_song_ids: [...] | content_ids: [...], playlist_id, after_header: "<name>" | place: "end"}` → the plan:
  `plan_id`, `playlist {name, place, insert_at, before, after, untouched, blocked?}`, `songs [{ref_id, title, position, action, file, source,
  import_as, reason, choices, notes, mapping [{section, group, how, line?}], guesses [{section, position, uses, group_name, group_index, groups, rule, text}],
  already_in_playlist}]` (`how` is `full`, `only`, `identical`, `guessed` or `missing`; `rule` is `in_order`, `last_reused` or
  `repeat_in_turn`), `to_add`, `stuck`, `can_apply`,
  `propresenter`, `warnings`. `action` is `reuse`, `copy`, `import_then_copy`, `needs_choice` or `blocked`. Nothing is written.
- `POST add-songs/apply {plan_id, skip: [ref_id, ...]}` → `202 {job_id}`. Refusals come at once, before anything is touched:
  `404 unknown_plan`, `409 plan_changed | songs_blocked | song_playing | busy | nothing_to_add | link_not_ready | place_blocked`.
- `GET add-songs/jobs/<id>` → `{state: running | done | failed, steps, songs [{ref_id, title, steps, result}], events, report}`; the
  report has `outcome` (`done`, `done_with_differences`, `needs_library_open`, `stopped`, `song_playing`, `playlist_refused`,
  `playlist_failed`, `backup_failed`, `not_reachable`, `nothing_to_do`, `live_on_screen`), `message`, `version`, `backup`,
  `differences` and one result per song (with its `guesses`, also for a song that was imported first).

To try it without ProPresenter: `tools/fake_propresenter_api.py --folder <the fake helper's folder> --lazy select` is
ProPresenter's API reading the same folder as `tools/fake_propresenter_link.py` (a new file gets its id only after the library
was selected, like the real lazy loading; a song item reads back the arrangement its file has selected; a wrong name or id makes
the whole write `404`).

## The screens, and a one-page guide for the operator

### The ProPresenter Live Link block (Configuration, Presentation)

Under the timer and Look fields (which are not touched by it) sits a block built like the Playback Live Link block: a title, two
pills (`ProPresenter: Connected`, `ProPresenter Link: Connected`) and one polite line that says both in words, a dot and words, never
colour alone.

| The line says | It means | What to do |
|---|---|---|
| `ProPresenter: Connected to ProPresenter at <host>` | the connection the timer and Looks use (the existing status) | nothing |
| `ProPresenter Link: Ready, on this computer` | ProPresenter runs on the computer StagePilot runs on | nothing, there is nothing to set up |
| `ProPresenter Link: Ready, version 0.1.1` | the helper on the other ProPresenter computer answers | nothing |
| `Can add songs, cannot import missing songs (macOS Accessibility not allowed)` | the Link works, but cannot drive ProPresenter's own Search to load a missing song | the sentence under the lines says how: allow it in System Settings > Privacy & Security > Accessibility |
| `Update available, version 0.0.9` | the helper works and is older than the one StagePilot carries | StagePilot updates it by itself when nothing is playing; if it says it cannot, press **Update the ProPresenter Link** and run the command |
| `Needs the command again` | the helper does not know this StagePilot's key | press **Set up the ProPresenter computer** and run the command again |
| `Not answering` | the other computer is off, asleep or not on the network | wake it; if it was set up again from another StagePilot, run the command here again |
| `Not installed` | there is no Link (grey, never a warning) | nothing, unless you want Add to ProPresenter |

When ProPresenter is on **this** computer the block has no setup button and says so. For **another** computer the button **Set up the
ProPresenter computer** opens a box: **press it** to get one command (nothing is made until you press), paste it in Terminal on the
ProPresenter computer, and wait: the box shows the command, **Copy**, how long it still works (30 minutes, this network only), **Cancel**,
and closes by itself with `The ProPresenter Link is ready.` when the helper answers. The command never holds the key. A computer without
the Link shows no warning anywhere, and the Link never changes the header's Ready / Check system verdict.

### Add to ProPresenter (Configuration, Playback, under the songs Playback plays)

The button is drawn **only when all three hold**: the ProPresenter Link is **Ready** (this computer, or the helper), the Playback Library
Link answers with fresh data, and the loaded plan is paired with a Playback setlist. Otherwise nothing is drawn. It opens one window:

1. **Songs**: the paired setlist songs, all ticked, each with Playback's **key** (a chip, `Key Ab`) and **arrangement name**. A song Playback
   gave no number cannot be added and says so. Under the list: *ProPresenter does not show the key for these songs; it is written into the
   copy and shown here.*
2. **Playlist** and **Place** (`After "Worship"` for each header of that playlist, or `At the end`). The last choice is kept in this
   browser only.
3. **Preview** (a button; nothing is written): one line per song with a stamp (`Already there`, `New copy`, `Load first`, `Needs a choice`,
   `Blocked`), what will happen in a sentence, how well Playback's sections matched the song's groups (guessed ones are listed, never blocking),
   *This imports songs, arrangements and keys into <playlist>. The playlist's contents are replaced with what the preview shows.* (also beside
   the buttons, before and after the preview) and *Nothing in ProPresenter changes until you press Add songs.* and *Adds 4 songs after Worship in Sunday Morning Service. Other items stay as they are.* On a wide screen the preview
   is beside the choice; on a phone it is below, and the window's buttons stay at the bottom edge.
4. **Add songs**: off, with its reason in words beside the buttons, until the preview is current (changing a song, the playlist or the place
   makes it stale), nothing is blocked (or **Skip blocked songs** is ticked), nothing plays (a song is playing, or the countdown runs), and the
   Link is ready. While it runs, each song and each step shows a word and a mark (`Done`, `Working`, `Failed`), the newest thing said is
   announced politely, and the window cannot be closed. The end is a result: what was added, anything ProPresenter shows differently from the
   plan, the name of the playlist backup and, under a song whose sections were guessed, a short *Check in ProPresenter:* note (information
   only). When ProPresenter is showing slides, media, announcements, props or messages the job stops before touching the screen:
   *Stopped: ProPresenter is showing something on screen*, with **Try again** once it is clear.

### One page

**Set up (once).** *Same computer:* nothing. *Another computer:* Configuration > Presentation > save the host and port > in **ProPresenter
Live Link** press **Set up the ProPresenter computer**, paste the command in Terminal on that computer, wait for `Ready`. For songs missing
from ProPresenter, allow ProPresenter Link (or StagePilot, when it is the same Mac) in System Settings > Privacy & Security > Accessibility.
In ProPresenter, Settings > Network must have the API on.

**Add the songs.** Playback panel > **Add to ProPresenter** > tick the songs > choose the playlist and the place > **Preview** > read it >
**Add songs**. It imports songs, arrangements and keys into the playlist, and the playlist's contents are replaced with what the preview
shows. Do it before the service, not while a song plays and not while ProPresenter shows slides.

**"Check in ProPresenter".** When a section of Playback's arrangement fits more than one group of a song (a repeated Interlude, several blank
Count Off groups), StagePilot picks one and goes on, then lists the pick under the song. Open the song in ProPresenter and change that
section by hand if the pick is wrong.

**"ProPresenter is showing something on screen. Clear it or wait, then press Try again."** StagePilot does not click or type in ProPresenter
while slides, media, announcements, props or messages are on screen. Clear the output (a camera input does not count), then press **Try again**.

**"Open the Playback library in ProPresenter once, then press Try again."** ProPresenter loads a new song file only when it looks at its
library. In ProPresenter, click the library **Playback** in the left list once, then press **Try again** (it makes a new preview, then press
**Add songs**). Nothing was added to the playlist, and the copies already made are reused.

**What the Link never does.** It never edits, renames or deletes a song or a playlist item; it never edits a file in `Songs`; it never
deletes a playlist; it never touches more than the `Playback` library folder, ProPresenter's own import into `Songs`, and **one** named
playlist (saved as a backup first, read back after); it never changes anything until you pressed Add songs after a preview; never while a
song plays; never clicks or types in ProPresenter while it shows something on screen; it never shows or sends the helper's key; it never quits or restarts ProPresenter; and ProPresenter itself does not show the key
Playback plays a song in.

To look at all of this without ProPresenter, see `docs/preview-live-link.md` ("The ProPresenter Link").

## Timer sequence

For `song.started` and `song.restarted`, StagePilot performs:

1. Stop
2. Set countdown duration
3. Reset
4. Start

`timer.stop_requested` stops the real ProPresenter timer while retaining its
configured duration. **Reset Position** performs a full reset sequence:

1. Stop
2. Set countdown duration to `0:00`
3. Reset

The dashboard records the start immediately before the ProPresenter start
request and advances the displayed remaining time on the first fractional
second. This keeps StagePilot's main countdown aligned with ProPresenter rather
than displaying the previous whole second for nearly one extra second.

## Recovery

StagePilot periodically probes ProPresenter. If ProPresenter is offline at startup or restarts later, the plugin retries with capped exponential backoff. Timer identities are rediscovered by visible name, and failed timer operations clear the cached timer before one immediate retry.

## macOS diagnostic log

Packaged macOS builds save backend diagnostics here:

`~/Library/Logs/org.stagepilot.desktop/stagepilot-backend.log`

In Finder, choose **Go → Go to Folder**, paste that path, and press Return. Fully quit and reopen StagePilot before reproducing a problem so the latest session appears at the end of the file. Review the file before sharing it; it should not contain saved secrets, but service and timer names may appear.

The dashboard distinguishes:

- API unreachable
- API connected but configured timer missing
- Timer found and ready
- Timer command failure
