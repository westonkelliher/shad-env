// Heightmap-lit text demo for shad-env.
//
//   - glyph SOURCE textures are pre-rendered by gen_glyphs.py (sharp, antialiased
//     white-on-black PNGs) into ./textures/ ; this program just LOADS them.
//   - all glyphs are stacked into one texture_2d_array.
//   - the string is laid out on the CPU (with word-wrap) into per-glyph quads,
//     uploaded as an INSTANCE buffer.
//   - one instanced draw renders every glyph; the fragment shader treats each
//     glyph as a heightmap and lights it with one directional light. Surface
//     angles come from a directional difference-of-Gaussians of the height.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use wgpu::util::DeviceExt;
use winit::{
    dpi::PhysicalSize,
    event::{Event, KeyEvent, WindowEvent},
    event_loop::EventLoop,
    keyboard::{Key, NamedKey},
    window::WindowBuilder,
};

const CELL: u32 = 256; // per-glyph texture size (must match gen_glyphs.py)
const EM: f32 = 170.0; // font size used by gen_glyphs.py (for line spacing)
const OX: f32 = 40.0; // pen origin x inside the cell (must match gen_glyphs.py)
const OY: f32 = 195.0; // baseline y inside the cell (must match gen_glyphs.py)

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Glyph {
    pos: [f32; 2],
    size: [f32; 2],
    layer: u32,
}

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
    dog_sigma: f32, // gaussian width (texels) for the angle estimate
    mode: u32,      // 0 = lit, 1 = show heightmap, 2 = show normals
    _pad: [u32; 3],
}

struct GMeta {
    layer: u32,
    advance: f32,
}

// Load the pre-rendered glyph textures + metrics from ./textures/ into one
// layer-major buffer for a texture_2d_array.
fn load_glyphs() -> (Vec<u8>, HashMap<char, GMeta>, Vec<char>) {
    let dir = std::path::Path::new("textures");
    let metrics = std::fs::read_to_string(dir.join("metrics.txt"))
        .expect("textures/metrics.txt missing -- run `python3 gen_glyphs.py` first");

    let mut data = Vec::new();
    let mut map = HashMap::new();
    let mut chars = Vec::new();
    for (layer, line) in metrics.lines().enumerate() {
        let mut it = line.split_whitespace();
        let cp: u32 = it.next().unwrap().parse().unwrap();
        let advance: f32 = it.next().unwrap().parse().unwrap();
        let file = it.next().unwrap();
        let ch = char::from_u32(cp).unwrap();

        let img = image::open(dir.join(file)).unwrap().to_luma8();
        assert_eq!(img.dimensions(), (CELL, CELL), "texture {file} wrong size");
        data.extend_from_slice(img.as_raw());
        map.insert(
            ch,
            GMeta {
                layer: layer as u32,
                advance,
            },
        );
        chars.push(ch);
    }
    (data, map, chars)
}

// CPU layout with word-wrap -> per-glyph quads.
fn layout(
    text: &str,
    map: &HashMap<char, GMeta>,
    scale: f32,
    origin: [f32; 2],
    max_w: f32,
) -> (Vec<Glyph>, Vec<String>) {
    let mut glyphs = Vec::new();
    let mut lines = Vec::new();
    let mut cur = String::new();
    let space_adv = map[&' '].advance * scale;
    let cell = CELL as f32 * scale;
    let line_h = EM * scale * 1.35;
    let mut pen_x = origin[0];
    let mut pen_y = origin[1];

    for word in text.split(' ') {
        let w: f32 = word.chars().map(|c| map[&c].advance * scale).sum();
        if pen_x > origin[0] && pen_x + w > origin[0] + max_w {
            lines.push(std::mem::take(&mut cur));
            pen_x = origin[0];
            pen_y += line_h;
        }
        for c in word.chars() {
            let g = &map[&c];
            glyphs.push(Glyph {
                pos: [pen_x - OX * scale, pen_y - OY * scale],
                size: [cell, cell],
                layer: g.layer,
            });
            pen_x += g.advance * scale;
            cur.push(c);
        }
        pen_x += space_adv;
        cur.push(' ');
    }
    lines.push(cur);
    (glyphs, lines)
}

