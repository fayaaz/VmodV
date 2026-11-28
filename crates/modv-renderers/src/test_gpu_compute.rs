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
    use crate::shader::uniforms::GLSL_MOD_F_WGSL;

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
}
