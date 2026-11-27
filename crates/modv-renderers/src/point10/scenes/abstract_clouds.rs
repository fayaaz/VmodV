//! Plane9 "Abstract Clouds" — Rust/wgpu port of `Cell/Abstract Clouds.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Cell/Abstract Clouds.xml` by Joakim
//! Dahl, CC BY-NC-SA 3.0.
//!
//! # The graph
//!
//! ```text
//! Shader1 (reaction-diffusion over the stored previous frame)
//!   -> RenderRect2 -> RenderToTexture1 (RGBA8)
//!     -> StoreTexture1 (previous frame)
//!     -> Shader2 (monochrome tint) -> RenderRect1 -> Screen1
//! ```
//!
//! `RenderToTexture1`'s inline `_fbm` fragment is dead: its `Render` input is
//! the `Shader1` chain, so the `RenderRect2.Render` connection overrides the
//! RT's own shader.
//!
//! # Ported exactly
//!
//! * the nine-tap `EvalAvg` / `EvalVar` ring at radii `2, 4, 8, 16`, rotated by
//!   `gIn2.z`, with the mean-red average and `|src - avg| / 9` variance;
//! * the per-ring threshold `0.5 + i * 0.05` and the `+1 / -1` change sum;
//! * the update `c = (v1+v2+v3+v4)*0.53 + (c1+c2+c3+c4)*0.002 + c*0.85`;
//! * the two-frame `_perm(p*0.1 + gIn3.xx)` seed;
//! * Shader2's `(c, c*0.25+0.75, 0.95)` cubic tint.
//!
//! The reaction-diffusion harness (ping-pong targets + seed/evolve passes) is
//! shared with [`super::magic_smoke_box`] through
//! [`super::plane9_common::Plane9Feedback`]; only the taps and constants live
//! here.
//!
//! `gFrameNr < 2` is a dedicated seed pass rather than a branch in the evolve
//! shader: frames 0 and 1 write the `_perm` seed into the current target and
//! swap, so frame 2's evolve pass samples the last seed through `previous_view`.
//! The seed is time-independent, so this is exactly the XML branch; from frame
//! 2 on only `EVOLVE` runs and `SEED_FRAMES` is never revisited.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::plane9_common::PLANE9_BATCH_WGSL;
use crate::point10::scenes::plane9_common::{
    Plane9Feedback, Plane9Pass, Plane9TextureBinding, Plane9Uniforms, plane9_sampler,
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
/// `RenderToTexture1` `Width`/`Height` modes (0 = 100%).
pub const RTT_MODE: u32 = 0;
/// `RenderToTexture1` `CreateMipMaps`.
pub const RTT_MIPS: bool = false;
/// `StoreTexture1` `SwapDelay` (read by the XML, unused by this port: the
/// ping-pong pair already hand the shader the previous frame).
pub const SWAP_DELAY: f32 = 0.05;
/// `EvalAvg`/`EvalVar` tap count.
pub const TAPS: u32 = 9;
/// The four ring radii.
pub const RADII: [f32; 4] = [2.0, 4.0, 8.0, 16.0];
/// The per-ring thresholds (`0.5 + i * 0.05`).
pub const THRESHOLDS: [f32; 4] = [0.50, 0.55, 0.60, 0.65];
/// Variance weight in the update.
pub const VARIANCE_WEIGHT: f32 = 0.53;
/// Change-sum weight in the update.
pub const CHANGE_WEIGHT: f32 = 0.002;
/// Previous-frame retention in the update.
pub const RETENTION: f32 = 0.85;
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
  let vc = p9_perm4_vec2(p * vec2<f32>(0.1) + u.in3.xx).r;
  return vec4<f32>(vc, vc, vc, 1.0);
}
"#;

const EVOLVE: &str = r#"
struct P9EvalVar {
  value: f32,
  change: f32,
}

fn p9_eval_avg(p: vec2<f32>, tp: vec2<f32>, radius: f32) -> f32 {
  var co = 0.0;
  for (var dir_nr = 0; dir_nr < 9; dir_nr = dir_nr + 1) {
    let dir = f32(dir_nr) * P9_PI2 / 9.0 + u.in2.z;
    let d = vec2<f32>(radius * cos(dir), radius * sin(dir));
    co = co + textureSampleLevel(g_texture1, g_sampler, fract(p + tp * d), 0.0).r;
  }
  return co / 9.0;
}

