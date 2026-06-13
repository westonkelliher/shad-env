// windowed-shad-env: the deliberate opposite of shad-env's no-target purity.
// shad-env owns the GPU state and draws into a view someone hands it; THIS crate
// owns the window + surface + swapchain loop, so an app writes only setup +
// per-frame game logic + shad calls, with zero visible winit/surface code. It
// builds the window/surface from shad-env's exposed wgpu handles (+ FORMAT),
// runs the winit event loop, drives each frame (acquire -> update -> render_to ->
// present -> request_redraw), and supplies dt + a small key Input. It also folds
// in headless `--screenshot [path]` (render one frame offscreen + read_rgba ->
// PNG), with a determinism hook (step a fixed-dt sim before the shot).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use shad_env::wgpu;
use winit::{
    dpi::PhysicalSize,
    event::{ElementState, Event, KeyEvent, WindowEvent},
    event_loop::EventLoop,
    window::WindowBuilder,
};

// This crate is the single dependency an app needs: re-export shad-env's API
// (so apps name `ShadEnv`/`UniformValue` through us, never depending on shad-env
// directly) and winit's key types (so apps name keys without a winit dep).
pub use shad_env::{self, ShadEnv, UniformValue};
pub use winit::keyboard::{Key, NamedKey};

/// Per-frame keyboard state: which keys are currently held. Esc is handled by
/// the wrapper (it closes the window), so apps only query their game keys.
#[derive(Default)]
pub struct Input {
    held: HashSet<Key>,
}

impl Input {
    /// True while `key` is held down, e.g. `Key::Named(NamedKey::ArrowUp)`.
    pub fn held(&self, key: Key) -> bool {
        self.held.contains(&key)
    }
}

/// Window config + the headless-screenshot knobs. Build with `new`, optionally
/// set the screenshot warmup/path, then `run` with a setup + per-frame update.
pub struct App {
    title: String,
    width: u32,
    height: u32,
    warmup_steps: u32,
    warmup_dt: f32,
    screenshot_path: String,
}

impl App {
    /// A window `width` x `height` px titled `title`. No screenshot warmup; the
    /// default `--screenshot` path is `screenshot.png`.
    pub fn new(title: &str, width: u32, height: u32) -> App {
        App {
            title: title.to_string(),
            width,
            height,
            warmup_steps: 0,
            warmup_dt: 1.0 / 60.0,
            screenshot_path: "screenshot.png".to_string(),
        }
    }

    /// In `--screenshot` mode, step the sim `steps` times at fixed `dt` before the
    /// shot, so a deterministic sim lands on a representative (lively) frame.
    pub fn screenshot_warmup(mut self, steps: u32, dt: f32) -> App {
        self.warmup_steps = steps;
        self.warmup_dt = dt;
        self
    }

    /// Default output path for `--screenshot` with no path arg.
    pub fn screenshot_path(mut self, path: &str) -> App {
        self.screenshot_path = path.to_string();
        self
    }

    /// Entry point. `--screenshot [path]` renders one frame headless to a PNG;
    /// otherwise open a live window. `setup` registers shaders/shads once;
    /// `update` runs each frame with dt + the held-key `Input`.
    pub fn run<S, U>(self, setup: S, update: U)
    where
        S: FnOnce(&mut ShadEnv),
        U: FnMut(&mut ShadEnv, f32, &Input),
    {
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(String::as_str) == Some("--screenshot") {
            let path = args.get(2).cloned().unwrap_or_else(|| self.screenshot_path.clone());
            pollster::block_on(self.screenshot(&path, setup, update));
        } else {
            pollster::block_on(self.run_windowed(setup, update));
        }
    }

    /// Headless: build the env, step the (deterministic) sim to a representative
    /// frame, render it into our own offscreen texture, and write it to `path`.
    async fn screenshot<S, U>(self, path: &str, setup: S, mut update: U)
    where
        S: FnOnce(&mut ShadEnv),
        U: FnMut(&mut ShadEnv, f32, &Input),
    {
        let mut env = ShadEnv::new().await;
        setup(&mut env);

        // advance the sim with no keys held; one no-op update for static scenes
        let input = Input::default();
        if self.warmup_steps == 0 {
            update(&mut env, 0.0, &input);
        }
        for _ in 0..self.warmup_steps {
            update(&mut env, self.warmup_dt, &input);
        }

        let (w, h) = (self.width, self.height);
        let target = env.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("screenshot"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ShadEnv::FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        env.render_to(&target.create_view(&Default::default()), w, h);
        let rgba = shad_env::read_rgba(env.device(), env.queue(), &target, w, h);
        image::RgbaImage::from_raw(w, h, rgba)
            .expect("buffer size matches dimensions")
            .save(path)
            .unwrap();
        println!("wrote {path}");
    }

    /// Live: build the window + surface from shad-env's handles, then run the
    /// winit event loop, driving each frame through `update` and `render_to`.
    async fn run_windowed<S, U>(self, setup: S, mut update: U)
    where
        S: FnOnce(&mut ShadEnv),
        U: FnMut(&mut ShadEnv, f32, &Input),
    {
        let event_loop = EventLoop::new().unwrap();
        let window = Arc::new(
            WindowBuilder::new()
                .with_title(&self.title)
                .with_inner_size(PhysicalSize::new(self.width, self.height))
                .build(&event_loop)
                .unwrap(),
        );

        let mut env = ShadEnv::new().await;
        setup(&mut env);

        // We own the surface + swapchain loop; shad-env only draws into the frame
        // view we hand it. Build the surface from shad-env's wgpu handles.
        let surface = env.instance().create_surface(window.clone()).unwrap();
        let size = window.inner_size();
        let mut config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: ShadEnv::FORMAT,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface.get_capabilities(env.adapter()).alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(env.device(), &config);

        let mut input = Input::default();
        let mut last = Instant::now();

        event_loop
            .run(move |event, elwt| {
                elwt.set_control_flow(winit::event_loop::ControlFlow::Poll);
                let Event::WindowEvent { event, .. } = event else { return };
                match event {
                    WindowEvent::CloseRequested => elwt.exit(),
                    WindowEvent::KeyboardInput {
                        event: KeyEvent { logical_key, state, .. }, ..
                    } => {
                        // Esc closes; everything else feeds the held-key set.
                        if logical_key == Key::Named(NamedKey::Escape) {
                            elwt.exit();
                        } else if state == ElementState::Pressed {
                            input.held.insert(logical_key);
                        } else {
                            input.held.remove(&logical_key);
                        }
                    }
                    WindowEvent::Resized(s) => {
                        config.width = s.width.max(1);
                        config.height = s.height.max(1);
                        surface.configure(env.device(), &config);
                    }
                    WindowEvent::RedrawRequested => {
                        let now = Instant::now();
                        let dt = (now - last).as_secs_f32().min(0.05);
                        last = now;

                        update(&mut env, dt, &input);
                        let frame = match surface.get_current_texture() {
                            Ok(f) => f,
                            // transient loss: reconfigure and skip this frame
                            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                                surface.configure(env.device(), &config);
                                return;
                            }
                            Err(e) => {
                                eprintln!("surface error: {e:?}");
                                return;
                            }
                        };
                        let view = frame.texture.create_view(&Default::default());
                        env.render_to(&view, config.width, config.height);
                        frame.present();
                        window.request_redraw();
                    }
                    _ => {}
                }
            })
            .unwrap();
    }
}
