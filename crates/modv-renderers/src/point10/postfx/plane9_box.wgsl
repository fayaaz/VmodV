// Plane9 bloom PASS3 — 4-tap box blur at one mip level.
struct Plane9Params {
  header: vec4<f32>,
  extra: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: Plane9Params;
@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let texel = params.header.xy;
  let uv = position.xy * texel;
  let half_texel = texel * 0.5;
  let a = textureSample(source_texture, source_sampler, uv + vec2<f32>(-half_texel.x, -half_texel.y));
  let b = textureSample(source_texture, source_sampler, uv + vec2<f32>(half_texel.x, -half_texel.y));
  let c = textureSample(source_texture, source_sampler, uv + vec2<f32>(-half_texel.x, half_texel.y));
  let d = textureSample(source_texture, source_sampler, uv + vec2<f32>(half_texel.x, half_texel.y));
  return (a + b + c + d) * 0.25;
}
