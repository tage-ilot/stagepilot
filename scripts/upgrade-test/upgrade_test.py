#!/usr/bin/env python3
"""Upgrade test: released StagePilot v2.42.0 -> current main on the same data.

Local harness only: loopback ports, scratch directories, fake values. See README.md.

    upgrade_test.py --old BIN_V2420 --new BIN_MAIN --report REPORT.md

Exit code 0 when every check passes, 1 when any check fails, 2 on a harness error.
"""
import argparse
import glob
import hashlib
import json
import os
import shutil
import socket
import struct
import sys
import tempfile
import threading
import time
import uuid

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from harness import Server, free_port  # noqa: E402

CHECKS = []  # (id, scenario, description, ok|None, detail)
NOTES = {}
PIN = "2468"
REMOTE_HDR = {"X-StagePilot-Remote": "1"}
PLAN = ["demo-battle-belongs", "demo-holy-forever", "demo-gratitude", "demo-firm-foundation"]


def check(cid, scenario, desc, ok, detail=""):
    CHECKS.append((cid, scenario, desc, bool(ok), str(detail)))
    print(f"[{'PASS' if ok else 'FAIL'}] {cid} {desc}" + (f" -- {str(detail)[:300]}" if detail and not ok else ""), flush=True)
    return bool(ok)


def info(cid, scenario, desc, detail):
    """A recorded observation that is not pass/fail (a documented behaviour)."""
    CHECKS.append((cid, scenario, desc, None, str(detail)))
    print(f"[INFO] {cid} {desc} -- {str(detail)[:300]}", flush=True)


# ------------------------------------------------------------------------------ OSC listener


def parse_osc(data):
    def pad(n):
        return (n + 3) & ~3

    end = data.index(b"\0")
    address = data[:end].decode()
    pos = pad(end + 1)
    if pos >= len(data) or data[pos:pos + 1] != b",":
        return address, ()
    tend = data.index(b"\0", pos)
    tags = data[pos + 1:tend].decode()
    pos = pad(tend + 1)
    args = []
    for tag in tags:
        if tag == "i":
            args.append(struct.unpack(">i", data[pos:pos + 4])[0]); pos += 4
        elif tag == "f":
            args.append(round(struct.unpack(">f", data[pos:pos + 4])[0], 4)); pos += 4
        elif tag == "d":
            args.append(round(struct.unpack(">d", data[pos:pos + 8])[0], 4)); pos += 8
        elif tag == "s":
            e = data.index(b"\0", pos)
            args.append(data[pos:e].decode()); pos = pad(e + 1)
        elif tag in "TF":
            args.append(tag == "T")
        elif tag == "N":
            args.append(None)
    return address, tuple(args)


class OscListener:
    """A loopback UDP socket standing in for Lightkey's OSC input. Never a real Lightkey."""

    def __init__(self):
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.sock.bind(("127.0.0.1", 0))
        self.sock.settimeout(0.2)
        self.port = self.sock.getsockname()[1]
        self.packets = []
        self._stop = False
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    def _run(self):
        while not self._stop:
            try:
                data, _ = self.sock.recvfrom(65535)
            except socket.timeout:
                continue
            except OSError:
                return
            try:
                self.packets.append((time.time(), *parse_osc(data)))
            except Exception:
                self.packets.append((time.time(), "<undecodable>", ()))

    def mark(self):
        return len(self.packets)

    def since(self, mark):
        return [(a, g) for (_, a, g) in self.packets[mark:]]

    def close(self):
        self._stop = True
        self.sock.close()


# ------------------------------------------------------------------------------ helpers


def sha(path):
    try:
        return hashlib.sha256(open(path, "rb").read()).hexdigest()
    except OSError:
        return None


def read_json(path):
    with open(path) as f:
        return json.load(f)


def corrupt_files(d):
    return sorted(os.path.basename(p) for p in glob.glob(os.path.join(d, "settings.json.corrupt-*")))


def flat(value, prefix=""):
    out = {}
    if isinstance(value, dict):
        if not value:
            out[prefix] = {}
        for k, v in value.items():
            out.update(flat(v, f"{prefix}.{k}" if prefix else k))
    elif isinstance(value, list):
        if not value:
            out[prefix] = []
        for i, v in enumerate(value):
            out.update(flat(v, f"{prefix}[{i}]"))
    else:
        out[prefix] = value
    return out


def num_eq(a, b):
    if isinstance(a, (int, float)) and isinstance(b, (int, float)) and not isinstance(a, bool) and not isinstance(b, bool):
        return abs(a - b) < 1e-9
    return a == b


