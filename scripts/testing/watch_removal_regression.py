"""Actor-side removal of provisional history targets, observed through real TUI."""
import json
from pathlib import Path

from app_server_transport_regression import require
from runtime_fault_regression import Session, environment

CASES = ['removal-provisional-keep', 'removal-provisional-delete',
         'removal-provisional-archive', 'removal-registered-keep',
         'removal-registered-delete', 'removal-registered-archive']


def mock_source(base):
    # Auxiliary search gets a separate log and never emits the main actor's
    # notification fences. A change in the base fixture fails visibly.
    replacements = [
        ("log = open(os.environ['MOCK_LOG'], 'w', buffering=1)",
         "log = open(os.environ['MOCK_LOG']+'.'+str(os.getpid()), 'a', buffering=1)"),
        ("threading.Thread(target=control, daemon=True).start()",
         "control_started = False"),
        ("if m=='turn/start':",
         "if m=='thread/list' and not control_started:\n"
         "        control_started = True\n"
         "        threading.Thread(target=control, daemon=True).start()\n"
         "    if m=='turn/start':"),
        ("if event['method']=='thread/deleted':",
         "if event['method'] in ('thread/deleted','thread/archived'):"),
        ("elif m=='thread/loaded/list':",
         "elif m=='thread/search': out={'data':[{'thread':{'id':'ghost'},'snippet':'GHOST_HIT'}],'nextCursor':None}\n"
         "        elif m=='thread/searchOccurrences': out={'data':[{'turnId':'turn-1','itemId':'item-1','snippet':'GHOST_HIT'}],'nextCursor':None}\n"
         "        elif m=='thread/loaded/list':"),
        ("'text':'BODY_'+str(seq)", "'text':'BODY_'+str(seq)+'_'+str(ident)"),
    ]
    for old, new in replacements:
        require(base.count(old) == 1, 'watch removal fixture source drift')
        base = base.replace(old, new, 1)
    return base


def validate_observations(observations, keep):
    require([item['seq'] for item in observations] == [1, 2, 3, 4],
            'incomplete or reordered removal observations')
    expected = [1, 0, int(keep), int(keep)]
    for item, reads in zip(observations, expected):
        require(item['expectedHistoryReads'] == reads,
                'removal observation expectation changed')
        require(item['actualHistoryReads'] == reads,
                'removal phase {}: expected {} historical reads, observed {}'.format(
                    item['seq'], reads, item['actualHistoryReads']))


def case(binary, output, home, mode, original_mock, sample_resources):
    require(mode in CASES, 'unknown removal scenario')
    short = mode[len('removal-'):]
    log = home / 'server.jsonl'
    env = environment(home, log, mode)
    (home / 'bin/codex').write_text(mock_source(original_mock), encoding='utf-8')
    app = Session(binary, output, home, env)
    observations = []

    def records():
        files = list(home.glob('server.jsonl.*'))
        require(sum(p.stat().st_size for p in files) <= 256 * 1024,
                'removal fixture trace budget exceeded')
        return [json.loads(line) for p in files for line in p.read_text().splitlines()]

    def boundary(seq, events, expected, target):
        value = {'seq': seq, 'events': events}
        temporary = home / 'control.tmp'
        temporary.write_text(json.dumps(value), encoding='utf-8')
        temporary.replace(home / 'control.json')
        # The later name notification can be rendered only after the actor has
        # completed any history RPC triggered by the preceding boundary.
        app.wait(lambda: 'ACK_{}_'.format(seq) in app.screen(),
                 'removal notification fence not observed')
        reads = sum(r['seq'] == seq and r['method'] == 'thread/read'
                    and r['params'].get('threadId') == target for r in records())
        observations.append({'seq': seq, 'actualHistoryReads': reads,
                             'expectedHistoryReads': expected,
                             'resources': sample_resources(app.proc.pid)})

    keep = short.endswith('-keep')
    try:
        app.wait(lambda: '2 total' in app.screen(), 'removal Registry missing')
        if short.startswith('provisional'):
            app.send(b'\x06')
            app.send(b'ghost\r')
            app.wait(lambda: 'GHOST_HIT' in app.screen(), 'provisional search hit missing')
            app.send(b'\r')
            target = 'ghost'
        else:
            app.send(b'\r')
            target = 'watch-a'
        app.wait(lambda: 'BODY_0_' + target in app.screen(),
                 'removal initial history missing')
        # Search launched from Board retains a reachable return to the target
        # and exposes the main actor's registered-row notification fence.
        app.send(b'b')
        app.send(b'/')
        boundary(1, [{'method': 'item/completed', 'params': {'threadId': target}}], 1, target)
        if keep:
            events = [{'method': 'thread/name/updated',
                       'params': {'threadId': 'watch-b', 'threadName': 'not-removal'}}]
        else:
            method = 'thread/archived' if short.endswith('-archive') else 'thread/deleted'
            events = [{'method': method, 'params': {'threadId': target}}]
        boundary(2, events, 0, target)
        for seq in [3, 4]:
            boundary(seq, [{'method': 'item/completed', 'params': {'threadId': target}}],
                     int(keep), target)
        for _ in range(3):
            app.send(b'\x1b')
        terminal = app.finish()
        validate_observations(observations, keep)
        return {'observations': observations, 'terminal': terminal}
    finally:
        try:
            (output / 'observations.json').write_text(
                json.dumps(observations, indent=2) + '\n', encoding='utf-8')
            (output / 'server.json').write_text(
                json.dumps(records(), indent=2) + '\n', encoding='utf-8')
        finally:
            app.close()
