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
StagePilot's key: run the setup command again), `not_answering`, `checking` or `waiting`. It is read when a ProPresenter host is saved and
at most every 30 seconds while a window asks (the Check button waits 2 seconds, 10 after a refusal); a helper that goes quiet
keeps its last good answer for a minute, marked `stale`. The state never changes the header's Ready / Check system verdict.

**Waiting for macOS (this computer only).** The first look at ProPresenter's folder (`~/Documents`) and the Accessibility check can
make macOS ask the person for permission, and macOS holds the read until they answer. So the look on this computer has a hard
limit of 8 seconds: when it is over, the status answers `state: "waiting"` with the message *macOS may be asking for permission
on this computer. Answer the prompt, then press Connect again.* (`import_blocker` is left out, `can_import` is `false`, no libraries
are listed) and never keeps the request or the status lock waiting. It is not remembered as asked: the next call looks again, and
joins the look that is still waiting for the person instead of starting another. The panel shows the pill `Presenter Link: Waiting
for macOS`. Another computer's helper (and the bundled helper's own checks) are not changed by this.

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
   when more than one qualifies StagePilot first applies **one rule, no guessing**: if the song's title (Playback's title when
   Playback has data, else the plan title; compared in lower case, trimmed, with single spaces) is exactly the file name without
   `.pro` of EXACTLY ONE of them, that file is used, and the preview says so (*Used "Firm Foundation (Live)": its name matches the
   Playback title.*; `chosen_by` is `title`). *Firm Foundation (Live).pro* and *Firm Foundation.pro* for the song "Firm Foundation"
   gives *Firm Foundation.pro*; with the song "Firm Foundation (Live)" it gives the other. Two or no names equal to the title (for
   example *Firm Foundation (Live).pro* and *Firm Foundation (Studio).pro*) leave the song `needs_choice` (*Two files are this song.
   Pick one.*), and the person **picks the file on the card** (below). A picked file is used when it is one of the song's files; if
   it cannot make the arrangement (a section has no group of that name) the song is `blocked` for THAT file in the usual words and
   no other file is used instead, so the person sees why and picks the other.
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

**A song paired by hand.** A plan song whose name differs from Playback's can be paired with the Playback song by a person (the Song order section of the Playback configuration, \"Pairing a song of another name by hand\" in `docs/playback-api.md`). Such a pair is an exact pair for everything above: the song is found in ProPresenter by the **Playback song's** MultiTracks number and its sections and key are Playback's, and the Auto-Presenter follows it like any other song (it finds the copy by that number too). Only the name differs: the Playback title still decides the title rule in step 4 (it is the name the file in ProPresenter carries), but a copy StagePilot **makes** for it, and with it the playlist entry, is named after the **plan song** (`wanted.plan_name`: `Our Name For It (1)` rather than `Battle Belongs (1)`), and the song's row in the preview shows the plan song's name. A copy that already plays what Playback plays is used as it is and keeps the name it has.

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
When that `404` happens StagePilot asks `GET /v1/presentation/<uuid>` for each copy of the run that was in the write (made or
reused, not a song that goes in as it is). ProPresenter lists a fresh Playback file before it has opened it and answers `404` for
it; such a copy is named in the answer as `not_opened` (its row shows `! Not opened`, the same warning look as `! Not loaded`) and
the message becomes one sentence: *ProPresenter has "Alpha (1)" in its Playback library but has not opened it yet, so it cannot go
in the playlist from here. In ProPresenter click the Playback library, right-click "Alpha (1)", choose Add to > "Sunday", then
press Try again.* (several copies are all listed). The whole write stays refused, so a song that is fine is not added either, and
only one write is tried. Try again skips a copy that is already an item of the playlist. What makes ProPresenter open a copy
without a person is not known; nothing is clicked for you.

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
- `POST add-songs/preview {setlist_song_ids: [...] | content_ids: [...] | titles: [...], playlist_id, after_header: "<name>" | place: "end", sources?: {"<ref_id>": "<file>"}}` → the plan
  (a `titles` request needs no Playback data; its songs are called `t1`, `t2`... in the order sent):
  `plan_id`, `playlist {name, place, insert_at, before, after, untouched, blocked?}`, `songs [{ref_id, title, position, action, file, source,
  import_as, reason, choices, chosen_by?, chosen_file?, notes, mapping [{section, group, how, line?}], guesses [{section, position, uses, group_name, group_index, groups, rule, text}],
  already_in_playlist}]` (`how` is `full`, `only`, `identical`, `guessed` or `missing`; `rule` is `in_order`, `last_reused` or
  `repeat_in_turn`), `to_add`, `stuck`, `can_apply`,
  `propresenter`, `warnings`. `action` is `reuse`, `copy`, `import_then_copy`, `as_is`, `needs_choice` or `blocked`. Nothing is written.
  **`sources`** is the file the person picked for a song that has more than one: the song's `ref_id` to the file written exactly as
  `choices` lists it (`"Songs / Firm Foundation.pro"`, the library, ` / `, the file name). It is lenient: a key that is no song of the
  request, a value that is not text, too long or has a control character, or a file that is none of THAT song's files is ignored (the
  song is asked about as before) and is never an error. `choices` lists the files that could be the song: for `needs_choice` the ones
  to pick from, and for a song settled by a pick or by the title rule the same list, with `chosen_by` (`pick` or `title`) and
  `chosen_file` naming the one used. The picks are part of the plan's identity: another pick is another `plan_id`, and `apply` (which
  still takes only `plan_id` and `skip`) does what the preview with those picks said.
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
connection: **Timer** and **Look** side by side on one line (stacked on a phone), with **Refresh timers and Looks** and the note that Refresh uses the
saved settings right under the Timer box (in its column, never under the taller Look box), the **Presenter Link** section (below; only while the Presenter Link is Ready), the Save bar, and the summary. A changed Host or API is marked `Not saved yet`
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
| `Waiting for macOS` | on this computer, macOS did not let StagePilot read ProPresenter's folder or check the Accessibility permission within 8 seconds, most likely because it is asking you for permission (a window such as "StagePilot would like to access files in your Documents folder", or "wants to control System Events") | answer the window, then press **Connect** again; nothing else is needed, StagePilot looks again at once |
| `Not answering` | the other computer is off, asleep or not on the network | wake it; if it was set up again from another StagePilot, run the command here again |
| `Not installed` | there is no Link (grey, never a warning) | nothing, unless you want Import songs to ProPresenter |

