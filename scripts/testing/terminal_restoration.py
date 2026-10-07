#!/usr/bin/env python3
"""Linux-only fake-backend PTY regression, never a human or SSH qualification."""
import argparse
import ctypes
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import sqlite3
import struct
import subprocess
import sys
import tempfile
import termios
import time


class ProbeFailure(RuntimeError):
    pass


def require(condition, message):
    # These checks must remain active under python -O/PYTHONOPTIMIZE.
    if not condition:
        raise ProbeFailure(message)


def validate_terminal(data, before, after, raw_observed, exit_code, failed_save):
    require(raw_observed, "raw mode was not observed")
    require(before == after, "terminal attributes were not restored")
    require(exit_code == (1 if failed_save else 0), "unexpected exit code")
    checks = {"rawModeObserved": True, "terminalAttributesRestored": True,
              "expectedExitCode": True}
    for name, enter, leave in (
        ("alternateScreenRestored", b"\x1b[?1049h", b"\x1b[?1049l"),
        ("bracketedPasteRestored", b"\x1b[?2004h", b"\x1b[?2004l"),
        ("cursorRestored", b"\x1b[?25l", b"\x1b[?25h"),
    ):
        first, last = data.rfind(enter), data.rfind(leave)
        require(first >= 0 and last > first, name + ": missing or out-of-order cleanup")
        checks[name] = True
    if failed_save:
        restored = max(data.rfind(token) for token in
                       (b"\x1b[?1049l", b"\x1b[?2004l", b"\x1b[?25h"))
        tail = data[restored:].lower()
        require(b"final" in tail and b"999" in tail,
                "the injected final-save error did not follow terminal restoration")
        checks["saveErrorAfterRestoration"] = True
    return checks


class CursorReplies:
    def __init__(self):
        self.tail = b""

    def feed(self, chunk):
        data = self.tail + chunk
        count = data.count(b"\x1b[6n")
        self.tail = data[-3:]
        return b"\x1b[1;1R" * count


def text(data):
    # Only used to recognize a freshly emitted palette heading, not a screen model.
    return re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", data)


