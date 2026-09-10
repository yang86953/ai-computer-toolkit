#!/usr/bin/env python3
"""Compare two Linux broker binaries using ONLY empty `sessions` requests.

No Portal session, screenshot, keyboard, pointer, or user application is opened.
Samples cover local process/socket time, not Codex/tool/model or application time.
Raw results must be written under the source build directory, not the workspace.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import select
import socket
import statistics
import subprocess
import tempfile
import time


CONTRACT = "act/linux-desktop-session-broker/v1"
LIMIT = 1024 * 1024


def read_line(pipe, timeout=10):
    deadline = time.monotonic() + timeout
    data = bytearray()
    while b"\n" not in data:
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not select.select([pipe], [], [], remaining)[0]:
            raise TimeoutError("broker response deadline")
        chunk = os.read(pipe.fileno(), min(65536, LIMIT + 1 - len(data)))
        if not chunk:
            raise RuntimeError("broker closed before response")
        data.extend(chunk)
        if len(data) > LIMIT:
            raise RuntimeError("broker response limit")
    # Each request is synchronous; unsolicited extra frames are not accepted.
    return json.loads(data)


def verify(request, response):
    for key in ("contractVersion", "brokerEpoch", "requestNonce", "operation"):
        if response.get(key) != request[key]:
            raise RuntimeError(f"response identity mismatch: {key}")
    if response.get("messageType") != "response" or response.get("completed") is not True:
        raise RuntimeError("broker did not complete the benchmark request")
    if request["operation"] == "sessions" and response.get("data", {}).get("sessions") != []:
        raise RuntimeError("benchmark broker unexpectedly has a desktop session")


class Broker:
    def __init__(self, binary, env, stdio=False):
        self.binary = str(binary)
        self.env = env
        self.stdio = stdio
        self.nonce = 0
        self.process = subprocess.Popen(
            [self.binary, "session-host", "desktop"] + ([] if stdio else ["--socket"]),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            env=env, bufsize=0,
        )
        try:
            ready = read_line(self.process.stdout)
            if (ready.get("contractVersion") != CONTRACT
                    or ready.get("messageType") != "broker-ready"
                    or ready.get("transport") != ("json-lines-stdio" if stdio else "json-lines-unix-socket")):
                raise RuntimeError("unexpected broker-ready")
            self.epoch = ready["brokerEpoch"]
            if len(self.epoch) != 32 or any(c not in "0123456789abcdef" for c in self.epoch):
                raise RuntimeError("invalid broker epoch")
            self.endpoint = f"/run/user/{os.geteuid()}/ai-computer-toolkit-desktop/{self.epoch}.sock"
        except BaseException:
            self.process.kill()
            self.process.wait(timeout=5)
            raise

    def call(self, mode, operation="sessions"):
        self.nonce += 1
        request = dict(contractVersion=CONTRACT, brokerEpoch=self.epoch,
                       requestNonce=f"{self.nonce:032x}", operation=operation)
        encoded = json.dumps(request, separators=(",", ":")).encode() + b"\n"
        started = time.perf_counter_ns()
        if mode == "cli":
            result = subprocess.run(
                [self.binary, "session-call", "desktop", "--input", "-"],
                input=encoded, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                env=self.env, timeout=10, check=True,
            )
            response = json.loads(result.stdout)
        elif mode == "socket":
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
                stream.settimeout(10)
                stream.connect(self.endpoint)
                stream.sendall(encoded)
                stream.shutdown(socket.SHUT_WR)
                raw = bytearray()
                while True:
                    chunk = stream.recv(min(65536, LIMIT + 1 - len(raw)))
                    if not chunk:
                        break
                    raw.extend(chunk)
                    if len(raw) > LIMIT:
                        raise RuntimeError("response limit")
                response = json.loads(raw)
        elif mode == "stdio":
            self.process.stdin.write(encoded)
            self.process.stdin.flush()
            response = read_line(self.process.stdout)
        else:
            raise ValueError(mode)
        elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
        verify(request, response)
        return elapsed_ms

    def close(self):
        try:
            self.call("stdio" if self.stdio else "socket", "shutdown")
            if self.process.wait(timeout=5) != 0:
                raise RuntimeError("broker shutdown failed")
        finally:
            if self.process.poll() is None:
                self.process.kill()
                self.process.wait(timeout=5)
            self.process.stdin.close()
            self.process.stdout.close()


def summarize(values):
    ordered = sorted(values)
    return dict(count=len(values), p50_ms=statistics.median(ordered),
                p95_ms=ordered[math.ceil(len(ordered) * .95) - 1],
                min_ms=ordered[0], max_ms=ordered[-1])


def binary_identity(path):
    with path.open("rb") as binary:
        digest = hashlib.file_digest(binary, "sha256").hexdigest()
    return dict(path=str(path), sha256=digest)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", required=True, type=Path)
    parser.add_argument("--after", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--samples", type=int, default=100)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--warmup", type=int, default=10)
    args = parser.parse_args()
    if not (1 <= args.samples <= 400 and 1 <= args.rounds <= 20 and 0 <= args.warmup <= 30):
        parser.error("samples=1..400, rounds=1..20, warmup=0..30 (bounded broker ledger)")
    output = args.output.resolve()
    build_root = Path(__file__).resolve().parents[1] / "target"
    if not output.is_relative_to(build_root):
        parser.error("output must be inside this repository's target directory")
    binaries = {name: getattr(args, name).resolve() for name in ("before", "after")}
    samples = {name: {mode: [] for mode in ("socket", "cli", "stdio")} for name in binaries}
    output.parent.mkdir(parents=True, exist_ok=True)
    # Reserve a fresh report before starting any child processes.
    with output.open("x", encoding="utf-8") as report_file:
        report = dict(scope="empty-session readonly transport; excludes Portal/EIS/UI/model/tool overhead",
                      platform=platform.platform(), python=platform.python_version(),
                      samples_per_round=args.samples, rounds=args.rounds, warmup=args.warmup,
                      binaries={key: binary_identity(path) for key, path in binaries.items()},
                      successful_requests=0, failed_requests=0, completed=False, samples_ms=samples)
        try:
            with tempfile.TemporaryDirectory(prefix="benchmark-home-", dir=output.parent) as home:
                env = dict(os.environ, HOME=home, TMPDIR=home,
                           XDG_CONFIG_HOME=home, XDG_DATA_HOME=home, XDG_CACHE_HOME=home,
                           DBUS_SESSION_BUS_ADDRESS=f"unix:path={home}/no-dbus",
                           WAYLAND_DISPLAY="no-wayland", DISPLAY="")
                for round_index in range(args.rounds):
                    order = ("before", "after") if round_index % 2 == 0 else ("after", "before")
                    for name in order:
                        for modes in (("socket", "cli"), ("stdio",)):
                            broker = Broker(binaries[name], env, stdio=modes == ("stdio",))
                            try:
                                for index in range(args.warmup + args.samples):
                                    # Alternate local socket and public CLI samples on the same host.
                                    for mode in modes if index % 2 == 0 else reversed(modes):
                                        value = broker.call(mode)
                                        report["successful_requests"] += 1
                                        if index >= args.warmup:
                                            samples[name][mode].append(value)
                            finally:
                                broker.close()
            report["summary"] = {name: {mode: summarize(values) for mode, values in modes.items()}
                                 for name, modes in samples.items()}
            report["completed"] = True
        except BaseException as error:
            report["failed_requests"] += 1
            report["failure_type"] = type(error).__name__
            raise
        finally:
            json.dump(report, report_file, ensure_ascii=False, indent=2)
            report_file.write("\n")
    print(json.dumps({"report": str(output), "summary": report["summary"],
                      "successful_requests": report["successful_requests"],
                      "failed_requests": report["failed_requests"]}, ensure_ascii=False))


if __name__ == "__main__":
    main()