When ProPresenter is on **this** computer the block has no setup button and says so. For **another** computer the button **Set up the
ProPresenter computer** opens a box: **press it** to get one command (nothing is made until you press), paste it in Terminal on the
ProPresenter computer, and wait: the box shows the command, **Copy**, how long it still works (30 minutes, this network only), **Cancel**,
and closes by itself with `The Presenter Link is ready.` when the helper answers. The command never holds the key. A computer without
the Link shows no warning anywhere, and the Link never changes the header's Ready / Check system verdict.

### Presenter Link (Configuration, Presentation, after the settings and before the Save bar)

The section is **always drawn**. While the Presenter Link is not Ready (no ProPresenter connection, or no Link on this computer or the
helper) it is greyed, nothing inside it can be pressed (the Import button reads "Presenter Link not ready") and nothing is asked of
ProPresenter; it unlocks by itself when the Link is Ready. It is built from the section templates (a settings group for the options, the notices, the action row) and
has no window; everything, including progress and the result, happens in place, under the button.

The section **starts folded**, and its header says whether the songs can be imported (`4 songs ready to import`, `3 of 4 songs can be imported`, `No songs can be imported`, `Not while a song plays`, `No songs in the plan`); a preview, an import, its result or a problem opens it by itself, and it stays open until you fold it.

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
   `Pick one`...) and a song with a guessed section a **Check** chip. A `Pick one` card has, **under the title, one small button per
   file** (the file name without `.pro`; the library too only when two libraries have a file of that name): a radio group named *Firm
   Foundation: use file*, one tab stop, arrow keys move between the buttons and Space or Enter picks (44 px tall, wrapping on a phone).
   Pressing one asks for a new preview with `sources`; the pressed button shows the loading sweep, nothing else moves, and the card turns
   into its real status (`New copy`, `Ready`...) with a quiet phrase, *Using “Firm Foundation (Live)”*, and a small **Change** button that
   brings the buttons back (a song settled by the title rule says the same, and its hover text says the name matches the Playback title).
   **The pick is remembered in this browser** (`localStorage`, key `stagepilot.presenter-song-file.<mtID>`, the value the file as written in
   `choices`), so the next preview sends it without asking again; a song with no mtID is not remembered (its pick lasts while the section is
   open). A remembered file that is no longer one of the song's files is dropped without a word and the card asks again. Nothing is saved in
   StagePilot's settings. A song that still needs a pick stops Import like any song that cannot be imported (the count line says `1 pick
   one`); pick a file, or leave it out with **Skip those and import the rest**.
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
| `+ New copy` | preview, cards | `Playback order, “(2)”`, or `Using “Firm Foundation (Live)”` when the file was picked or chosen by its name | a new file `<Song> (2)` is made with Playback's order |
| `↓ Will import` | preview | none | ProPresenter does not have the song; it is imported through ProPresenter's Search, then copied |
| `! Can't import` | preview, cards, result | `Needs a Mac`, `Rental ended`, `No arrangement`, `File unreadable`... | the song cannot be done; the full reason is the hover text |
| `? Pick one` | preview, cards | the file names, one per line (the card has the buttons) | more than one file in ProPresenter is this song and no file name is its title; pick one on the card |
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

