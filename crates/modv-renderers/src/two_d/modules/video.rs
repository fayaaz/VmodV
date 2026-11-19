//! Native-only `Video` module: `Texture 2D` plus an audio-linkable 0..1
//! `progress` prop that drives playback position on the app side.
//!
//! Drawing is identical to [`Texture2d`]; the `progress` input is consumed by
//! the app (not here) to seek video playback.

use super::texture_2d::texture_props;
use crate::two_d::modules::{DrawContext, Texture2d, TwoDModule, TwoDModuleMeta, TwoDProp};

/// The `Video` module: Texture 2D's props and drawing, plus a 0..1 `progress`
/// input that the existing input-link machinery can drive from audio.
#[derive(Default)]
pub struct Video;

impl TwoDModule for Video {
    fn meta(&self) -> TwoDModuleMeta {
        TwoDModuleMeta {
            name: "Video",
            author: "",
            version: "",
            category: None,
            audio_features: &[],
        }
    }

    fn props(&self) -> Vec<TwoDProp> {
        let mut props = texture_props();
        props.push(
            TwoDProp::float("progress", 0.0)
                .label("Progress")
                .range(0.0, 1.0)
                .step(0.001),
        );
        props
    }

    fn draw(&mut self, ctx: &mut DrawContext) {
        // Video draws exactly like Texture 2D; seeking by `progress` is an
        // app-side concern (B2) and does not change the 2D drawing here.
        let mut texture = Texture2d;
        texture.draw(ctx);
    }
}
