from pathlib import Path
import json
import subprocess

p = Path('src/app.rs')
text = p.read_text()
if subprocess.check_output(['git','rev-parse','HEAD:src/app.rs'],text=True).strip() != 'f85dd2d348f8df0f9d0e8558deec1712881ad0e0':
    raise SystemExit('app source drift; refusing fuzzy patch')

def replace(old, new):
    global text
    if text.count(old) != 1:
        raise SystemExit('non-unique patch anchor: ' + old[:100])
    text = text.replace(old, new, 1)

def block(start, end, new):
    global text
    if text.count(start) != 1 or text.count(end) != 1:
        raise SystemExit('non-unique block anchor')
    a, b = text.index(start), text.index(end)
    if b <= a:
        raise SystemExit('reversed block anchors')
    text = text[:a] + new + text[b:]

replace('mod prompt;\n', 'mod prompt;\nmod queue_editor;\n')
replace('    pub pending_thread_queue_mutation: Option<ThreadQueueMutation>,\n',
        '    pub pending_thread_queue_mutation: Option<ThreadQueueMutation>,\n    thread_queue_editor: Option<queue_editor::QueueEditor>,\n')
replace('            pending_thread_queue_mutation: None,\n',
        '            pending_thread_queue_mutation: None,\n            thread_queue_editor: None,\n')
block('        Action::ThreadQueueLoaded(snapshot) => {\n', '        Action::ThreadQueueFailed { thread_id, error } => {\n',
      '        Action::ThreadQueueLoaded(snapshot) => state.install_thread_queue(snapshot),\n')
block('        Action::BeginThreadQueueAdd => {\n', '        Action::BeginThreadQueueDelete => {\n',
      '        Action::BeginThreadQueueAdd => state.begin_queue_input(false),\n        Action::BeginThreadQueueEdit => state.begin_queue_input(true),\n')
block('            if matches!(mode, InputMode::ThreadQueueAdd | InputMode::ThreadQueueEdit) {\n',
      '            if mode == InputMode::TranscriptSearch {\n',
      '            if matches!(mode, InputMode::ThreadQueueAdd | InputMode::ThreadQueueEdit) {\n                return state.commit_queue_input();\n            }\n')
replace('        Action::CloseThreadQueue => {\n', '        Action::CloseThreadQueue => {\n            state.thread_queue_editor = None;\n')
replace('        Action::OpenThreadQueue => {\n', '        Action::OpenThreadQueue => {\n            state.thread_queue_editor = None;\n')
replace('        Action::CancelInput => {\n', '        Action::CancelInput => {\n            state.thread_queue_editor = None;\n')
replace('                    state.thread_queue_selected = to;\n',
        '                    // Keep the cursor on the observed item until upstream confirms order.\n')
# Keep existing mutation/confirmation assertions, but no speculative cursor move.
replace('        let effects = reduce(&mut app, Action::ReorderThreadQueue(1));\n        assert_eq!(app.thread_queue_selected, 1);\n',
        '        let effects = reduce(&mut app, Action::ReorderThreadQueue(1));\n        assert_eq!(app.thread_queue_selected, 0);\n        assert_eq!(app.selected_thread_queue_submission().unwrap().id, "q1");\n')
p.write_text(text)
p = Path('release/v1.4-plan.json')
text = p.read_text()
anchor = '    "src/app/review.rs": 140,'
if text.count(anchor) != 1:
    raise SystemExit('module ratchet structure drift')
p.write_text(text.replace(anchor, '    "src/app/queue_editor.rs": 200,\n' + anchor, 1))
Path('docs/implementation/v1.4-queue-identity.md').write_text('''# v1.4 queue editor identity closure

Baseline: `aceadffe2a4357a8dbf4fcd4fac1675f3cc19c94` (#228).

The queue editor previously looked up the numeric selection at commit time.
Queue refresh, removal or navigation could silently redirect an update, and a
same-ID changed/multimodal item could be overwritten by a stale text draft.
An add editor could also follow a different active thread. The displayed cursor
moved speculatively on reorder before the upstream snapshot changed.

The editor now captures one ephemeral thread/item baseline. It does not persist
queue content or create a second queue authority. Commit requires the same live
thread, open queue and (for edits) an exactly matching original item in the latest
observed snapshot. A conflict preserves the draft, reports a bilingual refusal
and emits no mutation. Cancel, close and successful submission clear the baseline.
Queue refresh preserves exact item ID; reorder waits for authoritative ordering.
Unknown/removed-thread snapshots are rejected before installation.

Eleven deterministic production-reducer tests include eight defect regressions
and three positive controls. The existing reorder test now asserts unchanged
observed identity before acknowledgement; its mutation and confirmation checks
remain. The test-only/candidate logs retain failures as failures. A controlled
before/after workflow checks the full suite, then exports exact Git blobs; it and
its patch script must be absent from merged source and squash history.

The extracted helper has a 200-line ceiling; no existing ceiling is raised. No
Cargo or persistent schema change, new API call, human PASS, stable publication,
branch-protection bypass or additional product scope. These are local observation
checks, not server-side compare-and-swap: a concurrent remote update after the
last observation remains possible. API exactly-once and global event ordering
are not claimed. Real account/internal GitLab/controlling-TTY gates remain separate.
''')
