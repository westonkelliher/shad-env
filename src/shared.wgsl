// Shared prelude prepended to every registered fragment shader. Defines the
// universal uniform `u` (must match `Uniforms` in lib.rs field-for-field) and
// a fullscreen-triangle vertex shader. The render pass sets the VIEWPORT to the
// shad's rect, so this triangle covers exactly that rect and `uv` runs 0..1
// across it (y-down, like screen coordinates).

struct U {
  // --- engine builtins (written every frame by render()) ---
  rect: vec4<f32>,       // x, y, w, h in window pixels
  resolution: vec2<f32>, // this shad's rect size in px
  mouse: vec2<f32>,      // cursor in shad-local px (reserved; 0 for now)
  time: f32,             // seconds since ShadEnv::new()
  _pad0: f32,
  _pad1: f32,
  _pad2: f32,
  // --- generic user slots (set via set_uniform_value) ---
  scalars: vec4<f32>,        // "s0".."s3" = scalars.x .. scalars.w
  vecs: array<vec4<f32>, 4>, // "v0".."v3"
};
@group(0) @binding(0) var<uniform> u: U;

// Generic data sources -- their MEANING is the shader's business: `tex` may be a
// font atlas, an LUT, a heightmap; `buf` may be a string, curve data, a tilemap.
// Shads that set neither get a 1x1 white texture and a 1-element buffer.
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var<storage, read> buf: array<u32>;

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
