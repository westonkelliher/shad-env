// shad-env API spec (prototype skeleton).
//
// Design rules encoded here:
//   * shad-env is the whole wgpu side: instance/adapter/device/queue + (after
//     `configure`) the surface. It is NOT responsible for window creation or
//     the event loop -- the caller's winit module owns those and hands in the
//     window.
//   * Command/query separation: a fn either RETURNS a value (query, no
//     mutation) or MUTATES state (command, returns only `Result<(), _>`),
//     never both. So commands never hand back a value; `add_shad` /
//     `register_shader` take explicit caller-chosen handles rather than
//     inventing ids.
//   * One universal surface format (`Bgra8Unorm`) so pipelines don't need the
//     surface at build time; the surface is needed only to present.
//
// See ../src/lib.rs for the implemented bodies and ../src/shared.wgsl for the
// matching uniform prelude.

use std::sync::Arc;
use winit::window::Window;

/// A value pushed into a named user slot via `set_uniform_value`.
/// Generic names: scalars are "s0".."s3", vec4s are "v0".."v3".
pub enum UniformValue {
    Scalar(f32),
    Vec4([f32; 4]),
}

pub enum ShadError {
    NoSurface,
    UnknownShader(String),
    UnknownShad(String),
    UnknownUniform(String),
    HashMismatch { expected: String, got: String },
    Io(std::io::Error),
    Surface(wgpu::SurfaceError),
}

pub struct ShadEnv { /* see src/lib.rs */ }

impl ShadEnv {
    // ---- construction (query: returns the value, mutates nothing external) --
    pub async fn new() -> ShadEnv { todo!() }

    // ---- commands (&mut self, return only Result<(), _>) -------------------

    /// Create + configure the surface from `window` (the sole surface creator).
    pub fn configure(&mut self, window: Arc<Window>) -> Result<(), ShadError> { todo!() }

    /// Reconfigure the surface to a new size (call on winit `Resized`).
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), ShadError> { todo!() }

    /// Read `path`, optionally validate its content hash against `hash` (err on
    /// mismatch when `validate` is true), compile, store under `shader_handle`.
    pub fn register_shader(&mut self, shader_handle: &str, path: &str, hash: &str, validate: bool)
        -> Result<(), ShadError> { todo!() }

    /// Store a 2D data source (raw RGBA8, `w*h*4` bytes). Meaning is the
    /// shader's; bind to a shad with `set_texture`.
    pub fn register_texture(&mut self, handle: &str, rgba: &[u8], width: u32, height: u32)
        -> Result<(), ShadError> { todo!() }

    /// Store an array data source (raw bytes, read in-shader as `array<u32>`).
    /// Bind to a shad with `set_buffer`.
    pub fn register_buffer(&mut self, handle: &str, data: &[u8])
        -> Result<(), ShadError> { todo!() }

    /// Bind `shader_handle` to the corner rect (x1,y1,x2,y2); z defaults to 0.
    pub fn add_shad(&mut self, shad_handle: &str, shader_handle: &str,
        corners: [f32; 4], z: Option<f32>) -> Result<(), ShadError> { todo!() }

    /// Bind a registered texture / buffer to a shad (`tex`+`samp` / `buf`).
    pub fn set_texture(&mut self, shad_handle: &str, tex_handle: &str)
        -> Result<(), ShadError> { todo!() }
    pub fn set_buffer(&mut self, shad_handle: &str, buf_handle: &str)
        -> Result<(), ShadError> { todo!() }

    /// Move/relayer an existing shad (keeps current z if `z` is None).
    pub fn move_shad(&mut self, shad_handle: &str, corners: [f32; 4], z: Option<f32>)
        -> Result<(), ShadError> { todo!() }

    /// Write `value` into the named user slot ("s0".."s3" / "v0".."v3").
    pub fn set_uniform_value(&mut self, shad_handle: &str, var_name: &str,
        value: UniformValue) -> Result<(), ShadError> { todo!() }

    /// Draw every shad (z asc, then insertion order) and present.
    pub fn render(&mut self) -> Result<(), ShadError> { todo!() }
}
