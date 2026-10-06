#!/usr/bin/env python3
"""dwell_server and the version lock on worlds (RELEASES.md §6).

Runs the real server binary on world files "saved by" other versions: it must open its own
version's world (and record the version), and refuse one saved by another compatibility line, a
newer build, or before versioned releases, naming the version to run — without touching the file.

usage: version_lock_test.py DWELL_SERVER DWELL_WORLD
"""
import os
import re
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time

server, world_tool = sys.argv[1], sys.argv[2]


def run_server(path):
    """Starts the server on `path`; returns (exit code, stdout, stderr) after it stops."""
    proc = subprocess.Popen(
        [server, "--world", path, "--visibility", "none", "--port", "0", "--rtc-port", "0"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    out = ""
    deadline = time.time() + 20
    try:
        # Opened: it prints its invite link. Refused: it exits at once.
        while time.time() < deadline and proc.poll() is None:
            line = proc.stdout.readline()
            out += line
            if "invite link:" in line:
                break
        if proc.poll() is None:
            time.sleep(0.3)
            proc.send_signal(signal.SIGINT)
        rest_out, err = proc.communicate(timeout=20)
        return proc.returncode, out + rest_out, err
    finally:
        if proc.poll() is None:
            proc.kill()


def set_meta(path, key, value):
    db = sqlite3.connect(path)
    if value is None:
        db.execute("DELETE FROM meta WHERE key = ?", (key,))
    else:
        db.execute("INSERT OR REPLACE INTO meta VALUES (?, ?)", (key, value))
    db.commit()
    db.close()


def info(path):
    return subprocess.run([world_tool, path, "info"], capture_output=True, text=True).stdout


failures = []


def check(ok, what):
    print(("ok   " if ok else "FAIL ") + what)
    if not ok:
        failures.append(what)


with tempfile.TemporaryDirectory() as tmp:
    path = os.path.join(tmp, "w.dwellworld")

    code, out, err = run_server(path)
    check(code == 0, f"a new world opens (exit {code}) {err.strip()}")
    build = re.search(r"dwell_server (\S+) \|", out)
    check(build is not None, "the server prints its app version")
    build = build.group(1) if build else "?"
    check(f"created by {build}, last saved by {build}" in info(path), "the new world records the version")
    check(f"v={build}" in out, "the invite link carries the server's version")

    code, out, err = run_server(path)
    check(code == 0, "the world reopens in the build that saved it")

    def refused(what, key, value):
        set_meta(path, key, value)
        before = info(path)
        code, out, err = run_server(path)
        check(code == 1, f"{what}: refused (exit {code})")
        check("cannot open world file" in err, f"{what}: says so")
        check(info(path) == before, f"{what}: the file is untouched")
        return err

    err = refused("a newer build's world", "app_version_last", "99.0.0")
    check("99.0.0" in err, "the message names the world's version")
    err = refused("a world of another line", "app_version_last", "0.0.1" if not build.startswith("0.0.") else "7.0.0")
    err = refused("a world with an unreadable version", "app_version_last", "not-a-version")
    err = refused("a world saved before versioned releases", "app_version_last", None)
    check("before versioned releases" in err, "the message says it predates versioned releases")

    # A compatible older version: a stable build opens and upgrades it.
    m = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", build)
    if m and int(m.group(3)) > 0:
        older = f"{m.group(1)}.{m.group(2)}.{int(m.group(3)) - 1}"
        set_meta(path, "app_version_last", older)
        code, out, err = run_server(path)
        check(code == 0, "a compatible older world opens")
        check(f"last saved by {build}" in info(path), "and is now last saved by this build")

if failures:
    print(f"{len(failures)} check(s) failed")
    sys.exit(1)
