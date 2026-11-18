//! The native shell's in-memory layout: panel sizes, floating window bounds and
//! right-panel tab placement.
//!
//! [`LayoutState`] is the runtime counterpart of
//! `modv-app::ui_layout::LayoutSettings`. The shell mutates it while drawing
//! (panel widths/heights captured from the real rects, window bounds captured on
//! move/resize) and `NativeApp` diffs it against the saved document to persist
//! changes (debounced, best-effort).
//!
//! The placement model is deliberately an exhaustive per-tab value rather than
//! a list of windows: every tab has exactly one [`TabPlacement`], so a tab can
//! never be rendered twice. Transition rules are pure and unit-tested here.

use std::collections::BTreeMap;

use crate::state::SidePanelTab;

/// One floating window's outer rectangle in logical points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl WindowBounds {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Build bounds from an egui rect (the window's outer rect).
    pub fn from_rect(rect: egui::Rect) -> Self {
        Self {
            x: rect.min.x,
            y: rect.min.y,
            width: rect.width(),
            height: rect.height(),
        }
    }

    /// Whether the geometry is finite and positively sized.
    pub fn is_usable(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
    }

    pub fn pos(&self) -> egui::Pos2 {
        egui::pos2(self.x, self.y)
    }

    pub fn size(&self) -> egui::Vec2 {
        egui::vec2(self.width, self.height)
    }

    /// The outer rectangle these bounds describe.
    pub fn as_rect(&self) -> egui::Rect {
        egui::Rect::from_min_size(self.pos(), self.size())
    }
}

/// Where a right-panel tab renders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabPlacement {
    /// The right-hand tab strip (the default).
    #[default]
    Tabs,
    /// The separate dock column between the centre and the right panel.
    Docked,
    /// An in-app floating egui window.
    Floating,
}

impl TabPlacement {
    /// The stable key written to `native-layout.json`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Tabs => "tabs",
            Self::Docked => "docked",
            Self::Floating => "floating",
        }
    }

    /// Parse a persisted key; unknown values fall back to the tab strip.
    pub fn from_key(value: &str) -> Self {
        match value {
            "docked" => Self::Docked,
            "floating" => Self::Floating,
            _ => Self::Tabs,
        }
    }
}

/// The persisted/current placement of every right-panel tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabLayout {
    placements: [TabPlacement; SidePanelTab::COUNT],
}

impl Default for TabLayout {
    fn default() -> Self {
        Self {
            placements: [TabPlacement::Tabs; SidePanelTab::COUNT],
        }
    }
}

impl TabLayout {
    pub fn placement(&self, tab: SidePanelTab) -> TabPlacement {
        self.placements[tab.index()]
    }

    /// Set a placement without touching the active tab.
    ///
    /// Use this when restoring a document; user gestures should call
    /// [`TabLayout::place`] so the strip selection follows.
    pub fn set_placement(&mut self, tab: SidePanelTab, placement: TabPlacement) {
        self.placements[tab.index()] = placement;
    }

    /// The pure detach/dock/float transition.
    pub fn place(&mut self, tab: SidePanelTab, placement: TabPlacement, active: &mut SidePanelTab) {
        let was_in_strip = self.placement(tab) == TabPlacement::Tabs;
        self.set_placement(tab, placement);
        if was_in_strip
            && placement != TabPlacement::Tabs
            && *active == tab
            && let Some(next) = self.next_strip_tab(tab)
        {
            *active = next;
        }
    }

    /// Tabs currently rendered in the right-hand strip.
    pub fn strip_tabs(&self) -> impl Iterator<Item = SidePanelTab> + '_ {
        SidePanelTab::ALL
            .into_iter()
            .filter(|tab| self.placement(*tab) == TabPlacement::Tabs)
    }

    /// Tabs currently stacked in the dock column.
    pub fn docked_tabs(&self) -> Vec<SidePanelTab> {
        SidePanelTab::ALL
            .into_iter()
            .filter(|tab| self.placement(*tab) == TabPlacement::Docked)
            .collect()
    }

    /// Tabs currently shown as floating windows.
    pub fn floating_tabs(&self) -> Vec<SidePanelTab> {
        SidePanelTab::ALL
            .into_iter()
            .filter(|tab| self.placement(*tab) == TabPlacement::Floating)
            .collect()
    }

    pub fn has_strip_tabs(&self) -> bool {
        self.strip_tabs().next().is_some()
    }

    /// The next tab after `tab` that is still in the strip (wrapping).
    pub fn next_strip_tab(&self, tab: SidePanelTab) -> Option<SidePanelTab> {
        (1..SidePanelTab::COUNT)
            .map(|offset| SidePanelTab::ALL[(tab.index() + offset) % SidePanelTab::COUNT])
            .find(|candidate| self.placement(*candidate) == TabPlacement::Tabs)
    }

    /// Keep `active` pointing at a tab that is actually in the strip.
    ///
    /// Called once per frame before drawing: a restored document may place the
    /// default active tab in the dock or a floating window.
    pub fn ensure_strip_active(&self, active: &mut SidePanelTab) {
        if self.placement(*active) == TabPlacement::Tabs {
            return;
        }
        if let Some(first) = self.strip_tabs().next() {
            *active = first;
        }
    }
}

