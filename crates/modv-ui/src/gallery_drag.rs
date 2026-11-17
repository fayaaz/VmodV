//! The Gallery → Group drag payload.
//!
//! A Gallery tile carries a [`GalleryDrag`] while the pointer drags it. Every
//! Group block reads that payload to highlight itself as a drop target and,
//! on release, queues the same `ui/addModuleToGroup` action as the tile's
//! double-click and context menu. The payload is transient pointer state, so
//! it lives in `modv-ui` rather than in the engine read model.

use crate::state::UiAction;

/// The registered module a Gallery tile is dragging.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GalleryDrag {
    /// The module registry name, e.g. `Bands`.
    pub module_name: String,
}

impl GalleryDrag {
    pub fn new(module_name: impl Into<String>) -> Self {
        Self {
            module_name: module_name.into(),
        }
    }

    /// The action queued when this payload is released over `group_id`.
    ///
    /// It is the same action as the double-click/context-menu path, so the
    /// engine's `ui/addModuleToGroup` handler decides what a duplicate means.
    pub fn drop_action(&self, group_id: &str) -> UiAction {
        UiAction::add_module_to_group(&self.module_name, group_id)
    }
}

/// Resolve a released Gallery payload against the Group block under the
/// pointer. `None` cancels the drag: released over empty space, or no payload.
pub fn gallery_drop_action(
    payload: Option<&GalleryDrag>,
    hovered_group: Option<&str>,
) -> Option<UiAction> {
    let payload = payload?;
    let group_id = hovered_group?;
    Some(payload.drop_action(group_id))
}
