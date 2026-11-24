# Changelog

## [Unreleased]

### Added

- **The control plane can fetch Bible text for VerseVision with the operator's keys, so VerseVision works without anyone holding a key.** A StagePilot installation asks `GET|POST /v1/installations/:id/bible/passage` (version + normalized reference) with its own installation credential and gets back the verses (`book_id`, `chapter`, `verse`, `text`), the copyright and the provider. The three provider keys (`API_BIBLE_API_KEY`, `YOUVERSION_API_KEY`, `ESV_API_KEY`) are Worker secrets only: they never reach an installation, a log or an error, and a version whose key is not set answers `503 provider_not_configured`. Each installation gets 600 passages an hour and 5,000 a day, the service stops at 20,000 uncached passages a day (all three adjustable with variables), a request is at most 40 verses, and only the listed versions and a parsed reference ever reach a provider. Non-ESV text is cached for 30 days; the ESV is never cached or stored. Nothing is deployed by this change: see "Bible text proxy" in `docs/remote-control-plane.md` for the route, the limits and the deploy steps.
- **Clean up glitches... repairs songs that are already saved.** Songs captured before 2.44.2 have brief flashes baked into their rows (a value goes back to an older value for a frame or two and returns), so a replay jerks even though it is faithful. The new button in the Lighting timeline toolbar (a song with captured rows) asks first how many flashes it found and on how many lights, removes them from the saved rows, and says what it did. The song keeps its looks and its loops (loops are never changed), rows you edited or made by hand are not touched, and **Undo clean-up** puts the old song back. Nothing runs by itself.

### Fixed

- **The Lighting computer's helper timestamps every frame when it arrives, so recordings and the live view are no longer smeared.** The helper (now 0.6.1; StagePilot updates it by itself when nothing is playing) used to stamp a Lightkey frame only after it had read it and while it decoded the one before, so about half of a busy song's events arrived bunched a few milliseconds apart and tens of milliseconds late. One thread now only reads and stamps, another decodes in order. `/health` also says `dmx_dropped`.

### Changed

- **A chase that repeats mostly cleanly is now saved as one loop plus small changes,** instead of thousands of rows. A cycle that is brighter, a pulse that is dropped once, a light that joins late or a hit between two cycles no longer stops the loop: the loop plays, and a small change on top (a "patch") corrects the few seconds that differ. A loop is looked for where 60 % of the points repeat (it was 90 %) and kept when the loop and its changes cost at most 60 % of the plain ramps. The Lighting timeline says how many changes a loop has (`4.0 s loop x 16 fixtures (+12 changes)`) and the loop's row lists them. Songs saved before this change are unchanged, and a loop with no changes is saved exactly as before. A StagePilot from before this change plays such a song's loops without their changes.

## [2.44.2] - 2026-11-23

StagePilot 2.44.2 smooths Lightkey replays, saves chases as loops, and tidies the Lighting timeline.

### Fixed

- **Busy songs no longer replay jagged in Lightkey.** The replay used to send at most 200 lighting messages a second, one packet at a time, so a busy song got only 5 to 7 updates for a fade that should have 20 or more and its lights moved in steps. The limit is now 2,500 messages a second (Lightkey was measured clean up to 4,900 on the rig), ramps and loops are looked at 40 times a second instead of 20, and everything one tick has due leaves at once in bundles of up to 16 messages. On the three Sunday songs a fading light now gets an update about every 25 ms where it got one every 55 to 135 ms before. Looks and the first look of a song also leave at once.
- **The colored bar on the Lighting song cards runs the whole width of the card.** The × now sits in the card's top right corner and the bar runs underneath it.
- **The cue lines on the Lighting timeline are thinner** (2 px, where they were 6), so the waveform behind them stays readable. They are as easy to press and drag as before.

### Changed

- **A song with a Playback waveform no longer uses the room's loudness.** The waveform is drawn behind the cues on the Lighting timeline instead. Recording that song does not open the microphone and saves no loudness, and the song has no loudness chip, no Clear loudness and no microphone prompt. Loudness already saved is hidden, not deleted. A song without a Playback waveform works as before.
- **A chase on a busy song is now saved as a loop.** The capture is cleaned before it is saved, because the rig delivers its changes with smeared timing that kept a chase from ever looking the same twice. A 7-minute song with 16 fixtures chasing a 4-second pulse used to come to 42,993 rows and no loop; it is now one loop and a few dozen rows. The raw recording on disk is unchanged.
- **The Lighting timeline always shows the whole song.** It is as wide as its box however short or long the song is: no sideways scrolling, and the zoom buttons are gone. Cues closer than 14 px still collapse into a numbered cluster; clicking a cluster picks its first cue.
- **The timeline height starts on Auto, and − and + set it by hand.** Auto fits the height to the cues showing. − and + change it by 40 px (160 to 800 px) and show the number; S, M or Auto go back to a preset. The height you chose before is not kept, so every device starts on Auto once.
- **On the Mac, StagePilot keeps its saved secrets in one keychain item instead of several,** so an update asks for keychain access fewer times. The first update to this version may still ask several times; after that it should ask about twice. Your saved logins are kept.

## [2.44.1] - 2026-11-22

StagePilot 2.44.1 is a small update to 2.44.0 that fixes four things you could see and tidies two pages.

### Fixed

- **Album covers show in the Mac and Windows app.** The app was blocking them, which left an empty gap before each title. A song without a picture now shows a small placeholder so the titles line up.
- **The Playback waveform shows for more songs.** About one song in nine, such as Been So Good, was stored under a different content number and never showed its waveform. The Library Link is now version 0.4.1, and StagePilot updates it by itself when nothing is playing. A song that really has no waveform no longer says "The Playback computer did not answer."
- **The colored section bar spans the whole song card** in the Presenter Link and Lighting lists on the Mac app.
- **The Lighting timeline header counts only the songs of the service on screen,** so it can no longer say "6 of 4 songs".

### Changed

- **The Presenter Link section folds, starts folded,** and its header says whether the songs can be imported.
- **On the Presentation page the Refresh button sits right under the Timer box.**

## [2.44.0] - 2026-11-20

StagePilot 2.44.0 is the Link release. StagePilot can now put the songs of the service plan into a ProPresenter playlist with Playback's arrangement and key, and every connection (Lighting, Playback, Presentation) looks and works the same. The candidates rc.1 to rc.3 below hold the step-by-step history.

### Added

