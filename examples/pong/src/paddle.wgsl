// A paddle drawn inside a FIXED full-height strip. The host passes the paddle's
// vertical position and size through the uniform:
//   s0 = paddle center y, normalized 0..1 down the strip
//   s1 = paddle half-height, normalized
// (this is the "pass in paddle height through a bind group" from the spec.)

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
  let center = u.scalars.x;
  let half = u.scalars.y;
  let d = abs(in.uv.y - center) - half;   // <0 inside the paddle bar
  let aa = fwidth(d);
  let alpha = 1.0 - smoothstep(-aa, aa, d);
  return vec4(0.85, 0.9, 1.0, alpha);
}
