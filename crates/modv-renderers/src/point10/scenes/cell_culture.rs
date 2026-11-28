//! Plane9 "Cell Culture" — Rust/wgpu port of `Cell/Cell Culture.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Cell/Cell Culture.xml` by Joakim Dahl,
//! CC BY-NC-SA 3.0. The XML's `Desc` is empty; its tags are
//! "Tags: `mellow`", so this port records the tags and invents no prose.
//!
//! # The graph
//!
//! ```text
//! Shader1 (reaction step over the stored previous frame)
//!   -> RenderRect2 -> RenderToTexture1 (RGBA8, 100%)
//!     -> StoreTexture1 (previous frame)
//!     -> Shader2 (tint composite) -> RenderRect1 -> Screen1
//! ```
//!
//! `RenderToTexture1`'s inline fractal-noise fragment is dead: the
//! `RenderRect2.Render` connection overrides the RTT's own shader, so only the
//! `Shader1` chain is ported.
//!
//! # Ported exactly
//!
//! * the nine-tap `EvalAvg` / `EvalVar` ring at radii `2, 4, 8, 16`, rotated by
//!   `gIn2.z`; as in the vendor XML, `EvalVar` accumulates the unnormalised
//!   `|src - avg|` deviations in the loop (XML 185) and applies the single `/ 9`
//!   on return (XML 191), exactly as `EvalAvg` does;
//! * the per-ring threshold `0.5 + i * 0.05` and the `+0.005 / -0.005` change
//!   sum;
//! * the update `c = ((v1+v2+v3+v4)*0.2 + (c1+c2+c3+c4) + c)*0.79`;
//! * the vendor radius asymmetry: the fourth average is taken at radius `16`
//!   but the fourth variance ring at radius `10` (XML 210-217), kept verbatim;
//! * the two-frame `_perm(p*0.15 + gIn3.xx)` binary seed;
//! * Shader2's `texture(gTexture1, si.tex) * si.diffuse` tint with
//!   `Color1 = (0.8, 0.9, 0.9, 1)`.
//!
//! The reaction-diffusion harness (ping-pong targets + seed/evolve passes) is
//! shared with [`super::abstract_clouds`] and [`super::magic_smoke_box`]
//! through [`super::plane9_common::Plane9Feedback`]; only the taps and
//! constants live here.
//!
//! `gFrameNr < 2` is a dedicated seed pass rather than a branch in the evolve
//! shader: frames 0 and 1 write the `_perm` seed into the current target and
//! swap, so frame 2's evolve pass samples the last seed through
//! `previous_view`. The seed is time-independent, so this is exactly the XML
//! branch; from frame 2 on only `EVOLVE` runs and `SEED_FRAMES` is never
//! revisited.
//!
//! `RenderToTexture1` requests `CreateMipMaps = true`, recorded here but
//! inert: every tap samples explicit LOD 0, the ping-pong targets are
//! single-mip, and no mip-generation pass is built.
//!
//! # Expression1
//!
//! ```text
//! in1.x = time;            // set but unread by either shader
//! in2.x = random()*100;
//! in2.y = random()*100;
//! in2.z = random()*PI*2;   // rotates every nine-tap ring each frame
//! in3.x = permrand;        // seeds _perm; the repository convention is 1.0
//! ```
//!
//! `in1.x` is vestigial: neither `SEED` nor `EVOLVE` binds `gIn1`.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::cube_mesh::{Mat4, perspective};
use crate::point10::scenes::plane9_common::{
    PLANE9_BATCH_WGSL, Plane9Feedback, Plane9Pass, Plane9TextureBinding, Plane9Uniforms,
    plane9_sampler,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::random::seeded_rand;
use crate::point10::{
    Point10Scene, SceneContext, SceneMeta, SceneProp, apply_lens, frame_dt, lens_value,
    plane9_scene_props,
};

/// `Screen1` camera position.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, -2.0];
/// `Screen1` camera rotation (zero).
pub const CAMERA_ROTATION: [f32; 3] = [0.0, 0.0, 0.0];
/// `Screen1` camera look direction, **relative** to [`CAMERA_POSITION`]: with
/// `CamLookAtInWorldSpace = false` it is added to the position (`P9Scene.js:573`).
pub const CAMERA_LOOK_AT: [f32; 3] = [0.0, 0.0, 1.0];
/// `Screen1` `CamLookAtInWorldSpace`.
pub const CAMERA_LOOK_AT_IN_WORLD_SPACE: bool = false;
/// `Screen1` field of view in degrees.
pub const CAMERA_FOV: f32 = 45.0;
/// `Screen1` near plane.
pub const CAMERA_NEAR: f32 = 0.1;
/// `Screen1` far plane.
pub const CAMERA_FAR: f32 = 1000.0;
/// `Screen1` `ScaleByAspect`.
pub const SCALE_BY_ASPECT: bool = false;
/// `RenderRect1` `TesselateW`.
pub const RENDER_RECT1_TESSELATE_W: u32 = 1;
/// `RenderRect1` `TesselateH`.
pub const RENDER_RECT1_TESSELATE_H: u32 = 1;
/// `RenderRect2` `TesselateW`.
pub const RENDER_RECT2_TESSELATE_W: u32 = 1;
/// `RenderRect2` `TesselateH`.
pub const RENDER_RECT2_TESSELATE_H: u32 = 1;
/// `Clear1` colour (opaque black).
// The fidelity contract pins this clear colour as a compact literal.
#[rustfmt::skip]
pub const CLEAR1: wgpu::Color = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
/// `Clear2` colour (opaque black).
// The fidelity contract pins this clear colour as a compact literal.
#[rustfmt::skip]
pub const CLEAR2: wgpu::Color = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
/// `RenderToTexture1` format (`Format 1` = RGBA8).
pub const RTT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// `RenderToTexture1` `Width`/`Height` mode (`0` = 100%).
pub const RTT_MODE: u32 = 0;
/// `RenderToTexture1` `CreateMipMaps`: recorded but inert (single-mip, all
/// sampling is LOD 0, no mip-generation pass exists).
pub const RTT_MIPS: bool = true;
/// `StoreTexture1` `SwapDelay`.
pub const SWAP_DELAY: f32 = 0.0;
/// The four average ring radii.
pub const RADII: [f32; 4] = [2.0, 4.0, 8.0, 16.0];
/// The four variance ring radii: the fourth uses `10` while its average was
/// taken at `16`, exactly as the XML does.
pub const VARIANCE_RADII: [f32; 4] = [2.0, 4.0, 8.0, 10.0];
/// The per-ring thresholds.
pub const THRESHOLDS: [f32; 4] = [0.50, 0.55, 0.60, 0.65];
/// Variance weight in the update.
pub const VARIANCE_WEIGHT: f32 = 0.2;
/// Per-tap change step (`+0.005` above the threshold, `-0.005` below).
pub const CHANGE_STEP: f32 = 0.005;
/// Previous-frame retention in the update.
pub const RETENTION: f32 = 0.79;
/// `gFrameNr < 2` seeds the reaction instead of advancing it.
pub const SEED_FRAMES: u32 = 2;
/// Expression1's `permrand` (`in3.x`), the repository convention.
pub const PERMRAND: f32 = 1.0;
/// `Shader2` `Color1`, the composite tint.
pub const TINT: [f32; 4] = [0.8, 0.9, 0.9, 1.0];
/// `Shader1` `DepthTest`.
pub const SHADER1_DEPTH_TEST: bool = true;
/// `Shader1` `DepthWrite`.
pub const SHADER1_DEPTH_WRITE: bool = true;
/// `Shader1` `SrcBlend`.
pub const SHADER1_SRC_BLEND: u32 = 1;
/// `Shader1` `DstBlend`.
pub const SHADER1_DST_BLEND: u32 = 0;
/// `Shader1` `CullMode` (the cull mode is inert on the full-screen rectangle).
pub const SHADER1_CULL: u32 = 2;
/// `Shader2` `DepthTest`.
pub const SHADER2_DEPTH_TEST: bool = true;
/// `Shader2` `DepthWrite`.
pub const SHADER2_DEPTH_WRITE: bool = true;
/// `Shader2` `SrcBlend`.
pub const SHADER2_SRC_BLEND: u32 = 1;
/// `Shader2` `DstBlend`.
pub const SHADER2_DST_BLEND: u32 = 0;
/// `Shader2` `CullMode` (the cull mode is inert on the full-screen rectangle).
pub const SHADER2_CULL: u32 = 2;

fn seed_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| format!("{P9_COMMON_WGSL}\n{PLANE9_BATCH_WGSL}\n{SEED}"))
}

