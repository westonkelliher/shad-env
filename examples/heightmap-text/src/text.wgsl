// Renders glyph quads, treating each glyph image (a layer of a
// texture_2d_array) as a HEIGHTMAP:
//
//   1. blur the sharp glyph image with a gaussian -> smooth height ramp
//   2. sqrt() the height -> curves the linear ramp into a rounded bevel
//   3. central-difference the height -> surface normal
//   4. diffuse lighting: color * max(-dot(normal, light_dir), 0)

struct U {
  resolution:   vec2<f32>,
  height_scale: f32,       // exaggerates the gradient -> steeper-looking bevels
  shininess:    f32,       // (unused in diffuse-only mode, kept for experiments)
  light_dir:    vec3<f32>, // points TOWARD the light (directional / infinitely far)
  intensity:    f32,
  light_color:  vec3<f32>,
  spec_strength:f32,       // (unused, see shininess)
  albedo:       vec3<f32>, // material color
  dog_sigma:    f32,       // gaussian blur width in texels
  mode:         u32,       // 0 = lit, 1 = show heightmap, 2 = show normals
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var font: texture_2d_array<f32>;
@group(0) @binding(2) var samp: sampler;

// per-instance data (one instance = one glyph quad), see Glyph in main.rs
struct Inst {
  @location(0) pos:   vec2<f32>,
  @location(1) size:  vec2<f32>,
  @location(2) layer: u32,
};

struct VOut {
  @builtin(position) clip: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) @interpolate(flat) layer: u32,
};

// Expand 4 triangle-strip vertices into the instance's screen rect.
@vertex
fn vs(@builtin(vertex_index) vi: u32, inst: Inst) -> VOut {
  let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u)); // (0,0)(1,0)(0,1)(1,1)
  let px  = inst.pos + corner * inst.size;
  let ndc = vec2(px.x / u.resolution.x * 2.0 - 1.0,
                 1.0 - px.y / u.resolution.y * 2.0);   // pixels (y-down) -> NDC (y-up)
  var o: VOut;
  o.clip  = vec4(ndc, 0.0, 1.0);
  o.uv    = corner;
  o.layer = inst.layer;
  return o;
}

const R: i32 = 5; // gaussian kernel half-width in texels

fn raw_height(uv: vec2<f32>, layer: u32) -> f32 {
  return textureSampleLevel(font, samp, uv, layer, 0.0).r;
}

// gaussian-blurred height (0-1): turns the sharp glyph edge into a smooth ramp
fn blurred_height(uv: vec2<f32>, layer: u32, texel: vec2<f32>) -> f32 {
  let s2 = 2.0 * u.dog_sigma * u.dog_sigma;
  var sum = 0.0;
  var wsum = 0.0;
  for (var j: i32 = -R; j <= R; j = j + 1) {
    for (var i: i32 = -R; i <= R; i = i + 1) {
      let w = exp(-f32(i * i + j * j) / s2);
      sum  = sum + w * raw_height(uv + vec2(f32(i), f32(j)) * texel, layer);
      wsum = wsum + w;
    }
  }
  return sum / wsum;
}

// the height we actually light: sqrt() steepens the ramp near zero, turning
// the gaussian S-curve into a rounded bevel profile
fn height(uv: vec2<f32>, layer: u32, texel: vec2<f32>) -> f32 {
  return sqrt(clamp(blurred_height(uv, layer, texel), 0.0, 1.0));
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
  let texel = 1.0 / vec2<f32>(textureDimensions(font));

  // surface normal from the height gradient (central difference)
  let h  = height(in.uv, in.layer, texel);
  let ex = vec2(texel.x, 0.0);
  let ey = vec2(0.0, texel.y);
  let hx = height(in.uv + ex, in.layer, texel) - height(in.uv - ex, in.layer, texel);
  let hy = height(in.uv + ey, in.layer, texel) - height(in.uv - ey, in.layer, texel);
  let N  = normalize(vec3(-hx * u.height_scale, -hy * u.height_scale, 1.0));

  // discard background so overlapping glyph cells don't paint over neighbors
  if (h < 0.06) { discard; }

  // debug views
  if (u.mode == 1u) { return vec4(vec3(h), 1.0); }       // heightmap
  if (u.mode == 2u) { return vec4(N * 0.5 + 0.5, 1.0); } // normals as RGB

  // diffuse: surface facing the light is bright, facing away is dark
  let L = normalize(u.light_dir);
  let n_dot_l = max(-dot(N, L), 0.0);
  let lit = u.albedo * u.light_color * (u.intensity * n_dot_l);

  let coverage = smoothstep(0.06, 0.7, h); // soft alpha at the glyph edge
  return vec4(lit, coverage);
}
