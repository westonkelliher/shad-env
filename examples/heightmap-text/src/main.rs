// Heightmap-lit text demo for shad-env.
//
// The big picture:
//   1. gen_glyphs.py pre-renders each unique character as a sharp grayscale
//      PNG (white glyph on black) into ./textures/. We just load those.
//   2. All glyph images are stacked into ONE texture_2d_array (one layer per
//      unique character).
//   3. The string is laid out on the CPU (with word wrap) into per-glyph
//      quads, uploaded once as an INSTANCE buffer.
//   4. A single instanced draw renders every glyph. The fragment shader
//      treats the glyph image as a heightmap and lights it (see text.wgsl).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use wgpu::util::DeviceExt;
use winit::{
    dpi::PhysicalSize,
    event::{Event, KeyEvent, WindowEvent},
    event_loop::EventLoop,
    keyboard::{Key, NamedKey},
    window::{Window, WindowBuilder},
};

// These must match gen_glyphs.py.
const CELL: u32 = 256; // per-glyph texture size in pixels
const EM: f32 = 170.0; // font size (used for line spacing)
const PEN: [f32; 2] = [40.0, 195.0]; // pen origin (x, baseline y) inside a cell

/// One quad on screen = one character. This is the per-instance vertex data.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Glyph {
    pos: [f32; 2],  // top-left, in pixels
    size: [f32; 2], // in pixels
    layer: u32,     // which texture-array layer holds this character
}

/// Lighting parameters, uploaded every frame. Layout matches `U` in text.wgsl
/// (WGSL pads vec3s to 16 bytes, so scalars are interleaved to fill the gaps).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    resolution: [f32; 2],
    height_scale: f32,
    shininess: f32,
    light_dir: [f32; 3],
    intensity: f32,
    light_color: [f32; 3],
    spec_strength: f32,
    albedo: [f32; 3],
    dog_sigma: f32, // gaussian width (texels) for the surface-angle estimate
    mode: u32,      // 0 = lit, 1 = show heightmap, 2 = show normals
    _pad: [u32; 3],
}

struct GlyphMeta {
    layer: u32,
    advance: f32, // pen advance in pixels (at EM size)
}

/// Load the pre-rendered glyph PNGs + metrics from ./textures/ into one
/// layer-major byte buffer, ready to upload as a texture_2d_array.
fn load_glyphs() -> (Vec<u8>, HashMap<char, GlyphMeta>) {
    let dir = std::path::Path::new("textures");
    let metrics = std::fs::read_to_string(dir.join("metrics.txt"))
        .expect("textures/metrics.txt missing -- run `python3 gen_glyphs.py` first");

    let mut pixels = Vec::new();
    let mut map = HashMap::new();
    for (layer, line) in metrics.lines().enumerate() {
        // each line: "<codepoint> <advance> <filename>"
        let mut it = line.split_whitespace();
        let ch = char::from_u32(it.next().unwrap().parse().unwrap()).unwrap();
        let advance: f32 = it.next().unwrap().parse().unwrap();
        let file = it.next().unwrap();

        let img = image::open(dir.join(file)).unwrap().to_luma8();
        assert_eq!(img.dimensions(), (CELL, CELL), "texture {file} wrong size");
        pixels.extend_from_slice(img.as_raw());
        map.insert(ch, GlyphMeta { layer: layer as u32, advance });
    }
    (pixels, map)
}

/// CPU text layout with word wrap: walk a pen across the screen, emitting one
/// quad per character. Each quad is a full CELLxCELL cell positioned so the
/// glyph's pen origin lands on the pen.
fn layout(text: &str, map: &HashMap<char, GlyphMeta>, scale: f32) -> Vec<Glyph> {
    let origin = [50.0, 150.0];
    let max_w = 1080.0;
    let line_h = EM * scale * 1.35;
    let cell = CELL as f32 * scale;

    let mut glyphs = Vec::new();
    let mut pen = origin;
    for word in text.split(' ') {
        let word_w: f32 = word.chars().map(|c| map[&c].advance * scale).sum();
        if pen[0] > origin[0] && pen[0] + word_w > origin[0] + max_w {
            pen = [origin[0], pen[1] + line_h]; // wrap
        }
        for c in word.chars() {
            let g = &map[&c];
            glyphs.push(Glyph {
                pos: [pen[0] - PEN[0] * scale, pen[1] - PEN[1] * scale],
                size: [cell, cell],
                layer: g.layer,
            });
            pen[0] += g.advance * scale;
        }
        pen[0] += map[&' '].advance * scale;
    }
    glyphs
}

