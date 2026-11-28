//! GPU-executed numeric tests for Point10 pass maths currently pinned only by
//! exact shader-source strings in the pass modules.
//!
//! The three passes here are driven through private pipeline structs (a final
//! pass inside `VolumeStage`, the Unreal composite with its five private blur
//! views), so a full render harness would require editing those modules.
//! Instead each test builds a compute shim around the **verbatim statements
//! sliced out of the shipped WGSL**: the arithmetic that runs on the GPU is
//! the shipped source text, and Rust only supplies inputs and literal expected
//! results. No pass maths is reimplemented in Rust.
//!
//! Like the rest of the crate's GPU tests, every test skips (with a log line)
//! when there is no adapter. The shim dispatches one workgroup per test, so
//! the added runtime is bounded by a handful of tiny dispatches.

#![cfg(test)]

use bytemuck::{Pod, Zeroable};

use crate::test_gpu_compute::ComputeTest;

/// Trimmed source line that first contains `needle`, or a descriptive panic.
/// The returned slice borrows the shipped `include_str!` source.
fn shipped_line<'a>(source: &'a str, needle: &str) -> &'a str {
    let at = source
        .find(needle)
        .unwrap_or_else(|| panic!("shipped shader lost the `{needle}` statement"));
    let start = source[..at].rfind('\n').map_or(0, |index| index + 1);
    let end = source[at..]
        .find('\n')
        .map_or(source.len(), |index| at + index);
    source[start..end].trim()
}

/// The expression of the shader's `return` statement, verbatim.
fn shipped_return_expr(source: &str) -> &str {
    shipped_line(source, "return ")
        .strip_prefix("return ")
        .and_then(|rest| rest.strip_suffix(';'))
        .unwrap_or_else(|| panic!("shader return statement is not `return <expr>;`"))
        .trim()
}

/// Statements from `start_marker` up to (but not including) the closing brace.
/// The shipped composite body has no nested braces.
fn shipped_body(source: &str, start_marker: &str) -> String {
    let at = source
        .find(start_marker)
        .unwrap_or_else(|| panic!("shipped shader lost `{start_marker}`"));
    let rest = &source[at..];
    let end = rest
        .find("\n}")
        .unwrap_or_else(|| panic!("shipped shader body has no closing brace"));
    rest[..end].to_string()
}

fn assert_close(actual: &[f32], expected: &[f32], label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}: lane count");
    for (lane, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (actual - expected).abs() < 1e-5,
            "{label} lane {lane}: GPU returned {actual}, expected {expected} (full output {actual:?})"
        );
    }
}

// ---------------------------------------------------------------------------
// Afterimage: `damped = old * damp * (old > 0.1)`, then `max(new, damped)`.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct AfterimageInput {
    new_sample: [f32; 4],
    old: [f32; 4],
    params: [f32; 4],
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct AfterimageOutput {
    value: [f32; 4],
}

fn afterimage_shim() -> String {
    let source = include_str!("point10/postfx/afterimage.wgsl");
    let damped = shipped_line(source, "let damped =");
    let result = shipped_return_expr(source);
    // The uniform stand-in mirrors `AfterimageUniforms` (16 bytes, pinned by
    // the pass module's own size test); only the shader's arithmetic is used.
    format!(
        r#"
struct AfterimageInput {{ new_sample: vec4<f32>, old: vec4<f32>, params: vec4<f32>, }}
struct AfterimageOutput {{ value: vec4<f32>, }}
struct AfterimageUniforms {{ params: vec4<f32>, }}
@group(0) @binding(0) var<storage, read> inputs: array<AfterimageInput>;
@group(0) @binding(1) var<storage, read_write> outputs: array<AfterimageOutput>;
var<private> u: AfterimageUniforms;
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
  let i = gid.x;
  let new_sample = inputs[i].new_sample;
  let old = inputs[i].old;
  u.params = inputs[i].params;
  {damped}
  outputs[i] = AfterimageOutput({result});
}}
"#
    )
}

