#!/usr/bin/env python3
"""Actual-program long-message navigation and follow; isolated Linux PTY only."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import struct
import termios
from runtime_fault_regression import Session
from app_server_transport_regression import require

MOCK = r'''#!/usr/bin/python3
import json, os, sys, threading, time
from pathlib import Path
root = Path(os.environ['TRANSCRIPT_PROBE_ROOT'])
lock = threading.RLock()
log = open(root/'server.jsonl','w',buffering=1)
appended = False
approval_sent = False
subscribed = False
row = {'id':'audit-long','name':'AUDIT_LONG_REPLY','cwd':str(root),
       'updatedAt':2000000000,'status':{'type':'idle'}}
long_text = 'FIRST_LINE\nSECOND_LINE\n\n' + '\n'.join(
    'LONG_LINE_%03d '%i + ('segment%03d '%i)*12 for i in range(1,201)) + '\nEND_LONG_MESSAGE_200'
(root/'expected-long-message.txt').write_text(long_text)
items = [{'turnId':'turn-1','item':{'id':'long-item','type':'agentMessage','text':long_text}}]
def emit(value):
    with lock: print(json.dumps(value,separators=(',',':')),flush=True)
def control():
    global appended, approval_sent
    while True:
        if (root/'append').exists() and not appended and subscribed:
            with lock:
                appended = True
                items.append({'turnId':'turn-2','item':{'id':'latest-item','type':'agentMessage',
                              'text':'FOLLOW_UP_LATEST_VISIBLE_SENTINEL'}})
                log.write(json.dumps({'control':'append-complete'})+'\n')
                emit({'method':'item/completed','params':{'threadId':'audit-long'}})
                row['name']='AUDIT_ACK_APPENDED'
                emit({'method':'thread/name/updated','params':{'threadId':'audit-long','threadName':row['name']}})
        if (root/'approval').exists() and not approval_sent and subscribed:
            with lock:
                approval_sent = True
                emit({'id':'fixture-approval', 'method':'item/commandExecution/requestApproval',
                      'params':{'threadId':'audit-long', 'turnId':'turn-2', 'itemId':'approval-long',
                                'command':'echo ' + 'detail '*800 + 'COMMAND_TAIL',
                                'cwd':'/fixture/CWD_DETAIL_SENTINEL',
                                'additionalPermissions':{'fileSystem':{'write':['/fixture/SCOPE_DETAIL_SENTINEL']}}}})
        time.sleep(.005)
threading.Thread(target=control,daemon=True).start()
for line in sys.stdin:
    request=json.loads(line)
    if 'id' not in request: continue
    method=request.get('method');params=request.get('params',{})
    log.write(json.dumps({'id':request['id'],'method':method,'params':params,'appended':appended})+'\n')
    if method is None: continue
    with lock:
        if method=='initialize': result={'serverInfo':{'name':'display-audit-fixture','version':'0.162.1'}}
        elif method=='thread/list': result={'data':[row],'nextCursor':None}
        elif method=='thread/loaded/list': result={'data':['audit-long'],'nextCursor':None}
        elif method=='thread/read': result={'thread':row}
        elif method=='thread/turns/list':
            turns=[{'id':'turn-1','status':'completed'}]
            if appended: turns.append({'id':'turn-2','status':'completed'})
            result={'data':list(reversed(turns)),'nextCursor':None}
        elif method=='thread/items/list': result={'data':list(reversed(items)),'nextCursor':None}
        elif method=='thread/resume':
            assert params.get('excludeTurns') is True
            subscribed = True
            result={'thread':row}
        elif method=='thread/unsubscribe':
            subscribed = False
            result={'status':'unsubscribed'}
        else:
            emit({'id':request['id'],'error':{'code':-32601,'message':'unsupported display fixture'}})
            continue
        emit({'id':request['id'],'result':result})
'''


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--source-sha")
    args = parser.parse_args()
    if args.source_sha is not None:
        require(re.fullmatch(r"[0-9a-f]{40}", args.source_sha), "invalid exact source SHA")
    binary, root = args.binary.resolve(strict=True), args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    shim = root / "codex-fixture"
    shim.write_text(MOCK, encoding="utf-8")
    shim.chmod(0o755)
    config = root / "config" / "codex-tui"
    config.mkdir(parents=True)
    (config / "config.toml").write_text(
        '[ui]\nlanguage="en"\n[app_server]\nactive="fixture"\n'
        '[app_server.targets.fixture]\ntransport="stdio"\ncodex_bin=' + json.dumps(str(shim)) + '\n',
        encoding="utf-8",
    )
    env = os.environ.copy()
    env.update(XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"),
               XDG_CACHE_HOME=str(root / "cache"), XDG_STATE_HOME=str(root / "state"),
               TRANSCRIPT_PROBE_ROOT=str(root), TERM="xterm-256color", LC_ALL="C.UTF-8")
    result = {"schema": "codex-tui/conversation-viewport-regression/v1",
              "sourceSha": args.source_sha,
              "binarySha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "fixtureOnly": True, "realAccountQualification": False, "manualQualification": False}
    app = Session(binary, root / "terminal", root, env)

    def snapshot(name):
        text = app.screen()
        (root / (name + ".screen.txt")).write_text(text, encoding="utf-8")
        return text

    try:
        app.wait(lambda: "1 total" in app.screen(), "Registry did not load")
        app.send(b"\r")
        app.wait(lambda: "END_LONG_MESSAGE_200" in app.screen(), "initial follow did not show tail")
        snapshot("initial-follow")
        app.send(b"\x1b[H")
        app.wait(lambda: "FIRST_LINE" in app.screen(), "Home did not show beginning")
        first = snapshot("home")
        require("SECOND_LINE" in first and "FIRST_LINE SECOND_LINE" not in first, "logical lines collapsed")
        seen = set()
        for _ in range(44):
            seen.update(re.findall(r"LONG_LINE_(\d{3})", app.screen()))
            app.send(b"\x1b[6~")
        require(seen == {str(n).zfill(3) for n in range(1, 201)}, "page navigation skipped message rows")
        require("END_LONG_MESSAGE_200" in snapshot("paged-to-tail"), "tail unreachable with PageDown")
        app.send(b"\x1b[H")
        app.wait(lambda: "FIRST_LINE" in app.screen(), "manual position did not reset")
        (root / "append").touch()
        app.wait(lambda: "items 1-2/2" in app.screen(), "new content was not loaded")
        manual = snapshot("append-while-manual")
        require("FIRST_LINE" in manual and "FOLLOW_UP_LATEST_VISIBLE_SENTINEL" not in manual,
                "new content moved a manually paused viewport")
        app.send(b"\x1b[F")
        app.wait(lambda: "FOLLOW_UP_LATEST_VISIBLE_SENTINEL" in app.screen(), "End did not restore follow")
        snapshot("end-follow")
        result["visibleNumberedLines"] = len(seen)

        # A small terminal must still expose the complete approval before a
        # decision. Browsing and closing details must never answer the request.
        fcntl.ioctl(app.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 14, 70, 0, 0))
        os.kill(app.proc.pid, signal.SIGWINCH)
        (root / "approval").touch()
        app.wait(lambda: "NEEDS YOU" in app.screen(), "approval summary was not visible")
        app.send(b"d")
        app.wait(lambda: "Request details" in app.screen(), "d did not open local request details")
        app.send(b"\x1b[H")
        app.wait(lambda: "Command approval: echo" in app.screen(), "complete approval did not open")
        snapshot("approval-details-home")
        app.send(b"\x1b[F")
        app.wait(lambda: "SCOPE_DETAIL_SENTINEL" in app.screen(), "approval scope tail was unreachable")
        details = snapshot("approval-details-end")
        require("COMMAND_TAIL" in details, "long approval command tail was unreachable")
        require("CWD_DETAIL_SENTINEL" in details, "approval cwd was unreachable")
        app.send(b"\x1b")
        app.wait(lambda: "NEEDS YOU" in app.screen() and "d full details" in app.screen()
                 and "Request details" not in app.screen(), "Esc did not return to pending approval")
        snapshot("approval-details-closed")
        records = [json.loads(line) for line in (root / "server.jsonl").read_text().splitlines()]
        require(not any(row["id"] == "fixture-approval" for row in records if "id" in row),
                "browsing details answered the approval")
        result["smallTerminalApprovalDetails"] = True
        result["detailsBrowsingDidNotRespond"] = True
        result["terminalRestoration"] = app.finish(signal.SIGTERM)
        result["status"] = "passed"
    except Exception as error:
        result["status"], result["error"] = "failed", str(error)
        raise
    finally:
        app.close()
        (root / "receipt.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
