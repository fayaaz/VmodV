//! Regression test: a non-hovered "still" Scene-grid cell must never paint
//! the live main-output texture when its own cached preview has been evicted
//! from the shared preview cache. Only a hovered cell may fall back to the
//! live preview.

use egui::TextureId;
use modv_ui::panels::{CellRef, SceneGridState};
use modv_ui::{EngineSnapshot, PreviewTexture, scene_preview_key};

#[test]
fn still_cell_never_paints_the_live_output_when_its_cached_preview_is_gone() {
    let mut grid = SceneGridState::default();
    let cell = CellRef::new("scene-a", 0);
    // Hover once to leave a still behind, as draw_scene_cell does.
    grid.keep_preview(&cell, &["Checkerboard".to_string()]);

    let snapshot = EngineSnapshot::default();
    // The still's isolated preview has been evicted from the shared cache.
    assert!(
        !snapshot
            .scene_previews
            .contains_key(&scene_preview_key("scene-a", 0))
    );
    let live = Some(TextureId::Managed(42));

    // Compute every policy branch up front so all three are exercised even
    // though the first assertion (the regression) is expected to fail.
    let evicted_still = grid.preview_texture_for(&cell, false, &snapshot, live);
    let hovered = grid.preview_texture_for(&cell, true, &snapshot, live);

    let mut cached_snapshot = snapshot.clone();
    cached_snapshot.scene_previews.insert(
        scene_preview_key("scene-a", 0),
        PreviewTexture {
            id: TextureId::Managed(7),
            width: 320,
            height: 180,
        },
    );
    let cached_still = grid.preview_texture_for(&cell, false, &cached_snapshot, live);

    // THE REGRESSION: an evicted still must not show the live main output.
    assert_eq!(
        evicted_still, None,
        "an evicted still must not show the live main output"
    );
    // A hovered cell may fall back to the live main output until its isolated
    // preview arrives; this behaviour must keep working.
    assert_eq!(
        hovered, live,
        "a hovered cell may fall back to the live preview"
    );
    // A still with its own cached texture must use that texture.
    assert_eq!(
        cached_still,
        Some(TextureId::Managed(7)),
        "a still must use its own cached preview texture"
    );
}

#[test]
fn non_preview_cell_never_paints_live_output() {
    let grid = SceneGridState::default();
    let cell = CellRef::new("scene-b", 1);
    let snapshot = EngineSnapshot::default();
    let live = Some(TextureId::Managed(99));
    assert_eq!(
        grid.preview_texture_for(&cell, false, &snapshot, live),
        None
    );
}
