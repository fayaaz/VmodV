//! Plane9 "Color Beams" — Rust/wgpu port of `Abstract/Color Beams.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Abstract/Color Beams.xml` by Joakim
//! Dahl, CC BY-NC-SA 4.0.
//!
//! # The graph
//!
//! ```text
//! Clear2 + CloneExpression1 (14x14 of Shader2) -> RenderToTexture1 (RGBA8)
//!   -> Shader1 (40-sample radial shafts) -> RenderToTexture2 (HDR)
//!     -> Bloom1 -> Shader3 (ACES tonemap) -> RenderRect2 -> Screen1
//! ```
//!
//! `Clear1` belongs to the clear of `RenderRect1` (the RTT2 chain); `Clear3`
//! clears the screen.
//!
//! # Ported exactly
//!
//! * `CamPos 0 0 15.1`, `CamLookAt 0 0 -1`;
//! * the per-clone expression (`nr`, `v = noise3(x*4, y*4, time*0.5)`,
//!   `pos = (x*1.75, y*2, 0)`, `r`, `d = noise3(x*14, y*14, time*0.5)`,
//!   `m`/`col`, `scale` from `noise2(time*0.4, nr*2)`) with 1-based clone
//!   coordinates and `Center`;
//! * the 40-sample shaft loop with `density 0.6`, `weight 0.2`,
//!   `decay 0.985` and the 4×4 ordered dither;
//! * the Bloom `(5, 0, 0, 1, false)` driver and the ACES tonemap.
//!
//! `Bloom1` reaches one mip at these parameters, so its `Intensity` is inert —
//! faithful to the Plane9 driver.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::postfx::{Plane9BloomParams, Plane9BloomPass};
use crate::point10::scenes::plane9_common::{
    PLANE9_TONEMAP_WGSL, Plane9Pass, Plane9QuadInstance, Plane9QuadPass, Plane9QuadUniforms,
    Plane9TextureBinding, Plane9Uniforms, create_colour_target, plane9_camera_lens, plane9_sampler,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::random::{seeded_rand, value_noise2, value_noise3};
use crate::point10::{
    HDR_FORMAT, Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, lens_value,
    plane9_scene_props,
};

/// `Screen1` camera position.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, 15.1];
/// `Screen1` camera look direction.
pub const CAMERA_LOOK: [f32; 3] = [0.0, 0.0, -1.0];
/// `Clear1`/`Clear2` colour (transparent black for both RTTs).
pub const CLEAR_RTT: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};
/// `Clear3` colour (opaque black, the screen).
pub const CLEAR_SCREEN: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
/// `CloneExpression1` `NumObjectsX`.
pub const NUM_OBJECTS_X: u32 = 14;
/// `CloneExpression1` `NumObjectsY`.
pub const NUM_OBJECTS_Y: u32 = 14;
/// `CloneExpression1` `NumObjectsZ`.
pub const NUM_OBJECTS_Z: u32 = 1;
/// `CloneExpression1` `Center`.
pub const CENTER: bool = true;
/// `CloneExpression1` `AdjustForAspect`.
pub const ADJUST_FOR_ASPECT: bool = true;
/// `RenderToTexture1` (clone grid) format (Format 1 = RGBA8).
pub const RTT1_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// `RenderToTexture2` (shafts) format (Format 5 = half-float).
pub const RTT2_FORMAT: wgpu::TextureFormat = HDR_FORMAT;
/// Shaft-loop sample count.
pub const NUM_SAMPLES: u32 = 40;
/// Shaft-loop density.
pub const DENSITY: f32 = 0.6;
/// Shaft-loop per-sample weight.
pub const WEIGHT: f32 = 0.2;
/// Shaft-loop illumination decay.
pub const DECAY: f32 = 0.985;
/// `Bloom1` `Radius`.
pub const BLOOM_RADIUS: f32 = 5.0;
/// `Bloom1` `Threshold`.
pub const BLOOM_THRESHOLD: f32 = 0.0;
/// `Bloom1` `ThresholdSmoothness`.
pub const BLOOM_SMOOTHNESS: f32 = 0.0;
/// `Bloom1` `Intensity`.
pub const BLOOM_INTENSITY: f32 = 1.0;
/// `Bloom1` `Antiflicker`.
pub const BLOOM_ANTIFLICKER: bool = false;
/// The per-clone grid spacing.
pub const GRID_SPACING: [f32; 2] = [1.75, 2.0];

