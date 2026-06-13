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

  // Kern via pen advance: each glyph is still drawn at full cell width (1/n,
  // undistorted), but the pen advances by only ADV of that per char. ADV<1
  // overlaps neighbors -- their transparent side padding overlaps harmlessly,
  // tightening the spacing. So a pixel can sit under two glyphs; sample both
  // candidate columns and keep the max coverage.
  let ADV = 0.66;
  let t = in.uv.x * f32(n);          // position in cell-widths along the line
  let g_hi = i32(floor(t / ADV));    // rightmost glyph that could cover this px

  var cover = 0.0;
  for (var g = g_hi - 1; g <= g_hi; g++) {
    let lx = t - f32(g) * ADV;       // x within glyph g's cell, 0..1 if covered
    if (g < 0 || u32(g) >= n || lx < 0.0 || lx > 1.0) { continue; }
    let code = buf[g];
    let ac = vec2(f32(code % 16u), f32(code / 16u));
    let atlas_uv = (ac + vec2(lx, in.uv.y)) / vec2(16.0, 8.0);
    cover = max(cover, textureSample(tex, samp, atlas_uv).a);
  }

  let ink = vec3(0.96, 0.86, 0.42);
  return vec4(ink, cover);
}
