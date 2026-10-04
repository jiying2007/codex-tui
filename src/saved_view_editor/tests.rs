use super::*;

fn view() -> SavedView {
    SavedView {
        id: "view:1".into(),
        name: "Active review".into(),
        source_scope: "all".into(),
        filter: "stage:review -snoozed:true".into(),
        group_by: Some("workspace".into()),
        order_by: Some("priority".into()),
        layout: SavedViewLayout::List,
        visible_fields: vec!["stage".into(), "attention".into()],
    }
}

#[test]
fn validation_rejects_invalid_filter_and_builtin_mutation() {
    assert!(validate_saved_view(&view()).is_ok());
    let mut bad = view();
    bad.filter = "unknown:value".into();
    assert!(validate_saved_view(&bad).is_err());
    bad = view();
    bad.id = "builtin:all".into();
    assert!(validate_saved_view(&bad).is_err());
}

#[test]
fn editor_cycles_structured_fields_and_parses_visible_fields() {
    let mut editor = SavedViewEditor::edit(&view()).expect("editor");
    editor.selected_field = SavedViewEditorField::SourceScope as usize;
    editor.cycle_value(1);
    assert_eq!(editor.draft.source_scope, "thread");

    editor.selected_field = SavedViewEditorField::VisibleFields as usize;
    editor
        .commit_text_value("stage, workspace,relationships".into())
        .expect("visible fields");
    assert_eq!(
        editor.draft.visible_fields,
        vec!["stage", "workspace", "relationships"]
    );
}

#[test]
fn create_from_builtin_clears_immutable_identity() {
    let builtin = SavedView {
        id: "builtin:all".into(),
        name: "All Work".into(),
        source_scope: "all".into(),
        filter: String::new(),
        group_by: Some("stage".into()),
        order_by: Some("priority".into()),
        layout: SavedViewLayout::Board,
        visible_fields: vec!["stage".into()],
    };
    let editor = SavedViewEditor::create_from(&builtin);
    assert!(editor.draft.id.is_empty());
    assert!(editor.creating);
    assert!(editor.validate().is_ok());
}