## Following Playback

StagePilot can show a song's lyrics in ProPresenter while Playback plays the song: when Playback reaches a verse, a chorus or a line,
StagePilot triggers the matching slide in ProPresenter over its HTTP API. **It is off until the switch is turned on.** With the switch
off, with no ProPresenter output or Presenter Link, with no Library Link or for a song with nothing to follow, StagePilot never
triggers anything in ProPresenter, never reads it for this and behaves exactly as before. It never sends MIDI to ProPresenter and
never writes anything in Playback. The feature is called the **Auto-Presenter**.

### What is followed

Only a song that StagePilot put into the playlist itself (or finds there unchanged): its song file in the `Playback` library carries
the `Playback` arrangement that Presenter Link wrote, one entry per Playback section, repeats separate, and that arrangement is the
selected one. A song that was edited since, or whose arrangement is something else, is not followed and the screen says why.

### Where the timing comes from

1. **Playback's lyric cues** (best). In Playback choose, for the song, Edit, the "..." on the song, MIDI OUT, Lyrics Bus, "ProPresenter 1 Line
   Template" (or the 2 or 4 Line one that matches the lines per slide of the song file), then Update. Playback then holds one cue for each
   lyric slide. A cue says the second it fires and the slide's number in the song file. Cues come a little before the lyric.
2. **Bars** (when a song has no usable cues). The first slide of a section is shown when the section starts and the section's other text
   slides are spread evenly over it. Blank and background-only slides are shown with the slide after them.

Cues are used only when they fit the file: at least three, every cue falls in a section, and the slide numbers fit the file's slides
(next section). Otherwise StagePilot says which test failed (for example "Cues do not match the file (cues go to slide 30, file has 54
cues)") and follows by bars.

ProPresenter's slide numbers count the slides of the SELECTED arrangement, so a chorus that is played twice has its slides twice.
StagePilot turns a cue's number (which counts the slides of the file once each) into ProPresenter's number from the song file, taking the
play of the group nearest the section Playback is in. That is why repeated sections and groups with the same name stay right.

### Extra slides in the song file

Playback numbers the slides of the song as it was when the Presenter Link wrote it. Someone may add blank or background slides in
ProPresenter afterwards: 1 or 2 at the start of the presentation, inside a group, between groups or at the end. The Auto-Presenter still
shows the right lyric slide.

