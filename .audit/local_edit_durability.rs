use codex_tui::{app::{Action, AppState, Effect, InputMode, reduce},
    backend::{CodexBackend, FakeBackend}, planning::{SourceRef, SourceKind, builtin_saved_views},
    saved_view_editor::{SavedViewEditor, SavedViewEditorField}};

fn app() -> AppState { AppState::new(FakeBackend::seeded().snapshot().threads) }
fn scratch(app: &mut AppState) {
    reduce(app, Action::BeginScratch);
    reduce(app, Action::InputText("retain scratch draft".into()));
}
fn note(app: &mut AppState, scratch: bool) {
    app.input_mode = InputMode::Note;
    app.input_buffer = "retain note draft".into();
    app.note_target = Some(if scratch { SourceRef {kind: SourceKind::ScratchWork, value:"scratch-1".into()} }
        else { SourceRef::codex_thread(&app.threads[0].id) });
}
fn saved(app: &mut AppState) {
    app.saved_view_editor = Some(SavedViewEditor::create_from(&builtin_saved_views()[0]));
}
#[test]
fn scratch_draft_survives_dispatch_until_a_receipt() {
    let mut app=app(); scratch(&mut app);
    let effects=reduce(&mut app,Action::CommitInput);
    assert!(matches!(&effects[..],[Effect::CreateScratch {title,..}] if title=="retain scratch draft"));
    assert_eq!(app.input_mode,InputMode::ScratchTitle);
    assert_eq!(app.input_buffer,"retain scratch draft");
}
#[test]
fn note_draft_and_owner_survive_dispatch() {
    let mut app=app(); note(&mut app,false); let owner=app.note_target.clone();
    assert!(matches!(&reduce(&mut app,Action::CommitInput)[..],[Effect::SaveSourceNote{..}]));
    assert_eq!(app.note_target,owner); assert_eq!(app.input_mode,InputMode::Note);
    assert_eq!(app.input_buffer,"retain note draft");
}
#[test]
fn scratch_note_draft_survives_dispatch() {
    let mut app=app(); note(&mut app,true); let owner=app.note_target.clone();
    assert!(matches!(&reduce(&mut app,Action::CommitInput)[..],[Effect::UpdateScratchNote{..}]));
    assert_eq!(app.note_target,owner); assert_eq!(app.input_buffer,"retain note draft");
}
#[test]
fn saved_view_editor_survives_dispatch() {
    let mut app=app(); saved(&mut app); let editor=app.saved_view_editor.clone();
    assert!(matches!(&reduce(&mut app,Action::SaveSavedViewEditor)[..],[Effect::SaveSavedView{..}]));
    assert_eq!(app.saved_view_editor,editor);
}
#[test]
fn invalid_visible_fields_keep_the_text_editor() {
    let mut app=app(); saved(&mut app);
    app.saved_view_editor.as_mut().unwrap().selected_field=SavedViewEditorField::ALL.iter().position(|f|*f==SavedViewEditorField::VisibleFields).unwrap();
    reduce(&mut app,Action::BeginSavedViewFieldEdit);
    app.input_buffer="field-that-is-not-supported".into();
    let draft=app.saved_view_editor.as_ref().unwrap().draft.clone();
    assert!(reduce(&mut app,Action::CommitInput).is_empty());
    assert!(app.saved_view_editor_error.is_some());
    assert_eq!(app.input_mode,InputMode::SavedViewField);
    assert_eq!(app.input_buffer,"field-that-is-not-supported");
    assert_eq!(app.saved_view_editor.as_ref().unwrap().draft,draft);
}
#[test]
fn unavailable_store_refuses_scratch_without_losing_draft() {
    let mut app=app(); scratch(&mut app);
    reduce(&mut app,Action::PlanningStoreDegraded(Some("read-only".into())));
    assert!(reduce(&mut app,Action::CommitInput).is_empty()); assert_eq!(app.input_buffer,"retain scratch draft");
}
#[test]
fn unavailable_store_refuses_note_without_losing_draft() {
    let mut app=app(); note(&mut app,false);
    reduce(&mut app,Action::PlanningStoreDegraded(Some("read-only".into())));
    assert!(reduce(&mut app,Action::CommitInput).is_empty()); assert_eq!(app.input_buffer,"retain note draft");
}
#[test]
fn unavailable_store_refuses_saved_view_without_losing_draft() {
    let mut app=app(); saved(&mut app); let editor=app.saved_view_editor.clone();
    reduce(&mut app,Action::PlanningStoreDegraded(Some("read-only".into())));
    assert!(reduce(&mut app,Action::SaveSavedViewEditor).is_empty()); assert_eq!(app.saved_view_editor,editor);
}
#[test]
fn empty_scratch_does_not_submit() {
    let mut app=app(); reduce(&mut app,Action::BeginScratch);
    assert!(reduce(&mut app,Action::CommitInput).is_empty()); assert_eq!(app.input_mode,InputMode::ScratchTitle);
}
#[test]
fn unsubmitted_draft_can_be_explicitly_cancelled() {
    let mut app=app(); scratch(&mut app);
    assert!(reduce(&mut app,Action::CancelInput).is_empty()); assert_eq!(app.input_mode,InputMode::Normal);
}
#[test]
fn valid_visible_fields_commit_only_to_the_editor() {
    let mut app=app(); saved(&mut app);
    app.saved_view_editor.as_mut().unwrap().selected_field=SavedViewEditorField::ALL.iter().position(|f|*f==SavedViewEditorField::VisibleFields).unwrap();
    reduce(&mut app,Action::BeginSavedViewFieldEdit); app.input_buffer="stage,attention".into();
    assert!(reduce(&mut app,Action::CommitInput).is_empty()); assert_eq!(app.input_mode,InputMode::Normal);
    assert_eq!(app.saved_view_editor.unwrap().draft.visible_fields,vec!["stage","attention"]);
}
