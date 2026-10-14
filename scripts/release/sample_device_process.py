#!/usr/bin/env python3
"""Read-only Linux process-family diagnostics. Never grants field qualification.

Source SHA is an operator assertion; independently verify it with doctor first.
No command arguments, environment values, process names or transcript are saved.
"""
import argparse
import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import re
import sys
import time

from _compat import require_python38

PROC = Path('/proc')


def proc_row(pid):
    root = PROC / str(pid)
    if root.stat().st_uid != os.getuid():
        raise PermissionError('different UID')
    raw = (root/'stat').read_text()
    right = raw.rfind(')')
    if right < 0:
        raise ValueError('invalid process stat')
    fields = raw[right+2:].split()
    return {'pid': pid, 'state': fields[0], 'ppid': int(fields[1]),
            'startTicks': int(fields[19]), 'cpuTicks': int(fields[11])+int(fields[12])}


def metrics(row):
    root = PROC / str(row['pid'])
    try:
        status = dict(line.split(':', 1) for line in (root/'status').read_text().splitlines()
                      if ':' in line)
        io = dict(line.split(':', 1) for line in (root/'io').read_text().splitlines())
        result = dict(row, rssKiB=int(status['VmRSS'].split()[0]),
                      hwmKiB=int(status['VmHWM'].split()[0]),
                      fdCount=len(list((root/'fd').iterdir())),
                      readBytes=int(io['read_bytes']), writeBytes=int(io['write_bytes']))
        if proc_row(row['pid'])['startTicks'] != row['startTicks']:
            return None
        return result
    except (OSError, ValueError, IndexError, KeyError):
        return None  # Unknown observation, never a fabricated zero measurement.


def active_family(table, seen):
    active = {pid for pid, ticks in seen.items()
              if pid in table and table[pid]['startTicks'] == ticks}
    changed = True
    while changed:
        changed = False
        for pid, row in table.items():
            if pid not in active and row['ppid'] in active:
                active.add(pid)
                changed = True
    # Retain observed reparented children while alive; don't grow an exit history.
    return {pid: table[pid]['startTicks'] for pid in active}


def image_identity(pid, binary):
    image = PROC/str(pid)/'exe'
    if not os.path.samefile(str(image), str(binary)):
        raise ValueError('PID does not execute selected binary')
    with image.open('rb') as stream:
        info = os.fstat(stream.fileno())
        h = hashlib.sha256()
        for data in iter(lambda: stream.read(1024*1024), b''):
            h.update(data)
    return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns), h.hexdigest()


def same_image(pid, binary, expected):
    try:
        a, b = (PROC/str(pid)/'exe').stat(), binary.stat()
        return all((s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns) == expected for s in (a, b))
    except OSError:
        return False


def main():
    require_python38()
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--pid', type=int, required=True)
    p.add_argument('--source-sha', required=True)
    p.add_argument('--binary', type=Path, required=True)
    p.add_argument('--seconds', type=float, default=28800)
    p.add_argument('--interval', type=float, default=5)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    if sys.platform != 'linux' or a.pid < 1:
        p.error('Linux and a positive PID are required')
    if not re.fullmatch('[0-9a-f]{40}', a.source_sha):
        p.error('Invalid SHA')
    if not (math.isfinite(a.seconds) and 0 < a.seconds <= 172800):
        p.error('Invalid duration')
    if not (math.isfinite(a.interval) and .05 <= a.interval <= 60):
        p.error('Invalid interval')
    os.umask(0o077)
    summary_path = Path(str(a.output)+'.summary.json')
    if any(path.exists() or path.is_symlink() for path in (a.output, summary_path)):
        p.error('Outputs exist; choose fresh paths')
    initial = proc_row(a.pid)
    binary = a.binary.resolve(strict=True)
    image, digest = image_identity(a.pid, binary)
    if proc_row(a.pid)['startTicks'] != initial['startTicks']:
        p.error('PID changed during preflight')
    seen = {a.pid: initial['startTicks']}
    started, count, incomplete = time.monotonic(), 0, 0
    reason, peak_rss, peak_fds, last = 'duration-complete', 0, 0, []
    identity = {'sourceSha': a.source_sha, 'sourceShaIndependentlyVerified': False,
                'binarySha256': digest, 'rootPid': a.pid, 'startTicks': initial['startTicks']}
    with a.output.open('x', encoding='utf-8') as stream:
        stream.write(json.dumps(dict(identity, type='identity',
                     clockTicksPerSecond=os.sysconf('SC_CLK_TCK'),
                     requestedSeconds=a.seconds, intervalSeconds=a.interval))+'\n')
        try:
            while True:
                table = {}
                for entry in PROC.iterdir():
                    if not entry.name.isdigit():
                        continue
                    try:
                        row = proc_row(int(entry.name)); table[row['pid']] = row
                    except (OSError, ValueError, IndexError):
                        continue
                seen = active_family(table, seen)
                root_alive = (table.get(a.pid, {}).get('startTicks') == initial['startTicks']
                              and table[a.pid]['state'] != 'Z')
                unchanged = root_alive and same_image(a.pid, binary, image)
                rows = [metrics(table[pid]) for pid in sorted(seen)]
                missing = sum(row is None for row in rows)
                rows = [row for row in rows if row is not None]
                complete = missing == 0 and root_alive and unchanged
                incomplete += int(not complete)
                elapsed = time.monotonic()-started
                rss, fds = sum(r['rssKiB'] for r in rows), sum(r['fdCount'] for r in rows)
                peak_rss, peak_fds = max(peak_rss, rss), max(peak_fds, fds)
                stream.write(json.dumps({'type': 'sample', 'elapsedSeconds': elapsed,
                    'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                    'rootAlive': root_alive, 'rootImageUnchanged': unchanged,
                    'measurementComplete': complete, 'unreadableObservedProcesses': missing,
                    'familyRssKiB': rss if complete else None,
                    'familyFds': fds if complete else None, 'processes': rows})+'\n')
                stream.flush(); count += 1; last = rows
                if not root_alive:
                    reason = 'root-exited-or-identity-lost'; break
                if not unchanged:
                    reason = 'root-image-changed'; break
                if elapsed >= a.seconds:
                    break
                time.sleep(min(a.interval, max(0, a.seconds-elapsed)))
        except KeyboardInterrupt:
            reason = 'operator-interrupted'
    summary = dict(identity, schema='codex-tui/device-process-observation/v1',
                   endReason=reason, elapsedSeconds=time.monotonic()-started,
                   requestedSeconds=a.seconds, sampleCount=count,
                   incompleteSampleCount=incomplete,
                   peakObservedRssKiB=peak_rss, peakObservedFds=peak_fds,
                   lastProcesses=last, qualificationGranted=False,
                   limits='sampled same-UID descendants only; missed/reparented-before-observation '
                          'and unrelated shared daemons excluded; no global resource/RPC guarantee')
    with summary_path.open('x', encoding='utf-8') as stream:
        json.dump(summary, stream, indent=2); stream.write('\n')
    print(json.dumps(summary, indent=2))
    return 0 if reason == 'duration-complete' and incomplete == 0 else 3


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, IndexError):
        sys.exit('Observation failed; inspect the private environment and retained samples.')
