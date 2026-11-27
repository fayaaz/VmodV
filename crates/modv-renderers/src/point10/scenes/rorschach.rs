//! Plane9 "Rorschach" — Rust/wgpu port of `Abstract/Rorschach.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Abstract/Rorschach.xml` by Joakim
//! Dahl, CC BY-NC-SA 4.0.
//!
//! # The graph
//!
//! ```text
//! Shader1 (mirrored FBM threshold) -> RenderRect1 -> RenderToTexture1 (RGBA8)
//!   -> Shader2 (texture × diffuse) -> Screen1
//! ```
//!
//! # Ported exactly
//!
//! * `v = _fbm(vec3(abs(p.x), p.y, gIn1.x), 7) * 0.5 + 0.5`, the radial
//!   `smoothstep(1.0, 0.5, length(p))` mask and the `1 - step(0.6, v)`
//!   threshold;
//! * the grayscale `vec4(v, v, v, 1)` output;
//! * the expression `permrand += (beat(0.01) + 0.01) * dt`, `in1.x =
//!   permrand`;
//! * `RenderToTexture1` at full size, RGBA8, no mipmaps.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::plane9_common::{
    Plane9Pass, Plane9TextureBinding, Plane9Uniforms, create_colour_target, plane9_sampler,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::{
    HDR_FORMAT, Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, scene_props_with_levers,
};

/// `Screen1` camera position.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, -2.0];
/// `Screen1` camera look direction.
pub const CAMERA_LOOK: [f32; 3] = [0.0, 0.0, 1.0];
/// `Clear1`/`Clear2` colour (opaque black).
pub const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
/// `RenderToTexture1` format (Format 1 = RGBA8).
pub const RTT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// `Shader1` FBM octaves.
pub const FBM_OCTAVES: u32 = 7;
/// `Shader1` threshold.
pub const THRESHOLD: f32 = 0.6;

const RORSCHACH_PROPS: &[SceneProp] = &[];

struct RorschachGpu {
    pass1: Plane9Pass,
    pass2: Plane9Pass,
    _rtt: wgpu::Texture,
    rtt_view: wgpu::TextureView,
    _sampler: wgpu::Sampler,
}

/// `Plane9 Rorschach` scene.
pub struct Rorschach {
    audio: P10Audio,
    permrand: f32,
    elapsed: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    uniforms: Plane9Uniforms,
    gpu: Option<RorschachGpu>,
}

impl Default for Rorschach {
    fn default() -> Self {
        Self::new()
    }
}

impl Rorschach {
    /// Create the scene.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            permrand: 1.0,
            elapsed: 0.0,
            size: (1, 1),
            target_format: HDR_FORMAT,
            device: None,
            queue: None,
            uniforms: Plane9Uniforms::new(1, 1),
            gpu: None,
        }
    }

    /// The current `permrand`.
    pub fn permrand(&self) -> f32 {
        self.permrand
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
        let (rtt, rtt_view) = create_colour_target(
            &device,
            "point10-plane9-rorschach-rtt",
            self.size.0,
            self.size.1,
            RTT_FORMAT,
        );
        let sampler = plane9_sampler(&device, "point10-plane9-rorschach-sampler");
        let pass1 = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-rorschach-pattern",
            rorschach_shader1_wgsl(),
            None,
        );
        let pass2 = Plane9Pass::new(
            &device,
            &queue,
            self.target_format,
            "point10-plane9-rorschach-present",
            RORSCHACH_SHADER2,
            Some(Plane9TextureBinding::D2 {
                view: &rtt_view,
                sampler: &sampler,
            }),
        );
        self.gpu = Some(RorschachGpu {
            pass1,
            pass2,
            _rtt: rtt,
            rtt_view,
            _sampler: sampler,
        });
        self.upload_uniforms();
    }

    fn upload_uniforms(&self) {
        if let Some(gpu) = &self.gpu {
            gpu.pass1.upload(&self.uniforms);
            gpu.pass2.upload(&self.uniforms);
        }
    }
}

fn rorschach_shader1_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| format!("{P9_COMMON_WGSL}\n{RORSCHACH_SHADER1}"))
}

const RORSCHACH_SHADER1: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let p = tex * vec2<f32>(2.0) - vec2<f32>(1.0);
  var v = p9_fbm(vec3<f32>(abs(p.x), p.y, u.in1.x), 7u) * 0.5 + 0.5;
  v = v * p9_smoothstep(1.0, 0.5, length(p));
  v = 1.0 - step(0.6, v);
  return vec4<f32>(v, v, v, 1.0);
}
"#;

const RORSCHACH_SHADER2: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  return textureSampleLevel(g_texture1, g_sampler, tex, 0.0) * u.color1;
}
"#;

impl Point10Scene for Rorschach {
    fn name(&self) -> &'static str {
        "Plane9 Rorschach"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.size = (ctx.width.max(1), ctx.height.max(1));
        self.uniforms = Plane9Uniforms::new(self.size.0, self.size.1);
        self.rebuild();
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
        let beat = self.audio.beat(0.01);
        let beat = if beat.is_finite() { beat.max(0.0) } else { 0.0 };
        self.permrand += (beat + 0.01) * dt;
        self.uniforms.time = self.elapsed;
        self.uniforms.in1 = [self.permrand, 0.0, 0.0, 0.0];
        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        gpu.pass1.draw(
            encoder,
            &gpu.rtt_view,
            CLEAR,
            "point10-plane9-rorschach-pattern",
        );
        gpu.pass2
            .draw(encoder, target, CLEAR, "point10-plane9-rorschach-present");
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(RORSCHACH_PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Rorschach", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn camera_clear_and_rtt_match_the_preset() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, -2.0]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, 1.0]);
        assert_eq!(CLEAR.a, 1.0);
        assert_eq!(RTT_FORMAT, wgpu::TextureFormat::Rgba8Unorm);
        assert_eq!(FBM_OCTAVES, 7);
        assert_eq!(THRESHOLD, 0.6);
    }

    #[test]
    fn shader_keeps_the_mirror_and_threshold() {
        let wgsl = rorschach_shader1_wgsl();
        for needle in [
            "vec3<f32>(abs(p.x), p.y, u.in1.x)",
            "p9_fbm(vec3<f32>(abs(p.x), p.y, u.in1.x), 7u)",
            "1.0 - step(0.6, v)",
        ] {
            assert!(wgsl.contains(needle), "lost {needle}");
        }
    }

    #[test]
    fn both_shaders_validate() {
        use crate::point10::scenes::plane9_common::{PLANE9_TEXTURE_WGSL, PLANE9_UNIFORMS_WGSL};
        let sources = [
            format!("{PLANE9_UNIFORMS_WGSL}\n{}", rorschach_shader1_wgsl()),
            format!("{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{RORSCHACH_SHADER2}"),
        ];
        for wgsl in sources {
            let module = naga::front::wgsl::parse_str(&wgsl)
                .unwrap_or_else(|error| panic!("Rorschach WGSL failed to parse: {error}"));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("Rorschach WGSL validates");
        }
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = Rorschach::new();
        let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        ctx.resize(64, 64);
        scene.init(&mut ctx);
        let mut bridge = AudioBridge::new();
        let mut payload = BandPayload::silent(44_100.0);
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