fn evolve_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| format!("{P9_COMMON_WGSL}\n{PLANE9_BATCH_WGSL}\n{EVOLVE}"))
}

const SEED: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let p = tex * vec2<f32>(2.0) - vec2<f32>(1.0);
  let vc = p9_perm4_vec2(p * vec2<f32>(0.15) + u.in3.xx).r;
  // XML 154 is `return vec4(step(vc,0.5));`: a scalar broadcast into red,
  // green, blue and alpha alike.
  let s = step(vc, 0.5);
  return vec4<f32>(s, s, s, s);
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
    co = co + abs(src - avg);
    if (src > lim) {
      change = change + 0.005;
    } else {
      change = change - 0.005;
    }
  }
  return P9EvalVar(co / 9.0, change);
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

  let v1 = p9_eval_var(tex, tp, 2.0, a1, 0.5);
  let v2 = p9_eval_var(tex, tp, 4.0, a2, 0.55);
  let v3 = p9_eval_var(tex, tp, 8.0, a3, 0.6);
  let v4 = p9_eval_var(tex, tp, 10.0, a4, 0.65);

  let next = ((v1.value + v2.value + v3.value + v4.value) * 0.2
    + (v1.change + v2.change + v3.change + v4.change)
    + c) * 0.79;
  return vec4<f32>(vec3<f32>(next), 1.0);
}
"#;

