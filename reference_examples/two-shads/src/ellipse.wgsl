// A breathing ellipse inscribed in the rect.

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
  let p = in.uv * 2.0 - 1.0;                  // -1..1 across the rect
  let radii = vec2(0.9, 0.7) * (1.0 + 0.07 * sin(u.time * 2.0));
  let d = length(p / radii) - 1.0;            // <0 inside the ellipse
  let aa = fwidth(d);                          // ~one pixel, for antialiasing
  let alpha = 1.0 - smoothstep(-aa, aa, d);
  return vec4(0.95, 0.55, 0.25, alpha);
}
