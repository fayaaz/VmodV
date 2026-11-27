//! Plane9 "Blob Fade" — Rust/wgpu port of `Other/Blob Fade.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Other/Blob Fade.xml` by Joakim
//! Dahl, CC BY-NC-SA 3.0. A 7×7 grid of 1x1 planes, each pulsing and fading
//! with the bass band, seen from `z = 8.3`.
//!
//! # The graph
//!
//! ```text
//! Expression1 (out1 = perm + permrand*10)
//!   -> CloneExpression1.In1   (7 × 7, AdjustForAspect, Center)
//!       -> RenderObject2 (Pos/Rot/Scale = defaults) -> Shader1
//! ```
//!
//! # Ported exactly
//!
//! * `dd = sin(in1 + band(-1,0,1,0) * 2 + rand() * 5) + 0.2`,
//!   `col.r = dd * 2 + 0.5`, `col.b = col.g = dd`;
//! * the clone positions `pos.x = x`, `pos.y = y`, `pos.z = 0` (PlanE9 clone
//!   coords are 1-based) recentred by `Center`;
//! * `ApplyForAspect` rescaling the clone counts (`nx *= aspect` or
//!   `ny /= aspect`), fixed at build;
//! * the shader `oColor = diffuse * smoothstep(0, 0.15, 1 - length(p)) *
//!   0.5`, including the alpha multiply;
//! * the expression `perm += (beat(0.1) * 10 + 0.2) * dt`,
//!   `out1 = perm + permrand * 10`.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::plane9_common::{
    Plane9QuadInstance, Plane9QuadPass, Plane9QuadUniforms, plane9_camera,
};
use crate::point10::shared::random::seeded_rand;
use crate::point10::{
    HDR_FORMAT, Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, scene_props_with_levers,
};

/// `Screen1` camera position.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, 8.3];
/// `Screen1` camera look direction.
pub const CAMERA_LOOK: [f32; 3] = [0.0, 0.0, -1.0];
/// `Clear1` colour (opaque black).
pub const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
/// `CloneExpression1` `NumObjectsX`.
pub const NUM_OBJECTS_X: u32 = 7;
/// `CloneExpression1` `NumObjectsY`.
pub const NUM_OBJECTS_Y: u32 = 7;
/// `CloneExpression1` `In1` default (fed by Expression1 in the live graph).
pub const IN1_DEFAULT: f32 = 8.37275;
/// `permrand` seed.
pub const PERM_RAND: f32 = 1.0;

const BLOB_FADE_PROPS: &[SceneProp] = &[];

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

    /// Instance count.
    pub fn count(&self) -> u32 {
        self.nx * self.ny
    }
}

/// The `Expression1` / `CloneExpression1` pair's live values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlobFadeDrive {
    /// `perm` for the next frame.
    pub perm: f32,
    /// `out1 = perm + permrand * 10`.
    pub out1: f32,
}

/// Pure port of `Expression1`.
pub fn blob_fade_drive(beat: f32, dt: f32, perm: f32) -> BlobFadeDrive {
    let beat = if beat.is_finite() { beat.max(0.0) } else { 0.0 };
    let step = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    let perm = if perm.is_finite() { perm } else { 1.0 };
    let next = perm + (beat * 10.0 + 0.2) * step;
    BlobFadeDrive {
        perm: next,
        out1: next + PERM_RAND * 10.0,
    }
}

/// Build the clone instances for one frame.
///
/// `bass` is `band(-1, 0, 1, 0)` (the raw mid band), `in1` is the connected
/// `out1`.
pub fn blob_fade_instances(grid: CloneGrid, in1: f32, bass: f32) -> Vec<Plane9QuadInstance> {
    let bass = if bass.is_finite() { bass } else { 0.0 };
    let midpoint_x = (grid.nx as f32 + 1.0) * 0.5;
    let midpoint_y = (grid.ny as f32 + 1.0) * 0.5;
    let mut instances = Vec::with_capacity(grid.count() as usize);
    for y in 0..grid.ny {
        for x in 0..grid.nx {
            let index = y * grid.nx + x;
            let random = seeded_rand((index as i32).wrapping_mul(7919), 1);
            let dd = (in1 + bass * 2.0 + random * 5.0).sin() + 0.2;
            instances.push(Plane9QuadInstance {
                position: [
                    (x as f32 + 1.0) - midpoint_x,
                    (y as f32 + 1.0) - midpoint_y,
                    0.0,
                ],
                _pad: 0.0,
                color: [dd * 2.0 + 0.5, dd, dd, 1.0],
            });
        }
    }
    instances
}

struct BlobFadeGpu {
    pass: Plane9QuadPass,
    grid: CloneGrid,
    instances: Vec<Plane9QuadInstance>,
}

/// `Plane9 Blob Fade` scene.
pub struct BlobFade {
    audio: P10Audio,
    perm: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    uniforms: Plane9QuadUniforms,
    gpu: Option<BlobFadeGpu>,
}

impl Default for BlobFade {
    fn default() -> Self {
        Self::new()
    }
}

