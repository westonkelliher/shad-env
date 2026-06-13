// Text on shad-env: ONE shad renders a whole line by reading two data sources --
// a prebaked ASCII atlas (texture) and the string's codepoints (buffer). The
// shader carves its own rect into character cells; the lib just draws one quad.
// Shows off `register_texture` / `set_texture` / `register_buffer` / `set_buffer`
// and `register_shader`'s new `validate` flag (false here -- no pinned hash).

use std::sync::Arc;

use font8x8::{UnicodeFonts, BASIC_FONTS};
use shad_env::{ShadEnv, UniformValue::Scalar};
use winit::{
    dpi::PhysicalSize,
    event::{Event, WindowEvent},
    event_loop::EventLoop,
    window::WindowBuilder,
};

const MSG: &str = "Hey now Brown Cow!";
const W: f32 = 800.0;
const H: f32 = 140.0;
const CELL: f32 = 40.0; // on-screen px per character

// font8x8 glyphs are 8x8; the atlas is a 16x8 grid of them, indexed by codepoint.
const GLYPH: u32 = 8;
const COLS: u32 = 16;
const ROWS: u32 = 8;

/// Bake the printable ASCII range into one RGBA8 atlas: white ink (opaque) on a
/// transparent ground, so the shader can use alpha as glyph coverage.
fn build_atlas() -> (Vec<u8>, u32, u32) {
    let (aw, ah) = (COLS * GLYPH, ROWS * GLYPH);
    let mut px = vec![0u8; (aw * ah * 4) as usize];
    for code in 0u32..(COLS * ROWS) {
        let Some(ch) = char::from_u32(code) else { continue };
        let Some(glyph) = BASIC_FONTS.get(ch) else { continue };
        let (cx, cy) = ((code % COLS) * GLYPH, (code / COLS) * GLYPH);
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..GLYPH {
                if (bits >> col) & 1 == 1 {
                    // font8x8: bit 0 is the leftmost column
                    let x = cx + col;
                    let y = cy + row as u32;
                    let i = ((y * aw + x) * 4) as usize;
                    px[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
    }
    (px, aw, ah)
}

fn main() {
    pollster::block_on(run());
}

async fn run() {
    let event_loop = EventLoop::new().unwrap();
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("shad-env: text")
            .with_inner_size(PhysicalSize::new(W as u32, H as u32))
            .build(&event_loop)
            .unwrap(),
    );

    let mut env = ShadEnv::new().await;
    env.configure(window.clone()).unwrap();

    // no pinned hash for this demo -> validate = false
    env.register_shader(
        "text",
        concat!(env!("CARGO_MANIFEST_DIR"), "/src/text.wgsl"),
        "",
        false,
    )
    .unwrap();

    // texture: the ASCII atlas. buffer: the message as one u32 codepoint per char.
    let (atlas, aw, ah) = build_atlas();
    env.register_texture("ascii", &atlas, aw, ah).unwrap();
    let codes: Vec<u32> = MSG.chars().map(|c| c as u32).collect();
    env.register_buffer("msg", bytemuck::cast_slice(&codes)).unwrap();

    // one shad, centered, sized so each cell is square (CELL x CELL)
    let tw = MSG.chars().count() as f32 * CELL;
    let x0 = (W - tw) / 2.0;
    let y0 = (H - CELL) / 2.0;
    env.add_shad("label", "text", [x0, y0, x0 + tw, y0 + CELL], None).unwrap();
    env.set_texture("label", "ascii").unwrap();
    env.set_buffer("label", "msg").unwrap();
    env.set_uniform_value("label", "s0", Scalar(codes.len() as f32)).unwrap();

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
