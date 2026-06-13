// Hello, World on shad-env: ONE shad renders a whole line by reading two data
// sources -- an ASCII spritesheet PNG (texture) and the string's codepoints
// (buffer). The shader carves its own rect into character cells; the lib draws
// one quad. Shows off `register_texture` / `set_texture` / `register_buffer` /
// `set_buffer` and `register_shader`'s `validate` flag (false here -- no hash).
//
// The atlas (`ascii_atlas.png`, a 16x8 grid of 32px glyph cells indexed by
// codepoint) is baked by `gen_atlas.py`; here we just decode it with `image`.

use std::sync::Arc;

use shad_env::{ShadEnv, UniformValue::Scalar};
use winit::{
    dpi::PhysicalSize,
    event::{Event, WindowEvent},
    event_loop::EventLoop,
    window::WindowBuilder,
};

const MSG: &str = "Hello, World!";
const W: f32 = 800.0;
const H: f32 = 140.0;
const CELL: f32 = 40.0; // on-screen px per character

/// Decode the ASCII spritesheet PNG to RGBA8. White antialiased glyphs on a
/// transparent ground, so the shader reads alpha as glyph coverage. The grid
/// (16x8, indexed by codepoint) lives in the shader, not here.
fn load_atlas() -> (Vec<u8>, u32, u32) {
    let bytes = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/ascii_atlas.png"));
    let img = image::load_from_memory(bytes).unwrap().to_rgba8();
    let (w, h) = img.dimensions();
    (img.into_raw(), w, h)
}

fn main() {
    // `cargo run -- --screenshot [path]` renders one offscreen frame to a PNG
    // (no window); otherwise open a live window. Same scene, same shaders.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--screenshot") {
        let path = args.get(2).map_or("hello_world.png", String::as_str);
        pollster::block_on(screenshot(path));
    } else {
        pollster::block_on(run());
    }
}

/// Register the shader + atlas + message and place the one centered shad.
fn setup(env: &mut ShadEnv) {
    // no pinned hash for this demo -> validate = false
    env.register_shader(
        "hello_world",
        concat!(env!("CARGO_MANIFEST_DIR"), "/src/hello_world.wgsl"),
        "",
        false,
    )
    .unwrap();

    // texture: the ASCII atlas. buffer: the message as one u32 codepoint per char.
    let (atlas, aw, ah) = load_atlas();
    env.register_texture("ascii", &atlas, aw, ah).unwrap();
    let codes: Vec<u32> = MSG.chars().map(|c| c as u32).collect();
    env.register_buffer("msg", bytemuck::cast_slice(&codes)).unwrap();

    // one shad, centered, sized so each cell is square (CELL x CELL)
    let tw = MSG.chars().count() as f32 * CELL;
    let x0 = (W - tw) / 2.0;
    let y0 = (H - CELL) / 2.0;
    env.add_shad("label", "hello_world", [x0, y0, x0 + tw, y0 + CELL], None).unwrap();
    env.set_texture("label", "ascii").unwrap();
    env.set_buffer("label", "msg").unwrap();
    env.set_uniform_value("label", "s0", Scalar(codes.len() as f32)).unwrap();
}

/// Headless: render one frame offscreen and write it to `path` as a PNG.
async fn screenshot(path: &str) {
    let mut env = ShadEnv::new().await;
    setup(&mut env);
    let rgba = env.render_to_target(W as u32, H as u32).unwrap();
    image::RgbaImage::from_raw(W as u32, H as u32, rgba)
        .expect("buffer size matches dimensions")
        .save(path)
        .unwrap();
    println!("wrote {path}");
}

async fn run() {
    let event_loop = EventLoop::new().unwrap();
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("shad-env: hello_world")
            .with_inner_size(PhysicalSize::new(W as u32, H as u32))
            .build(&event_loop)
            .unwrap(),
    );

    let mut env = ShadEnv::new().await;
    let size = window.inner_size();
    env.configure(window.clone(), size.width, size.height).unwrap();
    setup(&mut env);

    event_loop
        .run(move |event, elwt| {
            let Event::WindowEvent { event, .. } = event else { return };
            match event {
                WindowEvent::CloseRequested => elwt.exit(),
                WindowEvent::Resized(s) => env.resize(s.width, s.height).unwrap(),
                WindowEvent::RedrawRequested => {
                    if let Err(e) = env.render() {
                        eprintln!("render error: {e:?}");
                    }
                }
                _ => {}
            }
        })
        .unwrap();
}
