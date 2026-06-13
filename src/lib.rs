// shad-env: the wgpu side of a shader-driven-rectangle UI. Owns the GPU state
// (instance/adapter/device/queue), the compiled shads, and nothing about where
// pixels go. The sole renderer is `render_to(&view)`: the caller owns the
// render target -- a winit surface frame, an offscreen texture, someone else's
// pass -- and hands shad-env a view to draw into. Surface creation, the
// swapchain loop, present, and readback all live in the caller (typically a UI
// or game-engine layer built on top of this), using the exposed
// `instance()`/`adapter()`/`device()`/`queue()` handles. See
// specs/shad_env_api.rs for the design rules (command/query separation,
// explicit handles, single render-target format).

use std::collections::HashMap;
use std::time::Instant;

use wgpu::util::DeviceExt;

// Re-exported so callers build their surface/textures against the exact wgpu
// version shad-env links (mismatched wgpu versions don't interoperate).
pub use wgpu;

const SHARED: &str = include_str!("shared.wgsl");

/// One universal swapchain format. `Bgra8Unorm` is presentable on every native
/// backend and is WebGPU's guaranteed canvas format, and it's non-sRGB so
/// shader output colors are used as-is. Pipelines are built against this
/// constant, so they don't need the surface to exist yet. Public so callers
/// who hand their own target to `render_to` can allocate a matching texture.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// The standard bind-group-0 uniform every shad shares. Builtins are written by
/// `render_to` each frame; the generic `scalars`/`vecs` slots are what
/// `set_uniform_value` targets by name. Layout must match `U` in shared.wgsl.
#[repr(C)]
#[derive(Copy, Clone, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    rect: [f32; 4],       // builtin: x, y, w, h in window px
    resolution: [f32; 2], // builtin: shad rect size in px
    mouse: [f32; 2],      // builtin: cursor in shad-local px (reserved)
    time: f32,            // builtin: seconds since new()
    _pad: [f32; 3],       // -> 16-byte alignment before `scalars`
    scalars: [f32; 4],    // user slots "s0".."s3"
    vecs: [[f32; 4]; 4],  // user slots "v0".."v3"
}

/// A value the caller pushes into a named user slot.
pub enum UniformValue {
    Scalar(f32),    // -> "s0".."s3"
    Vec4([f32; 4]), // -> "v0".."v3"
}

#[derive(Debug)]
pub enum ShadError {
    UnknownShader(String),
    UnknownShad(String),
    UnknownUniform(String),
    UnknownTexture(String),
    UnknownBuffer(String),
    HashMismatch { expected: String, got: String },
    Io(std::io::Error),
}

/// A compiled shader (shared prelude + the registered fragment source),
/// reusable across many shads.
struct Shader {
    module: wgpu::ShaderModule,
}

/// A registered 2D data source (`register_texture`). What its pixels MEAN is up
/// to the shader sampling it.
struct Texture {
    #[allow(dead_code)]
    texture: wgpu::Texture, // kept alive; `view` is what the bind group references
    view: wgpu::TextureView,
}

/// One placed shader-rect: its pipeline, uniforms, and draw ordering.
struct Shad {
    rect: [f32; 4], // x, y, w, h
    z: f32,
    order: u64, // insertion index; tiebreak after z
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    uniforms: Uniforms,        // CPU-side copy, uploaded each render
    tex_handle: Option<String>, // bound via set_texture; None -> default white
    buf_handle: Option<String>, // bound via set_buffer; None -> default 1-elem
}

pub struct ShadEnv {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    bgl: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,

    // defaults bound when a shad sets no texture/buffer, so the shared bind
    // group layout is always satisfiable (keeps pong & co. unchanged).
    default_tex: wgpu::TextureView,
    default_sampler: wgpu::Sampler,
    default_buf: wgpu::Buffer,

    shaders: HashMap<String, Shader>,
    textures: HashMap<String, Texture>,
    buffers: HashMap<String, wgpu::Buffer>,
    shads: HashMap<String, Shad>,
    next_order: u64,
    start: Instant,
}

impl ShadEnv {
    /// The render-target format every pipeline is baked against; allocate
    /// caller-owned targets (surface config, offscreen textures) with it.
    /// Mirror of the crate-level `FORMAT`.
    pub const FORMAT: wgpu::TextureFormat = FORMAT;

