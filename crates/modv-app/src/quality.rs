//! Per-module render quality rules.
//!
//! The native app has a global *internal rendering resolution* preference
//! (Full / Balanced / Performance). This module decides, per module instance,
//! what that setting means, and it adds the per-module **Quality** prop
//! (`global` / `full` / `balanced` / `performance`) that overrides it.
//!
//! Classification at the `global` default:
//!
//! * **Heavy** — follows the global setting: ISF shaders whose resolved
//!   category is `3d-scenes`, and every Point10 scene.
//! * **Light** — always full resolution: every built-in 2D module, every
//!   built-in WGSL shader module, and ISF shaders whose category is
//!   anything else (effects, 2D Scenes, uncategorised).
//!
//! The per-module prop pins the scale absolutely: `full` is output-size,
//! `balanced` is three quarters, `performance` is half, `global` defers to
//! the rules above.

use serde_json::{Map, Value};

/// The synthetic prop name injected on every ISF module and Point10 scene.
pub const QUALITY_PROP: &str = "quality";

/// The `quality` prop value a module renders at when nothing is set.
pub const QUALITY_GLOBAL: &str = "global";

/// The scale factor each explicit override pins to, as a fraction of the
/// output resolution.
const BALANCED_SCALE: f32 = 0.75;
const PERFORMANCE_SCALE: f32 = 0.5;

/// Read the module's own `quality` prop, defaulting to `global` for missing
/// or malformed values.
pub fn module_quality(props: &Value) -> &str {
    props
        .get(QUALITY_PROP)
        .and_then(Value::as_str)
        .unwrap_or(QUALITY_GLOBAL)
}

/// Whether the module is heavy enough that the global render scale applies
/// at the `global` default.
///
/// `point10_heavy` is `Some(true)` for Point10 modules (all of which follow
/// the global setting), or `None` for non-point10 modules.
pub fn is_heavy(meta: &Map<String, Value>, point10_heavy: Option<bool>) -> bool {
    if let Some(heavy) = point10_heavy {
        return heavy;
    }
    meta.get("category")
        .and_then(Value::as_str)
        .is_some_and(|category| category == "3d-scenes")
}

/// The target size `module` renders at, in pixels.
///
/// `full_size` is the output resolution; `render_size` the global internal
/// rendering resolution. `point10_heavy` is `Some(true)` for Point10 modules
/// and `None` for anything else.
pub fn module_render_size(
    full_size: (u32, u32),
    render_size: (u32, u32),
    props: &Value,
    meta: &Map<String, Value>,
    point10_heavy: Option<bool>,
) -> (u32, u32) {
    let scale_dim =
        |dimension: u32, factor: f32| (dimension as f32 * factor).round().max(1.0) as u32;
    match module_quality(props) {
        "full" => full_size,
        "balanced" => (
            scale_dim(full_size.0, BALANCED_SCALE),
            scale_dim(full_size.1, BALANCED_SCALE),
        ),
        "performance" => (
            scale_dim(full_size.0, PERFORMANCE_SCALE),
            scale_dim(full_size.1, PERFORMANCE_SCALE),
        ),
        // "global" and anything unrecognised defer to the heavy/light split.
        _ => {
            if is_heavy(meta, point10_heavy) {
                render_size
            } else {
                full_size
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, Value, json};

    fn meta(category: Option<&str>) -> Map<String, Value> {
        let mut meta = Map::new();
        if let Some(category) = category {
            meta.insert("category".to_string(), json!(category));
        }
        meta
    }

    fn point10_props(value: &str) -> Value {
        json!({ "quality": value })
    }

    const FULL: (u32, u32) = (1920, 1080);
    const HALF: (u32, u32) = (960, 540);

    #[test]
    fn an_isf_with_no_3d_category_is_light_at_the_global_default() {
        let props = json!({});
        for category in [None, Some("effects"), Some("2d-scenes")] {
            let size = module_render_size(FULL, HALF, &props, &meta(category), None);
            assert_eq!(size, FULL, "category {category:?} should stay full");
        }
    }

    #[test]
    fn an_isf_in_the_3d_scenes_category_follows_the_global_setting() {
        let props = json!({});
        let size = module_render_size(FULL, HALF, &props, &meta(Some("3d-scenes")), None);
        assert_eq!(size, HALF);
    }

    #[test]
    fn point10_scenes_follow_the_global_setting_by_default() {
        let props = json!({});
        // The compositor classifies every Point10 module as heavy, whatever its
        // name, so the `global` default resolves to the global render size.
        assert_eq!(
            module_render_size(FULL, HALF, &props, &meta(None), Some(true)),
            HALF
        );
    }

    #[test]
    fn other_point10_scenes_follow_the_global_setting() {
        let props = json!({});
        assert_eq!(
            module_render_size(FULL, HALF, &props, &meta(None), Some(true)),
            HALF
        );
    }

    #[test]
    fn an_explicit_full_overrides_a_3d_scene_default() {
        let size = module_render_size(
            FULL,
            HALF,
            &point10_props("full"),
            &meta(Some("3d-scenes")),
            Some(true),
        );
        assert_eq!(size, FULL);
    }

    #[test]
    fn an_explicit_performance_forces_a_light_module_down() {
        let size = module_render_size(
            FULL,
            HALF,
            &point10_props("performance"),
            &meta(Some("effects")),
            None,
        );
        assert_eq!(size, (960, 540));
    }

    #[test]
    fn balanced_pins_three_quarters_of_the_output_resolution() {
        for (props, expected) in [("balanced", (1440, 810)), ("performance", (960, 540))] {
            let size =
                module_render_size(FULL, HALF, &json!({ "quality": props }), &meta(None), None);
            assert_eq!(size, expected, "{props}");
        }
    }

    #[test]
    fn a_missing_or_malformed_quality_prop_falls_back_to_global() {
        for props in [
            json!({}),
            json!({ "quality": null }),
            json!({ "quality": 42 }),
            json!({ "quality": "ultra" }),
        ] {
            let size = module_render_size(FULL, HALF, &props, &meta(Some("3d-scenes")), None);
            assert_eq!(size, HALF, "{props}");
        }
    }
}
