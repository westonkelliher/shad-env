// Pong, built on the shad-env lib. The lib owns all wgpu state and just draws
// shader-rects fed by uniforms; THIS file owns the window, the event loop, and
// every bit of game logic. See specs/pong_on_shad.txt.
//
//   - ball   : a small rect we move_shad to the ball's position each frame
//   - paddles: FIXED full-height strips; the paddle's position/size is pushed
//              in via set_uniform_value (s0 = center, s1 = half-height)
//   - scores : 7-segment digit shads, the count pushed in via s0

use std::sync::Arc;
use std::time::Instant;

use shad_env::{ShadEnv, UniformValue::Scalar};
use winit::{
    dpi::PhysicalSize,
    event::{ElementState, Event, KeyEvent, WindowEvent},
    event_loop::EventLoop,
    keyboard::{Key, NamedKey},
    window::WindowBuilder,
};

const W: f32 = 800.0;
const H: f32 = 500.0;
const PADDLE_W: f32 = 14.0;
const LEFT_FACE: f32 = 24.0 + PADDLE_W; // right face of the left paddle strip
const RIGHT_FACE: f32 = W - 24.0 - PADDLE_W; // left face of the right paddle strip
const PADDLE_HALF: f32 = 55.0;
const BALL_R: f32 = 9.0;
const PADDLE_SPEED: f32 = 380.0;
const AI_SPEED: f32 = 330.0;

// Each shader, pinned to the hash of its file (version-control guard: if the
// .wgsl changes without updating the hash, register_shader errors out).
const SHADERS: [(&str, &str, &str); 3] = [
    ("ball", concat!(env!("CARGO_MANIFEST_DIR"), "/src/ball.wgsl"), "bd6c2c9743b92532"),
    ("paddle", concat!(env!("CARGO_MANIFEST_DIR"), "/src/paddle.wgsl"), "dc63ade1c1d7f8c2"),
    ("digit", concat!(env!("CARGO_MANIFEST_DIR"), "/src/digit.wgsl"), "aaf951f760c4d2d6"),
];

/// A tiny xorshift32 so the right paddle wanders without a `rand` dependency.
struct Rng(u32);
impl Rng {
    fn next_unit(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x as f32 / u32::MAX as f32 // 0..1
    }
}

struct Game {
    ball: [f32; 2],
    vel: [f32; 2],
    left_y: f32,
    right_y: f32,
    score_l: u32,
    score_r: u32,
    up: bool,
    down: bool,
    ai_offset: f32,
    rng: Rng,
}

impl Game {
    fn new() -> Game {
        Game {
            ball: [W / 2.0, H / 2.0],
            vel: [-260.0, 150.0],
            left_y: H / 2.0,
            right_y: H / 2.0,
            score_l: 0,
            score_r: 0,
            up: false,
            down: false,
            ai_offset: 0.0,
            rng: Rng(0x1234_5678),
        }
    }

    fn update(&mut self, dt: f32) {
        // left paddle: player input
        if self.up {
            self.left_y -= PADDLE_SPEED * dt;
        }
        if self.down {
            self.left_y += PADDLE_SPEED * dt;
        }
        self.left_y = self.left_y.clamp(PADDLE_HALF, H - PADDLE_HALF);

        // right paddle: wander, biased toward the ball's height
        if self.rng.next_unit() < 0.03 {
            self.ai_offset = (self.rng.next_unit() * 2.0 - 1.0) * 90.0;
        }
        let target = (self.ball[1] + self.ai_offset).clamp(PADDLE_HALF, H - PADDLE_HALF);
        let step = (AI_SPEED * dt).min((target - self.right_y).abs());
        self.right_y += step * (target - self.right_y).signum();

        // ball
        self.ball[0] += self.vel[0] * dt;
        self.ball[1] += self.vel[1] * dt;

        // top/bottom walls
        if self.ball[1] < BALL_R {
            self.ball[1] = BALL_R;
            self.vel[1] = self.vel[1].abs();
        } else if self.ball[1] > H - BALL_R {
            self.ball[1] = H - BALL_R;
            self.vel[1] = -self.vel[1].abs();
        }

        // paddles
        if self.vel[0] < 0.0
            && self.ball[0] - BALL_R <= LEFT_FACE
            && self.ball[0] - BALL_R >= LEFT_FACE - 24.0
            && (self.ball[1] - self.left_y).abs() <= PADDLE_HALF
        {
            self.vel[0] = self.vel[0].abs();
            self.ball[0] = LEFT_FACE + BALL_R;
        }
        if self.vel[0] > 0.0
            && self.ball[0] + BALL_R >= RIGHT_FACE
            && self.ball[0] + BALL_R <= RIGHT_FACE + 24.0
            && (self.ball[1] - self.right_y).abs() <= PADDLE_HALF
        {
            self.vel[0] = -self.vel[0].abs();
            self.ball[0] = RIGHT_FACE - BALL_R;
        }

        // scoring: reset to middle, velocity unchanged (per spec)
        if self.ball[0] < -BALL_R {
            self.score_r += 1;
            self.ball = [W / 2.0, H / 2.0];
        } else if self.ball[0] > W + BALL_R {
            self.score_l += 1;
            self.ball = [W / 2.0, H / 2.0];
        }
    }

