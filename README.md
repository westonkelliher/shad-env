# shad-env

An interface for building graphical applications out of **shader-driven rectangles**.

You define rectangles within a window and bind a fragment shader to each one. Apps
are composed by placing, layering, and feeding inputs to these shader-rects.

## Concepts

- **shader** — a registered fragment shader, referenced by handle.
- **shad** — an instance of a shader bound to a rectangular subregion of the window.
- **inputs** — uniforms passed to a shad (time, resolution, mouse, user values).

Drawing is ordered by `z` (low first), then insertion order.

## Library (`src/lib.rs`)

`ShadEnv` is the whole wgpu side — instance/adapter/device/queue and, after
`configure`, the surface. The caller's winit module owns the window + event loop
and drives it. Conventions: command/query separation (mutators return only
`Result<(), _>`, queries return values), explicit caller-chosen handles (no
internal ids), and one universal `Bgra8Unorm` surface format.

| method | kind | purpose |
|---|---|---|
| `new()` | query | build device-level wgpu state (no surface) |
| `configure(window)` | command | create + configure the surface |
| `resize(w, h)` | command | reconfigure on winit `Resized` |
| `register_shader(handle, path, hash, validate)` | command | optionally hash-validate, compile, store |
| `register_texture(handle, rgba, w, h)` | command | store a 2D data source (raw RGBA8) |
| `register_buffer(handle, bytes)` | command | store an array data source (raw bytes) |
| `add_shad(handle, shader, corners, z)` | command | bind a shader to a rect |
| `move_shad(handle, corners, z)` | command | move/relayer a shad |
| `set_uniform_value(handle, name, val)` | command | write a named user slot |
| `set_texture(shad, tex)` | command | bind a texture to a shad (`tex`/`samp`) |
| `set_buffer(shad, buf)` | command | bind a buffer to a shad (`buf`) |
| `render()` | command | draw all shads (z, then order) + present |
| `get_shad_data(handle)` | query | snapshot rect/z/uniforms |

**Shader inputs** (`src/shared.wgsl`): builtins `rect`/`resolution`/`mouse`/`time`;
generic user slots `s0..s3` (scalars) and `v0..v3` (vec4s) via `set_uniform_value`;
plus two unopinionated data sources — `tex` (a `texture_2d`, with `samp`) and `buf`
(`array<u32>`) — bound per-shad via `set_texture`/`set_buffer`. What they *mean* is
the shader's business (font atlas, LUT, string, curves...). Shads that bind neither
get a 1×1 white texture and a 1-element buffer. `register_shader`'s `hash` is the
FNV-1a-64 hex of the file (`content_hash`); when `validate` is true, a mismatch errors out.

See `specs/shad_env_api.rs` for the prototypes + design rules.

## Examples

- **`examples/pong/`** — a full game: a ball shad moved each frame, full-height
  paddle strips fed their position via uniforms, and 7-segment score digits.
- **`examples/text/`** — one shad renders a whole line of text by reading a baked
  ASCII atlas (`tex`) and the string's codepoints (`buf`), carving its rect into
  character cells in-shader. Shows off `register_texture`/`register_buffer`.

Run with `cargo run` from the example's directory.

## Status

Early. Built on [`wgpu`](https://wgpu.rs) (Rust → native + WebGPU).
