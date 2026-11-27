//! Plane9 "Circle Crossover" — Rust/wgpu port of
//! `Psychedelic/Circle Crossover.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Psychedelic/Circle Crossover.xml`
//! by Joakim Dahl, CC BY-NC-SA 3.0. An 18×35 grid of offset, coloured 1x1
//! planes; each fragment is an animated sine ring whose radius is warped by
//! FBM, with a hard `discard` outside `d > 1`.
//!
//! # The graph
//!
//! ```text
//! Expression1 (in1.x = perm + permrand*10)
//!   -> CloneExpression1   (18 × 35, AdjustForAspect, Center)
//!       -> RenderObject1 (defaults) -> Shader1
//! ```
//!
//! # Ported exactly
//!
//! * `pos.x = x + mod(y, 2) * 0.5`, `pos.y = y * 0.5`,
//!   `pos.z = -y * 0.001` (1-based clone coords) recentred by `Center`;
//! * `col.r = rand()`, `col.a = (x + y * nx) / (nx * ny)`;
//! * the fragment `d = length(p) + _fbmfast(vec3(p + diffuse.r * 10,
//!   gIn1.x), 4) * 0.2`, `if (d > 1) discard`, and the sine ring
//!   `sin(diffuse.r * 100 + d * 30 - gIn1.x) * smoothstep(1.0, 0.95, d)`;
//! * the expression `perm += (band(-1,0,0,0.01) * 10 + 0.01) * dt`,
//!   `in1.x = perm + permrand * 10`.
//!
//! # Known fidelity limit
//!
//! The XML enables depth testing and writing, but [`Plane9QuadPass`] has no
//! depth attachment: instances composite in submission order (near-to-far),
//! so where rings overlap the far row wins instead of the near one. Adding a
//! depth texture to the shared pass would close the gap without touching the
//! clone expressions.

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
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, 20.0];
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
pub const NUM_OBJECTS_X: u32 = 18;
/// `CloneExpression1` `NumObjectsY`.
pub const NUM_OBJECTS_Y: u32 = 35;
/// `permrand` seed.
pub const PERM_RAND: f32 = 1.0;

const CIRCLE_PROPS: &[SceneProp] = &[];

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

/// One frame of `Expression1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CircleCrossoverDrive {
    /// `perm` for the next frame.
    pub perm: f32,
    /// `in1.x = perm + permrand * 10`.
    pub in1_x: f32,
}

/// Pure port of the Circle Crossover expression.
pub fn circle_crossover_drive(bass: f32, dt: f32, perm: f32) -> CircleCrossoverDrive {
    let bass = if bass.is_finite() { bass.max(0.0) } else { 0.0 };
    let step = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    let perm = if perm.is_finite() { perm } else { 1.0 };
    let next = perm + (bass * 10.0 + 0.01) * step;
    CircleCrossoverDrive {
        perm: next,
        in1_x: next + PERM_RAND * 10.0,
    }
}

/// Build the clone instances for one frame.
pub fn circle_crossover_instances(grid: CloneGrid) -> Vec<Plane9QuadInstance> {
    let total = grid.count() as f32;
    let min_x = 1.0;
    let max_x = grid.nx as f32 + 0.5;
    let min_y = 0.5;
    let max_y = grid.ny as f32 * 0.5;
    let min_z = -(grid.ny as f32) * 0.001;
    let max_z = -0.001;
    let mid = [
        (min_x + max_x) * 0.5,
        (min_y + max_y) * 0.5,
        (min_z + max_z) * 0.5,
    ];
    let mut instances = Vec::with_capacity(grid.count() as usize);
    for y in 0..grid.ny {
        for x in 0..grid.nx {
            let index = y * grid.nx + x;
            let ix = x as f32 + 1.0;
            let iy = y as f32 + 1.0;
            let random = seeded_rand((index as i32).wrapping_mul(7919), 1);
            instances.push(Plane9QuadInstance {
                position: [
                    ix + ((y + 1) % 2) as f32 * 0.5 - mid[0],
                    iy * 0.5 - mid[1],
                    -iy * 0.001 - mid[2],
                ],
                _pad: 0.0,
                color: [random, 0.0, 0.0, (ix + iy * grid.nx as f32) / total],
                scale: [1.0, 1.0],
            });
        }
    }
    instances
}

struct CircleCrossoverGpu {
    pass: Plane9QuadPass,
    _grid: CloneGrid,
    instances: Vec<Plane9QuadInstance>,
}

