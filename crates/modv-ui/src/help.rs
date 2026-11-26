//! Short in-app documentation for each feature, shown in the Help frame below
//! the Groups panel.
//!
//! This is the Rust successor to the legacy Electron *Info View* panel: that
//! app registered a `{title, body}` pair on each UI element with the
//! `v-infoView` directive and displayed the pair for the element under the
//! pointer. Here the pair is a plain [`HelpTopic`] chosen by the last click,
//! so the text lives in one testable table instead of being scattered across
//! widget markup.

use crate::state::SidePanelTab;

/// A feature with documentation in the Help frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HelpTopic {
    /// The Help frame itself, shown before the user clicks anything.
    #[default]
    Welcome,
    Groups,
    Modules,
    Gallery,
    Scenes,
    LiveOutput,
    Inspector,
    Media,
    Library,
    Quality,
    Output,
    Audio,
    Info,
    Plugins,
    Midi,
    Ndi,
    Osc,
}

impl HelpTopic {
    /// Every topic, in a stable order (also handy for tests and introspection).
    pub const ALL: &'static [Self] = &[
        Self::Welcome,
        Self::Groups,
        Self::Modules,
        Self::Gallery,
        Self::Scenes,
        Self::LiveOutput,
        Self::Inspector,
        Self::Media,
        Self::Library,
        Self::Quality,
        Self::Output,
        Self::Audio,
        Self::Info,
        Self::Plugins,
        Self::Midi,
        Self::Ndi,
        Self::Osc,
    ];

    /// The heading shown above the topic's body.
    pub const fn title(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::Groups => "Groups",
            Self::Modules => "Modules",
            Self::Gallery => "Module Gallery",
            Self::Scenes => "Scenes",
            Self::LiveOutput => "Live Output",
            Self::Inspector => "Inspector",
            Self::Media => "Media",
            Self::Library => "Library",
            Self::Quality => "Quality",
            Self::Output => "Output",
            Self::Audio => "Audio",
            Self::Info => "Info",
            Self::Plugins => "Plugins",
            Self::Midi => "MIDI",
            Self::Ndi => "NDI",
            Self::Osc => "OSC",
        }
    }

    /// The explanatory text, ported from the legacy `iVBody` strings and
    /// updated where the Rust shell names things differently.
    pub const fn body(self) -> &'static str {
        match self {
            Self::Welcome => {
                "The Help panel shows a short description of the feature you last clicked. \
                Click a Group, Gallery tile, tab or Scene to read about it here."
            }
            Self::Groups => {
                "Groups contain Modules. Modules within Groups can be rearranged to change \
                the drawing order. Groups can also be rearranged by dragging their title bar."
            }
            Self::Modules => {
                "Modules are the visuals that render inside a Group. Drag a tile from the \
                Module Gallery onto a Group to add one, or double-click the tile to add it to the \
                focused Group. Select a Module to edit its properties in the Inspector, link its \
                controls to audio, MIDI or tweens, reorder it with its drag handle, or remove it with x."
            }
            Self::Gallery => {
                "The Module Gallery displays all available Modules. A module can be dragged \
                into a Group. Double clicking a Module will add it to the currently focused Group."
            }
            Self::Scenes => {
                "Scenes are full snapshots of the app: every Group, Module, Input Link, \
                Expression, Tween and MIDI device is saved using the same JSON format as a preset. \
                The grid shows Scenes as columns and Groups as lanes: click a column header to apply \
                a whole Scene, or a cell to swap just that Group. Performance mode runs through the \
                enabled Scenes, in order or randomly, switching on musical phrase changes detected \
                from the bass audio band and crossfading the Groups between Scenes. Each Group \
                independently waits a random time between the default min and max cell times before \
                it advances, so Groups change on their own schedule rather than all at once. \
                Ctrl-drag a grid cell onto another Scene to copy its Group exactly. A Scene JSON can \
                be opened like a preset, and all Scenes can be exported and imported as a playlist JSON."
            }
            Self::LiveOutput => {
                "The Live Output is the composited frame from all enabled Groups, \
                rendered in real time. It is the source used for screenshots and Scene previews."
            }
            Self::Inspector => {
                "The Inspector shows the properties of the selected Module. Select a \
                Module in a Group or a Gallery tile to edit its values here. Each control can be \
                linked to audio, MIDI, expressions or tweens with Input Links."
            }
            Self::Media => {
                "The Media panel shows the audio and video sources available to VmodV. \
                Use it to select capture devices, cameras and media files."
            }
            Self::Library => {
                "The Library browses the media tree of the active project profile. Pick \
                an image or video here to fill a texture property in the Inspector."
            }
            Self::Quality => {
                "Quality sets the internal render resolution. Lower values trade image \
                detail for frame rate."
            }
            Self::Output => {
                "Output configures the render target and presentation settings for the \
                live output."
            }
            Self::Audio => {
                "The Audio panel shows live analysis of the audio input: bands, tempo, \
                phrase and features. Select an input device in the Media panel, then link these \
                values to Module controls."
            }
            Self::Info => {
                "Info shows the current engine adapter, backend, frame size, output and \
                audio status, together with any engine errors from this session."
            }
            Self::Plugins => {
                "The Plugins panel lists all available JavaScript plugins. Plugins are \
                opt-in: enabling the master switch consents to running them."
            }
            Self::Midi => {
                "Configure your MIDI inputs here. Use the toggles to accept MIDI inputs \
                (CC/notes) and MIDI Clock per device."
            }
            Self::Ndi => {
                "Configure your NDI settings here. Click Discover Sources to search for NDI \
                sources available on your network. Click Enable to create an NDI receiver so VmodV \
                can consume the NDI source. Once a receiver has been created, click Remove Receiver \
                to stop VmodV consuming the NDI source."
            }
            Self::Osc => {
                "The OSC panel configures OSC listeners. Add a host and port to receive OSC \
                messages and link them to Module controls."
            }
        }
    }

    /// The documentation to show when a right-panel tab is clicked.
    pub const fn from_side_panel_tab(tab: SidePanelTab) -> Self {
        match tab {
            SidePanelTab::Inspector => Self::Inspector,
            SidePanelTab::Media => Self::Media,
            SidePanelTab::Library => Self::Library,
            SidePanelTab::Quality => Self::Quality,
            SidePanelTab::Output => Self::Output,
            SidePanelTab::Audio => Self::Audio,
            SidePanelTab::Info => Self::Info,
            SidePanelTab::Plugins => Self::Plugins,
            SidePanelTab::Midi => Self::Midi,
            SidePanelTab::Ndi => Self::Ndi,
            SidePanelTab::Osc => Self::Osc,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_topic_has_a_heading_and_some_documentation() {
        for topic in HelpTopic::ALL {
            assert!(!topic.title().is_empty(), "{topic:?} needs a title");
            assert!(!topic.body().is_empty(), "{topic:?} needs a body");
        }
    }

    #[test]
    fn help_text_calls_groups_groups() {
        // The legacy Info View said "layers" and "rows" for what VmodV now
        // consistently calls Groups. None of that old wording may leak into
        // the new documentation.
        for topic in HelpTopic::ALL {
            let lower = topic.body().to_ascii_lowercase();
            let words: Vec<&str> = lower
                .split(|c: char| !c.is_ascii_alphabetic())
                .filter(|word| !word.is_empty())
                .collect();
            for forbidden in ["layer", "layers", "row", "rows"] {
                assert!(
                    !words.contains(&forbidden),
                    "{topic:?} still says \"{forbidden}\": {}",
                    topic.body()
                );
            }
        }
    }

    #[test]
    fn every_side_panel_tab_maps_to_its_own_documentation() {
        for tab in SidePanelTab::ALL {
            let topic = HelpTopic::from_side_panel_tab(tab);
            assert_eq!(
                topic.title(),
                tab.title(),
                "the {tab:?} tab must show docs titled after itself"
            );
        }
    }
}
