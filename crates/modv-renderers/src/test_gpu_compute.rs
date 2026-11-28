//! WGSL compute-shim unit tests — run real shader math on the test adapter.
//!
//! This is the shader-side equivalent of `egui_kittest`: instead of asserting
//! on the *text* the ISF translator emitted, a test supplies a complete WGSL
//! compute shader (a thin shim around the function under test), typed inputs
//! and a typed output buffer. The harness dispatches it on the shared
//! software adapter from [`crate::test_gpu`], reads the output buffer back and
//! hands the values to Rust for ordinary numeric assertions.
//!
//! # Binding contract
//!
//! The shim must declare exactly:
//!
//! ```wgsl
//! @group(0) @binding(0) var<storage, read> inputs: array<I>;
//! @group(0) @binding(1) var<storage, read_write> outputs: array<O>;
//! @compute @workgroup_size(N) fn main(@builtin(global_invocation_id) ...)
//! ```
//!
//! `I` and `O` are `bytemuck::Pod` types whose WGSL mirrors must have the
//! same size and alignment (use explicit padding members). The harness always
//! dispatches one workgroup per `N` inputs, so the shader should bounds-check
//! `gid.x` if it uses a workgroup size larger than one.
//!
//! CI runs this on lavapipe with `--test-threads=1`, the same device as the
//! rest of the GPU suite. When no adapter exists at all the test skips with
//! a log line rather than failing.

#![cfg(test)]

use bytemuck::{Pod, Zeroable};

/// One compute-shim invocation: WGSL source plus typed input/output buffers.
pub(crate) struct ComputeTest<'a, I, O> {
    label: &'a str,
    wgsl: &'a str,
    inputs: &'a [I],
    workgroup_size: u32,
    _output: core::marker::PhantomData<O>,
}

impl<'a, I: Pod, O: Pod> ComputeTest<'a, I, O> {
    pub(crate) fn new(label: &'a str, wgsl: &'a str, inputs: &'a [I]) -> Self {
        Self {
            label,
            wgsl,
            inputs,
            workgroup_size: 64,
            _output: core::marker::PhantomData,
        }
    }

    /// Override `@workgroup_size(N)` in the shim (defaults to 64).
    pub(crate) fn workgroup_size(mut self, size: u32) -> Self {
        self.workgroup_size = size;
        self
    }

    /// Dispatch on the shared test adapter and read every output slot back.
    /// `None` means there is no adapter in this environment.
    pub(crate) fn run(self) -> Option<Vec<O>> {
        use wgpu::util::DeviceExt;

        let (device, queue) = crate::test_gpu::gpu()?;
        if self.inputs.is_empty() {
            return Some(Vec::new());
        }
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(self.label),
            source: wgpu::ShaderSource::Wgsl(self.wgsl.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(self.label),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(self.label),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(self.label),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let input_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(self.label),
            contents: bytemuck::cast_slice(self.inputs),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let output_bytes = (size_of::<O>() * self.inputs.len()) as u64;
        let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.label),
            size: output_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.label),
            size: output_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(self.label),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output_buffer.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some(self.label),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            let groups = self.inputs.len().div_ceil(self.workgroup_size as usize) as u32;
            pass.dispatch_workgroups(groups, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output_buffer, 0, &readback, 0, output_bytes);
        queue.submit(Some(encoder.finish()));

