//! Plane9 "Color Blob" — Rust/wgpu port of `Other/Color Blob.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Other/Color Blob.xml` by Joakim Dahl,
//! CC BY-NC-SA 3.0.
//!
//! # The graph
//!
//! ```text
//! RenderObject1 (rotating 1.01251 plane, Shader1 samples the stored frame)
//!   -> RenderToTexture1 (RGBA8)
//!     -> StoreTexture1 (previous frame)
//!     -> Shader2 -> RenderRect1 -> Screen1
//! ```
//!
//! Inside `RenderObject1` the white `Clear1` and `Shader3`'s
//! `diffuse * gIn1.x` background are drawn first, then the mesh.
//!
//! # Ported exactly
//!
//! * `CamPos 0 0 -1.31`, `CamLookAt 0 0 1`, `ScaleByAspect true`;
//! * the `1.01251` plane scale and the `Rotator1` `RotSpeedZ = 0.5` deg/s
//!   (`RotZ = 0.5 * time`), with only `RotZ` connected;
//! * Shader1's reversed-edge mask
//!   `smoothstep(1.0, 0.99, |p.x|) * smoothstep(1.0, 0.99, |p.y|)`,
//!   `(1 - v*0.015)*texture*0.98 + v*0.01*hsv(sin(time)*0.5+0.5)`;
//! * Shader3's `1 - max(0, band(-1,1,0,0)*2)` background pulse and the
//!   `texture * diffuse` screen pass.
//!
//! `ScaleByAspect true` is pinned below but deliberately not applied: the
//! reference JS ignores that camera port as well, and the aspect is already
//! carried by [`plane9_camera`].

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::cube_mesh::{Mat4, mat4_mul, trs};
use crate::point10::scenes::plane9_common::{
    Plane9Feedback, Plane9MeshPass, Plane9MeshUniforms, Plane9Pass, Plane9TextureBinding,
    Plane9Uniforms, plane9_camera, plane9_sampler,
};
use crate::point10::{
    Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, scene_props_with_levers,
};

/// `Screen1` camera position.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, -1.31];
/// `Screen1` camera look direction.
pub const CAMERA_LOOK: [f32; 3] = [0.0, 0.0, 1.0];
/// `Screen1` `ScaleByAspect`.
pub const SCALE_BY_ASPECT: bool = true;
/// `Clear1` colour (white; the feedback background).
pub const CLEAR_FEEDBACK: wgpu::Color = wgpu::Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};
/// `Clear2` colour (opaque black; the screen).
pub const CLEAR_SCREEN: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
/// `RenderToTexture1` format (Format 1 = RGBA8).
pub const RTT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// `RenderToTexture1` `CreateMipMaps`.
pub const RTT_MIPS: bool = true;
/// `StoreTexture1` `SwapDelay`.
pub const SWAP_DELAY: f32 = 0.0;
/// `RenderObject1` `Scale` (x and y).
pub const MESH_SCALE: f32 = 1.01251;
/// `Rotator1` `RotSpeedZ`, degrees per second.
pub const ROT_SPEED_Z: f32 = 0.5;
/// `Clear1` alpha.
pub const FEEDBACK_ALPHA: f32 = 1.0;
/// Shader1's edge fade width.
pub const EDGE_FEATHER: f32 = 0.99;

const PROPS: &[SceneProp] = &[];

const BACKGROUND: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  // `si.diffuse * gIn1.x`; iColor, gColor1 and gColor are all white here.
  return u.color1 * u.in1.x;
}
"#;

const FEEDBACK: &str = r#"
@fragment
fn fs_main(in: Plane9MeshOut) -> @location(0) vec4<f32> {
  let p = in.tex * vec2<f32>(2.0) - vec2<f32>(1.0);
  let v = p9_smoothstep(1.0, 0.99, abs(p.x)) * p9_smoothstep(1.0, 0.99, abs(p.y));
  let c = vec4<f32>(p9_hsv2rgb(sin(u.time) * 0.5 + 0.5, 1.0, 1.0), 1.0);
  let prev = textureSampleLevel(g_texture1, g_sampler, in.tex, 0.0);
  return ((1.0 - v * 0.015) * prev * 0.98) + (v * 0.01 * c);
}
"#;

const PRESENT: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  return textureSampleLevel(g_texture1, g_sampler, tex, 0.0) * u.color1;
}
"#;

struct Gpu {
    background: Plane9Pass,
    mesh: Plane9MeshPass,
    present: Plane9Pass,
    feedback: Plane9Feedback,
    sampler: wgpu::Sampler,
}

/// `Plane9 Color Blob` scene.
pub struct ColorBlob {
    audio: P10Audio,
    elapsed: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    uniforms: Plane9Uniforms,
    mesh_uniforms: Plane9MeshUniforms,
    gpu: Option<Gpu>,
}

