use super::*;
use codex_tui::{app::{Effect, InputMode}, backend::{CodexBackend, FakeBackend},
    store::LocalStateV1};
use std::{sync::mpsc, time::{Duration, Instant}};

fn draft() -> (AppState, Vec<Effect>) {
    let mut app=AppState::new(FakeBackend::seeded().snapshot().threads);
    reduce(&mut app,Action::BeginScratch);
    reduce(&mut app,Action::InputText("durable draft".into()));
    let effects=reduce(&mut app,Action::CommitInput);
    (app,effects)
}
fn save(app:&mut AppState, worker:&mut StoreWorker, effect:Effect) {
    let ticket=app.local_edit_ticket_for(&effect);
    let Effect::CreateScratch{title,workspace}=effect else {panic!("scratch effect")};
    submit(app,worker,ticket,move |store|store.create_scratch(title,workspace));
}
fn settle(app:&mut AppState,worker:&mut StoreWorker) {
    let deadline=Instant::now()+Duration::from_secs(5);
    while app.pending_local_edit_ticket().is_some() {
        if let Some(event)=worker.try_event(){apply_event(app,event);}
        assert!(Instant::now()<deadline,"missing local save receipt");
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn real_worker_receipt_retires_only_the_committed_draft() {
    let root=tempfile::tempdir().unwrap();
    let mut worker=StoreWorker::at_for_test(root.path());
    let (mut app,effects)=draft();
    assert_eq!(app.input_buffer,"durable draft");
    save(&mut app,&mut worker,effects[0].clone());
    settle(&mut app,&mut worker);
    assert_eq!(app.input_mode,InputMode::Normal); assert!(app.input_buffer.is_empty());
    assert_eq!(app.planning_snapshot.scratch.len(),1);
    assert_eq!(worker.sqlite_clone().load_planning_snapshot().unwrap().scratch[0].title,"durable draft");
    worker.flush_operator_state_on_exit(&LocalStateV1::default()).unwrap();
}
#[test]
fn queue_pressure_preserves_draft_and_does_not_poison_the_store() {
    let root=tempfile::tempdir().unwrap(); let mut worker=StoreWorker::at_for_test(root.path());
    let (started,ready)=mpsc::sync_channel(0); let(release,wait)=mpsc::sync_channel(0);
    worker.submit(move |_|{started.send(()).unwrap();wait.recv().unwrap();StoreEvent::Notice(None)}).unwrap();
    ready.recv_timeout(Duration::from_secs(3)).unwrap();
    let mut accepted=0;
    while worker.submit(|_|StoreEvent::Notice(None)).is_ok(){accepted+=1;assert!(accepted<128);}
    assert_eq!(accepted,32);
    let(mut app,effects)=draft();save(&mut app,&mut worker,effects[0].clone());
    assert!(app.pending_local_edit_ticket().is_none()); assert_eq!(app.input_buffer,"durable draft");
    assert!(app.planning_store_error.is_none()); assert!(app.mutation_notice.as_ref().unwrap().contains("not accepted"));
    release.send(()).unwrap();
    let deadline=Instant::now()+Duration::from_secs(5);let mut seen=0;
    while seen<accepted+1{
        if let Some(event)=worker.try_event(){apply_event(&mut app,event);seen+=1;}
        assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(1));
    }
    let retry=reduce(&mut app,Action::CommitInput);save(&mut app,&mut worker,retry[0].clone());
    settle(&mut app,&mut worker);assert_eq!(app.planning_snapshot.scratch.len(),1);
    worker.flush_operator_state_on_exit(&LocalStateV1::default()).unwrap();
}
#[test]
fn real_storage_failure_keeps_draft_and_is_not_misreported_as_rollback() {
    let root=tempfile::tempdir().unwrap();let blocked=root.path().join("blocked");
    std::fs::write(&blocked,b"retain parent").unwrap();
    let mut worker=StoreWorker::at_for_test(&blocked);let(mut app,effects)=draft();
    save(&mut app,&mut worker,effects[0].clone());settle(&mut app,&mut worker);
    assert_eq!(app.input_buffer,"durable draft");assert_eq!(app.input_mode,InputMode::ScratchTitle);
    assert!(app.planning_store_error.is_some());assert!(app.mutation_notice.as_ref().unwrap().contains("not confirmed"));
    assert!(reduce(&mut app,Action::CommitInput).is_empty());
    assert!(worker.flush_operator_state_on_exit(&LocalStateV1::default()).is_err());
    assert_eq!(std::fs::read(blocked).unwrap(),b"retain parent");
}
#[test]
fn stopped_worker_unblocks_pending_draft_without_claiming_failure_before_commit() {
    let root=tempfile::tempdir().unwrap();let mut worker=StoreWorker::at_for_test(root.path());
    let(mut app,effects)=draft();let ticket=app.local_edit_ticket_for(&effects[0]);
    submit(&mut app,&mut worker,ticket,|_|panic!("intentional worker fault injection"));
    settle(&mut app,&mut worker);
    assert_eq!(app.input_buffer,"durable draft");assert!(app.planning_store_error.is_some());
    assert!(app.mutation_notice.as_ref().unwrap().contains("unconfirmed"));
    assert!(worker.try_event().is_none());
}
#[test]
fn buffered_editor_failure_is_retained_by_the_exit_barrier() {
    let root=tempfile::tempdir().unwrap();let mut worker=StoreWorker::at_for_test(root.path());
    worker.planning_with_ticket(|_|Err("accepted editor write failed".into()),None,Some(99)).unwrap();
    let error=worker.flush_operator_state_on_exit(&LocalStateV1::default()).unwrap_err();
    assert!(error.to_string().contains("accepted editor write failed"));
}
#[test]
fn old_receipts_do_not_clear_newer_submissions() {
    let(mut app,effects)=draft();let old=app.local_edit_ticket_for(&effects[0]).unwrap();
    app.finish_local_edit_write(old,Err("not accepted".into()));
    let effects=reduce(&mut app,Action::CommitInput);let new=app.local_edit_ticket_for(&effects[0]).unwrap();
    assert_ne!(old,new);app.finish_local_edit_write(old,Ok(()));
    assert_eq!(app.pending_local_edit_ticket(),Some(new));assert_eq!(app.input_buffer,"durable draft");
    app.finish_local_edit_write(new,Ok(()));assert!(app.input_buffer.is_empty());
}
#[test]
fn changed_then_undone_draft_is_not_cleared_by_old_success() {
    let(mut app,effects)=draft();let ticket=app.local_edit_ticket_for(&effects[0]).unwrap();
    reduce(&mut app,Action::InputChar('x'));reduce(&mut app,Action::Backspace);
    app.finish_local_edit_write(ticket,Ok(()));assert_eq!(app.input_buffer,"durable draft");
    assert_eq!(app.input_mode,InputMode::ScratchTitle);
}
#[test]
fn cancelled_and_identically_reopened_editor_survives_old_success() {
    let(mut app,effects)=draft();let ticket=app.local_edit_ticket_for(&effects[0]).unwrap();
    reduce(&mut app,Action::CancelInput);reduce(&mut app,Action::BeginScratch);
    reduce(&mut app,Action::InputText("durable draft".into()));
    assert!(reduce(&mut app,Action::CommitInput).is_empty());
    app.finish_local_edit_write(ticket,Ok(()));assert_eq!(app.input_buffer,"durable draft");
    assert!(app.pending_local_edit_ticket().is_none());assert!(!reduce(&mut app,Action::CommitInput).is_empty());
}
#[test]
fn pending_editor_can_be_closed_without_cancelling_the_committed_write() {
    let(mut app,effects)=draft();let ticket=app.local_edit_ticket_for(&effects[0]).unwrap();
    assert!(reduce(&mut app,Action::CommitInput).is_empty());reduce(&mut app,Action::CancelInput);
    assert_eq!(app.input_mode,InputMode::Normal);assert!(app.input_buffer.is_empty());
    app.finish_local_edit_write(ticket,Err("unconfirmed".into()));
    assert!(app.input_buffer.is_empty());assert_eq!(app.input_mode,InputMode::Normal);
}
#[test]
fn unrelated_planning_receipt_does_not_retire_local_editor() {
    let(mut app,effects)=draft();let ticket=app.local_edit_ticket_for(&effects[0]).unwrap();
    apply_event(&mut app,StoreEvent::Planning(Ok(PlanningSnapshot::default()),None,None));
    assert_eq!(app.pending_local_edit_ticket(),Some(ticket));assert_eq!(app.input_buffer,"durable draft");
}
