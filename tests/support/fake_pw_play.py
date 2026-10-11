#!/usr/bin/python3
"""Test-only backend: records arguments/PCM, never opens an audio device."""
import atexit
import hashlib
import os
from pathlib import Path
import signal
import sys
import time

root = Path(__file__).resolve().parent
case = (root / "case").read_text()
args = sys.argv[1:]
with (root / "calls").open("a") as log:
    log.write(" ".join(args) + "\n")
with (root / "pids").open("a") as log:
    log.write(str(os.getpid()) + "\n")


def terminate(signum, frame):
    (root / "term").write_text("received")
    if case != "ignore-term":
        sys.exit(0)


signal.signal(signal.SIGTERM, terminate)


def stall():
    while True:
        time.sleep(1)


if args == ["--help"]:
    if case == "stall-probe":
        (root / "ready").write_text("probe")
        stall()
    if case == "probe-fail":
        sys.stderr.write("fake capability failure\n")
        sys.exit(2)
    flags = "--properties --raw --format --rate --channels --volume --list-formats"
    if case == "missing-flag":
        flags = flags.replace("--raw", "--raw-unsupported")
    print("pw-play [options] [<file>|-]\n" + flags)
    if case == "oversized-help":
        print("X" * 100_000)
    sys.exit(0)
if args == ["--list-formats"]:
    print("Supported formats:\n  s16\n  " + ("f64" if case == "missing-f32" else "f32"))
    sys.exit(0)

# Each periodic start verifies the previous playback PID has been reaped.
previous = root / "previous-playback"
if previous.exists() and Path("/proc/" + previous.read_text()).exists():
    (root / "overlap").write_text("previous child still exists")
previous.write_text(str(os.getpid()))
with (root / "starts").open("a") as log:
    log.write(str(time.monotonic()) + "\n")

def finished():
    with (root / "finishes").open("a") as log:
        log.write(str(time.monotonic()) + "\n")

atexit.register(finished)
(root / "audio.args").write_text("\n".join(args))
(root / "ready").write_text("playback")
if case == "reject-playback":
    sys.stderr.write("fake unsupported invocation\n")
    sys.exit(2)
if case == "early-exit":
    sys.exit(0)
if case == "close-input":
    os.close(0)
    sys.stderr.write("fake closed stdin\n")
    sys.stderr.flush()
    stall()
if case in ("stall", "ignore-term"):
    stall()
if case == "closed-output":
    os.close(1)
    os.close(2)
    stall()
if case == "flood-stall":
    while True:
        os.write(2, b"X" * 4096)
if case == "flood":
    for _ in range(256):
        os.write(2, b"X" * 4096)

if case == "tone-slow":
    total = 0
    with (root / "audio.pcm").open("wb") as captured:
        while True:
            block = os.read(0, 4093)
            if not block:
                sys.exit(0)
            if total < 131072:
                captured.write(block[:131072 - total])
            total += len(block)
            if total >= 131072 and not (root / "tone-captured").exists():
                captured.flush()
                (root / "tone-captured").write_text("ready")
            time.sleep(0.005)

if case == "silence-slow":
    total = 0
    while True:
        block = os.read(0, 4093)
        if not block:
            sys.exit(0)
        if any(block):
            (root / "nonzero").write_text("bad PCM")
            sys.exit(8)
        total += len(block)
        if total >= 65536 and not (root / "zero-verified").exists():
            (root / "zero-verified").write_text(str(total))
        time.sleep(0.005)

with (root / "audio.pcm").open("wb") as captured:
    while True:
        block = os.read(0, 4093)
        if not block:
            break
        captured.write(block)
        if case == "slow":
            time.sleep(0.001)
with (root / "hashes").open("a") as log:
    log.write(hashlib.sha256((root / "audio.pcm").read_bytes()).hexdigest() + "\n")
(root / "eof").write_text("received")
if case == "overrun":
    time.sleep(1.2)
if case == "post-eof-hang":
    stall()
if case == "nonzero":
    sys.stderr.write("fake audio backend failed\n")
    sys.exit(7)
