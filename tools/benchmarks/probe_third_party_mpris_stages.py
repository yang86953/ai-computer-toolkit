#!/usr/bin/env python3
"""Stage-attribution and cold-start probe against installed Haruna, never the user's player.

Reuses the private bwrap PID/network/mount namespace, user-bus, Xvfb and silent
synthetic video method of tools/benchmarks/benchmark_third_party_mpris.py. All
playback writes go through ai-computer-toolkit AND an identity gate: immediately
before every write the gate re-reads the bound MPRIS name's owner bus PID from
the live bus and refuses the write unless it equals this run's own player child
PID. On mismatch or unknown owner nothing is sent at all — discovery and reads
continue, and cleanup only ever signals this run's own child handles. The stages
mode proves the refusal with a minimal negative counter-example whose audit
window (dbus-monitor over the counter-example arms only) must observe zero
org.mpris.MediaPlayer2.Player method calls. Host-load and player-behaviour
observations from earlier runs are correlates seen on those runs, not isolated
root-cause proofs; long evidence (120 s paced, 30 min soak, 5-round cold start)
stays historical and is not re-collected by short probe runs.

Modes:
  stages    settled layered latency decomposition: exec/CLI-boot/raw-bus baselines,
            discover, independent state read, and confirmed control transactions.
            Every sample is phase-tagged: "startup" covers player spawn through
            discovery polling and the settled precondition; "steady" is the
            attribution window after it. Distributions report the raw full set
            ("all") and each phase separately, so a filtered subset is never
            presented as the full set.
  coldstart per-round fresh player: observe first MPRIS target and first "playing",
            then immediately confirm-Pause (attempt A, retry B), compare with the
            legacy 10 s settled Pause (attempt C). No fixed sleep substitutes for
            readiness inside the boundary itself; the 10 s arm is the historical
            steady-state precondition kept only as the contrast baseline. A round
            whose identity is not proven records refusals instead of writes and
            still cleans up through its own child handle.

Results/logs must stay in this source repository's target directory.
"""

import argparse
import hashlib
import json
import math
import os
import pathlib
import statistics
import subprocess
import sys
import time


class IdentityRefused(RuntimeError):
    """A control write was refused because identity is unknown or mismatched."""


DECOY_SOURCE = """import sys
from gi.repository import GLib, Gio

def owned(connection, name):
    print("owned", flush=True)

Gio.bus_own_name(Gio.BusType.SESSION, sys.argv[1], Gio.BusNameOwnerFlags.NONE,
                 owned, None, None)
GLib.MainLoop().run()
"""


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def distribution(samples):
    if not samples:
        return {"count": 0}
    ordered = sorted(samples)
    return {"count": len(ordered), "p50_ms": statistics.median(ordered),
            "p95_ms": ordered[math.ceil(len(ordered) * .95) - 1],
            "p99_ms": ordered[math.ceil(len(ordered) * .99) - 1],
            "max_ms": ordered[-1]}