- **Nothing added** (the highest slide number plus one is the file's slide count): the slide number is the file's slide index, exactly as
  before.
- **Up to 8 more slides in the file than Playback numbers**: the numbers are matched to the file group by group. A cue near the start of
  the next section belongs to the section before it only when its slide number is the one after the cue before and that section's group
  still has room (if that is not clear, the way that puts the most cues on slides with text wins). Within a group, the slides Playback
  counts are taken to be the slides that have text: a blank slide is the likely extra one. The slide shown is therefore the lyric slide
  Playback meant, whatever slides were added around it.
- **Check.** Such a song is still followed by cues but says `Following by Playback cues, aligned with extra slides (check the first slide of
  <section>)`. Look at that section's first slide the first time: the section named is where a slide inside a group had to be guessed, else
  the first section that has a cue.
- **More than 8 more slides, or numbers no way of adding slides explains** (a slide group longer than its file group, the same slide
  in two groups) say `Cues do not match the file (cues go to slide N, file has M cues)`, name the Playback template that fits the file,
  and the song is followed by bars. A 2 or 4 Line template against a 1 Line file is no longer one of these: see the next part.

The usual lines of text per slide that has text (the commonest count of line breaks plus one; 1, 2 or 4 are Playback's templates) is read
from the file. It is what the advice names: `No cues: in Playback choose the ProPresenter 2 Line Template for this song`, and the same
template after a mismatch. A file whose slides have another number of lines says `a ProPresenter Line Template that matches the file`.

### Lines per slide

Playback's Lyrics Bus template counts 1, 2 or 4 lines per slide. The song file has as many lines on a slide as it was made with, and people
change them. StagePilot counts the lines of text on every slide of the file (a line break or a new paragraph starts a line; a blank or
background-only slide has none) and works out Playback's lines per slide from the cues themselves: for each template it checks how many of
the song's groups have exactly the number of slides that template makes of that many lines, and the template that fits clearly the most
groups is Playback's. When no template stands out the song is treated as before (numbers against the file's slides).

- **Same number of lines (the normal case).** Nothing changes: the rules above apply.
- **Different numbers** (for example Playback's 2 Line Template against a file of one-line slides, or a 4 Line Template against one-line
  slides): the song is no longer refused. Playback's slide 3 of a group shows lines 5 and 6 of that group, so StagePilot shows the slides of
  the file that hold those lines. When the file's slides are smaller than Playback's it shows each of them in turn, evenly spaced from
  Playback's cue to the next cue (or the end of the section); when they are bigger it shows the slide that holds the first line and does
  not show it again. Blank slides have no lines, so slides added to the file do not matter here either. A slide with 3 lines among
  2-line slides is followed the same way, by its lines.
- **Check.** Such a song says `Aligned by lines (Playback 2 per slide, the file 1 per slide)` and is still followed by cues, never blocked.
  The tip under it says that matching the template in Playback is still best (`choose the ProPresenter 1 Line Template`).
- **Refused only when it really does not fit**: no template can be told from the cues, or a group's slides fit no template (even with
  one slide more than its lines make). The song is then followed by bars and says `Cues do not match the file (...)`.

The song row shows both numbers, the file's `lines_per_slide` and Playback's `playback_lines` (null while there are no cues or they do not
say), in the status below.

### What StagePilot does while the song plays

- **A song starts, and its first slide appears as it starts.** Playback's own Play command is the earliest sign there is (the same
  one the countdown timer and the lights act on); StagePilot does not wait for the next heartbeat, which can be a second late. Within
  one 100 ms step of that command the playlist item is shown (`GET /v1/playlist/{playlist}/{item}/trigger`, the song's first slide
  with the background that usually belongs to the song), the active presentation is read (`GET /v1/presentation/active`) to be sure it
  is the song's own, and the wanted slide is triggered (`GET /v1/presentation/active/{n}/trigger`). A song started in the middle (a seek
  and then Play) shows the item and then the wanted slide at once. The item is shown once per start: not again when the same song is
  paused and resumed or after a seek, again when the song is started again from the top, and for the next song of the service. A song
  that is only selected or loaded and stays stopped shows nothing; neither does a song that cannot be followed, a switch that is off
  or a link that is down. (Without Playback's fast transport setting, or when Playback moves on to the next song by itself, the first
  heartbeat is the first sign, as before.)
- **The wanted slide comes from the position, never from a heartbeat.** About ten times a second the position is compared with the
  song's schedule: the last slide due at the position plus the offset. The position is the last thing Playback told us (a heartbeat,
  or a Play or seek command that came after it) taken at the moment it was received, moved on by the time since while the song plays.
  So a slide never waits for the next heartbeat, and a seek on Playback (which carries its new position) is followed in the next step.
  A section jump or loop carries no position: it is followed by the next heartbeat (at most about a second). A heartbeat that was
  already on its way when a command came (it still shows the old play state or place, and arrives within 1.5 s of the command) is
  ignored; the next one is believed. The only unknown is how long a heartbeat was on its way (a few milliseconds on a LAN); the rig
  measured Playback's clock against the wall clock at rate 1.0001 with a residual of 7 ms, so there is no drift to correct. Seeks and
  section loops are therefore followed by themselves, unlike the countdown timer, which ignores loops.
