//! Kittest coverage for the native Library tab.
//!
//! The Library browses the active profile's image/video/palette folders
//! recursively. These tests drive the real shell: the tab strip, folder
//! switching/back, the grid, the Rescan command, the click-to-assign flow and
//! a Library tile dropped onto a texture prop. The prop payload asserted here
//! is the same one the manual path editor writes.

use accesskit::Role;
use egui::{Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{LibraryKind, MediaAssetView, MediaLibraryView};
use modv_ui::state::SidePanelTab;
use modv_ui::{
    EngineSnapshot, GroupView, ModuleView, PropView, TabPlacement, USE_FROM_LIBRARY_LABEL,
    UiCommand, UiState,
};

fn asset(
    name: &str,
    kind: LibraryKind,
    folder: &str,
    size_bytes: u64,
    dimensions: Option<[u32; 2]>,
    duration_ms: Option<u64>,
) -> MediaAssetView {
    MediaAssetView {
        path: format!(
            "live/{}/{}{}",
            kind.folder(),
            if folder.is_empty() {
                String::new()
            } else {
                format!("{folder}/")
            },
            name
        ),
        absolute: Some(format!("/media/live/{}/{}", kind.folder(), name)),
        kind,
        name: name.to_string(),
        folder: folder.to_string(),
        size_bytes,
        dimensions,
        duration_ms,
        palette: (kind == LibraryKind::Palette).then(|| vec![[255, 0, 0, 255], [0, 255, 0, 255]]),
        thumbnail: None,
    }
}

fn library() -> MediaLibraryView {
    MediaLibraryView {
        profile: "live".to_string(),
        assets: vec![
            asset(
                "still.png",
                LibraryKind::Image,
                "",
                2048,
                Some([1920, 1080]),
                None,
            ),
            asset(
                "pack.png",
                LibraryKind::Image,
                "artist/pack",
                1024,
                Some([640, 480]),
                None,
            ),
            asset(
                "texture.png",
                LibraryKind::Image,
                "artist",
                4096,
                Some([800, 600]),
                None,
            ),
            asset(
                "clip.mp4",
                LibraryKind::Video,
                "",
                4_500_000,
                Some([1280, 720]),
                Some(12_400),
            ),
            asset("neon.json", LibraryKind::Palette, "", 120, None, None),
        ],
        truncated: false,
        error: None,
    }
}

fn module_snapshot(prop_type: &str) -> EngineSnapshot {
    EngineSnapshot {
        groups: vec![GroupView {
            id: "group-1".to_string(),
            name: "Effects".to_string(),
            enabled: true,
            hidden: false,
            alpha: 1.0,
            alpha_input_id: None,
            composite_operation: "normal".to_string(),
            composite_operation_input_id: None,
            inherit: true,
            clearing: true,
            pipeline: false,
            module_ids: vec!["module-1".to_string()],
        }],
        modules: vec![ModuleView {
            id: "module-1".to_string(),
            module_name: "Image Layer".to_string(),
            group_id: "group-1".to_string(),
            enabled: true,
            alpha: 1.0,
            alpha_input_id: None,
            composite_operation: "normal".to_string(),
            composite_operation_input_id: None,
            module_type: "isf".to_string(),
            props: vec![PropView {
                name: "photo".to_string(),
                prop_type: "texture".to_string(),
                value: serde_json::json!({ "type": prop_type, "options": { "path": "" } }),
                min: None,
                max: None,
                step: None,
                default: None,
                input_id: None,
                components: Vec::new(),
                tween: None,
                options: Vec::new(),
            }],
        }],
        library: library(),
        ..Default::default()
    }
}

fn build(snapshot: EngineSnapshot, state: UiState) -> Harness<'static, UiState> {
    let harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_step_dt(1.0 / 60.0)
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    modv_ui::theme::apply(&harness.ctx);
    harness
}

fn library_state() -> UiState {
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Library;
    state
}

#[test]
fn the_library_tab_is_in_the_strip_and_browses_images() {
    let mut harness = build(
        EngineSnapshot {
            library: library(),
            ..Default::default()
        },
        UiState::new(),
    );
    harness.run();

    harness.get_by_label("LIBRARY").click();
    harness.run();

    assert_eq!(harness.state().side_panel_tab, SidePanelTab::Library);
    harness.get_by_label("MEDIA LIBRARY");
    harness.get_by_label("Images");
    harness.get_by_label("Videos");
    harness.get_by_label("Palettes");
    harness.get_by_label("Rescan");
    harness.get_by_label("live / image");
    harness.get_by_label("still.png");
}