def settings_diff(before, after):
    """(changed, removed, added) between two settings dicts; `before` is what the user had."""
    fb, fa = flat(before), flat(after)
    changed, removed, added = [], [], []
    for k, v in fb.items():
        if k not in fa:
            removed.append((k, v))
        elif not num_eq(v, fa[k]):
            changed.append((k, v, fa[k]))
    for k, v in fa.items():
        if k not in fb:
            added.append((k, v))
    return changed, removed, added


def get_settings(srv):
    return srv.call("GET", "/api/v1/settings")


def login(srv, pin=PIN):
    return srv.call("POST", "/api/v1/dashboard-auth/login", {"pin": pin})


def osc_seq(packets):
    return [(a, tuple(g)) for a, g in packets if a.startswith("/fixture/") or a.startswith("/test/")]


def play_song(srv, osc, seconds):
    """start_next on the demo plan, collect OSC for `seconds`, stop the timer."""
    m = osc.mark()
    st, _ = srv.call("POST", "/api/v1/actions/start_next")
    time.sleep(seconds)
    srv.call("POST", "/api/v1/actions/stop_timer")
    time.sleep(0.3)
    return st, osc.since(m)


def cue(at, address, value, label="", **extra):
    row = {"id": str(uuid.uuid4()), "at_seconds": at, "address": address,
           "args": [{"type": "float", "value": value}], "label": label}
    row.update(extra)
    return row


# ------------------------------------------------------------------------------ test data


def manual_cue_map_a():
    return {
        "song_key": PLAN[0], "song_title": "Battle Belongs", "cues": [], "method": "osc",
        "osc_cues": [
            cue(0.4, "/test/manual/dimmer", 0.25, "Verse"),
            cue(0.9, "/test/manual/dimmer", 0.75, "Chorus"),
            cue(1.4, "/test/manual/color", 0.5, "Bridge"),
        ],
    }


def manual_cue_map_b():
    return {
        "song_key": PLAN[1], "song_title": "Holy Forever", "cues": [], "method": "osc",
        "osc_cues": [cue(0.5, "/test/manual/strobe", 1.0, "Hit")],
    }


def pre_release_rows():
    """Old-style thinned captured rows, as the mainline build before smooth capture wrote them:
    `source: captured`, a multi-message `messages` look, no `fade_seconds`, one hand-edited."""
    def look(at, fixture_values, edited=False):
        msgs = [{"address": f"/fixture/{name}/overrides/dimmer",
                 "args": [{"type": "float", "value": v}]} for name, v in fixture_values]
        row = {"id": str(uuid.uuid4()), "at_seconds": at, "address": msgs[0]["address"],
               "args": msgs[0]["args"], "label": f"Look - {len(msgs)} fixtures",
               "messages": msgs, "source": "captured", "color": None}
        if edited:
            row["edited"] = True
        return row
    return [look(1.0, [("MH1", 0.2), ("MH2", 0.2)]), look(1.5, [("MH1", 0.8), ("MH2", 0.6)], edited=True),
            look(2.0, [("MH1", 0.4), ("MH2", 0.4)])]


def takes_file_v242():
    """The shape v2.42.0's FileTakeStore writes (`{version, takes:[...]}`)."""
    return {"version": 1, "takes": [{
        "song_key": PLAN[2], "song_title": "Gratitude", "recorded_at": "2026-10-20T18:30:00Z",
        "duration_seconds": 352.0, "initial": ["Cue 1"],
        "edges": [{"at_seconds": 12.5, "added": ["Cue 2"], "removed": []},
                  {"at_seconds": 40.0, "added": [], "removed": ["Cue 2"]}],
    }]}


