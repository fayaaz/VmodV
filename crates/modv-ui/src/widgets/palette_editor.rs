//! Colour-palette tween helpers: detection, swatch maths and row edits.
//!
//! Electron's `PaletteControl.vue` edits a tween whose `data` rows are colours
//! (`Ball`/`Waveform`/`Polygon` `color` props declare `[r, g, b]` rows in
//! `0..=255`). The native tween editor shows those rows as a swatch strip and
//! edits them through an egui colour picker, keeping the descriptor the same
//! `{ data, keyframeTimes, ... }` shape.
//!
//! # Detection
//!
//! A tween is treated as a palette when every row is finite, has exactly three
//! (RGB) or four (RGBA) channels, every colour channel sits in `0..=255`, and at
//! least one channel is above `1.0` (the byte scale). The last condition keeps
//! a normalised `vec3` tween (`0..1`) on the numeric curve editor; a `vec3`
//! tween that genuinely ranges over `0..=255` is indistinguishable from a
//! palette and gets swatches. Fractional byte values are accepted, because a
//! keyframe inserted in the curve editor interpolates between swatches.
//!
//! The fourth channel is a `0..1` ratio when every alpha value is at most
//! `1.0`, otherwise a `0..=255` byte.
//!
//! # Times
//!
//! Palette rows are evenly timed. Adding or removing a swatch keeps an explicit
//! `keyframeTimes` list consistent by re-normalising it to the even grid for
//! three or more rows; a descriptor without times keeps its shorthand (anime's
//! evenly spaced keyframes), and two rows fall back to the `[from, to]`
//! shorthand.
//!
//! Pure helpers only: [`crate::widgets::tween_control::TweenControl`] draws them
//! and queues the `tweens/createTween` descriptor.

use egui::Color32;

/// How a colour-looking tween row is laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteShape {
    /// `3` for RGB rows, `4` for RGBA rows.
    pub channels: usize,
    /// Whether the fourth channel is a `0..1` ratio instead of a `0..=255`
    /// byte.
    pub alpha_is_ratio: bool,
}

/// Detect whether `data` is a colour palette, and how its rows are laid out.
pub fn palette_shape(data: &[Vec<f64>]) -> Option<PaletteShape> {
    let first = data.first()?;
    if data.len() < 2 {
        return None;
    }
    let channels = first.len();
    if !matches!(channels, 3 | 4) {
        return None;
    }
    if !data.iter().all(|row| {
        row.len() == channels
            && row
                .iter()
                .all(|value| value.is_finite() && (0.0..=255.0).contains(value))
    }) {
        return None;
    }
    // A normalised vec3 (`0..1`) is a numeric tween; colour rows are byte
    // scale. Fractional bytes are accepted: a curve-inserted keyframe
    // interpolates between swatches.
    if !data
        .iter()
        .flat_map(|row| row.iter().take(3))
        .any(|value| *value > 1.0)
    {
        return None;
    }
    let alpha_is_ratio = channels == 4
        && data
            .iter()
            .all(|row| row.get(3).is_some_and(|alpha| *alpha <= 1.0));
    Some(PaletteShape {
        channels,
        alpha_is_ratio,
    })
}

/// The swatch colour of one row (`0..=255` channels).
pub fn row_rgba(row: &[f64], shape: PaletteShape) -> [u8; 4] {
    let byte = |value: f64| {
        if value.is_finite() {
            value.clamp(0.0, 255.0).round() as u8
        } else {
            0
        }
    };
    let channel = |index: usize| row.get(index).copied().unwrap_or(0.0);
    let mut rgba = [byte(channel(0)), byte(channel(1)), byte(channel(2)), 255];
    if shape.channels == 4 {
        let alpha = row.get(3).copied().unwrap_or(1.0).clamp(0.0, 255.0);
        rgba[3] = if shape.alpha_is_ratio {
            (alpha * 255.0).round() as u8
        } else {
            alpha.round() as u8
        };
    }
    rgba
}

/// The swatch colour of one row as an egui colour.
pub fn row_color(row: &[f64], shape: PaletteShape) -> Color32 {
    let [r, g, b, a] = row_rgba(row, shape);
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// The `#RRGGBB` (or `#RRGGBBAA` for four channels) label of one row.
pub fn row_hex(row: &[f64], shape: PaletteShape) -> String {
    rgba_hex(row_rgba(row, shape), shape)
}

/// The `#RRGGBB` (or `#RRGGBBAA` for four channels) label of a colour.
pub fn rgba_hex(rgba: [u8; 4], shape: PaletteShape) -> String {
    let [r, g, b, a] = rgba;
    if shape.channels == 4 {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}")
    }
}

