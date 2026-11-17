//! The prop-driven widget set.
//!
//! P0 implements the highest-churn control, [`RangeControl`], and the reusable
//! status badges. P3 adds the remaining control types from `Control.vue`.

pub mod range_control;

pub use range_control::RangeControl;
