# Remote Access offline alert

If the user has turned Remote Access on and it cannot be reached, the dashboard says so. There is
nothing to configure and no outside service involved.

## What the user sees

- The system check (hover or tap the status pill in the header) gets an extra line, **Remote access
  offline**, marked *Optional*. It is listed only while the problem exists.
- The pill itself changes from **Ready** to **Remote access offline** in the alarm colours. The
  system is still ready for a service: the line is optional and does not block anything, and the
  other lines in the check keep their own state. If something required is also wrong, the pill
  keeps showing that problem and the Remote line is just in the list.
- Hovering the pill explains it: Remote Access is turned on but cannot be reached; StagePilot keeps
  retrying; this computer is unaffected.
- When the tunnel is back, the line and the pill change disappear by themselves (within 5 seconds).

## When it appears

Only when **all** of these hold:

1. Remote Access is switched on (`enabled` in `GET /api/v1/remote-access`).
2. Its state is anything other than `connected` (`enabling`, `reconnecting` or `error`).
3. That has been true for **2 continuous minutes** (`REMOTE_OFFLINE_GRACE_MS` in
   `frontend/src/hooks/useRemoteOffline.ts`). The tunnel retries on its own with a delay that
   doubles up to 30 seconds, and takes a few seconds after every launch, so shorter blips and a
   normal start-up never show it.
4. The viewer is the local operator (someone who can change settings). Viewers and remote sessions
   do not poll and never see it: a remote session proves the tunnel works.

It never appears while Remote Access is off, and a failed status request (StagePilot itself not
answering) neither raises nor clears it; that case already shows as "StagePilot disconnected".

The dashboard checks every 5 seconds. The timer is kept in the open dashboard, so it restarts when
the page is reloaded or reopened (after a reload the line can take up to 2 more minutes to show
for an outage that began earlier).

## Related code

`frontend/src/hooks/useRemoteOffline.ts` (the rule), `buildReadinessChecks` in
`frontend/src/components/dashboard/dashboardReadiness.ts` (the line), `Dashboard.tsx` (the pill).
The connector status itself comes from `crates/stagepilot-remote` (`health.rs` checks that its
heartbeat file is fresh). `crates/stagepilot-remote/src/health_alert.rs` can post a webhook for a
stale heartbeat but nothing calls it; it is kept as a library function only.

The Python entry point that used to run it from a timer (`python -m stagepilot.remote_health_alert`)
last existed at tag `v2.41.1`. The CT120 beta machine still runs it from its old checkout; that is
separate from the app.