/// Write a picked colour into one row.
///
/// RGB is always written; alpha is written only for four-channel rows (and
/// converted back to a ratio when `shape.alpha_is_ratio`). Anything beyond the
/// fourth channel is left untouched. Returns `true` when a value changed.
pub fn set_row_rgba(row: &mut [f64], shape: PaletteShape, rgba: [u8; 4]) -> bool {
    if row.len() < 3 {
        return false;
    }
    let mut changed = false;
    for (index, value) in rgba.iter().take(3).enumerate() {
        let next = f64::from(*value);
        if row[index] != next {
            row[index] = next;
            changed = true;
        }
    }
    if shape.channels == 4
        && let Some(slot) = row.get_mut(3)
    {
        let next = if shape.alpha_is_ratio {
            f64::from(rgba[3]) / 255.0
        } else {
            f64::from(rgba[3])
        };
        if *slot != next {
            *slot = next;
            changed = true;
        }
    }
    changed
}

/// Parse `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` (the `#` is optional).
pub fn parse_hex(text: &str) -> Option<[u8; 4]> {
    let text = text.trim();
    let digits = text.strip_prefix('#').unwrap_or(text);
    if !digits
        .chars()
        .all(|character| character.is_ascii_hexdigit())
    {
        return None;
    }
    let expand = |text: &str| -> Option<Vec<u8>> {
        text.chars()
            .map(|character| character.to_digit(16).map(|value| (value * 17) as u8))
            .collect()
    };
    let bytes = |text: &str| -> Option<Vec<u8>> {
        (0..text.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&text[index..index + 2], 16).ok())
            .collect()
    };
    let mut values = match digits.len() {
        3 | 4 => expand(digits)?,
        6 | 8 => bytes(digits)?,
        _ => return None,
    };
    if values.len() == 3 {
        values.push(255);
    }
    Some([values[0], values[1], values[2], values[3]])
}

/// Append a swatch that duplicates the last row. Returns its index.
pub fn add_row(data: &mut Vec<Vec<f64>>) -> Option<usize> {
    let last = data.last()?.clone();
    if last.is_empty() {
        return None;
    }
    let index = data.len();
    data.push(last);
    Some(index)
}

/// Remove the swatch at `index`, refusing to drop below two rows.
pub fn remove_row(data: &mut Vec<Vec<f64>>, index: usize) -> bool {
    if data.len() <= 2 || index >= data.len() {
        return false;
    }
    data.remove(index);
    true
}

/// Evenly spaced keyframe times for `rows` rows.
pub fn even_times(rows: usize) -> Vec<f64> {
    if rows < 2 {
        return vec![0.0; rows];
    }
    let last = (rows - 1) as f64;
    (0..rows).map(|index| index as f64 / last).collect()
}