// `Shader2`'s vertex stage is `so.diffuse = iColor * gColor1 * gColor`; the
// shared full-screen pass supplies white `iColor`/`gColor`, so the fragment's
// `texture(gTexture1, si.tex) * si.diffuse` reduces to `* u.color1`. SrcBlend
// 1 / DstBlend 0 is One/Zero (replace), so the presented alpha is the sampled
// alpha times `color1.a` and is left untouched here.
const PRESENT: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let tinted = textureSampleLevel(g_texture1, g_sampler, tex, 0.0) * u.color1;
  return tinted;
}
"#;

struct Gpu {
    seed: Plane9Pass,
    evolve: Plane9Pass,
    present: Plane9Pass,
    feedback: Plane9Feedback,
    sampler: wgpu::Sampler,
}

/// `Plane9 Cell Culture` scene.
pub struct CellCulture {
    audio: P10Audio,
    permrand: f32,
    elapsed: f32,
    frame: u32,
    lens: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    uniforms: Plane9Uniforms,
    gpu: Option<Gpu>,
}

impl Default for CellCulture {
    fn default() -> Self {
        Self::new()
    }
}

impl CellCulture {
    /// Create the scene.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            permrand: PERMRAND,
            elapsed: 0.0,
            frame: 0,
            lens: 1.0,
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

    /// The `Screen1` projection with the user-facing Lens applied.
    ///
    /// The preset's only geometry is a full-screen rectangle, so the reaction
    /// passes never project it; this records the XML framing and keeps the
    /// Lens lever meaningful.
    pub fn projection(&self) -> Mat4 {
        apply_lens(
            perspective(CAMERA_FOV.to_radians(), 1.0, CAMERA_NEAR, CAMERA_FAR),
            self.lens,
        )
    }

    /// Bind the modV props (this preset declares no `sceneProps`; the six
    /// responsiveness levers are read by the engine schema, and Lens is the
    /// only value to bind).
    pub fn apply_props(&mut self, mut props: impl FnMut(&str) -> Option<f32>) {
        self.lens = lens_value(props("lens"));
    }

    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        let feedback = Plane9Feedback::new(
            &device,
            "point10-plane9-cell-culture-feedback",
            self.size.0,
            self.size.1,
            RTT_FORMAT,
        );
        let sampler = plane9_sampler(&device, "point10-plane9-cell-culture-sampler");
        let seed = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-cell-culture-seed",
            seed_wgsl(),
            None,
        );
        let evolve = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-cell-culture-evolve",
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
            "point10-plane9-cell-culture-present",
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
        self.uniforms.color1 = TINT;
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

impl Point10Scene for CellCulture {
    fn name(&self) -> &'static str {
        "Plane9 Cell Culture"
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
        // The evolve pass samples `previous_view` and draws `current_view`.
        // Keeping it textually first also keeps the seed target's current
        // view after the previous-view sample.
        if self.frame >= SEED_FRAMES {
            gpu.evolve
                .rebind_texture(gpu.feedback.previous_view(), &gpu.sampler);
            gpu.evolve.upload(&self.uniforms);
            gpu.evolve.draw(
                encoder,
                gpu.feedback.current_view(),
                CLEAR1,
                "point10-plane9-cell-culture-evolve",
            );
        }
        if self.frame < SEED_FRAMES {
            gpu.seed.draw(
                encoder,
                gpu.feedback.current_view(),
                CLEAR2,
                "point10-plane9-cell-culture-seed",
            );
        }
        gpu.present
            .rebind_texture(gpu.feedback.current_view(), &gpu.sampler);
        gpu.present.upload(&self.uniforms);
        gpu.present.draw(
            encoder,
            target,
            CLEAR1,
            "point10-plane9-cell-culture-present",
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
        plane9_scene_props(&[])
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Cell Culture", "3d-scenes")
    }
}