const PROPS: &[SceneProp] = &[];

/// The aspect-adjusted clone counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloneGrid {
    /// Columns.
    pub nx: u32,
    /// Rows.
    pub ny: u32,
}

impl CloneGrid {
    /// `AdjustForAspect`: `aspect > 1` scales `nx`, otherwise `ny / aspect`.
    pub fn for_aspect(aspect: f32) -> Self {
        let aspect = if aspect.is_finite() && aspect > 0.0 {
            aspect
        } else {
            1.0
        };
        let mut nx = NUM_OBJECTS_X;
        let mut ny = NUM_OBJECTS_Y;
        if aspect > 1.0 {
            nx = ((nx as f32 * aspect).round() as u32).max(1);
        } else {
            ny = ((ny as f32 / aspect).round() as u32).max(1);
        }
        Self { nx, ny }
    }

    /// Instance count (`NumObjectsZ = 1`).
    pub fn count(&self) -> u32 {
        self.nx * self.ny
    }
}

/// Build the clone instances for one frame.
pub fn color_beams_instances(grid: CloneGrid, time: f32) -> Vec<Plane9QuadInstance> {
    let count = grid.count();
    let total = count as f32;
    let mid_x = (1.0 + grid.nx as f32) * GRID_SPACING[0] * 0.5;
    let mid_y = (1.0 + grid.ny as f32) * GRID_SPACING[1] * 0.5;
    let mut instances = Vec::with_capacity(count as usize);
    for y in 1..=grid.ny {
        for x in 1..=grid.nx {
            let index = (y - 1) * grid.nx + (x - 1);
            let nr = (x as f32 + y as f32 * grid.nx as f32) / total;
            let v = value_noise3(x as f32 * 4.0, y as f32 * 4.0, time * 0.5) * 0.25 + 0.75;
            let r = 0.25 + seeded_rand((index as i32).wrapping_mul(7919), 1) * 0.75;
            let d = value_noise3(x as f32 * 14.0, y as f32 * 14.0, time * 0.5);
            let m = if d > 0.9 { 1.0 } else { 0.1 };
            let red = (if d > 0.9 { 1.0 } else { 0.0 }) * m;
            let green = (x as f32 + 1.0) / grid.nx as f32 * v * r * m;
            let blue = (y as f32 + 1.0) / grid.ny as f32 * v * r * m;
            let scale_x = value_noise2(time * 0.4, nr * 2.0) * 0.5 + 0.6;
            let scale_y = value_noise2(time * 0.4, nr * 2.0 + std::f32::consts::PI) * 0.5 + 0.6;
            instances.push(Plane9QuadInstance {
                position: [
                    x as f32 * GRID_SPACING[0] - mid_x,
                    y as f32 * GRID_SPACING[1] - mid_y,
                    0.0,
                ],
                _pad: 0.0,
                color: [red, green, blue, 1.0],
                scale: [scale_x, scale_y],
            });
        }
    }
    instances
}

fn shafts_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| format!("{P9_COMMON_WGSL}\n{SHAFTS}"))
}

fn present_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| format!("{P9_COMMON_WGSL}\n{PLANE9_TONEMAP_WGSL}\n{PRESENT}"))
}

const SHAFTS: &str = r#"
fn p9_dither(x: i32, y: i32) -> f32 {
  // GLSL `mat4 ditherPattern` is column-major: `ditherPattern[x][y]` is
  // column x, row y, i.e. source order `x*4 + y`.
  let values = array<f32, 16>(
    0.0, 0.5, 0.125, 0.625,
    0.75, 0.22, 0.875, 0.375,
    0.1875, 0.6875, 0.0625, 0.5625,
    0.9375, 0.4375, 0.8125, 0.3125,
  );
  return values[u32(x % 4) * 4u + u32(y % 4)];
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let light_pos_tex = u.in1.xy;
  let density = 0.6;
  let weight = 0.2;
  let decay = 0.985;

  let delta_tex_coord = (tex - light_pos_tex) / 40.0 * density;
  let start_offset = p9_dither(i32(position.x), i32(position.y)) * delta_tex_coord;

  var c = textureSampleLevel(g_texture1, g_sampler, tex, 0.0).rgb;
  var illumination_decay = 1.0;
  for (var i = 0; i < 40; i = i + 1) {
    let uv = tex - delta_tex_coord * f32(i) - start_offset;
    c = c + textureSampleLevel(g_texture1, g_sampler, uv, 0.0).rgb
      * illumination_decay * weight;
    illumination_decay = illumination_decay * decay;
  }
  return vec4<f32>(c, 1.0);
}
"#;

