from pathlib import Path
import sys,json

def replace(path,old,new):
    p=Path(path);s=p.read_text();assert s.count(old)==1,(path,old[:100],s.count(old));p.write_text(s.replace(old,new))
if sys.argv[1]=='tests':
    Path('tests/local_edit_durability.rs').write_text(Path('.audit/local_edit_durability.rs').read_text())
    raise SystemExit(0)
assert sys.argv[1]=='fix'
for src,dst in [('local_edit.rs','src/app/local_edit.rs'),('runtime_local_edit.rs','src/runtime_local_edit.rs'),('runtime_local_tests.rs','src/runtime_local_edit/tests.rs')]:
    p=Path(dst);p.parent.mkdir(parents=True,exist_ok=True);p.write_text(Path('.audit',src).read_text())
replace('src/app.rs','mod lifecycle;','mod lifecycle;\nmod local_edit;')
replace('src/app.rs','    planning_generation: u64,','    planning_generation: u64,\n    local_edit_revision: u64,\n    pending_local_edit: Option<local_edit::PendingLocalEdit>,')
replace('src/app.rs','            planning_generation: 0,','            planning_generation: 0,\n            local_edit_revision: 0,\n            pending_local_edit: None,')
replace('src/app.rs','pub fn reduce(state: &mut AppState, action: Action) -> Vec<Effect> {','pub fn reduce(state: &mut AppState, action: Action) -> Vec<Effect> {\n    state.observe_local_edit_action(&action);')
replace('src/app.rs','''            let view = editor.draft.clone();
            state.saved_view_editor = None;
            state.saved_view_editor_error = None;
            state.input_mode = InputMode::Normal;
            state.input_buffer.clear();
            return vec![Effect::SaveSavedView { view }];''','''            let view = editor.draft.clone();
            return state.begin_local_edit_write(Effect::SaveSavedView { view });''')
replace('src/app.rs','''                let workspace = state.new_scratch_workspace.take();
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![Effect::CreateScratch { title, workspace }];''','''                let workspace = state.new_scratch_workspace.clone();
                return state.begin_local_edit_write(Effect::CreateScratch { title, workspace });''')
replace('src/app.rs','''            if mode == InputMode::SavedViewField {
                let value = state.input_buffer.clone();
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                let Some(editor) = state.saved_view_editor.as_mut() else {
                    return vec![];
                };
                if let Err(error) = editor.commit_text_value(value) {
                    state.saved_view_editor_error = Some(error);
                } else {
                    state.saved_view_editor_error = None;
                }
                return vec![];
            }''','''            if mode == InputMode::SavedViewField {
                let value = state.input_buffer.clone();
                let Some(editor) = state.saved_view_editor.as_mut() else {
                    return vec![];
                };
                if let Err(error) = editor.commit_text_value(value) {
                    state.saved_view_editor_error = Some(error);
                } else {
                    state.saved_view_editor_error = None;
                    state.input_mode = InputMode::Normal;
                    state.input_buffer.clear();
                }
                return vec![];
            }''')
replace('src/app.rs','''            if mode == InputMode::Note {
                let text = state.input_buffer.trim().to_string();
                let Some(target) = state.note_target.take() else {
                    state.input_mode = InputMode::Normal;
                    state.input_buffer.clear();
                    return vec![];
                };
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                if target.kind == SourceKind::ScratchWork {
                    return vec![Effect::UpdateScratchNote {
                        scratch_id: target.value,
                        note: (!text.is_empty()).then_some(text),
                    }];
                }
                return vec![Effect::SaveSourceNote {
                    owner: target,
                    text,
                }];
            }''','''            if mode == InputMode::Note {
                let text = state.input_buffer.trim().to_string();
                let Some(target) = state.note_target.clone() else {
                    return vec![];
                };
                if target.kind == SourceKind::ScratchWork {
                    return state.begin_local_edit_write(Effect::UpdateScratchNote {
                        scratch_id: target.value,
                        note: (!text.is_empty()).then_some(text),
                    });
                }
                return state.begin_local_edit_write(Effect::SaveSourceNote {
                    owner: target,
                    text,
                });
            }''')
p=Path('src/runtime_store_worker.rs');s=p.read_text()
s=s.replace('Planning(PlanningResult, Option<String>),','Planning(PlanningResult, Option<String>, Option<u64>),\n    Stopped,')
s=s.replace('StoreEvent::Planning(Err(error), _)','StoreEvent::Planning(Err(error), _, _)')
s=s.replace('StoreEvent::Planning(Err("accepted write failed".into()), None)','StoreEvent::Planning(Err("accepted write failed".into()), None, None)')
old='''        self.submit(move |store| StoreEvent::Planning(job(store), notice))
    }'''
assert s.count(old)==1
s=s.replace(old,'''        self.planning_with_ticket(job, notice, None)
    }
    pub(crate) fn planning_with_ticket<F>(
        &mut self, job: F, notice: Option<String>, ticket: Option<u64>,
    ) -> std::result::Result<(), String>
    where F: FnOnce(&mut StoreBackend) -> PlanningResult + Send + 'static {
        self.submit(move |store| StoreEvent::Planning(job(store), notice, ticket))
    }
    #[cfg(test)]
    pub(crate) fn at_for_test(root: &std::path::Path) -> Self {
        Self::start(StoreBackend::at_for_test(root)).unwrap()
    }''')
