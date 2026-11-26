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
    InputLinks,
    AudioLink,
    MidiLink,
    TweenLink,
    DrawLink,
    OscLink,
    StateLink,
    ExpressionLink,
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
        Self::InputLinks,
        Self::AudioLink,
        Self::MidiLink,
        Self::TweenLink,
        Self::DrawLink,
        Self::OscLink,
        Self::StateLink,
        Self::ExpressionLink,
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
            Self::InputLinks => "Input Links",
            Self::AudioLink => "Audio link",
            Self::MidiLink => "MIDI link",
            Self::TweenLink => "Tween link",
            Self::DrawLink => "Draw link",
            Self::OscLink => "OSC link",
            Self::StateLink => "State link",
            Self::ExpressionLink => "Expression",
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
            Self::InputLinks => {
                "Input Links automate a Module control. Select a Module in the Inspector, then \
                click + link on one of its controls to drive it from an audio feature, the \
                detected BPM, Draw, a Tween, MIDI or OSC. Use the editor to set the source, its \
                range and an optional expression, then click x to remove the link."
            }
            Self::AudioLink => {
                "Audio links drive a Module control from the analysed audio: a band such as \
                kick, bass, mid or treble, RMS, spectral features, or the detected BPM. Pick a \
                feature, then set its Min/Max and smoothing to shape how the value moves."
            }
            Self::MidiLink => {
                "MIDI links learn a CC or note from a device and use it to drive the control. Click \
                Learn, move the control on your MIDI device, then save the link; Forget releases it. \
                The toggles in the MIDI tab decide which devices and MIDI Clock are accepted."
            }
            Self::TweenLink => {
                "Tween links animate a control from one value to another over time. Edit the \
                keyframes and durations, then choose whether the cycle follows seconds or BPM; it \
                can loop or play once."
            }
            Self::DrawLink => {
                "Draw links drive a control from a freehand curve you sketch. Draw the shape across \
                the pad, set its duration, and the value follows the curve each cycle."
            }
            Self::OscLink => {
                "OSC links read a value written to a state path by an incoming OSC address. Click \
                Use OSC to fill the default path, or type the path that your OSC messages write to."
            }
            Self::StateLink => {
                "State links read a value straight from an engine state path, such as modules/active. \
                Use one when another Module or Scene writes the value you want to follow."
            }
            Self::ExpressionLink => {
                "Expressions shape the incoming value with JavaScript-style math. `value` is the \
                incoming number: `value * 3` triples it, `value / 2` halves it. The identity value \
                removes the assignment."
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
