// Plane9 bloom PASS4 — UpsampleCircular.
//
// Eight taps around the pixel: the four axis neighbours weigh 1 and the four
// diagonals weigh 2 (total 12, hence /12). The result is added to the finer
// level's own box-blurred mip and multiplied by the driver's `gScale`
// (`extra.x`), which is `intensity` only on the final level.
//
// `extra.y == 0` is the one-mip finalize path: the box result is copied
// through untouched, so `intensity` never applies.
struct Plane9Params {
  header: vec4<f32>,
  extra: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: Plane9Params;
@group(0) @binding(1) var base_texture: texture_2d<f32>;
@group(0) @binding(2) var blur_texture: texture_2d<f32>;
@group(0) @binding(3) var blur_sampler: sampler;

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let uv = position.xy / vec2<f32>(textureDimensions(base_texture));
  let base = textureSample(base_texture, blur_sampler, uv);
  if (params.extra.y == 0.0) {
    return base;
  }

  let t = params.header.xy;
  let left = textureSample(blur_texture, blur_sampler, uv - vec2<f32>(t.x, 0.0));
  let right = textureSample(blur_texture, blur_sampler, uv + vec2<f32>(t.x, 0.0));
  let top = textureSample(blur_texture, blur_sampler, uv - vec2<f32>(0.0, t.y));
  let bottom = textureSample(blur_texture, blur_sampler, uv + vec2<f32>(0.0, t.y));
  let top_left = textureSample(blur_texture, blur_sampler, uv + vec2<f32>(-t.x, -t.y));
  let top_right = textureSample(blur_texture, blur_sampler, uv + vec2<f32>(t.x, -t.y));
  let bottom_left = textureSample(blur_texture, blur_sampler, uv + vec2<f32>(-t.x, t.y));
  let bottom_right = textureSample(blur_texture, blur_sampler, uv + vec2<f32>(t.x, t.y));

  let blur = (left + right + top + bottom
    + (top_left + top_right + bottom_left + bottom_right) * 2.0) / 12.0;
  return (base + blur) * params.extra.x;
}
