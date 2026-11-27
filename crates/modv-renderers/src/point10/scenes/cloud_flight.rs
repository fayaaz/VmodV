//! Plane9 "Cloud Flight" — Rust/wgpu port of `Nature/Cloud Flight.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Nature/Cloud Flight.xml` by Joakim
//! Dahl, CC BY-NC-SA 3.0. Based on Inigo Quilez's Shadertoy raymarch
//! (`XslGRr`).
//!
//! # The graph
//!
//! ```text
//! Shader1 (100-step cloud raymarch) -> RenderToTexture1 (RGBA8)
//!   -> Shader2 (texture²) -> RenderRect2 -> Screen1
//! ```
//!
//! # Ported exactly
//!
//! * `map(p) = clamp(0.2 - p.y + 3*_fbmfast(p*gIn2.x - vec3(1,0.1,0)*gIn1.z,
//!   3), 0, 1)` with the warm/`0.7` colour mix;
//! * the 100-iteration front-to-back march, `step = max(0.1, 0.03*t)`, the
//!   `0.3` sun offset, the `dif`/`brdf` shading and the `0.001 + sum.w`
//!   normalisation;
//! * the camera `ro`/`ta`/`rd` and the sky/sun mix;
//! * the expression `perm += (band(-1,3,0,0.05)*3+0.01)*dt`,
//!   `in1.x = 1+noise2(t*0.5,0)*0.5`, `in1.y = -1.75+(noise2(t,1)*0.5+0.5)`,
//!   `in1.z = t*10`, `in2.x = 0.25`;
//! * `Width/Height = 2` (25%) with the port's `RT_SCALE 2.0`, i.e. half
//!   resolution.
//!
//! The XML vertex flips `iTexCoord.y`; this port's `@builtin(position)` uv is
//! already top-origin, so the flip is implicit.
//!
//! The reference JS (`P9Overrides.js`) slows this preset's drift to
//! `in1.z = t*1.5`, `noise2(t*0.15, …)`, `noise2(t*0.2, …)` and a
//! `band*1 + 0.005` `perm` step because its analytic noise loses coherence at
//! the vendor's large coordinates. This port uses the vendor XML values (and
//! its own analytic noise); matching the reference's tuning is a future
//! fidelity decision, not a requirement of this port. The half-resolution RTT
//! override (`RT_SCALE 2.0`) is applied.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::plane9_common::{
    Plane9Pass, Plane9TextureBinding, Plane9Uniforms, create_colour_target, plane9_sampler,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::random::value_noise2;
use crate::point10::{
    Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, lens_value, plane9_scene_props,
};

/// `Screen1` camera position.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, -2.0];
/// `Screen1` camera look direction.
pub const CAMERA_LOOK: [f32; 3] = [0.0, 0.0, 1.0];
/// `Clear1` colour (opaque black, the screen).
pub const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
/// `RenderToTexture1` format (Format 1 = RGBA8).
pub const RTT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// `RenderToTexture1` `Width`/`Height` mode (2 = 25%).
pub const RTT_MODE: u32 = 2;
/// The port's `RT_SCALE` for this preset (2.0 ⇒ effective half resolution).
pub const RT_SCALE: f32 = 2.0;
/// `RenderToTexture1` `CreateMipMaps`.
pub const RTT_MIPS: bool = false;
/// Raymarch iteration count.
pub const RAYMARCH_STEPS: u32 = 100;
/// FBM octaves in `map`.
pub const FBM_OCTAVES: u32 = 3;
/// The fixed `in2.x` noise scale.
pub const NOISE_SCALE: f32 = 0.25;

const PROPS: &[SceneProp] = &[];

/// The RTT size: `25% * RT_SCALE 2.0` (half resolution, min 2 px).
pub fn rtt_size(width: u32, height: u32) -> (u32, u32) {
    let scale = 0.25 * RT_SCALE;
    (
        ((width as f32 * scale).round() as u32).max(2),
        ((height as f32 * scale).round() as u32).max(2),
    )
}

fn march_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| format!("{P9_COMMON_WGSL}\n{MARCH}"))
}

const MARCH: &str = r#"
fn p9_cloud_map(p: vec3<f32>) -> vec4<f32> {
  var d = 0.2 - p.y;
  d = d + 3.0 * p9_fbm(
    p * u.in2.x - vec3<f32>(1.0, 0.1, 0.0) * u.in1.z,
    3u,
  );
  d = clamp(d, 0.0, 1.0);
  var res = vec4<f32>(d);
  res = vec4<f32>(mix(1.15 * vec3<f32>(1.0, 0.95, 0.8), vec3<f32>(0.7), vec3<f32>(res.x)), d);
  return res;
}

