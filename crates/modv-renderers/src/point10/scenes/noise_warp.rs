//! Plane9 "Noise Warp" — Rust/wgpu port of `Abstract/Noise Warp.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Abstract/Noise Warp.xml` by Joakim
//! Dahl, CC BY-NC-SA 4.0, based on Inigo Quilez's domain-warping article.
//!
//! # The graph
//!
//! ```text
//! Shader1 (2D domain warp) -> RenderRect1 -> RenderToTexture1 (RGBA8, full size)
//!   -> Shader2 (emboss + brightness/saturation/contrast + vignette) -> Screen1
//! ```
//!
//! `RenderToTexture1` also has a dead `_fbm` shader in the XML; the `Effect`
//! connection from `Shader1` makes Plane9 render the incoming chain instead
//! (`P9Scene.js:606-616`), so the port keeps the live chain only.
//!
//! # Ported exactly
//!
//! * the five `_fbmfast` calls at 6/4/5 octaves with the exact offsets
//!   `(3.2,2.2)`, `(1.3,7.4)`, `(5.2,5.3)` and the `4.0`/`8.0` warp gains;
//! * the fixed `col1 = (0.953, 0.502, 0.149)`, `col2 = (0.702, 0.773, 0.839)`
//!   and the two colour-port mixes;
//! * `Shader2`'s one-texel emboss, `_brightnessSaturationContrast(0.6, 1.5,
//!   1.5)` and `smoothstep(2.0, 0.5, length(p))` vignette;
//! * the expression `perm += (band(-1,1,1,0.03) + 0.01) * dt`.

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
/// `Clear1`/`Clear2` colour.
pub const CLEAR: wgpu::Color = wgpu::Color::TRANSPARENT;
/// `RenderToTexture1` format (Format 1 = RGBA8).
pub const RTT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// `Shader1`'s `Color1` port.
pub const SHADER1_COLOR1: [f32; 4] = [0.424, 0.824, 0.922, 1.0];
/// `Shader1`'s `Color2` port.
pub const SHADER1_COLOR2: [f32; 4] = [0.388, 0.235, 0.055, 1.0];

const NOISE_WARP_PROPS: &[SceneProp] = &[];

struct NoiseWarpGpu {
    pass1: Plane9Pass,
    pass2: Plane9Pass,
    _rtt: wgpu::Texture,
    rtt_view: wgpu::TextureView,
    _sampler: wgpu::Sampler,
}

/// `Plane9 Noise Warp` scene.
pub struct NoiseWarp {
    audio: P10Audio,
    perm: f32,
    elapsed: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    uniforms: Plane9Uniforms,
    gpu: Option<NoiseWarpGpu>,
}

impl Default for NoiseWarp {
    fn default() -> Self {
        Self::new()
    }
}

impl NoiseWarp {
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
            gpu: None,
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
        let (rtt, rtt_view) = create_colour_target(
            &device,
            "point10-plane9-noise-warp-rtt",
            self.size.0,
            self.size.1,
            RTT_FORMAT,
        );
        let sampler = plane9_sampler(&device, "point10-plane9-noise-warp-sampler");
        let pass1 = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-noise-warp-warp",
            noise_warp_shader1_wgsl(),
            None,
        );
        let pass2 = Plane9Pass::new(
            &device,
            &queue,
            self.target_format,
            "point10-plane9-noise-warp-emboss",
            noise_warp_shader2_wgsl(),
            Some(Plane9TextureBinding::D2 {
                view: &rtt_view,
                sampler: &sampler,
            }),
        );
        self.gpu = Some(NoiseWarpGpu {
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

fn noise_warp_shader1_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| format!("{P9_COMMON_WGSL}\n{NOISE_WARP_SHADER1}"))
}

fn noise_warp_shader2_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| {
        format!(
            "{P9_COMMON_WGSL}\n{}\n{NOISE_WARP_SHADER2}",
            crate::point10::scenes::plane9_common::PLANE9_BATCH_WGSL
        )
    })
}

