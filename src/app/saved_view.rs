use super::{AppState, ContextChoice, Effect, InputMode, SavedViewEditField, local_text};
use crate::planning::{SavedView, SavedViewLayout, validate_saved_view_filter};
use anyhow::{Result, anyhow};
use std::collections::BTreeSet;

const VISIBLE_FIELDS: &[&str] = &[
    "stage",
    "attention",
    "workspace",
    "source",
    "goal",
    "priority",
    "branch",
    "forge",
];

fn edit_field(choice: ContextChoice) -> Option<SavedViewEditField> {
    Some(match choice {
        ContextChoice::EditViewName => SavedViewEditField::Name,
        ContextChoice::EditViewSource => SavedViewEditField::Source,
        ContextChoice::EditViewFilter => SavedViewEditField::Filter,
        ContextChoice::EditViewLayout => SavedViewEditField::Layout,
        ContextChoice::EditViewGroup => SavedViewEditField::GroupBy,
        ContextChoice::EditViewOrder => SavedViewEditField::OrderBy,
        ContextChoice::EditViewFields => SavedViewEditField::VisibleFields,
        _ => return None,
    })
}

fn initial_value(view: &SavedView, field: SavedViewEditField) -> String {
    match field {
        SavedViewEditField::Name => view.name.clone(),
        SavedViewEditField::Source => view.source_scope.clone(),
        SavedViewEditField::Filter => view.filter.clone(),
        SavedViewEditField::Layout => match view.layout {
            SavedViewLayout::List => "list".into(),
            SavedViewLayout::Board => "board".into(),
            SavedViewLayout::ReviewQueue => "review-queue".into(),
        },
        SavedViewEditField::GroupBy => view.group_by.clone().unwrap_or_else(|| "none".into()),
        SavedViewEditField::OrderBy => view.order_by.clone().unwrap_or_else(|| "priority".into()),
        SavedViewEditField::VisibleFields => view.visible_fields.join(","),
    }
}

fn parse_visible_fields(value: &str) -> Result<Vec<String>> {
    let fields = value
        .split(',')
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    anyhow::ensure!(!fields.is_empty(), "visible fields must not be empty");
    anyhow::ensure!(
        fields.len() <= VISIBLE_FIELDS.len(),
        "too many visible fields"
    );
    let mut unique = BTreeSet::new();
    for field in &fields {
        anyhow::ensure!(
            VISIBLE_FIELDS.contains(&field.as_str()),
            "unsupported visible field: {field}"
        );
        anyhow::ensure!(
            unique.insert(field.clone()),
            "duplicate visible field: {field}"
        );
    }
    Ok(fields)
}

fn apply_saved_view_edit(
    view: &mut SavedView,
    field: SavedViewEditField,
    raw_value: &str,
) -> Result<()> {
    let value = raw_value.trim();
    match field {
        SavedViewEditField::Name => {
            anyhow::ensure!(!value.is_empty(), "SavedView name must not be empty");
            view.name = value.to_string();
        }
        SavedViewEditField::Source => {
            view.source_scope = match value.to_ascii_lowercase().as_str() {
                "all" => "all",
                "scratch" => "scratch",
                "thread" | "codex" => "thread",
                "forge" => "forge",
                _ => return Err(anyhow!("source must be all|scratch|thread|forge")),
            }
            .into();
        }
        SavedViewEditField::Filter => {
            anyhow::ensure!(
                validate_saved_view_filter(value),
                "SavedView filter syntax is invalid"
            );
            view.filter = value.to_string();
        }
        SavedViewEditField::Layout => {
            view.layout = match value.to_ascii_lowercase().as_str() {
                "list" => SavedViewLayout::List,
                "board" => SavedViewLayout::Board,
                "review" | "review-queue" => SavedViewLayout::ReviewQueue,
                _ => return Err(anyhow!("layout must be list|board|review-queue")),
            };
        }
        SavedViewEditField::GroupBy => {
            view.group_by = match value.to_ascii_lowercase().as_str() {
                "" | "none" => None,
                "stage" => Some("stage".into()),
                "workspace" => Some("workspace".into()),
                "source" => Some("source".into()),
                _ => return Err(anyhow!("group must be none|stage|workspace|source")),
            };
        }
        SavedViewEditField::OrderBy => {
            view.order_by = match value.to_ascii_lowercase().as_str() {
                "" | "none" | "priority" => None,
                "title" => Some("title".into()),
                "stage" => Some("stage".into()),
                "workspace" => Some("workspace".into()),
                _ => return Err(anyhow!("order must be priority|title|stage|workspace")),
            };
        }
        SavedViewEditField::VisibleFields => {
            view.visible_fields = parse_visible_fields(value)?;
        }
    }
    Ok(())
}

