#!/usr/bin/env python3
"""Consistent SQLite backup to a new file; no restore, migration or qualification."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import time

from _compat import require_python38

DEADLINE_SECONDS = 30


def snapshot(source, output):
    if source.is_symlink() or not source.is_file():
        raise ValueError('source must be a regular file, not a symlink')
    source = source.resolve(strict=True)
    output = output.absolute()
    if output.exists() or output.is_symlink():
        raise FileExistsError('backup destination already exists')
    output.parent.mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix='.snapshot-', suffix='.sqlite3', dir=str(output.parent))
    os.close(fd)
    stage, started = Path(name), time.monotonic()
    src = dst = None

    def expired():
        return time.monotonic()-started > DEADLINE_SECONDS

    def progress(status, remaining, total):
        if expired():
            raise TimeoutError('backup deadline')
    try:
        src = sqlite3.connect(source.as_uri()+'?mode=ro', uri=True, timeout=1)
        dst = sqlite3.connect(str(stage), timeout=1)
        src.backup(dst, pages=128, progress=progress, sleep=.05)
        dst.set_progress_handler(lambda: int(expired()), 10000)
        if dst.execute('PRAGMA integrity_check').fetchall() != [('ok',)] or expired():
            raise ValueError('backup failed integrity/deadline check')
        dst.close(); dst = None
        src.close(); src = None
        h = hashlib.sha256()
        with stage.open('rb') as stream:
            for block in iter(lambda: stream.read(1024*1024), b''):
                if expired():
                    raise TimeoutError('backup digest deadline')
                h.update(block)
            os.fsync(stream.fileno())
        size = stage.stat().st_size
        os.link(str(stage), str(output))  # Same-directory, no-overwrite publication.
        stage.unlink()
        dfd = os.open(str(output.parent), os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(dfd)
        finally:
            os.close(dfd)
        return {'schema': 'codex-tui/device-sqlite-snapshot/v1', 'sha256': h.hexdigest(),
                'bytes': size, 'integrity': 'ok', 'sourceConnectionReadOnly': True,
                'qualificationGranted': False}
    finally:
        if dst is not None:
            dst.close()
        if src is not None:
            src.close()
        if stage.exists():
            stage.unlink()  # Only our unpublished random staging file.


def main():
    require_python38()
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--database', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    if sys.platform != 'linux':
        p.error('Linux-only field backup helper')
    os.umask(0o077)
    print(json.dumps(snapshot(a.database, a.output), indent=2))
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, sqlite3.Error, ValueError, TimeoutError):
        sys.exit('Snapshot failed; retain the original store and any diagnostic output.')