    /// Push current state into the shads.
    fn sync(&self, env: &mut ShadEnv) {
        env.move_shad(
            "ball",
            [self.ball[0] - BALL_R, self.ball[1] - BALL_R, self.ball[0] + BALL_R, self.ball[1] + BALL_R],
            None,
        )
        .unwrap();
        for (handle, y) in [("left", self.left_y), ("right", self.right_y)] {
            env.set_uniform_value(handle, "s0", Scalar(y / H)).unwrap();
            env.set_uniform_value(handle, "s1", Scalar(PADDLE_HALF / H)).unwrap();
        }
        env.set_uniform_value("score_l", "s0", Scalar(self.score_l.min(9) as f32)).unwrap();
        env.set_uniform_value("score_r", "s0", Scalar(self.score_r.min(9) as f32)).unwrap();
    }
}

fn main() {
    pollster::block_on(run());
}

async fn run() {
    let event_loop = EventLoop::new().unwrap();
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("shad-env: pong")
            .with_inner_size(PhysicalSize::new(W as u32, H as u32))
            .build(&event_loop)
            .unwrap(),
    );

    let mut env = ShadEnv::new().await;
    env.configure(window.clone()).unwrap();

    for (handle, path, hash) in SHADERS {
        env.register_shader(handle, path, hash).unwrap();
    }

    // ball follows the ball; paddles are fixed full-height strips; scores sit
    // at the top corners. (x1,y1,x2,y2 corners.)
    env.add_shad("ball", "ball", [0.0, 0.0, 2.0 * BALL_R, 2.0 * BALL_R], Some(1.0)).unwrap();
    env.add_shad("left", "paddle", [24.0, 0.0, LEFT_FACE, H], None).unwrap();
    env.add_shad("right", "paddle", [RIGHT_FACE, 0.0, RIGHT_FACE + PADDLE_W, H], None).unwrap();
    env.add_shad("score_l", "digit", [W * 0.25 - 25.0, 20.0, W * 0.25 + 25.0, 90.0], None).unwrap();
    env.add_shad("score_r", "digit", [W * 0.75 - 25.0, 20.0, W * 0.75 + 25.0, 90.0], None).unwrap();

    let mut game = Game::new();
    let mut last = Instant::now();

    event_loop
        .run(move |event, elwt| {
            elwt.set_control_flow(winit::event_loop::ControlFlow::Poll);
            let Event::WindowEvent { event, .. } = event else { return };
            match event {
                WindowEvent::CloseRequested => elwt.exit(),
                WindowEvent::KeyboardInput { event: KeyEvent { logical_key, state, .. }, .. } => {
                    let down = state == ElementState::Pressed;
                    match logical_key {
                        Key::Named(NamedKey::Escape) => elwt.exit(),
                        Key::Named(NamedKey::ArrowUp) => game.up = down,
                        Key::Named(NamedKey::ArrowDown) => game.down = down,
                        _ => {}
                    }
                }
                WindowEvent::Resized(s) => {
                    env.resize(s.width, s.height).unwrap();
                }
                WindowEvent::RedrawRequested => {
                    let now = Instant::now();
                    let dt = (now - last).as_secs_f32().min(0.05);
                    last = now;

                    game.update(dt);
                    game.sync(&mut env);
                    if let Err(e) = env.render() {
                        eprintln!("render error: {e:?}");
                    }
                    window.request_redraw();
                }
                _ => {}
            }
        })
        .unwrap();
}