const CLONE_FRAGMENT: &str = r#"
@fragment
fn fs_main(in: Plane9QuadOut) -> @location(0) vec4<f32> {
  return in.diffuse;
}
"#;

const PRESENT: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let c1 = textureSampleLevel(g_texture1, g_sampler, tex, 0.0);
  return vec4<f32>(p9_tonemap_aces(c1.rgb), 1.0);
}
"#;

struct Gpu {
    clones: Plane9QuadPass,
    grid: CloneGrid,
    instances: Vec<Plane9QuadInstance>,
    clone_uniforms: Plane9QuadUniforms,
    shafts: Plane9Pass,
    shafts_uniforms: Plane9Uniforms,
    present: Plane9Pass,
    present_uniforms: Plane9Uniforms,
    bloom: Plane9BloomPass,
    _rtt1: wgpu::Texture,
    _rtt1_view: wgpu::TextureView,
    _rtt2: wgpu::Texture,
    _rtt2_view: wgpu::TextureView,
    _sampler: wgpu::Sampler,
}

/// `Plane9 Color Beams` scene.
pub struct ColorBeams {
    audio: P10Audio,
    permrand: f32,
    elapsed: f32,
    lens: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    gpu: Option<Gpu>,
}

impl Default for ColorBeams {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorBeams {
    /// Create the scene.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            permrand: 1.0,
            elapsed: 0.0,
            lens: 1.0,
            size: (1, 1),
            target_format: HDR_FORMAT,
            device: None,
            queue: None,
            gpu: None,
        }
    }

    /// The current `permrand`.
    pub fn permrand(&self) -> f32 {
        self.permrand
    }

    /// The current clone grid.
    pub fn grid(&self) -> CloneGrid {
        CloneGrid::for_aspect(self.size.0 as f32 / self.size.1.max(1) as f32)
    }

    /// Bind the shared Lens prop (this preset declares no other `sceneProps`).
    pub fn apply_props(&mut self, mut props: impl FnMut(&str) -> Option<f32>) {
        self.lens = lens_value(props("lens"));
        if let Some(gpu) = &mut self.gpu {
            gpu.clone_uniforms.view_proj = plane9_camera_lens(
                self.size.0 as f32 / self.size.1.max(1) as f32,
                CAMERA_POSITION,
                CAMERA_LOOK,
                45.0,
                0.1,
                1000.0,
                self.lens,
            );
            gpu.clones.upload_uniforms(&gpu.clone_uniforms);
        }
    }

    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        let grid = self.grid();
        let (rtt1, rtt1_view) = create_colour_target(
            &device,
            "point10-plane9-color-beams-clones",
            self.size.0,
            self.size.1,
            RTT1_FORMAT,
        );
        let (rtt2, rtt2_view) = create_colour_target(
            &device,
            "point10-plane9-color-beams-shafts",
            self.size.0,
            self.size.1,
            RTT2_FORMAT,
        );
        let sampler = plane9_sampler(&device, "point10-plane9-color-beams-sampler");

        let clones = Plane9QuadPass::new(
            &device,
            &queue,
            RTT1_FORMAT,
            "point10-plane9-color-beams-clones",
            CLONE_FRAGMENT,
            grid.count(),
        );
        let shafts = Plane9Pass::new(
            &device,
            &queue,
            RTT2_FORMAT,
            "point10-plane9-color-beams-shafts",
            shafts_wgsl(),
            Some(Plane9TextureBinding::D2 {
                view: &rtt1_view,
                sampler: &sampler,
            }),
        );
        let mut bloom = Plane9BloomPass::new(
            &device,
            &queue,
            self.size.0,
            self.size.1,
            Plane9BloomParams::new(
                BLOOM_RADIUS,
                BLOOM_THRESHOLD,
                BLOOM_SMOOTHNESS,
                BLOOM_INTENSITY,
                BLOOM_ANTIFLICKER,
            ),
        );
        bloom.set_input(&rtt2_view);
        let Some(bloom_view) = bloom.composite_view() else {
            return;
        };
        let present = Plane9Pass::new(
            &device,
            &queue,
            self.target_format,
            "point10-plane9-color-beams-present",
            present_wgsl(),
            Some(Plane9TextureBinding::D2 {
                view: bloom_view,
                sampler: &sampler,
            }),
        );

        let mut clone_uniforms = Plane9QuadUniforms::new();
        clone_uniforms.view_proj = plane9_camera_lens(
            self.size.0 as f32 / self.size.1.max(1) as f32,
            CAMERA_POSITION,
            CAMERA_LOOK,
            45.0,
            0.1,
            1000.0,
            self.lens,
        );
        let shafts_uniforms = Plane9Uniforms::new(self.size.0, self.size.1);
        let present_uniforms = Plane9Uniforms::new(self.size.0, self.size.1);
        let instances = color_beams_instances(grid, self.elapsed);
        clones.upload_uniforms(&clone_uniforms);
        clones.upload_instances(&instances);

        self.gpu = Some(Gpu {
            clones,
            grid,
            instances,
            clone_uniforms,
            shafts,
            shafts_uniforms,
            present,
            present_uniforms,
            bloom,
            _rtt1: rtt1,
            _rtt1_view: rtt1_view,
            _rtt2: rtt2,
            _rtt2_view: rtt2_view,
            _sampler: sampler,
        });
    }
}

