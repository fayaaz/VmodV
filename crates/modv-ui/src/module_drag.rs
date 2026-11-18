//! The Group module-row drag payload.
//!
//! A Module row in the Groups pane carries a [`ModuleDrag`] while the pointer
//! drags it. Every row (above/below its midpoint) and every Group block
//! (append after its last row) reads that payload to paint an insertion point
//! and, on release, queues `groups/MOVE_MODULE` with the destination index.
//! The payload is transient pointer state, so it lives in `modv-ui` rather
//! than in the engine read model.

use crate::state::UiAction;

/// An active Module a Groups-pane row is dragging.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleDrag {
    /// The active Module instance id.
    pub module_id: String,
    /// The Group the row was drawn in when the drag started.
    pub from_group_id: String,
    /// The row's index in that Group, so same-Group drops can tell an upward
    /// move from a downward one without re-reading the snapshot.
    pub from_index: usize,
}

impl ModuleDrag {
    pub fn new(
        module_id: impl Into<String>,
        from_group_id: impl Into<String>,
        from_index: usize,
    ) -> Self {
        Self {
            module_id: module_id.into(),
            from_group_id: from_group_id.into(),
            from_index,
        }
    }

    /// The action queued when this payload is released over `target_group` at
    /// `gap_index` (a slot in the pre-release row order, `0..=len`).
    ///
    /// Returns `None` for a same-Group drop that would leave the Module where
    /// it already is, so releasing on the dragged row itself is inert.
    pub fn drop_action(&self, target_group: &str, gap_index: usize) -> Option<UiAction> {
        let same_group = self.from_group_id == target_group;
        // Removing the Module first shifts every slot below it up by one, so a
        // downward same-Group drop lands one index earlier than its gap.
        let to_index = if same_group && gap_index > self.from_index {
            gap_index - 1
        } else {
            gap_index
        };
        if same_group && to_index == self.from_index {
            return None;
        }
        Some(UiAction::move_module(
            &self.module_id,
            &self.from_group_id,
            target_group,
            to_index,
        ))
    }
}

/// Resolve a released Module payload against the target Group and gap. `None`
/// cancels the drag: released over empty space, no payload, or a no-op.
pub fn module_drop_action(
    payload: Option<&ModuleDrag>,
    target_group: &str,
    gap_index: usize,
) -> Option<UiAction> {
    payload?.drop_action(target_group, gap_index)
}

/// The insertion slot a pointer at `pointer_y` targets among `rows`.
///
/// A row owns its upper half for an insertion above it and its lower half for
/// one below, so a pointer between two rows resolves to the gap they share.
/// `pointer_y` below the last midpoint appends (`rows.len()`).
pub fn insertion_index(rows: &[egui::Rect], pointer_y: f32) -> usize {
    rows.iter()
        .position(|row| pointer_y < row.center().y)
        .unwrap_or(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(top: f32, bottom: f32) -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(0.0, top), egui::pos2(100.0, bottom))
    }

    fn payload(from_index: usize) -> ModuleDrag {
        ModuleDrag::new("bands-1", "group-a", from_index)
    }

    #[test]
    fn same_group_drop_in_place_is_a_noop() {
        // Rows a (0), b (1), c (2). Dropping b on the gap above itself (1) or
        // the gap below itself (2) leaves the order unchanged.
        assert_eq!(payload(1).drop_action("group-a", 1), None);
        assert_eq!(payload(1).drop_action("group-a", 2), None);
    }

    #[test]
    fn same_group_move_down_lands_one_slot_earlier_than_the_gap() {
        // Dragging a below b (gap 2) removes a first, so the final index is 1.
        let action = payload(0)
            .drop_action("group-a", 2)
            .expect("moving down queues an action");
        assert_eq!(action.action, "groups/MOVE_MODULE");
        assert_eq!(
            action.payload,
            serde_json::json!({
                "moduleId": "bands-1",
                "fromGroupId": "group-a",
                "toGroupId": "group-a",
                "toIndex": 1,
            })
        );
    }

    #[test]
    fn same_group_move_up_uses_the_gap_index() {
        // Dragging c to the very top (gap 0) is unaffected by the removal.
        let action = payload(2)
            .drop_action("group-a", 0)
            .expect("moving up queues an action");
        assert_eq!(action.payload["toIndex"], serde_json::json!(0));
    }

    #[test]
    fn same_group_move_to_the_end_uses_the_last_index() {
        let action = payload(0)
            .drop_action("group-a", 3)
            .expect("appending within a Group queues an action");
        assert_eq!(action.payload["toIndex"], serde_json::json!(2));
    }

    #[test]
    fn cross_group_drop_keeps_the_gap_index() {
        let action = payload(0)
            .drop_action("group-b", 2)
            .expect("a cross-Group drop queues an action");
        assert_eq!(
            action.payload,
            serde_json::json!({
                "moduleId": "bands-1",
                "fromGroupId": "group-a",
                "toGroupId": "group-b",
                "toIndex": 2,
            })
        );
    }

    #[test]
    fn cross_group_append_past_the_last_row_queues_the_length() {
        let action = payload(1)
            .drop_action("group-b", 4)
            .expect("appending to another Group queues an action");
        assert_eq!(action.payload["toGroupId"], serde_json::json!("group-b"));
        assert_eq!(action.payload["toIndex"], serde_json::json!(4));
    }

    #[test]
    fn a_missing_payload_resolves_to_nothing() {
        assert_eq!(module_drop_action(None, "group-a", 0), None);
    }

    #[test]
    fn drop_action_resolves_the_payload_like_the_pure_resolver() {
        let drag = payload(0);
        assert_eq!(
            module_drop_action(Some(&drag), "group-b", 1),
            drag.drop_action("group-b", 1)
        );
    }

    #[test]
    fn insertion_index_splits_each_row_at_its_midpoint() {
        let rows = [rect(0.0, 20.0), rect(20.0, 40.0), rect(40.0, 60.0)];
        assert_eq!(insertion_index(&rows, 5.0), 0, "above the first midpoint");
        assert_eq!(insertion_index(&rows, 15.0), 1, "below the first midpoint");
        assert_eq!(insertion_index(&rows, 35.0), 2, "below the second midpoint");
        assert_eq!(insertion_index(&rows, 100.0), 3, "below the last row");
    }

    #[test]
    fn insertion_index_with_no_rows_appends_at_zero() {
        assert_eq!(insertion_index(&[], 10.0), 0);
    }
}
