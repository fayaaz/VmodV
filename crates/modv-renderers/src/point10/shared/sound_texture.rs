//! Reusable Plane9 `SoundTexture` node (`P9Nodes.js:569-616`).
//!
//! The node owns a `Width × History` RGBA8 texture whose columns are spectrum
//! bands (or waveform samples) and whose rows are a scrolling history: the
//! newest row sits at `v = 0` and every frame pushes the older rows one row
//! down. Each channel is a damping-smoothed, slew-limited follower of the raw
//! audio read, quantised to 8 bits. `r = g = b = value`, `a = 255`.
//!
//! This is a standalone helper rather than scene-local code because the
//! spectrum presets (Music Waves, Gravity Spectrum, Block Spectrum, Monster
//! Spectrum and Spectrum Ocean) all bind the same node with different ports.
//! Port defaults come from the XML (`SoundType 0` = spectrum, `History 100`,
//! `Width 256`, `Damping 0.4`, `IncreaseRate 1`, `DecreaseRate 0.5`,
//! `SpectrumAmplification 2`).
//!
//! # Ports
//!
//! * `SoundType` — `0` spectrum (`audio.spectrum`), `1` waveform
//!   (`audio.waveform * 0.5 + 0.5`).
//! * `History` / `Width` — texture height / width.
//! * `Damping` — `0` snaps to the raw read, `1` freezes the follower.
//! * `IncreaseRate` / `DecreaseRate` — per-second slew limits.
//! * `SpectrumAmplification` — pre-quantisation gain, spectrum only.
//!
//! # Determinism
//!
//! No audio leaves the texture untouched (`if (!audio || !audio.hasAudio)
//! return;`). A zero `dt` falls back to `1/60 s` exactly like the JS
//! (`(ctx && ctx.dt) || 1 / 60`), and `dt` is clamped to `[0, 0.25]`.

use crate::point10::audio_bridge::P10Audio;

/// `SoundType 0` — spectrum columns.
pub const SOUND_TYPE_SPECTRUM: u32 = 0;
/// `SoundType 1` — waveform columns.
pub const SOUND_TYPE_WAVEFORM: u32 = 1;

/// One `SoundTexture` node's ports (XML `SoundTexture` defaults).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundTextureConfig {
    /// `SoundType` (`0` spectrum, `1` waveform).
    pub sound_type: u32,
    /// Texture width (spectrum bands / waveform samples).
    pub width: u32,
    /// Texture height (scrolling rows).
    pub history: u32,
    /// Damping in `0..1`.
    pub damping: f32,
    /// Per-second increase slew.
    pub increase_rate: f32,
    /// Per-second decrease slew.
    pub decrease_rate: f32,
    /// Spectrum pre-quantisation gain.
    pub amplification: f32,
}

impl SoundTextureConfig {
    /// Build a config with all ports explicit.
    pub const fn new(
        sound_type: u32,
        width: u32,
        history: u32,
        damping: f32,
        increase_rate: f32,
        decrease_rate: f32,
        amplification: f32,
    ) -> Self {
        Self {
            sound_type,
            width,
            history,
            damping,
            increase_rate,
            decrease_rate,
            amplification,
        }
    }
}

impl Default for SoundTextureConfig {
    fn default() -> Self {
        Self::new(SOUND_TYPE_SPECTRUM, 256, 256, 0.3, 10.0, 10.0, 1.0)
    }
}

/// The scroll step: move the newest row from the top to `v = 0` and push the
/// older rows one row down.
pub(crate) fn scroll_history(data: &mut [u8], row: &[u8]) {
    let row_bytes = row.len();
    if row_bytes == 0 || row_bytes >= data.len() {
        return;
    }
    data.copy_within(0..data.len() - row_bytes, row_bytes);
    data[..row_bytes].copy_from_slice(row);
}

/// One column's damped/slewed/quantised value.
///
/// Returns `(smoothed, value, quantised)` so the caller keeps both latching
/// arrays. The expression order mirrors `P9Nodes.js:592-605`.
pub(crate) fn step_channel(
    raw: f32,
    smoothed: f32,
    previous: f32,
    config: SoundTextureConfig,
    dt: f32,
) -> (f32, f32, u8) {
    let SoundTextureConfig {
        damping,
        increase_rate,
        decrease_rate,
        amplification,
        sound_type,
        ..
    } = config;
    let is_spectrum = sound_type == SOUND_TYPE_SPECTRUM;
    let follow = f32::min(1.0, dt * 30.0) * (1.0 - damping);
    let smoothed = smoothed + (raw - smoothed) * follow;
    let rate = if smoothed >= previous {
        increase_rate
    } else {
        decrease_rate
    };
    let step = rate * dt;
    let mut value = previous + (smoothed - previous).clamp(-step, step);
    if is_spectrum {
        let scaled = value * amplification;
        value = scaled;
        value = value.min(1.0);
    }
    value = value.max(0.0);
    let quantized = (value * 255.0).round() as u8;
    (smoothed, value, quantized)
}

/// The `SoundTexture` node's GPU texture plus its CPU latching state.
pub struct SoundTexture {
    config: SoundTextureConfig,
    /// Damping-smoothed targets (`n._smoo`).
    smoothed: Vec<f32>,
    /// Last written values (`n._bands`).
    values: Vec<f32>,
    /// RGBA8 history; row 0 is the newest.
    data: Vec<u8>,
    /// The bound texture.
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    queue: wgpu::Queue,
}

