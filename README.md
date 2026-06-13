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
| `register_shader(handle, path, hash)` | command | hash-validate, compile, store |
| `add_shad(handle, shader, corners, z)` | command | bind a shader to a rect |
| `move_shad(handle, corners, z)` | command | move/relayer a shad |
| `set_uniform_value(handle, name, val)` | command | write a named user slot |
| `render()` | command | draw all shads (z, then order) + present |
| `get_shad_data(handle)` | query | snapshot rect/z/uniforms |

**Uniforms** (`src/shared.wgsl`): builtins `rect`/`resolution`/`mouse`/`time`
plus generic user slots `s0..s3` (scalars) and `v0..v3` (vec4s), addressed by
name in `set_uniform_value`. `register_shader`'s `hash` is the FNV-1a-64 hex of
the file (`content_hash`); mismatch errors out.

See `specs/shad_env_api.rs` for the prototypes + design rules.

## Status

Early. Built on [`wgpu`](https://wgpu.rs) (Rust → native + WebGPU).
See `responses/` for design notes and the current POC.
