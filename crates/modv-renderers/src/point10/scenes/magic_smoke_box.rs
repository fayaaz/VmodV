//! Plane9 "Magic Smoke Box" — Rust/wgpu port of `Cell/Magic Smoke Box.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Cell/Magic Smoke Box.xml` by Joakim
//! Dahl, CC BY-NC-SA 3.0.
//!
//! # The graph
//!
//! ```text
//! Shader1 (per-channel reaction-diffusion over the stored previous frame)
//!   -> RenderRect2 -> RenderToTexture1 (RGBA8)
//!     -> StoreTexture1 (previous frame)
//!     -> Shader2 (smoke threshold) -> RenderRect1 -> Screen1
//! ```
//!
//! The topology is identical to [`super::abstract_clouds`]; this preset is the
//! colour-lane variant. Only the taps/constants differ, so the shared
//! [`super::plane9_common::Plane9Feedback`] harness is reused.
//!
//! # Ported exactly
//!
//! * the nine-tap ring at radii `1, 2, 8, 16`, rotated by `gIn1.y`;
//! * per-channel `+1 / -1` change accumulation against
//!   `0.5 + i * gIn2.x`;
//! * `c = (v1+v2+v3+v4)*0.4 + (c1+c2+c3+c4)*0.001 + c*0.92`;
//! * the `_perm` RGB seed with `gIn3.xyz`;
//! * Shader2's `max(r,g,b)-0.25` squared smoke.
//!
//! The RTT requests `Width/Height = 1` (50%); `RT_SCALE` raises it back to
//! full resolution, matching the port's per-scene override.
//!
//! As in [`super::abstract_clouds`], the XML's `gFrameNr < 2` branch is a
//! dedicated seed pass: the first two frames write the `_perm` RGB seed into
//! the ping-pong target that frame 2's evolve pass then samples. It is
//! time-independent, so no steady-state frame can fall back into it.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::plane9_common::{
    PLANE9_BATCH_WGSL, Plane9Feedback, Plane9Pass, Plane9TextureBinding, Plane9Uniforms,
    plane9_sampler,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::random::seeded_rand;
use crate::point10::{
    Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, scene_props_with_levers,
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
/// `RenderToTexture1` `Width`/`Height` mode (1 = 50%).
pub const RTT_MODE: u32 = 1;
/// The port's `RT_SCALE` for this preset: 2.0, i.e. back to full resolution.
pub const RT_SCALE: f32 = 2.0;
/// `RenderToTexture1` `CreateMipMaps`.
pub const RTT_MIPS: bool = true;
/// `StoreTexture1` `SwapDelay`.
pub const SWAP_DELAY: f32 = 0.04;
/// The four ring radii.
pub const RADII: [f32; 4] = [1.0, 2.0, 8.0, 16.0];
/// Variance weight in the update.
pub const VARIANCE_WEIGHT: f32 = 0.4;
/// Change-sum weight in the update.
pub const CHANGE_WEIGHT: f32 = 0.001;
/// Previous-frame retention in the update.
pub const RETENTION: f32 = 0.92;
/// `gFrameNr < 2` seeds the reaction instead of advancing it.
pub const SEED_FRAMES: u32 = 2;

const PROPS: &[SceneProp] = &[];

fn evolve_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| format!("{P9_COMMON_WGSL}\n{PLANE9_BATCH_WGSL}\n{EVOLVE}"))
}

fn seed_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| format!("{P9_COMMON_WGSL}\n{PLANE9_BATCH_WGSL}\n{SEED}"))
}

const SEED: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let p = tex * vec2<f32>(2.0) - vec2<f32>(1.0);
  return vec4<f32>(
    p9_perm4_vec2(p + u.in3.xx).r,
    p9_perm4_vec2(p + u.in3.yy).r,
    p9_perm4_vec2(p + u.in3.zz).r,
    1.0,
  );
}
"#;

const EVOLVE: &str = r#"
struct P9EvalVar4 {
  value: vec4<f32>,
  change: vec4<f32>,
}

fn p9_eval_avg4(p: vec2<f32>, tp: vec2<f32>, radius: f32) -> vec4<f32> {
  var co = vec4<f32>(0.0);
  for (var dir_nr = 0; dir_nr < 9; dir_nr = dir_nr + 1) {
    let dir = f32(dir_nr) * P9_PI2 / 9.0 + u.in1.y;
    let d = vec2<f32>(radius * cos(dir), radius * sin(dir));
    co = co + textureSampleLevel(g_texture1, g_sampler, fract(p + tp * d), 0.0) / 9.0;
  }
  return co;
}

