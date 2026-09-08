//! The only place minifb is touched. It blits a CPU-computed buffer to X11
//! (XPutImage) and reports input; no OpenGL context, no GPU work.

use std::io;

use minifb::{Key, MouseButton, MouseMode, Scale, ScaleMode, Window, WindowOptions};

use crate::render::output::{Framebuffer, Output};

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
    /// Tab turns mouselook on and off.
    pub toggle_mouselook: bool,
    /// Escape: backs out of one thing at a time rather than killing the window.
    pub escape: bool,
    /// Whether the camera is currently following the mouse without a button.
    pub mouselook: bool,
    /// The pixel a click aims at: the crosshair under mouselook, the cursor
    /// otherwise.
    pub aim: Option<(f32, f32)>,
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
    /// Minecraft-style look: the camera follows the mouse with no button held,
    /// the pointer is hidden and clicks aim at the crosshair.
    mouselook: bool,
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
            mouselook: false,
        })
    }

    /// Turn mouselook on or off, hiding the pointer with it.
    pub fn set_mouselook(&mut self, on: bool) {
        if self.mouselook != on {
            self.mouselook = on;
            self.window.set_cursor_visibility(!on);
        }
    }

    pub fn mouselook(&self) -> bool {
        self.mouselook
    }

    /// Only the window manager closes the window now. Escape is reported as
    /// input instead: under mouselook the pointer is hidden and captured by the
    /// look, and a single key that both releases it and quits is a key that
    /// quits by accident.
    pub fn is_open(&self) -> bool {
        self.window.is_open()
    }

    /// Collect this frame's camera intent. Arrows and the mouse turn the camera,
    /// W/A/S/D move it, Space and Shift fly up and down, Q runs the cycle, E
    /// opens the inventory, Tab switches mouselook, and the mouse buttons edit
    /// the world.
    pub fn poll_input(&mut self) -> Input {
        let mut input = Input::default();

        let dragging = self.window.get_mouse_down(MouseButton::Left);
        // Clamped while looking: the pointer can leave the window and the
        // position keeps reading as the border, which is what lets the edge push
        // carry on turning instead of the look dying mid-swing.
        let mouse = self.window.get_mouse_pos(if self.mouselook {
            MouseMode::Clamp
        } else {
            MouseMode::Pass
        });
        let (win_w, win_h) = self.window.get_size();
        input.mouse = mouse;
        input.mouselook = self.mouselook;
        input.aim = if self.mouselook {
            Some((win_w as f32 / 2.0, win_h as f32 / 2.0))
        } else {
            mouse
        };

        // Nothing follows the mouse while the window is not the one being used:
        // otherwise a pointer left resting in the border margin would keep the
        // camera spinning in the background.
        if self.mouselook && self.window.is_active() {
            // The camera follows the pointer with no button held. There is no
            // pointer lock to be had here — the platform layer cannot warp the
            // cursor — so when it reaches the edge of the window the view keeps
            // turning by itself instead of stopping dead.
            if let (Some(now), Some(prev)) = (mouse, self.last_mouse) {
                input.orbit.0 += now.0 - prev.0;
                input.orbit.1 -= now.1 - prev.1;
            }
            if let Some(now) = mouse {
                let (px, py) = edge_push(now, (win_w as f32, win_h as f32));
                input.orbit.0 += px;
                input.orbit.1 -= py;
            }
            self.last_mouse = mouse;
        } else {
            match (dragging, mouse, self.last_mouse) {
                (true, Some(now), Some(prev)) => {
                    input.orbit = (now.0 - prev.0, now.1 - prev.1);
                    self.dragged += input.orbit.0.abs() + input.orbit.1.abs();
                    self.last_mouse = Some(now);
                }
                (true, Some(now), None) => self.last_mouse = Some(now),
                _ => self.last_mouse = None,
            }
        }

        // A click is a press and a release that did not travel: the same button
        // turns the camera, so anything that moved is a drag and nothing else.
        match (dragging, self.press_at) {
            (true, None) => {
                self.press_at = mouse;
                self.dragged = 0.0;
            }
            (false, Some(at)) => {
                // Under mouselook the button never turns the camera, so every
                // press is a click however far the pointer travelled.
                if self.mouselook || self.dragged < 4.0 {
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
        input.toggle_mouselook = self.window.is_key_pressed(Key::Tab, minifb::KeyRepeat::No);
        input.escape = self.window.is_key_pressed(Key::Escape, minifb::KeyRepeat::No);
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

/// How hard the view turns while the pointer sits against a border, in the same
/// units as a mouse delta. Zero anywhere in the middle of the window.
///
/// This is what stands in for a pointer lock: without it a look can never turn
/// further than the window is wide.
fn edge_push(pos: (f32, f32), size: (f32, f32)) -> (f32, f32) {
    const MARGIN: f32 = 48.0;
    const SPEED: f32 = 14.0;
    let axis = |v: f32, extent: f32| -> f32 {
        if v < MARGIN {
            -(MARGIN - v.max(0.0)) / MARGIN * SPEED
        } else if v > extent - MARGIN {
            (MARGIN - (extent - v).max(0.0)) / MARGIN * SPEED
        } else {
            0.0
        }
    };
    (axis(pos.0, size.0), axis(pos.1, size.1))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_middle_of_the_window_pushes_the_view_nowhere() {
        assert_eq!(edge_push((400.0, 300.0), (800.0, 600.0)), (0.0, 0.0));
    }

    #[test]
    fn the_borders_keep_the_view_turning() {
        let size = (800.0, 600.0);
        let (left, _) = edge_push((2.0, 300.0), size);
        let (right, _) = edge_push((798.0, 300.0), size);
        assert!(left < -10.0 && right > 10.0, "{left} {right}");
        // It builds up across the margin rather than switching on at the edge.
        let (near, _) = edge_push((40.0, 300.0), size);
        assert!(near < 0.0 && near > left, "{near} should be gentler than {left}");

        let (_, up) = edge_push((400.0, 1.0), size);
        let (_, down) = edge_push((400.0, 599.0), size);
        assert!(up < 0.0 && down > 0.0);
    }
}
