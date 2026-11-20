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
- API port (the port of ProPresenter's network API; saved as `port`)
- Timer name
- Request timeout (kept and sent back unchanged: the Presentation panel does not show or edit it; the backend still checks it, and `STAGEPILOT_PROPRESENTER_TIMEOUT_SECONDS` overrides it)
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

(In the app this is called the **Presenter Link**.) The Presenter Link is how StagePilot reaches the **files and the screen of the ProPresenter computer**: it reads the
song files (to find which MultiTracks song each one is), adds new files to a library named `Playback`, and, on a Mac, drives
ProPresenter's own Search window to import a song. What it is used for is [Adding Playback songs to a playlist](#adding-playback-songs-to-a-playlist)
below. It is optional: with no Presenter Link StagePilot works as before.

**Which connection (no switch, the saved ProPresenter computer decides).** The link is used while the ProPresenter output is
in use and a ProPresenter host is saved (`propresenter.host`). If that host is this computer (`127.0.0.1`, `localhost`, `::1`,
or one of this computer's own addresses) the link is **Local**: StagePilot reads and writes the ProPresenter folder itself, and
there is nothing to install, no key and no setup. Any other address is **Helper**: a small signed program on the
ProPresenter computer (the Presenter Link, port 8790) does it. While no ProPresenter host is saved nothing is asked, ever.

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

## Adding songs to a playlist

StagePilot can put the songs of the service into one ProPresenter playlist, at a section you choose. The button is **Import songs to ProPresenter**
in the **Presenter Link** section of the **ProPresenter (Presentation) panel**; it is **not** in the Playback panel any more. The list
has two sources, and it is ONE list:

- **Playback has given the songs** (the Playback Library Link answers with fresh data and the loaded plan is paired with a Playback setlist):
  the songs in Playback's setlist order, each **with the key Playback plays it in, its mtID and the order Playback plays**. This needs
  the Playback Library Link (for the songs, their sections, their keys and their MultiTracks numbers), the Presenter Link (for the files;
  **Ready**) and ProPresenter's own network setting (its API, port 1025).
- **No Playback data yet:** the upcoming plan's **Planning Center song names**; each card has a small **Planning Center** chip (the
  section says nothing more about where the list comes from). These need only the Presenter Link. Each name is looked up **by its title in ProPresenter's Songs
  library** and added to the playlist **as it is**: no copy, no import, no arrangement and no key change. A name ProPresenter does not
  have is marked **Not in ProPresenter** (a grey dashed card), cannot be selected, and says that Playback data is needed to import it.

The list changes by itself from one source to the other when Playback gives the songs or loses them; a song you turned off stays off.
Nothing happens unless a person presses the one button; it makes the preview and, when nothing in the preview is blocked, imports the songs.

**What pressing Import songs to ProPresenter does.** (The section itself says as little as it can: only what you must act on. This is the
explanation it leaves out.) It imports the songs, their arrangements and their keys into the chosen playlist: nothing in ProPresenter
changes until you press it. The playlist is written whole, so what is in it afterwards is what the preview showed; that is expected and the
section does not warn about it. Planning Center names go in **as they are** in ProPresenter: no copy, no new arrangement, no key change.

**What happens, song by song.**

1. The song is found in ProPresenter by its **MultiTracks number** (Playback's `content_id`, which is also the number inside the
   song file ProPresenter has), never by its title. (The one exception is a Planning Center name with no Playback data: it is a request
   by `titles`, and each title must be the name of exactly one file of the Songs library, compared like a file name, ignoring case,
   dots and spacing. That song is `as_is`: it goes into the playlist as it is, with the arrangement its own file has selected; it is
   never imported, copied or edited, and when the Songs library lacks it or has two of that name the song is blocked or needs a choice.)
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
   never blocks, needs no tick and does not stop the button; if it is wrong the person fixes the arrangement in
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

**Safety conditions** (checked again when the change is applied): the Presenter Link is Ready; no song is playing in
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
- `POST add-songs/titles {titles: [...]}` → `{titles: [{title, state}]}` with `state` `found`, `missing` or `several`: which of those titles the
  Songs library has (file names only; nothing is written, the screen is not touched). The Presenter Link section uses it to mark a Planning
  Center name that ProPresenter does not have.
- `POST add-songs/preview {setlist_song_ids: [...] | content_ids: [...] | titles: [...], playlist_id, after_header: "<name>" | place: "end"}` → the plan
  (a `titles` request needs no Playback data; its songs are called `t1`, `t2`... in the order sent):
  `plan_id`, `playlist {name, place, insert_at, before, after, untouched, blocked?}`, `songs [{ref_id, title, position, action, file, source,
  import_as, reason, choices, notes, mapping [{section, group, how, line?}], guesses [{section, position, uses, group_name, group_index, groups, rule, text}],
  already_in_playlist}]` (`how` is `full`, `only`, `identical`, `guessed` or `missing`; `rule` is `in_order`, `last_reused` or
  `repeat_in_turn`), `to_add`, `stuck`, `can_apply`,
  `propresenter`, `warnings`. `action` is `reuse`, `copy`, `import_then_copy`, `as_is`, `needs_choice` or `blocked`. Nothing is written.
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

The block is the **first block under the header** (and under the panel's alert lines), where the Playback and Lighting panels put their
Live Link blocks, in the same spacing. It is built like them: a title, two pills (`ProPresenter API: Connected`,
`Presenter Link: Connected`) and one polite line that says both in words, a dot and words, never colour alone. It holds the
**connection**, like the other two blocks do: under the two status lines, **Host** and **API** (the port of ProPresenter's network API); **Host** is the address box every Live Link block has (type any address, or open its arrow for the computers the last search found, under **Found**, and the ones that worked before, under **Recent**) and the
same two buttons every Live Link block ends its row with, **Search** and **Connect**; under those, the Presenter Link's setup box when there is one. What follows the block, in this order, is not the
connection: **Timer** and **Look** side by side on one line (stacked on a phone), **Refresh timers and Looks**, the note that Refresh uses the
saved settings, the **Presenter Link** section (below; only while the Presenter Link is Ready), the Save bar, and the summary. A changed Host or API is marked `Not saved yet`
(in words, with the warning border) and is saved by **Save configuration**, or by **Connect**, which saves the connection (Host and API only) and connects the way Save does;
with ProPresenter saved and nothing held in those two boxes, **Connect** only checks the saved connection again and the status line says `Checked again.` The two names are exactly **ProPresenter API** and **Presenter Link**; they are written once in the code
(`proPresenterLinkState.ts`). macOS still lists the little app that runs the Presenter Link as "ProPresenter Link" in System Settings.

**Timer and Look.** Each box lists only the real choices and marks the current one, so there is no `Current Look` option and no
placeholder choice. **Timer** lists the timers ProPresenter has by name; the saved one reads `<name> (current)` and is selected (a
timer that is not a countdown keeps `(converted when cued)`); with none saved nothing is marked and the box shows the hint `Choose a
timer`; a saved timer that is no longer found reads `<name> (unavailable)`. **Look** lists ProPresenter's Looks by name; the Look
ProPresenter shows now reads `<name> (current)` and is selected by default, and choosing it means *do not change the Look*: Save keeps
`look_id` empty, exactly what `Current Look` did. Any other Look saves its id and is applied by Save as before; a saved Look that is gone
reads `<id> (unavailable)`. With no Looks known (ProPresenter not connected) the box is dimmed and its hint says `Connect to
ProPresenter first`. Choosing a Look changes nothing in ProPresenter until you save. The request timeout is not on the screen: the saved
value stays in `settings.json` and is saved back as it was.

| The line says | It means | What to do |
|---|---|---|
| `ProPresenter API: Connected to ProPresenter at <host>` | the connection the timer and Looks use (the existing status) | nothing |
| `Presenter Link: Ready, on this computer` | ProPresenter runs on the computer StagePilot runs on | nothing, there is nothing to set up |
| `Presenter Link: Ready, version 0.1.1` | the helper on the other ProPresenter computer answers | nothing |
| `Can add songs, cannot import missing songs (macOS Accessibility not allowed)` | the Link works, but cannot drive ProPresenter's own Search to load a missing song | the sentence under the lines says how: allow it in System Settings > Privacy & Security > Accessibility |
| `Update available, version 0.0.9` | the helper works and is older than the one StagePilot carries | StagePilot updates it by itself when nothing is playing; if it says it cannot, press **Update the Presenter Link** and run the command |
| `Needs the command again` | the helper does not know this StagePilot's key | press **Set up the ProPresenter computer** and run the command again |
| `Not answering` | the other computer is off, asleep or not on the network | wake it; if it was set up again from another StagePilot, run the command here again |
| `Not installed` | there is no Link (grey, never a warning) | nothing, unless you want Import songs to ProPresenter |

When ProPresenter is on **this** computer the block has no setup button and says so. For **another** computer the button **Set up the
ProPresenter computer** opens a box: **press it** to get one command (nothing is made until you press), paste it in Terminal on the
ProPresenter computer, and wait: the box shows the command, **Copy**, how long it still works (30 minutes, this network only), **Cancel**,
and closes by itself with `The Presenter Link is ready.` when the helper answers. The command never holds the key. A computer without
the Link shows no warning anywhere, and the Link never changes the header's Ready / Check system verdict.

### Presenter Link (Configuration, Presentation, after the settings and before the Save bar)

The section is drawn **only while the Presenter Link is Ready** (this computer, or the helper). Otherwise nothing is drawn and nothing is
said: the feature is optional. It is built from the section templates (a settings group for the options, the notices, the action row) and
has no window; everything, including progress and the result, happens in place, under the button.

1. **Playlist** and **Section**, side by side. **Section** lists the playlist's section (header) names in the playlist's order and nothing
   else: **End of Playlist** is offered **only when the playlist has no section header at all** (then it is the one option). A section is
   every item after its header up to the next header; the songs go at the **end** of the chosen section, right before the next header (for
   the last section, at the end of the playlist, so the last section is the same place as the end). The default is the section that was
   last **used for an import** in that playlist (choosing one is not enough), while the playlist still has it; else the **last section**.
   A remembered *End of Playlist* in a playlist that now has sections falls back to its last section. The last playlist and these
   sections are kept in this browser only. When a playlist has two headers with the same name both are listed; choosing that name (it is
   the default when it is the last section) shows ProPresenter Link's own line under the box (*More than one header in this playlist is
   named "Worship". Pick another place.*) and the button stays off: StagePilot does not guess.
2. **Songs**: the same song cards as the Lighting tab's songs (an auto-fill grid, the cover when the Library Link has one, the title, a
   small line of facts, the thin arrangement bar), with no header row over them (the count, `3 of 4 selected`, is only read out politely by a screen reader; the list is named `Songs to import`). Every card starts selected: a selected card has a solid border, a tinted fill and the outline, a card that is off a dashed, dimmed border (there is no mark in the corner, so the colored bar runs the whole width of the card, as in Lighting); Space or Enter turns it on or off.
   With Playback data a card shows, as plain text and nothing to open: one line `C` and `mtID-23126` as two square chips (the key as letters, `Ab (was G)` for a
   key changed in Playback in the last day, then **mtID-** and the number, no spaces), and under it the **order Playback plays**, section by section
   (`Intro · Verse 1 · Chorus · Verse 2 · Chorus · Bridge · Chorus`, wrapping onto as many lines as it needs), **under** the thin bar (title, facts, bar, then the order). The
   **mtID** is the MultiTracks **Song Master ID** (the same number ProPresenter shows as MTID and the one to type in Playback's search and
   in ProPresenter's Search; the helper's `content_id`), never Playback's own song number (the long `setlist_song_id`). A song with no id
   shows `mtID—` (nothing follows); a song Playback gave no number at all is a grey card that says `Can't import`. Without Playback data a card is the
   Planning Center name and a **Planning Center** chip. After a preview, the cards it stopped for carry its chip (`Can't import`,
   `Pick one`...) and a song with a guessed section a **Check** chip.
3. Above the button, only when it is off or the preview stopped: why, in a few words.
4. **Import songs to ProPresenter**: ONE button (`Importing…` while it works). It makes the preview and, when nothing is blocked and no song is playing, imports the songs at once.
   If a song is blocked or needs a choice, the playlist side is blocked, there is nothing to add, or a song is playing, it **stops after the
   preview**. The preview is short: the place (`Sunday Morning Service › Worship`), ONE line of counts (`1 to add · 2 in playlist · 1
   can't import · 1 pick one`; zero counts are left out, red only when something blocks the whole import) and one row per song: the title,
   ONE status chip (an icon and a word) and at most one short phrase (the table below). The long sentences are the chip's hover text and
   what a screen reader hears. **Skip those and import the rest** is offered when something can still be imported without the blocked
   songs; it never imports half silently. Changing a selection, the playlist or the section after a preview marks it as old (*Out of
   date: press Import again*).
5. While it runs, each step is a chip with one word (`Backup`, `Library`, `Importing`, `Copy`, `Loading`, `Playlist`, `Check`) and a
   spinner, a check or a cross; the newest thing said is announced politely, and the button and the options are off. The end is a result
   under the button: `Done: 3 added`, `3 added, 1 differs` or `Stopped: Can't import "Battle Belongs"`, a chip per song, anything
   ProPresenter shows differently as short lines (`Item 7: duration 80 → 0`), under a song whose sections were guessed a `Check` chip with
   the guess in a few words (information only), the name of the playlist backup as a small line and, small, ProPresenter's version. When
   ProPresenter is showing slides, media, announcements, props or messages the job stops before touching the screen (`Live on screen:
   clear it, then try again`), with **Try again** once it is clear. **Try again** makes a fresh preview and adds it at once when it is clean.

#### The status words (preview, cards and result)

One chip per song, one set of words, in the status colours (green done, teal news, red only for what stops a song, grey left out). The
word is the status: colour never carries it alone.

| Chip | Where | Phrase beside it | Means |
| --- | --- | --- | --- |
| `✓ In playlist` | preview, result | none | the song is already an item of the playlist: it is not added again |
| `+ Ready` | preview | `Copy exists` or `Used as is` | a copy that plays this arrangement exists (or a Planning Center name goes in as it is), and it will be added |
| `+ New copy` | preview | `Playback order, “(2)”` | a new file `<Song> (2)` is made with Playback's order |
| `↓ Will import` | preview | none | ProPresenter does not have the song; it is imported through ProPresenter's Search, then copied |
| `! Can't import` | preview, cards, result | `Needs a Mac`, `Rental ended`, `No arrangement`, `File unreadable`... | the song cannot be done; the full reason is the hover text |
| `? Pick one` | preview, cards | the file names, one per line | more than one file in ProPresenter is this song; StagePilot does not guess |
| `! Can't map` | preview, result | `Missing: Bridge 2` | these sections have no group of that name in the song |
| `! No mtID` | preview | none | Playback gave no MultiTracks number for the song |
| `? Check` | preview, cards, result | `Interlude (3rd use): 2nd group` | a section fit more than one group and one was picked: look at it in ProPresenter (never blocks) |
| `− Skipped` | preview, result | none | left out on purpose (**Skip those and import the rest**) |
| `✓ Added` | result | none | the song is in the playlist now |
| `! Not loaded` | result | none | ProPresenter has not loaded the new copy yet |

Whole-job lines at the top: `Song playing: wait` (nothing is imported while a song plays) and, after the job, `Live on screen: clear it,
then try again`. The cards say `Not in ProPresenter` (with `Needs Playback data`) for a Planning Center name ProPresenter does not have.

### Search: finding ProPresenter on the network

**Search** in the block looks for ProPresenter's API (Search finds a computer; the word Scan is kept for reading the songs, the Playback
panel's **Scan Playback API** / **Scan Playback Library**). StagePilot asks every private address this computer is on (the networks it is attached to, never wider than the
attached /24, plus the /24 of the saved Host when that is a private address), and this computer, ONE thing: `GET /version`, with a short time limit, 32 addresses
at a time. It tries the saved API port first, then 1025. It sends nothing else, writes nothing, never asks a public address and saves nothing. While it looks the
button reads **Searching… (cancel)** and a line says how far it is (`Searching the network for ProPresenter… — Checked 64 of 255 addresses (6s)`). When ProPresenter runs on this computer, that is the answer and the search stops there.