fn p9_eval_var4(
  p: vec2<f32>,
  tp: vec2<f32>,
  radius: f32,
  avg: vec4<f32>,
  lim: f32,
) -> P9EvalVar4 {
  var co = vec4<f32>(0.0);
  var change = vec4<f32>(0.0);
  for (var dir_nr = 0; dir_nr < 9; dir_nr = dir_nr + 1) {
    let dir = f32(dir_nr) * P9_PI2 / 9.0 + u.in1.y;
    let d = vec2<f32>(radius * cos(dir), radius * sin(dir));
    let src = textureSampleLevel(g_texture1, g_sampler, fract(p + tp * d), 0.0);
    co = co + abs(src - avg) / 9.0;
    if (src.r > lim) { change.r = change.r + 1.0; } else { change.r = change.r - 1.0; }
    if (src.g > lim) { change.g = change.g + 1.0; } else { change.g = change.g - 1.0; }
    if (src.b > lim) { change.b = change.b + 1.0; } else { change.b = change.b - 1.0; }
  }
  return P9EvalVar4(co, change);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let tp = vec2<f32>(1.0) / u.resolution;

  let c = textureSampleLevel(g_texture1, g_sampler, tex, 0.0);

  let a1 = p9_eval_avg4(tex, tp, 1.0);
  let a2 = p9_eval_avg4(tex, tp, 2.0);
  let a3 = p9_eval_avg4(tex, tp, 8.0);
  let a4 = p9_eval_avg4(tex, tp, 16.0);

  let v1 = p9_eval_var4(tex, tp, 1.0, a1, 0.5 + 0.0 * u.in2.x);
  let v2 = p9_eval_var4(tex, tp, 2.0, a2, 0.5 + 1.0 * u.in2.x);
  let v3 = p9_eval_var4(tex, tp, 8.0, a3, 0.5 + 2.0 * u.in2.x);
  let v4 = p9_eval_var4(tex, tp, 16.0, a4, 0.5 + 3.0 * u.in2.x);

  let next = (v1.value + v2.value + v3.value + v4.value) * 0.4
    + (v1.change + v2.change + v3.change + v4.change) * 0.001
    + c * 0.92;
  return vec4<f32>(next.rgb, 1.0);
}
"#;

const PRESENT: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let sampled = textureSampleLevel(g_texture1, g_sampler, tex, 0.0) * u.color1;
  var c = vec4<f32>(max(sampled.r, max(sampled.g, sampled.b)) - 0.25);
  c = c * c;
  return c;
}
"#;

struct Gpu {
    seed: Plane9Pass,
    evolve: Plane9Pass,
    present: Plane9Pass,
    feedback: Plane9Feedback,
    sampler: wgpu::Sampler,
}

/// `Plane9 Magic Smoke Box` scene.
pub struct MagicSmokeBox {
    audio: P10Audio,
    permrand: f32,
    elapsed: f32,
    frame: u32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    uniforms: Plane9Uniforms,
    gpu: Option<Gpu>,
}

impl Default for MagicSmokeBox {
    fn default() -> Self {
        Self::new()
    }
}

impl MagicSmokeBox {
    /// Create the scene.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            permrand: 1.0,
            elapsed: 0.0,
            frame: 0,
            size: (1, 1),
            target_format: crate::point10::HDR_FORMAT,
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

    /// Bind the modV props (this preset declares no `sceneProps`).
    pub fn apply_props(&mut self, _props: impl FnMut(&str) -> Option<f32>) {}

    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        let feedback = Plane9Feedback::new(
            &device,
            "point10-plane9-magic-smoke-box-feedback",
            self.size.0,
            self.size.1,
            RTT_FORMAT,
        );
        let sampler = plane9_sampler(&device, "point10-plane9-magic-smoke-box-sampler");
        let seed = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-magic-smoke-box-seed",
            seed_wgsl(),
            None,
        );
        let evolve = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-magic-smoke-box-evolve",
            evolve_wgsl(),
            Some(Plane9TextureBinding::D2 {
                view: feedback.previous_view(),
                sampler: &sampler,
            }),
        );
        let present = Plane9Pass::new(
            &device,
            &queue,
            self.target_format,
            "point10-plane9-magic-smoke-box-present",
            PRESENT,
            Some(Plane9TextureBinding::D2 {
                view: feedback.current_view(),
                sampler: &sampler,
            }),
        );
        self.gpu = Some(Gpu {
            seed,
            evolve,
            present,
            feedback,
            sampler,
        });
        self.upload_uniforms();
    }

    fn upload_uniforms(&self) {
        if let Some(gpu) = &self.gpu {
            gpu.seed.upload(&self.uniforms);
            gpu.evolve.upload(&self.uniforms);
            gpu.present.upload(&self.uniforms);
        }
    }
}

