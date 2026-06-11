# shad-env

An interface for building graphical applications out of **shader-driven rectangles**.

You define rectangles within a window and bind a fragment shader to each one. Apps
are composed by placing, layering, and feeding inputs to these shader-rects.

## Concepts

- **shader** — a registered fragment shader, referenced by handle.
- **shad** — an instance of a shader bound to a rectangular subregion of the window.
- **inputs** — uniforms passed to a shad (time, resolution, mouse, user values).

Drawing is ordered by `z` (low first), then insertion order.

## Status

Early. Built on [`wgpu`](https://wgpu.rs) (Rust → native + WebGPU).
See `responses/` for design notes and the current POC.
