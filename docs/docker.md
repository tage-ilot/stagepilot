# StagePilot Server with Docker and RTP-MIDI

StagePilot Server is designed for this deployment path:

```text
Proxmox -> Ubuntu LXC -> Docker -> StagePilot Server + MIDI Gateway -> production LAN
```

The Compose stack contains two failure-isolated services. `stagepilot` runs FastAPI, the event
bus, automation, state, and the React dashboard. `stagepilot-midi` owns AppleMIDI/RTP-MIDI
sessions and translates network MIDI to a versioned JSON-lines protocol on the private
`/run/stagepilot-midi/midi.sock` Unix socket. The gateway contains no cue mappings or StagePilot
business logic.

Incoming messages follow this path:

```text
RTP-MIDI -> gateway -> private IPC -> existing bounded MIDI queue
         -> existing mapping/debounce/action/event pipeline
```

Lighting output follows the reverse transport path. Desktop StagePilot continues to use the
existing native Mido/RtMidi implementation unless explicitly configured otherwise.

## Requirements

- Linux Docker Engine 24+ with Docker Compose v2
- an Ubuntu LXC with ordinary bridged LAN access
- inbound and outbound UDP on the production LAN
- multicast UDP 5353 when mDNS/Bonjour discovery is enabled

No ALSA packages, `/dev/snd`, `/dev/snd/seq`, USB passthrough, privileged container, privileged
LXC, or additional Linux capabilities are required. Docker Desktop is not the target because
host networking and multicast behavior differ from a native Linux Docker host.

## Start

Copy the example and edit it before enabling real cues:

```sh
cp .env.docker.example .env
docker compose up -d --build
docker compose ps
```

Open `http://<lxc-address>:8765`. The initial browser PIN is `1234`; replace it immediately.
The default remains safe: the service plan and MIDI source are simulated. To enable incoming
network MIDI, set:

```dotenv
STAGEPILOT_MIDI_SOURCE=real
STAGEPILOT_MIDI_TRANSPORT=network
STAGEPILOT_MIDI_INPUT_NAME=StagePilot
```

The backend starts and serves the dashboard even when the gateway or all MIDI peers are down.
The MIDI plugin reports disconnected/error and retries through its existing bounded backoff.

## Networking and firewall

Both services use `network_mode: host`. RTP-MIDI uses a control UDP port and the immediately
following MIDI UDP port. The default inbound session therefore needs UDP 5004-5005. Each direct
outbound destination gets its own pair starting at 5010 (5010-5011, 5012-5013, and so on).
StagePilot's web server listens on TCP 8765 by default.

Host networking avoids unreliable UDP port prediction and lets mDNS use the LXC's LAN interface.
It also means Compose `ports:` mappings do not apply: bind/firewall the LXC deliberately. Permit
TCP 8765 only from dashboard clients, UDP 5004-5005 from MIDI peers, the configured outbound UDP
pairs, and UDP 5353 multicast only on the trusted LAN. Never forward RTP-MIDI to the public
internet or put it behind an HTTP reverse proxy.

For Proxmox, use a normal unprivileged Ubuntu LXC with nesting enabled for Docker and a bridged
virtual NIC on the production LAN. No sound-device or USB mapping is needed. If the Proxmox or
guest firewall is enabled, add the same narrow TCP/UDP rules there. Multicast snooping, VLAN ACLs,
or Wi-Fi client isolation can block Bonjour while direct IP still works.

## Discovery and direct peers

`STAGEPILOT_MIDI_DISCOVERY_ENABLED=true` advertises the inbound AppleMIDI session using the
configurable `STAGEPILOT_MIDI_SESSION_NAME` (default `StagePilot`). Failure to start mDNS is logged
separately and does not stop RTP-MIDI or the gateway.

Peers may connect directly to `<lxc-ip>:5004`; discovery is never required. Outbound destinations
are explicit so a cue is never broadcast to every connected participant:

```dotenv
STAGEPILOT_MIDI_OUTBOUND_PEERS=Lightkey=192.168.20.41:5004,Companion=192.168.20.42:5004
STAGEPILOT_LIGHTS_OUTPUT_NAME=Lightkey
STAGEPILOT_LIGHTS_ENABLED=true
STAGEPILOT_LIGHTS_TRANSPORT=network
```

The name before `=` is the StagePilot output name. The address is the destination's AppleMIDI
control port. The gateway creates one isolated local RTP-MIDI session per destination and retries
with bounded delays, so Lightkey messages are not also sent to Playback or Companion.

## Health and truthful status

- `/api/v1/health/live` checks the web process.
- `/api/v1/health/ready` preserves StagePilot's existing readiness semantics.
- `/api/v1/midi/network/status` separately reports gateway availability, session listening,
  discovery enabled/available, and connected input/output peers.
- `docker compose logs -f stagepilot stagepilot-midi` shows both sides of the private boundary.

A bound UDP socket is only `session_listening`; it is not a connected MIDI peer. The normal MIDI
connection becomes connected only after a peer exists and the configured `StagePilot` network
input can be opened. Missing peers do not make the HTTP process unhealthy.

## Persistent data and private IPC

`stagepilot-data` contains settings, the private Planning Center credential file, and the cached
service plan. `stagepilot-midi-run` contains only the runtime Unix socket and is shared by the two
containers. Both images run as UID/GID 10001. The IPC socket is not published on the LAN.