/// `Plane9 Circle Crossover` scene.
pub struct CircleCrossover {
    audio: P10Audio,
    perm: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    uniforms: Plane9QuadUniforms,
    gpu: Option<CircleCrossoverGpu>,
}

impl Default for CircleCrossover {
    fn default() -> Self {
        Self::new()
    }
}

impl CircleCrossover {
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
            "point10-plane9-circle-crossover",
            CIRCLE_FRAGMENT,
            grid.count(),
        );
        self.gpu = Some(CircleCrossoverGpu {
            pass,
            _grid: grid,
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
        let instances = circle_crossover_instances(grid);
        if let Some(gpu) = &self.gpu {
            gpu.pass.upload_uniforms(&self.uniforms);
            gpu.pass.upload_instances(&instances);
        }
        if let Some(gpu) = &mut self.gpu {
            gpu.instances = instances;
        }
    }
}

const CIRCLE_FRAGMENT: &str = r#"
@fragment
fn fs_main(in: Plane9QuadOut) -> @location(0) vec4<f32> {
  let p = in.tex * vec2<f32>(2.0) - vec2<f32>(1.0);
  var d = length(p);
  d = d + p9_fbm(vec3<f32>(p + in.diffuse.r * 10.0, u.in1.x), 4u) * 0.2;
  if (d > 1.0) {
    discard;
  }
  var a = sin(in.diffuse.r * 100.0 + d * 30.0 - u.in1.x);
  a = a * p9_smoothstep(1.0, 0.95, d);
  return vec4<f32>(a, a, a, 1.0);
}
"#;

impl Point10Scene for CircleCrossover {
    fn name(&self) -> &'static str {
        "Plane9 Circle Crossover"
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
        let drive = circle_crossover_drive(self.audio.band(-1, 0, 0, 0.01), dt, self.perm);
        self.perm = drive.perm;
        self.uniforms.in1 = [drive.in1_x, 0.0, 0.0, 0.0];
        if let Some(gpu) = &self.gpu {
            gpu.pass.upload_uniforms(&self.uniforms);
        }
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
            "point10-plane9-circle-crossover",
        );
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(CIRCLE_PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Circle Crossover", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn camera_clear_and_grid_match_the_preset() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, 20.0]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, -1.0]);
        assert_eq!(CLEAR.a, 1.0);
        assert_eq!(CloneGrid::for_aspect(1.0), CloneGrid { nx: 18, ny: 35 });
        assert_eq!(CloneGrid::for_aspect(2.0).nx, 36);
        assert_eq!(CloneGrid::for_aspect(0.5).ny, 70);
    }

    #[test]
    fn drive_matches_the_expression() {
        let drive = circle_crossover_drive(0.4, 0.5, 1.0);
        assert!((drive.perm - (1.0 + (4.0 + 0.01) * 0.5)).abs() < 1e-5);
        assert!((drive.in1_x - (drive.perm + 10.0)).abs() < 1e-5);
    }

    #[test]
    fn instances_follow_the_offset_grid_and_are_centred() {
        let grid = CloneGrid { nx: 18, ny: 35 };
        let instances = circle_crossover_instances(grid);
        assert_eq!(instances.len(), 630);
        let xs: Vec<f32> = instances
            .iter()
            .map(|instance| instance.position[0])
            .collect();
        let min = xs.iter().copied().fold(f32::INFINITY, f32::min);
        let max = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!((min + max).abs() < 1e-5, "not centred: {min} {max}");
        // Row 1 is offset by half a cell (x = 1.5), row 2 is not (x = 1.0);
        // `Center` subtracts (min + max) / 2 = 9.75.
        assert!((instances[0].position[0] - (1.5 - 9.75)).abs() < 1e-5);
        assert!((instances[18].position[0] - (1.0 - 9.75)).abs() < 1e-5);
    }

    #[test]
    fn wgsl_parses_and_keeps_the_discard_ring() {
        let source = format!(
            "{}\n{}\n{CIRCLE_FRAGMENT}",
            crate::point10::shared::p9_glsl::P9_COMMON_WGSL,
            crate::point10::scenes::plane9_common::PLANE9_QUAD_UNIFORMS_WGSL,
        );
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("Circle Crossover WGSL failed to parse: {error}"));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("Circle Crossover WGSL validates");
        assert!(CIRCLE_FRAGMENT.contains("if (d > 1.0)"));
        assert!(
            CIRCLE_FRAGMENT.contains("p9_fbm(vec3<f32>(p + in.diffuse.r * 10.0, u.in1.x), 4u)")
        );
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = CircleCrossover::new();
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