impl Default for ColorBlob {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorBlob {
    /// Create the scene.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            elapsed: 0.0,
            size: (1, 1),
            target_format: crate::point10::HDR_FORMAT,
            device: None,
            queue: None,
            uniforms: Plane9Uniforms::new(1, 1),
            mesh_uniforms: Plane9MeshUniforms::new(1, 1),
            gpu: None,
        }
    }

    /// The uniform block last uploaded.
    pub fn uniforms(&self) -> &Plane9Uniforms {
        &self.uniforms
    }

    /// The mesh uniform block last uploaded.
    pub fn mesh_uniforms(&self) -> &Plane9MeshUniforms {
        &self.mesh_uniforms
    }

    /// The plane model matrix at `time` seconds.
    pub fn model(&self, time: f32) -> Mat4 {
        let rotation = [0.0, 0.0, (ROT_SPEED_Z * time).to_radians()];
        trs([0.0, 0.0, 0.0], rotation, [MESH_SCALE, MESH_SCALE, 1.0])
    }

    /// Bind the modV props (this preset declares no `sceneProps`).
    pub fn apply_props(&mut self, _props: impl FnMut(&str) -> Option<f32>) {}

    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        let feedback = Plane9Feedback::new(
            &device,
            "point10-plane9-color-blob-feedback",
            self.size.0,
            self.size.1,
            RTT_FORMAT,
        );
        let sampler = plane9_sampler(&device, "point10-plane9-color-blob-sampler");
        let background = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-color-blob-background",
            BACKGROUND,
            None,
        );
        let mesh = Plane9MeshPass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-color-blob-mesh",
            FEEDBACK,
            feedback.previous_view(),
            &sampler,
        );
        let present = Plane9Pass::new(
            &device,
            &queue,
            self.target_format,
            "point10-plane9-color-blob-present",
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

impl Point10Scene for ColorBlob {
    fn name(&self) -> &'static str {
        "Plane9 Color Blob"
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
        let band = self.audio.band(-1, 1, 0, 0.0);
        let pulse = 1.0 - (band * 2.0).max(0.0);
        self.uniforms.time = self.elapsed;
        self.uniforms.in1 = [pulse, 0.0, 0.0, 0.0];
        self.mesh_uniforms.time = self.elapsed;
        let aspect = self.size.0 as f32 / self.size.1.max(1) as f32;
        let camera = plane9_camera(aspect, CAMERA_POSITION, CAMERA_LOOK, 45.0, 0.1, 1000.0);
        self.mesh_uniforms.view_proj = mat4_mul(&camera, &self.model(self.elapsed));
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
            "point10-plane9-color-blob-background",
        );
        gpu.mesh
            .rebind_texture(gpu.feedback.previous_view(), &gpu.sampler);
        gpu.mesh.upload(&self.mesh_uniforms);
        gpu.mesh.draw_load(
            encoder,
            gpu.feedback.current_view(),
            "point10-plane9-color-blob-mesh",
        );
        gpu.present
            .rebind_texture(gpu.feedback.current_view(), &gpu.sampler);
        gpu.present.upload(&self.uniforms);
        gpu.present.draw(
            encoder,
            target,
            CLEAR_SCREEN,
            "point10-plane9-color-blob-present",
        );
        gpu.feedback.swap();
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
        self.elapsed = 0.0;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Color Blob", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::scenes::plane9_common::{
        PLANE9_MESH_UNIFORMS_WGSL, PLANE9_TEXTURE_WGSL, PLANE9_UNIFORMS_WGSL,
    };
    use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn camera_clear_and_mesh_match_the_preset() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, -1.31]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, 1.0]);
        const { assert!(SCALE_BY_ASPECT) };
        assert_eq!(CLEAR_FEEDBACK.r, 1.0);
        assert_eq!(CLEAR_SCREEN.r, 0.0);
        assert_eq!(CLEAR_SCREEN.a, 1.0);
        assert_eq!(RTT_FORMAT, wgpu::TextureFormat::Rgba8Unorm);
        const { assert!(RTT_MIPS) };
        assert_eq!(SWAP_DELAY, 0.0);
        assert_eq!(MESH_SCALE, 1.01251);
        assert_eq!(ROT_SPEED_Z, 0.5);
        let scene = ColorBlob::new();
        let model = scene.model(2.0);
        // `Rotator1.RotZ = 0.5 * time` degrees, scaled by `1.01251`.
        let expected = 1.0_f32.to_radians();
        let rotation = crate::point10::scenes::cube_mesh::rotation_z(expected);
        for column in 0..3 {
            for row in 0..3 {
                let scale = if column == 2 || row == 2 {
                    1.0
                } else {
                    MESH_SCALE
                };
                assert!(
                    (model[column][row] - rotation[column][row] * scale).abs() < 1e-5,
                    "model[{column}][{row}]"
                );
            }
        }
    }

    #[test]
    fn shaders_keep_the_feedback_and_pulse() {
        for needle in [
            "p9_smoothstep(1.0, 0.99, abs(p.x))",
            "p9_smoothstep(1.0, 0.99, abs(p.y))",
            "1.0 - v * 0.015",
            "prev * 0.98",
            "v * 0.01 * c",
            "p9_hsv2rgb(sin(u.time) * 0.5 + 0.5, 1.0, 1.0)",
        ] {
            assert!(FEEDBACK.contains(needle), "lost {needle}");
        }
        assert!(
            BACKGROUND.contains("return u.color1 * u.in1.x;"),
            "lost the background pulse"
        );
        assert!(PRESENT.contains("textureSampleLevel(g_texture1, g_sampler, tex, 0.0) * u.color1"));
    }

    #[test]
    fn shaders_validate() {
        let sources = [
            format!("{PLANE9_UNIFORMS_WGSL}\n{BACKGROUND}"),
            format!("{P9_COMMON_WGSL}\n{PLANE9_MESH_UNIFORMS_WGSL}\n{FEEDBACK}"),
            format!("{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{PRESENT}"),
        ];
        for wgsl in sources {
            let module = naga::front::wgsl::parse_str(&wgsl)
                .unwrap_or_else(|error| panic!("Color Blob WGSL failed to parse: {error}"));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("Color Blob WGSL validates");
        }
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = ColorBlob::new();
        let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        ctx.resize(64, 64);
        scene.init(&mut ctx);
        let mut bridge = AudioBridge::new();
        let mut payload = BandPayload::silent(44_100.0);
        payload.lanes = [0.4; 12];
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
