//! Plane9 "Truchet Tentacles" — Rust/wgpu port of
//! `Fractals/Truchet Tentacles.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Fractals/Truchet Tentacles.xml` by
//! Joakim Dahl, CC BY-NC-SA 3.0, based on WAHa_06x36's Shadertoy
//! <https://www.shadertoy.com/view/ldfGWn>.
//!
//! # The graph
//!
//! ```text
//! StoreTexture1 (previous RTT1) ─────► Shader1.Texture1
//! Shader1 (64-step Truchet raymarch) -> RenderToTexture1 (RGBA8, mips)
//!   ├─► Shader2 (texture²) -> Screen1
//!   └─► StoreTexture1      (feedback for the next frame)
//! ```
//!
//! The feedback is one frame late (`P9Scene.js:598-616`: resolving an RTT
//! while it is mid-render returns the previous frame), so the port keeps two
//! targets: `history` (what Shader1 samples) and `rtt` (what Shader1 writes
//! this frame). A blit copies `rtt` into `history` after Shader2 presents.
//!
//! # Ported exactly
//!
//! * `Iterations 64`, `Thickness 0.1`, `SuperQuadPower 8`, `Fisheye 0.5`;
//! * the `rand(vec3)` hash, the three-axis `truchetcell` min, the eight
//!   orientation buckets and the `0.001` hit threshold;
//! * the `mat3` Y-rotation squared twice (GLSL columns preserved);
//! * the ray origin path and `col = (cos(ray_pos / 2) + 2) / 3`;
//! * the reflection lookup `yaw = 0.5 - atan(reflected.z, -reflected.x) / 2π`,
//!   `pitch = 0.5 - asin(reflected.y) / π`;
//! * the expression `perm += (beat(0.01) * 20 + 0.5) * dt`,
//!   `in1.x = permrand + perm`.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::plane9_common::{
    Plane9Pass, Plane9TextureBinding, Plane9Uniforms, create_colour_target, plane9_sampler,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::{
    HDR_FORMAT, Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, lens_value,
    plane9_scene_props,
};

/// `Screen1` camera position.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, -2.0];
/// `Screen1` camera look direction.
pub const CAMERA_LOOK: [f32; 3] = [0.0, 0.0, 1.0];
/// No `Clear` node: the RTT starts transparent black.
pub const CLEAR: wgpu::Color = wgpu::Color::TRANSPARENT;
/// `RenderToTexture1` format (Format 1 = RGBA8).
pub const RTT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// `#define Iterations 64.0`.
pub const ITERATIONS: u32 = 64;
/// `#define Thickness 0.1`.
pub const THICKNESS: f32 = 0.1;
/// `#define SuperQuadPower 8.0`.
pub const SUPER_QUAD_POWER: f32 = 8.0;
/// `#define Fisheye 0.5`.
pub const FISHEYE: f32 = 0.5;
/// Ray hit threshold.
pub const HIT_EPSILON: f32 = 0.001;
/// `StoreTexture1` `SwapDelay 0` (the stored frame is the previous one).
pub const SWAP_DELAY: u32 = 0;

const TRUCHET_PROPS: &[SceneProp] = &[];

struct TruchetGpu {
    pass1: Plane9Pass,
    pass2: Plane9Pass,
    blit: Plane9Pass,
    _rtt: wgpu::Texture,
    rtt_view: wgpu::TextureView,
    _history: wgpu::Texture,
    history_view: wgpu::TextureView,
    _sampler: wgpu::Sampler,
}

/// `Plane9 Truchet Tentacles` scene.
pub struct TruchetTentacles {
    audio: P10Audio,
    perm: f32,
    permrand: f32,
    elapsed: f32,
    lens: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    uniforms: Plane9Uniforms,
    gpu: Option<TruchetGpu>,
}

impl Default for TruchetTentacles {
    fn default() -> Self {
        Self::new()
    }
}

impl TruchetTentacles {
    /// Create the scene.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            perm: 1.0,
            permrand: 1.0,
            elapsed: 0.0,
            lens: 1.0,
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

    /// Bind the shared Lens prop (this preset declares no other `sceneProps`).
    pub fn apply_props(&mut self, mut props: impl FnMut(&str) -> Option<f32>) {
        self.lens = lens_value(props("lens"));
    }

    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        let (rtt, rtt_view) = create_colour_target(
            &device,
            "point10-plane9-truchet-rtt",
            self.size.0,
            self.size.1,
            RTT_FORMAT,
        );
        let (history, history_view) = create_colour_target(
            &device,
            "point10-plane9-truchet-history",
            self.size.0,
            self.size.1,
            RTT_FORMAT,
        );
        let sampler = plane9_sampler(&device, "point10-plane9-truchet-sampler");