def create_user_data(srv, osc_port, closed_port, sc):
    """Everything a v2.42.0 user could have, made through v2.42.0's own HTTP API."""
    st, _ = srv.call("POST", "/api/v1/dashboard-auth/settings", {"enabled": True, "pin": PIN})
    check(f"{sc}.seed.pin", sc, "v2.42.0: set dashboard PIN", st == 200, st)
    st, _ = login(srv)
    check(f"{sc}.seed.login", sc, "v2.42.0: PIN login works", st == 200, st)
    st, _ = srv.call("POST", "/api/v1/remote-access/bootstrap",
                     {"email": "admin@example.test", "password": "correct-horse-battery"}, REMOTE_HDR)
    check(f"{sc}.seed.user1", sc, "v2.42.0: first user (Operator) created", st == 201, st)
    st, _ = srv.call("POST", "/api/v1/remote-auth/users",
                     {"email": "volunteer@example.test", "password": "another-long-pass-1", "role": "Viewer"}, REMOTE_HDR)
    check(f"{sc}.seed.user2", sc, "v2.42.0: second user (Viewer) created", st == 201, st)

    st, cur = srv.call("GET", "/api/v1/settings")
    s = cur["settings"]
    s["timezone"] = "America/Chicago"
    s["planning_center"].update({"app_id": "fake-app-id-0000", "service_type_id": "111222",
                                 "plan_title_preference": "Sunday", "upcoming_lookahead_days": 21})
    s["midi"].update({"channel": 3, "note": 61})
    # The learned song order can only be written by Discover Song Order (it needs a real
    # playback app), so it is put into the file by hand below, in the shape Discover writes.
    s["playback_api"].update({"host": "192.0.2.50", "port": 8080, "auto_scan": True, "fast_transport": False})
    s["propresenter"].update({"host": "192.0.2.60", "port": 50001, "timer_name": "Service Timer"})
    s["lights"].update({"enabled": True, "method": "osc", "osc_host": "127.0.0.1", "osc_port": osc_port,
                        "pulse_ms": 150})
    st, body = srv.call("PUT", "/api/v1/settings", s)
    check(f"{sc}.seed.settings", sc, "v2.42.0: Services/Playback/Presentation/Lighting settings saved",
          st == 200, f"{st} {str(body)[:300]}")
    for name, cmap in (("A", manual_cue_map_a()), ("B", manual_cue_map_b())):
        st, body = srv.call("PUT", "/api/v1/lights/cue-map", cmap)
        check(f"{sc}.seed.cuemap{name}", sc, f"v2.42.0: cue map {name} saved", st == 200, f"{st} {str(body)[:300]}")
    pads = {"pads": [
        {"id": str(uuid.uuid4()), "label": "Blackout", "method": "osc", "address": "/test/pad/blackout",
         "args": [{"type": "float", "value": 0.0}], "color": "rose"},
        {"id": str(uuid.uuid4()), "label": "Full", "method": "osc", "address": "/test/pad/full",
         "args": [{"type": "int", "value": 1}]}]}
    st, body = srv.call("PUT", "/api/v1/lights/pads", pads)
    check(f"{sc}.seed.pads", sc, "v2.42.0: two pads saved", st == 200, f"{st} {str(body)[:300]}")
    st, body = srv.call("PUT", "/api/v1/lights/lightkey/settings", {"api_url": f"127.0.0.1:{closed_port}", "replay": True})
    check(f"{sc}.seed.lightkey", sc, "v2.42.0: Lightkey link settings saved", st == 200, f"{st} {str(body)[:300]}")


# ------------------------------------------------------------------------------ scenarios


