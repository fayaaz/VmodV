//! Plane9 "Mirror Tunnel" — Rust/wgpu port of `Abstract/Mirror Tunnel.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Abstract/Mirror Tunnel.xml` by Joakim
//! Dahl, CC BY-NC-SA 4.0.
//!
//! # The graph
//!
//! ```text
//! RenderObject1 (rotating1.01251 remapped cube face; Shader1 samples the
//!                 stored previous frame) over Clear1 + Shader3 (HSV backdrop)
//!   -> RenderToTexture1 (half-float)
//!     -> StoreTexture1 (previous frame)
//!     -> Shader2 (grey tint) -> RenderRect1 -> Screen1
//! ```
//!
//! # Ported exactly
//!
//! * `CamPos 0 0 -2.04`, `CamLookAt 0 0 1`, `ScaleByAspect true`;
//! * `Clear1 1 1 1 1`, `Clear2 0 0 0 1`, RTT Format 5 (`Rgba16Float`);
//! * `Expression1`'s wandering `Pos`
//!   (`(noise2(0,t), noise2(1.23,t), noise2(2.34,t)) * (0.05, 0.05, 0.2)`),
//!   `Expression2`'s `RotZ = perm`, and Shader3's own HSV expression;
//! * Shader1's `smoothstep(1.0, 0.97, …)` feedback wipe
//!   (`mix(vec4(0), prev*0.7, v)`);
//! * Shader2's `(texture * diffuse) * vec4(0.65, 0.65, 0.67, 1.0)`;
//! * Shader3's `_hsv2rgb(fract(gIn1.y), gIn1.x, gIn1.x)` backdrop.
//!
//! The XML's `Cube` is reduced to its camera-facing face. This is exact for a
//! Z-only rotation: the face is scaled by 1.01251 while the cube's depth stays
//! 1, so the front face's half-extent (`0.506255`) is wider than a side face's
//! (`0.5`). A ray from the camera through any side face therefore crosses the
//! front face first, and the `Expression1` x/y wander (±0.05) moves both faces
//! together. Rotating the front face about Z can only increase its extent along
//! any screen axis, so no translation or rotation in this preset reveals an
//! edge; a full cube would add no visible pixels.
//!
//! `ScaleByAspect true` is pinned below but not applied: the reference JS
//! ignores the node port too, and the camera aspect is already carried by
//! [`plane9_camera`].

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::cube_mesh::{Mat4, mat4_mul, trs};
use crate::point10::scenes::plane9_common::{
    Plane9Feedback, Plane9MeshPass, Plane9MeshUniforms, Plane9Pass, Plane9TextureBinding,
    Plane9Uniforms, plane9_camera_lens, plane9_sampler,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::random::{value_noise1, value_noise2};
use crate::point10::{
    HDR_FORMAT, Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, lens_value,
    plane9_scene_props,
};

/// `Screen1` camera position.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, -2.04];
/// `Screen1` camera look direction.
pub const CAMERA_LOOK: [f32; 3] = [0.0, 0.0, 1.0];
/// `Screen1` `ScaleByAspect`.
pub const SCALE_BY_ASPECT: bool = true;
/// `Clear1` colour (white, the feedback backdrop).
pub const CLEAR_FEEDBACK: wgpu::Color = wgpu::Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};
/// `Clear2` colour (opaque black, the screen).
pub const CLEAR_SCREEN: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
/// `RenderToTexture1` format (Format 5 = half-float).
pub const RTT_FORMAT: wgpu::TextureFormat = HDR_FORMAT;
/// `RenderToTexture1` `CreateMipMaps`.
pub const RTT_MIPS: bool = true;
/// `StoreTexture1` `SwapDelay`.
pub const SWAP_DELAY: f32 = 0.0;
/// `Cube1` `Size` (all axes).
pub const CUBE_SIZE: f32 = 1.0;
/// `RenderObject1` `Scale` (x and y).
pub const MESH_SCALE: f32 = 1.01251;
/// The offset of the camera-facing cube face from the cube centre along Z.
pub const FACE_OFFSET: f32 = -CUBE_SIZE * MESH_SCALE * 0.5;
/// Shader2's grey tint.
pub const SCREEN_TINT: [f32; 3] = [0.65, 0.65, 0.67];
/// Shader1's edge feather.
pub const EDGE_FEATHER: f32 = 0.97;
/// Shader1's previous-frame retention.
pub const FEEDBACK_RETENTION: f32 = 0.7;