/// Standard wgpu boilerplate: surface, adapter, device, surface config.
async fn init_wgpu(
    window: Arc<Window>,
) -> (
    wgpu::Surface<'static>,
    wgpu::Device,
    wgpu::Queue,
    wgpu::SurfaceConfiguration,
) {
    let instance = wgpu::Instance::default();
    let surface = instance.create_surface(window.clone()).unwrap();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        })
        .await
        .unwrap();
    let (device, queue) = adapter
        .request_device(&Default::default(), None)
        .await
        .unwrap();

    let size = window.inner_size();
    let caps = surface.get_capabilities(&adapter);
    // prefer a non-sRGB format so the shader's output colors are used as-is
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .unwrap_or(caps.formats[0]);
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: size.width,
        height: size.height,
        present_mode: caps.present_modes[0],
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &config);
    (surface, device, queue, config)
}

/// Upload all glyph images as one texture_2d_array (R8 = single gray channel).
fn create_glyph_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pixels: &[u8],
    layers: u32,
) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: CELL,
        height: CELL,
        depth_or_array_layers: layers,
    };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("glyph heightmaps"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        tex.as_image_copy(),
        pixels,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(CELL), // 1 byte per pixel
            rows_per_image: Some(CELL),
        },
        size,
    );
    tex.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

/// Build the render pipeline + bind group: uniforms, glyph texture, sampler.
fn create_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    uniform_buf: &wgpu::Buffer,
    glyph_view: &wgpu::TextureView,
) -> (wgpu::RenderPipeline, wgpu::BindGroup) {
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
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
                    view_dimension: wgpu::TextureViewDimension::D2Array,
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
        ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(glyph_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    let shader = device.create_shader_module(wgpu::include_wgsl!("text.wgsl"));
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&bgl],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs",
            // no vertex buffer -- corners come from vertex_index; this buffer
            // steps once per INSTANCE and carries one Glyph
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Glyph>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Uint32],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs",
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip, // 4 verts = quad
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        multiview: None,
    });
    (pipeline, bind_group)
}

fn main() {
    pollster::block_on(run());
}

async fn run() {
    // debug views: --height shows the heightmap, --angle shows the normals
    let mode = match std::env::args().nth(1).as_deref() {
        Some("--height") => 1u32,
        Some("--angle") => 2,
        _ => 0,
    };

    let (pixels, glyph_map) = load_glyphs();
    let glyphs = layout("Hey Now Brown Cow!", &glyph_map, 0.62);
    println!("{} glyph textures, {} glyphs on screen", glyph_map.len(), glyphs.len());

    let event_loop = EventLoop::new().unwrap();
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("shad-env: heightmap-lit text")
            .with_inner_size(PhysicalSize::new(1200u32, 360u32))
            .build(&event_loop)
            .unwrap(),
    );
    let (surface, device, queue, mut config) = init_wgpu(window.clone()).await;

    let glyph_view = create_glyph_texture(&device, &queue, &pixels, glyph_map.len() as u32);
    let instance_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("glyph instances"),
        contents: bytemuck::cast_slice(&glyphs),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("uniforms"),
        size: std::mem::size_of::<Uniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let (pipeline, bind_group) = create_pipeline(&device, config.format, &uniform_buf, &glyph_view);

    let n_glyphs = glyphs.len() as u32;
    let start = Instant::now();

    event_loop
        .run(move |event, elwt| {
            elwt.set_control_flow(winit::event_loop::ControlFlow::Poll);
            let Event::WindowEvent { event, .. } = event else { return };
            match event {
                WindowEvent::CloseRequested
                | WindowEvent::KeyboardInput {
                    event: KeyEvent { logical_key: Key::Named(NamedKey::Escape), .. },
                    ..
                } => elwt.exit(),
                WindowEvent::Resized(s) => {
                    config.width = s.width.max(1);
                    config.height = s.height.max(1);
                    surface.configure(&device, &config);
                }
                WindowEvent::RedrawRequested => {
                    // animate the light direction so highlights sweep across
                    let t = start.elapsed().as_secs_f32();
                    let uniforms = Uniforms {
                        resolution: [config.width as f32, config.height as f32],
                        height_scale: 9.0,
                        shininess: 28.0,
                        light_dir: [t.cos() * 0.7, t.sin() * 0.7, -0.6],
                        intensity: 1.1,
                        light_color: [1.0, 0.96, 0.88],
                        spec_strength: 0.7,
                        albedo: [0.85, 0.45, 0.2],
                        dog_sigma: 2.0,
                        mode,
                        _pad: [0; 3],
                    };
                    queue.write_buffer(&uniform_buf, 0, bytemuck::bytes_of(&uniforms));

                    let Ok(frame) = surface.get_current_texture() else {
                        surface.configure(&device, &config); // lost surface: recreate
                        return;
                    };
                    let view = frame.texture.create_view(&Default::default());
                    let mut encoder = device.create_command_encoder(&Default::default());
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: None,
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
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
                        pass.set_pipeline(&pipeline);
                        pass.set_bind_group(0, &bind_group, &[]);
                        pass.set_vertex_buffer(0, instance_buf.slice(..));
                        pass.draw(0..4, 0..n_glyphs); // 4 strip verts x N glyphs, ONE draw call
                    }
                    queue.submit([encoder.finish()]);
                    frame.present();
                    window.request_redraw();
                }
                _ => {}
            }
        })
        .unwrap();
}