def scenario_upgrade(args, root, level):
    sc = f"upgrade-{level}"
    d = os.path.join(root, sc)
    os.makedirs(d)
    osc = OscListener()
    closed = free_port()
    try:
        sp = os.path.join(d, "settings.json")
        json.dump({"web_dashboard_pin_enabled": False}, open(sp, "w"))
        remote = os.path.join(d, "remote")
        env = {"STAGEPILOT_DESKTOP_REMOTE_ROOT": remote, "STAGEPILOT_CLOUDFLARED_BINARY": "/bin/false"}
        takes_path = os.path.join(d, "lightkey-takes.json")
        json.dump(takes_file_v242(), open(takes_path, "w"), indent=1)

        port = free_port()  # the same port for both builds: the port is part of the effective settings
        old = Server("v2.42.0", args.old, d, port=port, extra_env=env)
        if not check(f"{sc}.old.start", sc, "v2.42.0 starts on the seeded directory", old.start()):
            print(old.log_text()[-2000:])
            return
        create_user_data(old, osc.port, closed, sc)
        # Lighting output settings are read at start: restart v2.42.0 on its own file, as a user would.
        check(f"{sc}.old.stop1", sc, "v2.42.0 stops cleanly after the data is created (exit code 0)", old.stop() == 0)
        old = Server("v2.42.0", args.old, d, port=port, extra_env=env)
        check(f"{sc}.old.restart", sc, "v2.42.0 restarts on the file it wrote", old.start())
        login(old)
        st, lk = old.call("GET", "/api/v1/lights/lightkey")
        takes = lk.get("takes") if isinstance(lk, dict) else None
        check(f"{sc}.old.takes", sc, "v2.42.0 reads lightkey-takes.json (control: the fixture is a valid takes file)",
              isinstance(takes, list) and len(takes) == 1 and not os.path.exists(takes_path + ".bad"),
              f"takes={str(takes)[:200]} bad_exists={os.path.exists(takes_path + '.bad')} keys={list(lk)[:20] if isinstance(lk, dict) else lk}")
        st, base_a = play_song(old, osc, 2.6)
        base_a = osc_seq(base_a)
        check(f"{sc}.old.replayA", sc, "v2.42.0 replays song A over loopback OSC (baseline, 3 rows)", len(base_a) == 3, base_a)
        st, r = old.call("PUT", "/api/v1/lights/lightkey/settings", {"detail_level": level})
        info(f"{sc}.old.detail", sc, f"v2.42.0 given detail_level={level}", f"HTTP {st} {str(r)[:160]}")
        code = old.stop()
        check(f"{sc}.old.stop", sc, "v2.42.0 stops cleanly (exit code 0)", code == 0, code)

        # What mainline builds between v2.42.0 and now wrote and v2.42.0 cannot: thinned captured
        # rows (source=captured, no fade_seconds) and a stored Detail level.
        data = read_json(sp)
        cm = data["lights"]["cue_maps"][PLAN[1]]
        cm["osc_cues"] = sorted(cm["osc_cues"] + pre_release_rows(), key=lambda r: r["at_seconds"])
        cm["capture"] = {"complete": False, "coverage_seconds": 2.0, "recorded_at": "2026-11-01T10:00:00Z"}
        data["lights"]["lightkey"]["detail"] = level
        data["playback_api"].update({
            "song_order": [3, 1, 4, 2], "captured_version": 7, "scan_plan_key": "demo-service-plan",
            "captured_at": "2026-10-30T15:00:00Z", "song_lengths": [281.0, 336.0, 352.0, 377.0],
            "skipped_song_ids": [3], "skipped_plan_song_ids": ["demo-gratitude"]})
        json.dump(data, open(sp, "w"), indent=1)
        before = read_json(sp)
        sha_file, sha_takes = sha(sp), sha(takes_path)
        sha_users = sha(os.path.join(remote, "identity.sqlite3"))

        new = Server("main", args.new, d, port=port, extra_env=env)
        if not check(f"{sc}.new.start", sc, "main starts on the same files", new.start()):
            print(new.log_text()[-3000:])
            return
        check(f"{sc}.new.noquarantine", sc, "no settings.json.corrupt-* file created", not corrupt_files(d), corrupt_files(d))
        st, _ = get_settings(new)
        check(f"{sc}.new.pin401", sc, "PIN still required (settings refused without a session)", st == 401, st)
        st, _ = login(new, "0000")
        check(f"{sc}.new.pinwrong", sc, "wrong PIN refused", st == 401, st)
        st, _ = login(new)
        check(f"{sc}.new.pinok", sc, "the PIN set on v2.42.0 logs in on main", st == 200, st)
        st, body = get_settings(new)
        check(f"{sc}.new.settings", sc, "GET /api/v1/settings answers 200", st == 200, st)
        check(f"{sc}.new.notdegraded", sc, "settings not degraded (no fall-back to defaults)",
              body.get("settings_degraded") is False, body.get("settings_degraded_reason"))
        after = body["settings"]
        changed, removed, added = settings_diff({k: v for k, v in before.items() if k != "web_dashboard_pin_hash"}, after)
        check(f"{sc}.new.diff", sc, "every pre-existing settings key has the same value on main",
              not changed and not removed, f"changed={changed[:5]} removed={removed[:5]}")
        NOTES.setdefault("settings_added", {})[level] = added
        NOTES.setdefault("settings_keys_compared", {})[level] = len(flat(before))
        cm_before = before["lights"]["cue_maps"]
        cmd = settings_diff(cm_before, after["lights"]["cue_maps"])
        check(f"{sc}.new.cuemaps", sc, "old cue maps (manual and captured rows) load unchanged",
              cm_before == after["lights"]["cue_maps"], f"changed={cmd[0][:6]} removed={cmd[1][:6]} added={cmd[2][:6]}")
        check(f"{sc}.new.pads", sc, "pads load unchanged", before["lights"]["pads"] == after["lights"]["pads"])
        check(f"{sc}.new.order", sc, "learned/scanned song order data preserved (every playback_api value unchanged)",
              not any(settings_diff(before["playback_api"], after["playback_api"])[:2]),
              settings_diff(before["playback_api"], after["playback_api"]))
        info(f"{sc}.new.order.kind", sc, "learned order: preserved or rebuilt?",
             f"scanned order preserved as stored and it takes precedence over passive learning (song_order={after['playback_api']['song_order']}, "
             f"captured_version={after['playback_api']['captured_version']}); nothing is rescanned")
        check(f"{sc}.new.file.untouched", sc, "starting main without saving does not rewrite settings.json",
              sha(sp) == sha_file, "file changed on start")
        st, lk = new.call("GET", "/api/v1/lights/lightkey")
        check(f"{sc}.new.detail.echo", sc, f"stored Detail ({level}) is still echoed as detail_level",
              isinstance(lk, dict) and lk.get("detail_level") == level, lk.get("detail_level") if isinstance(lk, dict) else lk)
        other = "coarse" if level == "fine" else "fine"
        st, put = new.call("PUT", "/api/v1/lights/lightkey/settings", {"detail_level": other})
        check(f"{sc}.new.detail.accept", sc, f"PUT detail_level={other} is accepted (200)", st == 200, f"{st} {str(put)[:200]}")
        st, g = get_settings(new)
        check(f"{sc}.new.detail.ignored", sc, "Detail does not change any cue map (ignored)",
              g["settings"]["lights"]["cue_maps"] == cm_before, settings_diff(cm_before, g["settings"]["lights"]["cue_maps"]))
        st, put = new.call("PUT", "/api/v1/lights/lightkey/settings", {"detail_level": level})
        check(f"{sc}.new.detail.restore", sc, f"PUT detail_level={level} accepted", st == 200, st)
        issues = new.log_issues()
        check(f"{sc}.new.takes", sc, "lightkey-takes.json is left untouched and no log line mentions it",
              sha(takes_path) == sha_takes and not os.path.exists(takes_path + ".bad")
              and not any("take" in i.lower() for i in issues), [i for i in issues if "take" in i.lower()])
        info(f"{sc}.new.takes.use", sc, "captures stored only in lightkey-takes.json",
             "not read and not migrated: a song whose capture lives only in the old takes file has no replay on main")
        st, users = new.call("GET", "/api/v1/remote-auth/users", headers=REMOTE_HDR)
        got = sorted((u["email"], u["role"], u["enabled"]) for u in users) if isinstance(users, list) else users
        check(f"{sc}.new.users", sc, "both users still listed with the same roles",
              got == [("admin@example.test", "Operator", True), ("volunteer@example.test", "Viewer", True)], got)
        check(f"{sc}.new.users.file", sc, "user store file unchanged on disk",
              sha(os.path.join(remote, "identity.sqlite3")) == sha_users, "identity.sqlite3 changed")
        st, got_a = play_song(new, osc, 2.6)
        got_a = osc_seq(got_a)
        check(f"{sc}.new.replayA", sc, "song A replays on main with the same OSC messages in the same order as v2.42.0",
              got_a == base_a, f"old={base_a} new={got_a}")
        st, got_b = play_song(new, osc, 3.0)
        got_b = osc_seq(got_b)
        info(f"{sc}.new.replayB", sc, "song B (1 manual row + 3 old captured rows, capture incomplete) on main, no Lightkey link",
             f"{len(got_b)} OSC messages: {got_b}")
        check(f"{sc}.new.replayB.manual", sc, "song B's manual row is still sent", ("/test/manual/strobe", (1.0,)) in got_b, got_b)
        NOTES.setdefault("log_warnings", {})[f"{sc} main"] = new.log_issues()
        st, put = new.call("PUT", "/api/v1/lights/lightkey/settings", {"replay": True})
        check(f"{sc}.new.save", sc, "main can save settings on the upgraded file", st == 200, st)
        code = new.stop()
        check(f"{sc}.new.stop", sc, "main stops cleanly (exit code 0)", code == 0, code)
        check(f"{sc}.new.noquarantine2", sc, "still no .corrupt file after save and stop", not corrupt_files(d), corrupt_files(d))
        NOTES.setdefault("log_warnings", {})[f"{sc} v2.42.0"] = old.log_issues()

        down = Server("v2.42.0-again", args.old, d, port=port, extra_env=env)
        ok = check(f"{sc}.down.start", sc, "downgrade: v2.42.0 starts on the file main saved", down.start())
        if ok:
            login(down)
            st, g = get_settings(down)
            check(f"{sc}.down.settings", sc, "downgrade: v2.42.0 reads the settings (not degraded)",
                  st == 200 and g.get("settings_degraded") is False, st)
            check(f"{sc}.down.noquarantine", sc, "downgrade: no .corrupt file", not corrupt_files(d), corrupt_files(d))
            st, put = down.call("PUT", "/api/v1/lights/lightkey/settings", {"replay": False})  # a real change, so it saves
            on_disk = read_json(sp)
            rows = on_disk["lights"]["cue_maps"][PLAN[1]]["osc_cues"]
            kept_new = [r for r in rows if "messages" in r or "source" in r]
            info(f"{sc}.down.save", sc, "downgrade: v2.42.0 saves over the old-format captured rows",
                 f"HTTP {st}; rows {len(rows)} of {len(before['lights']['cue_maps'][PLAN[1]]['osc_cues'])}; "
                 f"rows still carrying messages/source: {len(kept_new)}; lightkey.detail on disk: "
                 f"{on_disk['lights']['lightkey'].get('detail')!r}")
            NOTES.setdefault("log_warnings", {})[f"{sc} v2.42.0 again"] = down.log_issues()
        down.stop()
    finally:
        osc.close()


