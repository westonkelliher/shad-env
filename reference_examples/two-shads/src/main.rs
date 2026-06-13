// Two "shads" -- independent fragment shaders bound to two rectangular
// regions of one window. This is the core shad-env idea in miniature:
//
//   - a shad = a fragment shader + a rect (+ its own uniforms/pipeline)
//   - the vertex stage is shared boilerplate (shared.wgsl): one triangle
//     that covers the viewport, so the FRAGMENT shader alone decides what
//     the rect looks like
//   - per frame: one render pass; for each shad, set_viewport(its rect)
//     and draw the triangle

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

const SHARED: &str = include_str!("shared.wgsl");

/// Matches `U` in shared.wgsl.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    rect: [f32; 4], // x, y, w, h in window pixels
    time: f32,
    _pad: [f32; 3],
}

struct Shad {
    rect: [f32; 4],
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
}

/// Build one shad: shader module = shared prelude + this shad's fragment
/// shader, plus a uniform buffer and a pipeline that draws 3 vertices.
fn create_shad(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    fs_src: &str,
    rect: [f32; 4],
) -> Shad {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(format!("{SHARED}\n{fs_src}").into()),
    });

    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("shad uniforms"),
        contents: bytemuck::bytes_of(&Uniforms { rect, time: 0.0, _pad: [0.0; 3] }),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buf.as_entire_binding(),
        }],
    });

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
            buffers: &[], // no vertex data: vs generates the triangle itself
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
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview: None,
    });

    Shad { rect, pipeline, bind_group, uniform_buf }
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

fn main() {
    pollster::block_on(run());
}

async fn run() {
    let event_loop = EventLoop::new().unwrap();
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("shad-env: two shads")
            .with_inner_size(PhysicalSize::new(820u32, 420u32))
            .build(&event_loop)
            .unwrap(),
    );
    let (surface, device, queue, mut config) = init_wgpu(window.clone()).await;

    let shads = [
        create_shad(&device, config.format, include_str!("ellipse.wgsl"), [40.0, 60.0, 400.0, 300.0]),
        create_shad(&device, config.format, include_str!("rounded_rect.wgsl"), [490.0, 60.0, 290.0, 300.0]),
    ];

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
                    let t = start.elapsed().as_secs_f32();
                    for shad in &shads {
                        let u = Uniforms { rect: shad.rect, time: t, _pad: [0.0; 3] };
                        queue.write_buffer(&shad.uniform_buf, 0, bytemuck::bytes_of(&u));
                    }

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
                        for shad in &shads {
                            let [x, y, w, h] = shad.rect;
                            // clamp so the viewport never exceeds the window
                            // (a viewport outside the framebuffer is an error)
                            let w = w.min(config.width as f32 - x).max(0.0);
                            let h = h.min(config.height as f32 - y).max(0.0);
                            if w == 0.0 || h == 0.0 {
                                continue;
                            }
                            pass.set_viewport(x, y, w, h, 0.0, 1.0);
                            pass.set_pipeline(&shad.pipeline);
                            pass.set_bind_group(0, &shad.bind_group, &[]);
                            pass.draw(0..3, 0..1);
                        }
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