impl Point10Scene for ColorBeams {
    fn name(&self) -> &'static str {
        "Plane9 Color Beams"
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
        let band = self.audio.band(-1, 1, 0, 0.1);
        let step = band * 5.0;
        self.permrand += step * dt;
        let in1_x = value_noise2(self.permrand, 1.0) * 0.5 + 0.5;
        let in1_y = value_noise2(self.permrand, 2.0) * 0.5 + 0.5;

        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let instances = color_beams_instances(gpu.grid, self.elapsed);
        gpu.shafts_uniforms.time = self.elapsed;
        gpu.shafts_uniforms.in1 = [in1_x, in1_y, 0.0, 0.0];
        gpu.shafts.upload(&gpu.shafts_uniforms);
        gpu.present_uniforms.time = self.elapsed;
        gpu.present.upload(&gpu.present_uniforms);
        gpu.clone_uniforms.in1 = [self.elapsed, 0.0, 0.0, 0.0];
        gpu.clones.upload_uniforms(&gpu.clone_uniforms);
        gpu.clones.upload_instances(&instances);
        gpu.instances = instances;
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        gpu.clones.draw(
            encoder,
            &gpu._rtt1_view,
            CLEAR_RTT,
            gpu.instances.len() as u32,
            "point10-plane9-color-beams-clones",
        );
        gpu.shafts.draw(
            encoder,
            &gpu._rtt2_view,
            CLEAR_RTT,
            "point10-plane9-color-beams-shafts",
        );
        gpu.bloom.set_input(&gpu._rtt2_view);
        gpu.bloom.encode(encoder);
        gpu.present.draw(
            encoder,
            target,
            CLEAR_SCREEN,
            "point10-plane9-color-beams-present",
        );
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
        self.elapsed = 0.0;
    }

    fn props(&self) -> Vec<SceneProp> {
        plane9_scene_props(PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Color Beams", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::scenes::plane9_common::{
        PLANE9_QUAD_UNIFORMS_WGSL, PLANE9_TEXTURE_WGSL, PLANE9_UNIFORMS_WGSL,
    };
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn camera_grid_and_bloom_match_the_preset() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, 15.1]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, -1.0]);
        assert_eq!(CLEAR_RTT.a, 0.0);
        assert_eq!(CLEAR_SCREEN.a, 1.0);
        assert_eq!(CloneGrid::for_aspect(1.0), CloneGrid { nx: 14, ny: 14 });
        assert_eq!(CloneGrid::for_aspect(16.0 / 9.0).nx, 25);
        assert_eq!(CloneGrid::for_aspect(0.5).ny, 28);
        assert_eq!(NUM_SAMPLES, 40);
        assert_eq!(DENSITY, 0.6);
        assert_eq!(WEIGHT, 0.2);
        assert_eq!(DECAY, 0.985);
        assert_eq!(RTT1_FORMAT, wgpu::TextureFormat::Rgba8Unorm);
        assert_eq!(RTT2_FORMAT, HDR_FORMAT);
        let params = Plane9BloomParams::new(
            BLOOM_RADIUS,
            BLOOM_THRESHOLD,
            BLOOM_SMOOTHNESS,
            BLOOM_INTENSITY,
            BLOOM_ANTIFLICKER,
        );
        assert_eq!(params.radius(), 5.0);
        assert_eq!(params.threshold(), 0.0);
        assert_eq!(params.smoothness(), 0.0);
        assert_eq!(params.intensity(), 1.0);
        assert!(!params.antiflicker());
        // One mip at these parameters: `Intensity` is inert.
        assert_eq!(params.mip_count(), 1);
    }

    #[test]
    fn clone_instances_follow_the_expression() {
        let grid = CloneGrid { nx: 14, ny: 14 };
        let instances = color_beams_instances(grid, 0.0);
        assert_eq!(instances.len(), 196);
        // `Center` subtracts (min+max)/2 per axis.
        let xs: Vec<f32> = instances.iter().map(|i| i.position[0]).collect();
        let min = xs.iter().copied().fold(f32::INFINITY, f32::min);
        let max = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!((min + max).abs() < 1e-5, "x not centred: {min} {max}");
        // x = 1 maps to 1.75 - 13.125.
        assert!((instances[0].position[0] - (1.75 - 13.125)).abs() < 1e-5);
        // The scale lane is the `noise2(time*0.4, nr*2)` pair, not 1.
        assert!(instances.iter().all(|i| (0.1..=1.1).contains(&i.scale[0])));
        assert!(instances.iter().all(|i| (0.1..=1.1).contains(&i.scale[1])));
        // Every green/blue lane stays in 0..1 for a default field.
        assert!(
            instances
                .iter()
                .all(|i| (0.0..=1.0).contains(&i.color[1]) && (0.0..=1.0).contains(&i.color[2]))
        );
    }

    #[test]
    fn shaders_keep_the_shafts_and_dither() {
        let shafts = shafts_wgsl();
        for needle in [
            "let density = 0.6;",
            "let weight = 0.2;",
            "let decay = 0.985;",
            "let delta_tex_coord = (tex - light_pos_tex) / 40.0 * density;",
            "0.1875, 0.6875, 0.0625, 0.5625",
            // `mat4` is column-major: x selects the column, y the row.
            "values[u32(x % 4) * 4u + u32(y % 4)]",
            "p9_dither(i32(position.x), i32(position.y))",
            "tex - delta_tex_coord * f32(i) - start_offset",
            "illumination_decay * weight",
            "illumination_decay = illumination_decay * decay;",
        ] {
            assert!(shafts.contains(needle), "lost {needle}");
        }
        assert!(PRESENT.contains("p9_tonemap_aces(c1.rgb)"));
        assert!(CLONE_FRAGMENT.contains("return in.diffuse;"));
    }

    #[test]
    fn shaders_validate() {
        let clone_source =
            format!("{P9_COMMON_WGSL}\n{PLANE9_QUAD_UNIFORMS_WGSL}\n{CLONE_FRAGMENT}");
        let present_source = format!(
            "{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{}",
            present_wgsl()
        );
        let sources = [
            format!(
                "{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{}",
                shafts_wgsl()
            ),
            clone_source,
            present_source,
        ];
        for source in sources {
            let module = naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|error| panic!("Color Beams WGSL failed to parse: {error}"));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("Color Beams WGSL validates");
        }
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = ColorBeams::new();
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