impl AppState {
    pub(super) fn begin_saved_view_edit(&mut self, choice: ContextChoice) -> bool {
        let Some(field) = edit_field(choice) else {
            return false;
        };
        let view = self.active_saved_view();
        if !view.id.starts_with("view:") {
            self.mutation_notice = Some(
                local_text(
                    self.language,
                    "built-in views are immutable; save a copy first",
                    "内置视图不可直接修改；请先另存为自定义视图",
                )
                .into(),
            );
            return true;
        }

        self.input_buffer = initial_value(&view, field);
        self.saved_view_template = Some(view);
        self.saved_view_edit_field = Some(field);
        self.input_mode = InputMode::SavedViewEdit;
        self.mutation_notice = None;
        true
    }

    pub(super) fn commit_saved_view_edit(&mut self) -> Vec<Effect> {
        let Some(field) = self.saved_view_edit_field else {
            self.input_mode = InputMode::Normal;
            self.input_buffer.clear();
            return vec![];
        };
        let Some(mut view) = self.saved_view_template.clone() else {
            self.saved_view_edit_field = None;
            self.input_mode = InputMode::Normal;
            self.input_buffer.clear();
            return vec![];
        };

        if let Err(error) = apply_saved_view_edit(&mut view, field, &self.input_buffer) {
            self.mutation_notice = Some(format!(
                "{}: {error}",
                local_text(self.language, "invalid SavedView value", "SavedView 值无效",)
            ));
            return vec![];
        }

        self.saved_view_template = None;
        self.saved_view_edit_field = None;
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
        self.mutation_notice = None;
        vec![Effect::SaveSavedView { view }]
    }

    pub(super) fn cancel_saved_view_edit(&mut self) {
        self.saved_view_template = None;
        self.saved_view_edit_field = None;
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> SavedView {
        SavedView {
            id: "view:1".into(),
            name: "Mine".into(),
            source_scope: "all".into(),
            filter: String::new(),
            group_by: Some("stage".into()),
            order_by: None,
            layout: SavedViewLayout::Board,
            visible_fields: vec!["stage".into(), "attention".into()],
        }
    }

    #[test]
    fn every_saved_view_field_is_validated_before_write() {
        let mut view = view();
        apply_saved_view_edit(&mut view, SavedViewEditField::Source, "forge").unwrap();
        apply_saved_view_edit(&mut view, SavedViewEditField::Layout, "list").unwrap();
        apply_saved_view_edit(&mut view, SavedViewEditField::GroupBy, "workspace").unwrap();
        apply_saved_view_edit(&mut view, SavedViewEditField::OrderBy, "title").unwrap();
        apply_saved_view_edit(
            &mut view,
            SavedViewEditField::VisibleFields,
            "workspace,priority,branch",
        )
        .unwrap();
        assert_eq!(view.source_scope, "forge");
        assert_eq!(view.layout, SavedViewLayout::List);
        assert_eq!(view.group_by.as_deref(), Some("workspace"));
        assert_eq!(view.order_by.as_deref(), Some("title"));
        assert_eq!(view.visible_fields, vec!["workspace", "priority", "branch"]);
    }

    #[test]
    fn invalid_editor_values_fail_closed() {
        let mut view = view();
        assert!(apply_saved_view_edit(&mut view, SavedViewEditField::Layout, "grid").is_err());
        assert!(
            apply_saved_view_edit(&mut view, SavedViewEditField::VisibleFields, "stage,secret")
                .is_err()
        );
        assert!(
            apply_saved_view_edit(&mut view, SavedViewEditField::Filter, "\"unterminated").is_err()
        );
    }
}