        let pass1 = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-truchet-raymarch",
            truchet_shader1_wgsl(),
            Some(Plane9TextureBinding::D2 {
                view: &history_view,
                sampler: &sampler,
            }),
        );
        let pass2 = Plane9Pass::new(
            &device,
            &queue,
            self.target_format,
            "point10-plane9-truchet-present",
            TRUCHET_SHADER2,
            Some(Plane9TextureBinding::D2 {
                view: &rtt_view,
                sampler: &sampler,
            }),
        );
        let blit = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-truchet-store",
            PLANE9_COPY_FRAGMENT,
            Some(Plane9TextureBinding::D2 {
                view: &rtt_view,
                sampler: &sampler,
            }),
        );
        self.gpu = Some(TruchetGpu {
            pass1,
            pass2,
            blit,
            _rtt: rtt,
            rtt_view,
            _history: history,
            history_view,
            _sampler: sampler,
        });
        self.upload_uniforms();
    }

    fn upload_uniforms(&self) {
        if let Some(gpu) = &self.gpu {
            gpu.pass1.upload(&self.uniforms);
            gpu.pass2.upload(&self.uniforms);
            gpu.blit.upload(&self.uniforms);
        }
    }
}

fn truchet_shader1_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| format!("{P9_COMMON_WGSL}\n{TRUCHET_SHADER1}"))
}

const PLANE9_COPY_FRAGMENT: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  return textureSampleLevel(g_texture1, g_sampler, tex, 0.0);
}
"#;

const TRUCHET_SHADER1: &str = r#"
fn truchet_rand(r: vec3<f32>) -> f32 {
  return fract(sin(dot(r.xy, vec2<f32>(1.38984 * sin(r.z), 1.13233 * cos(r.z)))) * 653758.5453);
}

fn truchet_arc(pos: vec3<f32>) -> f32 {
  let r = length(pos.xy);
  return pow(
    pow(abs(r - 0.5), 8.0) + pow(abs(pos.z - 0.5), 8.0),
    1.0 / 8.0,
  ) - 0.1;
}

fn truchet_cell(pos: vec3<f32>) -> f32 {
  return min(
    min(
      truchet_arc(pos),
      truchet_arc(vec3<f32>(pos.z, 1.0 - pos.x, pos.y)),
    ),
    truchet_arc(vec3<f32>(1.0 - pos.y, 1.0 - pos.z, pos.x)),
  );
}

fn truchet_distfunc(pos: vec3<f32>) -> f32 {
  let cellpos = fract(pos);
  let gridpos = floor(pos);
  let rnd = truchet_rand(gridpos);
  if (rnd < 1.0 / 8.0) {
    return truchet_cell(vec3<f32>(cellpos.x, cellpos.y, cellpos.z));
  } else if (rnd < 2.0 / 8.0) {
    return truchet_cell(vec3<f32>(cellpos.x, 1.0 - cellpos.y, cellpos.z));
  } else if (rnd < 3.0 / 8.0) {
    return truchet_cell(vec3<f32>(1.0 - cellpos.x, cellpos.y, cellpos.z));
  } else if (rnd < 4.0 / 8.0) {
    return truchet_cell(vec3<f32>(1.0 - cellpos.x, 1.0 - cellpos.y, cellpos.z));
  } else if (rnd < 5.0 / 8.0) {
    return truchet_cell(vec3<f32>(cellpos.y, cellpos.x, cellpos.z));
  } else if (rnd < 6.0 / 8.0) {
    return truchet_cell(vec3<f32>(cellpos.y, 1.0 - cellpos.x, cellpos.z));
  } else if (rnd < 7.0 / 8.0) {
    return truchet_cell(vec3<f32>(1.0 - cellpos.y, cellpos.x, cellpos.z));
  }
  return truchet_cell(vec3<f32>(1.0 - cellpos.y, 1.0 - cellpos.x, cellpos.z));
}

fn truchet_gradient(pos: vec3<f32>) -> vec3<f32> {
  let eps = 0.0001;
  let mid = truchet_distfunc(pos);
  return vec3<f32>(
    truchet_distfunc(pos + vec3<f32>(eps, 0.0, 0.0)) - mid,
    truchet_distfunc(pos + vec3<f32>(0.0, eps, 0.0)) - mid,
    truchet_distfunc(pos + vec3<f32>(0.0, 0.0, eps)) - mid,
  );
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let coords = tex * vec2<f32>(2.0) - vec2<f32>(1.0);

  let a = u.in1.x / 3.0;
  let sa = sin(a);
  let ca = cos(a);
  let m0 = mat3x3<f32>(
    vec3<f32>(0.0, 1.0, 0.0),
    vec3<f32>(-sa, 0.0, ca),
    vec3<f32>(ca, 0.0, sa),
  );
  let m = m0 * m0 * m0 * m0;

  // `in2.x` carries the shared Lens; only the ray direction narrows, so the
  // `coords` reused by the vignette keep the vendor framing.
  let dir = normalize(vec3<f32>(
    1.4 * coords / u.in2.x,
    -1.0 + 0.5 * (coords.x * coords.x + coords.y * coords.y),
  ));
  let ray_dir = m * dir;

  let t = u.in1.x / 3.0;
  var ray_pos = vec3<f32>(
    2.0 * (sin(t + sin(2.0 * t) / 2.0) / 2.0 + 0.5),
    2.0 * (sin(t - sin(2.0 * t) / 2.0 - P9_PI / 2.0) / 2.0 + 0.5),
    2.0 * ((-2.0 * (t - sin(4.0 * t) / 4.0) / P9_PI) + 0.5 + 0.5),
  );

  var i = 64.0;
  for (var j = 0; j < 64; j = j + 1) {
    let dist = truchet_distfunc(ray_pos);
    ray_pos = ray_pos + dist * ray_dir;
    if (abs(dist) < 0.001) {
      i = f32(j);
      break;
    }
  }

  let normal = normalize(truchet_gradient(ray_pos));
  let ao = 1.0 - i / 64.0;
  let what = pow(max(0.0, dot(normal, -ray_dir)), 2.0);
  let vignette = sqrt(1.5 - length(coords));
  let light = ao * what * vignette * 1.4;
  let col = (cos(ray_pos / 2.0) + vec3<f32>(2.0)) / vec3<f32>(3.0);

  let reflected = reflect(ray_dir, normal);
  let yaw = 0.5 - atan2(reflected.z, -reflected.x) / P9_PI2;
  let pitch = 0.5 - asin(clamp(reflected.y, -1.0, 1.0)) / P9_PI;
  let env = textureSampleLevel(g_texture1, g_sampler, vec2<f32>(yaw, pitch), 0.0).rgb;

  return vec4<f32>(col * light + 0.1 * env, 1.0);
}
"#;

