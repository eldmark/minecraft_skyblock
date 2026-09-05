//! The camera, in two modes.
//!
//! **Orbit** keeps the island in the middle and spins around it — the right
//! control for looking at a diorama. **Free** flies: the same yaw and pitch, but
//! the eye goes wherever it is pushed, so one can walk the bridge, duck under the
//! island to find the ore, or put the head inside the crystal gate.
//!
//! Both modes share `yaw` and `pitch` and agree on what they mean, so switching
//! between them never moves the picture.

use crate::math::{vec3, Vec3};
use crate::world::Ray;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Orbit,
    Free,
}

pub struct Camera {
    pub mode: Mode,
    /// Where the eye sits in free mode. Ignored while orbiting.
    pub position: Vec3,
    /// How fast free flight moves, in blocks per second.
    pub speed: f32,
    pub target: Vec3,
    /// Radians. Yaw spins around the world's up axis, pitch lifts the eye.
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
    pub min_distance: f32,
    pub max_distance: f32,
}

impl Camera {
    pub fn new(target: Vec3, distance: f32) -> Camera {
        Camera {
            mode: Mode::Orbit,
            position: Vec3::ZERO,
            speed: 14.0,
            target,
            yaw: 0.9,
            pitch: 0.55,
            distance,
            fov_y: 0.6,
            min_distance: 4.0,
            max_distance: 220.0,
        }
    }

    pub fn eye(&self) -> Vec3 {
        match self.mode {
            Mode::Orbit => self.target + self.offset() * self.distance,
            Mode::Free => self.position,
        }
    }

    /// Unit vector from the target towards the eye, for the current yaw and pitch.
    fn offset(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        vec3(cp * sy, sp, cp * cy)
    }

    /// Where the camera looks. Both modes read yaw and pitch the same way, which
    /// is what makes the toggle seamless.
    pub fn forward(&self) -> Vec3 {
        -self.offset()
    }

    /// Screen right, always horizontal, so strafing never rolls the view.
    pub fn right(&self) -> Vec3 {
        self.forward().cross(vec3(0.0, 1.0, 0.0)).normalized()
    }

    /// Switch modes without the picture jumping.
    pub fn set_mode(&mut self, mode: Mode) {
        if mode == self.mode {
            return;
        }
        match mode {
            // Free flight starts exactly where the orbit left the eye.
            Mode::Free => self.position = self.eye(),
            // Orbiting resumes around whatever the camera is looking at, keeping
            // the current distance so the island does not jump closer or further.
            Mode::Orbit => self.target = self.position + self.forward() * self.distance,
        }
        self.mode = mode;
    }

    /// Turn in place. Used by free mode, where there is nothing to orbit around.
    pub fn look(&mut self, delta: (f32, f32)) {
        const LOOK_SPEED: f32 = 0.006;
        self.yaw -= delta.0 * LOOK_SPEED;
        self.pitch = (self.pitch - delta.1 * LOOK_SPEED).clamp(-1.45, 1.45);
    }

    /// Fly. `axes` is (forward, right, up) in `[-1, 1]`, `dt` in seconds.
    pub fn fly(&mut self, axes: (f32, f32, f32), dt: f32, boost: bool) {
        if axes == (0.0, 0.0, 0.0) {
            return;
        }
        let step = self.speed * dt * if boost { 3.0 } else { 1.0 };
        let motion = self.forward() * axes.0 + self.right() * axes.1 + vec3(0.0, axes.2, 0.0);
        self.position += motion * step;
        // A soft leash: far enough to circle the island from any side, close
        // enough that one cannot fly off and lose it.
        const REACH: f32 = 200.0;
        let centre = vec3(24.0, 30.0, 24.0);
        let away = self.position - centre;
        if away.length() > REACH {
            self.position = centre + away.normalized() * REACH;
        }
    }

    /// Orbit by screen-space deltas (pixels), zoom by wheel notches.
    pub fn apply(&mut self, orbit: (f32, f32), zoom: f32) {
        const ORBIT_SPEED: f32 = 0.006;
        self.yaw -= orbit.0 * ORBIT_SPEED;
        self.pitch = (self.pitch - orbit.1 * ORBIT_SPEED).clamp(-1.45, 1.45);
        if zoom != 0.0 {
            // Multiplicative zoom: constant feel at every distance.
            self.distance = (self.distance * (1.0 + zoom * 0.08))
                .clamp(self.min_distance, self.max_distance);
        }
    }