- **Presenter Link: songs from the plan into a ProPresenter playlist.** In the Presentation panel, pick a playlist and a section, tick the songs and press **Import songs to ProPresenter**. Each song gets a copy in the Playback library that follows the order Playback plays, with the key saved in it. A song ProPresenter does not have yet is imported through ProPresenter's own Search. A song with more than one file lets you pick the right file on its card. Nothing changes in ProPresenter until you press the button; the playlist is saved first and read back after, and the screen steps stop while ProPresenter is showing something.
- **The Library Link and the Playback Live Link block.** The Playback panel starts with its own connection block with two status pills, and a one-line command to set up the Library Link on the Playback computer. The Library Link and the Lighting Link update themselves when StagePilot starts. StagePilot connects to the Library Link by itself, takes the song order and lengths from it first, and the **Scan Playback Library** or **Scan Playback API** button names the best scan there is.
- **Playback library: key, BPM, length and arrangement of each plan song,** shown as cards with the cover and a colour bar of the arrangement. A key shows "(was X)" for one day after it changes in Playback.
- **One address box in every Live Link block.** Type any address, or open the arrow for recent and found ones. **Search** finds computers on the network and **Connect** connects.
- **Pick another service than the upcoming one** with arrows or a calendar, and a **Current Upcoming** button.
- **Lighting status:** two pills, **sACN** and **DMX** (for example "DMX: 66 fixtures identified").

### Changed

- **Every configuration page is built from the same section templates,** so Lighting, Playback and Presentation look alike: the connection first, then settings, actions, notices.
- **Song facts are small square chips** (key, length, BPM, mtID) in every panel; song cards look the same everywhere.
- **Presentation: Timer and Look on one line,** each showing what is current. The Timeout box is gone (the saved value is kept).
- **Plainer words everywhere:** "Lighting Link" instead of "helper", "Not connected" for every unconnected state, short statuses, every switched-off button says why, a reason is shown as a small pill, and the event stream uses plain names.
- **The app and the web page are about 7 MB smaller and load faster on phones.**

### Fixed

- **Presenter Link no longer says "Checking…" for ever while macOS asks for permission.** It says "Waiting for macOS", and the next try works once you answer.
- **The log no longer fills up when Playback is off the network.** A failing connection is logged once, then once a minute.
- **A Lighting computer command asked for while the Lighting Link already works no longer stops working within seconds.**
- **Switching to another service no longer keeps the highlighted song of the old one,** and "Match the Playback lengths to Planning Center" reads the service you picked.

### Removed

- "Forget the Library Link" is gone.

## [2.44.0-rc.3] - 2026-11-19

### Added

- **Presenter Link: pick the song's file on its card.** When ProPresenter has the same MultiTracks song in two files (*Firm Foundation (Live)* and *Firm Foundation*), the song used to say **Pick one** and the only way on was to delete a file in ProPresenter. Now the card has one small button per file under the title: press one and the card turns into **New copy** or **Ready** with *Using “Firm Foundation (Live)”* and a **Change** button. StagePilot remembers the pick for that song (in this browser, by its mtID), so the next time it does not ask. Before asking at all it looks at the names: when exactly one file is named like the song's title, that file is used and the card says so. Nothing in ProPresenter changes until you press Import songs to ProPresenter, as before.

- **Presenter Link: the reason next to Import songs to ProPresenter is a small pill.** *Choose a playlist* and *Select a song* used to be a line of bold text above the button that read like a heading. It is now a small grey pill with an icon beside the button (under it on a phone), never red; the words are also the button's hover text and screen-reader description.
- **Song order: no key or BPM under the songs.** The rows of Playback's Song order section show the titles, the order, each song's length and the match or *Check order* stamp. The key and BPM chips are gone from that section only (song cards everywhere keep them).

- **Presentation: Timer and Look on one line, no Timeout box; Presenter Link songs without a header row.** The two boxes are now just **Timer** and **Look** (they were *Detected timer* and *ProPresenter Look*), side by side (stacked on a phone). Each lists the real choices and marks the current one with `(current)`: the saved timer, and the Look ProPresenter shows now, which is also what the Look box starts on, so there is no `Current Look` option any more (saving with it selected still leaves the Look alone, as before). A saved timer or Look that is gone reads `(unavailable)`; with no timer saved the box says `Choose a timer`; with ProPresenter not connected the Look box is dimmed and says `Connect to ProPresenter first`. The **Timeout (seconds)** box is gone from the screen; the saved timeout is kept and saved back unchanged. In the Presenter Link section the row with *Songs*, *N of M selected*, *Select all* and *Select none* is gone; the cards work as before and a screen reader still hears the count.

- **Plainer words everywhere (design audit follow-ups).** The Lightkey helper is called **Lighting Link** wherever you see it (Lighting Link setup, Update the Lighting Link). "Backend" is gone from what you read: the startup screen, crash messages and updater say **StagePilot** for the app and **the engine** for the part that runs on this computer (only the fifth dashboard card keeps its title, Backend). **Not connected** is the one word for that state. Dates read `Thu 19 Nov` and times `2:31 PM` everywhere; Services says `Signed in`, `Plan date` and `Last loaded 2:31 PM`, and the button is `Planning Center`. The Event stream reads `Playback connected`, `Presentation connected`, `Service plan loaded`, `Timer stopped` (the raw name is the hover text, and the status-change rows are not shown). The Playback status line starts with `Playback API:`. The song order shows each fact once (`Order from a scan (4 songs)`, a grey `Learning order (0 of 4)` chip, no red sentence); the explanations are in `docs/playback-api.md`. Short text trims: one empty line on the Lighting timeline, the MIDI velocity rule as a column header, `No notes yet.`, shorter set-up boxes.
- **Every switched-off button says why.** Hover or focus a dimmed button (Test connection, Load service types, Load today's service plan, the service arrows, MIDI Connect and Disconnect, Undo last capture, Clear song, Clear all songs, Zoom out, Restart current) and a sentence tells you what to do first; a screen reader reads the same sentence. The unused cue-type chips in the Lighting legend are labels now, not buttons.
- **Header check list and Save bar.** A row that blocks a service and is not ready is red with a ×; an optional one (the Playback library) is grey with a !. On a phone the Save bar is the solid panel colour with a heavier top line, so cards scrolling under it do not show through.
- **Presenter Link song cards:** the section names sit under the coloured bar, there is no check mark in the corner (a selected card has a solid border, tint and outline; an unselected one a dashed, dimmed border) so the bar runs the whole width like Lighting's, and the chip reads `mtID-18195` (`mtID—` with no number).

- **Song facts are small square chips in every panel.** The key, length, BPM, `mtID` and capture words under a song's title used to be one line with dots between them (`Ab · 4:37`, `G · mtID 18191`, `Not captured · 65 BPM`). Each fact is now its own small square chip, the look the Presenter Link's key had before, with a small gap and no dot between. It is the same in the Playback config's songs, the Presenter Link's cards, Lighting's songs and the song matchup (where the key is the bare letter, as in the other panels). The words are unchanged; on a narrow phone the chips go onto a second row only when they must. Sentences and counts that just have a dot between clauses (`1 to add · 2 in playlist`, the Live Link status line) are unchanged.

- **Lighting Live Link: Search and Connect look like Playback's and Presentation's, and two status pills.** Lighting's Search and Connect used to be dark buttons with a coloured edge; they are now the same tinted buttons as in the Playback and Presentation blocks (only the colour is Lighting's). The single pill *Connected · 66 fixtures* is now two: **sACN: Connected** (the link to Lightkey; also *Connecting…*, *Not connected*, *Off*, *Lightkey is closed*) and **DMX: 66 fixtures identified** (*1 fixture identified*, *Connecting…* while it waits, *Not receiving* when the fixture data stops, and a grey *—* while sACN is not connected). Both pills are always shown; the reason for a problem is still the hover text.

