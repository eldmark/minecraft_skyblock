//! Orbital camera: it always looks at the island, and the user spins around it.

use crate::math::{vec3, Vec3};
use crate::world::Ray;

pub struct Camera {
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
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + vec3(cp * sy, sp, cp * cy) * self.distance
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
        let forward = (self.target - eye).normalized();
        let world_up = vec3(0.0, 1.0, 0.0);
        let right = forward.cross(world_up).normalized();
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
