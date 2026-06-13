// A single 7-segment digit for the score counters.
//   s0 = the digit value 0..9 (the host pushes the score in)
// Segment layout (uv 0..1, y-down):
//   a = top, b = top-right, c = bottom-right, d = bottom,
//   e = bottom-left, f = top-left, g = middle.

fn seg_mask(d: i32) -> u32 {
  // bit order: a=1 b=2 c=4 d=8 e=16 f=32 g=64
  var table = array<u32, 10>(63u, 6u, 91u, 79u, 102u, 109u, 125u, 7u, 127u, 111u);
  return table[clamp(d, 0, 9)];
}

// signed-distance to a horizontal segment centered at (cx,cy), half-length hl.
fn hseg(p: vec2<f32>, cx: f32, cy: f32, hl: f32, t: f32) -> f32 {
  let q = abs(p - vec2(cx, cy)) - vec2(hl, t);
  return max(q.x, q.y);
}
fn vseg(p: vec2<f32>, cx: f32, cy: f32, hl: f32, t: f32) -> f32 {
  let q = abs(p - vec2(cx, cy)) - vec2(t, hl);
  return max(q.x, q.y);
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
  let p = in.uv;                 // 0..1
  let mask = seg_mask(i32(round(u.scalars.x)));
  let t = 0.07;                  // segment half-thickness
  let on = vec3(1.0, 0.85, 0.3);

  // big distance = "off"; take the min over lit segments
  var d = 1e9;
  if ((mask & 1u)  != 0u) { d = min(d, hseg(p, 0.5, 0.12, 0.22, t)); } // a
  if ((mask & 2u)  != 0u) { d = min(d, vseg(p, 0.74, 0.31, 0.16, t)); } // b
  if ((mask & 4u)  != 0u) { d = min(d, vseg(p, 0.74, 0.69, 0.16, t)); } // c
  if ((mask & 8u)  != 0u) { d = min(d, hseg(p, 0.5, 0.88, 0.22, t)); } // d
  if ((mask & 16u) != 0u) { d = min(d, vseg(p, 0.26, 0.69, 0.16, t)); } // e
  if ((mask & 32u) != 0u) { d = min(d, vseg(p, 0.26, 0.31, 0.16, t)); } // f
  if ((mask & 64u) != 0u) { d = min(d, hseg(p, 0.5, 0.5, 0.22, t)); }  // g

  let aa = fwidth(d);
  let alpha = 1.0 - smoothstep(-aa, aa, d);
  return vec4(on, alpha);
}