- **Presenter Link: short statuses, plain song cards, a Section box without End of Playlist, and the Playback address box captioned Host.** The preview, the progress and the result used to say a lot in full sentences; now each song is ONE chip (an icon and a word, all lined up) and at most one short phrase: **In playlist**, **Ready** (*Copy exists*), **New copy** (*Playback order, “(2)”*), **Will import**, **Can't import** (*Needs a Mac*), **Pick one** (the file names, one per line), **Can't map** (*Missing: Bridge 2*), **No mtID**, **Check** (*Interlude (3rd use): 2nd group*). One neutral line of counts (*1 to add · 2 in playlist · 1 can't import · 1 pick one*) replaces the red sentence, the heading is the place (*Sunday Morning Service › Worship*), and the repeated sentences about the key and about a copy that already plays the arrangement are gone (the key is on the card; the full reasons are the chip's hover text and what a screen reader reads). Steps are one word each (*Backup, Library, Importing, Copy, Loading, Playlist, Check*); the result says *Done: 3 added*, *3 added, 1 differs* or *Stopped: Can't import “Battle Belongs”*, with the differences as short lines (*Item 7: duration 80 → 0*). The song cards show plain text: *C · mtID-23126* (the key letters, then the MultiTracks Song Master ID, never Playback's own song number; *mtID—* with none) and the order Playback plays written out on the card, always, with no Sections button; the Key chip and the arrangement name are gone. **Section** lists only the playlist's section names, and **End of Playlist** only for a playlist with no section; the default is the section last used for an import there, else the last section. In the Playback Live Link block the address box is captioned **Host**, like Lighting and ProPresenter. docs/propresenter.md has the status words in a table.

- **Playback: one song-scan button that names the best scan there is.** The Song order section has ONE button: **Scan Playback Library** when the Library Link works and its order holds (it reads the Library Link, touches nothing in Playback and works while a song plays), otherwise **Scan Playback API** (the confirmed scan that steps through the songs; the dialog is now "Scan Playback API?"). **Scan with Playback instead** is gone: when the Library Link's order fails, its plain line stays and the one button is Scan Playback API. The button is never hidden: when no scan can run it is greyed out, and hovering it (or a screen reader) says why: *Connect to the Playback API first.*, *Stop Playback to scan songs.* or *A scan is running.*

- **Live Link blocks: Search, not Scan.** In the Lighting, Playback and ProPresenter Live Link blocks the button that looks for a computer is now **Search** (**Searching… (cancel)** while it runs), next to **Connect**. The line under it says *Searching the network for Playback…* (or ProPresenter, or a Lighting computer), several answers say *Found 2 Playback computers. Choose one:*, the macOS help says to press Search, and the address list's group is **Found** (with **Recent**). The word Scan is kept for reading the songs (Scan Playback API / Library).

- **Presenter Link: Import songs to ProPresenter, song cards, and the Section box in plain section names.** The button is now **Import songs to ProPresenter** (*Importing…* while it works; the result says *Done*). The **Section** box lists the playlist's section names in order, (no more *After "..."*): the songs go at the end of the chosen section, right before the next header. The first default is the last section; after an import, that playlist offers the section you imported into first next time (choosing alone is not remembered), as long as the playlist still has it. A playlist with two headers of the same name lists both, and choosing that name shows a plain line and keeps the button off: StagePilot does not guess. The songs are the same cards as the Lighting tab's songs (cover, title, MultiTracks id, a key chip with *(was G)* when the key changed in Playback today, the arrangement and its bar, Sections for the order), each card ticked on or off with an outline and a check mark, all selected at first, with **Select all** / **Select none** and a spoken count (*3 of 4 selected*). Planning Center names are cards with a small Planning Center chip; a blocked song, a song the preview stopped for and a guessed song carry small chips on the card. The section explains less: the sentences about what the button does, that nothing changes until you press it, where the songs come from and the key note are gone from the page (docs/propresenter.md explains them).

- **Presenter Link: the Add to ProPresenter button moves from the Playback panel to a new "Presenter Link" section of the ProPresenter panel, and it adds in one press.** The section has a **Playlist** and a **Section** picker, ONE list of songs and ONE button. The list is Playback's songs (title, MultiTracks id, key, arrangement) when Playback has given them, and the plan's Planning Center names until then (it swaps by song and keeps your ticks); a name ProPresenter does not have says *Not in ProPresenter* and cannot be ticked. The button makes the preview and adds the songs at once, or stops with the reason and **Skip those and add the rest** when something is blocked; progress, the result and **Try again** appear in the section (no window). Planning Center names are added as they are from ProPresenter's Songs library, with no copy, arrangement or key change, through the new `POST /api/v1/propresenter-link/add-songs/titles` and a `titles` preview request. The Playback panel no longer has an Add control.

- **ProPresenter Link: a section that fits more than one group is guessed, and StagePilot says so.** When one of Playback's sections matches several groups of a song (which Interlude is repeated, which blank Count Off group), the song is no longer blocked: StagePilot takes the Nth group of that name in the order the song plays them, reuses the last group once when Playback uses the name once too often, and goes round and round when it uses it two or more times too often. The preview lists every guess (*Interlude (3rd use) uses the 2nd "Interlude" group: check it in ProPresenter*), and after the import each song with a guess gets a short *Check in ProPresenter:* note. A guess never blocks and needs no tick; a section no group has still blocks that song.

- **ProPresenter Link: the window says what Add songs does.** The preview and the buttons now say: *This imports songs, arrangements and keys into the playlist. The playlist's contents are replaced with what the preview shows.* (*Nothing in ProPresenter changes until you press Add songs* stays.)

- **ProPresenter Link: never touches the screen while ProPresenter is showing something.** Before importing a song through ProPresenter's Search or selecting the Playback library on screen, StagePilot asks ProPresenter what is on its outputs. If slides, media, announcements, props or messages are showing (or the answer cannot be read) nothing is typed or clicked: the job stops with *ProPresenter is showing something on screen. Clear it or wait, then press Try again.*, files already made are kept and Try again is safe. A camera input and audio are ignored.

- **ProPresenter Link: the screen script checks what it typed.** Before every key it sends it checks that ProPresenter is the front app and the right field has the keyboard; after typing the song number and the name it reads each field back and stops if it is not exactly what was typed (or if another field of the Import dialog, or the Library, changed). StagePilot also checks that the import left the playlist's item count alone. The ProPresenter Link helper is version 0.1.1.

