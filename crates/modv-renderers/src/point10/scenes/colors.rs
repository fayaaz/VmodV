//! Plane9 "Colors" — Rust/wgpu port of `Abstract/Colors.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Abstract/Colors.xml` by Joakim Dahl,
//! CC BY-NC-SA 4.0.
//!
//! The preset is a single `Shader1` over the full screen: a Voronoi cell
//! position (`_voronoi` at `si.tex * gIn1.y`) feeds a noise gradient, and the
//! nearest cell's position drives a cosine palette. The expression advances
//! `perm` with the bass band and sets `in1.x` (the palette phase) and `in1.y`
//! (the Voronoi frequency, 6..36).
//!
//! # Ported exactly
//!
//! * the palette arguments `a = 1`, `b = (0, 0.2, 0.6)` scaled by `0.3` on
//!   `_noise(centerPos) * 0.5 + 0.5`;
//! * `centerPos = vec3(pos1.xy * 0.05, gIn1.x * 0.8)`;
//! * the radial mask `smoothstep(1.75, 0.5, length(p))` (reversed edges are
//!   GLSL's, via `p9_smoothstep`);
//! * the expression `perm += min(0.5, band(-1,0,0,0.01) * 5 + 0.001) * dt`,
//!   `in1.x = perm + permrand * 10`, `in1.y = 6 + permrand * 30`.
//!
//! The GLSL's `v` (turbulence/`f2 - f1`) and `light` are computed but never
//! read in the shipped shader, so the port keeps the live palette path only.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::plane9_common::{Plane9Pass, Plane9Uniforms};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::p9_shading::P9_SHADING_WGSL;
use crate::point10::{
    HDR_FORMAT, Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, scene_props_with_levers,
};

/// `Screen1` camera position.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, 2.0];
/// `Screen1` camera look direction (`CamLookAtInWorldSpace false`).
pub const CAMERA_LOOK: [f32; 3] = [0.0, 0.0, -1.0];
/// `Clear1` colour.
pub const CLEAR: wgpu::Color = wgpu::Color::TRANSPARENT;
/// `perm` advance cap.
pub const MAX_PERM_STEP: f32 = 0.5;
/// `permrand` seed (the XML `RandomSeed`).
pub const PERM_RAND: f32 = 1.0;

const COLORS_PROPS: &[SceneProp] = &[];

/// One frame of the Colors expression.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorsDrive {
    /// `perm` for the next frame.
    pub perm: f32,
    /// `in1.x = perm + permrand * 10`.
    pub in1_x: f32,
    /// `in1.y = 6 + permrand * 30`.
    pub in1_y: f32,
}

/// Pure port of the Colors expression.
pub fn colors_drive(bass: f32, dt: f32, perm: f32) -> ColorsDrive {
    let bass = if bass.is_finite() { bass.max(0.0) } else { 0.0 };
    let step = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    let perm = if perm.is_finite() { perm } else { 1.0 };
    let advance = (bass * 5.0 + 0.001).min(MAX_PERM_STEP) * step;
    let next = perm + advance;
    ColorsDrive {
        perm: next,
        in1_x: next + PERM_RAND * 10.0,
        in1_y: 6.0 + PERM_RAND * 30.0,
    }
}

/// `Plane9 Colors` scene.
pub struct Colors {
    audio: P10Audio,
    perm: f32,
    elapsed: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    uniforms: Plane9Uniforms,
    pass: Option<Plane9Pass>,
}

impl Default for Colors {
    fn default() -> Self {
        Self::new()
    }
}

impl Colors {
    /// Create the scene.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            perm: 1.0,
            elapsed: 0.0,
            size: (1, 1),
            target_format: HDR_FORMAT,
            device: None,
            queue: None,
            uniforms: Plane9Uniforms::new(1, 1),
            pass: None,
        }
    }

    /// The current `perm`.
    pub fn perm(&self) -> f32 {
        self.perm
    }

    /// The uniform block last uploaded (for tests).
    pub fn uniforms(&self) -> &Plane9Uniforms {
        &self.uniforms
    }

    /// Bind the modV props. These presets declare no `sceneProps` of their
    /// own, so this only exists to satisfy the registry's prop binder.
    pub fn apply_props(&mut self, _props: impl FnMut(&str) -> Option<f32>) {}

    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        self.pass = Some(Plane9Pass::new(
            &device,
            &queue,
            self.target_format,
            "point10-plane9-colors",
            colors_wgsl(),
            None,
        ));
    }
}

