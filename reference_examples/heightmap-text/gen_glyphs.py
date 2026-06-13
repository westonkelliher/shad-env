#!/usr/bin/env python3
"""Pre-render glyph source textures for the heightmap-text demo.

Each unique character is rendered as a normal, sharp, antialiased font glyph
(white on black, grayscale) into a CELL x CELL PNG. The natural semi-white
edge pixels ARE the height ramp -- no blurring. A metrics.txt records the pen
advance per glyph so the Rust side can lay text out without a font library.
"""
import os
from PIL import Image, ImageDraw, ImageFont

CELL = 256          # texture size, must match the Rust CELL const
EM = 170            # font size in px (pen origin / baseline convention shared with Rust)
OX, OY = 40, 195    # pen origin inside the cell: (left, baseline)
TEXT = "Hey Now Brown Cow!"
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "textures")
os.makedirs(out, exist_ok=True)
font = ImageFont.truetype(FONT, EM)

chars = sorted(set(TEXT))
lines = []
for i, ch in enumerate(chars):
    img = Image.new("L", (CELL, CELL), 0)                 # black background
    ImageDraw.Draw(img).text((OX, OY), ch, fill=255,      # sharp white glyph
                             font=font, anchor="ls")       # left edge, baseline
    fname = f"{i:02d}_U{ord(ch):04X}.png"
    img.save(os.path.join(out, fname))
    lines.append(f"{ord(ch)} {font.getlength(ch):.3f} {fname}")

open(os.path.join(out, "metrics.txt"), "w").write("\n".join(lines) + "\n")
print(f"wrote {len(chars)} glyph textures + metrics.txt to {out}")