- **ProPresenter Link: a connection to the files and the screen of the ProPresenter computer (no feature uses it yet).** StagePilot can now read the ProPresenter song files, add new files to a library named `Playback`, and (on a Mac with the Accessibility permission) drive ProPresenter's own Search window to import a song. If the saved ProPresenter computer is the computer StagePilot runs on, nothing needs installing. For another computer, **Set up the ProPresenter computer** gives one command to paste into Terminal there, exactly like the Library Link's; a small signed helper (port 8790) then answers only this StagePilot, never replaces, renames or deletes a file, and updates itself like the other helpers. `GET /api/v1/propresenter-link/status` says which connection is used and whether it is ready, not installed, can be updated, needs the command again or is not answering. A new optional setting, `propresenter.link.data_folder`, points to a ProPresenter folder that is not `~/Documents/ProPresenter`. With no ProPresenter computer saved nothing is asked, and nothing changes in ProPresenter. Details: `docs/propresenter.md`.

- **Add the songs Playback plays to a ProPresenter playlist, with Playback's arrangement and key (engine and routes; the window comes next).** `POST /api/v1/propresenter-link/add-songs/preview` says, for each chosen song, whether ProPresenter has it (found by its MultiTracks number), whether a copy that already plays the right sections and key is in the `Playback` library, or what would be made: an import through ProPresenter's own Search, then a new file `<Song> (n)` in the `Playback` library with all the song's own arrangements kept plus one named `Playback`, selected, and Playback's key written in. It also says where in the playlist the songs go and that everything else stays as it is. `POST .../add-songs/apply` does exactly that as a job, after a backup of the playlist, writes the playlist once, reads it back and lists every difference; it refuses while a song is playing, when anything changed since the preview, or when a song that cannot be added was not left out. A section that no group of the song has blocks that song; a section that fits several groups is guessed and listed (see the next entries). Nothing is ever deleted or overwritten. If ProPresenter has not loaded a new file after 30 seconds, StagePilot asks you to open the Playback library once and press Try again. Playback's setlist songs now carry their MultiTracks number (`content_id`). Details: `docs/propresenter.md`.

- **Add to ProPresenter: the window, and the ProPresenter Live Link block.** In Configuration > Presentation, a new block under the timer and Look fields (which are unchanged) shows two lines, `ProPresenter: Connected to ProPresenter at <host>` and `ProPresenter Link: Ready, on this computer` / `Ready, version 0.1.0` / `Not installed` / `Update available...` / `Needs the command again` / `Not answering` / `Can add songs, cannot import missing songs (macOS Accessibility not allowed)`. On the ProPresenter computer itself there is nothing to set up and the block says so, with the one-sentence fix when songs cannot be loaded; for another computer the button **Set up the ProPresenter computer** gives one command to paste in Terminal there (a 30-minute countdown, Copy, Cancel, it closes by itself when the helper answers). In Configuration > Playback, under the songs Playback plays, **Add to ProPresenter** appears only when the ProPresenter Link is ready, the Library Link works and the plan is paired with a Playback setlist; it opens a window to tick the songs (each with Playback's key and arrangement), choose a playlist and a place (after a header, or at the end), **Preview** exactly what would happen (nothing changes until you press **Add songs**), then add them with progress per song and a result that names the backup and anything ProPresenter shows differently. When ProPresenter has not loaded the new songs yet, it says to open the Playback library once and offers **Try again**. A computer without the Link sees nothing new and no warning. `tools/preview_propresenter_link.py` shows all of it in a browser without ProPresenter (`docs/preview-live-link.md`).

- **Every Live Link block has the same address box: type any address, or open the little arrow for the ones to choose from.** In Configuration > Lighting, Playback and Presentation the computer's address is now one ordinary text box in all three. It is always open to typing, pasting and editing: nothing has to be picked first. The small arrow inside its right edge (or the Down arrow key) opens a list with two parts: **Scanned**, the computers the last scan found (name and address, only while that scan is the latest), and **Recent**, the last five addresses that worked, newest first. An address that is in both is shown once. Choosing one only puts it in the box (and, for ProPresenter, its API port); **Connect** still does the connecting, and Enter in the box connects. Playback's *Other computer…* choice and its second box are gone, and so are the *Recent* buttons under Host. With nothing to list, the list says *No recent or scanned addresses yet.* The section gallery shows the box in all three blocks in every state.

### Changed

- **Plainer, more consistent words and colours in a few places (design audit).** The fifth status card on the dashboard is titled **StagePilot** like its panel (it said *Backend*) and says *Connecting to StagePilot…* while it starts. The StagePilot panel no longer says *backend* or *frontend* (*Timezone, logging and port changes take effect after StagePilot restarts.*; *Sends recent logs and app info to the developer. Once a minute at most.*). **Copy log** and **Update and restart** are in sentence case like every other button. Lighting's buttons that ask first end in one *…* (**Clear song…**, **Clear all songs…**, **Remove loop…**). The Presentation panel's header says only *Sends the countdown timer to Presentation.* *Remote access* is spelled one way everywhere. Two lines that are not problems are no longer red: Remote access's temporary-link note and Lighting's helper-update line (capture and replay keep working). **Release lights** says why it is off (*No lights to release.*), like **Read fixtures** beside it. On a touch screen the Lighting timeline's cue-type chips are 44 px tall like the buttons beside them.

- **Every Live Link block ends its row with the same two buttons, Scan and Connect, and the field names say what they are.** In Configuration > Lighting, Playback and Presentation the buttons that used to read Test connection, Check again, Scan Network and Scan again are now always **Scan** and **Connect**, in that order, the same size in the same place. **Scan** looks for the computer on the network and reads *Scanning… (cancel)* while it looks; **Connect** connects, and on a computer that is already connected it only checks again and the status line says so. Presentation gets a new scan for ProPresenter's API: it asks the private addresses on this network (and this computer) one thing, `GET /version`, saves nothing, and fills Host and API when one ProPresenter answers or lets you choose when several do; Connect then saves and connects. Lighting's Scan starts the search that *Search again* used to start (the search still runs by itself in the background); several computers found are chosen from the same list Playback uses. Lighting's field now reads **Host** (its full name, *Host or IP (OSC output + Live Link)*, is the box's hint and is in the Details), and Presentation's port box reads **API** (the port of ProPresenter's network API; the saved setting is still `port`).

- **Every configuration page is built from the same section templates, so one design change updates them all (no change to what they do).** Besides the Live Link section, the Settings group (fields in the 1, 2 or 4 column grid with the "Not saved yet" mark, hint, error line and disabled-while-busy), the Test / Refresh buttons row, the API / timer summary, the notice lines, the collapsible optional sections (open or closed as you left them) and the order of the whole page are now one template each. Presentation, Services, StagePilot, Playback, MIDI, Lighting and Remote access use them. Small, deliberate differences: the gaps between the parts of a page are one size (16 px) everywhere, a failure or message on Services and StagePilot now sits just above the Save bar instead of below it, and the notes under a group of fields all use the Lighting size. The section gallery now shows every page and every template in every state at 360 px and 1280 px. A lint rule now stops a configuration page from drawing its own copy of a section (it says which template to use), each page has a test that fails if its sections change order, and the gallery also shows one frame per connection side by side, the panel order, a worked example of a new connection page, and the commit it was built from. A few leftover notes and test-cue rows on the Lighting, Playback and Presentation blocks now use the same templates (text size 12 px, as before; the buttons of a test cue row sit 4 px closer together).