    /// QUERY: build the device-level wgpu state and return it. No surface, no
    /// configuration -- nothing external is mutated.
    pub async fn new() -> ShadEnv {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: None, // device init is independent of any surface
                force_fallback_adapter: false,
            })
            .await
            .expect("no suitable GPU adapter");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await
            .expect("failed to create device");

        // One layout shared by every shad: uniform buffer (0), plus a generic
        // texture (1) + sampler (2) + storage buffer (3) that shaders may ignore.
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shad bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shad pipeline layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        // Default bindings for shads that set no texture/buffer: a 1x1 white
        // pixel, a nearest sampler, and a 1-element zero buffer.
        let default_tex_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("default white"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &default_tex_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255u8, 255, 255, 255],
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let default_tex = default_tex_tex.create_view(&Default::default());
        let default_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("default sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let default_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("default storage"),
            contents: bytemuck::cast_slice(&[0u32]),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        ShadEnv {
            instance,
            adapter,
            device,
            queue,
            bgl,
            pipeline_layout,
            default_tex,
            default_sampler,
            default_buf,
            shaders: HashMap::new(),
            textures: HashMap::new(),
            buffers: HashMap::new(),
            shads: HashMap::new(),
            next_order: 0,
            start: Instant::now(),
        }
    }

    /// QUERY: the wgpu handles shad-env owns, lent to the caller so it can build
    /// its own render target. `instance`/`adapter` to create + configure a
    /// surface (use `ShadEnv::FORMAT` as its format); `device`/`queue` to
    /// allocate offscreen textures or read pixels back. shad-env never touches a
    /// surface itself.
    pub fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }
    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// COMMAND: read `path`, optionally validate its content hash against `hash`
    /// (err on mismatch when `validate` is true), compile (prelude + fragment
    /// src), store under `shader_handle`. `hash` is the FNV-1a-64 hex digest of
    /// the file bytes (see `content_hash`).
    pub fn register_shader(
        &mut self,
        shader_handle: &str,
        path: &str,
        hash: &str,
        validate: bool,
    ) -> Result<(), ShadError> {
        let src = std::fs::read_to_string(path).map_err(ShadError::Io)?;
        if validate {
            let got = content_hash(src.as_bytes());
            if !got.eq_ignore_ascii_case(hash) {
                return Err(ShadError::HashMismatch {
                    expected: hash.to_string(),
                    got,
                });
            }
        }
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(shader_handle),
                source: wgpu::ShaderSource::Wgsl(format!("{SHARED}\n{src}").into()),
            });
        self.shaders
            .insert(shader_handle.to_string(), Shader { module });
        Ok(())
    }

    /// COMMAND: bind `shader_handle` to the corner rect, building its pipeline,
    /// uniform buffer, and bind group. z defaults to 0.
    pub fn add_shad(
        &mut self,
        shad_handle: &str,
        shader_handle: &str,
        corners: [f32; 4],
        z: Option<f32>,
    ) -> Result<(), ShadError> {
        let shader = self
            .shaders
            .get(shader_handle)
            .ok_or_else(|| ShadError::UnknownShader(shader_handle.to_string()))?;
        let rect = corners_to_rect(corners);
        let uniforms = Uniforms {
            rect,
            resolution: [rect[2], rect[3]],
            ..Default::default()
        };

        let uniform_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(shad_handle),
                contents: bytemuck::bytes_of(&uniforms),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        // starts on the defaults; set_texture/set_buffer rebuild it later
        let bind_group = make_bind_group(
            &self.device,
            &self.bgl,
            &uniform_buf,
            &self.default_tex,
            &self.default_sampler,
            &self.default_buf,
            shad_handle,
        );
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(shad_handle),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader.module,
                    entry_point: "vs",
                    buffers: &[], // vs generates the triangle itself
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader.module,
                    entry_point: "fs",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
            });

        self.shads.insert(
            shad_handle.to_string(),
            Shad {
                rect,
                z: z.unwrap_or(0.0),
                order: self.next_order,
                pipeline,
                bind_group,
                uniform_buf,
                uniforms,
                tex_handle: None,
                buf_handle: None,
            },
        );
        self.next_order += 1;
        Ok(())
    }

    /// COMMAND: register a 2D data source under `handle` from raw RGBA8 bytes
    /// (`width*height*4` long, row-major). Raw bytes by design -- the lib never
    /// assumes an image codec, and the same path serves non-image data (LUTs,
    /// packed glyph curves). Bind it to a shad with `set_texture`.
    pub fn register_texture(
        &mut self,
        handle: &str,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), ShadError> {
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(handle),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            size,
        );
        let view = texture.create_view(&Default::default());
        self.textures.insert(handle.to_string(), Texture { texture, view });
        Ok(())
    }

    /// COMMAND: register an array data source under `handle` from raw bytes
    /// (read in-shader as `buf: array<u32>`). Bind it with `set_buffer`. Meaning
    /// is the shader's: a string of codepoints, curve data, a tilemap, etc.
    pub fn register_buffer(&mut self, handle: &str, data: &[u8]) -> Result<(), ShadError> {
        let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(handle),
            contents: data,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });
        self.buffers.insert(handle.to_string(), buffer);
        Ok(())
    }

    /// COMMAND: bind registered texture `tex_handle` to `shad_handle` (the shad's
    /// `tex`/`samp`). Rebuilds the shad's bind group.
    pub fn set_texture(&mut self, shad_handle: &str, tex_handle: &str) -> Result<(), ShadError> {
        if !self.textures.contains_key(tex_handle) {
            return Err(ShadError::UnknownTexture(tex_handle.to_string()));
        }
        self.shads
            .get_mut(shad_handle)
            .ok_or_else(|| ShadError::UnknownShad(shad_handle.to_string()))?
            .tex_handle = Some(tex_handle.to_string());
        self.rebuild_bind_group(shad_handle);
        Ok(())
    }

    /// COMMAND: bind registered buffer `buf_handle` to `shad_handle` (the shad's
    /// `buf`). Rebuilds the shad's bind group.
    pub fn set_buffer(&mut self, shad_handle: &str, buf_handle: &str) -> Result<(), ShadError> {
        if !self.buffers.contains_key(buf_handle) {
            return Err(ShadError::UnknownBuffer(buf_handle.to_string()));
        }
        self.shads
            .get_mut(shad_handle)
            .ok_or_else(|| ShadError::UnknownShad(shad_handle.to_string()))?
            .buf_handle = Some(buf_handle.to_string());
        self.rebuild_bind_group(shad_handle);
        Ok(())
    }

    /// Rebuild a shad's bind group from its current texture/buffer handles,
    /// falling back to the defaults. Cheap; called only on set_texture/set_buffer.
    fn rebuild_bind_group(&mut self, shad_handle: &str) {
        let Some(shad) = self.shads.get(shad_handle) else { return };
        let tex_view = shad
            .tex_handle
            .as_ref()
            .and_then(|h| self.textures.get(h))
            .map(|t| &t.view)
            .unwrap_or(&self.default_tex);
        let buf = shad
            .buf_handle
            .as_ref()
            .and_then(|h| self.buffers.get(h))
            .unwrap_or(&self.default_buf);
        let bind_group = make_bind_group(
            &self.device,
            &self.bgl,
            &shad.uniform_buf,
            tex_view,
            &self.default_sampler,
            buf,
            shad_handle,
        );
        self.shads.get_mut(shad_handle).unwrap().bind_group = bind_group;
    }

    /// COMMAND: move/relayer an existing shad. Keeps the current z if `z` is None.
    pub fn move_shad(
        &mut self,
        shad_handle: &str,
        corners: [f32; 4],
        z: Option<f32>,
    ) -> Result<(), ShadError> {
        let shad = self
            .shads
            .get_mut(shad_handle)
            .ok_or_else(|| ShadError::UnknownShad(shad_handle.to_string()))?;
        shad.rect = corners_to_rect(corners);
        if let Some(z) = z {
            shad.z = z;
        }
        Ok(())
    }

    /// COMMAND: write `value` into the named user slot ("s0".."s3"/"v0".."v3").
    /// Mutates the CPU-side copy; `render` uploads it to the GPU.
    pub fn set_uniform_value(
        &mut self,
        shad_handle: &str,
        var_name: &str,
        value: UniformValue,
    ) -> Result<(), ShadError> {
        let shad = self
            .shads
            .get_mut(shad_handle)
            .ok_or_else(|| ShadError::UnknownShad(shad_handle.to_string()))?;
        match (parse_slot(var_name), value) {
            (Some(('s', i)), UniformValue::Scalar(x)) if i < 4 => shad.uniforms.scalars[i] = x,
            (Some(('v', i)), UniformValue::Vec4(v)) if i < 4 => shad.uniforms.vecs[i] = v,
            _ => return Err(ShadError::UnknownUniform(var_name.to_string())),
        }
        Ok(())
    }

    /// COMMAND: the one renderer. Write per-frame builtins into every shad, draw
    /// them (z ascending, then insertion order) into the caller-owned `view`,
    /// sized `width` x `height` (px), and submit. Does NOT present or read back
    /// -- shad-env doesn't own the target; that epilogue is the caller's (present
    /// a winit surface frame, read back an offscreen texture, sample it into
    /// another pass, ...). The view's texture MUST be `ShadEnv::FORMAT`, since
    /// pipelines are baked against it.
    pub fn render_to(&mut self, view: &wgpu::TextureView, width: u32, height: u32) {
        let time = self.start.elapsed().as_secs_f32();
        let encoder = self.encode_scene(view, width as f32, height as f32, time);
        self.queue.submit([encoder.finish()]);
    }

    /// Refresh builtins, upload uniforms, and encode the painter-ordered draw of
    /// every shad into `view` (a `cw` x `ch` target, px). The body of `render_to`.
    fn encode_scene(
        &mut self,
        view: &wgpu::TextureView,
        cw: f32,
        ch: f32,
        time: f32,
    ) -> wgpu::CommandEncoder {
        // refresh builtins and upload each shad's whole uniform block
        for shad in self.shads.values_mut() {
            shad.uniforms.rect = shad.rect;
            shad.uniforms.resolution = [shad.rect[2], shad.rect[3]];
            shad.uniforms.time = time;
            self.queue
                .write_buffer(&shad.uniform_buf, 0, bytemuck::bytes_of(&shad.uniforms));
        }

        let mut encoder = self.device.create_command_encoder(&Default::default());

        // painter's order: z ascending, then insertion order
        let mut order: Vec<&Shad> = self.shads.values().collect();
        order.sort_by(|a, b| {
            a.z.partial_cmp(&b.z)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.order.cmp(&b.order))
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.06,
                            g: 0.07,
                            b: 0.09,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            for shad in &order {
                let [x, y, w, h] = shad.rect;
                // clamp to the framebuffer so the viewport stays valid: a
                // partially off-screen shad is squished at the edge, a fully
                // off-screen one is skipped. (A viewport whose origin is
                // negative or that exceeds the target is a wgpu error.)
                let x0 = x.max(0.0);
                let y0 = y.max(0.0);
                let vw = (x + w).min(cw) - x0;
                let vh = (y + h).min(ch) - y0;
                if vw <= 0.0 || vh <= 0.0 {
                    continue;
                }
                pass.set_viewport(x0, y0, vw, vh, 0.0, 1.0);
                pass.set_pipeline(&shad.pipeline);
                pass.set_bind_group(0, &shad.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }

        encoder
    }
}

/// Read a `COPY_SRC` texture back to tightly-packed RGBA8 (row-major). A
/// stateless GPU utility, not a renderer: the caller owns the `texture` (e.g. an
/// offscreen target it allocated, rendered into with `render_to`, and now wants
/// on the CPU for a screenshot or test). Blocks until the GPU is done. Assumes
/// the texture is `ShadEnv::FORMAT` (Bgra8) and swizzles to RGBA on the way out.
pub fn read_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Vec<u8> {
    // copy texture -> a mappable buffer; bytes_per_row must be 256-aligned
    let bpp = 4u32;
    let unpadded = width * bpp;
    let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &readback,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);

    // block until the GPU finishes and the buffer is mapped
    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| { let _ = tx.send(r); });
    device.poll(wgpu::Maintain::Wait);
    rx.recv().ok().and_then(|r| r.ok()).expect("readback map failed");

    // drop row padding and swizzle Bgra8 -> Rgba8
    let data = slice.get_mapped_range();
    let mut out = vec![0u8; (width * height * bpp) as usize];
    for row in 0..height as usize {
        let src = row * padded as usize;
        let dst = row * unpadded as usize;
        for px in 0..width as usize {
            let (s, d) = (src + px * 4, dst + px * 4);
            out[d] = data[s + 2]; // R <- B
            out[d + 1] = data[s + 1];
            out[d + 2] = data[s]; // B <- R
            out[d + 3] = data[s + 3];
        }
    }
    drop(data);
    readback.unmap();
    out
}