const PROPS: &[SceneProp] = &[];

fn background_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| format!("{P9_COMMON_WGSL}\n{BACKGROUND}"))
}

const BACKGROUND: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  var out = u.color1 * u.in1.x;
  out = vec4<f32>(p9_hsv2rgb(fract(u.in1.y), u.in1.x, u.in1.x), 1.0);
  return out;
}
"#;

const FEEDBACK: &str = r#"
@fragment
fn fs_main(in: Plane9MeshOut) -> @location(0) vec4<f32> {
  let p = in.tex * vec2<f32>(2.0) - vec2<f32>(1.0);
  let v = p9_smoothstep(1.0, 0.97, abs(p.x)) * p9_smoothstep(1.0, 0.97, abs(p.y));
  let prev = textureSampleLevel(g_texture1, g_sampler, in.tex, 0.0);
  return mix(vec4<f32>(0.0), prev * 0.7, v);
}
"#;

const PRESENT: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  return (textureSampleLevel(g_texture1, g_sampler, tex, 0.0) * u.color1)
    * vec4<f32>(0.65, 0.65, 0.67, 1.0);
}
"#;

struct Gpu {
    background: Plane9Pass,
    mesh: Plane9MeshPass,
    present: Plane9Pass,
    feedback: Plane9Feedback,
    sampler: wgpu::Sampler,
}

/// `Plane9 Mirror Tunnel` scene.
pub struct MirrorTunnel {
    audio: P10Audio,
    /// `Expression1` state (the cube position wander).
    pos_perm: f32,
    /// `Expression2` state (the cube Z rotation).
    rot_perm: f32,
    /// Shader3's own expression state (the HSV backdrop).
    backdrop_perm: f32,
    elapsed: f32,
    position: [f32; 3],
    rotation_degrees: f32,
    lens: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    uniforms: Plane9Uniforms,
    mesh_uniforms: Plane9MeshUniforms,
    gpu: Option<Gpu>,
}

impl Default for MirrorTunnel {
    fn default() -> Self {
        Self::new()
    }
}

impl MirrorTunnel {
    /// Create the scene.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            pos_perm: 1.0,
            rot_perm: 1.0,
            backdrop_perm: 1.0,
            elapsed: 0.0,
            position: [0.0, 0.0, 0.0],
            rotation_degrees: 1.0,
            lens: 1.0,
            size: (1, 1),
            target_format: HDR_FORMAT,
            device: None,
            queue: None,
            uniforms: Plane9Uniforms::new(1, 1),
            mesh_uniforms: Plane9MeshUniforms::new(1, 1),
            gpu: None,
        }
    }

    /// The cube's current `Pos` (`Expression1`).
    pub fn position(&self) -> [f32; 3] {
        self.position
    }

    /// The cube's current `RotZ`, in degrees (`Expression2`).
    pub fn rotation_degrees(&self) -> f32 {
        self.rotation_degrees
    }

    /// Bind the shared Lens prop (this preset declares no other `sceneProps`).
    pub fn apply_props(&mut self, mut props: impl FnMut(&str) -> Option<f32>) {
        self.lens = lens_value(props("lens"));
    }

    /// The cube-face model matrix at the current position/rotation.
    pub fn model(&self) -> Mat4 {
        trs(
            [
                self.position[0],
                self.position[1],
                self.position[2] + FACE_OFFSET,
            ],
            [0.0, 0.0, self.rotation_degrees.to_radians()],
            [MESH_SCALE, MESH_SCALE, 1.0],
        )
    }

    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        let feedback = Plane9Feedback::new(
            &device,
            "point10-plane9-mirror-tunnel-feedback",
            self.size.0,
            self.size.1,
            RTT_FORMAT,
        );
        let sampler = plane9_sampler(&device, "point10-plane9-mirror-tunnel-sampler");
        let background = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-mirror-tunnel-background",
            background_wgsl(),
            None,
        );
        let mesh = Plane9MeshPass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-mirror-tunnel-mesh",
            FEEDBACK,
            feedback.previous_view(),
            &sampler,
        );
        let present = Plane9Pass::new(
            &device,
            &queue,
            self.target_format,
            "point10-plane9-mirror-tunnel-present",
            PRESENT,
            Some(Plane9TextureBinding::D2 {
                view: feedback.current_view(),
                sampler: &sampler,
            }),
        );
        self.gpu = Some(Gpu {
            background,
            mesh,
            present,
            feedback,
            sampler,
        });
        self.upload_uniforms();
    }

    fn upload_uniforms(&self) {
        if let Some(gpu) = &self.gpu {
            gpu.background.upload(&self.uniforms);
            gpu.mesh.upload(&self.mesh_uniforms);
            gpu.present.upload(&self.uniforms);
        }
    }
}

