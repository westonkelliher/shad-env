// Renders glyph quads. Each glyph's texture (a layer in a texture_2d_array) is
// treated as a HEIGHTMAP. The source is a SHARP font render, so the surface
// angle is estimated with a directional difference-of-Gaussians (a derivative-
// of-gaussian kernel): a gaussian-weighted central difference of the height
// along each axis. That gives a smooth, stable gradient from crisp input.

struct U {
  resolution:   vec2<f32>,
  height_scale: f32,       // exaggerates the gradient -> normal strength
  shininess:    f32,
  light_dir:    vec3<f32>, // points TOWARD the light (infinitely far away)
  intensity:    f32,
  light_color:  vec3<f32>,
  spec_strength:f32,
  albedo:       vec3<f32>, // fixed material color
  dog_sigma:    f32,       // gaussian width in texels for the angle estimate
  mode:         u32,       // 0 = lit, 1 = heightmap, 2 = normals
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var font: texture_2d_array<f32>;
@group(0) @binding(2) var samp: sampler;

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

@vertex
fn vs(@builtin(vertex_index) vi: u32, inst: Inst) -> VOut {
  let c  = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)); // (0,0)(1,0)(0,1)(1,1)
  let px = inst.pos + c * inst.size;
  let ndc = vec2(px.x / u.resolution.x * 2.0 - 1.0,
                 1.0 - px.y / u.resolution.y * 2.0);       // y-down -> NDC
  var o: VOut;
  o.clip  = vec4(ndc, 0.0, 1.0);
  o.uv    = c;
  o.layer = inst.layer;
  return o;
}

const R: i32 = 5;  // kernel half-width in texels

fn h(uv: vec2<f32>, layer: u32) -> f32 {
  return textureSampleLevel(font, samp, uv, layer, 0.0).r;
}

// gaussian-smoothed height (0-1)
fn smooth_h(uv: vec2<f32>, layer: u32, texel: vec2<f32>) -> f32 {
  let s2 = 2.0 * u.dog_sigma * u.dog_sigma;
  var hs = 0.0;
  var wsum = 0.0;
  for (var j: i32 = -R; j <= R; j = j + 1) {
    for (var i: i32 = -R; i <= R; i = i + 1) {
      let fi = f32(i);
      let fj = f32(j);
      let w  = exp(-(fi * fi + fj * fj) / s2);
      hs = hs + w * h(uv + vec2(fi * texel.x, fj * texel.y), layer);
      wsum = wsum + w;
    }
  }
  return hs / wsum;
}

// the actual height we light: sqrt() pulls the smoothed height up (0-1 -> 0-1)
fn rh(uv: vec2<f32>, layer: u32, texel: vec2<f32>) -> f32 {
  return sqrt(clamp(smooth_h(uv, layer, texel), 0.0, 1.0));
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
  let texel = 1.0 / vec2<f32>(textureDimensions(font));

  // normal from a plain central difference OF THE SQRT HEIGHT (no chain rule)
  let c  = rh(in.uv, in.layer, texel);
  let ex = vec2(texel.x, 0.0);
  let ey = vec2(0.0, texel.y);
  let hx = rh(in.uv + ex, in.layer, texel) - rh(in.uv - ex, in.layer, texel);
  let hy = rh(in.uv + ey, in.layer, texel) - rh(in.uv - ey, in.layer, texel);
  let N  = normalize(vec3(-hx * u.height_scale, -hy * u.height_scale, 1.0));

  // discard background so overlapping glyph cells don't paint over neighbors
  if (c < 0.06) { discard; }

  // debug views
  if (u.mode == 1u) { return vec4(vec3(c), 1.0); }       // sqrt heightmap
  if (u.mode == 2u) { return vec4(N * 0.5 + 0.5, 1.0); } // normals (RGB)

  // dot-product intensity: intensity = -(N . L), output = color * intensity
  let L = normalize(u.light_dir);
  let ndl = max(-dot(N, L), 0.0);
  let lit = u.albedo * u.light_color * (u.intensity * ndl);

  let cov = smoothstep(0.06, 0.7, c);
  return vec4(lit, cov);
}