fn p9_cloud_raymarch(ro: vec3<f32>, rd: vec3<f32>) -> vec4<f32> {
  var sum = vec4<f32>(0.0);
  var t = 0.0;
  for (var i = 0; i < 100; i = i + 1) {
    let pos = ro + t * rd;
    var col = p9_cloud_map(pos);
    let dif = clamp(
      (col.w - p9_cloud_map(pos + 0.3 * vec3<f32>(-1.0, 0.0, 0.0)).w) / 0.6,
      0.0,
      1.0,
    );
    let brdf = vec3<f32>(0.65, 0.68, 0.7) * 1.35
      + 0.45 * vec3<f32>(0.7, 0.5, 0.3) * dif;
    col = vec4<f32>(col.rgb * brdf, col.a);
    col = vec4<f32>(col.rgb * (col.a * 0.35), col.a * 0.35);
    sum = sum + col * (1.0 - sum.a);
    t = t + max(0.1, 0.03 * t);
  }
  sum = vec4<f32>(sum.xyz / (0.001 + sum.w), sum.w);
  return clamp(sum, vec4<f32>(0.0), vec4<f32>(1.0));
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  var p = -1.0 + 2.0 * tex;
  p.x = p.x * (u.resolution.x / u.resolution.y);
  // `in2.y` carries the shared Lens, applied after the aspect correction so
  // lens 1.0 restores the vendor framing exactly.
  p = p / u.in2.y;
  let mo = u.in1.xy;

  let ro = 4.0 * normalize(vec3<f32>(
    cos(2.75 - 3.0 * mo.x),
    0.7 + (mo.y + 1.0),
    sin(2.75 - 3.0 * mo.x),
  ));
  let ta = vec3<f32>(0.0, 1.0, 0.0);
  let ww = normalize(ta - ro);
  let uu = normalize(cross(vec3<f32>(0.0, -1.0, 0.0), ww));
  let vv = normalize(cross(ww, uu));
  let rd = normalize(p.x * uu + p.y * vv + 1.5 * ww);

  let res = p9_cloud_raymarch(ro, rd * 2.0);
  let sun = clamp(dot(vec3<f32>(-1.0, 0.0, 0.0), rd), 0.0, 1.0);
  var col = vec3<f32>(0.6, 0.71, 0.75) - rd.y * 0.2 * vec3<f32>(1.0, 0.5, 1.0) + 0.15 * 0.5;
  col = col + 0.2 * vec3<f32>(1.0, 0.6, 0.1) * pow(sun, 8.0);
  col = col * 0.95;
  col = mix(col, res.xyz, vec3<f32>(res.w));
  col = col + 0.1 * vec3<f32>(1.0, 0.4, 0.2) * pow(sun, 3.0);
  return vec4<f32>(col, 1.0);
}
"#;

const PRESENT: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  var c = textureSampleLevel(g_texture1, g_sampler, tex, 0.0) * u.color1;
  c = c * c;
  return c;
}
"#;

struct Gpu {
    march: Plane9Pass,
    present: Plane9Pass,
    march_uniforms: Plane9Uniforms,
    present_uniforms: Plane9Uniforms,
    _rtt: wgpu::Texture,
    _rtt_view: wgpu::TextureView,
    _sampler: wgpu::Sampler,
}

/// `Plane9 Cloud Flight` scene.
pub struct CloudFlight {
    audio: P10Audio,
    perm: f32,
    elapsed: f32,
    lens: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    gpu: Option<Gpu>,
}

impl Default for CloudFlight {
    fn default() -> Self {
        Self::new()
    }
}

impl CloudFlight {
    /// Create the scene.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            perm: 1.0,
            elapsed: 0.0,
            lens: 1.0,
            size: (1, 1),
            target_format: crate::point10::HDR_FORMAT,
            device: None,
            queue: None,
            gpu: None,
        }
    }

    /// The current `perm`.
    pub fn perm(&self) -> f32 {
        self.perm
    }

    /// The RTT size for the current target.
    pub fn rtt(&self) -> (u32, u32) {
        rtt_size(self.size.0, self.size.1)
    }

    /// Bind the shared Lens prop (this preset declares no other `sceneProps`).
    pub fn apply_props(&mut self, mut props: impl FnMut(&str) -> Option<f32>) {
        self.lens = lens_value(props("lens"));
    }

    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        let (rtt_w, rtt_h) = self.rtt();
        let (rtt, rtt_view) = create_colour_target(
            &device,
            "point10-plane9-cloud-flight-rtt",
            rtt_w,
            rtt_h,
            RTT_FORMAT,
        );
        let sampler = plane9_sampler(&device, "point10-plane9-cloud-flight-sampler");
        let march = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-cloud-flight-march",
            march_wgsl(),
            None,
        );
        let present = Plane9Pass::new(
            &device,
            &queue,
            self.target_format,
            "point10-plane9-cloud-flight-present",
            PRESENT,
            Some(Plane9TextureBinding::D2 {
                view: &rtt_view,
                sampler: &sampler,
            }),
        );
        let mut march_uniforms = Plane9Uniforms::new(rtt_w, rtt_h);
        march_uniforms.in2 = [NOISE_SCALE, self.lens, 0.0, 0.0];
        let present_uniforms = Plane9Uniforms::new(self.size.0, self.size.1);
        march.upload(&march_uniforms);
        present.upload(&present_uniforms);
        self.gpu = Some(Gpu {
            march,
            present,
            march_uniforms,
            present_uniforms,
            _rtt: rtt,
            _rtt_view: rtt_view,
            _sampler: sampler,
        });
    }
}

