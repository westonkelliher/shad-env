// The ball: a filled disc inscribed in its (small, ball-tracking) rect.

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
  let p = in.uv * 2.0 - 1.0;          // -1..1 across the rect
  let d = length(p) - 0.9;            // <0 inside the disc
  let aa = fwidth(d);
  let alpha = 1.0 - smoothstep(-aa, aa, d);
  return vec4(0.95, 0.95, 0.95, alpha);
}