#[test]
fn gpu_afterimage_gates_history_then_takes_the_maximum() {
    if crate::test_gpu::gpu().is_none() {
        eprintln!("skipping GPU afterimage test: no GPU adapter in this environment");
        return;
    }
    let wgsl = afterimage_shim();
    let inputs = [
        // History above the gate survives damping and wins over the new frame.
        AfterimageInput {
            new_sample: [0.1, 0.1, 0.1, 0.1],
            old: [0.5, 0.5, 0.5, 0.5],
            params: [0.8, 0.0, 0.0, 0.0],
        },
        // History at/below the gate is discarded; the new frame must survive.
        AfterimageInput {
            new_sample: [0.2, 0.2, 0.2, 0.2],
            old: [0.05, 0.05, 0.05, 0.05],
            params: [0.9, 0.0, 0.0, 0.0],
        },
        // The maximum picks the new frame when it is the larger operand.
        AfterimageInput {
            new_sample: [0.9, 0.9, 0.9, 0.9],
            old: [0.2, 0.2, 0.2, 0.2],
            params: [0.5, 0.0, 0.0, 0.0],
        },
        // Per-lane gate: the fourth lane's history is exactly 0.1 (`>` not
        // `>=`), so it is discarded while its siblings are damped.
        AfterimageInput {
            new_sample: [0.0, 0.3, 0.1, 0.0],
            old: [0.2, 0.05, 0.4, 0.1],
            params: [0.5, 0.0, 0.0, 0.0],
        },
        // Negative history also fails the gate.
        AfterimageInput {
            new_sample: [0.15, 0.0, 0.0, 0.0],
            old: [-0.5, 0.0, 0.0, 0.0],
            params: [2.0, 0.0, 0.0, 0.0],
        },
    ];
    let outputs = ComputeTest::<AfterimageInput, AfterimageOutput>::new(
        "afterimage-gate-max",
        &wgsl,
        &inputs,
    )
    .workgroup_size(1)
    .run()
    .expect("shared test adapter is available");
    assert_eq!(outputs.len(), inputs.len());

    let expected = [
        [0.4, 0.4, 0.4, 0.4], // 0.5 * 0.8
        [0.2, 0.2, 0.2, 0.2],
        [0.9, 0.9, 0.9, 0.9],
        [0.1, 0.3, 0.2, 0.0], // 0.2*0.5, gate, 0.4*0.5, gate
        [0.15, 0.0, 0.0, 0.0],
    ];
    for (index, (output, expected)) in outputs.iter().zip(expected).enumerate() {
        assert_close(&output.value, &expected, &format!("afterimage row {index}"));
    }
}

// ---------------------------------------------------------------------------
// Volume final pass: `rgb = scene + bloom`, `alpha = max(rgb)` in overlay mode,
// `1.0` otherwise, with the max clamped to [0, 1].
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct FinalInput {
    scene: [f32; 4],
    bloom: [f32; 4],
    overlay_and_pad: [f32; 4],
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct FinalOutput {
    value: [f32; 4],
}

fn final_shim() -> String {
    let source = include_str!("point10/volume/passes/final.wgsl");
    let rgb = shipped_line(source, "let rgb = scene.rgb");
    let overlay_alpha = shipped_line(source, "let overlay_alpha = clamp(");
    let alpha = shipped_line(source, "let alpha = select(");
    let result = shipped_return_expr(source);
    // Stand-in mirrors `FinalUniforms` (16 bytes, pinned in stage.rs).
    format!(
        r#"
struct FinalInput {{ scene: vec4<f32>, bloom: vec4<f32>, overlay_and_pad: vec4<f32>, }}
struct FinalOutput {{ value: vec4<f32>, }}
struct FinalUniforms {{ resolution: vec2<f32>, overlay: f32, _pad: f32, }}
@group(0) @binding(0) var<storage, read> inputs: array<FinalInput>;
@group(0) @binding(1) var<storage, read_write> outputs: array<FinalOutput>;
var<private> u: FinalUniforms;
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
  let i = gid.x;
  let scene = inputs[i].scene;
  let bloom = inputs[i].bloom;
  u.overlay = inputs[i].overlay_and_pad.x;
  {rgb}
  {overlay_alpha}
  {alpha}
  outputs[i] = FinalOutput({result});
}}
"#
    )
}

