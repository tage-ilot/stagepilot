# Archive

Historical records of the 2.42.0 move from the Python backend to the Rust server. They are kept
for the reasoning, the test maps and the rig results; they are not maintained and may describe
code that no longer exists.

- `rust-migration/`: phase plans, the behaviour-preservation test maps (which Python test became
  which Rust test), the contract notes and the rig verification runs.
- `rust-handoff/`: notes written for porting the Lightkey record/replay and fixture relay
  features.

The Python backend itself was removed after 2.42.0. Its last released state is the tag
`v2.41.1`; for example `git show v2.41.1:backend/src/stagepilot/core/lightkey.py`.
