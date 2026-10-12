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
import unicodedata


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
def inventory_fault():
    if mode.endswith('-eof'):
        os._exit(0)
    if mode.endswith('-malformed'):
        os.write(1, b'{invalid-json}\n')
        return
    count = 1025 if mode.endswith('-count') else 70
    text = 'x' if mode.endswith('-count') else 'x' * (256 * 1024)
    for _ in range(count):
        emit({'method':'thread/name/updated', 'params':{'threadId':'audit-thread', 'threadName':text}})
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
        if mode.startswith('registry-') and request.get('params', {}).get('cursor') is not None:
            inventory_fault()
            continue
        result = {'data':[{'id':'audit-thread', 'name':'AAAAAAAAAAAAAAA', 'cwd':os.environ['HOME'], 'status':{'type':'idle'}}], 'nextCursor':'repeat' if mode == method else ('registry-next' if mode.startswith('registry-') else None)}
    elif method == 'thread/loaded/list':
        if mode == 'loaded-unsupported':
            emit({'id':ident, 'error':{'code':-32601, 'message':'optional method unavailable'}})
            continue
        if mode.startswith('loaded-'):
            inventory_fault()
            continue
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


def screen_text(data):
    """Reconstruct this fixed Ratatui fixture, not a general terminal emulator.

    Raw ANSI-stripped output is not screen text: unchanged cells are not repainted.
    Unexpected cursor controls fail closed; an incomplete trailing CSI waits for bytes.
    """
    grid = [[' '] * 180 for _ in range(40)]
    row = col = offset = 0
    text = bytes(data).decode('utf-8', errors='replace')
    while offset < len(text):
        char = text[offset]
        if char == '\x1b':
            match = re.match(r'\x1b\[([0-?]*)([ -/]*)([@-~])', text[offset:])
            if not match:
                require(text[offset:] == '\x1b' or text[offset:].startswith('\x1b['), 'unexpected non-CSI control')
                break
            arguments, intermediate, operation = match.groups()
            require(not intermediate, 'unexpected CSI intermediate')
            if operation in ('H', 'f'):
                values = (arguments or '1;1').split(';')
                row = int(values[0] or '1') - 1
                col = int(values[1] or '1') - 1 if len(values) > 1 else 0
                require(0 <= row < 40 and 0 <= col < 180, 'cursor outside fixture grid')
            elif operation == 'J':
                require(arguments in ('2', '3'), 'unsupported partial erase')
                grid = [[' '] * 180 for _ in range(40)]
            elif operation not in ('m', 'h', 'l'):
                raise RuntimeError('unexpected fixture CSI: ' + repr(match.group()))
            offset += len(match.group())
            continue
        if char == '\r':
            col = 0
        elif char == '\n':
            row = min(row + 1, 39)
        elif ord(char) >= 32 and not unicodedata.combining(char):
            if col < 180:
                grid[row][col] = char
                width = 2 if unicodedata.east_asian_width(char) in ('W', 'F') else 1
                if width == 2 and col + 1 < 180:
                    grid[row][col + 1] = ''
                col += width
        offset += 1
    return '\n'.join(''.join(line) for line in grid)


def check_observer():
    sparse = b'\x1b[1;1HAAA\x1b[1;1HZ\x1b[1;3HZ'
    require(screen_text(sparse).startswith('ZAZ'), 'observer lost an unchanged cell')
    require(screen_text(sparse + b'\x1b[2;').startswith('ZAZ'), 'observer lost a fragmented CSI')
    require(screen_text(sparse + b'\x1b[2;1HB').splitlines()[1].startswith('B'), 'observer cursor reconstruction failed')
    try:
        screen_text(b'\x1b[2A')
    except RuntimeError:
        pass
    else:
        raise RuntimeError('observer accepted an unsupported cursor operation')


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
                screen_writes = screen_text(raw)
                if mode == 'eof':
                    observed = 'closed' in screen_writes and 'stdout' in screen_writes and 'deliberate-eof' in trace
                else:
                    observed = 'ZZZ_READ_OK_ZZZ' in screen_writes and 'alive-after-notification' in trace
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
            (output / (mode + '.screen.txt')).write_text(screen_text(raw), encoding='utf-8')
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
        require(result.get('source', {}).get('connected') is True, method + ': pagination warning poisoned an intact session')
        require('repeated cursor' in result.get('source', {}).get('error', ''), method + ': missing cursor diagnostic')
        require(len(result.get('threads', [])) == 1, method + ': valid partial row was discarded')
        count = sum(line.startswith(method + ' ') for line in log.read_text(encoding='utf-8').splitlines())
        require(count == 2, method + ': repeated cursor did not stop before a third request')
        require(not stderr, method + ': unexpected process error')
    return {'method':method, 'passed':True, 'requests':count, 'degraded':True, 'partialRows':1}


def probe_fault_case(binary, output, mode):
    log = output / (mode + '.server.log')
    with tempfile.TemporaryDirectory(prefix='ctui-probe-fault-') as temporary:
        home = Path(temporary)
        process = subprocess.Popen([str(binary), 'headless', 'threads', '--json'],
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   env=environment(home, log, mode), cwd=home, start_new_session=True)
        try:
            stdout, stderr = process.communicate(timeout=8)
        finally:
            stop(process)
        (output / (mode + '.json')).write_bytes(stdout)
        result = json.loads(stdout)
        require(process.returncode == 3 and result.get('degraded') is True, mode + ': fault not degraded')
        connected = mode == 'loaded-unsupported'
        require(result.get('source', {}).get('connected') is connected, mode + ': wrong usable-connection state')
        require(bool(result.get('source', {}).get('error')), mode + ': missing fault diagnostic')
        require(len(result.get('threads', [])) == 1, mode + ': valid partial inventory was discarded')
        error = result['source']['error']
        if mode.startswith('registry-'):
            require(error.startswith('Registry inventory incomplete:'), mode + ': original Registry fault was overwritten')
        if mode.endswith('-count'):
            require('queued-message capacity exceeded' in error, mode + ': wrong count-limit diagnosis')
        if mode.endswith('-bytes'):
            require('queued-message retained-byte budget exceeded' in error, mode + ': wrong byte-limit diagnosis')
        if mode == 'loaded-unsupported':
            require('optional method unavailable' in error, mode + ': wrong optional-method diagnosis')
        methods = [line.split(' ', 1)[0] for line in log.read_text(encoding='utf-8').splitlines() if not line.startswith('pid ')]
        expected = ['initialize', 'thread/list', 'thread/list' if mode.startswith('registry-') else 'thread/loaded/list']
        require(methods == expected, mode + ': unexpected later request or mutation')
        require(not stderr, mode + ': unexpected process error')
    return {'mode':mode, 'passed':True, 'connected':connected, 'partialRows':1}



def main():
    check_observer()
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
        for mode in ('loaded-eof', 'loaded-malformed', 'loaded-count', 'loaded-bytes',
                     'registry-count', 'registry-bytes', 'registry-eof', 'registry-malformed',
                     'loaded-unsupported'):
            receipt['checks'].append(probe_fault_case(binary, output, mode))
        receipt['status'] = 'passed'
    finally:
        (output/'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
