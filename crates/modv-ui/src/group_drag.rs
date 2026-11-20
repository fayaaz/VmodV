//! The Group → Scene-cell drag payload.
//!
//! A Group header carries a [`GroupDrag`] while the pointer drags it. A Scene
//! grid cell reads that payload on release and queues the same
//! `scenes/setSceneCellGroup` action as the engine path. Occupied cells ask for
//! confirmation first; the payload is transient pointer state, so it lives in
//! `modv-ui` rather than in the engine read model.

use crate::state::UiAction;

/// The Group a Scene-grid cell is being pointed at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupDrag {
    pub group_id: String,
    pub group_name: String,
}

impl GroupDrag {
    pub fn new(group_id: impl Into<String>, group_name: impl Into<String>) -> Self {
        Self {
            group_id: group_id.into(),
            group_name: group_name.into(),
        }
    }

    /// The action queued when this payload is released over `scene_id` row `row`.
    pub fn drop_action(&self, scene_id: &str, row: usize) -> UiAction {
        UiAction::new(
            "scenes/setSceneCellGroup",
            serde_json::json!({
                "sceneId": scene_id,
                "rowIndex": row,
                "groupId": self.group_id,
            }),
        )
    }
}
