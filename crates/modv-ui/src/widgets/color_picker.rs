//! Accessible square/wheel colour picker.

use egui::{Color32, Mesh, Pos2, Sense, Stroke, StrokeKind, WidgetInfo, vec2};

use crate::theme;

/// The active colour-picking surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorPickerMode {
    #[default]
    Square,
    Wheel,
}

/// HSV -> sRGB ratios. `hue_degrees` is wrapped into 0..360; saturation/value clamped to 0..=1.
pub fn hsv_to_rgb(hue_degrees: f32, saturation: f32, value: f32) -> [f32; 3] {
    let hue = hue_degrees.rem_euclid(360.0);
    let saturation = saturation.clamp(0.0, 1.0);
    let value = value.clamp(0.0, 1.0);
    let chroma = value * saturation;
    let sector = hue / 60.0;
    let second = chroma * (1.0 - ((sector % 2.0) - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let base = value - chroma;
    [r + base, g + base, b + base]
}

/// sRGB ratios -> [hue 0..360, saturation 0..=1, value 0..=1]. Greys have hue 0.
pub fn rgb_to_hsv(rgb: [f32; 3]) -> [f32; 3] {
    let r = rgb[0].clamp(0.0, 1.0);
    let g = rgb[1].clamp(0.0, 1.0);
    let b = rgb[2].clamp(0.0, 1.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta <= f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let saturation = if max <= f32::EPSILON {
        0.0
    } else {
        delta / max
    };
    [hue, saturation, max]
}

/// `x` = saturation 0..=1, `y` = value 0..=1 (both clamped) at the given hue.
pub fn square_position_to_rgb(hue_degrees: f32, x: f32, y: f32) -> [f32; 3] {
    hsv_to_rgb(hue_degrees, x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))
}

/// `(x, y)` are offsets from the wheel centre normalised so the rim is radius 1
/// (x right, y down). angle = atan2(y, x) in degrees (0° at +x, 90° at +y),
/// radius clamped to 1 = saturation, value fixed at 1.
pub fn wheel_position_to_rgb(x: f32, y: f32) -> [f32; 3] {
    let hue = y.atan2(x).to_degrees().rem_euclid(360.0);
    let saturation = (x * x + y * y).sqrt().clamp(0.0, 1.0);
    hsv_to_rgb(hue, saturation, 1.0)
}

/// One reusable colour picker and its selected surface.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorPicker {
    pub rgba: [f32; 4],
    pub mode: ColorPickerMode,
}

impl ColorPicker {
    /// Clamp channels to 0..=1, mode Square.
    pub fn new(rgba: [f32; 4]) -> Self {
        Self {
            rgba: rgba.map(|channel| channel.clamp(0.0, 1.0)),
            mode: ColorPickerMode::Square,
        }
    }

    pub fn with_mode(self, mode: ColorPickerMode) -> Self {
        Self {
            rgba: self.rgba,
            mode,
        }
    }

    /// Clamp, write channels 0..2, keep alpha, return true when any RGB channel changed.
    pub fn set_rgb(&mut self, rgb: [f32; 3]) -> bool {
        let rgb = rgb.map(|channel| channel.clamp(0.0, 1.0));
        let changed = self.rgba[0] != rgb[0] || self.rgba[1] != rgb[1] || self.rgba[2] != rgb[2];
        if changed {
            self.rgba[0] = rgb[0];
            self.rgba[1] = rgb[1];
            self.rgba[2] = rgb[2];
        }
        changed
    }

    /// Draw the mode toggles plus the active surface; true when picking changed RGB.
    /// Surfaces must be labelled through WidgetInfo for accessibility:
    /// "Colour square" (square mode) and "Colour wheel" (wheel mode).
    /// Mode buttons read exactly "Square" and "Wheel".
    pub fn draw(&mut self, ui: &mut egui::Ui) -> bool {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.mode == ColorPickerMode::Square, "Square")
                .clicked()
            {
                self.mode = ColorPickerMode::Square;
            }
            if ui
                .selectable_label(self.mode == ColorPickerMode::Wheel, "Wheel")
                .clicked()
            {
                self.mode = ColorPickerMode::Wheel;
            }
            let (preview, _) = ui.allocate_exact_size(vec2(48.0, 18.0), Sense::hover());
            let painter = ui.painter_at(preview);
            paint_swatch(
                &painter,
                preview,
                rgba_color(self.rgba),
                Color32::from_gray(90),
            );
        });

        let mut changed = false;
        let mut hsv = self.hsv();
        match self.mode {
            ColorPickerMode::Square => {
                if ui
                    .add(egui::Slider::new(&mut hsv[0], 0.0..=360.0).text("Hue"))
                    .changed()
                {
                    changed |= self.set_rgb(hsv_to_rgb(hsv[0], hsv[1], hsv[2]));
                    hsv = self.hsv();
                }
                let [hue, saturation, value] = hsv;
                let (rect, response) =
                    ui.allocate_exact_size(vec2(140.0, 140.0), Sense::click_and_drag());
                response
                    .widget_info(|| WidgetInfo::labeled(egui::Role::Canvas, true, "Colour square"));
                let painter = ui.painter_at(rect);
                paint_square(&painter, rect, hue);
                painter.rect_stroke(
                    rect,
                    egui::CornerRadius::same(0),
                    Stroke::new(1.0, Color32::from_gray(90)),
                    StrokeKind::Inside,
                );
                paint_marker(
                    &painter,
                    Pos2::new(
                        rect.left() + saturation * rect.width(),
                        rect.top() + (1.0 - value) * rect.height(),
                    ),
                    self.rgba,
                );
                if (response.clicked() || response.dragged())
                    && let Some(pos) = pointer_pos(ui, &response)
                {
                    let x = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                    let y = ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
                    changed |= self.set_rgb(square_position_to_rgb(hue, x, 1.0 - y));
                }
            }
            ColorPickerMode::Wheel => {
                if ui
                    .add(egui::Slider::new(&mut hsv[2], 0.0..=1.0).text("Value"))
                    .changed()
                {
                    changed |= self.set_rgb(hsv_to_rgb(hsv[0], hsv[1], hsv[2]));
                    hsv = self.hsv();
                }
                let [hue, saturation, value] = hsv;
                let (rect, response) =
                    ui.allocate_exact_size(vec2(140.0, 140.0), Sense::click_and_drag());
                response
                    .widget_info(|| WidgetInfo::labeled(egui::Role::Canvas, true, "Colour wheel"));
                let painter = ui.painter_at(rect);
                paint_wheel(&painter, rect);
                let centre = rect.center();
                let radius = rect.width().min(rect.height()) * 0.5;
                let angle = hue.to_radians();
                paint_marker(
                    &painter,
                    centre + vec2(angle.cos(), angle.sin()) * (radius * saturation),
                    self.rgba,
                );
                painter.circle_stroke(centre, radius, Stroke::new(1.0, Color32::from_gray(90)));
                if (response.clicked() || response.dragged())
                    && let Some(pos) = pointer_pos(ui, &response)
                {
                    let half = radius.max(1.0);
                    let x = (pos.x - centre.x) / half;
                    let y = (pos.y - centre.y) / half;
                    let [pick_hue, pick_saturation, _] = rgb_to_hsv(wheel_position_to_rgb(x, y));
                    changed |= self.set_rgb(hsv_to_rgb(pick_hue, pick_saturation, value));
                }
            }
        }
        changed
    }

    fn hsv(&self) -> [f32; 3] {
        rgb_to_hsv([self.rgba[0], self.rgba[1], self.rgba[2]])
    }
}

