use crate::planning::{SavedView, SavedViewLayout};

pub const SAVED_VIEW_VISIBLE_FIELDS: &[&str] = &[
    "stage",
    "attention",
    "goal",
    "source",
    "workspace",
    "branch",
    "forge",
    "change-request",
    "relationships",
];

const SOURCE_SCOPES: &[&str] = &["all", "thread", "scratch", "forge"];
const GROUP_BY: &[Option<&str>] = &[None, Some("stage"), Some("workspace"), Some("source")];
const ORDER_BY: &[Option<&str>] = &[
    Some("priority"),
    Some("title"),
    Some("stage"),
    Some("workspace"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavedViewEditorField {
    Name,
    SourceScope,
    Filter,
    GroupBy,
    OrderBy,
    Layout,
    VisibleFields,
}

impl SavedViewEditorField {
    pub const ALL: [Self; 7] = [
        Self::Name,
        Self::SourceScope,
        Self::Filter,
        Self::GroupBy,
        Self::OrderBy,
        Self::Layout,
        Self::VisibleFields,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::SourceScope => "Source",
            Self::Filter => "Filter",
            Self::GroupBy => "Group",
            Self::OrderBy => "Order",
            Self::Layout => "Layout",
            Self::VisibleFields => "Visible fields",
        }
    }

    pub const fn text_editable(self) -> bool {
        matches!(self, Self::Name | Self::Filter | Self::VisibleFields)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedViewEditor {
    pub draft: SavedView,
    pub selected_field: usize,
    pub creating: bool,
}

impl SavedViewEditor {
    pub fn create_from(template: &SavedView) -> Self {
        let mut draft = template.clone();
        draft.id.clear();
        draft.name = format!("{} Copy", draft.name);
        Self {
            draft,
            selected_field: 0,
            creating: true,
        }
    }

    pub fn edit(view: &SavedView) -> Result<Self, String> {
        if is_builtin_saved_view(view) {
            return Err("built-in Saved Views are immutable; create a custom copy instead".into());
        }
        Ok(Self {
            draft: view.clone(),
            selected_field: 0,
            creating: false,
        })
    }

    pub fn field(&self) -> SavedViewEditorField {
        SavedViewEditorField::ALL
            [self.selected_field.min(SavedViewEditorField::ALL.len().saturating_sub(1))]
    }

    pub fn move_field(&mut self, delta: i32) {
        let len = SavedViewEditorField::ALL.len() as i32;
        self.selected_field = (self.selected_field as i32 + delta).rem_euclid(len) as usize;
    }

    pub fn begin_text_value(&self) -> Option<String> {
        match self.field() {
            SavedViewEditorField::Name => Some(self.draft.name.clone()),
            SavedViewEditorField::Filter => Some(self.draft.filter.clone()),
            SavedViewEditorField::VisibleFields => Some(self.draft.visible_fields.join(",")),
            _ => None,
        }
    }

    pub fn commit_text_value(&mut self, value: String) -> Result<(), String> {
        match self.field() {
            SavedViewEditorField::Name => {
                self.draft.name = value.trim().to_string();
            }
            SavedViewEditorField::Filter => {
                self.draft.filter = value.trim().to_string();
            }
            SavedViewEditorField::VisibleFields => {
                self.draft.visible_fields = parse_visible_fields(&value)?;
            }
            field => {
                return Err(format!("{} is not a text-editable Saved View field", field.label()));
            }
        }
        Ok(())
    }

    pub fn cycle_value(&mut self, delta: i32) {
        match self.field() {
            SavedViewEditorField::SourceScope => {
                let index = SOURCE_SCOPES
                    .iter()
                    .position(|value| *value == self.draft.source_scope)
                    .unwrap_or(0);
                self.draft.source_scope =
                    SOURCE_SCOPES[cycle_index(index, SOURCE_SCOPES.len(), delta)].into();
            }
            SavedViewEditorField::GroupBy => {
                let current = self.draft.group_by.as_deref();
                let index = GROUP_BY.iter().position(|value| *value == current).unwrap_or(0);
                self.draft.group_by =
                    GROUP_BY[cycle_index(index, GROUP_BY.len(), delta)].map(ToOwned::to_owned);
            }
            SavedViewEditorField::OrderBy => {
                let current = self.draft.order_by.as_deref();
                let index = ORDER_BY.iter().position(|value| *value == current).unwrap_or(0);
                self.draft.order_by =
                    ORDER_BY[cycle_index(index, ORDER_BY.len(), delta)].map(ToOwned::to_owned);
            }
            SavedViewEditorField::Layout => {
                const LAYOUTS: [SavedViewLayout; 3] = [
                    SavedViewLayout::List,
                    SavedViewLayout::Board,
                    SavedViewLayout::ReviewQueue,
                ];
                let index = LAYOUTS
                    .iter()
                    .position(|value| *value == self.draft.layout)
                    .unwrap_or(0);
                self.draft.layout = LAYOUTS[cycle_index(index, LAYOUTS.len(), delta)];
            }
            SavedViewEditorField::Name
            | SavedViewEditorField::Filter
            | SavedViewEditorField::VisibleFields => {}
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_saved_view(&self.draft)
    }
}

pub fn is_builtin_saved_view(view: &SavedView) -> bool {
    view.id.starts_with("builtin:")
}

pub fn validate_saved_view(view: &SavedView) -> Result<(), String> {
    if is_builtin_saved_view(view) {
        return Err("built-in Saved Views are immutable".into());
    }
    let name = view.name.trim();
    if name.is_empty() {
        return Err("Saved View name must not be empty".into());
    }
    if name.chars().count() > 80 {
        return Err("Saved View name must be 80 characters or fewer".into());
    }
    if !SOURCE_SCOPES.contains(&view.source_scope.as_str()) {
        return Err(format!("unsupported Saved View source scope: {}", view.source_scope));
    }
    validate_filter(&view.filter)?;
    if !GROUP_BY.contains(&view.group_by.as_deref()) {
        return Err(format!(
            "unsupported Saved View group field: {}",
            view.group_by.as_deref().unwrap_or("<none>")
        ));
    }
    if !ORDER_BY.contains(&view.order_by.as_deref()) {
        return Err(format!(
            "unsupported Saved View order field: {}",
            view.order_by.as_deref().unwrap_or("<none>")
        ));
    }
    if view.visible_fields.is_empty() {
        return Err("Saved View must retain at least one visible field".into());
    }
    for field in &view.visible_fields {
        if !SAVED_VIEW_VISIBLE_FIELDS.contains(&field.as_str()) {
            return Err(format!("unsupported Saved View visible field: {field}"));
        }
    }
    Ok(())
}

fn validate_filter(filter: &str) -> Result<(), String> {
    let Some(terms) = parse_filter_terms(filter) else {
        return Err("Saved View filter has an unterminated quote/escape or empty term".into());
    };
    for term in terms {
        let token = term.trim_start_matches('-');
        if token == "needs-you" || !token.contains(':') {
            continue;
        }
        let (field, value) = token
            .split_once(':')
            .ok_or_else(|| "invalid Saved View filter term".to_string())?;
        if value.is_empty() {
            return Err(format!("Saved View filter field {field:?} requires a value"));
        }
        let valid = match field {
            "status" => matches!(value, "needs-you" | "snoozed" | "active"),
            "stage" => matches!(
                value.to_ascii_lowercase().as_str(),
                "inbox" | "ready" | "working" | "review" | "done"
            ),
            "workspace" | "project" | "branch" | "tag" | "goal" | "link" | "related"
            | "worktree" => true,
            "forge" => matches!(value.to_ascii_lowercase().as_str(), "gitlab" | "github"),
            "mr" | "cr" => matches!(
                value,
                "open" | "opened" | "closed" | "merged" | "draft" | "none"
            ),
            "attention" => true,
            "source" => matches!(value, "scratch" | "thread" | "codex" | "forge"),
            "pinned" | "snoozed" => matches!(value, "true" | "false" | "yes" | "no" | "1" | "0"),
            _ => false,
        };
        if !valid {
            return Err(format!("unsupported Saved View filter term: {token}"));
        }
    }
    Ok(())
}

fn parse_filter_terms(filter: &str) -> Option<Vec<String>> {
    let mut terms = Vec::new();
    let mut chars = filter.chars().peekable();
    loop {
        while chars.peek().is_some_and(|ch| ch.is_whitespace()) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }

        let mut value = String::new();
        if chars.peek() == Some(&'-') {
            value.push('-');
            chars.next();
        }

        let mut quoted = false;
        let mut escaped = false;
        for ch in chars.by_ref() {
            if escaped {
                value.push(ch);
                escaped = false;
                continue;
            }
            if quoted && ch == '\\' {
                escaped = true;
                value.push(ch);
                continue;
            }
            if ch == '"' {
                quoted = !quoted;
                value.push(ch);
                continue;
            }
            if !quoted && ch.is_whitespace() {
                break;
            }
            value.push(ch);
        }

        if quoted || escaped || value.trim_matches('-').is_empty() {
            return None;
        }
        terms.push(value.to_ascii_lowercase().replace('"', ""));
    }
    Some(terms)
}

fn parse_visible_fields(value: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    for field in value.split(',').map(str::trim).filter(|value| !value.is_empty()) {
        let normalized = field.to_ascii_lowercase();
        if !SAVED_VIEW_VISIBLE_FIELDS.contains(&normalized.as_str()) {
            return Err(format!("unsupported Saved View visible field: {field}"));
        }
        if !fields.contains(&normalized) {
            fields.push(normalized);
        }
    }
    if fields.is_empty() {
        return Err("visible fields must contain at least one field".into());
    }
    Ok(fields)
}

fn cycle_index(current: usize, len: usize, delta: i32) -> usize {
    (current as i32 + delta).rem_euclid(len as i32) as usize
}

#[cfg(test)]
mod tests;
