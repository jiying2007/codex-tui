use super::{
    SavedView, SavedViewLayout, WorkCardProjection, card_matches_filter, source_scope_matches,
};
use std::cmp::Ordering;

pub fn apply_saved_view<'a>(
    cards: &'a [WorkCardProjection],
    view: &SavedView,
) -> Vec<&'a WorkCardProjection> {
    let mut selected = cards
        .iter()
        .filter(|card| source_scope_matches(card, &view.source_scope))
        .filter(|card| card_matches_filter(card, &view.filter))
        .collect::<Vec<_>>();
    selected.sort_by(|left, right| {
        let group = if view.layout == SavedViewLayout::Board {
            Ordering::Equal
        } else {
            match view.group_by.as_deref() {
                Some("stage") => left.stage.cmp(&right.stage),
                Some("workspace") => left
                    .workspace
                    .as_deref()
                    .unwrap_or("No workspace")
                    .cmp(right.workspace.as_deref().unwrap_or("No workspace")),
                Some("source") => left.anchor.kind.cmp(&right.anchor.kind),
                _ => Ordering::Equal,
            }
        };
        group.then_with(|| match view.order_by.as_deref() {
            Some("title") => left.title.cmp(&right.title),
            Some("stage") => left
                .stage
                .cmp(&right.stage)
                .then_with(|| left.title.cmp(&right.title)),
            Some("workspace") => left
                .workspace
                .as_deref()
                .unwrap_or("")
                .cmp(right.workspace.as_deref().unwrap_or(""))
                .then_with(|| left.title.cmp(&right.title)),
            _ => left
                .overlay
                .priority
                .unwrap_or(i32::MAX)
                .cmp(&right.overlay.priority.unwrap_or(i32::MAX))
                .then_with(|| left.title.cmp(&right.title)),
        })
    });
    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{
        ScratchState, ScratchWork, SourceKind, WorkflowStage, reconcile_scratch_card,
        saved_view_group_key,
    };

    fn card(id: &str, workspace: Option<&str>, priority: i32) -> WorkCardProjection {
        reconcile_scratch_card(&ScratchWork {
            id: id.into(),
            title: id.into(),
            note: None,
            workspace: workspace.map(str::to_string),
            priority: Some(priority),
            state: ScratchState::Ready,
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        })
    }

    fn view() -> SavedView {
        SavedView {
            id: "test".into(),
            name: "test".into(),
            source_scope: "all".into(),
            filter: String::new(),
            group_by: Some("workspace".into()),
            order_by: Some("priority".into()),
            layout: SavedViewLayout::List,
            visible_fields: vec![],
        }
    }

    fn ids<'a>(cards: &'a [WorkCardProjection], view: &SavedView) -> Vec<&'a str> {
        apply_saved_view(cards, view)
            .into_iter()
            .map(|card| card.local_id.as_str())
            .collect()
    }

    #[test]
    fn workspace_groups_are_contiguous_with_selected_order_inside_each_group() {
        let cards = vec![
            card("z", Some("A"), 1),
            card("b", Some("B"), 2),
            card("a", Some("A"), 3),
        ];
        let original = cards.clone();
        let mut view = view();
        for layout in [SavedViewLayout::List, SavedViewLayout::ReviewQueue] {
            view.layout = layout;
            view.order_by = Some("priority".into());
            assert_eq!(ids(&cards, &view), ["z", "a", "b"]);
            view.order_by = Some("title".into());
            assert_eq!(ids(&cards, &view), ["a", "z", "b"]);
        }
        assert_eq!(cards, original);
    }

    #[test]
    fn ungrouped_and_board_views_keep_the_existing_global_order() {
        let cards = vec![
            card("a", Some("A"), 3),
            card("b", Some("B"), 1),
            card("c", Some("A"), 2),
        ];
        let mut view = view();
        view.group_by = None;
        assert_eq!(ids(&cards, &view), ["b", "c", "a"]);
        view.layout = SavedViewLayout::Board;
        view.group_by = Some("workspace".into());
        assert_eq!(ids(&cards, &view), ["b", "c", "a"]);
    }

    #[test]
    fn workspace_grouping_matches_missing_empty_and_literal_display_keys() {
        let cards = vec![
            card("missing", None, 1),
            card("empty", Some(""), 2),
            card("literal", Some("No workspace"), 3),
            card("path", Some("A"), 4),
        ];
        let selected = apply_saved_view(&cards, &view());
        assert_eq!(
            ids(&cards, &view()),
            ["empty", "path", "missing", "literal"]
        );
        assert_eq!(
            selected
                .iter()
                .map(|card| saved_view_group_key(card, Some("workspace")))
                .collect::<Vec<_>>(),
            ["", "A", "No workspace", "No workspace"]
        );
    }

    #[test]
    fn stage_and_source_groups_preserve_filters_and_order_within_groups() {
        let mut cards = vec![card("a", None, 1), card("b", None, 2), card("c", None, 3)];
        cards[0].stage = WorkflowStage::Review;
        cards[1].stage = WorkflowStage::Inbox;
        cards[2].stage = WorkflowStage::Review;
        cards[0].anchor.kind = SourceKind::CodexThread;
        cards[2].anchor.kind = SourceKind::CodexThread;
        let mut view = view();
        view.group_by = Some("stage".into());
        assert_eq!(ids(&cards, &view), ["b", "a", "c"]);
        view.group_by = Some("source".into());
        assert_eq!(ids(&cards, &view), ["b", "a", "c"]);
        view.source_scope = "thread".into();
        view.filter = "stage:review".into();
        assert_eq!(ids(&cards, &view), ["a", "c"]);
        view.filter = "stage:inbox".into();
        assert!(ids(&cards, &view).is_empty());
    }
}