impl Point10Scene for MirrorTunnel {
    fn name(&self) -> &'static str {
        "Plane9 Mirror Tunnel"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.size = (ctx.width.max(1), ctx.height.max(1));
        self.uniforms = Plane9Uniforms::new(self.size.0, self.size.1);
        self.mesh_uniforms = Plane9MeshUniforms::new(self.size.0, self.size.1);
        self.rebuild();
    }

    fn resize(&mut self, width: u32, height: u32) {
        let size = (width.max(1), height.max(1));
        if self.size == size {
            return;
        }
        self.size = size;
        self.uniforms.resolution = [size.0 as f32, size.1 as f32];
        self.mesh_uniforms.resolution = [size.0 as f32, size.1 as f32];
        self.rebuild();
    }

    fn update(&mut self, dt: f32, audio: &AudioBridge) {
        let dt = frame_dt(dt);
        self.audio.update(dt, audio);
        self.elapsed += dt;

        let bass = self.audio.band(-1, 0, 0, 0.01);
        self.pos_perm += (bass * 5.0 + 0.01) * dt;
        let t = self.pos_perm;
        self.position = [
            value_noise2(0.0, t) * 0.05,
            value_noise2(1.23, t) * 0.05,
            value_noise2(2.34, t) * 0.2,
        ];

        let mid = self.audio.band(-1, 1, 1, 0.01);
        self.rot_perm += mid * 100.0 * dt;
        self.rotation_degrees = self.rot_perm;

        self.backdrop_perm += bass * 50.0 * dt;
        self.uniforms.time = self.elapsed;
        self.uniforms.in1 = [
            value_noise1(self.backdrop_perm) * 0.5 + 0.5,
            self.backdrop_perm * 0.01,
            0.0,
            0.0,
        ];
        self.mesh_uniforms.time = self.elapsed;
        let aspect = self.size.0 as f32 / self.size.1.max(1) as f32;
        let camera = plane9_camera_lens(
            aspect,
            CAMERA_POSITION,
            CAMERA_LOOK,
            45.0,
            0.1,
            1000.0,
            self.lens,
        );
        self.mesh_uniforms.view_proj = mat4_mul(&camera, &self.model());
        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        gpu.background.draw(
            encoder,
            gpu.feedback.current_view(),
            CLEAR_FEEDBACK,
            "point10-plane9-mirror-tunnel-background",
        );
        gpu.mesh
            .rebind_texture(gpu.feedback.previous_view(), &gpu.sampler);
        gpu.mesh.upload(&self.mesh_uniforms);
        gpu.mesh.draw_load(
            encoder,
            gpu.feedback.current_view(),
            "point10-plane9-mirror-tunnel-mesh",
        );
        gpu.present
            .rebind_texture(gpu.feedback.current_view(), &gpu.sampler);
        gpu.present.upload(&self.uniforms);
        gpu.present.draw(
            encoder,
            target,
            CLEAR_SCREEN,
            "point10-plane9-mirror-tunnel-present",
        );
        gpu.feedback.swap();
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
        self.elapsed = 0.0;
        self.pos_perm = 1.0;
        self.rot_perm = 1.0;
        self.backdrop_perm = 1.0;
    }

    fn props(&self) -> Vec<SceneProp> {
        plane9_scene_props(PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Mirror Tunnel", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::scenes::plane9_common::{
        PLANE9_MESH_UNIFORMS_WGSL, PLANE9_TEXTURE_WGSL, PLANE9_UNIFORMS_WGSL,
    };
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn camera_clear_and_cube_match_the_preset() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, -2.04]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, 1.0]);
        const { assert!(SCALE_BY_ASPECT) };
        assert_eq!(CLEAR_FEEDBACK.r, 1.0);
        assert_eq!(CLEAR_SCREEN.a, 1.0);
        assert_eq!(RTT_FORMAT, HDR_FORMAT);
        const { assert!(RTT_MIPS) };
        assert_eq!(SWAP_DELAY, 0.0);
        assert_eq!(CUBE_SIZE, 1.0);
        assert_eq!(MESH_SCALE, 1.01251);
        assert!((FACE_OFFSET + 0.506255).abs() < 1e-5);
        assert_eq!(SCREEN_TINT, [0.65, 0.65, 0.67]);
        assert_eq!(EDGE_FEATHER, 0.97);
        assert_eq!(FEEDBACK_RETENTION, 0.7);
    }

    /// The single-face reduction is only valid while the front face is wider
    /// than the cube's side faces; pin the geometry inequality it relies on.
    #[test]
    fn front_face_occludes_the_cube_sides() {
        let face_half = CUBE_SIZE * MESH_SCALE * 0.5;
        let side_half = CUBE_SIZE * 0.5;
        assert!(face_half > side_half, "{face_half} <= {side_half}");
        // A Z rotation's smallest axis-aligned extent is still `face_half`.
        for degrees in [0.0_f32, 15.0, 45.0, 90.0, 137.5] {
            let radians = degrees.to_radians();
            let extent = face_half * (radians.cos().abs() + radians.sin().abs());
            assert!(extent >= face_half, "{degrees}: {extent}");
        }
    }

    #[test]
    fn expressions_and_shaders_keep_the_drive() {
        let scene = MirrorTunnel::new();
        assert_eq!(scene.position(), [0.0, 0.0, 0.0]);
        assert_eq!(scene.rotation_degrees(), 1.0);
        // The model sits the camera-facing face at pos.z - half the scaled cube.
        let model = scene.model();
        assert!((model[3][2] - FACE_OFFSET).abs() < 1e-5);
        for needle in [
            "p9_smoothstep(1.0, 0.97, abs(p.x))",
            "p9_smoothstep(1.0, 0.97, abs(p.y))",
            "mix(vec4<f32>(0.0), prev * 0.7, v)",
        ] {
            assert!(FEEDBACK.contains(needle), "lost {needle}");
        }
        for needle in [
            "p9_hsv2rgb(fract(u.in1.y), u.in1.x, u.in1.x)",
            "* vec4<f32>(0.65, 0.65, 0.67, 1.0)",
        ] {
            assert!(
                BACKGROUND.contains(needle) || PRESENT.contains(needle),
                "lost {needle}"
            );
        }
    }

    #[test]
    fn shaders_validate() {
        let sources = [
            format!("{PLANE9_UNIFORMS_WGSL}\n{}", background_wgsl()),
            format!("{P9_COMMON_WGSL}\n{PLANE9_MESH_UNIFORMS_WGSL}\n{FEEDBACK}"),
            format!("{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{PRESENT}"),
        ];
        for wgsl in sources {
            let module = naga::front::wgsl::parse_str(&wgsl)
                .unwrap_or_else(|error| panic!("Mirror Tunnel WGSL failed to parse: {error}"));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("Mirror Tunnel WGSL validates");
        }
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = MirrorTunnel::new();
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