- **One computer answered:** Host and API are filled in (a change not saved yet), a line says which computer, and **Connect** saves and connects.
- **Several:** a list (`Found 2 ProPresenter computers. Choose one:`, the same one the Playback search uses): choosing one fills Host and API.
- **Nobody:** a short "Make sure" list stays under the row.
- **macOS blocks the local network:** the red notice with the steps (System Settings > Privacy & Security > Local Network), **Open Local Network settings** and **Copy diagnostic details**.

The routes (the same PIN, origin and remote-access rules as every other write; a remote viewer cannot start a search): `POST /api/v1/propresenter/scan` (an empty body or `{}`),
`POST /api/v1/propresenter/scan/cancel` and `GET /api/v1/propresenter/scan` answer `{state: idle|scanning|found|not_found, candidates: [{host, port, name, version}], reason,
current, total, phase, error_class: permission_denied|no_route|null, elapsed, networks, hosts_probed, hosts_total, details, settings_url}`. `name` and `version` are what
ProPresenter's own answer says (`name`, `host_description`).

### One page

**Set up (once).** *Same computer:* nothing. *Another computer:* Configuration > Presentation > in **ProPresenter
Live Link** type the host and the API port, **Save configuration** (or press **Connect**), then press **Set up the ProPresenter computer**, paste the command in Terminal on that computer, wait for `Ready`. For songs missing
from ProPresenter, allow "ProPresenter Link" (the name macOS shows for the Presenter Link; or StagePilot, when it is the same Mac) in System Settings > Privacy & Security > Accessibility.
In ProPresenter, Settings > Network must have the API on.

