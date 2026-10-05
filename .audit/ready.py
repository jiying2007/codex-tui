from pathlib import Path
root=Path('.')
def replace(path,old,new):
 p=root/path;s=p.read_text();assert s.count(old)==1,(path,old[:60],s.count(old));p.write_text(s.replace(old,new))
p=root/'src/app/queue_confirmation.rs';s=p.read_text().replace('fn confirmed_queue_snapshot','pub(super) fn ready_queue_snapshot').replace('self.confirmed_queue_snapshot()','self.ready_queue_snapshot()').replace('fn refuse_queue_confirmation','pub(super) fn refuse_queue_mutation').replace('self.refuse_queue_confirmation()','self.refuse_queue_mutation()');p.write_text(s)
replace('src/app.rs','''        Action::ReorderThreadQueue(delta) => {
            let Some(snapshot) = state.thread_queue_snapshot.as_ref() else {
                return vec![];
            };''','''        Action::ReorderThreadQueue(delta) => {
            let Some(snapshot) = state.ready_queue_snapshot() else {
                state.refuse_queue_mutation();
                return vec![];
            };''')
replace('src/app/queue_editor.rs','''            let selected = self
                .thread_queue_snapshot
                .as_ref()
                .filter(|snapshot| snapshot.thread_id == thread_id)''','''            let selected = self
                .ready_queue_snapshot()
                .filter(|snapshot| snapshot.thread_id == thread_id)''')
replace('src/app/queue_editor.rs','''                    (Some(original), InputMode::ThreadQueueEdit) => self
                        .thread_queue_snapshot
                        .as_ref()
                        .filter''','''                    (Some(original), InputMode::ThreadQueueEdit) => self
                        .ready_queue_snapshot()
                        .filter''')
replace('tests/queue_editor_identity.rs','''    edit(&mut app);
    assert_eq!(
        reduce(&mut app, Action::CloseThreadQueue),''','''    load(&mut app, &id, &[("a", "first"), ("b", "second")]);
    edit(&mut app);
    assert_eq!(
        reduce(&mut app, Action::CloseThreadQueue),''')
p=root/'docs/implementation/v1.4-queue-confirmation.md';s=p.read_text();s=s.replace('There are 24 new tests:', 'There are 30 new tests:').replace('they produce 20 failures and four positive passes.', 'they produce 24 failures and six positive passes.').replace('all 572 Rust and 34 Python tests','all 578 Rust and 34 Python tests');s=s.replace('No partial list becomes authority for a reorder or confirmation.','''No partial list becomes authority for a reorder or confirmation.
The same ready-snapshot accessor also guards starting/committing edits and reordering;
a failed or in-flight refresh cannot authorize mutations using an older cached list.
An independent Add to the same live thread does not require a loaded queue list.''');p.write_text(s)
