// A rounded rectangle with an animated corner radius (SDF-based).

// classic signed-distance function for a box with rounded corners;
// p is relative to the box center, `half` is the half-size, r the radius
fn sd_round_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
  let q = abs(p) - half + vec2(r);
  return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
  // work in PIXELS (centered on the rect) so the corners stay circular
  // regardless of the rect's aspect ratio
  let p = (in.uv - 0.5) * u.rect.zw;
  let half = u.rect.zw * 0.5 - 16.0;          // inset 16px from the rect edge
  let r = 45.0 + 35.0 * sin(u.time);          // corner radius sweeps 10..80px
  let d = sd_round_box(p, half, r);
  let alpha = 1.0 - smoothstep(-1.0, 1.0, d); // d is in pixels: 2px soft edge
  return vec4(0.35, 0.75, 0.55, alpha);
}
