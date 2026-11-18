//! Library → texture-prop drag payload and the prop value it writes.
//!
//! A Library tile carries a [`MediaLibraryDrag`] while the pointer drags it.
//! Every texture prop of kind `image`/`video` reads that payload to highlight
//! itself and, on release, writes the same value as typing the path by hand:
//! `{ "type": "<kind>", "options": { "path": "<media-root-relative path>" } }`.
//!
//! The payload and the value builder are pure so the click-to-assign flow and
//! the drag/drop flow cannot drift apart.

use serde_json::Value;

use crate::model::{LibraryKind, MediaAssetView};

/// The small control label that arms a texture prop for a library pick.
pub const USE_FROM_LIBRARY_LABEL: &str = "Use from library…";

/// The media asset a Library tile is dragging.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaLibraryDrag {
    /// Media-root-relative path (`default/image/loop.png`).
    pub path: String,
    /// Which media folder the asset came from.
    pub kind: LibraryKind,
    /// The file name, used by the drop-target chip.
    pub name: String,
}

impl MediaLibraryDrag {
    pub fn new(asset: &MediaAssetView) -> Self {
        Self {
            path: asset.path.clone(),
            kind: asset.kind,
            name: asset.name.clone(),
        }
    }
}

/// The texture-prop value for a library asset, or `None` for palettes.
pub fn texture_value_for_asset(kind: LibraryKind, path: &str) -> Option<Value> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let control = kind.texture_type()?;
    Some(serde_json::json!({
        "type": control,
        "options": { "path": path },
    }))
}

/// The value a drop writes, when the dragged asset can fill a texture prop of
/// `control_kind`. A mismatched asset (a video on an image prop) is not a drop
/// target, so the control never rewrites its own kind by accident.
pub fn texture_value_for_control(control_kind: &str, drag: &MediaLibraryDrag) -> Option<Value> {
    if drag.kind.texture_type()? != control_kind {
        return None;
    }
    texture_value_for_asset(drag.kind, &drag.path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(path: &str, kind: LibraryKind) -> MediaAssetView {
        MediaAssetView {
            path: path.to_string(),
            kind,
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn an_image_asset_writes_the_media_relative_prop_payload() {
        assert_eq!(
            texture_value_for_asset(LibraryKind::Image, "default/image/still.png"),
            Some(serde_json::json!({
                "type": "image",
                "options": { "path": "default/image/still.png" },
            }))
        );
    }

    #[test]
    fn a_video_asset_writes_a_video_prop_payload() {
        let drag = MediaLibraryDrag::new(&asset("live/video/clip.mp4", LibraryKind::Video));
        assert_eq!(drag.name, "clip.mp4");
        assert_eq!(
            texture_value_for_control("video", &drag),
            Some(serde_json::json!({
                "type": "video",
                "options": { "path": "live/video/clip.mp4" },
            }))
        );
    }

    #[test]
    fn mismatched_and_palette_assets_are_not_drop_targets() {
        let video = MediaLibraryDrag::new(&asset("live/video/clip.mp4", LibraryKind::Video));
        assert_eq!(
            texture_value_for_control("image", &video),
            None,
            "a video does not replace an image prop"
        );

        let palette = MediaLibraryDrag::new(&asset("live/palette/neon.json", LibraryKind::Palette));
        assert_eq!(texture_value_for_control("palette", &palette), None);
        assert_eq!(
            texture_value_for_asset(LibraryKind::Palette, "live/palette/neon.json"),
            None
        );
    }

    #[test]
    fn empty_paths_never_produce_a_payload() {
        assert_eq!(
            texture_value_for_asset(LibraryKind::Image, "   "),
            None,
            "an empty path would clear the prop instead of assigning"
        );
    }
}