- **Pause, stop and the end of the song** trigger nothing: the slide stays. On resume the wanted slide is shown if it is another one.
- **The right song must be on screen.** If another presentation is active before a slide is triggered, the playlist item is shown again,
  at most once every 2 seconds.
- **A click in ProPresenter holds the song.** After every trigger StagePilot remembers the slide it expects and reads ProPresenter's current
  slide about four times a second (`GET /v1/presentation/slide_index`). When it is another slide or another presentation more than 0.6
  seconds after StagePilot's own trigger, a person clicked: the status says "Paused: someone changed the slide in ProPresenter" and nothing
  more is triggered for that song until its schedule moves into another section (a seek or loop that stays in the same section does not end
  it), when the song's slide for that moment is shown again. A new song, or turning the switch off and on, also ends it.
- **Failures.** A trigger that fails is tried again no sooner than a second later. The first failure and then one a minute are written to the
  log. After five failed triggers in a row following is paused ("Paused: ProPresenter did not take 5 slide changes in a row ...") until the
  next song or the **Try again** button (`POST /api/v1/propresenter-follow/try-again`). It never retries in a tight loop.
- **The numbers for the rig.** Every trigger is written down (the last 20 are in the status as `recent_triggers`): how late it left
  against the moment it was due, how long ProPresenter took to answer, and how long after it the status poll first saw the slide on
  screen. Together they split the delay between a slide's cue and the slide on the output into StagePilot, the beat of Playback's cues and
  ProPresenter itself. Each also goes into the log as one `presenter_follow_trigger` line at DEBUG level (never in the default log).
  While a trigger waits to be seen, ProPresenter's slide is read every 100 ms instead of every 250 ms.