#[test]
fn gpu_final_pass_adds_bloom_and_applies_the_alpha_rule() {
    if crate::test_gpu::gpu().is_none() {
        eprintln!("skipping GPU final-pass test: no GPU adapter in this environment");
        return;
    }
    let wgsl = final_shim();
    let inputs = [
        // Default mode: additive bloom, opaque alpha regardless of sources.
        FinalInput {
            scene: [0.2, 0.3, 0.4, 1.0],
            bloom: [0.1, 0.1, 0.1, 0.9],
            overlay_and_pad: [0.0, 0.0, 0.0, 0.0],
        },
        // Overlay mode: alpha comes from max(rgb), not the scene's alpha.
        FinalInput {
            scene: [0.2, 0.3, 0.4, 1.0],
            bloom: [0.1, 0.1, 0.1, 0.9],
            overlay_and_pad: [1.0, 0.0, 0.0, 0.0],
        },
        FinalInput {
            scene: [0.1, 0.05, 0.02, 1.0],
            bloom: [0.0, 0.0, 0.0, 1.0],
            overlay_and_pad: [1.0, 0.0, 0.0, 0.0],
        },
        // HDR rgb stays unclamped; only the overlay alpha is clamped.
        FinalInput {
            scene: [1.5, 0.2, 0.3, 0.0],
            bloom: [0.5, 0.0, 0.0, 0.0],
            overlay_and_pad: [1.0, 0.0, 0.0, 0.0],
        },
        // The override switches strictly above 0.5 (exactly 0.5 stays opaque).
        FinalInput {
            scene: [0.4, 0.2, 0.1, 1.0],
            bloom: [0.0, 0.0, 0.0, 0.0],
            overlay_and_pad: [0.5, 0.0, 0.0, 0.0],
        },
    ];
    let outputs = ComputeTest::<FinalInput, FinalOutput>::new("volume-final-pass", &wgsl, &inputs)
        .workgroup_size(1)
        .run()
        .expect("shared test adapter is available");
    assert_eq!(outputs.len(), inputs.len());

    let expected = [
        [0.3, 0.4, 0.5, 1.0],
        [0.3, 0.4, 0.5, 0.5],
        [0.1, 0.05, 0.02, 0.1],
        [2.0, 0.2, 0.3, 1.0],
        [0.4, 0.2, 0.1, 1.0],
    ];
    for (index, (output, expected)) in outputs.iter().zip(expected).enumerate() {
        assert_close(&output.value, &expected, &format!("final pass row {index}"));
    }
}

// ---------------------------------------------------------------------------
// Unreal bloom composite: tap-weighted mip sum multiplied by
// `bloom_scale * bloom_strength`; alpha is max(rgb).
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct CompositeInput {
    taps: [[f32; 4]; 5],
    blur1: [f32; 4],
    blur2: [f32; 4],
    blur3: [f32; 4],
    blur4: [f32; 4],
    blur5: [f32; 4],
    // x = bloom_strength, y = bloom_radius, z = bloom_scale.
    params: [f32; 4],
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct CompositeOutput {
    value: [f32; 4],
}

fn bloom_composite_shim() -> String {
    let source = include_str!("point10/postfx/bloom_composite.wgsl");
    let mut body = shipped_body(source, "var bloom = vec3<f32>(0.0);");
    // The compute shim has no textures: feed each mip's sampled rgb as a typed
    // input. Only the texture fetch is substituted; every arithmetic operator
    // and the scale multiplication remain the shipped source text.
    for name in ["blur1", "blur2", "blur3", "blur4", "blur5"] {
        let fetch = format!("textureSample({name}, blur_sampler, uv).rgb");
        assert!(body.contains(&fetch), "composite lost its {name} fetch");
        body = body.replace(&fetch, &format!("inputs[i].{name}.xyz"));
    }
    let result = shipped_return_expr(&body);
    body = body.replace(
        &format!("return {result};"),
        &format!("outputs[i] = CompositeOutput({result});"),
    );
    format!(
        r#"
struct CompositeInput {{
  taps: array<vec4<f32>, 5>,
  blur1: vec4<f32>, blur2: vec4<f32>, blur3: vec4<f32>,
  blur4: vec4<f32>, blur5: vec4<f32>,
  params: vec4<f32>,
}}
struct CompositeOutput {{ value: vec4<f32>, }}
struct CompositeUniforms {{
  bloom_strength: f32, bloom_radius: f32, bloom_scale: f32, _pad: f32,
  taps: array<vec4<f32>, 5>,
}}
@group(0) @binding(0) var<storage, read> inputs: array<CompositeInput>;
@group(0) @binding(1) var<storage, read_write> outputs: array<CompositeOutput>;
var<private> u: CompositeUniforms;
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
  let i = gid.x;
  u.bloom_strength = inputs[i].params.x;
  u.bloom_radius = inputs[i].params.y;
  u.bloom_scale = inputs[i].params.z;
  u.taps[0] = inputs[i].taps[0];
  u.taps[1] = inputs[i].taps[1];
  u.taps[2] = inputs[i].taps[2];
  u.taps[3] = inputs[i].taps[3];
  u.taps[4] = inputs[i].taps[4];
  {body}
}}
"#
    )
}