#[test]
fn folder_switching_filters_the_recursive_grid_and_up_walks_back() {
    let mut harness = build(
        EngineSnapshot {
            library: library(),
            ..Default::default()
        },
        library_state(),
    );
    harness.run();

    harness.get_by_label("still.png");
    harness.get_by_label("texture.png");
    harness.get_by_label("pack.png");

    harness.get_by_label("artist/").click();
    harness.run();
    assert_eq!(harness.state().library_folder, "artist");
    harness.get_by_label("live / image / artist");
    assert!(
        harness.query_by_label("still.png").is_none(),
        "the root file leaves the artist scope"
    );
    harness.get_by_label("texture.png");
    harness.get_by_label("pack.png");

    harness.get_by_label("pack/").click();
    harness.run();
    assert_eq!(harness.state().library_folder, "artist/pack");
    harness.get_by_label("live / image / artist / pack");
    harness.get_by_label("pack.png");
    assert!(harness.query_by_label("texture.png").is_none());

    harness.get_by_label("Up").click();
    harness.run();
    assert_eq!(harness.state().library_folder, "artist");

    harness.get_by_label("Videos").click();
    harness.run();
    assert_eq!(harness.state().library_kind, LibraryKind::Video);
    assert_eq!(
        harness.state().library_folder,
        "",
        "a folder switch starts the new kind at its root"
    );
    harness.get_by_label("live / video");
    harness.get_by_label("clip.mp4");
    assert!(harness.query_by_label("pack.png").is_none());

    harness.get_by_label("Palettes").click();
    harness.run();
    assert_eq!(harness.state().library_kind, LibraryKind::Palette);
    harness.get_by_label("neon.json");
}

#[test]
fn the_empty_and_error_states_are_explained() {
    let mut harness = build(
        EngineSnapshot {
            library: MediaLibraryView {
                profile: "live".to_string(),
                assets: Vec::new(),
                truncated: false,
                error: Some(
                    "image: could not read /media/live/image: permission denied".to_string(),
                ),
            },
            ..Default::default()
        },
        library_state(),
    );
    harness.run();

    harness.get_by_label("image: could not read /media/live/image: permission denied");
    harness.get_by_label("No images in the live profile yet.");
    harness.get_by_label("Copy files into live / image (subfolders are scanned too), then Rescan.");
    harness.get_by_label("0 assets");
}

#[test]
fn rescan_queues_the_library_refresh_command() {
    let mut harness = build(
        EngineSnapshot {
            library: library(),
            ..Default::default()
        },
        library_state(),
    );
    harness.run();

    harness.get_by_label("Rescan").click();
    harness.run();

    assert_eq!(
        harness.state_mut().take_commands(),
        vec![UiCommand::RescanLibrary]
    );
}

#[test]
fn use_from_library_arms_the_prop_and_a_click_assigns_it() {
    let mut state = library_state();
    state.side_panel_tab = SidePanelTab::Inspector;
    state.selected_group = Some("group-1".to_string());
    state.selected_module = Some("module-1".to_string());
    let mut harness = build(module_snapshot("image"), state);
    harness.run();

    harness.get_by_label(USE_FROM_LIBRARY_LABEL).click();
    harness.run();

    assert_eq!(
        harness.state().side_panel_tab,
        SidePanelTab::Library,
        "the Library opens on the matching folder"
    );
    assert_eq!(harness.state().library_kind, LibraryKind::Image);
    harness.get_by_label("Pick images for Image Layer / photo");

    harness.get_by_label("still.png").click();
    harness.run();

    assert_eq!(
        harness.state_mut().take_actions(),
        vec![modv_ui::texture_prop_action(
            "module-1",
            "photo",
            serde_json::json!({
                "type": "image",
                "options": { "path": "live/image/still.png" },
            })
        )],
        "the exact prop payload the manual path editor writes"
    );
    assert!(harness.state().library_target.is_none());
    assert!(
        harness
            .query_by_label("Pick images for Image Layer / photo")
            .is_none(),
        "the banner clears after the pick"
    );
}