        // Same readback handshake as `isf::runtime::read_texture`: map_async
        // completes only once the device is polled.
        let slice = readback.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the shared test device polls");
        receiver
            .recv()
            .expect("the map callback fires")
            .expect("the readback buffer maps");
        let mapped = slice
            .get_mapped_range()
            .expect("the readback range is mapped");
        let values = bytemuck::cast_slice::<u8, O>(&mapped).to_vec();
        drop(mapped);
        readback.unmap();
        Some(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::scenes::plane9_common::{PLANE9_BATCH_WGSL, PLANE9_TONEMAP_WGSL};
    use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
    use crate::point10::shared::p9_shading::P9_SHADING_WGSL;
    use crate::shader::uniforms::{GLSL_MOD_F_WGSL, GLSL_MOD_V2_WGSL};

    #[derive(Clone, Copy, Pod, Zeroable)]
    #[repr(C)]
    struct ModInput {
        x: f32,
        y: f32,
    }

    #[derive(Clone, Copy, Pod, Zeroable)]
    #[repr(C)]
    struct ModOutput {
        value: f32,
        _pad: [f32; 3],
    }

    /// The GLSL-mod const is currently pinned by source checks only. Executing
    /// it proves the floor-based semantics the ported modules rely on — WGSL's
    /// own `%` truncates and would fail these three cases.
    #[test]
    fn compute_shim_executes_glsl_mod_f_on_gpu() {
        if crate::test_gpu::gpu().is_none() {
            eprintln!("skipping compute-shim test: no GPU adapter in this environment");
            return;
        }
        let wgsl = format!(
            r#"
struct ModInput {{ x: f32, y: f32, }}
struct ModOutput {{ value: f32, pad0: f32, pad1: f32, pad2: f32, }}
@group(0) @binding(0) var<storage, read> inputs: array<ModInput>;
@group(0) @binding(1) var<storage, read_write> outputs: array<ModOutput>;
{GLSL_MOD_F_WGSL}
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
  let i = gid.x;
  outputs[i] = ModOutput(glsl_mod_f(inputs[i].x, inputs[i].y), 0.0, 0.0, 0.0);
}}
"#
        );
        let inputs = [
            ModInput { x: 5.5, y: 2.0 },
            ModInput { x: -1.5, y: 2.0 },
            ModInput { x: 1.5, y: -2.0 },
        ];
        let outputs = ComputeTest::<ModInput, ModOutput>::new("glsl_mod_f", &wgsl, &inputs)
            .workgroup_size(1)
            .run()
            .expect("shared test adapter is available");
        let values: Vec<f32> = outputs.iter().map(|o| o.value).collect();
        assert_eq!(values.len(), inputs.len());
        for (actual, expected) in values.iter().zip([1.5_f32, 0.5, -0.5]) {
            assert!(
                (actual - expected).abs() < 1e-5,
                "glsl_mod_f on the GPU returned {values:?}; expected floor-based mod"
            );
        }
    }

    #[derive(Clone, Copy, Pod, Zeroable)]
    #[repr(C)]
    struct Vec2ModPair {
        x: [f32; 2],
        y: [f32; 2],
    }

    #[derive(Clone, Copy, Pod, Zeroable)]
    #[repr(C)]
    struct Vec2ModOutput {
        value: [f32; 4],
    }

    /// `glsl_mod_v2` is spliced into Point10 modules alongside the scalar form,
    /// but only the scalar form was exercised numerically. Each vector lane
    /// below has a negative operand, so WGSL's `%` (which truncates the
    /// quotient toward zero) gives a different answer from GLSL's floor-based
    /// `mod`: the remainder's sign follows the divisor, not the dividend.
    #[test]
    fn compute_shim_executes_glsl_mod_v2_on_gpu() {
        if crate::test_gpu::gpu().is_none() {
            eprintln!("skipping compute-shim test: no GPU adapter in this environment");
            return;
        }
        let wgsl = format!(
            r#"
struct Vec2ModPair {{ x: vec2<f32>, y: vec2<f32>, }}
struct Vec2ModOutput {{ value: vec4<f32>, }}
@group(0) @binding(0) var<storage, read> inputs: array<Vec2ModPair>;
@group(0) @binding(1) var<storage, read_write> outputs: array<Vec2ModOutput>;
{GLSL_MOD_V2_WGSL}
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
  let i = gid.x;
  let result = glsl_mod_v2(inputs[i].x, inputs[i].y);
  outputs[i] = Vec2ModOutput(vec4<f32>(result.x, result.y, 0.0, 0.0));
}}
"#
        );
        let inputs = [
            // Negative dividend / positive divisor: floor(-0.75) == -1.
            Vec2ModPair {
                x: [-1.5, 5.5],
                y: [2.0, 2.0],
            },
            // Positive dividend / negative divisor: floor(-0.75) == -1.
            Vec2ModPair {
                x: [1.5, -1.5],
                y: [-2.0, -2.0],
            },
            // Both operands negative: lane 0's result flips sign, lane 1's does not.
            Vec2ModPair {
                x: [-0.25, -0.25],
                y: [0.5, -0.5],
            },
        ];
        let outputs = ComputeTest::<Vec2ModPair, Vec2ModOutput>::new("glsl_mod_v2", &wgsl, &inputs)
            .workgroup_size(1)
            .run()
            .expect("shared test adapter is available");
        let values: Vec<[f32; 2]> = outputs.iter().map(|o| [o.value[0], o.value[1]]).collect();
        assert_eq!(values.len(), inputs.len());
        for (actual, expected) in values
            .iter()
            .zip([[0.5_f32, 1.5], [-0.5, -1.5], [0.25, -0.25]])
        {
            for lane in 0..2 {
                assert!(
                    (actual[lane] - expected[lane]).abs() < 1e-5,
                    "glsl_mod_v2 on the GPU returned {values:?}; expected floor-based vec2 mod: lane {lane} was {} instead of {}",
                    actual[lane],
                    expected[lane]
                );
            }
        }
    }

    #[derive(Clone, Copy, Pod, Zeroable)]
    #[repr(C)]
    struct UvInput {
        resolution: [f32; 4],
        frag: [f32; 4],
    }

    #[derive(Clone, Copy, Pod, Zeroable)]
    #[repr(C)]
    struct UvOutput {
        value: [f32; 4],
    }

    /// `modv_uv` ships only as text inside `SHARED_UNIFORM_WGSL`, so this test
    /// extracts the *actual* helper body rather than re-implementing the
    /// division in the shim. A private stand-in for `u` supplies `iResolution`
    /// per invocation; the shader still compiles the shipped function.
    #[test]
    fn compute_shim_executes_shipped_modv_uv_on_gpu() {
        if crate::test_gpu::gpu().is_none() {
            eprintln!("skipping compute-shim test: no GPU adapter in this environment");
            return;
        }
        let shared = crate::shader::uniforms::SHARED_UNIFORM_WGSL;
        let from_fn = &shared[shared
            .find("fn modv_uv(")
            .expect("SHARED_UNIFORM_WGSL declares modv_uv")..];
        let end = from_fn
            .find("\n}\n")
            .expect("modv_uv body has a closing brace")
            + "\n}\n".len();
        let helper = &from_fn[..end];

        let wgsl = format!(
            r#"
struct UvStandIn {{ iResolution: vec3<f32>, _pad0: f32, }}
struct UvInput {{ resolution: vec4<f32>, frag: vec4<f32>, }}
struct UvOutput {{ value: vec4<f32>, }}
@group(0) @binding(0) var<storage, read> inputs: array<UvInput>;
@group(0) @binding(1) var<storage, read_write> outputs: array<UvOutput>;
var<private> u: UvStandIn;
{helper}
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
  let i = gid.x;
  u.iResolution = inputs[i].resolution.xyz;
  let uv = modv_uv(inputs[i].frag.xy);
  outputs[i] = UvOutput(vec4<f32>(uv.x, uv.y, 0.0, 0.0));
}}
"#
        );
        let inputs = [
            UvInput {
                resolution: [1920.0, 1080.0, 1.0, 0.0],
                frag: [0.0, 0.0, 0.0, 0.0],
            },
            UvInput {
                resolution: [1920.0, 1080.0, 1.0, 0.0],
                frag: [960.0, 540.0, 0.0, 0.0],
            },
            // A y-flip would return 0.25 here; the shipped helper must not flip.
            UvInput {
                resolution: [1920.0, 1080.0, 1.0, 0.0],
                frag: [480.0, 810.0, 0.0, 0.0],
            },
            UvInput {
                resolution: [800.0, 600.0, 1.0, 0.0],
                frag: [200.0, 450.0, 0.0, 0.0],
            },
        ];
        let outputs = ComputeTest::<UvInput, UvOutput>::new("modv_uv", &wgsl, &inputs)
            .workgroup_size(1)
            .run()
            .expect("shared test adapter is available");
        let values: Vec<[f32; 2]> = outputs.iter().map(|o| [o.value[0], o.value[1]]).collect();
        assert_eq!(values.len(), inputs.len());
        for (actual, expected) in
            values
                .iter()
                .zip([[0.0_f32, 0.0], [0.5, 0.5], [0.25, 0.75], [0.25, 0.75]])
        {
            for lane in 0..2 {
                assert!(
                    (actual[lane] - expected[lane]).abs() < 1e-5,
                    "modv_uv on the GPU returned {values:?}; expected top-left-origin normalised uv: lane {lane} was {} instead of {}",
                    actual[lane],
                    expected[lane]
                );
            }
        }
    }

    // ---- Shipped Point10 / Plane9 helpers, executed on the GPU ------------
    //
    // The `contains(...)` needles in p9_glsl, p9_shading and plane9_common pin
    // only the *text* of these helpers. Each test below splices the shipped WGSL
    // const straight into a compute shim, so naga compiles the real arithmetic
    // and the shared lavapipe adapter executes it. Rust only checks numbers.

    /// Slice one function out of a shipped WGSL const, keeping its source
    /// byte-for-byte. Used for helpers that live alongside functions wgpu
    /// cannot compile in a compute stage (`p9_perturb_normal` uses `dpdx`).
    fn extract_fn<'a>(wgsl: &'a str, name: &str) -> &'a str {
        let marker = format!("fn {name}(");
        let start = wgsl
            .find(&marker)
            .unwrap_or_else(|| panic!("shipped WGSL is missing {marker}"));
        let tail = &wgsl[start..];
        let end = tail
            .find("\n}\n")
            .unwrap_or_else(|| panic!("{marker} has no closing brace"))
            + "\n}\n".len();
        &tail[..end]
    }

    fn assert_close(actual: f32, expected: f32, tol: f32, label: &str) {
        assert!(
            (actual - expected).abs() <= tol,
            "{label}: expected {expected}, got {actual} (tol {tol})"
        );
    }

    // vec4 is used for every storage field: WGSL's vec4<f32> has a 16-byte
    // stride and `[f32; 4]` mirrors it exactly, including in an array.
    #[derive(Clone, Copy, Debug, Pod, Zeroable)]
    #[repr(C)]
    struct P9Vec4Input {
        value: [f32; 4],
    }

    #[derive(Clone, Copy, Debug, Pod, Zeroable)]
    #[repr(C)]
    struct P9Vec4Output {
        value: [f32; 4],
    }

    #[derive(Clone, Copy, Debug, Pod, Zeroable)]
    #[repr(C)]
    struct P9DualInput {
        a: [f32; 4],
        b: [f32; 4],
    }

    #[derive(Clone, Copy, Debug, Pod, Zeroable)]
    #[repr(C)]
    struct P9DualOutput {
        a: [f32; 4],
        b: [f32; 4],
    }

    /// Execute the shipped `p9_smoothstep` from `P9_COMMON_WGSL`. WGSL's
    /// built-in `smoothstep` is undefined when `edge0 >= edge1`, which is
    /// exactly how the ported Plane9 scenes call it; the shipped helper must
    /// still produce GLSL's values in that order.
    #[test]
    fn compute_shim_executes_p9_smoothstep_reversed_edges_on_gpu() {
        if crate::test_gpu::gpu().is_none() {
            eprintln!("skipping compute-shim test: no GPU adapter in this environment");
            return;
        }
        let wgsl = format!(
            r#"
struct P9Vec4Input {{ value: vec4<f32>, }}
struct P9Vec4Output {{ value: vec4<f32>, }}
@group(0) @binding(0) var<storage, read> inputs: array<P9Vec4Input>;
@group(0) @binding(1) var<storage, read_write> outputs: array<P9Vec4Output>;
{P9_COMMON_WGSL}
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
  let v = inputs[gid.x].value;
  outputs[gid.x] = P9Vec4Output(vec4<f32>(p9_smoothstep(v.x, v.y, v.z), 0.0, 0.0, 0.0));
}}
"#
        );
        // (edge0, edge1, x) -> GLSL smoothstep, forward and reversed edges.
        let cases: [([f32; 4], f32); 6] = [
            ([0.0, 1.0, 0.25, 0.0], 0.156_25),
            ([1.0, 0.9, 0.9, 0.0], 1.0),
            ([1.0, 0.9, 1.0, 0.0], 0.0),
            ([1.0, 0.9, 0.95, 0.0], 0.5),
            ([1.0, 0.9, 0.8, 0.0], 1.0),
            ([1.0, 0.9, 1.1, 0.0], 0.0),
        ];
        let inputs: Vec<P9Vec4Input> = cases
            .iter()
            .map(|(c, _)| P9Vec4Input { value: *c })
            .collect();
        let outputs =
            ComputeTest::<P9Vec4Input, P9Vec4Output>::new("p9_smoothstep", &wgsl, &inputs)
                .workgroup_size(1)
                .run()
                .expect("shared test adapter is available");
        assert_eq!(outputs.len(), cases.len());
        for (i, (out, (_, expected))) in outputs.iter().zip(cases.iter()).enumerate() {
            assert_close(
                out.value[0],
                *expected,
                1e-5,
                &format!("p9_smoothstep case {i}"),
            );
        }
    }

    /// `p9_mod` (shading const) and `p9_mod_f` (batch const) are two copies of
    /// GLSL's floor-based `mod`. Both remainders must follow the divisor's
    /// sign, unlike WGSL's truncated `%` — the discrepancy the text needles in
    /// `shading_wgsl_keeps_the_ported_formulas` and `batch_wgsl_keeps_...` pin.
    #[test]
    fn compute_shim_executes_p9_mod_divisor_sign_on_gpu() {
        if crate::test_gpu::gpu().is_none() {
            eprintln!("skipping compute-shim test: no GPU adapter in this environment");
            return;
        }
        let p9_mod = extract_fn(P9_SHADING_WGSL, "p9_mod");
        let p9_mod_f = extract_fn(PLANE9_BATCH_WGSL, "p9_mod_f");
        let wgsl = format!(
            r#"
struct P9Vec4Input {{ value: vec4<f32>, }}
struct P9DualOutput {{ a: vec4<f32>, b: vec4<f32>, }}
@group(0) @binding(0) var<storage, read> inputs: array<P9Vec4Input>;
@group(0) @binding(1) var<storage, read_write> outputs: array<P9DualOutput>;
{p9_mod}
{p9_mod_f}
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
  let v = inputs[gid.x].value;
  outputs[gid.x] = P9DualOutput(
    vec4<f32>(p9_mod(v.x, v.y), 0.0, 0.0, 0.0),
    vec4<f32>(p9_mod_f(v.x, v.y), 0.0, 0.0, 0.0)
  );
}}
"#
        );
        let cases: [([f32; 4], f32); 6] = [
            ([-3.0, 20.0, 0.0, 0.0], 17.0),
            ([23.0, 20.0, 0.0, 0.0], 3.0),
            ([1.5, -2.0, 0.0, 0.0], -0.5),
            ([-1.5, 2.0, 0.0, 0.0], 0.5),
            ([0.0, 20.0, 0.0, 0.0], 0.0),
            ([20.0, 20.0, 0.0, 0.0], 0.0),
        ];
        let inputs: Vec<P9Vec4Input> = cases
            .iter()
            .map(|(c, _)| P9Vec4Input { value: *c })
            .collect();
        let outputs = ComputeTest::<P9Vec4Input, P9DualOutput>::new("p9_mod", &wgsl, &inputs)
            .workgroup_size(1)
            .run()
            .expect("shared test adapter is available");
        assert_eq!(outputs.len(), cases.len());
        for (i, (out, (_, expected))) in outputs.iter().zip(cases.iter()).enumerate() {
            assert_close(out.a[0], *expected, 1e-5, &format!("p9_mod case {i}"));
            assert_close(out.b[0], *expected, 1e-5, &format!("p9_mod_f case {i}"));
        }
    }

    /// Execute the Narkowicz ACES fit from the shipped `PLANE9_TONEMAP_WGSL`:
    /// clamped to [0, 1], monotone in luminance, channel order kept.
    #[test]
    fn compute_shim_executes_p9_tonemap_aces_on_gpu() {
        if crate::test_gpu::gpu().is_none() {
            eprintln!("skipping compute-shim test: no GPU adapter in this environment");
            return;
        }
        let wgsl = format!(
            r#"
struct P9Vec4Input {{ value: vec4<f32>, }}
struct P9Vec4Output {{ value: vec4<f32>, }}
@group(0) @binding(0) var<storage, read> inputs: array<P9Vec4Input>;
@group(0) @binding(1) var<storage, read_write> outputs: array<P9Vec4Output>;
{P9_COMMON_WGSL}
{PLANE9_TONEMAP_WGSL}
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
  let v = inputs[gid.x].value;
  outputs[gid.x] = P9Vec4Output(vec4<f32>(p9_tonemap_aces(v.xyz), 0.0));
}}
"#
        );
        let inputs = [
            P9Vec4Input {
                value: [0.0, 0.0, 0.0, 0.0],
            },
            P9Vec4Input {
                value: [0.25, 0.25, 0.25, 0.0],
            },
            P9Vec4Input {
                value: [0.5, 0.5, 0.5, 0.0],
            },
            P9Vec4Input {
                value: [1.0, 1.0, 1.0, 0.0],
            },
            P9Vec4Input {
                value: [4.0, 4.0, 4.0, 0.0],
            },
            P9Vec4Input {
                value: [16.0, 16.0, 16.0, 0.0],
            },
            P9Vec4Input {
                value: [0.5, 1.0, 2.0, 0.0],
            },
        ];
        let outputs =
            ComputeTest::<P9Vec4Input, P9Vec4Output>::new("p9_tonemap_aces", &wgsl, &inputs)
                .workgroup_size(1)
                .run()
                .expect("shared test adapter is available");
        assert_eq!(outputs.len(), inputs.len());
        for (out, input) in outputs.iter().zip(inputs.iter()) {
            for lane in 0..3 {
                let value = out.value[lane];
                assert!(
                    (0.0..=1.0).contains(&value),
                    "p9_tonemap_aces left [0, 1]: {out:?} for {input:?}"
                );
            }
        }
        for value in outputs[0].value.iter().take(3) {
            assert_close(*value, 0.0, 1e-7, "aces maps black to black");
        }
        // Narkowicz: (1*(2.51+0.03)) / (1*(2.43+0.59)+0.14) = 2.54/3.16.
        assert_close(outputs[3].value[0], 2.54 / 3.16, 1e-4, "aces white point");
        for i in 1..5 {
            assert!(
                outputs[i - 1].value[0] < outputs[i].value[0],
                "p9_tonemap_aces is not monotone: {:?}",
                outputs.iter().map(|o| o.value[0]).collect::<Vec<_>>()
            );
        }
        assert!(
            outputs[4].value[0] < outputs[5].value[0] || (outputs[5].value[0] - 1.0).abs() < 1e-5,
            "the bright end must keep rising or saturate at 1"
        );
        assert!(
            outputs[6].value[0] < outputs[6].value[1] && outputs[6].value[1] < outputs[6].value[2],
            "p9_tonemap_aces must preserve channel order: {:?}",
            outputs[6].value
        );
    }

    /// Execute the shipped `p9_hsv2rgb`. Check the primaries so a
    /// swapped/garbled ramp cannot pass on text alone.
    #[test]
    fn compute_shim_executes_p9_hsv2rgb_primaries_on_gpu() {
        if crate::test_gpu::gpu().is_none() {
            eprintln!("skipping compute-shim test: no GPU adapter in this environment");
            return;
        }
        let wgsl = format!(
            r#"
struct P9Vec4Input {{ value: vec4<f32>, }}
struct P9Vec4Output {{ value: vec4<f32>, }}
@group(0) @binding(0) var<storage, read> inputs: array<P9Vec4Input>;
@group(0) @binding(1) var<storage, read_write> outputs: array<P9Vec4Output>;
{P9_COMMON_WGSL}
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
  let v = inputs[gid.x].value;
  outputs[gid.x] = P9Vec4Output(vec4<f32>(p9_hsv2rgb(v.x, v.y, v.z), 0.0));
}}
"#
        );
        // (h, s, v) -> RGB.
        let cases: [([f32; 4], [f32; 3]); 6] = [
            ([0.0, 1.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
            ([1.0 / 3.0, 1.0, 1.0, 0.0], [0.0, 1.0, 0.0]),
            ([2.0 / 3.0, 1.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            ([0.5, 1.0, 0.5, 0.0], [0.0, 0.5, 0.5]),
            ([0.0, 0.0, 1.0, 0.0], [1.0, 1.0, 1.0]),
            ([0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        ];
        let inputs: Vec<P9Vec4Input> = cases
            .iter()
            .map(|(c, _)| P9Vec4Input { value: *c })
            .collect();
        let outputs = ComputeTest::<P9Vec4Input, P9Vec4Output>::new("p9_hsv2rgb", &wgsl, &inputs)
            .workgroup_size(1)
            .run()
            .expect("shared test adapter is available");
        for (i, (out, (_, expected))) in outputs.iter().zip(cases.iter()).enumerate() {
            for lane in 0..3 {
                assert_close(
                    out.value[lane],
                    expected[lane],
                    1e-4,
                    &format!("p9_hsv2rgb case {i} lane {lane}"),
                );
            }
        }
    }

    /// Execute the shipped `p9_brightness_saturation_contrast` for its
    /// `0.2126/0.7152/0.0722` luma weights. With saturation 0 the output is
    /// exactly that luma, so the
    /// three primary colours reveal each weight directly.
    #[test]
    fn compute_shim_executes_p9_brightness_saturation_contrast_luma_on_gpu() {
        if crate::test_gpu::gpu().is_none() {
            eprintln!("skipping compute-shim test: no GPU adapter in this environment");
            return;
        }
        let wgsl = format!(
            r#"
struct P9DualInput {{ a: vec4<f32>, b: vec4<f32>, }}
struct P9Vec4Output {{ value: vec4<f32>, }}
@group(0) @binding(0) var<storage, read> inputs: array<P9DualInput>;
@group(0) @binding(1) var<storage, read_write> outputs: array<P9Vec4Output>;
{P9_COMMON_WGSL}
{PLANE9_BATCH_WGSL}
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
  let input = inputs[gid.x];
  let c = p9_brightness_saturation_contrast(input.a.xyz, input.b.x, input.b.y, input.b.z);
  outputs[gid.x] = P9Vec4Output(vec4<f32>(c, 0.0));
}}
"#
        );
        // (colour), (brightness, saturation, contrast) -> expected RGB.
        let cases: [([f32; 4], [f32; 4], [f32; 3]); 6] = [
            (
                [1.0, 0.0, 0.0, 0.0],
                [1.0, 0.0, 1.0, 0.0],
                [0.2126, 0.2126, 0.2126],
            ),
            (
                [0.0, 1.0, 0.0, 0.0],
                [1.0, 0.0, 1.0, 0.0],
                [0.7152, 0.7152, 0.7152],
            ),
            (
                [0.0, 0.0, 1.0, 0.0],
                [1.0, 0.0, 1.0, 0.0],
                [0.0722, 0.0722, 0.0722],
            ),
            ([1.0, 1.0, 1.0, 0.0], [0.5, 1.0, 1.0, 0.0], [0.5, 0.5, 0.5]),
            ([1.0, 0.0, 0.0, 0.0], [1.0, 1.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
            ([1.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0], [0.5, 0.5, 0.5]),
        ];
        let inputs: Vec<P9DualInput> = cases
            .iter()
            .map(|(c, p, _)| P9DualInput { a: *c, b: *p })
            .collect();
        let outputs = ComputeTest::<P9DualInput, P9Vec4Output>::new(
            "p9_brightness_saturation_contrast",
            &wgsl,
            &inputs,
        )
        .workgroup_size(1)
        .run()
        .expect("shared test adapter is available");
        assert_eq!(outputs.len(), cases.len());
        for (i, (out, (_, _, expected))) in outputs.iter().zip(cases.iter()).enumerate() {
            for lane in 0..3 {
                assert_close(
                    out.value[lane],
                    expected[lane],
                    1e-5,
                    &format!("brightness_saturation_contrast case {i} lane {lane}"),
                );
            }
        }
    }

    /// Execute the noise family (`p9_fbm`, `p9_noise`, `p9_turbulence` from the
    /// common const and `p9_ridgedmf_fast` from the shading const). Check
    /// bounded, finite, repeatable output; one octave of FBM is exactly one
    /// `p9_noise` sample, which also proves the two shipped
    /// functions really ran rather than a Rust stand-in.
    #[test]
    fn compute_shim_executes_p9_noise_family_range_and_determinism_on_gpu() {
        if crate::test_gpu::gpu().is_none() {
            eprintln!("skipping compute-shim test: no GPU adapter in this environment");
            return;
        }
        let p9_ridgedmf_fast = extract_fn(P9_SHADING_WGSL, "p9_ridgedmf_fast");
        let wgsl = format!(
            r#"
struct P9Vec4Input {{ value: vec4<f32>, }}
struct P9Vec4Output {{ value: vec4<f32>, }}
@group(0) @binding(0) var<storage, read> inputs: array<P9Vec4Input>;
@group(0) @binding(1) var<storage, read_write> outputs: array<P9Vec4Output>;
{P9_COMMON_WGSL}
{p9_ridgedmf_fast}
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
  let v = inputs[gid.x].value;
  let p = v.xyz;
  let octaves = u32(v.w);
  outputs[gid.x] = P9Vec4Output(vec4<f32>(
    p9_fbm(p, octaves),
    p9_noise(p),
    p9_turbulence(p, octaves),
    p9_ridgedmf_fast(p, octaves)
  ));
}}
"#
        );
        let points: [[f32; 4]; 6] = [
            [0.1, 0.2, 0.3, 1.0],
            [0.1, 0.2, 0.3, 1.0], // repeat: bit-identical readback expected
            [1.7, -2.3, 0.4, 3.0],
            [1.7, -2.3, 0.4, 3.0],
            [5.0, 5.0, 5.0, 5.0],
            [-3.1, 0.7, 2.2, 3.0],
        ];
        let inputs: Vec<P9Vec4Input> = points.iter().map(|p| P9Vec4Input { value: *p }).collect();
        let outputs =
            ComputeTest::<P9Vec4Input, P9Vec4Output>::new("p9_noise_family", &wgsl, &inputs)
                .workgroup_size(1)
                .run()
                .expect("shared test adapter is available");
        assert_eq!(outputs.len(), points.len());
        for (i, out) in outputs.iter().enumerate() {
            for (lane, &value) in out.value.iter().enumerate() {
                assert!(
                    value.is_finite(),
                    "p9 noise family case {i} lane {lane}: {value}"
                );
                assert!(
                    (-1.5..=1.5).contains(&value),
                    "p9 noise family case {i} lane {lane} out of range: {value}"
                );
            }
            // Turbulence (abs of noise) and ridged noise are non-negative.
            assert!(
                out.value[2] >= 0.0,
                "p9_turbulence went negative: {:?}",
                out.value
            );
            assert!(
                out.value[3] >= 0.0,
                "p9_ridgedmf_fast went negative: {:?}",
                out.value
            );
        }
        for (i, (fbm, point)) in outputs
            .iter()
            .zip(points.iter())
            .filter(|(_, p)| p[3] == 1.0)
            .enumerate()
        {
            assert_close(
                fbm.value[0],
                fbm.value[1],
                1e-6,
                &format!("one-octave fbm must equal p9_noise (case {i} at {point:?})"),
            );
        }
        for (a, b) in [(0, 1), (2, 3)] {
            for lane in 0..4 {
                assert_eq!(
                    outputs[a].value[lane].to_bits(),
                    outputs[b].value[lane].to_bits(),
                    "p9 noise family must be deterministic: {:?} vs {:?}",
                    outputs[a].value,
                    outputs[b].value
                );
            }
        }
        assert!(
            outputs[0].value[1] != outputs[4].value[1]
                && outputs[0].value[1] != outputs[5].value[1],
            "p9_noise returned the same value at different points"
        );
    }

    /// `p9_rand4` / `p9_perm4_vec2` / `p9_voronoi3` are pin-only batch helpers.
    /// Check the hash is in [0, 1), the perm alias agrees, and voronoi reports
    /// its nearest feature before its second nearest.
    #[test]
    fn compute_shim_executes_p9_batch_hash_and_voronoi_on_gpu() {
        if crate::test_gpu::gpu().is_none() {
            eprintln!("skipping compute-shim test: no GPU adapter in this environment");
            return;
        }
        let wgsl = format!(
            r#"
struct P9Vec4Input {{ value: vec4<f32>, }}
struct P9DualOutput {{ a: vec4<f32>, b: vec4<f32>, }}
@group(0) @binding(0) var<storage, read> inputs: array<P9Vec4Input>;
@group(0) @binding(1) var<storage, read_write> outputs: array<P9DualOutput>;
{P9_COMMON_WGSL}
{PLANE9_BATCH_WGSL}
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
  let p = inputs[gid.x].value.xyz;
  let r = p9_rand4(p.xy);
  let cell = p9_voronoi3(p);
  outputs[gid.x] = P9DualOutput(
    r,
    vec4<f32>(cell.f1, cell.f2, p9_perm4_vec2(p.xy).x, 0.0)
  );
}}
"#
        );
        let points: [[f32; 4]; 4] = [
            [0.1, 0.2, 0.3, 0.0],
            [0.1, 0.2, 0.3, 0.0],
            [2.5, -1.0, 4.0, 0.0],
            [-4.0, 3.25, 0.5, 0.0],
        ];
        let inputs: Vec<P9Vec4Input> = points.iter().map(|p| P9Vec4Input { value: *p }).collect();
        let outputs =
            ComputeTest::<P9Vec4Input, P9DualOutput>::new("p9_batch_hash_voronoi", &wgsl, &inputs)
                .workgroup_size(1)
                .run()
                .expect("shared test adapter is available");
        assert_eq!(outputs.len(), points.len());
        for (i, out) in outputs.iter().enumerate() {
            for lane in 0..4 {
                assert!(
                    (0.0..1.0).contains(&out.a[lane]),
                    "p9_rand4 case {i} lane {lane} outside [0, 1): {:?}",
                    out.a
                );
            }
            assert_close(out.b[2], out.a[0], 1e-7, "p9_perm4_vec2 is p9_rand4");
            assert!(
                out.b[1] >= out.b[0] && out.b[0] >= 0.0,
                "p9_voronoi3 must order f1 <= f2: {:?}",
                out.b
            );
        }
        for lane in 0..4 {
            assert_eq!(
                outputs[0].a[lane].to_bits(),
                outputs[1].a[lane].to_bits(),
                "p9_rand4 must be deterministic"
            );
        }
        assert_ne!(
            outputs[0].a, outputs[2].a,
            "p9_rand4 is constant across points"
        );
    }
}