- **The ProPresenter Live Link block has the same layout as the Lighting and Playback ones: the address first, the setup behind a button.** In Configuration > Presentation the block now shows its title and the two status pills, then Host, Port and **Test connection** on top, then one line of status words (`ProPresenter API: ... · Presenter Link: ...`), then the button **Presenter Link setup** (**Update the Presenter Link** when it is behind and StagePilot cannot update it). What used to sit between the status and the boxes (the note that ProPresenter is on this computer, the sentence about importing songs, the update line and the setup command) is behind that button, closed, and opens by itself only when the Link needs setup or has a warning. Playback's block gets the same button, **Library Link setup**. Nothing else changes: the same boxes, buttons, words and saving.

- **Lighting, Playback and Presentation share one Live Link section and one setup box (no change to what they do).** The three blocks are built from the same section and the three "set up the other computer" boxes (Lightkey, Library Link, Presenter Link) are one box, so a design change to either is made once and shows on every page. New: a gallery of every state of the three blocks for design review (`npm run build:gallery`, `tools/preview_sections.py`); it is not part of the app.

- **The ProPresenter Live Link block is now the first block of the Presentation panel, and holds the connection, like Playback and Lighting.** In Configuration > Presentation the block sits right under the header (where Playback and Lighting put theirs) with **Host**, **Port** (and the recent addresses) and **Test connection** inside it, under the two status lines. Detected timer, Timeout, the Look, **Refresh timers and Looks**, the Save bar and the summary come after it. Nothing is saved or tested differently: a changed Host or Port still says *Not saved yet* and waits for **Save configuration**. The two statuses are now called **ProPresenter API** and **Presenter Link** (pills, lines, hover text, screen readers, and every sentence that named the helper, including the ones StagePilot sends for the window to show).

### Fixed

- **A Lighting computer command asked for while the helper already works no longer stops working within seconds.** The setup session used to end itself as soon as StagePilot saw a current helper, so the link was dead before the other computer used it. It now stays open for its 30 minutes and ends early only when the helper's version changes.

## [2.44.0-rc.2] - 2026-11-17

### Added

- **Pick another service than the upcoming one: arrows, a calendar, and a "Not the upcoming service" pill.** In the Services section, above the service type, you now see the date and title of the service StagePilot has loaded (`Sun, Nov 22, 2026 - Sunday Service`) with an "Upcoming service" tag when it is the upcoming one. The arrows step to the previous or next service (not day) among the services from today on, and the Calendar button opens a month where the days with a service are marked and the other days are dimmed; it works with the keyboard (arrow keys, Page Up and Page Down for the month, Enter, Escape) and fills the width of a phone. Picking any other service loads it exactly as the upcoming one is loaded (songs, lighting, captures, the ProPresenter timer and the Playback arrangement all follow it) and keeps it: across reloads and restarts, until you change it, until its date has passed (it ends at midnight in your Planning Center time zone and a note says "Back to the upcoming service"), or until the plan is gone from Planning Center (the note says so). While it is picked, a "Not the upcoming service: Sun, Nov 29" pill with a Back to upcoming button sits in the Service plan header and in the Services section; the pill is a neutral warning and never changes Ready or Check system. The choice is kept in its own small file (`service-pin.json`, next to the settings) and is refused while a countdown is running. In the demo (`STAGEPILOT_SERVICE_SOURCE=demo`) there are now six weekly Sunday services to try it with. New routes: `GET /api/v1/planning-center/plans/upcoming`, `POST` and `DELETE /api/v1/planning-center/plans/pin`; while a service is picked the state carries two new fields, `selection` and `upcoming` (they are left out otherwise, so nothing else in the state changes).

- **Playback Live Link: a new first block in the Playback API panel with two separate status lines, and a command that sets up the Playback computer.** The block is built like the Lighting "sACN DMX Live Link" block. It says `Playback API: Connected to Playback at 10.30.0.141` and, on its own line, `Library Link: Ready, version 0.3.0` (or Not installed, Update available, Needs the command again, Not answering, Forgotten); the pill beside the title says Connected, API only or Not connected. Scan Network, the computer chooser and Connect moved into the block unchanged, and choosing a computer now sets both the Playback API host and the Library Link's address (the script is on the same computer, port 8780). The old "Use the Playback library" switch is gone: the Library Link is asked whenever a Playback computer is saved. **Set up the Playback computer** gives one command to paste into Terminal on the Playback computer, like the Lightkey helper's: it works for 30 minutes, on this network only, needs no internet (StagePilot carries the helper, version 0.3.0) and ends itself when the script answers. StagePilot makes a key for the script, keeps it in the secure credential store (never in the settings file, never shown or logged) and signs every request to the script with it, so a script on a shared network answers only this StagePilot. **Forget the Library Link** (Advanced) deletes the key. A computer without the Library Link shows no warning anywhere and looks as before; a script that was in use and goes quiet is the one thing the header pill mentions. Previewing without Playback: `docs/preview-live-link.md`.

- **Song arrangement: the Playback library now feeds the matchup, and the block has a new retro look.** The block that lines up the service plan's songs with Playback's songs is one ledger now: the plan on the left, Playback on the right, a numbered join mark between them and a stamp on every row (Matched, Check key, Check length, Check title, Check order, Only in plan, Only in Playback, Skipped). When the Library Link answers, the Playback side shows Playback's own titles, a 28 px cover, the key Playback plays, the BPM, where the countdown ends and the coloured arrangement bar, and the sentences under a row say what differs in plain words ("Plan says key A, Playback plays Ab.", "Playback counts this song down to 7:22; the plan says 6:48."). A new **Playback setlist** line says which Playback setlist is used (the one on the loaded service's date, whatever date that is); when Playback has none for that date it says so and offers its other setlists to choose from. Titles are paired by their words, and a pair that is only alike is marked "Check title" with both titles shown; nothing is paired by guesswork. A song only the plan has, or only Playback has, is its own row. With a saved scan, each row also says which Playback song will really start its timer, and the Skip switches on the song that is only on one side line the rest up. StagePilot still never changes the saved song order from the library: only a scan does. Without the Library Link the block works as before (Playback song 1, 2, 3 from the scan), in the new look. Planning Center's key for a song is now read (and shown only here).