def scenario_downgrade_new_fields(args, root):
    """main writes fade_seconds + effects, then v2.42.0 starts on that file."""
    sc = "downgrade-newfields"
    d = os.path.join(root, sc)
    os.makedirs(d)
    json.dump({"web_dashboard_pin_enabled": False}, open(os.path.join(d, "settings.json"), "w"))
    new = Server("main", args.new, d)
    if not check(f"{sc}.new.start", sc, "main starts", new.start()):
        return
    effect = {"id": str(uuid.uuid4()), "label": "Pan sweep", "start_seconds": 4.0, "end_seconds": 20.0,
              "period_seconds": 4.0, "members": [{"fixture": "MH1"}, {"fixture": "MH2", "offset_seconds": 0.5}],
              "cycle": [{"at_seconds": 0.0, "property": "panAngle", "value": [30.0], "fade_seconds": 2.0},
                        {"at_seconds": 2.0, "property": "panAngle", "value": [-30.0], "fade_seconds": 2.0}],
              "source": "captured"}
    msgs = [{"address": "/fixture/MH1/overrides/dimmer", "args": [{"type": "float", "value": 0.5}]},
            {"address": "/fixture/MH2/overrides/dimmer", "args": [{"type": "float", "value": 0.5}]}]
    cmap = {"song_key": PLAN[0], "song_title": "Battle Belongs", "cues": [], "method": "osc",
            "osc_cues": [cue(1.0, "/fixture/MH1/overrides/dimmer", 0.5, "Ramp", fade_seconds=1.5, source="captured", messages=msgs),
                         cue(8.0, "/test/manual/dimmer", 0.1, "Manual")],
            "effects": [effect],
            "capture": {"complete": True, "coverage_seconds": 270.0, "recorded_at": "2026-11-04T10:00:00Z"}}
    st, body = new.call("PUT", "/api/v1/lights/cue-map", cmap)
    check(f"{sc}.new.put", sc, "main saves a cue map with fade_seconds, effects, messages and source", st == 200, f"{st} {str(body)[:300]}")
    # `effects` and `capture` normally come from a recording (needs Lightkey); the full settings
    # route accepts them as well, which is how this test gets them onto disk.
    login(new)
    st, cur = get_settings(new)
    full = cur["settings"]
    full["lights"]["cue_maps"][PLAN[0]]["effects"] = [effect]
    full["lights"]["cue_maps"][PLAN[0]]["capture"] = cmap["capture"]
    st, body = new.call("PUT", "/api/v1/settings", full)
    check(f"{sc}.new.putfull", sc, "main saves effects and capture through the full settings route", st == 200, f"{st} {str(body)[:300]}")
    cm = read_json(new.settings_path)["lights"]["cue_maps"].get(PLAN[0], {})
    check(f"{sc}.new.disk", sc, "main wrote fade_seconds and effects to settings.json",
          bool(cm.get("effects")) and any("fade_seconds" in r for r in cm.get("osc_cues", [])), list(cm.keys()))
    new.stop()
    shutil.copy(new.settings_path, os.path.join(d, "settings.main-written.json"))

    old = Server("v2.42.0", args.old, d)
    ok = check(f"{sc}.old.start", sc, "v2.42.0 starts on a file containing fade_seconds / effects / messages / source", old.start())
    if ok:
        st, g = get_settings(old)
        degraded = isinstance(g, dict) and g.get("settings_degraded")
        check(f"{sc}.old.notdegraded", sc, "v2.42.0 does not treat the file as corrupt or newer (no fall-back to defaults)",
              st == 200 and not degraded, f"{st} {g.get('settings_degraded_reason') if isinstance(g, dict) else g}")
        check(f"{sc}.old.noquarantine", sc, "v2.42.0 creates no .corrupt file", not corrupt_files(d), corrupt_files(d))
        cm_old = g["settings"]["lights"]["cue_maps"].get(PLAN[0], {}) if st == 200 else {}
        info(f"{sc}.old.view", sc, "what v2.42.0 reports for that cue map (read only)",
             f"cue map keys={sorted(cm_old.keys())}; row keys={sorted(set(k for r in cm_old.get('osc_cues', []) for k in r))}")
        info(f"{sc}.old.fileuntouched", sc, "v2.42.0 start alone leaves settings.json unchanged",
             sha(old.settings_path) == sha(os.path.join(d, "settings.main-written.json")))
        st, put = old.call("PUT", "/api/v1/lights/lightkey/settings", {"replay": False})  # a real change, so it saves
        cm_after = read_json(old.settings_path)["lights"]["cue_maps"].get(PLAN[0], {})
        lost = [f"cue map `{k}` dropped" for k in ("effects", "capture") if k in cm and k not in cm_after]
        kb = set(k for r in cm.get("osc_cues", []) for k in r)
        ka = set(k for r in cm_after.get("osc_cues", []) for k in r)
        lost += [f"row field `{k}` dropped" for k in sorted(kb - ka)]
        info(f"{sc}.old.save", sc, "after v2.42.0 saves any setting, what is left of main's data",
             f"HTTP {st}; rows {len(cm_after.get('osc_cues', []))}/{len(cm.get('osc_cues', []))}; " + ("; ".join(lost) or "nothing lost"))
        # Unknown keys are written back by index path: what happens when v2.42.0 deletes the first row?
        survivor = cue(8.0, "/test/manual/dimmer", 0.1, "Manual")
        manual_row = [r for r in cm_after.get("osc_cues", []) if r.get("address") == "/test/manual/dimmer"]
        if manual_row:
            survivor = {k: manual_row[0][k] for k in ("id", "at_seconds", "address", "args", "label", "color") if k in manual_row[0]}
        st2, _ = old.call("PUT", "/api/v1/lights/cue-map", {"song_key": PLAN[0], "song_title": "Battle Belongs",
                                                           "cues": [], "method": "osc", "osc_cues": [survivor]})
        rows2 = read_json(old.settings_path)["lights"]["cue_maps"].get(PLAN[0], {}).get("osc_cues", [])
        moved = sorted(k for k in (rows2[0] if rows2 else {}) if k in ("fade_seconds", "source", "messages", "edited"))
        info(f"{sc}.old.rowdelete", sc, "v2.42.0 deletes the first row of a cue map that holds main-only row fields",
             f"HTTP {st2}; remaining row is the Manual row; main-only fields now on it: {moved or 'none'}")
        NOTES["downgrade_rowshift"] = moved
        NOTES["downgrade_lost"] = lost
        NOTES["downgrade_rows_before"] = len(cm.get("osc_cues", []))
        NOTES["downgrade_rows_after"] = len(cm_after.get("osc_cues", []))
        NOTES.setdefault("log_warnings", {})[f"{sc} v2.42.0"] = old.log_issues()
    old.stop()
    new2 = Server("main-again", args.new, d)
    if check(f"{sc}.back.start", sc, "re-upgrade: main starts on the file v2.42.0 saved", new2.start()):
        login(new2)
        st, g = get_settings(new2)
        cm_back = g["settings"]["lights"]["cue_maps"].get(PLAN[0], {}) if st == 200 else {}
        info(f"{sc}.back.view", sc, "main on the file v2.42.0 saved",
             f"effects={len(cm_back.get('effects', []))} rows={len(cm_back.get('osc_cues', []))} "
             f"rows with fade_seconds={sum(1 for r in cm_back.get('osc_cues', []) if r.get('fade_seconds'))}")
        check(f"{sc}.back.noquarantine", sc, "no .corrupt file", not corrupt_files(d), corrupt_files(d))
        new2.stop()


