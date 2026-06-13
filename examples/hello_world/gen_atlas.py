#!/usr/bin/env python3
"""Bake printable ASCII (codepoints 0..127) into one RGBA spritesheet PNG:
a 16x8 grid of CELL-sized cells, white antialiased glyphs on a transparent
ground so a shader can read alpha as glyph coverage. Basic mono font."""

from PIL import Image, ImageDraw, ImageFont

COLS, ROWS, CELL = 16, 8, 32  # 512x256 atlas, indexed by codepoint
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
OUT = "ascii_atlas.png"

font = ImageFont.truetype(FONT, 26)
img = Image.new("RGBA", (COLS * CELL, ROWS * CELL), (0, 0, 0, 0))
draw = ImageDraw.Draw(img)

# Shared baseline: center the whole ascent+descent line box in the cell so all
# glyphs rest on one baseline (descenders drop, dots/commas sit low) instead of
# each glyph being individually centered by its own ink bbox.
ascent, descent = font.getmetrics()
baseline = (CELL - (ascent + descent)) / 2 + ascent

for code in range(COLS * ROWS):
    ch = chr(code)
    if not ch.isprintable() or ch == " ":
        continue
    cx, cy = (code % COLS) * CELL, (code // COLS) * CELL
    l, _, r, _ = draw.textbbox((0, 0), ch, font=font)
    x = cx + (CELL - (r - l)) / 2 - l
    y = cy + baseline
    draw.text((x, y), ch, font=font, fill=(255, 255, 255, 255), anchor="ls")

img.save(OUT)
print(f"wrote {OUT}  ({img.width}x{img.height}, {COLS}x{ROWS} cells of {CELL}px)")