#[test]
fn gpu_bloom_composite_scale_one_and_three_are_calibrated() {
    if crate::test_gpu::gpu().is_none() {
        eprintln!("skipping GPU bloom-composite test: no GPU adapter in this environment");
        return;
    }
    let wgsl = bloom_composite_shim();
    // One active tap with a white tint keeps the expected sum hand-computable:
    // blur1.rgb = (0.25, 0.5, 0.75), strength = 1.
    let taps = [
        [1.0, 1.0, 1.0, 1.0],
        [0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
    ];
    let inputs = [
        CompositeInput {
            taps,
            blur1: [0.25, 0.5, 0.75, 0.0],
            blur2: [0.0; 4],
            blur3: [0.0; 4],
            blur4: [0.0; 4],
            blur5: [0.0; 4],
            // strength, radius, scale.
            params: [1.0, 0.0, 1.0, 0.0],
        },
        CompositeInput {
            taps,
            blur1: [0.25, 0.5, 0.75, 0.0],
            blur2: [0.0; 4],
            blur3: [0.0; 4],
            blur4: [0.0; 4],
            blur5: [0.0; 4],
            params: [1.0, 0.0, 3.0, 0.0],
        },
        // Two taps prove the per-mip weighting runs on the GPU before the
        // global scale: 0.5*(0.2,0.1,0.0) + 0.25*(0.0,0.4,0.8) =
        // (0.1, 0.15, 0.2), then scale 2 -> (0.2, 0.3, 0.4).
        CompositeInput {
            taps: [
                [0.5, 1.0, 1.0, 1.0],
                [0.0, 0.0, 0.0, 0.0],
                [0.25, 0.0, 1.0, 1.0],
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
            ],
            blur1: [0.2, 0.1, 0.0, 0.0],
            blur2: [0.0; 4],
            blur3: [0.0, 0.4, 0.8, 0.0],
            blur4: [0.0; 4],
            blur5: [0.0; 4],
            params: [1.0, 0.0, 2.0, 0.0],
        },
    ];
    let outputs = ComputeTest::<CompositeInput, CompositeOutput>::new(
        "bloom-composite-scale",
        &wgsl,
        &inputs,
    )
    .workgroup_size(1)
    .run()
    .expect("shared test adapter is available");
    assert_eq!(outputs.len(), inputs.len());

    let expected = [
        [0.25, 0.5, 0.75, 0.75],
        [0.75, 1.5, 2.25, 2.25],
        [0.2, 0.3, 0.4, 0.4],
    ];
    for (index, (output, expected)) in outputs.iter().zip(expected).enumerate() {
        assert_close(&output.value, &expected, &format!("composite row {index}"));
    }

    // The calibration assertion: scale 3 is exactly three times scale 1.
    for lane in 0..4 {
        assert!(
            (outputs[1].value[lane] - 3.0 * outputs[0].value[lane]).abs() < 1e-5,
            "bloom scale 3 must triple scale 1: lane {lane} was {} vs {}",
            outputs[1].value[lane],
            outputs[0].value[lane]
        );
    }
}