## Production credentials, recovery, and logs

The Compose example remains a separate installation path and does not enable managed Remote.
For any production deployment, keep credentials out of the repository, image layers, Compose
file, and command line. Mount a private data directory owned by UID/GID 10001 with mode `0700`.
Planning Center PATs have no independently selectable product scopes: they inherit the creating
user's permissions. Create the PAT from a dedicated least-privileged user that can read the
selected Services service type, plans, plan times, plan items, and songs, without People, Giving,
Calendar, Check-Ins, or organization-administration access where the organization's role model
allows that separation. Store its application ID in settings and its secret only in the private
credential file, mode `0600`.

Select and test these values before enabling automation: Planning Center service type and plan
preference, ProPresenter host/API port/timer/look, Playback MIDI input/session, and lighting MIDI
output. Start with actions and lighting disconnected, prove one harmless MIDI input and output,
then enable production cues. A healthy web process is not proof that these integrations are ready;
inspect `/api/v1/health/ready` and `/api/v1/midi/network/status`.

Back up the persisted data volume using a storage-consistent volume snapshot or while `stagepilot`
is stopped. At minimum preserve `settings.json`, `planning-center-secret`, plan cache, lighting cue
maps, and (when managed Remote is separately configured) `identity.sqlite3`, `remote.json`, and
`connector.token`. Backup media contains live credentials: encrypt it, restrict access, retain at
least one offline copy, and test restoration into an isolated state directory. Never restore one
installation's connector token into a concurrently running second installation.

Container stdout/stderr is structured JSON with common credential fields and bearer/token text
redacted. Configure the Docker logging driver with bounded rotation, for example `local` or
`json-file` with `max-size` and `max-file`, and search with `docker compose logs stagepilot
stagepilot-midi`. Do not ship raw logs to a third party until their redaction and retention policy
has been reviewed.

## Harmless end-to-end validation

1. Start the stack in simulated mode and confirm both containers are healthy.
2. Enable `STAGEPILOT_MIDI_SOURCE=real`, recreate `stagepilot`, and leave production actions
   disconnected while testing.
3. In MultiTracks Playback's Network MIDI/AppleMIDI connections, select the advertised
   `StagePilot` session. If it is not listed, create a direct connection to the LXC IP and UDP
   control port 5004.
4. Configure a harmless cue on channel 1, note 112 (`E7` in Playback), using a test velocity that
   is safe for the loaded plan.
5. Send it once and inspect **MIDI / Playback -> Recent messages** or
   `GET /api/v1/midi/messages`. Confirm channel, note, velocity, disposition, and action before
   connecting production automation.
6. For output, run an AppleMIDI test destination, add it to
   `STAGEPILOT_MIDI_OUTBOUND_PEERS`, set the matching Lighting output name, and use the dashboard's
   existing harmless lighting test cue. Confirm one note-on and one note-off at the destination.
7. If discovery fails, check `docker compose logs stagepilot-midi`, UDP 5353, VLAN multicast, and
   Avahi/Bonjour availability on the client. Keep discovery disabled if the network filters it;
   direct IP sessions continue to work.

## Troubleshooting and limitations

- `gateway_available=false`: check the `stagepilot-midi` container and the shared runtime volume.
- `session_listening=true` with no input peers: the LAN session is up but no remote peer completed
  the AppleMIDI invitation handshake.
- output missing from the output list: verify the direct address/port and that the destination
  accepts invitations; reconnection does not require restarting StagePilot.
- duplicate or delayed cues retain StagePilot's existing held-note, debounce, and bounded-queue
  behavior. Slow IPC subscribers drop bounded events and emit a warning rather than growing
  memory without limit.
- the `rtpmidi` 0.4.4 dependency supports invitations, clock sync, SysEx parsing, and session
  discovery but does not implement the RTP-MIDI recovery journal. Packet loss is therefore not
  reconstructed; use a reliable wired production LAN and validate with actual Playback hardware.
- `rtpmidi` does not expose packet SSRC in its message callback. StagePilot records the inbound
  session and connected peer set, but exact source attribution is ambiguous if multiple peers send
  at the same time.

## Production host status (agent-hub, verified 2026-09-28)

The production managed install on `agent-hub` runs native (systemd user units:
`stagepilot-beta-managed.service` / `stagepilot-beta-managed-connector.service`),
not this Docker Compose path — Docker/RTP-MIDI networking above applies if/when
the MIDI-gateway container path is adopted there. `systemd-analyze --user verify`
passed for both installed units; `loginctl show-user` reports `Linger=yes`;
`/media/SONY` (the filesystem backing the repo checkout) is mounted. See
`docs/remote-deployment-acceptance.md` → "Live verification" and "Backup and
recovery — policy and procedure" for the current PCO/ProPresenter/MIDI status
and the backup/restore procedure (target intentionally not yet provisioned).

## Reverse proxy and security

RTP-MIDI is trusted-LAN control traffic and has no application authentication. Do not expose its
UDP ports publicly. A web reverse proxy may handle TCP 8765 and `/ws`, but it must not proxy MIDI.
Keep StagePilot's existing PIN, CORS, and forwarded-address controls; do not broaden them for MIDI.
