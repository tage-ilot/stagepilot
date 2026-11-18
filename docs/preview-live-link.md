# Previewing the Playback Live Link without Playback

The Playback Live Link block (Playback API panel) shows two things: the Playback API connection and the optional **Library Link**
on the Playback computer. To look at it in a browser you need neither Playback nor the real helper. Three small programs on this
computer stand in for them. All use only the Python standard library, bind to `127.0.0.1` and write nothing.

| Stand-in | File | What it pretends to be |
|---|---|---|
| Playback API | `tools/fake_playback_api.py` | Playback's remote connection (heartbeats every second): the "Playback API: Connected" line |
| Library Link | `tools/fake_playback_library.py` | `huntrw6/playback-library` 0.3.0: it checks the HMAC key exactly like the real helper |
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

The block now reads **Playback API: Connected to Playback on this computer** and **Library Link: Update available, version 0.3.0**
(a helper with no key: it works, and the setup is offered). The Songs in Playback chips appear with covers.

## Pair it, like a real install

The setup command is for the *Playback computer's* Terminal; for the preview, the stand-in takes the key from the same one-time script.

1. In the block, press **Set up the Playback computer** (or open **Library Link setup**). The command appears at once with a countdown; StagePilot uses this computer's own network address and never asks for it.
2. Stop the stand-in and start it again with the key taken from that script (the command itself is not run):

   ```sh
   python3 tools/fake_playback_library.py --port 18780 --key-file /tmp/pbl-key.txt \
     --pair-from http://192.168.1.20:<port>/playback-helper/<token>/install.sh
   ```

   (Use the address in the command StagePilot shows.) Within a few seconds the box says **The Library Link is ready.**, closes and the block reads
   **Library Link: Ready, version 0.3.0** with the pill **Connected**.
3. **Advanced > Forget the Library Link** asks to confirm, deletes the key and the line reads **Forgotten**; the stand-in still runs but
   is answered with 401 when it is asked again.

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

The stand-ins speak the protocol the way the written spec and `playback_library.py` 0.3.0 describe it (the fake is tested against the bundled
file's own signing code). They cannot show how the real installer behaves on macOS (launchd, Playback's permission prompts, `curl` and `sh`
versions), or the real covers and arrangements. Run the setup command once on the real Playback computer to see that.