def scenario_fresh(args, root):
    sc = "fresh"
    for label, binary in (("v2.42.0", args.old), ("main", args.new)):
        d = os.path.join(root, f"fresh-{label}")
        os.makedirs(d)
        srv = Server(label, binary, d)
        if not check(f"{sc}.{label}.start", sc, f"{label} starts with no files at all", srv.start()):
            continue
        st, h = srv.call("GET", "/api/v1/health")
        check(f"{sc}.{label}.health", sc, f"{label} fresh install is healthy", st == 200 and h.get("status") == "healthy", h)
        st, acc = srv.call("GET", "/api/v1/access")
        info(f"{sc}.{label}.pin", sc, f"{label} fresh install access",
             f"mode={acc.get('mode')} authentication={acc.get('authentication')}; GET /settings without a session -> {srv.call('GET', '/api/v1/settings')[0]}")
        check(f"{sc}.{label}.noquarantine", sc, f"{label} fresh install writes no .corrupt file", not corrupt_files(d), corrupt_files(d))
        NOTES.setdefault("log_warnings", {})[f"fresh {label}"] = srv.log_issues()
        srv.stop()
        NOTES.setdefault("fresh_files", {})[label] = sorted(f for f in os.listdir(d) if not f.endswith(".log"))
    d = os.path.join(root, "fresh-main-seeded")
    os.makedirs(d)
    json.dump({"web_dashboard_pin_enabled": False}, open(os.path.join(d, "settings.json"), "w"))
    srv = Server("main", args.new, d)
    if check(f"{sc}.seeded.start", sc, "main starts on a minimal seeded settings.json", srv.start()):
        st, g = get_settings(srv)
        s = g["settings"]
        check(f"{sc}.seeded.defaults", sc, "defaults are filled in (schema_version 2, osc, Lightkey replay on)",
              s["schema_version"] == 2 and s["lights"]["method"] == "osc" and s["lights"]["lightkey"]["replay"] is True, "")
        st, put = srv.call("PUT", "/api/v1/lights/lightkey/settings", {"detail_level": "fine"})
        check(f"{sc}.seeded.detail", sc, "fresh main accepts detail_level", st == 200, st)
        srv.stop()
        check(f"{sc}.seeded.noquarantine", sc, "no .corrupt file", not corrupt_files(d), corrupt_files(d))


