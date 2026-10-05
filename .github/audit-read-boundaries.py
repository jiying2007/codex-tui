"""One-shot exact baseline patch; removed before final-source verification."""
from pathlib import Path
import json

def replace(path, old, new, count=1):
    p=Path(path); text=p.read_text()
    if text.count(old) != count:
        raise SystemExit(f'{path}: expected {count} exact matches for {old[:90]!r}, got {text.count(old)}')
    p.write_text(text.replace(old,new))

replace('src/lib.rs','pub mod git;','pub mod git;\nmod latest_read;')
for module, name, constant in [('git','Git','GIT'),('forge','Forge','FORGE')]:
    path=f'src/{module}.rs'
    p=Path(path); text=p.read_text()
    text='use crate::latest_read::{ReadEnvelope, ReadFence, ReadScope, next_current};\n'+text
    text=text.replace(f'mpsc::Sender<{name}Command>',f'mpsc::Sender<ReadEnvelope<{name}Command>>')
    text=text.replace(f'mpsc::Receiver<{name}Command>',f'mpsc::Receiver<ReadEnvelope<{name}Command>>')
    text=text.replace(f'mpsc::Sender<{name}Event>',f'mpsc::Sender<ReadEnvelope<{name}Event>>')
    text=text.replace(f'mpsc::Receiver<{name}Event>',f'mpsc::Receiver<ReadEnvelope<{name}Event>>')
    marker='    task: JoinHandle<()>,\n'
    if text.count(marker) != 1: raise SystemExit('unexpected actor handle')
    text=text.replace(marker,marker+'    reads: ReadFence,\n',1)
    marker='            task,\n'
    if text.count(marker) != 1: raise SystemExit('unexpected handle construction')
    text=text.replace(marker,marker+'            reads: ReadFence::default(),\n',1)
    start=text.index(f'    fn queue_command(&self, command: {name}Command) -> Result<()> {{')
    end=text.index(f'    pub fn try_recv(&mut self) -> Option<{name}Event>',start)
    scope=(f'{name}Command::LoadReview {{ thread_id, .. }} => ReadScope::Review(thread_id.0.clone()),' if name=='Git'
           else 'ForgeCommand::ProbeReview(target) => ReadScope::Review(target.thread_id.0.clone()),')
    method=f'''    fn queue_command(&self, command: {name}Command) -> Result<()> {{
        let scope = match &command {{
            {name}Command::Probe {{ cwd, .. }} => ReadScope::Snapshot(cwd.clone()),
            {scope}
        }};
        self.reads.submit(&self.command_tx, scope, command, "{name}")
    }}

'''
    text=text[:start]+method+text[end:]
    marker='        self.event_rx.try_recv().ok()'
    if text.count(marker)!=1: raise SystemExit('unexpected event consumer')
    text=text.replace(marker,f'        next_current(&mut self.event_rx, {constant}_EVENT_QUEUE_CAPACITY)',1)
    marker='                    Some(command) => {'
    if text.count(marker)!=1: raise SystemExit('unexpected actor admission')
    text=text.replace(marker,'                    Some(ReadEnvelope { value: command, ticket }) => {',1)
    marker='let _ = event_tx.send(event).await;'
    if text.count(marker)!=1: raise SystemExit('unexpected actor delivery')
    text=text.replace(marker,'let _ = event_tx.send(ReadEnvelope { value: event, ticket }).await;',1)
    p.write_text(text)

replace('src/forge.rs','ForgeReviewSummary, ForgeReviewTarget,','ForgeReviewResult, ForgeReviewSummary, ForgeReviewTarget,')
replace('src/forge.rs','    Review(ForgeReviewSummary),','    Review(Box<ForgeReviewResult>),')
replace('src/forge.rs','                                    ForgeEvent::Review(provider.probe_review(target).await)',
'''                                    let summary = provider.probe_review(target.clone()).await;
                                    ForgeEvent::Review(Box::new(ForgeReviewResult { target, summary }))''')