/// Build a shad's bind group: uniform buffer (0), texture (1), sampler (2),
/// storage buffer (3). One creation path for `add_shad` and `rebuild_bind_group`.
fn make_bind_group(
    device: &wgpu::Device,
    bgl: &wgpu::BindGroupLayout,
    uniform_buf: &wgpu::Buffer,
    tex_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    buf: &wgpu::Buffer,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout: bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniform_buf.as_entire_binding() },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(tex_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry { binding: 3, resource: buf.as_entire_binding() },
        ],
    })
}

/// Corners (x1,y1,x2,y2) -> rect (x,y,w,h), order-independent.
fn corners_to_rect([x1, y1, x2, y2]: [f32; 4]) -> [f32; 4] {
    [x1.min(x2), y1.min(y2), (x2 - x1).abs(), (y2 - y1).abs()]
}

/// Split a slot name like "s2"/"v0" into (kind, index).
fn parse_slot(name: &str) -> Option<(char, usize)> {
    let mut chars = name.chars();
    let kind = chars.next()?;
    let index: usize = chars.as_str().parse().ok()?;
    Some((kind, index))
}

/// FNV-1a 64-bit hex digest of `bytes` -- the content hash `register_shader`
/// validates against. Dependency-free and deterministic; generate the expected
/// value with this same function.
pub fn content_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}
