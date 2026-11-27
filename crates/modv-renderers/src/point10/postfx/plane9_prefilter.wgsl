// Plane9 bloom prefilter — PASS0 (1 tap) / PASS1 (prefilter + median, 5 taps).
//
// The driver's knee is (threshold - threshold*smoothness,
// 2*threshold*smoothness, 0.25/(threshold*smoothness)). `header.z` is the knee
// start, `header.w` its width, `extra.x` the scale (0 when the product is 0, in
// which case the cut is hard at the start).
struct Plane9Params {
  header: vec4<f32>,
  extra: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: Plane9Params;
@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;

fn luminance(colour: vec3<f32>) -> f32 {
  return dot(vec3<f32>(0.2126, 0.7152, 0.0722), colour);
}

fn knee(colour: vec4<f32>) -> vec4<f32> {
  let luma = luminance(colour.rgb);
  let over = luma - params.header.z;
  var t: f32;
  if (params.extra.x > 0.0) {
    t = clamp(over * params.extra.x, 0.0, 1.0);
  } else {
    t = select(0.0, 1.0, over > 0.0);
  }
  return vec4<f32>(colour.rgb * t, t);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let texel = params.header.xy;
  return knee(textureSample(source_texture, source_sampler, position.xy * texel));
}

// PASS1: prefilter each of five taps, then a centre-weighted median-ish blend.
@fragment
fn fs_median(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let texel = params.header.xy;
  let uv = position.xy * texel;
  let centre = knee(textureSample(source_texture, source_sampler, uv));
  let left = knee(textureSample(source_texture, source_sampler, uv - vec2<f32>(texel.x, 0.0)));
  let right = knee(textureSample(source_texture, source_sampler, uv + vec2<f32>(texel.x, 0.0)));
  let top = knee(textureSample(source_texture, source_sampler, uv - vec2<f32>(0.0, texel.y)));
  let bottom = knee(textureSample(source_texture, source_sampler, uv + vec2<f32>(0.0, texel.y)));
  return (centre * 2.0 + left + right + top + bottom) / 6.0;
}