/// The full in-memory layout the shell reads and mutates while drawing.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutState {
    /// Groups side-panel width in logical points.
    pub groups_width: f32,
    /// Right tab-strip width in logical points.
    pub inspector_width: f32,
    /// Live Output section height; `0` means the automatic first-run split.
    pub live_height: f32,
    /// Gallery section height; `0` means the automatic first-run split.
    pub gallery_height: f32,
    /// Dock column width in logical points.
    pub dock_width: f32,
    /// Per-tab placement.
    pub tabs: TabLayout,
    /// Saved floating-window bounds keyed by the stable window id.
    pub windows: BTreeMap<String, WindowBounds>,
}

/// Default dock column width.
pub const DEFAULT_DOCK_WIDTH: f32 = 320.0;
/// Minimum dock column width.
pub const MIN_DOCK_WIDTH: f32 = 250.0;

/// Stable window ids used as `LayoutState::windows` / `LayoutSettings::windows`
/// keys.
pub const WINDOW_PALETTE: &str = "palette";
pub const WINDOW_TEMPO: &str = "tempo";
pub const WINDOW_SHORTCUTS: &str = "shortcuts";
pub const WINDOW_ABOUT: &str = "about";
pub const WINDOW_SCENE_TIMING: &str = "scene_timing";
pub const WINDOW_PERFORMANCE_ROW_NAMES: &str = "performance_row_names";
pub const WINDOW_TWEEN_CURVE: &str = "tween_curve";
pub const WINDOW_DRAW_CURVE: &str = "draw_curve";
pub const WINDOW_INPUT_LINK: &str = "input_link";

impl Default for LayoutState {
    fn default() -> Self {
        Self {
            groups_width: 270.0,
            inspector_width: 340.0,
            live_height: 0.0,
            gallery_height: 0.0,
            dock_width: DEFAULT_DOCK_WIDTH,
            tabs: TabLayout::default(),
            windows: BTreeMap::new(),
        }
    }
}

impl LayoutState {
    /// The saved bounds for a floating window, if any.
    pub fn window(&self, window: &str) -> Option<WindowBounds> {
        self.windows.get(window).copied()
    }