/// The `keyframeTimes` list after a row-count change.
///
/// An explicit list is re-normalised evenly; a descriptor without times keeps
/// `None`, and two rows use the `[from, to]` shorthand.
pub fn times_after_row_change(existing: Option<&[f64]>, rows: usize) -> Option<Vec<f64>> {
    if rows <= 2 || existing.is_none() {
        None
    } else {
        Some(even_times(rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb() -> Vec<Vec<f64>> {
        vec![vec![0.0, 0.0, 0.0], vec![255.0, 255.0, 255.0]]
    }

    fn rgba() -> Vec<Vec<f64>> {
        vec![vec![255.0, 0.0, 0.0, 255.0], vec![0.0, 255.0, 0.0, 128.0]]
    }

    #[test]
    fn ball_style_rgb_rows_are_a_palette() {
        let shape = palette_shape(&rgb()).expect("Ball palette");
        assert_eq!(
            shape,
            PaletteShape {
                channels: 3,
                alpha_is_ratio: false
            }
        );
        assert_eq!(row_rgba(&rgb()[1], shape), [255, 255, 255, 255]);
        assert_eq!(row_hex(&rgb()[0], shape), "#000000");
    }

    #[test]
    fn four_channel_rows_carry_alpha() {
        let shape = palette_shape(&rgba()).expect("RGBA palette");
        assert_eq!(shape.channels, 4);
        assert!(!shape.alpha_is_ratio);
        assert_eq!(row_rgba(&rgba()[1], shape), [0, 255, 0, 128]);
        assert_eq!(row_hex(&rgba()[0], shape), "#FF0000FF");
        assert_eq!(rgba_hex([255, 0, 128, 255], shape), "#FF0080FF");

        let ratio_rows = vec![vec![255.0, 0.0, 0.0, 1.0], vec![0.0, 255.0, 0.0, 0.25]];
        let ratio = palette_shape(&ratio_rows).expect("ratio alpha palette");
        assert!(ratio.alpha_is_ratio);
        assert_eq!(row_rgba(&ratio_rows[1], ratio), [0, 255, 0, 64]);
        assert_eq!(row_hex(&ratio_rows[1], ratio), "#00FF0040");
    }

    #[test]
    fn normalised_and_fractional_data_is_not_a_palette() {
        assert!(
            palette_shape(&[vec![0.0, 0.0, 0.0], vec![1.0, 0.5, 0.25]]).is_none(),
            "normalised vec3 stays on the curve editor"
        );
        assert!(
            palette_shape(&[vec![0.0], vec![1.0]]).is_none(),
            "one channel is a scalar tween"
        );
        assert!(
            palette_shape(&[vec![0.0, 0.0], vec![1.0, 2.0]]).is_none(),
            "two channels are a vec2"
        );
        assert!(
            palette_shape(&[vec![0.0, 0.0, 0.0, 0.0, 0.0], vec![1.0; 5]]).is_none(),
            "five channels are not a colour"
        );
        assert!(
            palette_shape(&[vec![0.0, 0.0, 0.0], vec![0.0, 300.0, 0.0]]).is_none(),
            "above the byte range"
        );
        assert!(
            palette_shape(&[vec![0.0, 0.0, 0.0], vec![f64::NAN, 0.0, 0.0]]).is_none(),
            "non-finite"
        );
        assert!(
            palette_shape(&[vec![0.0, 0.0, 0.0], vec![1.0, 2.0]]).is_none(),
            "ragged rows"
        );
        assert!(palette_shape(&[]).is_none());
        assert!(palette_shape(&[vec![0.0, 0.0, 255.0]]).is_none(), "one row");
    }

    #[test]
    fn fractional_swatches_stay_a_palette_and_round() {
        let rows = vec![[12.5, 100.25, 200.75], [0.0, 0.0, 255.0]]
            .into_iter()
            .map(|row| row.to_vec())
            .collect::<Vec<_>>();
        let shape = palette_shape(&rows).expect("interpolated palette");
        assert_eq!(row_rgba(&rows[0], shape), [13, 100, 201, 255]);
    }

    #[test]
    fn setting_a_swatch_preserves_the_unrelated_channels() {
        let mut rows = rgba();
        let shape = palette_shape(&rows).expect("RGBA palette");
        assert!(set_row_rgba(&mut rows[1], shape, [10, 20, 30, 255]));
        assert_eq!(rows[1], vec![10.0, 20.0, 30.0, 255.0]);
        assert!(!set_row_rgba(&mut rows[1], shape, [10, 20, 30, 255]));

        let mut ratio_rows = vec![vec![255.0, 0.0, 0.0, 1.0], vec![0.0, 255.0, 0.0, 0.5]];
        let ratio = palette_shape(&ratio_rows).expect("ratio palette");
        assert!(set_row_rgba(&mut ratio_rows[1], ratio, [0, 0, 255, 64]));
        assert_eq!(ratio_rows[1], vec![0.0, 0.0, 255.0, 64.0 / 255.0]);

        let mut rgb_rows = rgb();
        let shape = palette_shape(&rgb_rows).expect("RGB palette");
        assert!(set_row_rgba(&mut rgb_rows[0], shape, [1, 2, 3, 7]));
        assert_eq!(rgb_rows[0], vec![1.0, 2.0, 3.0], "RGB rows carry no alpha");
    }

    #[test]
    fn extra_channels_are_left_alone() {
        let mut row = vec![0.0, 0.0, 0.0, 1.0, 42.0];
        let shape = PaletteShape {
            channels: 3,
            alpha_is_ratio: false,
        };
        assert!(set_row_rgba(&mut row, shape, [9, 8, 7, 255]));
        assert_eq!(row, vec![9.0, 8.0, 7.0, 1.0, 42.0]);
    }

    #[test]
    fn hex_parsing_accepts_the_css_forms() {
        assert_eq!(parse_hex("#FF0080"), Some([255, 0, 128, 255]));
        assert_eq!(parse_hex("ff0080"), Some([255, 0, 128, 255]));
        assert_eq!(parse_hex("#F08"), Some([255, 0, 136, 255]));
        assert_eq!(parse_hex("#F08A"), Some([255, 0, 136, 170]));
        assert_eq!(parse_hex("#FF008080"), Some([255, 0, 128, 128]));
        assert_eq!(parse_hex("  #00ff00  "), Some([0, 255, 0, 255]));
        assert_eq!(parse_hex("#GG0000"), None);
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn adding_a_swatch_duplicates_the_last_colour() {
        let mut rows = rgb();
        let index = add_row(&mut rows).expect("added");
        assert_eq!(index, 2);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2], vec![255.0, 255.0, 255.0]);
        assert_eq!(add_row(&mut Vec::new()), None);
    }

    #[test]
    fn removing_a_swatch_keeps_two_rows() {
        let mut rows = vec![
            vec![0.0, 0.0, 0.0],
            vec![255.0, 0.0, 0.0],
            vec![0.0, 255.0, 0.0],
        ];
        assert!(remove_row(&mut rows, 1));
        assert_eq!(rows, vec![vec![0.0, 0.0, 0.0], vec![0.0, 255.0, 0.0]]);
        assert!(!remove_row(&mut rows, 0), "two rows are the minimum");
        assert_eq!(rows.len(), 2);
        assert!(!remove_row(&mut rows, 9), "out of range");
    }

    #[test]
    fn row_changes_keep_explicit_times_even() {
        assert_eq!(even_times(3), vec![0.0, 0.5, 1.0]);
        assert_eq!(even_times(1), vec![0.0]);
        assert_eq!(
            times_after_row_change(Some(&[0.0, 0.1, 1.0]), 4),
            Some(vec![0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0])
        );
        assert_eq!(times_after_row_change(None, 4), None);
        assert_eq!(times_after_row_change(Some(&[0.0, 0.1, 1.0]), 2), None);
        assert_eq!(times_after_row_change(None, 2), None);
    }
}