**Import the songs.** Configuration > Presentation > **Presenter Link** > choose the playlist and the section (the last one is the default; **End of Playlist** only for a playlist with no section) >
leave the songs you want selected (they are Playback's when Playback has given them, else the plan's Planning Center names) > **Import
songs to ProPresenter**. It imports songs, arrangements and keys into the playlist, at the end of the chosen section (names already in
ProPresenter go in as they are), and the playlist's contents are replaced with what the preview shows. If something is blocked it stops
and tells you; read it, then **Skip those and import the rest** or fix it and press the button again. Do it
before the service, not while a song plays and not while ProPresenter shows slides.

**"Check in ProPresenter".** When a section of Playback's arrangement fits more than one group of a song (a repeated Interlude, several blank
Count Off groups), StagePilot picks one and goes on, then lists the pick under the song. Open the song in ProPresenter and change that
section by hand if the pick is wrong.

**"Live on screen: clear it, then try again."** (ProPresenter's own sentence: *ProPresenter is showing something on screen. Clear it or wait, then press Try again.*) StagePilot does not click or type in ProPresenter
while slides, media, announcements, props or messages are on screen. Clear the output (a camera input does not count), then press **Try again**.

**"Open the Playback library in ProPresenter once, then press Try again."** ProPresenter loads a new song file only when it looks at its
library. In ProPresenter, click the library **Playback** in the left list once, then press **Try again** (it makes a new preview and imports the songs). Nothing was added to the
playlist, and the copies already made are reused.

**What the Link never does.** It never edits, renames or deletes a song or a playlist item; it never edits a file in `Songs`; it never
deletes a playlist; it never touches more than the `Playback` library folder, ProPresenter's own import into `Songs`, and **one** named
playlist (saved as a backup first, read back after); it never changes anything until you pressed Import songs to ProPresenter; never while a
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
