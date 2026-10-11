#!/usr/bin/env python3
"""Actual-binary Linux stdio/EOF/pagination regressions; no real account or SSH."""
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
import struct
import subprocess
import sys
import tempfile
import termios
import time


def require(value, message):
    if not value:
        raise RuntimeError(message)


MOCK = r'''#!/usr/bin/python3
import json, os, sys, threading, time
log = open(os.environ['MOCK_LOG'], 'w', buffering=1)
log.write('pid ' + str(os.getpid()) + '\n')
lock = threading.Lock()
started = False
mode = os.environ['MOCK_MODE']
def emit(value):
    with lock:
        sys.stdout.write(json.dumps(value, separators=(',', ':')) + '\n')
        sys.stdout.flush()
def notify():
    time.sleep(0.08)
    if mode == 'eof':
        time.sleep(0.40)
        log.write('deliberate-eof\n')
        os._exit(0)
    payload = b'{"method":"thread/name/updated","params":{"threadId":"audit-thread","threadName":"ZZZ_READ_OK_ZZZ"}}\n'
    with lock:
        if mode == 'split':
            os.write(1, payload[:31])
            log.write('partial-written\n')
            time.sleep(0.60)
            os.write(1, payload[31:])
        else:
            os.write(1, payload)
    log.write('notification-written\n')
    time.sleep(0.20)
    log.write('alive-after-notification\n')
for line in sys.stdin:
    request = json.loads(line)
    method, ident = request.get('method'), request.get('id')
    if ident is None:
        continue
    log.write(method + ' ' + str(request.get('params', {}).get('cursor')) + '\n')
    if method == 'initialize':
        result = {'serverInfo': {'name':'isolated-regression', 'version':'0.0.0'}}
    elif method == 'thread/list':
        result = {'data':[{'id':'audit-thread', 'name':'AAAAAAAAAAAAAAA', 'cwd':os.environ['HOME'], 'status':{'type':'idle'}}], 'nextCursor':'repeat' if mode == method else None}
    elif method == 'thread/loaded/list':
        result = {'data':['audit-thread'], 'nextCursor':'repeat' if mode == method else None}
    else:
        emit({'id':ident, 'error':{'code':-32601, 'message':'unsupported in fixture'}})
        continue
    emit({'id':ident, 'result':result})
    if method == 'thread/loaded/list' and mode in ('whole','split','eof') and not started:
        started = True
        threading.Thread(target=notify, daemon=True).start()
'''


def environment(home, log, mode):
    bindir = home / 'bin'
    bindir.mkdir()
    script = bindir / 'codex'
    script.write_text(MOCK, encoding='utf-8')
    script.chmod(0o755)
    return {'HOME':str(home), 'USERPROFILE':str(home), 'XDG_CONFIG_HOME':str(home/'config'),
            'XDG_DATA_HOME':str(home/'data'), 'XDG_CACHE_HOME':str(home/'cache'),
            'XDG_STATE_HOME':str(home/'state'), 'CODEX_HOME':str(home/'codex'),
            'PATH':str(bindir)+':/usr/bin:/bin', 'TERM':'xterm-256color', 'LANG':'C.UTF-8',
            'MOCK_LOG':str(log), 'MOCK_MODE':mode}


def stop(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=2)


def normalized(data):
    return re.sub(rb'\x1b\[[0-?]*[ -/]*[@-~]', b'', data)


def terminal_case(binary, output, mode):
    raw = bytearray()
    log = output / (mode + '.server.log')
    log.write_text('', encoding='utf-8')
    with tempfile.TemporaryDirectory(prefix='ctui-stdio-regression-') as temporary:
        home = Path(temporary)
        env = environment(home, log, mode)
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 180, 0, 0))
        before = termios.tcgetattr(slave)
        def setup():
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
        process = subprocess.Popen([str(binary)], stdin=slave, stdout=slave, stderr=slave,
                                   env=env, cwd=home, preexec_fn=setup, close_fds=True)
        started = time.monotonic()
        observed = False
        sent_quit = False
        try:
            while time.monotonic() - started < 10:
                trace = log.read_text(encoding='utf-8')
                screen_writes = normalized(raw)
                if mode == 'eof':
                    observed = b'closed' in screen_writes and b'stdout' in screen_writes and 'deliberate-eof' in trace
                else:
                    observed = b'ZZZ_READ_OK_ZZZ' in screen_writes and 'alive-after-notification' in trace
                if observed and not sent_quit:
                    os.write(master, b'q')
                    sent_quit = True
                ready, _, _ = select.select([master], [], [], 0.05)
                if ready:
                    try:
                        chunk = os.read(master, 65536)
                    except OSError as error:
                        if error.errno == errno.EIO:
                            break
                        raise
                    if not chunk:
                        break
                    raw.extend(chunk)
                if process.poll() is not None:
                    break
            if process.poll() is None and sent_quit:
                process.wait(timeout=3)
            after = termios.tcgetattr(slave)
        finally:
            stop(process)
            os.close(master)
            os.close(slave)
            (output / (mode + '.terminal.bin')).write_bytes(raw)
        require(observed, mode + ': expected UI state never observed; see retained raw terminal and mock log')
        require(process.returncode == 0, mode + ': TUI did not quit normally')
        require(before == after, mode + ': controlling terminal attributes not restored')
        require(b'decode App Server stdio JSON' not in raw, mode + ': legal JSON was corrupted')
    return {'mode':mode, 'passed':True, 'terminalStateObserved':True, 'terminalRestored':True}


def cursor_case(binary, output, method):
    log = output / (method.replace('/', '-') + '.server.log')
    with tempfile.TemporaryDirectory(prefix='ctui-cursor-regression-') as temporary:
        home = Path(temporary)
        env = environment(home, log, method)
        process = subprocess.Popen([str(binary), 'headless', 'threads', '--json'],
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   env=env, cwd=home, start_new_session=True)
        try:
            stdout, stderr = process.communicate(timeout=8)
        finally:
            stop(process)
        (output / (method.replace('/', '-') + '.json')).write_bytes(stdout)
        require(process.returncode == 3, method + ': incomplete inventory must exit degraded')
        result = json.loads(stdout)
        require(result.get('degraded') is True, method + ': partial scan reported healthy')
        require('repeated cursor' in result.get('source', {}).get('error', ''), method + ': missing cursor diagnostic')
        require(len(result.get('threads', [])) == 1, method + ': valid partial row was discarded')
        count = sum(line.startswith(method + ' ') for line in log.read_text(encoding='utf-8').splitlines())
        require(count == 2, method + ': repeated cursor did not stop before a third request')
        require(not stderr, method + ': unexpected process error')
    return {'method':method, 'passed':True, 'requests':count, 'degraded':True, 'partialRows':1}


def main():
    binary, output = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
    output.mkdir(parents=True, exist_ok=False)
    receipt = {'schema':'codex-tui/app-server-transport-regression/v1', 'status':'failed',
               'sourceSha':os.environ.get('CODEX_TUI_PTY_SOURCE_SHA'),
               'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
               'realCodexQualification':False, 'realSshQualification':False, 'checks':[]}
    try:
        for mode in ('whole', 'split', 'eof'):
            receipt['checks'].append(terminal_case(binary, output, mode))
        for method in ('thread/list', 'thread/loaded/list'):
            receipt['checks'].append(cursor_case(binary, output, method))
        receipt['status'] = 'passed'
    finally:
        (output/'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