def probe(binary, output, failed_save):
    output.mkdir(parents=True, exist_ok=False)  # Never reuse an old passing receipt.
    stream, events = bytearray(), []
    proc = None
    master = slave = None
    started = time.monotonic()
    receipt = {"schema": "codex-tui/automated-pty-regression/v2", "status": "failed",
               "scenario": "failed-final-save" if failed_save else "normal-exit",
               "binarySha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
               "sourceSha": os.environ.get("CODEX_TUI_PTY_SOURCE_SHA"),
               "manualQualification": False, "realSshQualification": False,
               "authority": "linux-fake-backend-controlling-pty-only"}
    try:
        with tempfile.TemporaryDirectory(prefix="codex-tui-pty-") as directory:
            home = Path(directory).resolve()
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 160, 0, 0))
            before = termios.tcgetattr(slave)
            env = os.environ.copy()
            for name in ("HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "XDG_CONFIG_HOME",
                         "XDG_DATA_HOME", "XDG_STATE_HOME", "CODEX_HOME"):
                env[name] = str(home / name.lower())
            env.update(TERM="xterm-256color", SHELL="/bin/sh", LANG="C.UTF-8",
                       LC_ALL="C.UTF-8", LANGUAGE="en")
            parent_pid = os.getpid()

            def controlling():
                # A killed harness must not leave its application child running.
                libc = ctypes.CDLL(None, use_errno=True)
                if libc.prctl(1, signal.SIGKILL, 0, 0, 0) != 0 or os.getppid() != parent_pid:
                    os._exit(125)
                os.setsid()
                fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

            proc = subprocess.Popen([str(binary), "--fake"], cwd=home, env=env,
                                    stdin=slave, stdout=slave, stderr=slave,
                                    preexec_fn=controlling, close_fds=True)
            receipt.update(pid=proc.pid, command=[str(binary), "--fake"])
            replies = CursorReplies()

            def drain(seconds):
                deadline = time.monotonic() + seconds
                while time.monotonic() < deadline:
                    ready = select.select([master], [], [], min(.05, max(0, deadline - time.monotonic())))[0]
                    if not ready:
                        continue
                    try:
                        chunk = os.read(master, 65536)
                    except OSError as exc:
                        if exc.errno == errno.EIO:
                            break
                        raise
                    if not chunk:
                        break
                    stream.extend(chunk)
                    require(len(stream) <= 4 * 1024 * 1024, "PTY output exceeds 4 MiB")
                    reply = replies.feed(chunk)
                    if reply and proc.poll() is None:
                        os.write(master, reply)

            def wait_for(predicate, message, seconds=8):
                deadline = time.monotonic() + seconds
                while not predicate() and proc.poll() is None and time.monotonic() < deadline:
                    drain(.05)
                require(predicate(), message)

            def send(data, label):
                offset = len(stream)
                os.write(master, data)
                events.append({"action": label, "elapsedMs": round((time.monotonic() - started) * 1000)})
                drain(.3)
                return offset

            wait_for(lambda: b"\x1b[?1049h" in stream, "alternate screen not entered")
            drain(.35)
            raw = not bool(termios.tcgetattr(slave)[3] & (termios.ICANON | termios.ECHO))
            require(raw, "raw mode not entered")
            send(b"?", "help input")
            send(b"\x1b", "close help input")
            offset = send(b"\x0b", "open command palette")
            try:
                wait_for(lambda: b"Command Palette" in text(bytes(stream[offset:])),
                         "command palette heading not observed")
            except ProbeFailure as error:
                # The fake backend uses an isolated HOME. Emit bounded frame context
                # so a rendering regression is not misdiagnosed as a timing timeout.
                frame_tail = text(bytes(stream[offset:]))[-384:]
                raise ProbeFailure(
                    f"{error}; child_exit={proc.poll()}; emitted_bytes={len(stream) - offset}; "
                    f"frame_tail={frame_tail!r}"
                ) from error
            send(b"\x1b", "close palette")
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 28, 100, 0, 0))
            os.kill(proc.pid, signal.SIGWINCH)
            events.append({"action": "resize 100x28"})
            drain(.35)
            if failed_save:
                databases = list(home.rglob("*.sqlite3"))
                require(len(databases) == 1, "expected one disposable state database")
                db = databases[0]
                require(not db.is_symlink() and home in db.resolve().parents,
                        "refusing database outside the disposable profile")
                conn = sqlite3.connect(str(db), timeout=2)
                try:
                    conn.execute("PRAGMA user_version=999")
                    conn.commit()
                finally:
                    conn.close()
                receipt["isolatedInjectedDatabase"] = str(db.relative_to(home))
            send(b"q", "quit from Registry")
            wait_for(lambda: proc.poll() is not None, "application did not exit", seconds=12)
            drain(.2)
            checks = validate_terminal(bytes(stream), before, termios.tcgetattr(slave),
                                       raw, proc.returncode, failed_save)
            receipt.update(status="passed", checks=checks, exitCode=proc.returncode)
    except Exception as exc:
        receipt["error"] = type(exc).__name__ + ": " + str(exc)
        raise
    finally:
        try:
            if proc is not None and proc.poll() is None:
                os.killpg(proc.pid, signal.SIGKILL)  # Only our isolated child session.
                proc.wait(timeout=5)
        finally:
            for descriptor in (master, slave):
                if descriptor is not None:
                    os.close(descriptor)
            receipt.update(events=events, outputBytes=len(stream),
                           durationMs=round((time.monotonic() - started) * 1000))
            (output / "terminal-output.bin").write_bytes(stream)
            (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    require(sys.platform == "linux", "this probe requires Linux")
    binary = args.binary.resolve(strict=True)
    require(binary.is_file() and os.access(str(binary), os.X_OK), "binary is not executable")
    for failed_save, name in ((False, "normal"), (True, "failed-save")):
        probe(binary, args.output / name, failed_save)
    print("Verified two automated PTY scenarios; no manual or SSH qualification")


if __name__ == "__main__":
    main()
