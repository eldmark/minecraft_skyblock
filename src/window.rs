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
    /// `Q` starts and stops the day/night cycle.
    pub toggle_cycle: bool,
    /// `,` and `.` nudge the clock by hand, in fractions of a day.
    pub time_nudge: f32,
    /// `F` swaps between orbiting the island and flying freely.
    pub toggle_free: bool,
    /// Movement intent: (forward, right, up), each in `[-1, 1]`. W/A/S/D drive
    /// forward and right in both camera modes; Space and Shift drive up and down
    /// while flying.
    pub move_axes: (f32, f32, f32),
    /// Ctrl: move faster.
    pub boost: bool,
    /// `1`-`9` pick a slot on the hotbar.
    pub select_slot: Option<usize>,
    /// Enter: use the selected item, once.
    pub use_item: bool,
    /// Enter held down: for the items that sweep instead of firing.
    pub use_held: bool,
    /// `H` hides and shows the overlay.
    pub toggle_hud: bool,
    /// `E` opens and closes the inventory.
    pub toggle_inventory: bool,
    /// Where the cursor is, in pixels, when it is over the window.
    pub mouse: Option<(f32, f32)>,
    /// Left button pressed and released without dragging: dragging is how the
    /// camera turns, so only a click that stays put counts as a click.
    pub click_left: bool,
    /// Right button, on the frame it goes down.
    pub click_right: bool,
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
            && !self.use_item
    }
}

pub struct WindowOutput {
    window: Window,
    title: String,
    last_mouse: Option<(f32, f32)>,
    /// Where the left button went down, and how far it has travelled since:
    /// that is what separates a click from a drag.
    press_at: Option<(f32, f32)>,
    dragged: f32,
    right_was_down: bool,
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
            press_at: None,
            dragged: 0.0,
            right_was_down: false,
        })
    }

    pub fn is_open(&self) -> bool {
        self.window.is_open() && !self.window.is_key_down(Key::Escape)
    }

    /// Collect this frame's camera intent. Arrows and the mouse turn the camera,
    /// W/A/S/D move it, Space and Shift fly up and down, Q runs the cycle, E
    /// opens the inventory, and the mouse buttons edit the world.
    pub fn poll_input(&mut self) -> Input {
        let mut input = Input::default();

        let dragging = self.window.get_mouse_down(MouseButton::Left);
        let mouse = self.window.get_mouse_pos(MouseMode::Pass);
        input.mouse = mouse;
        match (dragging, mouse, self.last_mouse) {
            (true, Some(now), Some(prev)) => {
                input.orbit = (now.0 - prev.0, now.1 - prev.1);
                self.dragged += input.orbit.0.abs() + input.orbit.1.abs();
                self.last_mouse = Some(now);
            }
            (true, Some(now), None) => self.last_mouse = Some(now),
            _ => self.last_mouse = None,
        }

        // A click is a press and a release that did not travel: the same button
        // turns the camera, so anything that moved is a drag and nothing else.
        match (dragging, self.press_at) {
            (true, None) => {
                self.press_at = mouse;
                self.dragged = 0.0;
            }
            (false, Some(at)) => {
                if self.dragged < 4.0 {
                    input.click_left = true;
                    input.mouse = input.mouse.or(Some(at));
                }
                self.press_at = None;
            }
            _ => {}
        }

        let right_down = self.window.get_mouse_down(MouseButton::Right);
        input.click_right = right_down && !self.right_was_down;
        self.right_was_down = right_down;

        let arrow_step = 6.0;
        if self.window.is_key_down(Key::Left) {
            input.orbit.0 -= arrow_step;
        }
        if self.window.is_key_down(Key::Right) {
            input.orbit.0 += arrow_step;
        }
        // Up looks up. The arrows are a head turning, not a hand dragging the
        // island around, so the vertical sign is the opposite of the mouse's.
        if self.window.is_key_down(Key::Up) {
            input.orbit.1 += arrow_step;
        }
        if self.window.is_key_down(Key::Down) {
            input.orbit.1 -= arrow_step;
        }

        if let Some((_, wheel)) = self.window.get_scroll_wheel() {
            input.zoom -= wheel;
        }

        // W/S mean "closer/further" while orbiting and "forward/back" while
        // flying, and A/D mean "swing around" against "strafe"; the caller picks
        // which reading to use, so both are reported for the same keys.
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
        input.toggle_cycle = self.window.is_key_pressed(Key::Q, minifb::KeyRepeat::No);
        input.toggle_inventory = self.window.is_key_pressed(Key::E, minifb::KeyRepeat::No);
        if self.window.is_key_down(Key::Comma) {
            input.time_nudge -= 0.004;
        }
        if self.window.is_key_down(Key::Period) {
            input.time_nudge += 0.004;
        }
        input.screenshot = self.window.is_key_pressed(Key::P, minifb::KeyRepeat::No);
        input.toggle_hud = self.window.is_key_pressed(Key::H, minifb::KeyRepeat::No);

        // The number row picks a slot on the hotbar, as in the game; the
        // resolution scale moved to the function keys to make room for it.
        let slot_keys = [
            Key::Key1,
            Key::Key2,
            Key::Key3,
            Key::Key4,
            Key::Key5,
            Key::Key6,
            Key::Key7,
            Key::Key8,
            Key::Key9,
        ];
        for (i, key) in slot_keys.iter().enumerate() {
            if self.window.is_key_pressed(*key, minifb::KeyRepeat::No) {
                input.select_slot = Some(i);
            }
        }
        input.use_item = self.window.is_key_pressed(Key::Enter, minifb::KeyRepeat::No);
        input.use_held = self.window.is_key_down(Key::Enter);
        for (i, key) in [Key::F1, Key::F2, Key::F3, Key::F4].iter().enumerate() {
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
