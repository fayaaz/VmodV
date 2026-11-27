//! Plane9 "Though The Noise" — Rust/wgpu port of `Other/Though The Noise.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Other/Though The Noise.xml` by
//! Joakim Dahl, CC BY-NC-SA 3.0. A 50-step raymarch through
//! `abs(_noise(p * 0.5))`, lit with Plane9's Cook-Torrance directional light
//! and a Stormy Days cubemap reflection.
//!
//! # Ported exactly
//!
//! * `Dist(p) = abs(_noise(p * 0.5))` and the `/d` forward-difference normal
//!   (`d = 0.4`), with the preset's unnormalised normal fed to the lighting;
//! * the `rotateX(rotateY(...))` ray/origin rotation, the `50`-step march
//!   (`dist < 0.003`, `point += rayDir * dist * 0.25`) and the `t` distance;
//! * `roughness = gIn3.x = 0.459`, `metallic = 1`, albedo white (so the
//!   diffuse albedo is zero and the specular albedo is one);
//! * the directional white `* 0.1` light along `(0,0,1)`, the cubemap
//!   `textureLod(reflect(-rayDir, normal), roughness² * 8.0) * 0.03`, and
//!   `_fresnelRoughness`;
//! * the exponential fog to black, `c *= 5` and the radial vignette;
//! * the expression `perm += (beat(0) * 3 + 0.01) * dt`,
//!   `in1.x = noise2(perm, 0)`, `in1.y = noise2(perm, 1)`,
//!   `in1.z = time * 0.1 + perm * 3`;
//! * `FileTexture1` `data\Stormy days` from the bundled six-face cubemap.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::plane9_common::{
    Plane9Pass, Plane9TextureBinding, Plane9Uniforms, fallback_cube, load_stormy_cube,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::p9_shading::P9_SHADING_WGSL;
use crate::point10::shared::random::value_noise2;
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
/// `Shader1` `In3` (roughness) default.
pub const ROUGHNESS: f32 = 0.459;
/// `Shader1` `Color1` default.
pub const COLOR1: [f32; 4] = [0.094, 0.702, 0.0, 1.0];
/// March step cap.
pub const MARCH_STEPS: u32 = 50;
/// March hit threshold.
pub const HIT_EPSILON: f32 = 0.003;
/// March step length.
pub const MARCH_STEP: f32 = 0.25;

const NOISE_RAY_PROPS: &[SceneProp] = &[];

/// One frame of the Though The Noise expression.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThoughTheNoiseDrive {
    /// `perm` for the next frame.
    pub perm: f32,
    /// `gIn1`.
    pub in1: [f32; 4],
}

/// Pure port of the Though The Noise expression.
pub fn though_the_noise_drive(beat: f32, time: f32, dt: f32, perm: f32) -> ThoughTheNoiseDrive {
    let beat = if beat.is_finite() { beat.max(0.0) } else { 0.0 };
    let step = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    let perm = if perm.is_finite() { perm } else { 1.0 };
    let next = perm + (beat * 3.0 + 0.01) * step;
    let time = if time.is_finite() { time } else { 0.0 };
    ThoughTheNoiseDrive {
        perm: next,
        in1: [
            value_noise2(next, 0.0),
            value_noise2(next, 1.0),
            time * 0.1 + next * 3.0,
            0.0,
        ],
    }
}

struct NoiseRayGpu {
    pass: Plane9Pass,
    _cube: wgpu::Texture,
    _cube_view: wgpu::TextureView,
    _sampler: wgpu::Sampler,
}

/// `Plane9 Though The Noise` scene.
pub struct ThoughTheNoise {
    audio: P10Audio,
    perm: f32,
    elapsed: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    assets_dir: Option<PathBuf>,
    uniforms: Plane9Uniforms,
    gpu: Option<NoiseRayGpu>,
}

impl Default for ThoughTheNoise {
    fn default() -> Self {
        Self::new()
    }
}

impl ThoughTheNoise {
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
            assets_dir: None,
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
        let (cube, cube_view, sampler) =
            load_stormy_cube(&device, &queue, self.assets_dir.as_deref())
                .unwrap_or_else(|| fallback_cube(&device, &queue));
        let pass = Plane9Pass::new(
            &device,
            &queue,
            self.target_format,
            "point10-plane9-though-the-noise",
            though_the_noise_wgsl(),
            Some(Plane9TextureBinding::Cube {
                view: &cube_view,
                sampler: &sampler,
            }),
        );
        self.gpu = Some(NoiseRayGpu {
            pass,
            _cube: cube,
            _cube_view: cube_view,
            _sampler: sampler,
        });
        self.upload_uniforms();
    }

    fn upload_uniforms(&self) {
        if let Some(gpu) = &self.gpu {
            gpu.pass.upload(&self.uniforms);
        }
    }
}

fn though_the_noise_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| {
        format!(
            "{P9_COMMON_WGSL}\n{P9_SHADING_WGSL}\n{}",
            THOUGH_THE_NOISE_FRAGMENT
        )
    })
}

const THOUGH_THE_NOISE_FRAGMENT: &str = r#"
fn rotate_x(p: vec3<f32>, a: f32) -> vec3<f32> {
  let sa = sin(a);
  let ca = cos(a);
  return vec3<f32>(p.x, ca * p.y - sa * p.z, sa * p.y + ca * p.z);
}

fn rotate_y(p: vec3<f32>, a: f32) -> vec3<f32> {
  let sa = sin(a);
  let ca = cos(a);
  return vec3<f32>(ca * p.x + sa * p.z, p.y, -sa * p.x + ca * p.z);
}

fn noise_dist(p: vec3<f32>) -> f32 {
  return abs(p9_noise(p * 0.5));
}

