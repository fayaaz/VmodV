//! Palette-prop entry helpers used by the Inspector.
//!
//! A `palette` prop is an array of colour entries. Each entry is either a
//! legacy `{ r, g, b, a }` object of `0..1` ratios or a numeric `[r, g, b(, a)]`
//! array (byte-scaled when a colour channel exceeds `1.0`, otherwise `0..1`
//! ratios). These helpers read an entry as byte RGBA, build its CSS hex label
//! and write a picked colour back, keeping the entry's object/array shape and
//! numeric scale intact. They are pure so the shape rules can be unit-tested
//! without an egui harness.

use serde_json::Value;

use super::palette_editor;

/// Clamp a `0..1` ratio to a byte.
fn ratio_to_byte(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Numeric arrays are byte-scaled when any of their first three channels is
/// above `1.0`, matching `palette_editor`'s palette detection.
fn is_byte_scale(items: &[Value]) -> bool {
    items
        .iter()
        .take(3)
        .filter_map(Value::as_f64)
        .any(|value| value > 1.0)
}

/// The byte RGBA of one palette entry.
///
/// Objects carry `r`/`g`/`b`/`a` as `0..1` ratios; numeric arrays are read as
/// ratios when every colour channel is at most `1.0`, otherwise as bytes.
pub fn entry_rgba(entry: &Value) -> [u8; 4] {
    match entry {
        Value::Object(map) => {
            let channel = |key: &str, fallback: f64| {
                ratio_to_byte(map.get(key).and_then(Value::as_f64).unwrap_or(fallback))
            };
            [
                channel("r", 0.0),
                channel("g", 0.0),
                channel("b", 0.0),
                channel("a", 1.0),
            ]
        }
        Value::Array(items) => {
            let raw = |index: usize| items.get(index).and_then(Value::as_f64);
            let byte_scale = is_byte_scale(items);
            let convert = |value: f64| {
                if byte_scale {
                    value.clamp(0.0, 255.0).round() as u8
                } else {
                    ratio_to_byte(value)
                }
            };
            [
                convert(raw(0).unwrap_or(0.0)),
                convert(raw(1).unwrap_or(0.0)),
                convert(raw(2).unwrap_or(0.0)),
                if items.len() >= 4 {
                    convert(raw(3).unwrap_or(255.0))
                } else {
                    255
                },
            ]
        }
        _ => [0, 0, 0, 255],
    }
}

/// The `#RRGGBB` (or `#RRGGBBAA` when the alpha is not opaque) label of one
/// palette entry.
pub fn entry_hex(entry: &Value) -> String {
    let rgba = entry_rgba(entry);
    // Reuse the tween editor's hex formatter: an opaque alpha keeps the short
    // three-channel label, a translucent one includes the byte alpha.
    let shape = palette_editor::PaletteShape {
        channels: if rgba[3] == 255 { 3 } else { 4 },
        alpha_is_ratio: false,
    };
    palette_editor::rgba_hex(rgba, shape)
}

/// Write a byte RGBA back into one palette entry, keeping the entry's
/// object/array shape (and array byte-vs-ratio scale) intact.
pub fn set_entry(entry: &mut Value, rgba: [u8; 4]) {
    match entry {
        Value::Object(map) => {
            for (index, key) in ["r", "g", "b"].iter().enumerate() {
                map.insert(
                    (*key).to_string(),
                    serde_json::json!(f64::from(rgba[index]) / 255.0),
                );
            }
            if map.contains_key("a") {
                map.insert(
                    "a".to_string(),
                    serde_json::json!(f64::from(rgba[3]) / 255.0),
                );
            }
        }
        Value::Array(items) => {
            let byte_scale = is_byte_scale(items);
            let convert = |value: u8| {
                if byte_scale {
                    f64::from(value)
                } else {
                    f64::from(value) / 255.0
                }
            };
            for (index, value) in rgba.iter().take(3).enumerate() {
                if let Some(slot) = items.get_mut(index) {
                    *slot = serde_json::json!(convert(*value));
                }
            }
            if items.len() >= 4
                && let Some(slot) = items.get_mut(3)
            {
                *slot = serde_json::json!(convert(rgba[3]));
            }
        }
        _ => {}
    }
}

/// Parse a CSS hex label and write it into `palette[index]`.
///
/// Returns `false` (leaving `palette` untouched) when the text is not a valid
/// colour, the value is not an array, or `index` is out of range.
pub fn set_entry_hex(palette: &mut Value, index: usize, hex: &str) -> bool {
    let Some(rgba) = palette_editor::parse_hex(hex) else {
        return false;
    };
    let Value::Array(entries) = palette else {
        return false;
    };
    let Some(entry) = entries.get_mut(index) else {
        return false;
    };
    set_entry(entry, rgba);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: Option<f64>, expected: f64) {
        let actual = actual.unwrap_or(f64::NAN);
        assert!(
            (actual - expected).abs() <= 1.0 / 255.0,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn object_entries_read_as_ratio_rgba_and_hex() {
        let red = serde_json::json!({"r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0});
        assert_eq!(entry_rgba(&red), [255, 0, 0, 255]);
        assert_eq!(entry_hex(&red), "#FF0000");

        let translucent = serde_json::json!({"r": 0.0, "g": 1.0, "b": 0.0, "a": 0.5});
        assert_eq!(entry_rgba(&translucent), [0, 255, 0, 128]);
        assert_eq!(entry_hex(&translucent), "#00FF0080");
    }

    #[test]
    fn object_shape_and_alpha_presence_round_trip() {
        let mut entry = serde_json::json!({"r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0});
        set_entry(&mut entry, [0, 255, 0, 255]);
        assert_eq!(
            entry,
            serde_json::json!({"r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0})
        );
        assert_eq!(entry_hex(&entry), "#00FF00");

        // An object without `a` keeps its three-channel shape.
        let mut rgb = serde_json::json!({"r": 1.0, "g": 1.0, "b": 1.0});
        set_entry(&mut rgb, [10, 20, 30, 255]);
        assert!(rgb.get("a").is_none());
        close(rgb["r"].as_f64(), 10.0 / 255.0);
        assert_eq!(entry_hex(&rgb), "#0A141E");
    }

    #[test]
    fn array_entries_detect_byte_and_ratio_scale() {
        let bytes = serde_json::json!([255, 0, 0]);
        assert_eq!(entry_rgba(&bytes), [255, 0, 0, 255]);
        assert_eq!(entry_hex(&bytes), "#FF0000");

        let ratios = serde_json::json!([1.0, 0.0, 0.0]);
        assert_eq!(entry_rgba(&ratios), [255, 0, 0, 255]);

        let bytes_with_alpha = serde_json::json!([255, 0, 0, 128]);
        assert_eq!(entry_rgba(&bytes_with_alpha), [255, 0, 0, 128]);
        assert_eq!(entry_hex(&bytes_with_alpha), "#FF000080");
    }

    #[test]
    fn alpha_follows_the_array_scale() {
        let ratio = serde_json::json!([1.0, 0.0, 0.0, 0.5]);
        assert_eq!(entry_rgba(&ratio), [255, 0, 0, 128]);
        assert_eq!(entry_hex(&ratio), "#FF000080");

        let bytes = serde_json::json!([255, 0, 0, 128]);
        assert_eq!(entry_rgba(&bytes), [255, 0, 0, 128]);
    }

    #[test]
    fn array_shape_and_scale_round_trip() {
        let mut bytes = serde_json::json!([255, 0, 0]);
        set_entry(&mut bytes, [0, 255, 0, 255]);
        // Byte scale is kept: channels stay full-byte values and a 3-channel
        // array does not gain an alpha slot.
        assert_eq!(bytes.as_array().map(Vec::len), Some(3));
        let channels = bytes.as_array().expect("array");
        close(channels[0].as_f64(), 0.0);
        close(channels[1].as_f64(), 255.0);
        close(channels[2].as_f64(), 0.0);

        let mut ratios = serde_json::json!([1.0, 0.0, 0.0, 1.0]);
        set_entry(&mut ratios, [0, 255, 0, 128]);
        assert_eq!(ratios[0], 0.0);
        assert_eq!(ratios[1], 1.0);
        assert_eq!(ratios[2], 0.0);
        close(ratios[3].as_f64(), 128.0 / 255.0);
    }

    #[test]
    fn out_of_range_channels_are_clamped() {
        let object = serde_json::json!({"r": 2.0, "g": -1.0, "b": 0.5, "a": 1.0});
        assert_eq!(entry_rgba(&object), [255, 0, 128, 255]);
        let bytes = serde_json::json!([300, -5, 0]);
        assert_eq!(entry_rgba(&bytes), [255, 0, 0, 255]);
    }

    #[test]
    fn empty_and_non_colour_entries_are_safe() {
        assert_eq!(entry_rgba(&serde_json::json!([])), [0, 0, 0, 255]);
        assert_eq!(entry_rgba(&serde_json::json!(null)), [0, 0, 0, 255]);
        let mut empty = serde_json::json!([]);
        set_entry(&mut empty, [1, 2, 3, 4]);
        assert_eq!(empty, serde_json::json!([]));
    }

    #[test]
    fn set_entry_hex_updates_only_the_target_entry() {
        let mut palette = serde_json::json!([
            {"r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0},
            {"r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0},
        ]);
        assert!(set_entry_hex(&mut palette, 0, "#0000FF"));
        assert_eq!(entry_hex(&palette[0]), "#0000FF");
        assert_eq!(entry_hex(&palette[1]), "#00FF00");
    }

    #[test]
    fn set_entry_hex_ignores_invalid_input() {
        let original = serde_json::json!([{"r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0}]);
        let mut palette = original.clone();
        assert!(!set_entry_hex(&mut palette, 0, "not-a-colour"));
        assert_eq!(palette, original);
        assert!(!set_entry_hex(&mut palette, 9, "#FFFFFF"));
        assert_eq!(palette, original);
        let mut not_array = serde_json::json!({});
        assert!(!set_entry_hex(&mut not_array, 0, "#FFFFFF"));
    }
}