def inside(config):
    if pathlib.Path(__file__).resolve() != pathlib.Path("/runner") or os.getpid() > 10:
        raise RuntimeError("the inner runner must be started in the private bwrap PID/mount namespace")
    root = pathlib.Path("/work")
    env = dict(os.environ, HOME="/work/home", XDG_RUNTIME_DIR=config["runtime"],
               XDG_CONFIG_HOME="/work/config", XDG_CACHE_HOME="/work/cache",
               XDG_DATA_HOME="/work/data", DBUS_SESSION_BUS_ADDRESS=config["bus"],
               DISPLAY=":99", WAYLAND_DISPLAY="", QT_QPA_PLATFORM="xcb",
               LIBGL_ALWAYS_SOFTWARE="1", PULSE_SERVER="unix:/work/no-audio",
               PIPEWIRE_REMOTE="no-pipewire")
    children, logs, rows = [], [], []
    phase = ["startup"]
    report = {"completed": False, "failure": None, "mode": config["mode"],
              "identity_checks": [], "rounds": [], "stages": {}, "negative_checks": []}

    def start(name, args):
        log = (root / (name + ".log")).open("xb")
        logs.append(log)
        process = subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL,
                                   stdout=log, stderr=log)
        children.append(process)
        return process

    def stop(process):
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)

    def wait_path(path, process):
        deadline = time.monotonic() + 10
        while not pathlib.Path(path).exists():
            if process.poll() is not None or time.monotonic() >= deadline:
                raise RuntimeError("isolated service did not start: " + path)
            time.sleep(.05)

    def record(kind, extra, elapsed):
        rows.append({"kind": kind, "phase": phase[0], "ms": elapsed, **extra})
        with (root / "calls.jsonl").open("a") as log:
            log.write(json.dumps(rows[-1]) + "\n")

    def raw_call(args, kind, timeout=10, stdin=None):
        started = time.perf_counter_ns()
        result = subprocess.run(args, env=env, input=stdin, text=True,
                                capture_output=True, timeout=timeout)
        elapsed = (time.perf_counter_ns() - started) / 1e6
        record(kind, {"returncode": result.returncode}, elapsed)
        return result, elapsed

    def public_call(action, capability, data, target=None, confirmed=False):
        args = ["/binary", "run", "app", action, "--capability", capability,
                "--input", "-", "--strict-isolation"]
        if target is not None:
            args += ["--target", "sessionId=" + target]
        if confirmed:
            args += ["--confirm"]
        result, elapsed = raw_call(args, action, stdin=json.dumps(data))
        # A rejected control still answers with a JSON error envelope on stdout.
        if not result.stdout.strip():
            return None, elapsed
        try:
            return json.loads(result.stdout), elapsed
        except json.JSONDecodeError:
            return None, elapsed

    def dbus_send_field(args):
        reply = subprocess.run(["/usr/bin/dbus-send", "--print-reply"] + args,
                               env=env, text=True, capture_output=True, timeout=5)
        if reply.returncode != 0:
            return None
        lines = [line for line in reply.stdout.splitlines() if line.strip()]
        return lines[-1].split()[-1].strip('"') if lines else None

    def observed_owner_pid(name):
        owner = dbus_send_field(["--dest=org.freedesktop.DBus", "/org/freedesktop/DBus",
                                 "org.freedesktop.DBus.GetNameOwner", "string:" + name])
        if owner is None:
            return None
        pid = dbus_send_field(["--dest=org.freedesktop.DBus", "/org/freedesktop/DBus",
                               "org.freedesktop.DBus.GetConnectionUnixProcessID",
                               "string:" + owner])
        return int(pid) if pid is not None and pid.isdigit() else None

    def gated_write_allowed(session, label):
        # Write gate: re-verify identity on the live bus immediately before any
        # control write; refuse on mismatch or unknown owner. Discovery and
        # reads never pass through here.
        if session.get("mpris_name") is None:
            raise IdentityRefused(label + ": identity unknown (no bound MPRIS name)")
        owner = observed_owner_pid(session["mpris_name"])
        if owner is None:
            raise IdentityRefused(label + ": identity unknown (owner bus PID unresolvable for "
                                  + session["mpris_name"] + ")")
        if owner != session["player"].pid:
            raise IdentityRefused(label + ": identity mismatch (owner bus PID %d != own player child PID %d)"
                                  % (owner, session["player"].pid))

    def mpris_names():
        reply = subprocess.run(["/usr/bin/dbus-send", "--print-reply",
                                "--dest=org.freedesktop.DBus", "/org/freedesktop/DBus",
                                "org.freedesktop.DBus.ListNames"],
                               env=env, text=True, capture_output=True, timeout=5)
        names = []
        for line in reply.stdout.splitlines():
            parts = line.split('"')
            if len(parts) >= 2 and parts[1].startswith("org.mpris.MediaPlayer2."):
                names.append(parts[1])
        return names

    def read_status(target):
        value, elapsed = public_call("read", "media.playback.state.read@3",
                                     {"timeoutMs": 3000}, target)
        if value is None or value.get("ok") is not True:
            return None, elapsed
        if value["data"]["sessionId"] != target:
            raise RuntimeError("read target binding changed")
        return value["data"]["playbackStatus"], elapsed

    def confirmed_pause(session, label):
        try:
            gated_write_allowed(session, label)
        except IdentityRefused as error:
            record("control." + label, {"outcome": "identity-refused"}, 0.0)
            raise
        value, elapsed = public_call("apply", "media.playback.control@3",
                                     {"operation": "pause", "timeoutMs": 3000},
                                     session["target"], True)
        outcome = "confirmed"
        if value is None:
            outcome = "no-json-reply"
        elif value.get("ok") is True:
            verification = value["data"]["verification"]
            if verification["conditionObserved"] is not True:
                outcome = "unverified-reply"
        else:
            outcome = value["error"]["code"]
        record("control." + label, {"outcome": outcome}, elapsed)
        return outcome, elapsed

    def start_player():
        t0 = time.perf_counter()
        player = start("haruna-" + str(len(children)), ["/usr/bin/haruna", "/work/synthetic.mkv"])
        target = None
        first_target_ms = first_playing_ms = None
        owner_pid_matches = None
        mpris_name = mpris_owner = None
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if player.poll() is not None:
                raise RuntimeError("Haruna exited at startup")
            value, _ = public_call("discover", "media.session.discover@3",
                                   {"timeoutMs": 3000})
            if value is not None and value.get("ok") is True:
                data = value["data"]
                if data["count"] == 1 and data["complete"] and not data["truncated"]:
                    if first_target_ms is None:
                        first_target_ms = (time.perf_counter() - t0) * 1000
                        names = mpris_names()
                        owner_pid = observed_owner_pid(names[0]) if len(names) == 1 else None
                        owner_pid_matches = owner_pid == player.pid
                        mpris_name = names[0] if len(names) == 1 else None
                        mpris_owner = dbus_send_field(
                            ["--dest=org.freedesktop.DBus", "/org/freedesktop/DBus",
                             "org.freedesktop.DBus.GetNameOwner",
                             "string:" + mpris_name]) if mpris_name else None
                    target = data["sessions"][0]["sessionId"]
                    status, _ = read_status(target)
                    if status == "playing":
                        first_playing_ms = (time.perf_counter() - t0) * 1000
                        break
                    # Any other initial publication (stopped/paused/unknown) is
                    # part of the cold phase; keep polling for first "playing".
            time.sleep(.12)
        if first_playing_ms is None:
            raise RuntimeError("player never published playing within 20 s")
        # A mismatch or unknown owner is recorded, not fatal: the write gate
        # refuses every control write for such a session while reads continue.
        return {"player": player, "target": target, "first_target_ms": first_target_ms,
                "first_playing_ms": first_playing_ms,
                "owner_pid_matches": owner_pid_matches,
                "identity_proven": owner_pid_matches is True,
                "player_pid": player.pid,
                "mpris_name": mpris_name, "mpris_owner": mpris_owner}

    def identity_gate_counterexample(session):
        # Minimal negative counter-example for the write gate: with identity
        # unknown or mismatched, a write attempt must be refused before anything
        # is sent. A dbus-monitor profile window over exactly these arms must
        # observe zero org.mpris.MediaPlayer2.Player method calls, proving no
        # apply reached the bus by any path during the refusals.
        audit = start("identity-audit",
                      ["/usr/bin/dbus-monitor", "--session", "--profile",
                       "type='method_call',interface='org.mpris.MediaPlayer2.Player'"])
        time.sleep(.2)
        if audit.poll() is not None:
            raise RuntimeError("identity-gate audit monitor did not start")
        bound_owner = observed_owner_pid(session["mpris_name"]) if session["mpris_name"] else None
        arms = []
        absent = {"player": session["player"], "target": None,
                  "mpris_name": "org.mpris.MediaPlayer2.absent.run" + str(os.getpid())}
        try:
            gated_write_allowed(absent, "counterexample.unknown-owner")
            arms.append({"arm": "unknown-owner", "refused": False, "reason": None})
        except IdentityRefused as error:
            arms.append({"arm": "unknown-owner", "refused": True, "reason": str(error)})
        decoy_name = "org.mpris.MediaPlayer2.decoy.run" + str(os.getpid())
        decoy = start("decoy-owner", ["/usr/bin/python3", "-c", DECOY_SOURCE, decoy_name])
        bystander = start("bystander-child", ["/usr/bin/sleep", "60"])
        deadline = time.monotonic() + 8
        while observed_owner_pid(decoy_name) != decoy.pid:
            if decoy.poll() is not None or time.monotonic() >= deadline:
                raise RuntimeError("decoy bus name owner did not start")
            time.sleep(.1)
        foreign = {"player": bystander, "target": None, "mpris_name": decoy_name}
        try:
            gated_write_allowed(foreign, "counterexample.owner-mismatch")
            arms.append({"arm": "owner-mismatch", "refused": False, "reason": None})
        except IdentityRefused as error:
            arms.append({"arm": "owner-mismatch", "refused": True, "reason": str(error)})
        stop(bystander)
        stop(decoy)
        time.sleep(.2)
        stop(audit)
        calls = [line for line in (root / "identity-audit.log").read_text().splitlines()
                 if line.startswith("mc\t")]
        report["identity_gate_counterexample"] = {
            "arms": arms,
            "bound_name_owner_matches_own_child": bound_owner == session["player"].pid,
            "audit_expected_player_method_calls": 0,
            "audit_actual_player_method_calls": len(calls),
            "audit_window": "counter-example arms only; filter type='method_call',"
                            "interface='org.mpris.MediaPlayer2.Player'"}
        report["negative_checks"] += ["identity-gate:" + arm["arm"] for arm in arms]
        if any(not arm["refused"] for arm in arms):
            raise RuntimeError("identity gate allowed a write with unknown/mismatched identity")
        if calls:
            raise RuntimeError("identity-gate arms produced MPRIS method calls on the bus")

    def settle_and_run_stages(config):
        subprocess.run(["/usr/bin/ffmpeg", "-hide_banner", "-loglevel", "error",
                        "-f", "lavfi", "-i", "color=c=blue:s=320x180:r=1", "-t", "3600",
                        "-an", "-c:v", "ffv1", "-n", "/work/synthetic.mkv"],
                       env=env, check=True, timeout=30)
        bus = start("dbus", ["/usr/bin/dbus-daemon", "--session", "--nofork", "--nopidfile",
                             "--address=" + config["bus"]])
        wait_path(config["runtime"] + "/bus", bus)
        display = start("xvfb", ["/usr/bin/Xvfb", ":99", "-screen", "0", "800x600x24",
                                 "-nolisten", "tcp"])
        wait_path("/tmp/.X11-unix/X99", display)
        session = start_player()
        report["identity_checks"].append({
            "first_discovery_owner_pid_matches": session["owner_pid_matches"],
            "player_pid": session["player"].pid,
            "write_gate": "refuse unless write-time owner bus PID == own player child PID"})
        identity_proven = session["identity_proven"]
        target = session["target"]
        # Steady-state precondition shared with the existing benchmark; the staged
        # numbers below are steady-state attributions, not launch readiness claims.
        time.sleep(config["startup_seconds"])
        status, _ = read_status(target)
        if status != "playing":
            raise RuntimeError("settled player is not playing")
        phase[0] = "steady"
        send = ["/usr/bin/dbus-send", "--address=" + config["bus"], "--print-reply",
                "--dest=org.freedesktop.DBus", "/org/freedesktop/DBus",
                "org.freedesktop.DBus.GetId"]
        plan = [("exec_true", ["/usr/bin/true"], config["baseline_calls"], .02),
                ("cli_version", ["/binary", "--version"], config["baseline_calls"], .02),
                ("raw_bus_getid", send, config["baseline_calls"], .02),
                ("discover", None, config["stage_calls"], .1),
                ("read", None, config["stage_calls"], .1)]
        next_tick = time.monotonic()
        for kind, args, count, pause in plan:
            for _ in range(count):
                if kind == "discover":
                    value, elapsed = public_call("discover", "media.session.discover@3",
                                                 {"timeoutMs": 3000})
                    if value is None or value.get("ok") is not True:
                        raise RuntimeError("discover failed during stage probe")
                elif kind == "read":
                    status, elapsed = read_status(target)
                    if status != "playing":
                        raise RuntimeError("read failed during stage probe")
                else:
                    _, elapsed = raw_call(args, kind)
                next_tick += pause
                time.sleep(max(0, next_tick - time.monotonic()))
        if identity_proven:
            # Confirmed transactions with an independent read, mirroring the
            # benchmark; every write re-passes the gate immediately before it.
            for _ in range(config["transactions"]):
                operation = "pause" if status == "playing" else "play"
                expected = {"pause": "paused", "play": "playing"}[operation]
                gated_write_allowed(session, "stage_transaction")
                begin = time.perf_counter_ns()
                value, control_ms = public_call("apply", "media.playback.control@3",
                                                {"operation": operation, "timeoutMs": 3000},
                                                target, True)
                if value is None or value.get("ok") is not True:
                    raise RuntimeError("stage transaction control failed")
                verification = value["data"]["verification"]
                if verification["conditionObserved"] is not True:
                    raise RuntimeError("stage transaction was not verified")
                status, read_ms = read_status(target)
                if status != expected:
                    raise RuntimeError("stage transaction read disagreed")
                record("transaction", {"operation": operation, "control_ms": control_ms,
                                       "read_ms": read_ms,
                                       "transaction_ms": (time.perf_counter_ns() - begin) / 1e6}, control_ms)
                next_tick += .25
                time.sleep(max(0, next_tick - time.monotonic()))
            gated_write_allowed(session, "settled_stop")
            value, _ = public_call("apply", "media.playback.control@3",
                                   {"operation": "stop", "timeoutMs": 3000}, target, True)
            if value is None or value.get("ok") is not True:
                raise RuntimeError("settled stop failed")
        else:
            # Identity gate: without proof, no control write (transactions and
            # stop included) is attempted; the read-only stages above stand and
            # cleanup uses this run's own child handle only.
            report["identity_writes_refused"] = True
            report["identity_gate"] = ("transactions and stop refused: identity not proven "
                                       "at first discovery")
        stop(session["player"])
        identity_gate_counterexample(session)

    def run_coldstart_rounds(config):
        subprocess.run(["/usr/bin/ffmpeg", "-hide_banner", "-loglevel", "error",
                        "-f", "lavfi", "-i", "color=c=blue:s=320x180:r=1", "-t", "3600",
                        "-an", "-c:v", "ffv1", "-n", "/work/synthetic.mkv"],
                       env=env, check=True, timeout=30)
        bus = start("dbus", ["/usr/bin/dbus-daemon", "--session", "--nofork", "--nopidfile",
                             "--address=" + config["bus"]])
        wait_path(config["runtime"] + "/bus", bus)
        display = start("xvfb", ["/usr/bin/Xvfb", ":99", "-screen", "0", "800x600x24",
                                 "-nolisten", "tcp"])
        wait_path("/tmp/.X11-unix/X99", display)
        phase[0] = "cold"
        for round_number in range(config["rounds"]):
            deadline_seen = time.monotonic() + 15
            while mpris_names():
                if time.monotonic() >= deadline_seen:
                    raise RuntimeError("stale MPRIS name before cold round")
                time.sleep(.1)
            session = start_player()
            target = session["target"]
            round_row = {"round": round_number, **{key: session[key] for key in
                         ("first_target_ms", "first_playing_ms", "owner_pid_matches",
                          "identity_proven", "player_pid", "mpris_name", "mpris_owner")}}
            if not session["identity_proven"]:
                # Identity gate: without proof this round records a refusal and
                # an observation, sends no MPRIS write, and cleans up through
                # its own child handle.
                status, _ = read_status(target)
                round_row["identity_writes_refused"] = True
                round_row["status_observed_without_writes"] = status
                stop(session["player"])
                round_row["player_exit_confirmed"] = session["player"].poll() is not None
                report["rounds"].append(round_row)
                with (root / "calls.jsonl").open("a") as log:
                    log.write(json.dumps({"kind": "round", **round_row}) + "\n")
                continue
            outcome_a, control_a_ms = confirmed_pause(session, "cold_A_first_playing")
            round_row["attempt_A"] = {"outcome": outcome_a, "control_ms": control_a_ms}
            status, _ = read_status(target)
            round_row["status_after_A"] = status
            if outcome_a != "confirmed" and status == "playing":
                outcome_b, control_b_ms = confirmed_pause(session, "cold_B_immediate_retry")
                round_row["attempt_B"] = {"outcome": outcome_b, "control_b_ms": control_b_ms}
                status, _ = read_status(target)
            # Contrast arm: drive to playing, then require playback to survive a
            # stability gate (two "playing" reads >= 1.5 s apart) so the pause is
            # measured while rendering is actually underway, not inside another
            # load window. This is an observation gate, not a fixed sleep.
            time.sleep(config["startup_seconds"])
            status, _ = read_status(target)
            round_row["status_settled_before_C"] = status
            if status != "playing":
                # A failed cold pause may leave the player paused OR stopped;
                # restore playing from either before measuring the settled arm.
                gated_write_allowed(session, "settled_play")
                value, _ = public_call("apply", "media.playback.control@3",
                                       {"operation": "play", "timeoutMs": 3000}, target, True)
                if value is None or value.get("ok") is not True:
                    raise RuntimeError("settled play failed")
                status, _ = read_status(target)
            stable_since = None
            gate_deadline = time.monotonic() + 15
            while status == "playing" and time.monotonic() < gate_deadline:
                if stable_since is not None and time.monotonic() - stable_since >= 1.5:
                    break
                if stable_since is None:
                    stable_since = time.monotonic()
                time.sleep(.75)
                status, _ = read_status(target)
            if status != "playing":
                raise RuntimeError("settled player is not playing")
            outcome_c, control_c_ms = confirmed_pause(session, "settled_C")
            round_row["attempt_C"] = {"outcome": outcome_c, "control_ms": control_c_ms}
            gated_write_allowed(session, "cold_stop")
            value, _ = public_call("apply", "media.playback.control@3",
                                   {"operation": "stop", "timeoutMs": 3000}, target, True)
            round_row["stop_confirmed"] = value is not None and value.get("ok") is True
            stop(session["player"])
            round_row["player_exit_confirmed"] = session["player"].poll() is not None
            if not round_row["stop_confirmed"] or not round_row["player_exit_confirmed"]:
                raise RuntimeError("cold round did not end with a confirmed stopped player")
            report["rounds"].append(round_row)
            with (root / "calls.jsonl").open("a") as log:
                log.write(json.dumps({"kind": "round", **round_row}) + "\n")

    player = None
    try:
        if config["mode"] == "stages":
            settle_and_run_stages(config)
        else:
            run_coldstart_rounds(config)
        report["completed"] = True
    except Exception as error:
        report["failure"] = type(error).__name__ + ": " + str(error)
    finally:
        for child in reversed(children):
            stop(child)
        for log in logs:
            log.close()
        report["all_owned_children_reaped"] = all(p.poll() is not None for p in children)
        for kind in sorted({row["kind"] for row in rows}):
            if kind in ("discover", "read", "cli_version", "raw_bus_getid", "exec_true",
                        "control.cold_A_first_playing", "control.cold_B_immediate_retry",
                        "control.settled_C"):
                entry = {}
                samples = [row["ms"] for row in rows if row["kind"] == kind]
                if samples:
                    entry["all"] = distribution(samples)
                for tag in ("startup", "steady", "cold"):
                    picked = [row["ms"] for row in rows
                              if row["kind"] == kind and row["phase"] == tag]
                    if picked:
                        entry[tag] = distribution(picked)
                report["stages"][kind] = entry
        transactions = [row for row in rows if row["kind"] == "transaction"]
        if transactions:
            report["stages"]["transaction"] = {
                metric: distribution([row[metric] for row in transactions])
                for metric in ("control_ms", "read_ms", "transaction_ms")}
            report["stages"]["residual_control_minus_read"] = distribution(
                [row["control_ms"] - row["read_ms"] for row in transactions])
        report["sampling"] = {
            "phases": {
                "startup": "stages: player spawn -> discovery polling -> settled precondition",
                "steady": "stages: attribution window after the settled precondition, "
                          "through baselines/stage calls/transactions, until the gated stop",
                "cold": "coldstart: each fresh-player round"},
            "rule": "every sample in calls.jsonl carries its phase; 'all' is the raw full set "
                    "including startup/cold polling, the phase keys are filtered attribution "
                    "subsets and are never reported as the full set",
            "counts": {tag: sum(1 for row in rows if row["phase"] == tag)
                       for tag in ("startup", "steady", "cold")}}
        (root / "result.json").write_text(json.dumps(report, indent=2))
    return 0 if report["completed"] and report["failure"] is None else 1


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--inside":
        return inside(json.loads(pathlib.Path(sys.argv[2]).read_text()))
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--mode", choices=("stages", "coldstart"), default="stages")
    parser.add_argument("--baseline-calls", type=int, default=50)
    parser.add_argument("--stage-calls", type=int, default=100)
    parser.add_argument("--transactions", type=int, default=40)
    parser.add_argument("--rounds", type=int, default=5)
    args = parser.parse_args()
    repo = pathlib.Path(__file__).resolve().parents[2]
    output = args.output.resolve()
    if not output.is_relative_to(repo / "target"):
        parser.error("output must be inside this source repository's target directory")
    if not (1 <= args.baseline_calls <= 1000 and 1 <= args.stage_calls <= 1000 and
            1 <= args.transactions <= 1000 and 1 <= args.rounds <= 20):
        parser.error("sampling arguments exceed the manual probe's bounds")
    binary = args.binary.resolve(strict=True)
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("x"):
        pass
    artifacts = output.parent / (output.stem + ".artifacts")
    artifacts.mkdir(mode=0o700)
    for directory in ("home", "runtime", "config", "cache", "data"):
        (artifacts / directory).mkdir(mode=0o700)
    uid = os.geteuid()
    runtime = "/run/user/" + str(uid)
    config = {key: getattr(args, key) for key in ("mode", "baseline_calls",
                                                  "stage_calls", "transactions", "rounds")}
    config.update(runtime=runtime, bus="unix:path=" + runtime + "/bus",
                  startup_seconds=10,
                  started_utc=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                  binary_sha256=sha256(binary),
                  haruna_sha256=sha256(pathlib.Path("/usr/bin/haruna")),
                  identity_proof="every write re-verifies on the live bus that the bound MPRIS "
                                 "name owner PID == own player child PID; refused on mismatch or "
                                 "unknown owner; discovery/read-only unaffected; cleanup only "
                                 "signals own child handles",
                  desktop_input=False, physical_audio=False, network=False, human_baseline=False)
    (artifacts / "config.json").write_text(json.dumps(config, indent=2))
    command = ["/usr/bin/bwrap", "--unshare-all", "--die-with-parent", "--new-session",
               "--ro-bind", "/usr", "/usr", "--ro-bind", "/etc", "/etc",
               "--symlink", "usr/bin", "/bin", "--symlink", "usr/lib", "/lib",
               "--symlink", "usr/lib", "/lib64", "--proc", "/proc", "--dev", "/dev",
               "--tmpfs", "/tmp", "--dir", "/run", "--dir", "/run/user",
               "--bind", str(artifacts / "runtime"), runtime,
               "--bind", str(artifacts), "/work", "--ro-bind",
               str(pathlib.Path(__file__).resolve()), "/runner",
               "--ro-bind", str(binary), "/binary",
               "--chdir", "/work", "/usr/bin/python3", "/runner", "--inside",
               "/work/config.json"]
    limit = (args.rounds * 45 + 180) if args.mode == "coldstart" else 600
    try:
        result = subprocess.run(command, timeout=limit)
    except (subprocess.TimeoutExpired, KeyboardInterrupt) as error:
        output.write_text(json.dumps({"completed": False, "failure": type(error).__name__,
                                      "all_owned_children_reaped": False}))
        if isinstance(error, KeyboardInterrupt):
            raise
        return 1
    inner_result = artifacts / "result.json"
    if inner_result.exists():
        output.write_bytes(inner_result.read_bytes())
        report = json.loads(output.read_text())
        print(json.dumps({key: report[key] for key in ("completed", "failure",
                         "all_owned_children_reaped")}), flush=True)
        print(json.dumps(report.get("stages", {})), flush=True)
        print(json.dumps(report.get("identity_gate_counterexample")), flush=True)
        for row in report.get("rounds", []):
            print(json.dumps(row), flush=True)
    else:
        output.write_text(json.dumps({"completed": False,
                                      "failure": "isolated runner did not publish a result"}))
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
