//! Plane9 "Tv Noise" — Rust/wgpu port of `Other/Tv Noise.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Other/Tv Noise.xml` by Joakim Dahl,
//! CC BY-NC-SA 3.0. White noise with a CRT-style corner roll-off:
//! `v = _rand(tex + gIn1.xy)` attenuated by
//! `0.1 + 0.9 * (1 - pow(smoothstep(0.9, 1.0, x² + y² - x²y²), 2.0))`.
//!
//! # Ported exactly
//!
//! * the `_rand(vec2)` hash (WGSL `p9_rand2`);
//! * the radial expression `(p.x² + p.y²) - (p.x² * p.y²)`;
//! * `in1.x = random()` / `in1.y = random()` per frame, replaced by a
//!   deterministic per-frame [`seeded_rand`] pair so the scene is reproducible
//!   (the JS uses `Math.random`, which a GPU test cannot pin).

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::plane9_common::{Plane9Pass, Plane9Uniforms};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::random::seeded_rand;
use crate::point10::{
    HDR_FORMAT, Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, scene_props_with_levers,
};

/// `Screen1` camera position.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, -2.0];
/// `Screen1` camera look direction.
pub const CAMERA_LOOK: [f32; 3] = [0.0, 0.0, 1.0];
/// `Clear1` colour (opaque black).
pub const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

const TV_NOISE_PROPS: &[SceneProp] = &[];

/// The two random offsets the expression pushes each frame.
pub fn tv_noise_offset(frame: u32) -> [f32; 2] {
    [
        seeded_rand(frame as i32, 0x7a11),
        seeded_rand(frame as i32, 0x1ce5),
    ]
}

/// `Plane9 Tv Noise` scene.
pub struct TvNoise {
    audio: P10Audio,
    frame: u32,
    elapsed: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    uniforms: Plane9Uniforms,
    pass: Option<Plane9Pass>,
}

impl Default for TvNoise {
    fn default() -> Self {
        Self::new()
    }
}

impl TvNoise {
    /// Create the scene.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            frame: 0,
            elapsed: 0.0,
            size: (1, 1),
            target_format: HDR_FORMAT,
            device: None,
            queue: None,
            uniforms: Plane9Uniforms::new(1, 1),
            pass: None,
        }
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
            "point10-plane9-tv-noise",
            tv_noise_wgsl(),
            None,
        ));
    }
}

fn tv_noise_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| format!("{P9_COMMON_WGSL}\n{TV_NOISE_FRAGMENT}"))
}

const TV_NOISE_FRAGMENT: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let p = tex * 2.0 - vec2<f32>(1.0);
  var v = p9_rand2(tex + u.in1.xy);
  let radial = ((p.x * p.x) + (p.y * p.y)) - ((p.x * p.x) * (p.y * p.y));
  v = v * (0.1 + 0.9 * (1.0 - pow(p9_smoothstep(0.9, 1.0, radial), 2.0)));
  return vec4<f32>(vec3<f32>(v), 1.0);
}
"#;

impl Point10Scene for TvNoise {
    fn name(&self) -> &'static str {
        "Plane9 Tv Noise"
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
        let offset = tv_noise_offset(self.frame);
        self.frame = self.frame.wrapping_add(1);
        self.uniforms.time = self.elapsed;
        self.uniforms.in1 = [offset[0], offset[1], 0.0, 0.0];
        if let Some(pass) = &self.pass {
            pass.upload(&self.uniforms);
        }
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        if let Some(pass) = &self.pass {
            pass.draw(encoder, target, CLEAR, "point10-plane9-tv-noise");
        }
    }

    fn dispose(&mut self) {
        self.pass = None;
        self.device = None;
        self.queue = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(TV_NOISE_PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Tv Noise", "3d-scenes")
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
        assert_eq!(CLEAR.a, 1.0);
    }

    #[test]
    fn offsets_are_deterministic_and_change_per_frame() {
        let first = tv_noise_offset(0);
        assert_eq!(first, tv_noise_offset(0));
        assert_ne!(first, tv_noise_offset(1));
        assert!((0.0..1.0).contains(&first[0]));
        assert!((0.0..1.0).contains(&first[1]));
    }

    #[test]
    fn wgsl_parses_and_keeps_the_radial_mask() {
        use crate::point10::scenes::plane9_common::PLANE9_UNIFORMS_WGSL;
        let source = format!("{}\n{}", PLANE9_UNIFORMS_WGSL, tv_noise_wgsl());
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("Tv Noise WGSL failed to parse: {error}"));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("Tv Noise WGSL validates");
        assert!(tv_noise_wgsl().contains("p9_rand2(tex + u.in1.xy)"));
        assert!(tv_noise_wgsl().contains("(p.x * p.x) * (p.y * p.y)"));
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = TvNoise::new();
        let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        ctx.resize(64, 64);
        scene.init(&mut ctx);
        let mut bridge = AudioBridge::new();
        let payload = BandPayload::silent(44_100.0);
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