fn main() {
    pollster::block_on(run());
}

async fn run() {
    let text = "Hey Now Brown Cow!";

    // debug view: --height shows the heightmap, --angle shows the normals
    let args: Vec<String> = std::env::args().collect();
    let mode: u32 = if args.iter().any(|a| a == "--height") {
        1
    } else if args.iter().any(|a| a == "--angle") {
        2
    } else {
        0
    };

    // --- load pre-rendered glyph source textures ---
    let (tex_data, gmap, chars) = load_glyphs();
    let n_layers = chars.len() as u32;

    let scale = 0.62;
    let (glyphs, lines) = layout(text, &gmap, scale, [50.0, 150.0], 1080.0);
    println!(
        "loaded {} glyph textures; laid out {} glyphs:",
        n_layers,
        glyphs.len()
    );
    for l in &lines {
        println!("  |{}|", l.trim_end());
    }

    // --- window + wgpu ---
    let ev = EventLoop::new().unwrap();
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("shad-env: heightmap-lit text")
            .with_inner_size(PhysicalSize::new(1200u32, 360u32))
            .build(&ev)
            .unwrap(),
    );

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
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .unwrap_or(caps.formats[0]);
    let mut config = wgpu::SurfaceConfiguration {
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

    // --- upload glyph heightmaps as a texture array ---
    let font_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("font glyphs"),
        size: wgpu::Extent3d {
            width: CELL,
            height: CELL,
            depth_or_array_layers: n_layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &font_tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &tex_data,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(CELL),
            rows_per_image: Some(CELL),
        },
        wgpu::Extent3d {
            width: CELL,
            height: CELL,
            depth_or_array_layers: n_layers,
        },
    );
    let font_view = font_tex.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let samp = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    // --- buffers ---
    let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("glyph instances"),
        contents: bytemuck::cast_slice(&glyphs),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("uniforms"),
        size: std::mem::size_of::<Uniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // --- bind group ---
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
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: ubuf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&font_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&samp),
            },
        ],
    });

    // --- pipeline ---
    let shader = device.create_shader_module(wgpu::include_wgsl!("text.wgsl"));
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&bgl],
        push_constant_ranges: &[],
    });
    let inst_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Glyph>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 8,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 2,
                format: wgpu::VertexFormat::Uint32,
            },
        ],
    };
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs",
            buffers: &[inst_layout],
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
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        multiview: None,
    });

    let n_glyphs = glyphs.len() as u32;
    let start = Instant::now();

    ev.run(move |event, elwt| {
        elwt.set_control_flow(winit::event_loop::ControlFlow::Poll);
        if let Event::WindowEvent { event, .. } = event {
            match event {
                WindowEvent::CloseRequested
                | WindowEvent::KeyboardInput {
                    event:
                        KeyEvent {
                            logical_key: Key::Named(NamedKey::Escape),
                            ..
                        },
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
                    let u = Uniforms {
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
                    queue.write_buffer(&ubuf, 0, bytemuck::bytes_of(&u));

                    let frame = match surface.get_current_texture() {
                        Ok(f) => f,
                        Err(_) => {
                            surface.configure(&device, &config);
                            return;
                        }
                    };
                    let view = frame.texture.create_view(&Default::default());
                    let mut enc = device.create_command_encoder(&Default::default());
                    {
                        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
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
                        rp.set_pipeline(&pipeline);
                        rp.set_bind_group(0, &bind, &[]);
                        rp.set_vertex_buffer(0, instances.slice(..));
                        rp.draw(0..4, 0..n_glyphs); // 4 strip verts x N glyphs, one draw
                    }
                    queue.submit([enc.finish()]);
                    frame.present();
                    window.request_redraw();
                }
                _ => {}
            }
        }
    })
    .unwrap();
}
