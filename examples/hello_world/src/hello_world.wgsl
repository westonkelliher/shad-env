// One shad draws a whole line of text. The shad's rect subdivides INTO character
// cells inside the shader -- no per-glyph shads, no instancing. Two data sources:
//   tex = an ASCII spritesheet, 16 cols x 8 rows of glyphs (white ink, alpha mask)
//   buf = the string, one ASCII codepoint per u32
//   s0  = the character count N
// Per pixel: find which cell we're in, read that cell's codepoint, sample its
// glyph from the atlas. This is the "terminal grid" pattern.

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
  let n = u32(round(u.scalars.x));
  if (n == 0u) { return vec4(0.0); }      // uniform branch: safe for sampling

  let col = min(u32(floor(in.uv.x * f32(n))), n - 1u);
  let cell = vec2(fract(in.uv.x * f32(n)), in.uv.y); // 0..1 within the cell
  let code = buf[col];

  // atlas cell for this codepoint (16 x 8 grid)
  let ac = vec2(f32(code % 16u), f32(code / 16u));
  let atlas_uv = (ac + cell) / vec2(16.0, 8.0);

  let ink = vec3(0.96, 0.86, 0.42);
  let cover = textureSample(tex, samp, atlas_uv).a;
  return vec4(ink, cover);
}