- **The cost of following is small.** The 100 ms step and the reader never copy the settings or the service plan: what they need (the
  switch, the offset, ProPresenter's address) is copied once and again only after a save, and the plan is read where it lies. A
  `settings.json` of 49 MB (every song's lighting data) no longer costs a core while the switch is on.
- **Nothing here waits for anything else.** Following runs in two tasks of its own (one that follows every 100 ms and one that reads
  ProPresenter's playlist and song files), so a slow ProPresenter can delay a slide but never the countdown timer, the lights or the screen.

### Settings

`propresenter.presenter_follow` in `settings.json` (an older file without it reads as off; it is left out of the saved file while it holds the
defaults, so an older StagePilot never meets it):

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `false` | The master switch. |
| `offset_ms` | `0` | From -2000 to 2000. Positive shows every slide earlier. Tune it on the rig: Playback's cues come about half a second to a second before the lyric. |

It is saved with the other settings (`PUT /api/v1/settings`), behind the same PIN and origin rules as every other write route, so a remote
viewer cannot change it. A save that leaves the key out keeps what was set. The binding of each song (below) is saved beside the settings
file as `presenter-follow-bindings.json` (ids, file names and slide counts; nothing secret, no words of a song).

### Where a song lives in ProPresenter (the binding)

A plan song is tied, by its MultiTracks number, to a playlist, the item in it, its presentation, the song file in the `Playback` library and a
fingerprint of its arrangement: the ordered ids of the groups the `Playback` arrangement plays (and `Playback` being the selected
arrangement). Slides added to or taken out of a group, or a group the arrangement does not play, do not change it, and the slide map is
read from the file as it is now every time. A binding saved by an older StagePilot (whose fingerprint counted every group's slides) is
rebuilt from the file the first time the song is read, without a word on screen.

1. **Remembered** when the Presenter Link adds the song to a playlist (or finds it there already).
2. **Found on demand** in ProPresenter's ACTIVE playlist (the one last triggered): the item whose file in the `Playback` library carries that
   MultiTracks number and has the `Playback` arrangement selected. The files are read through the Presenter Link's host, directly or through
   the helper.

The fingerprint must equal what the file holds now. Otherwise (another order, another length, another group in the arrangement) the song
is "Arrangement was changed in ProPresenter" and is not followed.
The songs are worked out again when the plan, the ProPresenter settings or a binding change, when the switch is turned on, and every 15
seconds while no song plays (never while one plays).

### The status

`GET /api/v1/propresenter-follow/status` answers, whatever the switch says:

```
{enabled, offset_ms, state, message, song: {key, title} | null, section: {index, name} | null, slide, hold_reason, can_try_again,
 songs: [{key, title, following: "cues" | "bars" | null, code, text, advice, lines_per_slide, playback_lines, template}],
 recent_triggers: [{seq, song, kind: "item" | "slide", why: "play" | "cue" | "catch_up", slide, ok, late_ms, answer_ms, seen_ms}]}
```

`state` is `off`, `idle` (nothing to follow), `ready` (the song is loaded and not playing, or was shown and Playback is stopped or paused:
`Ready: waiting for Playback to play the song`; the summary never says `Following` then), `following` (the song plays), `held` (a click holds
it), `paused` (failures) or `not_followed`.

`recent_triggers` holds the last 20 triggers, oldest first, numbers in milliseconds with one decimal. `late_ms` is how late the trigger left
against its moment on Playback's clock: for a slide the position (with the offset) minus the time of its cue when the request was sent, for
the item the time since StagePilot learned that the song plays (`null` for the slide before the first cue). `why` is `play` (the item),
`cue` (the position reached the cue) or `catch_up` (the position jumped: a seek, a loop, a song started in the middle; `late_ms` then says
how far past the cue it was). `answer_ms` is how long ProPresenter took to answer the trigger; `seen_ms` how long after the trigger the status
poll first saw that slide on screen (`null` until it did, or when it never did within 5 seconds, or when the next trigger came first). `slide` is ProPresenter's number of the slide StagePilot last showed. It is **not** part of `/api/v1/health`: no
state of following changes the header's Ready or Check-system verdict.

`lines_per_slide` is the usual number of text lines of a slide in the song's file and `template` the Playback Lyrics Bus template that
fits it (`ProPresenter 1 Line Template`, 2 or 4), both `null` until the file has been read or when it has no text slide.
`playback_lines` is the lines per slide of the template Playback used (1, 2 or 4), read from the song's cues; `null` when the song has no
cues or they do not say.

Each plan song says, in the words of the screen:

| `code` | `text` |
|---|---|
| `cues` | Following by Playback cues |
| `cues_aligned` | Following by Playback cues, aligned with extra slides (check the first slide of Intro) |
| `cues_by_lines` | Aligned by lines (Playback 2 per slide, the file 1 per slide) (followed by cues; `advice`: Matching the template is still best: in Playback choose a ProPresenter 1 Line Template for this song) |
| `bars` | Following by bars (no Playback cues) (`advice`: No cues: in Playback choose a ProPresenter 1 Line Template for this song; the 2 or 4 Line Template when the file's slides have 2 or 4 lines) |
| `cues_do_not_match` | Cues do not match the file (cues go to slide N, file has M cues) (followed by bars; `advice`: In Playback choose the ProPresenter 2 Line Template for this song) |
| `bars_cues_unusable` | Following by bars (cues not used: a cue after the song end / a cue is in no section / ...) |
| `arrangement_changed` | Arrangement was changed in ProPresenter |
| `not_in_playlist` | Not in the ProPresenter playlist |
| `link_not_ready` | ProPresenter Link not ready |
| `switch_off` | Switch is off |
| `not_in_playback` | Not in Playback's setlist |
| `sections_changed` | The song's sections changed in Playback since it was added to ProPresenter |
| `no_sections` | Playback has not given this song's sections yet |
| `library_not_ready` | Playback's Library Link not ready |

### The window

Configuration → Presentation holds the **Auto-Presenter** as a card of its own, under the **Presenter Link** card and the **VerseVision** card, just above the Save bar, and in the same style (a rail and a soft wash in the Presentation orange, the title in bold with one line under it, a chevron, a rule under the header). The card is always drawn, even when you cannot use it yet.

- **The switch** sits on the card's header row, not inside the card. Off until you turn it on; it is saved with the other settings (`propresenter.presenter_follow.enabled`) and turned off the same way. The window reads the switch before it writes, so what you set elsewhere is never put back to an older value.
- **One summary line** under the title, always a mark and words, never a colour alone: `Off`; `Following: Chorus, slide 3` (the section Playback is in, and ProPresenter's slide counted the way ProPresenter's window counts, from 1: its Count Off is slide 1); `Ready: ...` while a song is loaded and not playing; `Paused: someone changed the slide in ProPresenter` (a click in ProPresenter holds the song until the next section; there is nothing to press); `Paused: ProPresenter did not take 5 slide changes in a row` (the whole sentence, with the reason, is inside the card); `Not following: ...` with the reason when the song that plays cannot be followed; `Needs the Playback Library Link` while it is locked (below). It is the first thing a screen reader hears too.
- **Fold.** The card opens and closes like the Presenter Link and remembers what you chose on this computer. With the switch off it starts folded. It opens by itself when it needs you (paused by failures, a song that cannot be followed, a switch that is on but locked) until you close it; a click in ProPresenter, which only holds one song, does not open it.
- **Locked, not hidden.** The Auto-Presenter can be turned on only when **both** links work: the **Presenter Link** is ready, and Playback's **Library Link** is connected and answers with current data (not an old copy). Otherwise the switch cannot be pressed, everything inside is greyed and ONE reason says which link to set up and where: `Needs the Playback Library Link` (Configuration → Playback, Library Link setup), `Needs the Presenter Link` (Configuration → Presentation, Presenter Link setup), `Needs current data from the Playback Library Link` (the Library Link answers but has not lately), or both named in one line. A switch that was saved as on and then locked says `Waiting for the Playback Library Link` (or the Presenter Link): nothing on the card says it is following, no song row claims it, and the switch can still be turned off. It follows again by itself when the link is back.
- **The songs.** One row for every plan song that has a Playback song behind it (matched by name or paired by hand), with its cover and title and one chip from `GET /api/v1/propresenter-follow/status` (`songs[].text`, the table above), each with a mark (`✓` followed by cues, `~` by bars, `!` something to look at, `−` nothing to follow now). While the card is locked the rows are only the cover and title, greyed, with no chip: the one reason box above them says which link is missing. A song followed by bars because Playback holds no cues has ONE chip, `~ No cues: following by bars` (the info look of the fallback chips, never red; the status's `Following by bars (no Playback cues)` is its hover text), and one sentence that names the template its song file needs: *In Playback: Edit, the "..." on the song, MIDI OUT, Lyrics Bus, ProPresenter 2 Line Template, Update.* (the 1, 2 or 4 Line Template by the lines per slide the song file has, as the status's `advice` and `template` say; a song followed by lines (Playback and the file count lines per slide differently) has the chip `! Aligned by lines (Playback 2 per slide, the file 1 per slide)` and the same sentence as the tip; a file with another number says to pick the Line Template that matches the file). A song whose cues do not fit the file says `Cues do not match the file (cues go to slide N, file has M cues)`, `Followed by bars` and the same sentence with the template that fits. A song followed by cues with extra slides in its file says `Following by Playback cues, aligned with extra slides (check the first slide of Intro)`. The song cards of the Presenter Link itself carry none of this. Plan songs with no Playback song (not in Playback's setlist, or not paired yet) get no row: one quiet sentence under the list names them (`Not followed: Oh My Soul, Give Thanks. They are not in Playback's setlist, or not paired with a Playback song yet. Pair a song by hand in the Song order section of Configuration → Playback.`).
- **Try again** appears only when following is paused by failures (`POST /api/v1/propresenter-follow/try-again`). It is the only button of the card.
- **Timing** (folded). One small number box, **Offset (ms)**, from -2000 to 2000, saved when you leave the box or press Enter (`propresenter.presenter_follow.offset_ms`). Positive shows every slide earlier. Leave it at 0 until it has been tuned on the rig.

While the Presentation panel is open the window reads the status every 2 seconds with the switch on, and every 30 seconds with it off; with the panel closed it reads nothing. The switch and the offset are changed only by someone allowed to change the configuration.

### What has not been tried on the real rig

Everything above was tested against fakes (a fake ProPresenter that numbers slides like the real one, and the recorded numbers of one real
song). Still to measure with a real ProPresenter and Playback: how far Playback's cues come before the lyric (the offset to set), the 2
second lead used to place a cue in a section, ProPresenter's own answers to `/v1/presentation/active` and `/v1/presentation/slide_index`
while a song is shown, how long a trigger takes, that `/trigger` really follows the selected arrangement for a repeated group in a Presenter
Link copy, and how it behaves with a real operator clicking. Bars are spread evenly: snapping them to the song's bar lines needs the beat grid,
which the plan does not carry yet.

## VerseVision

The **VerseVision** button under the settings of the Presentation panel opens VerseVision, which builds verse slides in ProPresenter from a pastor's notes, on the same address and behind the same PIN as the dashboard. It uses this connection (host and port) and works on the computer StagePilot runs on. See [versevision.md](versevision.md).

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