- **A song's key shows "(was X)" only for a day after the key was changed in Playback.** The song cards used to say `Ab (was C)` whenever the key was not the original recording's, which was true all the time and told nobody anything. Now a card shows just the key (`Ab`), and `Ab (was G)` only when StagePilot saw the key being changed in Playback within the last 24 hours, where G is the key it had before that change; the hover text says it in words ("Key changed in Playback from G to Ab in the last day"). Playback does not tell when a key was changed, so StagePilot notices it: each time it reads a setlist it compares every song's key with the key it saw the time before (a song seen for the first time has no change, a change back is a new change). It asks nothing extra to do this. What it remembers (the song's Playback number, key names and times; no titles) is kept in its own small file, `playback-keys.json`, next to the settings, so a restart keeps it; it is written only when a key changed or a song was seen for the first time, holds at most 500 songs, drops songs not seen for a week, and a damaged or missing file is simply ignored. The cards drop the words by themselves when the day is over. With no Library Link nothing is read, remembered or shown, and forgetting the Library Link (or choosing another Playback computer) empties the file.

- **The Library Link and the Lightkey helper update themselves when StagePilot starts.** When the helper on the Playback or Lighting computer is older than the one inside StagePilot, StagePilot sends it the new files by itself once nothing is playing, no song timer runs and no take is being recorded, and the block says `Updated to 0.4.0 automatically.` There is no Update now button. A helper installs an update only if the StagePilot project signed it (an Ed25519 signature that StagePilot also checks before it sends anything; the helper key alone cannot install code), keeps the old files, and puts them back by itself if the new version will not start. The helpers do it from Library Link 0.4.0 and Lightkey helper 0.6.0; an older one is updated once with the setup command and then looks after itself. `helper_auto_update` in `settings.json` turns it off.

### Removed

- **"Forget the Library Link" is gone**, with its route (`POST /api/v1/playback-library/forget`), the `forgotten` state and every message for it. Nobody needs it: the key is made once and kept, and the setup command can be run again at any time and reuses it. An older `settings.json` that holds `playback_library.enabled: false` from an earlier forget still loads, and the Library Link then shows what the script on the Playback computer answers.

