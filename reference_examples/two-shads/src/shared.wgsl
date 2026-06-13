// Shared prelude prepended to every shad's fragment shader: the uniform
// struct and a fullscreen-triangle vertex shader. The render pass sets the
// VIEWPORT to the shad's rect, so this triangle covers exactly that rect
// and `uv` runs 0..1 across it (y-down, like screen coordinates).

struct U {
  rect: vec4<f32>, // x, y, w, h in window pixels
  time: f32,
  // three scalar pads (not a vec3, which would align to 16) -> 32 bytes total
  _pad0: f32,
  _pad1: f32,
  _pad2: f32,
};
@group(0) @binding(0) var<uniform> u: U;

struct VOut {
  @builtin(position) clip: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VOut {
  // one oversized triangle: vertices (0,0) (2,0) (0,2) in uv space
  let xy = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
  var o: VOut;
  o.clip = vec4(xy.x * 2.0 - 1.0, 1.0 - xy.y * 2.0, 0.0, 1.0);
  o.uv = xy;
  return o;
}
