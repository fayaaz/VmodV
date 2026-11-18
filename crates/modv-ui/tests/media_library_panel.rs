//! Kittest coverage for the native Library tab.
//!
//! The Library browses the active profile's image/video/palette folders
//! recursively. These tests drive the real shell: the tab strip, folder
//! switching/back, the grid, the Rescan command and the empty/error states.
//! Asset assignment is covered by the library/texture-control tests.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{LibraryKind, MediaAssetView, MediaLibraryView};
use modv_ui::state::SidePanelTab;
use modv_ui::{EngineSnapshot, UiCommand, UiState};

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
