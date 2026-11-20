//! Native-only `Video` module: `Texture 2D` plus `start` and `finish` trim
//! props (both 0..1 fractions of the clip) that bound playback on the app side.
//!
//! Drawing is identical to [`Texture2d`]; the `start`/`finish` trims are
//! consumed by the app (not here), which loops playback between them.

use super::texture_2d::texture_props;
use crate::two_d::modules::{DrawContext, Texture2d, TwoDModule, TwoDModuleMeta, TwoDProp};

/// The `Video` module: Texture 2D's props and drawing, plus `start`/`finish`
/// trims (0 = beginning, 1 = end, defaults 0 and 1) that the existing
/// input-link machinery can drive to loop a clip between two points.
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
            TwoDProp::float("start", 0.0)
                .label("Start")
                .range(0.0, 1.0)
                .step(0.001),
        );
        props.push(
            TwoDProp::float("finish", 1.0)
                .label("Finish")
                .range(0.0, 1.0)
                .step(0.001),
        );
        props
    }

    fn draw(&mut self, ctx: &mut DrawContext) {
        // Video draws exactly like Texture 2D; looping between the `start` and
        // `finish` trims is an app-side concern and does not change drawing.
        let mut texture = Texture2d;
        texture.draw(ctx);
    }
}