- **Song order now comes from the Library Link first; the song scan is the backup.** When the Library Link is ready, Playback is connected, and every song of the loaded plan is in the setlist Playback has open (in the same place as in the plan), StagePilot saves the song order and the song lengths by itself, with no scan and nothing to click. It saves again only when something changed (the plan, the setlist, Playback's setlist version or the lengths in the library), never on every look. A scan you run yourself stays in charge until the plan, the setlist or Playback's setlist version changes. When the Library Link's order cannot be trusted, nothing changes from before (the learned order, or the scan button) and the Song order section says why in one line, for example "Not using the Library Link's order: Faithfulness is not in Playback's setlist." That line is information only and never changes Ready or Check system. The Song order summary now says where the order came from: "From the Library Link (6 songs)", "From a scan (6 songs)" or "Learning as songs play". Without a Library Link everything works as before.

- **StagePilot connects to the Library Link by itself when it starts, and "Scan Playback Songs" reads it instead of stepping through Playback.** On start-up, with no window open, StagePilot now asks the Library Link right away (it used to wait for a window, and then for the first look five seconds later): it reads the helper, the setlist Playback has open and its songs, and saves the song order when it can be trusted. If the Playback computer is not up yet, StagePilot tries again after 2 seconds, 5 seconds, 15 seconds and then every 30 seconds until it answers. Starting never waits for this, a computer that was never set up with the Library Link is never asked, and the key is never written to the log. When the Library Link works, **Scan Playback Songs** no longer steps through your songs: no confirmation, nothing is played or selected, and it works while Playback is playing. It reads the Library Link again right now, replaces an older scan with the Library Link's order, and says "Read 6 songs from the Library Link"; while it reads, the button itself shows a loading animation (at least a moment, so you see it). If the Library Link's order cannot be trusted, the Song order section says why and offers **Scan with Playback instead**, which is the same confirmed scan as before. Without a working Library Link the button is exactly as it was. The header reminder "Scan Playback again" stays quiet while the order comes from the Library Link.

### Fixed

- **"Match the Playback lengths to Planning Center" reads the service you picked.** It used to look for the upcoming plan by itself; while another service is picked, the scanned lengths belong to that service, so the preview now reads the same one (or says that it is gone from Planning Center).
- **Switching to another service no longer keeps the highlighted song of the old one.** A reload of the same service keeps your place; a different service starts at its first song.

## [2.44.0-rc.1] - 2026-11-17

### Added

- **Optional "Playback library": see each plan song's key, BPM, length and arrangement from Playback.** In the Playback API panel you can switch on the Playback library and give the address of the Playback computer (port 8780 unless you add one). StagePilot then shows, on the Songs strip and in the panel, each plan song's key (and "was C" when it is not the original), BPM, countdown length (the start of the song's last measure; if that is more than 6 seconds before the end of the song, or the song has no bar lines, the end of the song), number of sections and arrangement, and the header pill warns when a plan song is not in Playback's setlist, is in a different place, is a rental that ends before the service, or Playback has no setlist for that date. A length that differs from the plan by more than 15 s is shown as information only. It is off by default, only reads, and changes nothing in Playback, Planning Center, the saved scan, the timers or the lights. The helper on the Playback computer is installed by hand for now.
- **The Playback library shows each song as a chip with its cover and a color bar of its arrangement.** In the Playback API panel, each plan song now has a small cover (when Playback holds a picture, pictures reach the window only through StagePilot and are kept in memory only), its key, BPM and countdown length, and a thin bar of its sections tinted like Playback. StagePilot also now asks the Playback library helper only for its change token every 30 seconds and reads the songs again only when Playback's data changed (or every 10 minutes). Needs `playback-library` 0.2 for the covers and the change token; an older helper still works, without covers.
- **The lighting timeline can show the song's arrangement bar and waveform from the Playback library.** When the Playback library is on and the selected song is paired, two rows appear between the ruler and the cues: the song's sections as tinted blocks with Playback's short names where they fit, and the song's waveform (no cover picture). Both scroll and zoom with the cues, are view-only, and can be hidden with the new **Song map** button (remembered per browser; while it is off the waveform is not asked for). The waveform comes only through StagePilot (`/api/v1/playback-library/songs/<id>/waveform`), is asked only when that song's timeline is on screen, is kept in memory only (16 songs), and a song without one just shows the bar. With the library off, the song unpaired or Playback silent, the timeline is exactly as before.

### Fixed

- **The Songs strip and the timeline now update by themselves when a recording finishes; before, the window kept showing "not captured" until you restarted StagePilot.** After a service the window could still say "0 of 4 fully captured" although the backend had saved all four songs; only the Lightkey strip's own sentence was current. The backend now counts every save of the settings (a recording, a loudness take, a cue-map save) and the open window loads the settings again, once per burst of saves and never when nothing changed; what you are typing is not disturbed. **Clear all songs...** now also refuses, like every other cue-map save, to erase a song that was recorded after the window last loaded.
- **Shutting StagePilot down no longer waits for a check-in with the control plane that is backing off.** The check-in every install makes can be told to wait up to five minutes before trying again, and stopping the app used to wait for that. Now stopping starts no further attempt and ends the waiting, and an attempt already on the wire is left to finish by itself. What StagePilot sends is unchanged. Five tests that raced the clock (keychain answer, launch, the dismissed Lightkey offer, microphone open, the dense-song screen) now wait for what they check instead, so they no longer fail on a slow computer.
- **The test of a take with a link drop and a seek no longer fails on slow machines.** `a_take_with_a_drop_and_a_seek_is_saved_once_complete_with_the_gaps_counted` failed once on the rc.12 Windows runner (recorded 7.14 s, expected 6.11 s). It compared the seconds the link counted with the real time measured around real sleeps, left out the stretch recorded after the reconnection, and so had only 0.2 s of room before the poll lag of the link used it up. It now runs the link, the fake relay and the fake HTTP agent on one clock the test moves by hand, and the expected seconds are exactly the stretches the link recorded. The test for recording changes at song time also expects the seconds each change was sent at instead of the length of its sleeps. The fake relay of these tests also keeps serving when Windows reports a datagram sent to a closed port (WSAECONNRESET) as an error of its next read, and the take test waits until the relay has read the link's unsubscribe before it moves on: without that the two take tests hung on the Windows runner. Nothing in StagePilot itself changes.

### Changed

- **The app and the web page are about 7 MB smaller and load faster on phones; the pictures look the same.** The five connection icons were 1024 px pictures of 1.4 MB each, shown at 25 to 70 px; they are now 256 px (22 to 45 KB each). The grainy page background keeps its size and is saved with a lighter setting (1.5 MB down to 0.8 MB). An unused extra Lighting icon (1.5 MB) is gone, and the browser tab icon is 11 KB lighter (same picture). A test now fails if a picture in `frontend/src/assets` grows past 300 KB (the background has its own limit).

## [2.43.0] - 2026-11-11

StagePilot 2.43.0 is the Lightkey release. StagePilot records what the lights do during a song, replays it by itself the next time the song plays, and sets up the Lighting computer for you. It also reminds you to scan Playback again before the service. The candidates rc.3 to rc.12 below hold the step-by-step history.

### Added

- **Lightkey capture and replay, song by song.** Each song is in Auto, Replay or Record. In Auto, a song without a complete capture is recorded and a song with one is replayed. A capture is complete when the recorded time is within 10 s of the song's length, gaps allowed. Every capture is saved, finishes by itself at the song's end, and keeps the starting look of all the lights (house lights included), so a replay begins from the right picture. The raw recording of each finished song is kept in `lightkey-recordings/` next to `settings.json`.
- **Smooth replay.** A recorded song is fitted with straight-line ramps and detected effect loops instead of being thinned, and replayed through a pacing queue so a big look lands on the beat. A very busy song is simplified evenly instead of freezing the moving heads, and a song is never thrown away for size.
- **The operator wins, per light.** If you change a light in Lightkey during a replay, StagePilot leaves that light alone for 30 seconds.
- **Set up the Lighting computer from the Lighting section.** One copy-ready command installs the Lightkey helper on the Lighting Mac, and StagePilot finds that computer by itself afterwards. The Lighting section shows the Lightkey project and its fixture count, and the Lighting card says Connected when capture and replay are both live.
- **A reminder to scan Playback again.** The header status shows "Scan Playback again" when the next service is less than an hour away and Playback has not been scanned since. It stays quiet while the service plan is going (also between songs, such as a sermon), works for any number of services, and a sound check before the service does not hide it. A fresh scan always sets the exact song order and lengths.
- **Countdowns work before you scan.** StagePilot learns which Playback song is which plan song as they play. This learned order is a stopgap: scan to correct it.
- **Problems show in the header status pill**, like every other problem: a damaged song, unreadable settings, a recording that must be made again.
- **Lighting timeline.** Captured lights show as Look rows, cue types are colour-coded with a legend, the room loudness is drawn behind the timeline, and Clear and Clear all ask in a proper dialog first. The Lighting card says when it is saving a long recording.
- Configuration panels open and close with a short animation from their connection card.

### Changed

- **Old Lightkey takes are not replayed.** Captures stored only in `lightkey-takes.json` are ignored (the file is left alone). Record those songs again. Cue maps saved in settings load and replay as before.
- The Location setting is gone. Once a week the app asks db-ip.com for the city of its internet address (only the city is kept).

### Fixed

- **Opening StagePilot on another device during a song no longer stops the lights.** Every page load saved the settings again and Lighting treated that as a change. Saving settings that are already running now changes nothing, and a real change mid-song resumes at the second the song had reached.
- **Reloading the page or losing the connection during a song scan no longer leaves the scan stuck on "running".** A cancelled or failed scan puts Playback back on the song it started on.
- **Jumping into the middle of a newly selected song and pressing play now switches StagePilot to that song.** Pause, jump, play follows the jump.
- **The moving heads no longer freeze for 30 seconds after a song starts**, and a preset switched on just before a song no longer keeps the song's first look away from those lights.
- **Captures made after a link drop are no longer stored about 6.6 s early.** A pause, even late in the song, no longer ends the recording early.
- **A damaged lighting recording or a setting saved from an open page can no longer reset StagePilot's settings or erase captures.**
- **Planning Center no longer stays in error after the keychain prompt is answered**, and a keychain prompt that is pending or refused can no longer hold StagePilot up at start.
- The song countdown no longer runs about 0.6 s behind Playback, and Elapsed time follows the song position after a pause, resume or seek.
- A test cue's result stays on the Lighting card instead of flipping straight back to Ready.

### Compatibility and known limits

- **Do not edit Lighting cues in v2.42.0 after trying this version.** v2.42.0 starts on a file saved here, but deleting or reordering rows there can attach ramp or message data to the wrong row. Do not go back to an older version with Look rows saved.
- The first look of a song can still leave 290 to 485 ms after its time (it cannot be sent before the song starts); later looks are within about 45 ms.
- Lightkey capture and replay is for Lightkey only.

## [2.43.0-rc.12] - 2026-11-11

### Fixed

- **The Lighting card keeps the result of the test cue instead of flipping straight back to Ready.** After a test cue the card says `Sent <address> to <target>.`; a change of the Lightkey helper link a few milliseconds later (the link's first status changes arrive just after start) redrew it as `Replay ready ... Capture is not connected yet.` and the test cue's result vanished. The card now keeps the `Sent ...` text for 3 seconds; a link change inside that time is applied when the 3 seconds are over, so the card still ends up showing the real state. This also stops the test `osc_test_cue_sends_and_reports_sent_not_connected` from failing on slow machines.
- **Reloading the page or losing the connection during a song scan no longer leaves the scan stuck on "running".** On the iMac (rc.11) a client that closed its request 10 s into **Scan Playback Songs** left the scan "running" with the same progress for good: every new scan was refused ("Song order discovery or Planning Center update is already running."), the Planning Center length update was blocked, Playback was left stopped at the end of song 1 and only quitting StagePilot cleared it. The scan now runs on its own, not inside the browser request, so it finishes whatever happens to the request: it returns the song to the start and the original selection and ends as done or failed. If it is ever stopped hard (a crash inside it, or the app shutting down) it ends as failed with a plain message, never "running". A scan that fails part-way now makes the same best-effort walk back to the original song that a cancelled one already did. New: `POST /api/v1/playback-api/discover-song-order/cancel` stops a running scan (a Rust-only extension). Also, **Release lights**, the Record switch and the Lighting and Lightkey settings saves now finish even if the page that sent them goes away, so a closed tab can no longer leave lights held or a save half applied. See \"Discover Song Order\" in `docs/playback-api.md`.
- **Opening StagePilot on another device during a song no longer stops the lights.** Every dashboard page load saved the ProPresenter, Lighting and Planning Center settings again, and Lighting treated that as a change: it ended the song's lights, reconnected Lightkey and never resumed (rc.11 iMac test: the replay ended 27.5 s into a song when a second page was opened and stayed off for the remaining 47 s). Now saving settings that are already running changes nothing: no reconnect, no Connecting flicker, no re-sync of the lights, no ProPresenter Look triggered again, no Planning Center reload. "The same" covers every setting that changes behaviour (see \"Saving Lighting settings while a song plays\" in `docs/lights.md`). The dashboard also skips services whose status is already connected. A real change in the middle of a song is still applied; the lights replay of that song now starts again at the second it had reached instead of staying off until the next song.

## [2.43.0-rc.11] - 2026-11-10

### Changed

- **A preset you switch on just before a song no longer keeps the song's first look away from those lights.** StagePilot gave a light to the operator for 30 s whenever it changed in a way the replay had not caused, also when no song was playing. On the 2026-11-08 rig test `clear_all` pressed 16 s before play left 19 lights out of the first look of Ever Be: they did not follow the plan for the first 12 s and the moving heads not for 35 s. Now what is touched while no song is replaying (before the first look, after a song ended) is not a take-over: when a song starts from the top, or play is pressed on a song that is not on (a resume or seek), every older take-over is dropped and the first look goes to every light it covers. A touch during the song, also while it is paused, works as before (the light is yours for 30 s, then handed back; a touch of the last 30 s stays yours after a jump). Nothing is released at a song's end or on stop. A Lightkey preset on a held light still shows nothing until **Release lights**. This is a behaviour decision ("option A", the first look holds the whole rig). See \"The operator always wins, per fixture\" in `docs/lights.md`.

### Fixed

- Jumping into the middle of a newly selected song and pressing play now switches StagePilot to that song. In Playback, selecting a song that is not StagePilot's current song, jumping into it and pressing play sent `song.selected`, `seek`, `song.resumed` (rig test of 2026-11-08: song 30049080 at 232.8 s); a play only counted as a song start below 2 s, and a resume of a song other than the current one was dropped, so StagePilot stayed on the previous song, its timer stood still, the lights did not replay the new song and its position was not followed. That resume is now handled as the song being started at that position (current song, countdown, ProPresenter timer and lights from the jump position, not from 0). A deliberate difference from the Python app. Resumes inside the current song, auto-advance, navigation without play, pause, stop and section loops are unchanged. See "How a song is recognised" in `docs/playback-api.md`.
- **The moving heads no longer freeze for 30 seconds after a song starts.** On the 2026-11-08 rig test (nobody touched anything) some lights were counted as taken by the operator at the start of a replay and then ignored for 30 s: Elohim from the start lost MH1, MH3 and MH4 until +29.6 s, Ever Be from 175 s lost Z2, and Ever Be from the top lost the heads for about 30 s after the first look (MH1-MH4 matched the plan in only 3 to 11 of 24 samples between 8 and 20 s). Three causes, three rules (see \"The operator always wins, per fixture\" in `docs/lights.md`): the relay reports the *old* position once more right after the first send (the first value a property is sent has no earlier sent value, so the relay's last value is now its baseline and a change equal to it within 500 ms is the echo); a head that has to travel more than 5 % of its range takes up to a second and settles with a small overshoot (for 1.5 s after such a send a value in the stretch old to new, widened by the overshoot, is the echo); and an 8-bit pan moves in steps of 2.47 degrees, wider than the 1 degree allowed (angles now match within 0.5 % of the range). A real hand-set move still takes the light on the first change (tested with Z1 pan -20 then +20) and gives it back after 30 quiet seconds. The offline check `lightkey_yield_check::run_start` replays the helper's log around a song start against the engine; the tests replay the three real logs and show that the rc.10 rules yield the lights and the new rules yield none.
- **On a very busy song the moving and chasing lights no longer stop; the show is simplified evenly instead.** StagePilot sends at most 200 packets a second to Lightkey. On the busiest saved song (7 loops, 36 chasing lights, 16 colour fades every 100 ms) the colour fades took all of it and the loops got nothing: they changed about 4 times in 30 s where the recording changed them 500 to 2,300 times. When a song asks for much more than that for more than 2 seconds, the loops now get at least 35% of the packets and the colour fades at most 65%, and every light is served in turn (on that song: every loop light 3.4 to 4 times a second, every colour fade about 4, where the loops had none). The Lighting panel says `This song is very busy: some lights update less often than recorded.` while that happens (new `replay_load` with `busy` and `served_percent` in the lights status). A song that fits (both Sunday songs) plays exactly as before. The 200 packets a second limit is **not** raised: whether Lightkey takes more is not known and needs the rig. See "A very busy song" in `docs/lights.md`.
### Added

- **A damaged song or unreadable settings now show in the header status pill, like every other problem.** If a song's lighting recording is damaged when StagePilot starts, only that song is taken out and the rest loads (rc.10), but nothing on screen said so. Now the pill reads `Record <song> again` (in the same red as `Remote access offline`) and its list, which opens when you point at or focus the pill, has one row per song with the sentence `The lighting data for <song> was damaged and set aside; record it again.` The row and the pill message go away on their own as soon as you record the song again; no restart and nothing to close. If the whole settings file could not be used, the pill reads `Error: Saved settings could not be read` (or `Error: Settings are from a newer StagePilot`), and the list explains that StagePilot started with its default settings. Nothing is shown when nothing is wrong, and the read-only Live status view is unchanged. The Settings response has a new `settings_set_aside` list (`{song_key, song_title}` per song; a Rust-only extension in the contract) next to `settings_notice`. See "A damaged song never costs you the settings" in `docs/lights.md`.
- **The Lighting card says it is saving a long recording.** A busy 20-minute song takes 35-45 s to fit after it ends and the card showed nothing, so it looked as if the recording was lost. From the moment the recording ends until the looks are fitted and saved the card now reads `Saving the recording of “<song>”…` with a pulsing dot, then the usual `Saved N looks...` message (or `Not saved...` / `Could not save...` as before). New `saving` field on the Lightkey status (`{song, started_at}`, left out when nothing is being saved; a Rust-only extension in the contract). The line is easier to read: its explanation is brighter, and the dot keeps its full colour while a soft ring beside it pulses (a still dot when motion is reduced).

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