fn noise_normal(p: vec3<f32>) -> vec3<f32> {
  let d = 0.4;
  let f0 = noise_dist(p);
  let fx = noise_dist(p + vec3<f32>(d, 0.0, 0.0));
  let fy = noise_dist(p + vec3<f32>(0.0, d, 0.0));
  let fz = noise_dist(p + vec3<f32>(0.0, 0.0, d));
  return vec3<f32>(fx - f0, fy - f0, fz - f0) / d;
}

fn apply_fog(org_col: vec3<f32>, dist_to_cam: f32) -> vec3<f32> {
  let fog_amount = 1.0 - exp(-dist_to_cam * 0.2);
  return mix(org_col, vec3<f32>(0.0, 0.0, 0.0), fog_amount);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let p = -1.0 + 2.0 * tex;
  var point = vec3<f32>(p.x, p.y, -1.0 + u.in1.z);
  var ray_dir = normalize(vec3<f32>(p.x, p.y, 1.0));
  var t = 0.0;

  ray_dir = rotate_x(rotate_y(ray_dir, u.in1.x), u.in1.y);
  point = rotate_x(rotate_y(point - vec3<f32>(0.0, 0.0, u.in1.z), u.in1.x), u.in1.y);
  point.z += u.in1.z;

  for (var step_nr = 0; step_nr < 50; step_nr = step_nr + 1) {
    let dist = noise_dist(point);
    if (dist < 0.003) {
      break;
    }
    point += ray_dir * dist * 0.25;
    t += dist;
  }

  let dist_to_cam = t;
  let normal = noise_normal(point);

  let light_color = vec3<f32>(1.0, 1.0, 1.0) * 0.1;
  let albedo_color = vec3<f32>(1.0, 1.0, 1.0);
  let real_albedo = albedo_color - albedo_color * vec3<f32>(1.0);
  let real_specular_color = mix(vec3<f32>(0.04), albedo_color, vec3<f32>(1.0));
  let light = p9_light_directional(
    real_albedo,
    real_specular_color,
    normal,
    u.in3.x,
    light_color,
    vec3<f32>(0.0, 0.0, 1.0),
    ray_dir,
  );

  let mip_index = u.in3.x * u.in3.x * 8.0;
  let env_color = textureSampleLevel(
    g_texture1,
    g_sampler,
    reflect(-ray_dir, normal),
    mip_index,
  ).rgb;
  let env_fresnel = p9_fresnel_roughness(real_specular_color, u.in3.x, normal, ray_dir);

  var c = light + env_fresnel * env_color * 0.03 + real_albedo;
  c = apply_fog(c, dist_to_cam);
  c = c * 5.0;
  c = c * p9_smoothstep(2.0, 0.5, length(p));
  return vec4<f32>(c, 1.0);
}
"#;

impl Point10Scene for ThoughTheNoise {
    fn name(&self) -> &'static str {
        "Plane9 Though The Noise"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.assets_dir = ctx.assets_dir.clone();
        self.size = (ctx.width.max(1), ctx.height.max(1));
        self.uniforms = Plane9Uniforms::new(self.size.0, self.size.1);
        self.uniforms.in3 = [ROUGHNESS, 0.0, 0.0, 0.0];
        self.uniforms.color1 = COLOR1;
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
        let drive = though_the_noise_drive(self.audio.beat(0.0), self.elapsed, dt, self.perm);
        self.perm = drive.perm;
        self.uniforms.time = self.elapsed;
        self.uniforms.in1 = drive.in1;
        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        if let Some(gpu) = &self.gpu {
            gpu.pass
                .draw(encoder, target, CLEAR, "point10-plane9-though-the-noise");
        }
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(NOISE_RAY_PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Though The Noise", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn camera_clear_and_material_match_the_preset() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, -2.0]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, 1.0]);
        assert_eq!(CLEAR.a, 1.0);
        assert_eq!(ROUGHNESS, 0.459);
        assert_eq!(COLOR1, [0.094, 0.702, 0.0, 1.0]);
        assert_eq!(MARCH_STEPS, 50);
        assert_eq!(HIT_EPSILON, 0.003);
    }

    #[test]
    fn drive_matches_the_expression() {
        let drive = though_the_noise_drive(0.5, 2.0, 0.5, 1.0);
        let perm = 1.0 + (1.5 + 0.01) * 0.5;
        assert!((drive.perm - perm).abs() < 1e-6);
        assert!((drive.in1[0] - value_noise2(perm, 0.0)).abs() < 1e-6);
        assert!((drive.in1[2] - (2.0 * 0.1 + perm * 3.0)).abs() < 1e-6);
    }

    #[test]
    fn shader_keeps_the_raymarch_and_lighting() {
        let wgsl = though_the_noise_wgsl();
        for needle in [
            "abs(p9_noise(p * 0.5))",
            "let d = 0.4;",
            "if (dist < 0.003)",
            "point += ray_dir * dist * 0.25;",
            "vec3<f32>(0.0, 0.0, 1.0)",
            "u.in3.x * u.in3.x * 8.0",
            "env_fresnel * env_color * 0.03",
            "c = c * 5.0;",
        ] {
            assert!(wgsl.contains(needle), "lost {needle}");
        }
    }

    #[test]
    fn shader_validates() {
        use crate::point10::scenes::plane9_common::{PLANE9_CUBE_WGSL, PLANE9_UNIFORMS_WGSL};
        let source = format!(
            "{PLANE9_UNIFORMS_WGSL}\n{PLANE9_CUBE_WGSL}\n{}",
            though_the_noise_wgsl()
        );
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("Though The Noise WGSL failed to parse: {error}"));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("Though The Noise WGSL validates");
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = ThoughTheNoise::new();
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