const TRUCHET_SHADER2: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let c = textureSampleLevel(g_texture1, g_sampler, tex, 0.0) * u.color1;
  return c * c;
}
"#;

impl Point10Scene for TruchetTentacles {
    fn name(&self) -> &'static str {
        "Plane9 Truchet Tentacles"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.size = (ctx.width.max(1), ctx.height.max(1));
        self.uniforms = Plane9Uniforms::new(self.size.0, self.size.1);
        self.uniforms.in2 = [self.lens, 0.0, 0.0, 0.0];
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
        self.perm += (beat * 20.0 + 0.5) * dt;
        self.uniforms.time = self.elapsed;
        self.uniforms.in1 = [self.permrand + self.perm, 0.0, 0.0, 0.0];
        self.uniforms.in2 = [self.lens, 0.0, 0.0, 0.0];
        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        // Shader1 reads the previous frame's RTT and writes this frame's.
        gpu.pass1.draw(
            encoder,
            &gpu.rtt_view,
            CLEAR,
            "point10-plane9-truchet-raymarch",
        );
        // Shader2 squares the current RTT onto the screen.
        gpu.pass2
            .draw(encoder, target, CLEAR, "point10-plane9-truchet-present");
        // StoreTexture1 (SwapDelay 0): keep this frame for the next one.
        gpu.blit.draw(
            encoder,
            &gpu.history_view,
            CLEAR,
            "point10-plane9-truchet-store",
        );
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        plane9_scene_props(TRUCHET_PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Truchet Tentacles", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn camera_clear_and_graph_constants_match_the_preset() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, -2.0]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, 1.0]);
        assert_eq!(CLEAR.a, 0.0);
        assert_eq!(RTT_FORMAT, wgpu::TextureFormat::Rgba8Unorm);
        assert_eq!(ITERATIONS, 64);
        assert_eq!(THICKNESS, 0.1);
        assert_eq!(SUPER_QUAD_POWER, 8.0);
        assert_eq!(FISHEYE, 0.5);
        assert_eq!(SWAP_DELAY, 0);
    }

    #[test]
    fn shader_keeps_the_truchet_maths() {
        let wgsl = truchet_shader1_wgsl();
        for needle in [
            "1.38984 * sin(r.z), 1.13233 * cos(r.z)",
            "653758.5453",
            "1.0 - pos.x, pos.y",
            "1.0 - cellpos.y, 1.0 - cellpos.x, cellpos.z",
            "rnd < 7.0 / 8.0",
            "1.0 / 8.0)",
            "vec3<f32>(-sa, 0.0, ca)",
            "let m = m0 * m0 * m0 * m0;",
            "0.5 - atan2(reflected.z, -reflected.x) / P9_PI2",
            "0.5 - asin(clamp(reflected.y, -1.0, 1.0)) / P9_PI",
            "col * light + 0.1 * env",
        ] {
            assert!(wgsl.contains(needle), "lost {needle}");
        }
    }

    #[test]
    fn all_shaders_validate() {
        use crate::point10::scenes::plane9_common::{PLANE9_TEXTURE_WGSL, PLANE9_UNIFORMS_WGSL};
        let sources = [
            format!(
                "{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{}",
                truchet_shader1_wgsl()
            ),
            format!("{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{TRUCHET_SHADER2}"),
            format!("{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{PLANE9_COPY_FRAGMENT}"),
        ];
        for wgsl in sources {
            let module = naga::front::wgsl::parse_str(&wgsl)
                .unwrap_or_else(|error| panic!("Truchet WGSL failed to parse: {error}"));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("Truchet WGSL validates");
        }
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = TruchetTentacles::new();
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