/// Draw an accessible swatch button that opens the square/wheel picker popup.
///
/// The button is labelled `Colour picker for {label}`; `mode_key` namespaces the
/// persisted mode so it survives the per-frame control rebuild. Returns the
/// picker's RGBA when a pick happened (alpha is left for the caller to keep).
pub(crate) fn show_picker_button(
    ui: &mut egui::Ui,
    label: &str,
    rgba: [f32; 4],
    size: egui::Vec2,
    swatch_color: Color32,
    mode_key: &'static str,
) -> Option<[f32; 4]> {
    let mode_id = ui.make_persistent_id((mode_key, label));
    let mode = ui
        .data_mut(|data| data.get_temp::<ColorPickerMode>(mode_id))
        .unwrap_or_default();
    let mut picker = ColorPicker::new(rgba).with_mode(mode);

    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("Colour picker for {label}"),
        )
    });
    if ui.is_rect_visible(rect) {
        paint_swatch(ui.painter(), rect, swatch_color, theme::BORDER);
    }

    let mut picked = false;
    egui::Popup::from_toggle_button_response(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| picked = picker.draw(ui));
    ui.data_mut(|data| data.insert_temp(mode_id, picker.mode));
    picked.then_some(picker.rgba)
}

fn pointer_pos(ui: &egui::Ui, response: &egui::Response) -> Option<Pos2> {
    response
        .interact_pointer_pos()
        .or_else(|| ui.input(|input| input.pointer.interact_pos()))
}

fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn rgba_color(rgba: [f32; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(
        channel(rgba[0]),
        channel(rgba[1]),
        channel(rgba[2]),
        channel(rgba[3]),
    )
}

/// RGB ratios as an opaque `Color32`.
pub(crate) fn rgb_color(rgb: [f32; 3]) -> Color32 {
    Color32::from_rgb(channel(rgb[0]), channel(rgb[1]), channel(rgb[2]))
}

fn paint_swatch(painter: &egui::Painter, rect: egui::Rect, color: Color32, border: Color32) {
    let corner = egui::CornerRadius::same(3);
    painter.rect_filled(rect, corner, color);
    painter.rect_stroke(rect, corner, Stroke::new(1.0, border), StrokeKind::Inside);
}

/// A saturation/value square at `hue`: white top-left, hue top-right, black bottom.
fn paint_square(painter: &egui::Painter, rect: egui::Rect, hue: f32) {
    const STEPS: usize = 16;
    let stride = STEPS + 1;
    let mut mesh = Mesh::default();
    for iy in 0..=STEPS {
        for ix in 0..=STEPS {
            let x = ix as f32 / STEPS as f32;
            let y = iy as f32 / STEPS as f32;
            let pos = Pos2::new(
                rect.left() + x * rect.width(),
                rect.top() + y * rect.height(),
            );
            mesh.colored_vertex(pos, rgb_color(square_position_to_rgb(hue, x, 1.0 - y)));
        }
    }
    for iy in 0..STEPS {
        for ix in 0..STEPS {
            let index = (iy * stride + ix) as u32;
            let right = index + 1;
            let below = index + stride as u32;
            let below_right = below + 1;
            mesh.add_triangle(index, right, below_right);
            mesh.add_triangle(index, below_right, below);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// A hue wheel: white centre, fully saturated rim, blended linearly.
fn paint_wheel(painter: &egui::Painter, rect: egui::Rect) {
    const SEGMENTS: usize = 64;
    let centre = rect.center();
    let radius = rect.width().min(rect.height()) * 0.5;
    let mut mesh = Mesh::default();
    mesh.colored_vertex(centre, Color32::WHITE);
    for segment in 0..=SEGMENTS {
        let angle = segment as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let pos = centre + vec2(angle.cos(), angle.sin()) * radius;
        mesh.colored_vertex(pos, rgb_color(hsv_to_rgb(angle.to_degrees(), 1.0, 1.0)));
    }
    for segment in 0..SEGMENTS {
        let current = 1 + segment as u32;
        mesh.add_triangle(0, current, current + 1);
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn paint_marker(painter: &egui::Painter, pos: Pos2, rgba: [f32; 4]) {
    painter.circle_filled(pos, 5.0, rgba_color(rgba));
    painter.circle_stroke(pos, 5.0, Stroke::new(1.5, Color32::WHITE));
    painter.circle_stroke(pos, 6.5, Stroke::new(1.0, Color32::from_black_alpha(160)));
}