    /// Apply saved outer bounds to a floating window on its first show.
    ///
    /// `egui::Window::show` returns the area's outer rect, which is what the
    /// shell records afterwards (title bar included).
    pub fn apply_window<'a>(&self, window: egui::Window<'a>, id: &str) -> egui::Window<'a> {
        if let Some(bounds) = self.window(id) {
            window.default_pos(bounds.pos()).default_size(bounds.size())
        } else {
            window
        }
    }

    /// Record a window's outer rect after it was shown.
    ///
    /// Unusable rects are ignored. The caller decides when to persist; this
    /// only updates memory, and a stable rect does not allocate.
    pub fn record_window_rect(&mut self, window: &str, rect: egui::Rect) {
        let bounds = WindowBounds::from_rect(rect);
        if !bounds.is_usable() || self.window(window) == Some(bounds) {
            return;
        }
        self.windows.insert(window.to_string(), bounds);
    }

    /// The stable id of a tab's floating window (`tab:<key>`).
    pub fn tab_window_id(tab: SidePanelTab) -> String {
        format!("tab:{}", tab.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, height))
    }

    #[test]
    fn a_fresh_layout_puts_every_tab_in_the_strip() {
        let layout = TabLayout::default();
        assert_eq!(layout.strip_tabs().count(), SidePanelTab::COUNT);
        assert!(layout.docked_tabs().is_empty());
        assert!(layout.floating_tabs().is_empty());
        assert!(layout.has_strip_tabs());
        for tab in SidePanelTab::ALL {
            assert_eq!(layout.placement(tab), TabPlacement::Tabs);
        }
    }

    #[test]
    fn placements_are_exclusive_and_a_tab_has_one_instance() {
        let mut layout = TabLayout::default();

        layout.set_placement(SidePanelTab::Inspector, TabPlacement::Docked);
        layout.set_placement(SidePanelTab::Inspector, TabPlacement::Floating);
        assert_eq!(
            layout.placement(SidePanelTab::Inspector),
            TabPlacement::Floating,
            "a tab can only be in one place"
        );
        assert!(layout.docked_tabs().is_empty());
        assert_eq!(layout.floating_tabs(), vec![SidePanelTab::Inspector]);
        assert!(
            !layout
                .strip_tabs()
                .any(|tab| tab == SidePanelTab::Inspector)
        );

        layout.set_placement(SidePanelTab::Inspector, TabPlacement::Tabs);
        assert_eq!(
            layout.strip_tabs().collect::<Vec<_>>(),
            SidePanelTab::ALL.to_vec()
        );
    }

    #[test]
    fn detaching_the_active_tab_selects_the_next_strip_tab() {
        let mut layout = TabLayout::default();
        let mut active = SidePanelTab::Inspector;

        layout.place(SidePanelTab::Inspector, TabPlacement::Floating, &mut active);
        assert_eq!(active, SidePanelTab::Media);
        assert!(
            !layout
                .strip_tabs()
                .any(|tab| tab == SidePanelTab::Inspector)
        );

        // Wrapping from the end of the strip reaches the first remaining tab.
        let mut layout = TabLayout::default();
        let mut active = SidePanelTab::Osc;
        layout.place(SidePanelTab::Osc, TabPlacement::Docked, &mut active);
        assert_eq!(active, SidePanelTab::Inspector);
    }

    #[test]
    fn docking_a_strip_tab_does_not_change_another_selection() {
        let mut layout = TabLayout::default();
        let mut active = SidePanelTab::Audio;

        layout.place(SidePanelTab::Inspector, TabPlacement::Docked, &mut active);
        assert_eq!(active, SidePanelTab::Audio);

        layout.place(SidePanelTab::Inspector, TabPlacement::Tabs, &mut active);
        assert_eq!(active, SidePanelTab::Audio);
        assert_eq!(
            layout.placement(SidePanelTab::Inspector),
            TabPlacement::Tabs
        );
    }

    #[test]
    fn the_active_tab_falls_back_when_every_tab_has_left_the_strip() {
        let mut layout = TabLayout::default();
        let mut active = SidePanelTab::Inspector;

        for tab in SidePanelTab::ALL {
            layout.place(tab, TabPlacement::Docked, &mut active);
        }

        assert!(!layout.has_strip_tabs());
        assert_eq!(
            active,
            SidePanelTab::Osc,
            "with no strip tab left there is nothing to fall back to; the strip hides"
        );
        assert!(layout.next_strip_tab(SidePanelTab::Inspector).is_none());
    }

    #[test]
    fn ensure_strip_active_restores_a_usable_selection() {
        let mut layout = TabLayout::default();
        layout.set_placement(SidePanelTab::Inspector, TabPlacement::Docked);

        let mut active = SidePanelTab::Inspector;
        layout.ensure_strip_active(&mut active);
        assert_eq!(active, SidePanelTab::Media);

        let mut active = SidePanelTab::Audio;
        layout.ensure_strip_active(&mut active);
        assert_eq!(active, SidePanelTab::Audio, "a strip tab is left alone");
    }

    #[test]
    fn dock_float_and_back_to_tabs_round_trip() {
        let mut layout = TabLayout::default();
        let mut active = SidePanelTab::Inspector;

        layout.place(SidePanelTab::Inspector, TabPlacement::Docked, &mut active);
        assert_eq!(layout.docked_tabs(), vec![SidePanelTab::Inspector]);

        layout.place(SidePanelTab::Inspector, TabPlacement::Floating, &mut active);
        assert!(layout.docked_tabs().is_empty());
        assert_eq!(layout.floating_tabs(), vec![SidePanelTab::Inspector]);

        layout.place(SidePanelTab::Inspector, TabPlacement::Tabs, &mut active);
        assert!(layout.floating_tabs().is_empty());
        assert!(
            layout
                .strip_tabs()
                .any(|tab| tab == SidePanelTab::Inspector)
        );
    }

    #[test]
    fn window_bounds_round_trip_and_reject_unusable_rects() {
        let mut layout = LayoutState::default();
        assert_eq!(layout.window("tempo"), None);

        layout.record_window_rect("tempo", rect(10.0, 20.0, 320.0, 200.0));
        assert_eq!(
            layout.window("tempo"),
            Some(WindowBounds::new(10.0, 20.0, 320.0, 200.0))
        );

        // A stable rect is a no-op; a broken one keeps the last good geometry.
        let before = layout.window("tempo");
        layout.record_window_rect("tempo", rect(10.0, 20.0, 320.0, 200.0));
        assert_eq!(layout.window("tempo"), before);
        layout.record_window_rect("tempo", rect(f32::NAN, 0.0, 320.0, 200.0));
        assert_eq!(layout.window("tempo"), before);

        layout.record_window_rect("tempo", rect(30.0, 40.0, 360.0, 240.0));
        assert_eq!(
            layout.window("tempo"),
            Some(WindowBounds::new(30.0, 40.0, 360.0, 240.0))
        );

        assert_eq!(LayoutState::tab_window_id(SidePanelTab::Audio), "tab:audio");
    }
}
