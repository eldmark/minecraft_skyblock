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
}

impl Input {
    pub fn is_idle(&self) -> bool {
        self.orbit == (0.0, 0.0) && self.zoom == 0.0 && !self.reseed && self.quality.is_none()
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
        if self.window.is_key_down(Key::W) {
            input.zoom -= 0.5;
        }
        if self.window.is_key_down(Key::S) {
            input.zoom += 0.5;
        }

        input.reseed = self.window.is_key_pressed(Key::R, minifb::KeyRepeat::No);
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