impl Point10Scene for CloudFlight {
    fn name(&self) -> &'static str {
        "Plane9 Cloud Flight"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.size = (ctx.width.max(1), ctx.height.max(1));
        self.rebuild();
    }

    fn resize(&mut self, width: u32, height: u32) {
        let size = (width.max(1), height.max(1));
        if self.size == size {
            return;
        }
        self.size = size;
        self.rebuild();
    }

    fn update(&mut self, dt: f32, audio: &AudioBridge) {
        let dt = frame_dt(dt);
        self.audio.update(dt, audio);
        self.elapsed += dt;
        let band = self.audio.band(-1, 3, 0, 0.05);
        self.perm += (band * 3.0 + 0.01) * dt;
        let t = self.perm;
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        gpu.march_uniforms.time = self.elapsed;
        gpu.march_uniforms.in1 = [
            1.0 + (value_noise2(t * 0.5, 0.0) * 0.5),
            -1.75 + (value_noise2(t, 1.0) * 0.5 + 0.5),
            t * 10.0,
            0.0,
        ];
        gpu.march_uniforms.in2 = [NOISE_SCALE, self.lens, 0.0, 0.0];
        gpu.march.upload(&gpu.march_uniforms);
        gpu.present_uniforms.time = self.elapsed;
        gpu.present.upload(&gpu.present_uniforms);
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        gpu.march.draw(
            encoder,
            &gpu._rtt_view,
            CLEAR,
            "point10-plane9-cloud-flight-march",
        );
        gpu.present.draw(
            encoder,
            target,
            CLEAR,
            "point10-plane9-cloud-flight-present",
        );
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
        self.elapsed = 0.0;
        self.perm = 1.0;
    }

    fn props(&self) -> Vec<SceneProp> {
        plane9_scene_props(PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Cloud Flight", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::scenes::plane9_common::{PLANE9_TEXTURE_WGSL, PLANE9_UNIFORMS_WGSL};
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn camera_clear_and_rtt_match_the_preset() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, -2.0]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, 1.0]);
        assert_eq!(CLEAR.a, 1.0);
        assert_eq!(RTT_FORMAT, wgpu::TextureFormat::Rgba8Unorm);
        assert_eq!(RTT_MODE, 2);
        assert_eq!(RT_SCALE, 2.0);
        const { assert!(!RTT_MIPS) };
        assert_eq!(RAYMARCH_STEPS, 100);
        assert_eq!(FBM_OCTAVES, 3);
        assert_eq!(NOISE_SCALE, 0.25);
        // mode 2 (25%) * RT_SCALE 2.0 = half resolution.
        assert_eq!(rtt_size(64, 64), (32, 32));
        assert_eq!(rtt_size(1920, 1080), (960, 540));
    }

    #[test]
    fn shader_keeps_the_raymarch() {
        let march = march_wgsl();
        for needle in [
            "var d = 0.2 - p.y;",
            "3.0 * p9_fbm(",
            "0.3 * vec3<f32>(-1.0, 0.0, 0.0)",
            "vec3<f32>(0.65, 0.68, 0.7) * 1.35",
            "t = t + max(0.1, 0.03 * t);",
            "sum.xyz / (0.001 + sum.w)",
            "4.0 * normalize(vec3<f32>(",
            "let rd = normalize(p.x * uu + p.y * vv + 1.5 * ww);",
            "pow(sun, 8.0)",
        ] {
            assert!(march.contains(needle), "lost {needle}");
        }
        for needle in [
            "textureSampleLevel(g_texture1, g_sampler, tex, 0.0) * u.color1",
            "c = c * c",
        ] {
            assert!(PRESENT.contains(needle), "lost {needle}");
        }
    }

    #[test]
    fn shaders_validate() {
        let sources = [
            format!("{PLANE9_UNIFORMS_WGSL}\n{}", march_wgsl()),
            format!("{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{PRESENT}"),
        ];
        for wgsl in sources {
            let module = naga::front::wgsl::parse_str(&wgsl)
                .unwrap_or_else(|error| panic!("Cloud Flight WGSL failed to parse: {error}"));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("Cloud Flight WGSL validates");
        }
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = CloudFlight::new();
        let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        ctx.resize(64, 64);
        scene.init(&mut ctx);
        let mut bridge = AudioBridge::new();
        let mut payload = BandPayload::silent(44_100.0);
        payload.lanes = [0.5; 12];
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
