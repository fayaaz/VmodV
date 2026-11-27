//! Plane9 "Gradient" — Rust/wgpu port of `Abstract/Gradient.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Abstract/Gradient.xml` by Joakim
//! Dahl, CC BY-NC-SA 3.0. A soft plasma: three 3D-noise values at different
//! UV scales drive two `_hsv2rgb` colours, mixed by a third noise field, with
//! a radial vignette.
//!
//! # Ported exactly
//!
//! * colours 1/2: `_noise(vec3(tex * 0.6|0.7, gIn3.x * 0.1)) * 0.5 + 0.5`
//!   (the second with the `123.0` seed) feeding
//!   `_hsv2rgb(fract(v * 0.4|0.3 + gIn1.x|gIn2.x), gIn1.y|gIn2.y,
//!   gIn1.z|gIn2.z)`;
//! * the mix noise `_noise(vec3(tex * 0.5, 234.0 + gIn3.x * 0.1))`;
//! * the vignette `0.5 + 0.5 * smoothstep(2.0, 0.5, dot(p, p))`;
//! * the six `noise2` expression lanes at scales 0.01/0.1/0.12/0.012/0.075/
//!   0.05 with seeds 0..5, the `intensity` boost and `in3.x = t`.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::plane9_common::{Plane9Pass, Plane9Uniforms};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::random::value_noise2;
use crate::point10::{
    HDR_FORMAT, Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, scene_props_with_levers,
};

/// `Screen1` camera position.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, -2.0];
/// `Screen1` camera look direction.
pub const CAMERA_LOOK: [f32; 3] = [0.0, 0.0, 1.0];
/// `Clear1` colour.
pub const CLEAR: wgpu::Color = wgpu::Color::TRANSPARENT;
/// `perm` cap is unbounded in this preset (`band * 50 + 0.01`).
pub const PERM_GAIN: f32 = 50.0;
/// `permrand` seed.
pub const PERM_RAND: f32 = 1.0;
/// `intensity = band * 20`.
pub const INTENSITY_GAIN: f32 = 20.0;

const GRADIENT_PROPS: &[SceneProp] = &[];

/// One frame of the Gradient expression.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientDrive {
    /// `perm` for the next frame.
    pub perm: f32,
    /// `gIn1`.
    pub in1: [f32; 4],
    /// `gIn2`.
    pub in2: [f32; 4],
    /// `gIn3`.
    pub in3: [f32; 4],
}

/// Pure port of the Gradient expression.
///
/// `speed = band(-1, 2, 0, 0.02)` and `intensity = band(-1, 2, 1, 0.01)`.
pub fn gradient_drive(speed: f32, raw_intensity: f32, dt: f32, perm: f32) -> GradientDrive {
    let speed = if speed.is_finite() {
        speed.max(0.0)
    } else {
        0.0
    };
    let raw_intensity = if raw_intensity.is_finite() {
        raw_intensity.max(0.0)
    } else {
        0.0
    };
    let step = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    let perm = if perm.is_finite() { perm } else { 1.0 };
    let next = perm + (speed * PERM_GAIN + 0.01) * step;
    let t = next + PERM_RAND * 100.0;
    let intensity = raw_intensity * INTENSITY_GAIN;
    let boost = |seed: f32, scale: f32| {
        (min1((value_noise2(t * scale, seed) * 0.5 + 0.5) * 0.75 + 0.25) + intensity).min(1.0)
    };
    GradientDrive {
        perm: next,
        in1: [
            value_noise2(t * 0.01, 0.0) * 0.5 + 0.5,
            boost(1.0, 0.1),
            value_noise2(t * 0.12, 2.0) * 0.5 + 0.5,
            0.0,
        ],
        in2: [
            value_noise2(t * 0.012, 3.0) * 0.5 + 0.5,
            boost(4.0, 0.075),
            value_noise2(t * 0.05, 5.0) * 0.5 + 0.5,
            0.0,
        ],
        in3: [t, 0.0, 0.0, 0.0],
    }
}

fn min1(value: f32) -> f32 {
    value.min(1.0)
}

