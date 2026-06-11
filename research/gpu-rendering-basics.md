# GPU Rendering Basics — notes for shad-env

## Shaders & coordinates
- A **fragment shader executes once per pixel**, but the coords you give it are your choice:
  - `@builtin(position)` → framebuffer pixel coords (resolution-dependent).
  - **UV (0..1 across the rect)** → resolution-independent "fraction of area." Usually what you want.
- Design rule: feed each shad a UV (0→1) *plus* its pixel size as a uniform, so the shader can pick.

## Triangles → rectangles
- The GPU's only filled primitive is the **triangle**. A rectangle = **two triangles** (a "quad").
- (Fullscreen-triangle trick: one oversized triangle covering the viewport, slightly cheaper than two.)

## The pipeline: rasterizer then shader
1. **Rasterizer** (fixed hardware) runs FIRST. Takes the triangle's corner positions, computes
   **which pixels are covered** (coverage only — no color). Outputs a list of "fragments."
2. **Fragment shader** (your code) runs once per covered pixel, returns that pixel's **color**.
- The rasterizer **passes each pixel's data into the shader**: its `@builtin(position)`, plus any
  **interpolated** per-vertex values (e.g. UV set to 0..1 at corners → smoothly interpolated so each
  pixel gets its own in-between value).
- So: corners → rasterizer decides coverage → shader colors each covered pixel.
- Rounded corners = rasterizer fills the full rect, then the shader makes out-of-SDF pixels
  transparent per-pixel.

## Instancing — "many rects cheaply"
- Each `draw()` call has fixed CPU overhead (~µs). Thousands of per-rect calls = CPU-bound, GPU idle.
- Fix: store all rects as **rows in a buffer**, issue **one** `draw(0..6, 0..N)` — N instances.
  Vertex shader reads its row via `@builtin(instance_index)`.
- Only rects sharing a shader (pipeline) can be in one instanced draw → **batch shads by shader**.
- For shad-env: a shad's rect = a buffer row (not a `set_viewport` call); move = rewrite a row.

## How browsers render (the model we're rebuilding)
- Pipeline: DOM+CSS → layout → paint (**display list** of high-level ops) → raster → composite.
- Modern browsers raster + composite on the **GPU with shaders** (Chrome=Skia, Firefox=WebRender).
  The old "CPU rasterize + blit bitmap" path is pre-~2012.
- They target low-level GPU APIs (D3D/Metal/Vulkan) via their own portability layer (ANGLE/Dawn),
  NOT high-level OS 2D APIs.
- One OS dependency: **font rasterization** (DirectWrite/CoreText/FreeType) → glyphs become an
  **atlas texture** sampled by shaders.
- **WebRender** ≈ production version of shad-env: whole page as batched/instanced shader-drawn rects.

## Implications for shad-env's design
- Bottom layer should assume **lots of rects** (instance data, not per-rect calls).
- Make **texture** a first-class shader input (glyph atlases, images, multi-pass), not just scalars.
- Add a **clip/scissor rect** so an upper scroll layer can mask overflow.
- Higher layers (layout engine, display list, markdown/HTML viewer) build *on top* — this is the
  lowest layer.
