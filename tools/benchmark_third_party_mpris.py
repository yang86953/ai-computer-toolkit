#!/usr/bin/env python3
"""Manual public-CLI benchmark against installed Haruna, never the user's player.

Uses a private bwrap PID/network/mount namespace, user bus, Xvfb, HOME and silent
synthetic video. All playback writes use ai-computer-toolkit; dbus-monitor only
audits method counts. No MPRIS mock, UIX app, permission change or input replay.
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


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def distribution(samples):
    if not samples:
        return {"count": 0}
    ordered = sorted(samples)
    return {"count": len(samples), "p50_ms": statistics.median(ordered),
            "p95_ms": ordered[math.ceil(len(ordered) * .95) - 1],
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
    children, logs, rows, expected_methods = [], [], [], []
    report = {"completed": False, "failure": None, "samples": rows,
              "application": "Haruna", "metadata": config,
              "negative_checks": [], "player_exit_confirmed": False,
              "stop_confirmed": False, "elapsed_control_seconds": 0}

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

    def call(variant, action, capability, data, target=None, confirmed=False,
             expected_error=None):
        args = ["/binary-" + variant, "run", "app", action, "--capability",
                capability, "--input", "-", "--strict-isolation"]
        if target is not None:
            args += ["--target", "sessionId=" + target]
        if confirmed:
            args += ["--confirm"]
        started = time.perf_counter_ns()
        result = subprocess.run(args, env=env, input=json.dumps(data), text=True,
                                capture_output=True, timeout=10)
        elapsed = (time.perf_counter_ns() - started) / 1e6
        value = json.loads(result.stdout)
        with (root / "calls.jsonl").open("a") as log:
            log.write(json.dumps({"variant": variant, "action": action,
                                  "input": data, "ms": elapsed, "reply": value}) + "\n")
        if expected_error:
            if result.returncode == 0 or value.get("error", {}).get("code") != expected_error:
                raise RuntimeError("expected public rejection: " + expected_error)
        elif result.returncode != 0 or value.get("ok") is not True:
            raise RuntimeError("public call failed: " + json.dumps(value))
        if result.stderr:
            raise RuntimeError("public CLI emitted unexpected stderr")
        return value, elapsed

    def read(variant, target):
        state, elapsed = call(variant, "read", "media.playback.state.read@3",
                              {"timeoutMs": 3000}, target)
        if state["data"]["sessionId"] != target:
            raise RuntimeError("read target binding changed")
        return state["data"]["playbackStatus"], elapsed

    status = None

    def transact(variant, target, round_number, warmup):
        nonlocal status
        operation = "pause" if status == "playing" else "play"
        expected = {"pause": "paused", "play": "playing"}[operation]
        begin = time.perf_counter_ns()
        expected_methods.append(operation.title())
        value, control_ms = call(variant, "apply", "media.playback.control@3",
                                 {"operation": operation, "timeoutMs": 3000}, target, True)
        verification = value["data"]["verification"]
        if (value["data"]["sessionId"] != target or
                verification["conditionObserved"] is not True or
                verification["observedPlaybackStatus"] != expected):
            raise RuntimeError("control was not verified against expected target/state")
        status, read_ms = read(variant, target)
        if status != expected:
            raise RuntimeError("independent public state read disagreed")
        rows.append({"variant": variant, "round": round_number, "warmup": warmup,
                     "operation": operation, "control_ms": control_ms,
                     "read_ms": read_ms,
                     "transaction_ms": (time.perf_counter_ns() - begin) / 1e6})

    player = audit = None
    try:
        subprocess.run(["/usr/bin/ffmpeg", "-hide_banner", "-loglevel", "error",
                        "-f", "lavfi", "-i", "color=c=blue:s=320x180:r=1", "-t", "3600",
                        "-an", "-c:v", "ffv1", "-n", "/work/synthetic.mkv"],
                       env=env, check=True, timeout=30)
        bus = start("dbus", ["/usr/bin/dbus-daemon", "--session", "--nofork",
                             "--nopidfile", "--address=" + config["bus"]])
        wait_path(config["runtime"] + "/bus", bus)
        display = start("xvfb", ["/usr/bin/Xvfb", ":99", "-screen", "0",
                                  "800x600x24", "-nolisten", "tcp"])
        wait_path("/tmp/.X11-unix/X99", display)
        player = start("haruna", ["/usr/bin/haruna", "/work/synthetic.mkv"])
        # Haruna can publish MPRIS while its initial media load is still in flight.
        # This steady-state benchmark excludes the declared startup settling phase;
        # it does not treat early publication of "playing" as launch completion.
        time.sleep(config["startup_seconds"])
        deadline = time.monotonic() + 15
        while True:
            if player.poll() is not None:
                raise RuntimeError("Haruna exited at startup")
            discovery, _ = call("after", "discover", "media.session.discover@3",
                                {"timeoutMs": 3000})
            data = discovery["data"]
            if not data["complete"] or data["truncated"] or data["count"] > 1:
                raise RuntimeError("private bus discovery is not a single complete target")
            if data["count"] == 1:
                target = data["sessions"][0]["sessionId"]
                status, _ = read("after", target)
                if status in ("playing", "paused"):
                    break
            if time.monotonic() >= deadline:
                raise RuntimeError("real player did not publish a playable MPRIS target")
            time.sleep(.1)
        audit = start("audit", ["/usr/bin/dbus-monitor", "--session", "--profile",
                               "type='method_call',interface='org.mpris.MediaPlayer2.Player'"])
        # Only setup waits are fixed; operation timings start after this audit startup.
        time.sleep(.1)
        if audit.poll() is not None:
            raise RuntimeError("read-only method audit did not start")
        for variant in ("before", "after"):
            call(variant, "apply", "media.playback.control@3", {"operation": "pause"},
                 target, expected_error="CONFIRMATION_REQUIRED")
            report["negative_checks"].append(variant + ":unconfirmed")
            call(variant, "apply", "media.playback.control@3", {"operation": "togglePlayPause"},
                 target, True, "INVALID_ARGUMENT")
            report["negative_checks"].append(variant + ":unsupported-operation")
            stale = target[:-1] + ("0" if target[-1] != "0" else "1")
            rejection, _ = call(variant, "apply", "media.playback.control@3", {"operation": "pause"},
                                stale, True, "OPERATION_FAILED")
            details = rejection["error"]["details"]
            if details["accepted"] or details["targetMayHaveMutated"]:
                raise RuntimeError("stale target rejection permits mutation")
            report["negative_checks"].append(variant + ":stale-target")

        control_start = time.monotonic()
        if config["seconds"]:
            for _ in range(config["warmup"]):
                transact("after", target, -1, True)
            control_start = time.monotonic()
            next_tick = control_start
            progress_tick = control_start + 60
            while time.monotonic() - control_start < config["seconds"]:
                transact("after", target, 0, False)
                next_tick += config["interval_ms"] / 1000
                time.sleep(max(0, min(next_tick - time.monotonic(),
                                      control_start + config["seconds"] - time.monotonic())))
                if time.monotonic() >= progress_tick:
                    print(json.dumps({"elapsed_seconds": time.monotonic() - control_start,
                                      "transactions": len(rows) - config["warmup"]}), flush=True)
                    progress_tick += 60
        else:
            for round_number in range(config["rounds"]):
                variants = ("before", "after") if round_number % 2 == 0 else ("after", "before")
                for variant in variants:
                    for index in range(config["warmup"] + config["samples"]):
                        transact(variant, target, round_number, index < config["warmup"])
        report["elapsed_control_seconds"] = time.monotonic() - control_start
        expected_methods.append("Stop")
        result, _ = call("after", "apply", "media.playback.control@3",
                         {"operation": "stop", "timeoutMs": 3000}, target, True)
        report["stop_confirmed"] = result["data"]["verification"]["conditionObserved"]
        stop(player)
        report["player_exit_confirmed"] = player.poll() is not None
        stop(audit)
        actual_methods = [line.split("\t")[-1] for line in
                          (root / "audit.log").read_text().splitlines() if line.startswith("mc\t")]
        report["method_audit"] = {"expected": len(expected_methods), "actual": len(actual_methods),
                                  "exact_order_match": actual_methods == expected_methods}
        if actual_methods != expected_methods:
            raise RuntimeError("MPRIS audit differs: duplicate, missing or unexpected writes")
        report["completed"] = report["stop_confirmed"] and report["player_exit_confirmed"]
    except Exception as error:
        report["failure"] = type(error).__name__ + ": " + str(error)
    finally:
        for child in reversed(children):
            stop(child)
        for log in logs:
            log.close()
        report["all_owned_children_reaped"] = all(p.poll() is not None for p in children)
        report["summary"] = {variant: {metric: distribution([r[metric] for r in rows
            if r["variant"] == variant and not r["warmup"]])
            for metric in ("control_ms", "read_ms", "transaction_ms")}
            for variant in ("before", "after")}
        (root / "result.json").write_text(json.dumps(report, indent=2))
    return 0 if report["completed"] and report["failure"] is None else 1


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--inside":
        return inside(json.loads(pathlib.Path(sys.argv[2]).read_text()))
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=pathlib.Path, required=True)
    parser.add_argument("--after", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--samples", type=int, default=100)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--warmup", type=int, default=5)
    parser.add_argument("--seconds", type=int, default=0)
    parser.add_argument("--interval-ms", type=int, default=250)
    args = parser.parse_args()
    repo = pathlib.Path(__file__).resolve().parents[1]
    output = args.output.resolve()
    if not output.is_relative_to(repo / "target"):
        parser.error("output must be inside this source repository's target directory")
    if not (1 <= args.samples <= 10000 and 1 <= args.rounds <= 10 and
            0 <= args.warmup <= 100 and 0 <= args.seconds <= 3600 and
            10 <= args.interval_ms <= 60000):
        parser.error("sampling arguments exceed the manual benchmark's bounds")
    binaries = {"before": args.before.resolve(strict=True), "after": args.after.resolve(strict=True)}
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("x"):
        pass
    artifacts = output.parent / (output.stem + ".artifacts")
    artifacts.mkdir(mode=0o700)
    for directory in ("home", "runtime", "config", "cache", "data"):
        (artifacts / directory).mkdir(mode=0o700)
    uid = os.geteuid()
    runtime = "/run/user/" + str(uid)
    config = {key: getattr(args, key) for key in ("samples", "rounds", "warmup", "seconds", "interval_ms")}
    config.update(runtime=runtime, bus="unix:path=" + runtime + "/bus",
                  startup_seconds=10,
                  started_utc=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                  sampling_mode="paced-soak" if args.seconds else "back-to-back-comparison",
                  binaries={key: {"sha256": sha256(path)} for key, path in binaries.items()},
                  haruna_sha256=sha256(pathlib.Path("/usr/bin/haruna")),
                  timing_scope="CLI launch + production identity/worker + real MPRIS state; transaction adds independent public state read",
                  desktop_input=False, physical_audio=False, network=False, human_baseline=False)
    (artifacts / "config.json").write_text(json.dumps(config, indent=2))
    command = ["/usr/bin/bwrap", "--unshare-all", "--die-with-parent", "--new-session",
               "--ro-bind", "/usr", "/usr", "--ro-bind", "/etc", "/etc",
               "--symlink", "usr/bin", "/bin", "--symlink", "usr/lib", "/lib",
               "--symlink", "usr/lib", "/lib64", "--proc", "/proc", "--dev", "/dev",
               "--tmpfs", "/tmp", "--dir", "/run", "--dir", "/run/user",
               "--bind", str(artifacts / "runtime"), runtime,
               "--bind", str(artifacts), "/work", "--ro-bind", str(pathlib.Path(__file__).resolve()), "/runner"]
    for variant, binary in binaries.items():
        command += ["--ro-bind", str(binary), "/binary-" + variant]
    command += ["--chdir", "/work", "/usr/bin/python3", "/runner", "--inside", "/work/config.json"]
    limit = (args.seconds + 180 if args.seconds else
             args.rounds * (args.samples + args.warmup) * 2 * 20 + 120)
    try:
        result = subprocess.run(command, timeout=limit)
    except (subprocess.TimeoutExpired, KeyboardInterrupt) as error:
        # subprocess.run kills and waits for its own bwrap child on interruption;
        # never substitute a later run's result for this interrupted execution.
        output.write_text(json.dumps({"completed": False, "failure": type(error).__name__,
                                      "all_owned_children_reaped": False}))
        if isinstance(error, KeyboardInterrupt):
            raise
        return 1
    inner_result = artifacts / "result.json"
    if inner_result.exists():
        output.write_bytes(inner_result.read_bytes())
        report = json.loads(output.read_text())
        print(json.dumps({key: report[key] for key in ("completed", "failure", "summary",
                         "elapsed_control_seconds", "all_owned_children_reaped")}), flush=True)
    else:
        output.write_text(json.dumps({"completed": False, "failure": "isolated runner did not publish a result"}))
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