impl Point10Scene for MagicSmokeBox {
    fn name(&self) -> &'static str {
        "Plane9 Magic Smoke Box"
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
        let frame = self.frame as i32;
        let random = |salt: i32| seeded_rand(frame, salt);
        self.uniforms.time = self.elapsed;
        self.uniforms.in1[0] = self.elapsed;
        self.uniforms.in1[1] = random(101) * std::f32::consts::TAU;
        self.uniforms.in1[2] = random(103) * std::f32::consts::TAU;
        self.uniforms.in2[0] = random(107) * 0.1 + 0.9;
        self.uniforms.in2[1] = random(109);
        self.uniforms.in3 = [
            self.permrand,
            self.permrand + 1.23,
            self.permrand + std::f32::consts::PI,
            0.0,
        ];
        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        if self.frame < SEED_FRAMES {
            gpu.seed.draw(
                encoder,
                gpu.feedback.current_view(),
                CLEAR,
                "point10-plane9-magic-smoke-box-seed",
            );
        } else {
            gpu.evolve
                .rebind_texture(gpu.feedback.previous_view(), &gpu.sampler);
            gpu.evolve.upload(&self.uniforms);
            gpu.evolve.draw(
                encoder,
                gpu.feedback.current_view(),
                CLEAR,
                "point10-plane9-magic-smoke-box-evolve",
            );
        }
        gpu.present
            .rebind_texture(gpu.feedback.current_view(), &gpu.sampler);
        gpu.present.upload(&self.uniforms);
        gpu.present.draw(
            encoder,
            target,
            CLEAR,
            "point10-plane9-magic-smoke-box-present",
        );
        gpu.feedback.swap();
        self.frame = self.frame.saturating_add(1);
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
        self.frame = 0;
        self.elapsed = 0.0;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Magic Smoke Box", "3d-scenes")
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
        assert_eq!(RTT_MODE, 1);
        assert_eq!(RT_SCALE, 2.0);
        const { assert!(RTT_MIPS) };
        assert_eq!(SWAP_DELAY, 0.04);
        assert_eq!(RADII, [1.0, 2.0, 8.0, 16.0]);
        assert_eq!(VARIANCE_WEIGHT, 0.4);
        assert_eq!(CHANGE_WEIGHT, 0.001);
        assert_eq!(RETENTION, 0.92);
        assert_eq!(SEED_FRAMES, 2);
    }

    #[test]
    fn shaders_keep_the_colour_lanes_and_smoke() {
        let evolve = evolve_wgsl();
        for needle in [
            "f32(dir_nr) * P9_PI2 / 9.0 + u.in1.y",
            "if (src.r > lim)",
            "if (src.g > lim)",
            "if (src.b > lim)",
            "(v1.value + v2.value + v3.value + v4.value) * 0.4",
            "c * 0.92",
        ] {
            assert!(evolve.contains(needle), "lost {needle}");
        }
        let seed = seed_wgsl();
        for needle in [
            "p9_perm4_vec2(p + u.in3.xx).r",
            "p9_perm4_vec2(p + u.in3.yy).r",
            "p9_perm4_vec2(p + u.in3.zz).r",
        ] {
            assert!(seed.contains(needle), "lost {needle}");
        }
        assert!(
            !seed.contains("u.time") && !seed.contains("g_texture1"),
            "the seed must stay time-independent and texture-free so frames 0/1 are identical"
        );
        for needle in [
            "max(sampled.r, max(sampled.g, sampled.b)) - 0.25",
            "c = c * c",
        ] {
            assert!(PRESENT.contains(needle), "lost {needle}");
        }
    }

    #[test]
    fn shaders_validate() {
        let sources = [
            format!("{PLANE9_UNIFORMS_WGSL}\n{}", seed_wgsl()),
            format!(
                "{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{}",
                evolve_wgsl()
            ),
            format!("{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{PRESENT}"),
        ];
        for wgsl in sources {
            let module = naga::front::wgsl::parse_str(&wgsl)
                .unwrap_or_else(|error| panic!("Magic Smoke Box WGSL failed to parse: {error}"));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("Magic Smoke Box WGSL validates");
        }
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = MagicSmokeBox::new();
        let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        ctx.resize(64, 64);
        scene.init(&mut ctx);
        let mut bridge = AudioBridge::new();
        let mut payload = BandPayload::silent(44_100.0);
        payload.lanes = [0.5; 12];
        let target = TestTarget::new(&device, 64, 64);
        let mut encoder = device.create_command_encoder(&Default::default());
        for _ in 0..4 {
            bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
            scene.update(1.0 / 60.0, &bridge);
            scene.render(&mut encoder, &target.view);
        }
        queue.submit(Some(encoder.finish()));
        assert!(any_lit(&target.read(&device, &queue)));
    }
}
