#!/usr/bin/env python3
"""Actual-binary first-connection retry; absolute stdio fixture, never real Codex."""
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time

from app_server_transport_regression import require, screen_text, stop

MOCK = r'''#!/usr/bin/python3
import json, os, sys, threading, time
from pathlib import Path
root = Path(os.environ['RETRY_FIXTURE'])
log = root / 'server.log'
with log.open('a', encoding='utf-8') as handle:
    handle.write('START\n')
starts = log.read_text(encoding='utf-8').splitlines().count('START')
if starts == 1:
    sys.exit(0)
if starts != 2:
    sys.exit(97)
# Keep bootstrap in flight while the harness sends additional F5 presses.
deadline = time.monotonic() + 4
while not (root / 'release').exists():
    if time.monotonic() >= deadline:
        sys.exit(98)
    time.sleep(0.01)
def eof_when_requested():
    while not (root / 'eof').exists():
        time.sleep(0.01)
    os._exit(0)
threading.Thread(target=eof_when_requested, daemon=True).start()
for line in sys.stdin:
    request = json.loads(line)
    method, ident = request.get('method'), request.get('id')
    with log.open('a', encoding='utf-8') as handle:
        handle.write('RPC ' + str(method) + '\n')
    if ident is None:
        continue
    if method == 'initialize':
        result = {'serverInfo': {'name': 'startup-fixture', 'version': '0.0.0'}}
    elif method == 'thread/list':
        result = {'data': [{'id': 'retry-fixture-thread', 'name': 'STARTUP_RETRY_READY',
                           'cwd': os.environ['HOME'], 'status': {'type': 'idle'}}],
                  'nextCursor': None}
    elif method == 'thread/loaded/list':
        result = {'data': [], 'nextCursor': None}
    else:
        print(json.dumps({'id': ident, 'error': {'code': -32601, 'message': 'fixture unsupported'}}), flush=True)
        continue
    print(json.dumps({'id': ident, 'result': result}), flush=True)
'''


def run(binary, output):
    binary = binary.resolve(strict=True)
    output.mkdir(parents=True, exist_ok=False)
    raw = bytearray()
    with tempfile.TemporaryDirectory(prefix='ctui-startup-retry-') as temporary:
        home = Path(temporary)
        mock = home / 'mock-server.py'
        mock.write_text(MOCK, encoding='utf-8')
        mock.chmod(0o755)
        config = home / 'config/codex-tui/config.toml'
        config.parent.mkdir(parents=True)
        config.write_text(
            '[ui]\nlanguage="en"\n[app_server]\nactive="startup-fixture"\n'
            '[app_server.targets.startup-fixture]\ntransport="stdio"\n'
            'codex_bin=' + json.dumps(str(mock)) + '\n', encoding='utf-8')
        log = home / 'server.log'
        log.write_text('', encoding='utf-8')
        env = {'HOME': str(home), 'USERPROFILE': str(home),
               'XDG_CONFIG_HOME': str(home / 'config'), 'XDG_DATA_HOME': str(home / 'data'),
               'XDG_STATE_HOME': str(home / 'state'), 'XDG_CACHE_HOME': str(home / 'cache'),
               'CODEX_HOME': str(home / 'unused-codex'), 'PATH': '/usr/bin:/bin',
               'TERM': 'xterm-256color', 'LANG': 'C.UTF-8', 'RETRY_FIXTURE': str(home)}
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 180, 0, 0))
        before = termios.tcgetattr(slave)

        def setup():
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

        process = subprocess.Popen([str(binary)], stdin=slave, stdout=slave, stderr=slave,
                                   env=env, cwd=home, preexec_fn=setup, close_fds=True)

        def starts():
            return log.read_text(encoding='utf-8').splitlines().count('START')

        def pump(duration):
            deadline = time.monotonic() + duration
            while time.monotonic() < deadline:
                ready, _, _ = select.select([master], [], [], min(0.02, max(0, deadline - time.monotonic())))
                if ready:
                    try:
                        data = os.read(master, 65536)
                    except OSError as error:
                        if error.errno == errno.EIO:
                            return
                        raise
                    if not data:
                        return
                    raw.extend(data)
                    require(len(raw) <= 4 * 1024 * 1024, 'terminal output exceeded fixture budget')

        def wait_for(predicate, label):
            deadline = time.monotonic() + 8
            while time.monotonic() < deadline:
                pump(0.03)
                if predicate():
                    return
                require(process.poll() is None, label + ': TUI exited early')
            raise RuntimeError(label + ': observation deadline expired')

        try:
            wait_for(lambda: starts() == 1 and 'F5: retry startup' in screen_text(raw), 'startup failure hint')
            pump(0.25)
            require(starts() == 1, 'startup failure retried automatically')
            os.write(master, b'?')
            pump(0.15)
            os.write(master, b'\x1b[15~')
            pump(0.25)
            require(starts() == 1, 'F5 stole help focus')
            os.write(master, b'\x1b')
            pump(0.15)
            os.write(master, b'\x1b[15~')
            wait_for(lambda: starts() == 2, 'explicit retry admitted')
            os.write(master, b'\x1b[15~' * 8)
            pump(0.25)
            require(starts() == 2, 'multiple startup attempts overlapped')
            (home / 'release').touch()
            wait_for(lambda: 'STARTUP_RETRY_READY' in screen_text(raw), 'successful retry installed')
            os.write(master, b'\x1b[15~')
            pump(0.15)
            require(starts() == 2, 'F5 restarted an established session')
            (home / 'eof').touch()
            wait_for(lambda: 'stdout' in screen_text(raw) and 'closed' in screen_text(raw), 'established disconnect')
            os.write(master, b'\x1b[15~')
            pump(0.25)
            require(starts() == 2, 'F5 replayed an established disconnected session')
            os.write(master, b'q')
            wait_for(lambda: process.poll() is not None, 'normal quit')
            require(process.returncode == 0, 'TUI did not quit successfully')
            require(termios.tcgetattr(slave) == before, 'controlling terminal was not restored')
            methods = [line[4:] for line in log.read_text(encoding='utf-8').splitlines() if line.startswith('RPC ')]
            allowed = {'initialize', 'initialized', 'thread/list', 'thread/loaded/list', 'thread/goal/get'}
            require(set(methods) <= allowed, 'unexpected non-bootstrap RPC: ' + repr(set(methods) - allowed))
            require('initialize' in methods and 'thread/list' in methods, 'missing real bootstrap exchange')
            receipt = {'schema': 'codex-tui/startup-retry-regression/v1', 'status': 'passed',
                       'starts': starts(), 'singleFlight': True, 'modalFocusPreserved': True,
                       'noAutomaticRetry': True, 'noEstablishedReconnect': True,
                       'terminalRestored': True, 'writeRpcCount': 0, 'methods': methods,
                       'binarySha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                       'realCodexQualification': False, 'realSshQualification': False}
            (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
        finally:
            stop(process)
            os.close(master)
            os.close(slave)
            (output / 'terminal.bin').write_bytes(raw)
            (output / 'screen.txt').write_text(screen_text(raw), encoding='utf-8')
            (output / 'server.log').write_text(log.read_text(encoding='utf-8'), encoding='utf-8')


if __name__ == '__main__':
    require(len(sys.argv) == 3, 'usage: startup_retry_regression.py BINARY OUTPUT')
    run(Path(sys.argv[1]), Path(sys.argv[2]))
