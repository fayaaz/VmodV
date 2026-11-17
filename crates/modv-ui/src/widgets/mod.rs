//! The prop-driven widget set.
//!
//! P0 implements the highest-churn control, [`RangeControl`], and the reusable
//! status badges. P3 adds the remaining control types from `Control.vue`.

pub mod color_control;
pub mod range_control;
pub mod tween_control;
pub mod vec_control;

pub use color_control::ColorControl;
pub use range_control::RangeControl;
pub use tween_control::TweenControl;
pub use vec_control::VecControl;