/// `Plane9 Gradient` scene.
pub struct Gradient {
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

impl Default for Gradient {
    fn default() -> Self {
        Self::new()
    }
}

impl Gradient {
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

    /// The uniform block last uploaded.
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
            "point10-plane9-gradient",
            gradient_wgsl(),
            None,
        ));
    }
}

fn gradient_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| format!("{P9_COMMON_WGSL}\n{GRADIENT_FRAGMENT}"))
}

const GRADIENT_FRAGMENT: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let p = -1.0 + 2.0 * tex;

  let v1 = p9_noise(vec3<f32>(tex * 0.6, u.in3.x * 0.1)) * 0.5 + 0.5;
  let c1 = p9_hsv2rgb(fract(v1 * 0.4 + u.in1.x), u.in1.y, u.in1.z);

  let v2 = p9_noise(vec3<f32>(tex * 0.7, 123.0 + u.in3.x * 0.1)) * 0.5 + 0.5;
  let c2 = p9_hsv2rgb(fract(v2 * 0.3 + u.in2.x), u.in2.y, u.in2.z);

  var c = mix(c1, c2, p9_noise(vec3<f32>(tex * 0.5, 234.0 + u.in3.x * 0.1)) * 0.5 + 0.5);
  c = c * (0.5 + 0.5 * p9_smoothstep(2.0, 0.5, dot(p, p)));
  return vec4<f32>(c, 1.0);
}
"#;

impl Point10Scene for Gradient {
    fn name(&self) -> &'static str {
        "Plane9 Gradient"
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
        let drive = gradient_drive(
            self.audio.band(-1, 2, 0, 0.02),
            self.audio.band(-1, 2, 1, 0.01),
            dt,
            self.perm,
        );
        self.perm = drive.perm;
        self.uniforms.time = self.elapsed;
        self.uniforms.in1 = drive.in1;
        self.uniforms.in2 = drive.in2;
        self.uniforms.in3 = drive.in3;
        if let Some(pass) = &self.pass {
            pass.upload(&self.uniforms);
        }
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        if let Some(pass) = &self.pass {
            pass.draw(encoder, target, CLEAR, "point10-plane9-gradient");
        }
    }

    fn dispose(&mut self) {
        self.pass = None;
        self.device = None;
        self.queue = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(GRADIENT_PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Gradient", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn camera_and_clear_match_the_preset() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, -2.0]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, 1.0]);
        assert_eq!(CLEAR.a, 0.0);
    }

    #[test]
    fn drive_matches_the_six_noise_lanes() {
        let drive = gradient_drive(0.0, 0.0, 0.0, 1.0);
        let t = 1.0 + 100.0;
        assert_eq!(drive.in3[0], t);
        assert!((drive.in1[0] - (value_noise2(t * 0.01, 0.0) * 0.5 + 0.5)).abs() < 1e-6);
        assert!((drive.in1[2] - (value_noise2(t * 0.12, 2.0) * 0.5 + 0.5)).abs() < 1e-6);
        assert!((drive.in2[0] - (value_noise2(t * 0.012, 3.0) * 0.5 + 0.5)).abs() < 1e-6);
        assert!((drive.in2[2] - (value_noise2(t * 0.05, 5.0) * 0.5 + 0.5)).abs() < 1e-6);
    }

    #[test]
    fn intensity_boosts_the_saturation_lanes_and_clamps() {
        let drive = gradient_drive(0.0, 0.5, 0.0, 1.0);
        assert!(drive.in1[1] <= 1.0);
        assert!(drive.in2[1] <= 1.0);
    }

    #[test]
    fn wgsl_parses_and_keeps_the_constants() {
        use crate::point10::scenes::plane9_common::PLANE9_UNIFORMS_WGSL;
        let source = format!("{}\n{}", PLANE9_UNIFORMS_WGSL, gradient_wgsl());
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("Gradient WGSL failed to parse: {error}"));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("Gradient WGSL validates");
        for needle in ["123.0", "234.0", "* 0.6", "* 0.7", "* 0.5"] {
            assert!(gradient_wgsl().contains(needle), "lost {needle}");
        }
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = Gradient::new();
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
