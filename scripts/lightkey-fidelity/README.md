# Lightkey fidelity check

A closed-loop check that StagePilot's replay of a song looks the same as the original run. The judge is
Lightkey's **raw sACN output** (what the lights really get), decoded with the helper's own decoder, for both
runs. The helper's log is not used: its timestamps smear. A tool for the person on the rig; not part of the app.

Python 3.9+, standard library only. Nothing here sends to Lightkey, and nothing changes the helper.

## The procedure

1. **Original:** on the Lighting computer run `python3 sniff.py original.pkl 90` (add `--universes 1,2,3` if the
   rig uses more than 1 and 2) and, while it listens, let the operator run the lights through the stretch to check.
2. **Replay:** put the same stretch on the Playback side and replay the song from the same start. Record again:
   `python3 sniff.py replay.pkl 90`.
3. Copy both files and the helper's fixture map (`fixtures.json`, the file `lightkey_api.py export-fixtures` writes)
   to any computer with this repository and run:

       python3 compare.py --fixtures fixtures.json --a original.pkl --b replay.pkl [--from S --to S]

   `--from/--to` are seconds on A's clock (A's first frame is 0); use them to leave out the start and the end.
4. **Against a saved song** (no second recording): dump the song's grid with
   `cargo run -p stagepilot-lights --example dump_song -- cue_map.json song_grid.json 87 147` (song seconds 87 to 147),
   then `python3 compare.py --fixtures fixtures.json --a replay.pkl --saved song_grid.json --from 90 --to 144`.
   The song is then the reference ("A") and the capture is "B". Auto alignment starts from "B's first frame is
   where A's window starts"; if that is far off, give `--align-guess S` (seconds, B later than A) or a fixed
   `--align S`; `--align none` means offset 0.

`--check` makes the exit code 1 when the verdict is not "indistinguishable".

## What the report says

* **Alignment.** B is lined up with A by one constant offset (reported in ms, found by cross-correlating the
  decoded series, then settled on the real 25 ms grid) and the largest offset any single property needs on top
  of it (the spread: a property that plays later or earlier than the rest). A looping show matches itself one
  loop later too; the offset nearest the guess wins, so give `--align-guess` if the start differs by more.
* **Per class** (brightness = dimmer, colour, pan/tilt, zoom/focus, other options): the number of fixtures,
  the share of 25 ms grid samples where B differs from A by more than one DMX step (1/255 of the property's
  range; angles in degrees against the fixture's pan or tilt range) and by more than 0.02 (2% of the range),
  p50/p90/p99/max of the difference **in DMX steps**, and the one-frame flashes per fixture per minute in A and in B
  (a value leaves where it was by more than 5% for one sample and comes back to within 2%).
* **Worst addresses:** the 5 addresses with the largest share of samples more than one step apart, each with the
  times of its biggest differences and both values.
* Options and names (gobo, shutter, colour wheel) that differ count as the whole range (255 steps).

**Indistinguishable here** means: at most one DMX step apart for 99% of samples in every class, no one-frame flashes
in B that A does not have, and a constant offset under 25 ms. A flash that is in A (baked into an old song's rows)
and in B is a copy, not a fault of the replay.

## Files

* `sniff.py`: listen to the sACN multicast of the chosen universes for N seconds and save every frame as
  `(wall time, universe, sequence, 512 slots)` in a pickle (protocol 4). It only receives.
* `compare.py`: the comparison. It imports the helper from this repository
  (`crates/stagepilot-lights/assets/lightkey-helper/lightkey_api.py`; `--helper PATH` for another copy) and
  refuses a capture file that names any Python object (only tuples, numbers and bytes load), but still only open
  captures you made yourself.
* `test_fidelity.py`: tests on synthetic captures, run by CI: `python -m unittest discover -s scripts/lightkey-fidelity`.
* `crates/stagepilot-lights/examples/dump_song.rs`: writes a saved song's 25 ms grid, from the same
  `Timeline::playback_at` the replay plays.

Captures, fixture maps and song grids of a real rig are private: keep them out of the repository.