    /// Ray through pixel `(x, y)` with a sub-pixel offset in `[0, 1)`.
    pub fn ray(&self, x: usize, y: usize, width: usize, height: usize, jitter: (f32, f32)) -> Ray {
        let eye = self.eye();
        let forward = self.forward();
        let right = self.right();
        let up = right.cross(forward);

        let aspect = width as f32 / height.max(1) as f32;
        let half_h = (self.fov_y * 0.5).tan();
        let half_w = half_h * aspect;

        // Screen y grows downward, world up grows upward: hence the minus.
        let sx = ((x as f32 + jitter.0) / width as f32 * 2.0 - 1.0) * half_w;
        let sy = -((y as f32 + jitter.1) / height as f32 * 2.0 - 1.0) * half_h;

        Ray::new(eye, forward + right * sx + up * sy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Camera {
        Camera::new(vec3(0.0, 0.0, 0.0), 10.0)
    }

    #[test]
    fn eye_keeps_its_distance_from_the_target() {
        let c = cam();
        assert!(((c.eye() - c.target).length() - 10.0).abs() < 1e-4);
    }

    #[test]
    fn the_center_ray_points_at_the_target() {
        let c = cam();
        let ray = c.ray(50, 50, 101, 101, (0.5, 0.5));
        let to_target = (c.target - ray.origin).normalized();
        assert!(ray.dir.dot(to_target) > 0.9999);
    }

    #[test]
    fn pitch_is_clamped_short_of_the_poles() {
        let mut c = cam();
        c.apply((0.0, -100000.0), 0.0);
        assert!(c.pitch < 1.5);
        c.apply((0.0, 100000.0), 0.0);
        assert!(c.pitch > -1.5);
    }

    #[test]
    fn zoom_stays_within_its_limits() {
        let mut c = cam();
        for _ in 0..500 {
            c.apply((0.0, 0.0), -1.0);
        }
        assert!((c.distance - c.min_distance).abs() < 1e-3);
        for _ in 0..500 {
            c.apply((0.0, 0.0), 1.0);
        }
        assert!((c.distance - c.max_distance).abs() < 1e-3);
    }

    #[test]
    fn switching_to_free_flight_keeps_the_view() {
        let mut c = cam();
        let (eye, forward) = (c.eye(), c.forward());
        c.set_mode(Mode::Free);
        assert!((c.eye() - eye).length() < 1e-4);
        assert!((c.forward() - forward).length() < 1e-4);
    }

    #[test]
    fn switching_back_to_orbit_keeps_the_view_and_the_distance() {
        let mut c = cam();
        c.set_mode(Mode::Free);
        c.fly((1.0, 0.0, 0.0), 1.0, false);
        c.look((30.0, 10.0));
        let (eye, forward) = (c.eye(), c.forward());
        c.set_mode(Mode::Orbit);
        assert!((c.eye() - eye).length() < 1e-3, "the eye moved on switching back");
        assert!((c.forward() - forward).length() < 1e-3);
    }

    #[test]
    fn flying_forward_follows_the_view() {
        let mut c = cam();
        c.set_mode(Mode::Free);
        let before = c.eye();
        let forward = c.forward();
        c.fly((1.0, 0.0, 0.0), 0.5, false);
        let moved = c.eye() - before;
        assert!(moved.length() > 0.1);
        assert!(moved.normalized().dot(forward) > 0.999, "flight went sideways");
    }

    #[test]
    fn strafing_is_horizontal_and_perpendicular_to_the_view() {
        let mut c = cam();
        c.set_mode(Mode::Free);
        let before = c.eye();
        c.fly((0.0, 1.0, 0.0), 0.5, false);
        let moved = c.eye() - before;
        assert!(moved.y.abs() < 1e-4, "strafing should not change height");
        assert!(moved.normalized().dot(c.forward()).abs() < 1e-3);
    }

    #[test]
    fn boost_moves_further_in_the_same_time() {
        let mut slow = cam();
        slow.set_mode(Mode::Free);
        let mut fast = cam();
        fast.set_mode(Mode::Free);
        let start = slow.eye();
        slow.fly((1.0, 0.0, 0.0), 0.2, false);
        fast.fly((1.0, 0.0, 0.0), 0.2, true);
        let (a, b) = (
            (slow.eye() - start).length(),
            (fast.eye() - start).length(),
        );
        assert!(b > a * 2.5, "boost travelled {b}, normal {a}");
    }

    #[test]
    fn free_flight_is_leashed_to_the_island() {
        let mut c = cam();
        c.set_mode(Mode::Free);
        for _ in 0..500 {
            c.fly((1.0, 0.0, 0.0), 1.0, true);
        }
        assert!(
            (c.eye() - vec3(24.0, 30.0, 24.0)).length() <= 200.1,
            "the camera flew away"
        );
    }

    #[test]
    fn looking_around_clamps_the_pitch() {
        let mut c = cam();
        c.set_mode(Mode::Free);
        c.look((0.0, -100000.0));
        assert!(c.pitch < 1.5);
        c.look((0.0, 100000.0));
        assert!(c.pitch > -1.5);
    }

    #[test]
    fn horizontal_pixels_map_left_to_right() {
        let c = cam();
        let left = c.ray(0, 50, 101, 101, (0.5, 0.5));
        let right = c.ray(100, 50, 101, 101, (0.5, 0.5));
        let eye = c.eye();
        let forward = (c.target - eye).normalized();
        let screen_right = forward.cross(vec3(0.0, 1.0, 0.0)).normalized();
        assert!(left.dir.dot(screen_right) < right.dir.dot(screen_right));
    }
}
