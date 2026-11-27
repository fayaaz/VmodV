// Plane9 bloom PASS2 — Downsample36.
//
// 13 taps over a 5x5 neighbourhood, each weighted by 1 / (1 + brightness) so a
// single very bright pixel cannot dominate the smaller mip (the driver's
// firefly rejection). The weights are normalised by their sum.
struct Plane9Params {
  header: vec4<f32>,
  extra: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: Plane9Params;
@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;

fn bright_weight(colour: vec4<f32>) -> f32 {
  let brightness = max(colour.r, max(colour.g, colour.b));
  return 1.0 / (1.0 + max(brightness, 0.0));
}

fn accumulate(
  sum: ptr<function, vec4<f32>>,
  total: ptr<function, f32>,
  uv: vec2<f32>,
) {
  let colour = textureSample(source_texture, source_sampler, uv);
  let weight = bright_weight(colour);
  *sum = *sum + colour * weight;
  *total = *total + weight;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let t = params.header.xy;
  let uv = position.xy * t;
  var sum = vec4<f32>(0.0);
  var total = 0.0;

  accumulate(&sum, &total, uv);
  accumulate(&sum, &total, uv + vec2<f32>(t.x, 0.0));
  accumulate(&sum, &total, uv - vec2<f32>(t.x, 0.0));
  accumulate(&sum, &total, uv + vec2<f32>(0.0, t.y));
  accumulate(&sum, &total, uv - vec2<f32>(0.0, t.y));
  accumulate(&sum, &total, uv + vec2<f32>(t.x, t.y));
  accumulate(&sum, &total, uv + vec2<f32>(-t.x, t.y));
  accumulate(&sum, &total, uv + vec2<f32>(t.x, -t.y));
  accumulate(&sum, &total, uv + vec2<f32>(-t.x, -t.y));
  accumulate(&sum, &total, uv + vec2<f32>(2.0 * t.x, 0.0));
  accumulate(&sum, &total, uv - vec2<f32>(2.0 * t.x, 0.0));
  accumulate(&sum, &total, uv + vec2<f32>(0.0, 2.0 * t.y));
  accumulate(&sum, &total, uv - vec2<f32>(0.0, 2.0 * t.y));

  return sum / total;
}