fn colors_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| {
        format!(
            "{P9_COMMON_WGSL}\n{P9_SHADING_WGSL}\n{}\n{COLORS_FRAGMENT}",
            crate::point10::scenes::plane9_common::PLANE9_BATCH_WGSL
        )
    })
}

const COLORS_FRAGMENT: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let p = tex * vec2<f32>(2.0) - vec2<f32>(1.0);
  let t = u.in1.x * 0.8;
  let vor = p9_voronoi3(vec3<f32>(tex.xy * u.in1.y, 0.0));
  let center_pos = vec3<f32>(vor.pos1.xy * 0.05, t);
  let tone = p9_noise(center_pos) * 0.5 + 0.5;
  var c = p9_palette(
    tone,
    vec3<f32>(0.5),
    vec3<f32>(0.5),
    vec3<f32>(1.0, 1.0, 1.0),
    vec3<f32>(0.0, 0.2, 0.6),
  ) * 0.3;
  c = c * p9_smoothstep(1.75, 0.5, length(p));
  return vec4<f32>(c, 1.0);
}
"#;

impl Point10Scene for Colors {
    fn name(&self) -> &'static str {
        "Plane9 Colors"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.size = (ctx.width.max(1), ctx.height.max(1));
        self.uniforms = Plane9Uniforms::new(self.size.0, self.size.1);
        self.rebuild();
        if let Some(pass) = &self.pass {
            pass.upload(&self.uniforms);
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        let size = (width.max(1), height.max(1));
        if self.size == size {
            return;
        }
        self.size = size;
        self.uniforms.resolution = [size.0 as f32, size.1 as f32];
        self.rebuild();
    }

    fn update(&mut self, dt: f32, audio: &AudioBridge) {
        let dt = frame_dt(dt);
        self.audio.update(dt, audio);
        self.elapsed += dt;
        let drive = colors_drive(self.audio.band(-1, 0, 0, 0.01), dt, self.perm);
        self.perm = drive.perm;
        self.uniforms.time = self.elapsed;
        self.uniforms.in1 = [drive.in1_x, drive.in1_y, 0.0, 0.0];
        if let Some(pass) = &self.pass {
            pass.upload(&self.uniforms);
        }
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        if let Some(pass) = &self.pass {
            pass.draw(encoder, target, CLEAR, "point10-plane9-colors");
        }
    }

    fn dispose(&mut self) {
        self.pass = None;
        self.device = None;
        self.queue = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(COLORS_PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Colors", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn camera_and_clear_match_the_preset() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, 2.0]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, -1.0]);
        assert_eq!(CLEAR.a, 0.0);
    }

    #[test]
    fn drive_matches_the_expression() {
        let drive = colors_drive(0.2, 1.0 / 60.0, 1.0);
        let expected_perm = 1.0 + ((0.2_f32 * 5.0 + 0.001).min(0.5) / 60.0);
        assert!((drive.perm - expected_perm).abs() < 1e-5);
        assert!((drive.in1_x - (expected_perm + 10.0)).abs() < 1e-5);
        assert_eq!(drive.in1_y, 36.0);
    }

    #[test]
    fn drive_caps_the_perm_step_and_sanitises() {
        let capped = colors_drive(10.0, 10.0, 0.0);
        assert_eq!(capped.perm, MAX_PERM_STEP * 10.0);
        assert_eq!(colors_drive(f32::NAN, f32::NAN, f32::NAN).in1_y, 36.0);
    }

    #[test]
    fn wgsl_parses_and_keeps_the_palette() {
        use crate::point10::scenes::plane9_common::PLANE9_UNIFORMS_WGSL;
        let source = format!("{}\n{}", PLANE9_UNIFORMS_WGSL, colors_wgsl());
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("Colors WGSL failed to parse: {error}"));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("Colors WGSL validates");
        assert!(colors_wgsl().contains("vor.pos1.xy * 0.05"));
        assert!(colors_wgsl().contains("* 0.3"));
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = Colors::new();
        let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        ctx.resize(64, 64);
        scene.init(&mut ctx);
        let mut bridge = AudioBridge::new();
        let mut payload = BandPayload::silent(44_100.0);
        payload.lanes = [0.6; 12];
        payload.beat = 0.8;
        let target = TestTarget::new(&device, 64, 64);
        let mut encoder = device.create_command_encoder(&Default::default());
        for _ in 0..2 {
            bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
            scene.update(1.0 / 60.0, &bridge);
            scene.render(&mut encoder, &target.view);
        }
        queue.submit(Some(encoder.finish()));
        assert!(any_lit(&target.read(&device, &queue)));
    }
}