#[test]
fn typing_a_path_by_hand_still_writes_the_prop() {
    let mut state = UiState::new();
    state.selected_group = Some("group-1".to_string());
    state.selected_module = Some("module-1".to_string());
    let mut harness = build(module_snapshot("image"), state);
    harness.run();

    // The Inspector's image path field is the only text input on the right.
    // Queue focus + text while the node borrow is alive, then run once.
    {
        let field = harness
            .get_all_by_role(Role::TextInput)
            .find(|node| node.rect().left() > 1000.0)
            .expect("image path field");
        field.focus();
        field.type_text("live/image/manual.png");
    }
    harness.run();

    assert_eq!(
        harness.state_mut().take_actions(),
        vec![modv_ui::texture_prop_action(
            "module-1",
            "photo",
            serde_json::json!({
                "type": "image",
                "options": { "path": "live/image/manual.png" },
            })
        )],
        "the manual path editor still writes the same prop value"
    );
}

#[test]
fn cancelling_a_pending_pick_keeps_the_prop_untouched() {
    let mut state = library_state();
    state.side_panel_tab = SidePanelTab::Inspector;
    state.selected_group = Some("group-1".to_string());
    state.selected_module = Some("module-1".to_string());
    let mut harness = build(module_snapshot("video"), state);
    harness.run();

    harness.get_by_label(USE_FROM_LIBRARY_LABEL).click();
    harness.run();
    assert_eq!(harness.state().library_kind, LibraryKind::Video);
    harness.get_by_label("Pick videos for Image Layer / photo");

    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(harness.state().library_target.is_none());
    harness.get_by_label("clip.mp4").click();
    harness.run();
    assert!(
        harness.state().actions.is_empty(),
        "a click without an armed prop only selects: {:?}",
        harness.state().actions
    );
    assert_eq!(
        harness.state().selected_library_asset.as_deref(),
        Some("live/video/clip.mp4")
    );
}

/// Drag from `from` to `to`, releasing with a raw pointer event (the pattern
/// `gallery_drag_drop.rs` uses).
fn drag(harness: &mut Harness<'static, UiState>, from: egui::Pos2, to: egui::Pos2) {
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();
}

#[test]
fn a_library_tile_carries_the_typed_asset_payload_while_dragging() {
    let mut harness = build(
        EngineSnapshot {
            library: library(),
            ..Default::default()
        },
        library_state(),
    );
    harness.run();

    let tile = harness.get_by_label("still.png").rect().center();
    harness.hover_at(tile);
    harness.run();
    harness.drag_at(tile);
    harness.run();
    harness.hover_at(tile + egui::vec2(0.0, 20.0));
    harness.run();

    let payload = egui::DragAndDrop::payload::<modv_ui::MediaLibraryDrag>(&harness.ctx)
        .expect("the tile is a drag source");
    assert_eq!(payload.path, "live/image/still.png");
    assert_eq!(payload.kind, LibraryKind::Image);
    assert_eq!(payload.name, "still.png");
}

#[test]
fn dragging_an_asset_onto_a_texture_prop_writes_the_asset_payload() {
    let mut state = UiState::new();
    state.selected_group = Some("group-1".to_string());
    state.selected_module = Some("module-1".to_string());
    // Dock the Library so it is visible next to the Inspector.
    state
        .layout
        .tabs
        .set_placement(SidePanelTab::Library, TabPlacement::Docked);
    let mut harness = build(module_snapshot("image"), state);
    harness.run();

    let tile = harness.get_by_label("still.png").rect().center();
    let control = harness.get_by_label("photo").rect().center();
    drag(&mut harness, tile, control);

    assert_eq!(
        harness.state_mut().take_actions(),
        vec![modv_ui::texture_prop_action(
            "module-1",
            "photo",
            serde_json::json!({
                "type": "image",
                "options": { "path": "live/image/still.png" },
            })
        )]
    );
}

#[test]
fn a_video_asset_cannot_be_dropped_on_an_image_prop() {
    let mut state = UiState::new();
    state.selected_group = Some("group-1".to_string());
    state.selected_module = Some("module-1".to_string());
    state
        .layout
        .tabs
        .set_placement(SidePanelTab::Library, TabPlacement::Docked);
    state.library_kind = LibraryKind::Video;
    let mut harness = build(module_snapshot("image"), state);
    harness.run();

    let tile = harness.get_by_label("clip.mp4").rect().center();
    let control = harness.get_by_label("photo").rect().center();
    drag(&mut harness, tile, control);

    assert!(
        harness.state().actions.is_empty(),
        "an image prop keeps its kind: {:?}",
        harness.state().actions
    );
}

#[test]
fn library_panel_snapshot() {
    let mut state = library_state();
    state.enter_library_folder("artist");
    let mut harness = build(
        EngineSnapshot {
            library: library(),
            ..Default::default()
        },
        state,
    );
    harness.run();
    harness.snapshot("library_panel");
}