old='''                Some(StoreEvent::Operator(Some(
                    "local state worker stopped".into(),
                )))'''
assert s.count(old)==1;s=s.replace(old,'                Some(StoreEvent::Stopped)');p.write_text(s)
replace('src/main.rs','mod runtime_input;','mod runtime_input;\nmod runtime_local_edit;')
p=Path('src/main.rs');s=p.read_text()
a=s.index('                match event {\n                    StoreEvent::Operator(error) => {')
b=s.index('                needs_render = true;',a)
s=s[:a]+'                planning_dirty |= runtime_local_edit::apply_event(&mut app, event);\n'+s[b:]
old='    for effect in effects {\n        match effect {'
assert s.count(old)==1
s=s.replace(old,'    for effect in effects {\n        let local_ticket = app.local_edit_ticket_for(&effect);\n        match effect {')
for label,next_label,body in [
('Effect::CreateScratch { title, workspace } => {','Effect::SnoozeWorkCard {','runtime_local_edit::submit(app, store, local_ticket, move |store| store.create_scratch(title, workspace));'),
('Effect::SaveSourceNote { owner, text } => {','Effect::UpdateScratchNote {','runtime_local_edit::submit(app, store, local_ticket, move |store| store.save_source_note(owner, text));'),
('Effect::UpdateScratchNote { scratch_id, note } => {','Effect::CreateBookmark {','runtime_local_edit::submit(app, store, local_ticket, move |store| store.update_scratch_note(scratch_id, note));'),
('Effect::SaveSavedView { view } => {','Effect::DeleteSavedView {','runtime_local_edit::submit(app, store, local_ticket, move |store| store.save_view(view));')]:
    a=s.index('            '+label);b=s.index('            '+next_label,a)
    s=s[:a]+'            '+label+'\n                '+body+'\n            }\n'+s[b:]
s=s.replace('use runtime_store_worker::{StoreEvent, StoreWorker as RuntimeStore};','use runtime_store_worker::StoreWorker as RuntimeStore;')
p.write_text(s)
p=Path('release/v1.4-plan.json');data=json.loads(p.read_text())
def add_caps(obj):
    if isinstance(obj,dict):
        if 'src/app/palette_intent.rs' in obj:
            obj['src/app/local_edit.rs']=240
            obj['src/runtime_local_edit.rs']=150
            obj['src/runtime_local_edit/tests.rs']=310
            return True
        return any(add_caps(v) for v in obj.values())
    if isinstance(obj,list): return any(add_caps(v) for v in obj)
    return False
assert add_caps(data),'module cap map absent'
p.write_text(json.dumps(data,indent=2,ensure_ascii=False)+'\n')
p=Path('docs/implementation/v1.4-local-edit-durability.md')
p.write_text('''# Local editor write receipts\n\nFrozen v1.4 defect follow-up from f7ad98c092214977f2c2512a448940a4f3445ec1.\n\nScratch creation, thread/source notes, scratch notes and Saved Views used to clear\nthe editor before admission to the bounded SQLite queue. Failed admission or\nwrite/refresh could therefore lose the only draft. Invalid visible-field input\nalso cleared before validation.\n\nThe original operation still owns its existing typed Effect. One ephemeral\nin-flight editor slot adds a process-unique ticket and editor revision. Only the\nmatching ordered worker receipt can retire unchanged input. Later input, including\nedit/undo and cancel/reopen, is retained. Duplicate submission is refused while\npending, but editing/navigation/explicit close remain responsive. Closing does\nnot undo an accepted write, and a later failure does not resurrect discarded input.\n\nQueue refusal retains the draft without declaring a healthy database read-only.\nAccepted failures and stopped workers report an unconfirmed outcome, never a\nrollback or a safe automatic retry. A write may have committed before projection\nrefresh failed. Existing backend read-only and exit-barrier error handling remain.\nUnrelated planning receipts cannot close an editor. Invalid Saved View fields stay\nin the text editor for correction. No schema, Cargo, RPC or persistent shadow copy\nwas added. Existing module limits and real qualification/publication gates remain.\n\nValidation uses production reducers, actual bounded StoreWorker admission and\nSQLite writes, old/matching receipts, full-queue recovery, write failure, injected\nworker termination and the final-save barrier. The before/after and final-main\nresults belong in the PR evidence; no local execution is asserted when unavailable.\n\nScope limits: at most one pending local editor write; drafts remain memory-only\nwhile editing. Explicit exit/SIGKILL/power loss is not draft durability certification.\nThis is not multi-process CAS or a cross-table atomic write-plus-projection read.\nSynthetic diagnostics do not establish human/token ROI or real terminal SLO.\n''')
for p in Path('.audit').glob('*'): p.unlink()
Path('.audit').rmdir()
Path('.github/workflows/audit-local-edit.yml').unlink()
