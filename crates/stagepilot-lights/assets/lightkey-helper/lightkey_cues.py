"""lightkey-api: optional active-cue-name reader (the Shortcut). Install it next to lightkey_api.py.

Lightkey gives the names of the active cues only through its "Get Active Cues" Shortcuts action. This module runs
that Shortcut, publishes the result as /state and /log, and builds the Shortcut for install-shortcut. The agent
loads it only when "cues" is true in the config. Everything else works without this file.
"""
import json
import os
import plistlib
import shutil
import subprocess
import tempfile
import threading
import time
from collections import deque

DEFAULTS = {
    "shortcut": "Lightkey API Active Cues",  # name of the imported Shortcut
    "backstop_s": 10.0,              # read at least this often while Lightkey runs, even if DMX looks idle
    "quiet_gap_s": 0.25,             # min gap between reads when DMX changes are sparse
    "busy_gap_s": 1.0,               # min gap when DMX changes constantly but the cue set does not (chases, fades)
    "max_parallel_reads": 3,
    "read_timeout_s": 15,
    "sample_offset_ms": 425,         # the Shortcut samples Lightkey this long after launch (measured, +-25 ms)
    "log_size": 500,                 # cue-change events kept in memory
}

core = None          # the agent module, set by attach()
applied_start = [0]
apply_lock = threading.Lock()
fails = [0]
cue = {"active": None, "seq": 0, "changed_at": 0, "read_at": 0, "read_started_at": 0, "read_ms": 0, "reads": 0}
LOG = deque(maxlen=500)      # cue-change events, oldest first
DMXLOG = deque(maxlen=400)   # (ms, universe) of DMX frames that differed
prev_sample = [0]


def attach(agent):
    """Share the agent's state with this module, add the cue settings, and register the cue routes."""
    global core, LOG
    core = agent
    for key, value in DEFAULTS.items():
        agent.CFG.setdefault(key, value)
    LOG = deque(LOG, maxlen=agent.CFG["log_size"])
    agent.S.update({"active": None, "reads": 0, "seq": 0, "changed_at": 0, "read_at": 0, "read_started_at": 0, "read_ms": 0})
    agent.HOOKS["online"].append(on_online)
    agent.HOOKS["dmx"].append(on_dmx)
    agent.HOOKS["get"].update({"/state": get_state, "/log": get_log, "/events": get_events})
    agent.INDEX["endpoints"].update({
        "/state": "current snapshot (online, active cues, timestamps)",
        "/log?since=N": "cue changes with seq > N",
        "/events": "Server-Sent Events: 'cues' per change and 'state' snapshots",
    })
    return globals()


def _key(online, active):
    return (online, tuple(active) if active is not None else None)


def publish(**kw):
    """Update shared state; when the cue set or online flag changes, append a timestamped event."""
    S, cfg = core.S, core.CFG
    with core.cv:
        before = _key(S["online"], S["active"])
        old_active = S["active"]
        S.update(kw)
        after = _key(S["online"], S["active"])
        if before != after:
            S["seq"] += 1
            S["changed_at"] = core.now_ms()
            ev = {"seq": S["seq"], "online": S["online"], "logged_at": S["changed_at"]}
            if "read_started_at" in kw and S["active"] is not None:
                sample = kw["read_started_at"] + cfg["sample_offset_ms"]
                lo = prev_sample[0] if prev_sample[0] else sample - 1000
                dm = [t for (t, _u) in DMXLOG if lo < t <= sample + 25]
                base = set(old_active or [])
                cur = set(S["active"])
                ev.update({
                    "added": sorted(cur - base),
                    "removed": sorted(base - cur),
                    "initial": old_active is None,
                    "window_start_ms": lo,
                    "sample_ms": sample,
                    "dmx_changes_ms": dm[:20],
                    # best estimate of WHEN the change happened on this machine's clock:
                    # the first DMX frame that differed, else the moment the read sampled Lightkey
                    "effective_ms": dm[0] if dm else sample,
                    "effective_src": "dmx" if dm else "sample",
                    "read_ms": kw.get("read_ms"),
                })
                core.streak[0] = 0
            LOG.append(ev)
        if "read_started_at" in kw and S["active"] is not None:
            prev_sample[0] = max(prev_sample[0], kw["read_started_at"] + cfg["sample_offset_ms"])
        core.cv.notify_all()


def on_online(up):
    """Lightkey started or stopped (called by the agent's process watcher)."""
    if up:
        core.dirty_at[0] = time.monotonic()
        core.wake.set()
    else:
        prev_sample[0] = 0
        publish(online=False, active=None)