impl BlobFade {
    /// Create the scene.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            perm: 1.0,
            size: (1, 1),
            target_format: HDR_FORMAT,
            device: None,
            queue: None,
            uniforms: Plane9QuadUniforms::new(),
            gpu: None,
        }
    }

    /// The current `perm`.
    pub fn perm(&self) -> f32 {
        self.perm
    }

    /// The uniform block last uploaded.
    pub fn uniforms(&self) -> &Plane9QuadUniforms {
        &self.uniforms
    }

    /// The current clone grid.
    pub fn grid(&self) -> CloneGrid {
        CloneGrid::for_aspect(self.size.0 as f32 / self.size.1.max(1) as f32)
    }

    /// Bind the modV props. These presets declare no `sceneProps` of their
    /// own, so this only exists to satisfy the registry's prop binder.
    pub fn apply_props(&mut self, _props: impl FnMut(&str) -> Option<f32>) {}

    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        let grid = self.grid();
        let pass = Plane9QuadPass::new(
            &device,
            &queue,
            self.target_format,
            "point10-plane9-blob-fade",
            BLOB_FADE_FRAGMENT,
            grid.count(),
        );
        self.gpu = Some(BlobFadeGpu {
            pass,
            grid,
            instances: Vec::new(),
        });
        self.uniforms.view_proj = plane9_camera(
            self.size.0 as f32 / self.size.1.max(1) as f32,
            CAMERA_POSITION,
            CAMERA_LOOK,
            45.0,
            0.1,
            1000.0,
        );
        if let Some(gpu) = &self.gpu {
            gpu.pass.upload_uniforms(&self.uniforms);
        }
    }
}

const BLOB_FADE_FRAGMENT: &str = r#"
@fragment
fn fs_main(in: Plane9QuadOut) -> @location(0) vec4<f32> {
  let p = -1.0 + 2.0 * in.tex;
  return in.diffuse * p9_smoothstep(0.0, 0.15, 1.0 - length(p)) * 0.5;
}
"#;

impl Point10Scene for BlobFade {
    fn name(&self) -> &'static str {
        "Plane9 Blob Fade"
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
        let drive = blob_fade_drive(self.audio.beat(0.1), dt, self.perm);
        self.perm = drive.perm;
        let bass = self.audio.band(-1, 0, 1, 0.0);
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let instances = blob_fade_instances(gpu.grid, drive.out1, bass);
        self.uniforms.in1 = [drive.out1, 0.0, 0.0, 0.0];
        gpu.pass.upload_uniforms(&self.uniforms);
        gpu.pass.upload_instances(&instances);
        gpu.instances = instances;
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        gpu.pass.draw(
            encoder,
            target,
            CLEAR,
            gpu.instances.len() as u32,
            "point10-plane9-blob-fade",
        );
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(BLOB_FADE_PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Blob Fade", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn camera_clear_and_grid_match_the_preset() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, 8.3]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, -1.0]);
        assert_eq!(CLEAR.a, 1.0);
        assert_eq!(CloneGrid::for_aspect(1.0), CloneGrid { nx: 7, ny: 7 });
        assert_eq!(CloneGrid::for_aspect(2.0).nx, 14);
        assert_eq!(CloneGrid::for_aspect(0.5).ny, 14);
    }

    #[test]
    fn drive_matches_the_expression() {
        let drive = blob_fade_drive(0.5, 0.5, 1.0);
        assert!((drive.perm - (1.0 + (5.0 + 0.2) * 0.5)).abs() < 1e-5);
        assert!((drive.out1 - (drive.perm + 10.0)).abs() < 1e-5);
    }

    #[test]
    fn instances_are_centred_and_coloured() {
        let grid = CloneGrid { nx: 7, ny: 7 };
        let instances = blob_fade_instances(grid, 11.0, 0.6);
        assert_eq!(instances.len(), 49);
        let xs: Vec<f32> = instances
            .iter()
            .map(|instance| instance.position[0])
            .collect();
        let min = xs.iter().copied().fold(f32::INFINITY, f32::min);
        let max = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!((min + max).abs() < 1e-6, "not centred: {min} {max}");
        // The colour expression is deterministic and sets alpha to 1.
        assert_eq!(instances[0].color[3], 1.0);
        assert!(
            instances
                .iter()
                .all(|instance| instance.color[1] == instance.color[2])
        );
    }

    #[test]
    fn wgsl_parses_and_keeps_the_radial_fade() {
        let source = format!(
            "{}\n{}\n{BLOB_FADE_FRAGMENT}",
            crate::point10::shared::p9_glsl::P9_COMMON_WGSL,
            crate::point10::scenes::plane9_common::PLANE9_QUAD_UNIFORMS_WGSL,
        );
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("Blob Fade WGSL failed to parse: {error}"));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("Blob Fade WGSL validates");
        assert!(BLOB_FADE_FRAGMENT.contains("p9_smoothstep(0.0, 0.15, 1.0 - length(p))"));
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = BlobFade::new();
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