const NOISE_WARP_SHADER1: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  var p = tex * vec2<f32>(2.0) - vec2<f32>(1.0);
  p = p * 2.0;
  let t = u.in1.x;

  var q: vec2<f32>;
  q.x = p9_fbm(vec3<f32>(p + vec2<f32>(0.0, 0.0), t), 6u);
  q.y = p9_fbm(vec3<f32>(p + vec2<f32>(3.2, 2.2), t), 6u);

  var r: vec2<f32>;
  r.x = p9_fbm(vec3<f32>(p + 4.0 * q + vec2<f32>(1.3, 7.4), t), 4u);
  r.y = p9_fbm(vec3<f32>(p + 4.0 * q + vec2<f32>(5.2, 5.3), t), 4u);

  let v = p9_fbm(vec3<f32>(p + 8.0 * r, t), 5u) + 0.5;
  let col1 = vec3<f32>(0.953, 0.502, 0.149);
  let col2 = vec3<f32>(0.702, 0.773, 0.839);
  let col3 = u.color1.rgb;
  let col4 = u.color2.rgb;
  var c = mix(col1, col2, v);
  c = mix(c, col3, length(q) * 3.0);
  c = mix(c, col4, r.y + 0.5);
  return vec4<f32>(p9_saturate3(c), 1.0);
}
"#;

const NOISE_WARP_SHADER2: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let p = tex * vec2<f32>(2.0) - vec2<f32>(1.0);
  let texel = vec2<f32>(1.0) / u.resolution;
  var c = textureSampleLevel(g_texture1, g_sampler, tex, 0.0);
  let embos = textureSampleLevel(g_texture1, g_sampler, tex - texel, 0.0).rgb
    - textureSampleLevel(g_texture1, g_sampler, tex + texel, 0.0).rgb;
  c = vec4<f32>(c.rgb + vec3<f32>((embos.r + embos.g + embos.b) / 3.0), c.a);
  var rgb = p9_saturate3(p9_brightness_saturation_contrast(c.rgb, 0.6, 1.5, 1.5));
  rgb = rgb * p9_smoothstep(2.0, 0.5, length(p));
  return vec4<f32>(rgb, c.a);
}
"#;

impl Point10Scene for NoiseWarp {
    fn name(&self) -> &'static str {
        "Plane9 Noise Warp"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.size = (ctx.width.max(1), ctx.height.max(1));
        self.uniforms = Plane9Uniforms::new(self.size.0, self.size.1);
        self.uniforms.color1 = SHADER1_COLOR1;
        self.uniforms.color2 = SHADER1_COLOR2;
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
        let band = self.audio.band(-1, 1, 1, 0.03);
        let band = if band.is_finite() { band.max(0.0) } else { 0.0 };
        self.perm += (band + 0.01) * dt;
        self.uniforms.time = self.elapsed;
        self.uniforms.in1 = [self.perm + 10.0, 0.0, 0.0, 0.0];
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
            "point10-plane9-noise-warp-warp",
        );
        gpu.pass2
            .draw(encoder, target, CLEAR, "point10-plane9-noise-warp-emboss");
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(NOISE_WARP_PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Noise Warp", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn camera_clear_and_colours_match_the_preset() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, -2.0]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, 1.0]);
        assert_eq!(CLEAR.a, 0.0);
        assert_eq!(SHADER1_COLOR1, [0.424, 0.824, 0.922, 1.0]);
        assert_eq!(SHADER1_COLOR2, [0.388, 0.235, 0.055, 1.0]);
        assert_eq!(RTT_FORMAT, wgpu::TextureFormat::Rgba8Unorm);
    }

    #[test]
    fn shader1_keeps_the_five_octave_warp() {
        let wgsl = noise_warp_shader1_wgsl();
        for needle in [
            "p9_fbm(vec3<f32>(p + vec2<f32>(3.2, 2.2), t), 6u)",
            "p9_fbm(vec3<f32>(p + 4.0 * q + vec2<f32>(5.2, 5.3), t), 4u)",
            "p9_fbm(vec3<f32>(p + 8.0 * r, t), 5u)",
            "vec3<f32>(0.953, 0.502, 0.149)",
            "vec3<f32>(0.702, 0.773, 0.839)",
            "mix(c, col4, r.y + 0.5)",
        ] {
            assert!(wgsl.contains(needle), "lost {needle}");
        }
    }

    #[test]
    fn both_shaders_validate() {
        use crate::point10::scenes::plane9_common::{PLANE9_TEXTURE_WGSL, PLANE9_UNIFORMS_WGSL};
        let sources = [
            format!("{PLANE9_UNIFORMS_WGSL}\n{}", noise_warp_shader1_wgsl()),
            format!(
                "{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{}",
                noise_warp_shader2_wgsl()
            ),
        ];
        for wgsl in sources {
            let module = naga::front::wgsl::parse_str(&wgsl)
                .unwrap_or_else(|error| panic!("Noise Warp WGSL failed to parse: {error}"));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("Noise Warp WGSL validates");
        }
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = NoiseWarp::new();
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
