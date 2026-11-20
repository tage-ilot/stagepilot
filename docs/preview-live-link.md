# Previewing the Playback Live Link without Playback

The Playback Live Link block (Playback API panel) shows two things: the Playback API connection and the optional **Library Link**
on the Playback computer. To look at it in a browser you need neither Playback nor the real helper. Three small programs on this
computer stand in for them. All use only the Python standard library, bind to `127.0.0.1` and write nothing.

| Stand-in | File | What it pretends to be |
|---|---|---|
| Playback API | `tools/fake_playback_api.py` | Playback's remote connection (heartbeats every second): the "Playback API: Connected" line |
| Library Link | `tools/fake_playback_library.py` | `huntrw6/playback-library` 0.4.0: it checks the HMAC key exactly like the real helper |
| StagePilot | `cargo run -p stagepilot-server -- --demo` | the demo backend (a demo Planning Center plan, no keychain) |

The demo backend has no Playback API of its own, so the stand-in is a real listener the demo's own Playback connection finds. Nothing in
the product is switched on for this: it is the same code path as a real Playback.

## Run it

```sh
# 1. the stand-ins (three terminals, or put & after each)
python3 tools/fake_playback_api.py --port 18080
python3 tools/fake_playback_library.py --port 18780 --key-file /tmp/pbl-key.txt   # no key yet: it answers everyone, like a helper without a key
cd frontend && npm run build && cd ..
cargo run -p stagepilot-server -- --demo --port 8765 --web-root frontend/dist
```

Open `http://127.0.0.1:8765`, then the Playback API panel (Configuration).

```sh
# 2. the Playback API: a Playback computer at 127.0.0.1, port 18080 (the default port is 8080: use that with --port 8080 and skip the PUT)
curl -s -X PUT http://127.0.0.1:8765/api/v1/playback-api/settings -H 'content-type: application/json' \
  -d '{"enabled":true,"host":"127.0.0.1","port":18080,"auto_scan":false,"fast_transport":true}'
# 3. the Library Link's address (an override, because the stand-in is on its own port; a real helper is on 8780 and needs none)
curl -s -X PUT http://127.0.0.1:8765/api/v1/playback-library/settings -H 'content-type: application/json' -d '{"address":"127.0.0.1:18780"}'
```

The block now reads **Playback API: Connected to Playback on this computer** and **Library Link: Update available, version 0.4.0**
(a helper with no key: it works, and the setup is offered). The Songs in Playback chips appear with covers.

## Pair it, like a real install

The setup command is for the *Playback computer's* Terminal; for the preview, the stand-in takes the key from the same one-time script.

1. In the block, press **Set up the Playback computer** (or open **Library Link setup**). The command appears at once with a countdown; StagePilot uses this computer's own network address and never asks for it.
2. Stop the stand-in and start it again with the key taken from that script (the command itself is not run):

   ```sh
   python3 tools/fake_playback_library.py --port 18780 --key-file /tmp/pbl-key.txt \
     --pair-from http://127.0.0.1:<port>/playback-helper/<token>/install.sh
   ```

   (Use the address in the command StagePilot shows.) Within a few seconds the box says **The Library Link is ready.**, closes and the block reads
   **Library Link: Ready, version 0.4.0** with the pill **Connected**.

## Other states to look at

| To see | Do |
|---|---|
| Not installed | Stop the library stand-in (nothing answers). Grey, no warning anywhere |
| Update available | `--version 0.2.9`, or `--old` (no `auth` field: helper 0.2.0), or no key at all |
| Needs the command again | Keep the key file but put another key in it (`echo wrong-key... > /tmp/pbl-key.txt`, 32+ characters) |
| Not answering | Pair it, then stop the stand-in: the one state with a notice in the header pill |
| Another date | The stand-in gives a six-song setlist for `--date` (default today) and one song for any other day: pick another service with the date picker |
| The API alone | Stop `fake_playback_library.py`: the pill says **API only** |
| Playback API not connected | Stop `fake_playback_api.py` |

## What this does not prove

The stand-ins speak the protocol the way the written spec and `playback_library.py` 0.4.0 describe it (the fake is tested against the bundled
file's own signing code). They cannot show how the real installer behaves on macOS (launchd, Playback's permission prompts, `curl` and `sh`
versions), or the real covers and arrangements. Run the setup command once on the real Playback computer to see that.

## The ProPresenter Link (the block and the "Add to ProPresenter" window)

`tools/preview_propresenter_link.py` starts the stand-ins and a small **preview server** in front of the demo backend:

```sh
cd frontend && npm run build && cd ..
cargo run -p stagepilot-server -- --demo --port 8765 --web-root frontend/dist     # the demo backend, in one terminal
python3 tools/preview_propresenter_link.py --bind 0.0.0.0 --port 18900            # the rest, in another
```

It makes a made-up ProPresenter folder in a temporary directory (tiny synthetic songs, never a real one), serves it as ProPresenter's API
(`tools/fake_propresenter_api.py`, with the playlists *Sunday Morning Service* (headers Welcome, Worship, Message, Closing), *Wednesday Night* and
*Youth Night* in a folder), starts `fake_playback_api.py` and `fake_playback_library.py` (on 8780, where the real Library Link always is) and
tells the demo backend where they are. Then it prints two addresses: open the first one, `http://<this computer>:18900/?pv=<word>`. The preview
server asks for that one-time word (kept in a cookie) because it sits in front of a backend that trusts its own computer; do not give the
address to anyone who should not see the demo. **Ctrl-C** stops everything and removes the folder.

| To see | Do |
|---|---|
| The block, this computer | Configuration > Presentation. Reads `ProPresenter Link: Can add songs, cannot import missing songs (needs a Mac), on this computer` (this computer is not a Mac) |
| The window | Configuration > Playback > **Add to ProPresenter**. With the demo's four songs: Holy Forever and Gratitude become new copies, Battle Belongs is not in ProPresenter (blocked: import needs a Mac), Firm Foundation has two files (needs a choice). Tick **Skip blocked songs**, **Add songs**, wait about 15 seconds |
| `Already there` | Add the songs once, then **Preview** the same songs again |
| Another computer's Link | `http://<this computer>:18900/__preview/` lists the states (Not installed, Ready, cannot import, Update available (by hand / by itself), Needs the command again, Not answering). They change only what the status and setup routes say; adding songs always goes through the real routes and the made-up folder. **Set up the ProPresenter computer** shows a made-up command with the countdown; switch to *Ready* and the box closes by itself |
| `Open the Playback library once` | Stop the preview, start it again with `--lazy select`: new files get their id only after the library was selected. Add songs, wait 30 seconds for the instruction, press **I opened the Playback library in ProPresenter** on the states page, then **Try again** in the window |
| Start over | **Start over** on the states page forgets the songs added to the playlists |

It cannot show how the real helper installs on macOS, ProPresenter's own lazy loading, the Search import, or whether ProPresenter honours the
selected arrangement when a song is loaded from a playlist: those are in the PR's "Not proven without the rig".