# ------------------------------------------------------------------------------ report


def render_report(args):
    pass_n = sum(1 for c in CHECKS if c[3] is True)
    fail_n = sum(1 for c in CHECKS if c[3] is False)
    info_n = sum(1 for c in CHECKS if c[3] is None)
    L = ["# StagePilot upgrade test: v2.42.0 -> main\n",
         f"Run: {time.strftime('%Y-%m-%d %H:%M:%S %Z')}  ",
         f"Old build: `{args.old_label}`  New build: `{args.new_label}`\n",
         f"**Result: {pass_n} PASS, {fail_n} FAIL, {info_n} recorded observations.**\n",
         "## Checks\n", "| Check | Description | Result | Detail |", "|---|---|---|---|"]
    for cid, sc, desc, ok, detail in CHECKS:
        res = "PASS" if ok is True else ("FAIL" if ok is False else "INFO")
        det = "" if ok is True else (detail or "").replace("|", "\\|").replace("\n", " ")[:500]
        L.append(f"| `{cid}` | {desc} | {res} | {det} |")
    L.append("\n## Settings diff (data saved by v2.42.0 -> main's GET /api/v1/settings)\n")
    for level in ("fine", "coarse"):
        added = NOTES.get("settings_added", {}).get(level)
        if added is None:
            continue
        L.append(f"Run with stored `detail: {level}`: {NOTES['settings_keys_compared'][level]} existing leaf values compared; changes are listed in the checks above (none when `new.diff` passes).\n")
        L.append("Keys main adds that the file did not have:\n")
        L.extend([f"- `{k}` = `{json.dumps(v)}`" for k, v in added] or ["- none"])
        L.append("")
    L.append("## Log warnings and errors\n")
    for label, lines in NOTES.get("log_warnings", {}).items():
        L.extend([f"**{label}**", "```", *(lines or ["(no WARN or ERROR lines)"]), "```"])
    L.append("\n## Downgrade (data written by main, then v2.42.0)\n")
    L.append(f"- Cue-map rows before v2.42.0 saved: {NOTES.get('downgrade_rows_before')}; after: {NOTES.get('downgrade_rows_after')}.")
    L.append("- Lost when v2.42.0 saves: " + ("; ".join(NOTES.get("downgrade_lost", [])) or "nothing") + ".")
    L.append(f"- Main-only row fields that ended up on a different row after v2.42.0 deleted the first row: {NOTES.get('downgrade_rowshift') or 'none'}.")
    L.append("\n## Files left by a fresh install\n")
    L.extend(f"- {label}: {files}" for label, files in NOTES.get("fresh_files", {}).items())
    return "\n".join(L) + "\n", fail_n


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--old", required=True, help="stagepilot-headless built from tag v2.42.0")
    ap.add_argument("--new", required=True, help="stagepilot-headless built from main")
    ap.add_argument("--report", required=True)
    ap.add_argument("--old-label", default="v2.42.0")
    ap.add_argument("--new-label", default="main")
    ap.add_argument("--keep", action="store_true", help="keep the scratch directories")
    args = ap.parse_args()
    root = tempfile.mkdtemp(prefix="sp-upgrade-test-")
    code = 2
    try:
        for level in ("fine", "coarse"):
            scenario_upgrade(args, root, level)
        scenario_downgrade_new_fields(args, root)
        scenario_fresh(args, root)
        text, fails = render_report(args)
        os.makedirs(os.path.dirname(os.path.abspath(args.report)), exist_ok=True)
        open(args.report, "w").write(text)
        print(f"\nreport: {args.report}\n{'FAILED' if fails else 'OK'}: {sum(1 for c in CHECKS if c[3] is True)} pass, {fails} fail")
        code = 1 if fails else 0
    finally:
        if args.keep:
            print("scratch kept:", root)
        else:
            shutil.rmtree(root, ignore_errors=True)
    return code


if __name__ == "__main__":
    sys.exit(main())