impl SoundTexture {
    /// Create the texture and its empty history at `config`.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, config: SoundTextureConfig) -> Self {
        let width = config.width.max(1);
        let history = config.history.max(1);
        let config = SoundTextureConfig {
            width,
            history,
            ..config
        };
        let data = vec![0_u8; (width * history * 4) as usize];
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("point10-sound-texture"),
            size: wgpu::Extent3d {
                width,
                height: history,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            smoothed: vec![0.0; width as usize],
            values: vec![0.0; width as usize],
            data,
            _texture: texture,
            view,
            queue: queue.clone(),
            config,
        }
    }

    /// The node's ports.
    pub fn config(&self) -> SoundTextureConfig {
        self.config
    }

    /// The texture view the scene binds as `gTexture1`.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Advance one frame and upload the scrolled history.
    pub fn update(&mut self, dt: f32, audio: &P10Audio) {
        if !audio.has_audio() {
            return;
        }
        let dt = if dt.is_finite() && dt != 0.0 {
            dt
        } else {
            1.0 / 60.0
        };
        let dt = dt.clamp(0.0, 0.25);

        let width = self.config.width;
        let is_spectrum = self.config.sound_type == SOUND_TYPE_SPECTRUM;
        // The JS clamps these ports once per frame, before the column loop.
        let limits = SoundTextureConfig {
            damping: self.config.damping.clamp(0.0, 1.0),
            increase_rate: self.config.increase_rate.max(0.0),
            decrease_rate: self.config.decrease_rate.max(0.0),
            amplification: self.config.amplification.max(0.0),
            ..self.config
        };

        let mut row = vec![0_u8; width as usize * 4];
        for x in 0..width {
            let p = if width > 1 {
                x as f32 / (width - 1) as f32
            } else {
                0.5
            };
            let raw = if is_spectrum {
                audio.spectrum(0, p, 0.0)
            } else {
                audio.waveform(0, p, 0.0) * 0.5 + 0.5
            };
            let index = x as usize;
            let (smoothed, value, quantized) =
                step_channel(raw, self.smoothed[index], self.values[index], limits, dt);
            self.smoothed[index] = smoothed;
            self.values[index] = value;
            let offset = index * 4;
            row[offset] = quantized;
            row[offset + 1] = quantized;
            row[offset + 2] = quantized;
            row[offset + 3] = 255;
        }

        scroll_history(&mut self.data, &row);
        self.upload();
    }

    /// Upload the whole CPU history to the texture.
    fn upload(&self) {
        let history = self.config.history as usize;
        let row_bytes = self.config.width as usize * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let padded = row_bytes.div_ceil(align) * align;
        let mut bytes = vec![0_u8; padded * history];
        for y in 0..history {
            bytes[y * padded..y * padded + row_bytes]
                .copy_from_slice(&self.data[y * row_bytes..(y + 1) * row_bytes]);
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self._texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded as u32),
                rows_per_image: Some(self.config.history),
            },
            wgpu::Extent3d {
                width: self.config.width,
                height: self.config.history,
                depth_or_array_layers: 1,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damping_and_slew_limit_the_follower() {
        // Half damping, dt = 1/60: follow = 0.5 * 0.5 = 0.25.
        let config = SoundTextureConfig::new(SOUND_TYPE_SPECTRUM, 1, 1, 0.5, 1.0, 0.5, 2.0);
        let (smoothed, value, quantized) = step_channel(1.0, 0.0, 0.0, config, 1.0 / 60.0);
        assert!((smoothed - 0.25).abs() < 1e-6, "{smoothed}");
        // The rise is slew-clamped to increase_rate * dt before amplification.
        let expected = ((1.0_f32 / 60.0) * 2.0).min(1.0);
        assert!((value - expected).abs() < 1e-6, "{value}");
        assert_eq!(quantized, (expected * 255.0).round() as u8);
    }

    #[test]
    fn fall_is_slew_limited_by_the_decrease_rate() {
        // From 1 with a zero target the value may only fall by dec * dt.
        let config = SoundTextureConfig::new(SOUND_TYPE_SPECTRUM, 1, 1, 0.0, 4.0, 0.5, 1.0);
        let (_, value, _) = step_channel(0.0, 1.0, 1.0, config, 0.1);
        assert!((value - 0.95).abs() < 1e-6, "{value}");
    }

    #[test]
    fn spectrum_amplification_clamps_and_quantises() {
        let config = SoundTextureConfig::new(SOUND_TYPE_SPECTRUM, 1, 1, 0.0, 4.0, 4.0, 2.0);
        let (_, value, quantized) = step_channel(1.0, 1.0, 1.0, config, 1.0);
        assert_eq!(value, 1.0);
        assert_eq!(quantized, 255);
    }

    #[test]
    fn newest_row_scrolls_to_the_top() {
        let mut data = vec![0_u8; 8];
        data.copy_from_slice(&[1, 1, 1, 1, 2, 2, 2, 2]);
        scroll_history(&mut data, &[9, 9, 9, 9]);
        assert_eq!(data, [9, 9, 9, 9, 1, 1, 1, 1]);
        scroll_history(&mut data, &[8, 8, 8, 8]);
        assert_eq!(data, [8, 8, 8, 8, 9, 9, 9, 9]);
    }
}