fn p9_eval_var(
  p: vec2<f32>,
  tp: vec2<f32>,
  radius: f32,
  avg: f32,
  lim: f32,
) -> P9EvalVar {
  var co = 0.0;
  var change = 0.0;
  for (var dir_nr = 0; dir_nr < 9; dir_nr = dir_nr + 1) {
    let dir = f32(dir_nr) * P9_PI2 / 9.0 + u.in2.z;
    let d = vec2<f32>(radius * cos(dir), radius * sin(dir));
    let src = textureSampleLevel(g_texture1, g_sampler, fract(p + tp * d), 0.0).r;
    co = co + abs(src - avg) / 9.0;
    if (src > lim) {
      change = change + 1.0;
    } else {
      change = change - 1.0;
    }
  }
  return P9EvalVar(co, change);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let p = tex * vec2<f32>(2.0) - vec2<f32>(1.0);
  let tp = vec2<f32>(1.0) / u.resolution;

  let c = textureSampleLevel(g_texture1, g_sampler, tex, 0.0).r;

  let a1 = p9_eval_avg(tex, tp, 2.0);
  let a2 = p9_eval_avg(tex, tp, 4.0);
  let a3 = p9_eval_avg(tex, tp, 8.0);
  let a4 = p9_eval_avg(tex, tp, 16.0);

  let v1 = p9_eval_var(tex, tp, 2.0, a1, 0.5 + 0.0 * 0.05);
  let v2 = p9_eval_var(tex, tp, 4.0, a2, 0.5 + 1.0 * 0.05);
  let v3 = p9_eval_var(tex, tp, 8.0, a3, 0.5 + 2.0 * 0.05);
  let v4 = p9_eval_var(tex, tp, 16.0, a4, 0.5 + 3.0 * 0.05);

  let next = (v1.value + v2.value + v3.value + v4.value) * 0.53
    + (v1.change + v2.change + v3.change + v4.change) * 0.002
    + c * 0.85;
  return vec4<f32>(next, next, next, 1.0);
}
"#;

const PRESENT: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let c = textureSampleLevel(g_texture1, g_sampler, tex, 0.0).r;
  var out = vec4<f32>(c, c * 0.25 + 0.75, 0.95, 1.0);
  out = vec4<f32>(out.rgb * out.rgb * out.rgb, out.a);
  return out;
}
"#;

struct Gpu {
    seed: Plane9Pass,
    evolve: Plane9Pass,
    present: Plane9Pass,
    feedback: Plane9Feedback,
    sampler: wgpu::Sampler,
}

/// `Plane9 Abstract Clouds` scene.
pub struct AbstractClouds {
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

impl Default for AbstractClouds {
    fn default() -> Self {
        Self::new()
    }
}

impl AbstractClouds {
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
            "point10-plane9-abstract-clouds-feedback",
            self.size.0,
            self.size.1,
            RTT_FORMAT,
        );
        let sampler = plane9_sampler(&device, "point10-plane9-abstract-clouds-sampler");
        let seed = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-abstract-clouds-seed",
            seed_wgsl(),
            None,
        );
        let evolve = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-abstract-clouds-evolve",
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
            "point10-plane9-abstract-clouds-present",
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

impl Point10Scene for AbstractClouds {
    fn name(&self) -> &'static str {
        "Plane9 Abstract Clouds"
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
        self.uniforms.in1 = [self.elapsed, 0.0, 0.0, 0.0];
        self.uniforms.in2 = [
            random(101) * 100.0,
            random(103) * 100.0,
            random(107) * std::f32::consts::TAU,
            0.0,
        ];
        self.uniforms.in3 = [self.permrand, 0.0, 0.0, 0.0];
        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let seeding = self.frame < SEED_FRAMES;
        if seeding {
            gpu.seed.draw(
                encoder,
                gpu.feedback.current_view(),
                CLEAR,
                "point10-plane9-abstract-clouds-seed",
            );
        } else {
            gpu.evolve.rebind_texture(
                gpu.feedback.previous_view(),
                // Borrowed before the mutable rebind below.
                &gpu.sampler,
            );
            gpu.evolve.upload(&self.uniforms);
            gpu.evolve.draw(
                encoder,
                gpu.feedback.current_view(),
                CLEAR,
                "point10-plane9-abstract-clouds-evolve",
            );
        }
        gpu.present
            .rebind_texture(gpu.feedback.current_view(), &gpu.sampler);
        gpu.present.upload(&self.uniforms);
        gpu.present.draw(
            encoder,
            target,
            CLEAR,
            "point10-plane9-abstract-clouds-present",
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
        SceneMeta::for_plane9_scene("Plane9 Abstract Clouds", "3d-scenes")
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
        assert_eq!(RTT_MODE, 0);
        const { assert!(!RTT_MIPS) };
        assert_eq!(SWAP_DELAY, 0.05);
        assert_eq!(TAPS, 9);
        assert_eq!(RADII, [2.0, 4.0, 8.0, 16.0]);
        assert_eq!(THRESHOLDS, [0.50, 0.55, 0.60, 0.65]);
        assert_eq!(SEED_FRAMES, 2);
    }

    #[test]
    fn shaders_keep_the_reaction_and_tint() {
        let evolve = evolve_wgsl();
        for needle in [
            "f32(dir_nr) * P9_PI2 / 9.0 + u.in2.z",
            "co + abs(src - avg) / 9.0",
            "0.5 + 3.0 * 0.05",
            "(v1.value + v2.value + v3.value + v4.value) * 0.53",
            "c * 0.85",
        ] {
            assert!(evolve.contains(needle), "lost {needle}");
        }
        let seed = seed_wgsl();
        assert!(
            seed.contains("p9_perm4_vec2(p * vec2<f32>(0.1) + u.in3.xx)"),
            "lost the seed"
        );
        assert!(
            !seed.contains("u.time") && !seed.contains("g_texture1"),
            "the seed must stay time-independent and texture-free so frames 0/1 are identical"
        );
        for needle in ["c * 0.25 + 0.75", "0.95", "out.rgb * out.rgb * out.rgb"] {
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
                .unwrap_or_else(|error| panic!("Abstract Clouds WGSL failed to parse: {error}"));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("Abstract Clouds WGSL validates");
        }
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = AbstractClouds::new();
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
