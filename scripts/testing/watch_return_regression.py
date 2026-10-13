"""Real search-return validity and backend watch observations for provisional targets."""
import json
from pathlib import Path
from app_server_transport_regression import require
from runtime_fault_regression import Session, environment
from watch_removal_regression import mock_source

CASES = ('return-provisional-live', 'return-provisional-retired',
         'return-registered-live', 'return-registered-retired')


def validate_return(returned, keep, reads):
    require(returned == keep, 'search cancel restored the wrong return surface')
    require(reads == int(keep),
            'return watch expected {} history reads, observed {}'.format(int(keep), reads))


def case(binary, output, home, mode, original_mock, sample_resources):
    require(mode in CASES, 'unknown return scenario')
    provisional = mode.startswith('return-provisional-')
    keep = mode.endswith('-live')
    target = 'ghost' if provisional else 'watch-a'
    env = environment(home, home/'server.jsonl', mode)
    # The protocol is unchanged; the disposable interpreter needs no site packages.
    source = mock_source(original_mock).replace(
        '#!/usr/bin/python3\n', '#!/usr/bin/python3 -S\n', 1)
    (home/'bin/codex').write_text(source, encoding='utf-8')
    app = Session(binary, output, home, env)
    observations = []

    def records():
        files = list(home.glob('server.jsonl.*'))
        require(sum(f.stat().st_size for f in files) < 256*1024,
                'return fixture trace budget exceeded')
        result = []
        for path in files:
            text = path.read_text()
            result.extend(json.loads(row) for row in text[:text.rfind('\n')+1].splitlines())
        return result

    def boundary(seq, events):
        temporary = home/'control.tmp'
        temporary.write_text(json.dumps({'seq':seq, 'events':events}), encoding='utf-8')
        temporary.replace(home/'control.json')
        app.wait(lambda: 'ACK_{}_'.format(seq) in app.screen(), 'return actor fence missing')
        return sum(row['seq']==seq and row['method']=='thread/read' and
                   row['params'].get('threadId')==target for row in records())

    try:
        app.wait(lambda: '2 total' in app.screen(), 'return Registry missing')
        if provisional:
            app.send(b'\x06')
            app.send(b'ghost\r')
            app.wait(lambda: 'GHOST_HIT' in app.screen(), 'return search hit missing')
        app.send(b'\r')
        app.wait(lambda: 'BODY_0_'+target in app.screen(), 'return initial body missing')
        app.send(b'/')
        if not keep:
            reads = boundary(1, [{'method':'thread/deleted', 'params':{'threadId':target}}])
            require(reads == 0, 'retirement itself reloaded the target')
        app.send(b'\x1b')
        returned = 'BODY_0_'+target in app.screen()
        (output/'after-cancel.txt').write_text(app.screen(), encoding='utf-8')
        require(returned == keep, 'search cancel restored the wrong return surface')
        if returned:
            app.send(b'/')  # Expose registered-row fence while retaining the live return.
        reads = boundary(2, [{'method':'item/completed', 'params':{'threadId':target}}])
        validate_return(returned, keep, reads)
        observations.append({'returned':returned,'live':keep,'historyReads':reads,
                             'resources':sample_resources(app.proc.pid)})
        if returned:
            app.send(b'\x1b')
            app.wait(lambda: 'BODY_2_'+target in app.screen(),
                     'returned provisional view missed its live history update')
            app.send(b'\x1b')
        require(boundary(3, [{'method':'item/completed','params':{'threadId':target}}]) == 0,
                'leaving the restored view retained an idle history watch')
        return {'observations':observations,'terminal':app.finish()}
    finally:
        try:
            (output/'observations.json').write_text(json.dumps(observations,indent=2)+'\n')
            (output/'server.json').write_text(json.dumps(records(),indent=2)+'\n')
        finally:
            app.close()