def on_dmx(t, universe):
    """A DMX frame differed (called by the sACN loop): note it and ask the reader for a fresh look."""
    DMXLOG.append((t, universe))
    core.dirty_at[0] = time.monotonic()
    core.wake.set()


def get_state(handler, q):
    s = core.snapshot()
    s["count"] = len(s["active"]) if s["active"] is not None else None
    return handler._json(s)


def get_log(handler, q):
    try:
        since = int(q.get("since", 0))
    except ValueError:
        return handler._json({"error": "since must be an integer"}, 400)
    with core.cv:
        evs = [e for e in LOG if e["seq"] > since]
    s = core.snapshot()
    s["count"] = len(s["active"]) if s["active"] is not None else None
    return handler._json({"events": evs, "server_time": core.now_ms(), "state": s})


def get_events(handler, q):
    handler.send_response(200)
    handler.send_header("Content-Type", "text/event-stream")
    handler.send_header("Cache-Control", "no-store")
    handler.send_header("Connection", "close")
    handler.end_headers()
    last = -1
    try:
        while True:
            with core.cv:
                core.cv.wait_for(lambda: core.S["seq"] != last, timeout=15)
                changed = core.S["seq"] != last
            if changed:
                s = core.snapshot()
                s["count"] = len(s["active"]) if s["active"] is not None else None
                with core.cv:
                    evs = [e for e in LOG if e["seq"] > last]
                last = s["seq"]
                for e in evs:
                    handler.wfile.write(("event: cues\ndata: %s\n\n" % json.dumps(dict(e, server_time=core.now_ms()))).encode())
                handler.wfile.write(("event: state\ndata: %s\n\n" % json.dumps(s)).encode())
            else:
                handler.wfile.write(b": keepalive\n\n")
            handler.wfile.flush()
    except (BrokenPipeError, ConnectionResetError, OSError):
        return


def read_active():
    """Run the Shortcut once; one output file per active cue."""
    cfg = core.CFG
    out = tempfile.mkdtemp(prefix="lkapi-")
    try:
        p = subprocess.run(["/usr/bin/shortcuts", "run", cfg["shortcut"], "-o", os.path.join(out, "o")],
                           capture_output=True, text=True, timeout=cfg["read_timeout_s"])
        err = "\n".join(l for l in p.stderr.splitlines()
                        if l.strip() and not l.startswith("ERROR: Unrecognized attribute"))
        if p.returncode != 0 or err:
            raise RuntimeError(err or ("shortcuts exited %d" % p.returncode))
        d = os.path.join(out, "o")
        names = []
        if os.path.isdir(d):
            for f in sorted(os.listdir(d)):
                try:
                    txt = open(os.path.join(d, f), encoding="utf-8").read().strip()
                except Exception:
                    txt = ""
                names.append(txt or (f[:-4] if f.endswith(".txt") else f))
        return sorted(set(names))
    finally:
        shutil.rmtree(out, ignore_errors=True)


def one_read(started, inflight):
    try:
        names = read_active()
        done = core.now_ms()
        with apply_lock:
            if started < applied_start[0]:
                return                      # a newer read already published
            applied_start[0] = started
            with core.cv:
                core.S["reads"] += 1
            publish(active=names, read_at=done, read_started_at=started, read_ms=done - started, error=None)
        fails[0] = 0
    except Exception as e:
        fails[0] += 1
        core.log.warning("read failed (%d): %s", fails[0], e)
        publish(error=str(e)[:300])
    finally:
        inflight.release()


def reader_loop():
    cfg = core.CFG
    inflight = threading.Semaphore(cfg["max_parallel_reads"])
    last_start = 0.0
    handled = 0.0
    while True:
        core.wake.wait(timeout=1.0)
        if not core.S["online"]:
            core.wake.clear()
            continue
        t = time.monotonic()
        due_dirty = core.dirty_at[0] > handled
        due_backstop = t - last_start >= cfg["backstop_s"]
        if not (due_dirty or due_backstop):
            core.wake.clear()
            continue
        gap = cfg["quiet_gap_s"] if core.streak[0] < 2 else cfg["busy_gap_s"]
        wait = max(gap - (t - last_start), 0.0)
        if fails[0]:
            wait = max(wait, min(2 ** fails[0], 15) - (t - last_start))
        if wait > 0:
            time.sleep(wait)
        if not inflight.acquire(timeout=5):
            continue
        core.wake.clear()
        handled = core.dirty_at[0]
        if due_dirty:
            core.streak[0] += 1
        last_start = time.monotonic()
        threading.Thread(target=one_read, args=(core.now_ms(), inflight), daemon=True).start()


