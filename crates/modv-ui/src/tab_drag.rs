//! Right-panel tab drag payload and pure drop resolution.
//!
//! A strip tab and a floating window's grip both carry a [`TabDrag`] while the
//! pointer drags them. The tab strip, the dock column and the central area read
//! that payload to advertise themselves as drop targets and, on release, move
//! the tab to their [`TabPlacement`]. Placements stay exclusive: the payload
//! names one tab and a drop writes exactly one placement.
//!
//! The payload is transient pointer state, so it lives in `modv-ui` rather than
//! in the engine read model or the persisted layout document.

use crate::layout::{TabPlacement, WindowBounds};
use crate::state::SidePanelTab;

/// The right-panel tab a drag is carrying.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabDrag {
    pub tab: SidePanelTab,
}

impl TabDrag {
    pub fn new(tab: SidePanelTab) -> Self {
        Self { tab }
    }
}

/// Where a released tab drag should land.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabDropTarget {
    /// Back into the right-hand tab strip.
    Strip,
    /// The separate dock column.
    Dock,
    /// A floating window at this release position.
    Float(egui::Pos2),
}

impl TabDropTarget {
    /// The exclusive placement this target writes.
    pub fn placement(self) -> TabPlacement {
        match self {
            Self::Strip => TabPlacement::Tabs,
            Self::Dock => TabPlacement::Docked,
            Self::Float(_) => TabPlacement::Floating,
        }
    }
}

/// The default size of a tab's floating window, matching the shell's
/// `.default_size([360.0, 460.0])`.
pub const FLOATING_TAB_SIZE: egui::Vec2 = egui::vec2(360.0, 460.0);

/// Clamp a floating window's outer bounds so a drop at `pointer` is fully
/// on-screen.
///
/// The window's top-left starts at the release position; near an edge it is
/// pushed back inside `screen`. A screen smaller than the window shrinks the
/// window instead of opening it past the edge.
pub fn floating_bounds_for_drop(pointer: egui::Pos2, screen: egui::Rect) -> WindowBounds {
    let screen = if screen.is_positive() {
        screen
    } else {
        egui::Rect::from_min_size(egui::Pos2::ZERO, FLOATING_TAB_SIZE)
    };
    let width = FLOATING_TAB_SIZE.x.min(screen.width()).max(1.0);
    let height = FLOATING_TAB_SIZE.y.min(screen.height()).max(1.0);
    let x = pointer
        .x
        .clamp(screen.left(), (screen.right() - width).max(screen.left()));
    let y = pointer
        .y
        .clamp(screen.top(), (screen.bottom() - height).max(screen.top()));
    WindowBounds::new(x, y, width, height)
}
