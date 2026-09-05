//! The only place minifb is touched. It blits a CPU-computed buffer to X11
//! (XPutImage) and reports input; no OpenGL context, no GPU work.

use std::io;

use minifb::{Key, MouseButton, MouseMode, Scale, ScaleMode, Window, WindowOptions};

use crate::output::{Framebuffer, Output};

/// What the user did this frame, in renderer terms rather than key codes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub orbit: (f32, f32),
    pub zoom: f32,
    pub reseed: bool,
    pub screenshot: bool,
    pub quality: Option<usize>,
    /// `D` starts and stops the day/night cycle.
    pub toggle_cycle: bool,
    /// `,` and `.` nudge the clock by hand, in fractions of a day.
    pub time_nudge: f32,
    /// `F` swaps between orbiting the island and flying freely.
    pub toggle_free: bool,
    /// Free flight: (forward, right, up), each in `[-1, 1]`.
    pub move_axes: (f32, f32, f32),
    /// Left Ctrl: fly faster.
    pub boost: bool,
}

impl Input {
    pub fn is_idle(&self) -> bool {
        self.orbit == (0.0, 0.0)
            && self.zoom == 0.0
            && !self.reseed
            && self.quality.is_none()
            && !self.toggle_cycle
            && self.time_nudge == 0.0
            && !self.toggle_free
            && self.move_axes == (0.0, 0.0, 0.0)
    }
}

pub struct WindowOutput {
    window: Window,
    title: String,
    last_mouse: Option<(f32, f32)>,
}

impl WindowOutput {
    pub fn new(title: &str, width: usize, height: usize) -> io::Result<Self> {
        let window = Window::new(
            title,
            width,
            height,
            WindowOptions {
                resize: true,
                scale: Scale::X1,
                scale_mode: ScaleMode::Stretch,
                ..WindowOptions::default()
            },
        )
        .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(WindowOutput {
            window,
            title: title.to_string(),
            last_mouse: None,
        })
    }

    pub fn is_open(&self) -> bool {
        self.window.is_open() && !self.window.is_key_down(Key::Escape)
    }

    /// Collect this frame's camera intent. Mouse drag orbits, wheel and W/S zoom.
    pub fn poll_input(&mut self) -> Input {
        let mut input = Input::default();

        let dragging = self.window.get_mouse_down(MouseButton::Left);
        let mouse = self.window.get_mouse_pos(MouseMode::Pass);
        match (dragging, mouse, self.last_mouse) {
            (true, Some(now), Some(prev)) => {
                input.orbit = (now.0 - prev.0, now.1 - prev.1);
                self.last_mouse = Some(now);
            }
            (true, Some(now), None) => self.last_mouse = Some(now),
            _ => self.last_mouse = None,
        }

        let arrow_step = 6.0;
        if self.window.is_key_down(Key::Left) {
            input.orbit.0 -= arrow_step;
        }
        if self.window.is_key_down(Key::Right) {
            input.orbit.0 += arrow_step;
        }
        if self.window.is_key_down(Key::Up) {
            input.orbit.1 -= arrow_step;
        }
        if self.window.is_key_down(Key::Down) {
            input.orbit.1 += arrow_step;
        }

        if let Some((_, wheel)) = self.window.get_scroll_wheel() {
            input.zoom -= wheel;
        }

        // W/S mean "closer/further" while orbiting and "forward/back" while
        // flying; the caller picks which reading to use, so both are reported.
        let axis = |window: &Window, positive: Key, negative: Key| -> f32 {
            (window.is_key_down(positive) as i32 - window.is_key_down(negative) as i32) as f32
        };
        let forward = axis(&self.window, Key::W, Key::S);
        input.zoom -= forward * 0.5;
        input.move_axes = (
            forward,
            axis(&self.window, Key::D, Key::A),
            axis(&self.window, Key::Space, Key::LeftShift),
        );
        input.boost = self.window.is_key_down(Key::LeftCtrl);
        input.toggle_free = self.window.is_key_pressed(Key::F, minifb::KeyRepeat::No);

        input.reseed = self.window.is_key_pressed(Key::R, minifb::KeyRepeat::No);
        input.toggle_cycle = self.window.is_key_pressed(Key::D, minifb::KeyRepeat::No);
        if self.window.is_key_down(Key::Comma) {
            input.time_nudge -= 0.004;
        }
        if self.window.is_key_down(Key::Period) {
            input.time_nudge += 0.004;
        }
        input.screenshot = self.window.is_key_pressed(Key::P, minifb::KeyRepeat::No);
        for (i, key) in [Key::Key1, Key::Key2, Key::Key3, Key::Key4].iter().enumerate() {
            if self.window.is_key_pressed(*key, minifb::KeyRepeat::No) {
                input.quality = Some(i + 1);
            }
        }
        input
    }

    pub fn size(&self) -> (usize, usize) {
        self.window.get_size()
    }
}

impl Output for WindowOutput {
    fn present(&mut self, frame: &Framebuffer) -> io::Result<bool> {
        self.window
            .update_with_buffer(&frame.pixels, frame.width, frame.height)
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(self.is_open())
    }

    fn set_status(&mut self, status: &str) {
        self.window.set_title(&format!("{} — {}", self.title, status));
    }
}