def build_shortcut_plist():
    """The one-action Shortcut: Lightkey > Get Active Cues > output."""
    uid = "9F8A3C1E-5B7D-4C2A-8E6F-0A1B2C3D4E5F"
    action = {
        "WFWorkflowActionIdentifier": "de.monospc.Lightkey.GetActiveCuesIntent",
        "WFWorkflowActionParameters": {"UUID": uid, "IntentAppIdentifier": "de.monospc.Lightkey"},
    }
    output = {
        "WFWorkflowActionIdentifier": "is.workflow.actions.output",
        "WFWorkflowActionParameters": {"WFOutput": {
            "Value": {"string": "\ufffc", "attachmentsByRange": {"{0, 1}": {
                "OutputUUID": uid, "OutputName": "Active Cues", "Type": "ActionOutput"}}},
            "WFSerializationType": "WFTextTokenString"}},
    }
    return {
        "WFWorkflowClientVersion": "2900", "WFWorkflowClientRelease": "15.0",
        "WFWorkflowMinimumClientVersion": 900, "WFWorkflowMinimumClientVersionString": "900",
        "WFWorkflowIcon": {"WFWorkflowIconStartColor": 4282601983, "WFWorkflowIconGlyphNumber": 59511},
        "WFWorkflowTypes": [], "WFWorkflowInputContentItemClasses": [], "WFWorkflowImportQuestions": [],
        "WFWorkflowActions": [action, output],
    }


def enable_cues():
    """Persist "cues": true in the config file (the agent reads it at start-up)."""
    path = os.environ.get("LIGHTKEY_API_CONFIG", core.DEFAULT_CONFIG_PATH)
    try:
        with open(path) as f:
            cfg = json.load(f)
    except (OSError, ValueError):
        cfg = {}
    cfg["cues"] = True
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        json.dump(cfg, f, indent=2)
    print("Set \"cues\": true in %s. Restart the agent to start reading cues." % path)


def install_shortcut():
    name = core.CFG["shortcut"]
    d = tempfile.mkdtemp(prefix="lkapi-sc-")
    src = os.path.join(d, name + ".shortcut")       # `shortcuts sign` only accepts a .shortcut extension
    dst = os.path.join(d, "signed", name + ".shortcut")
    os.makedirs(os.path.dirname(dst))
    with open(src, "wb") as f:
        plistlib.dump(build_shortcut_plist(), f, fmt=plistlib.FMT_XML)
    r = subprocess.run(["/usr/bin/shortcuts", "sign", "-m", "anyone", "-i", src, "-o", dst],
                       capture_output=True, text=True)
    if not os.path.exists(dst):
        print("Could not sign the Shortcut:\n" + r.stderr[-400:])
        return 1
    subprocess.run(["/usr/bin/open", dst])
    enable_cues()
    print("The Shortcuts app should now show an 'Add Shortcut' sheet named '%s'.\n"
          "1. Click Add Shortcut.\n"
          "2. Run:  python3 lightkey_api.py check   (macOS asks once: Allow \"%s\" to access \"Lightkey\"? -> Allow)"
          % (name, name))
    return 0


def check_cues(line, running):
    """The Shortcut part of `check`: is it installed, and does it read the active cues?"""
    name = core.CFG["shortcut"]
    lst = subprocess.run(["/usr/bin/shortcuts", "list"], capture_output=True, text=True).stdout.splitlines()
    have = name in [l.strip() for l in lst]
    line(have, "Shortcut '%s' %s" % (name, "found" if have else "missing; run: python3 lightkey_api.py install-shortcut"))
    if have and running:
        t = time.time()
        try:
            names = read_active()
            line(True, "read %d active cue(s) in %.0f ms" % (len(names), (time.time() - t) * 1000))
            for n in names:
                print("        - " + n)
        except Exception as e:
            line(False, "reading active cues failed: %s\n"
                        "        Most likely the one-time approval prompt is waiting: 'Allow \"%s\" to access \"Lightkey\"?'.\n"
                        "        It can be hidden. Quit the Shortcuts app, then run this command again from a Terminal on the\n"
                        "        Mac itself (not over SSH) and click Allow. Shortcuts also needs a logged-in, unlocked desktop."
                        % (str(e).strip()[:200], name))
