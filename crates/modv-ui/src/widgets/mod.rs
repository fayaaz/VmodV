//! The prop-driven widget set.
//!
//! P0 implements the highest-churn control, [`RangeControl`], and the reusable
//! status badges. P3 adds the remaining control types from `Control.vue`.

pub mod color_control;
pub mod input_link;
pub mod palette_editor;
pub mod range_control;
pub mod texture_control;
pub mod tween_control;
pub mod tween_curve;
pub mod vec_control;

pub use color_control::ColorControl;
pub use range_control::RangeControl;
pub use texture_control::TextureControl;
pub use tween_control::TweenControl;
pub use tween_curve::{
    CHANNEL_COLORS, MIN_KEYFRAME_GAP, ParsedTweenJson, TweenAxis, TweenCurve, canvas_size,
    handle_pos, nearest_handle, parse_tween_json, progress_to_x, tween_axis, valid_times,
    value_to_y, x_to_progress, y_to_value,
};
pub use vec_control::VecControl;