replace('src/app/types.rs','ForgeObservation, ForgeReviewSummary, ForgeReviewTarget','ForgeObservation, ForgeReviewResult, ForgeReviewTarget')
replace('src/app/types.rs','    ForgeReviewLoaded(ForgeReviewSummary),','    ForgeReviewLoaded(Box<ForgeReviewResult>),')

result_type='''/// In-memory result retaining the exact target that the actor actually requested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeReviewResult {
    pub target: ForgeReviewTarget,
    pub summary: ForgeReviewSummary,
}
impl ForgeReviewResult {
    pub fn unavailable(target: ForgeReviewTarget, error: String) -> Self {
        let summary = ForgeReviewSummary {
            thread_id: target.thread_id.clone(), cwd: target.cwd.clone(),
            change_request_iid: target.change_request_iid,
            approvals_required: None, approvals_left: None, approved_by_count: 0,
            changes_requested_by_count: 0, discussions_total: 0, unresolved_discussions: 0,
            approvals_available: false, discussions_available: false,
            observed_at_unix_ms: now_unix_ms(), error: Some(error),
        };
        Self { target, summary }
    }
}

'''
replace('src/forge_types.rs',"pub type ForgeFuture<'a, T>",result_type+"pub type ForgeFuture<'a, T>")
replace('src/app.rs','''        Action::ForgeReviewLoaded(review) => {
            if let Some(observation)''','''        Action::ForgeReviewLoaded(result) => {
            let crate::forge::ForgeReviewResult { target, summary: review } = *result;
            if state.thread_by_id(&target.thread_id).is_none_or(|thread| thread.metadata.cwd != target.cwd)
                || target.thread_id != review.thread_id || target.cwd != review.cwd
                || target.change_request_iid != review.change_request_iid
            {
                return vec![];
            }
            if let Some(observation)''')
replace('src/app.rs','''                && observation.cwd == review.cwd
                && observation
                    .change_requests''','''                && observation.cwd == review.cwd
                && observation.identity.as_ref().is_some_and(|identity| {
                    identity.provider == target.provider && identity.host == target.host
                        && identity.project_id == target.project_id
                        && identity.path_with_namespace == target.project_path
                })
                && observation
                    .change_requests''')
replace('src/app.rs','''fn propagate_forge_observation(state: &mut AppState, observation: ForgeObservation) {
    let source_cwd''','''fn propagate_forge_observation(state: &mut AppState, observation: ForgeObservation) {
    if state.thread_by_id(&observation.thread_id).is_none_or(|thread| thread.metadata.cwd != observation.cwd) {
        return;
    }
    let source_cwd''')
replace('src/app.rs','''            .get(&thread_id.0)
            .and_then(|existing| existing.review.clone())''','''            .get(&thread_id.0)
            .filter(|existing| existing.identity.is_some() && existing.identity == observation.identity)
            .and_then(|existing| existing.review.clone())''')
# Replace the verbose queue-failure fallback with the same target-bound result.
p=Path('src/main.rs'); text=p.read_text()
start=text.index('Action::ForgeReviewLoaded(forge::ForgeReviewSummary {')
end=text.index('\n                        }),',start)+len('\n                        }),')
text=text[:start]+'''Action::ForgeReviewLoaded(Box::new(forge::ForgeReviewResult::unavailable(
                            fallback, error.to_string(),
                        ))),'''+text[end:]
p.write_text(text)
replace('tests/forge_result_identity.rs','''    // Adapt this call when the result type begins retaining the request target.
    let _ = target;
    assert!(reduce(state, Action::ForgeReviewLoaded(summary)).is_empty());''','''    assert!(reduce(state, Action::ForgeReviewLoaded(Box::new(
        codex_tui::forge::ForgeReviewResult { target, summary },
    ))).is_empty());''')
# Before-stage already registers the production Forge actor tests.
p=Path('src/git.rs'); p.write_text(p.read_text()+'\n#[cfg(test)]\nmod read_order_tests;\n')
p=Path('release/v1.4-plan.json'); plan=json.loads(p.read_text())
plan['moduleRatchet'].update({'src/latest_read.rs':260,'src/forge/read_order_tests.rs':180,'src/git/read_order_tests.rs':160})
p.write_text(json.dumps(plan,indent=2)+'\n')
