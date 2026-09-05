//! Vector math. Written from scratch: the course forbids external crates.

use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub const fn vec3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3 { x, y, z }
}

impl Vec3 {
    pub const ZERO: Vec3 = vec3(0.0, 0.0, 0.0);
    pub const ONE: Vec3 = vec3(1.0, 1.0, 1.0);

    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        vec3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    pub fn normalized(self) -> Vec3 {
        let len = self.length();
        if len > 0.0 {
            self / len
        } else {
            Vec3::ZERO
        }
    }

    /// Component-wise product. Used constantly for tinting colors by albedo.
    pub fn mul_elem(self, o: Vec3) -> Vec3 {
        vec3(self.x * o.x, self.y * o.y, self.z * o.z)
    }

    /// Used by tests to compare brightness between shaded points.
    #[cfg(test)]
    pub fn max_component(self) -> f32 {
        self.x.max(self.y).max(self.z)
    }

    pub fn axis(self, axis: usize) -> f32 {
        match axis {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }

    pub fn lerp(self, o: Vec3, t: f32) -> Vec3 {
        self * (1.0 - t) + o * t
    }

    /// Mirror direction around a normal. `self` points at the surface.
    pub fn reflect(self, n: Vec3) -> Vec3 {
        self - n * (2.0 * self.dot(n))
    }

    /// Snell refraction. `eta` is the ratio n_from / n_into.
    /// Returns `None` on total internal reflection.
    pub fn refract(self, n: Vec3, eta: f32) -> Option<Vec3> {
        let cos_i = (-self).dot(n).clamp(-1.0, 1.0);
        let k = 1.0 - eta * eta * (1.0 - cos_i * cos_i);
        if k < 0.0 {
            None
        } else {
            Some(self * eta + n * (eta * cos_i - k.sqrt()))
        }
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        vec3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        vec3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f32) -> Vec3 {
        vec3(self.x * s, self.y * s, self.z * s)
    }
}

impl MulAssign<f32> for Vec3 {
    fn mul_assign(&mut self, s: f32) {
        *self = *self * s;
    }
}

impl Div<f32> for Vec3 {
    type Output = Vec3;
    fn div(self, s: f32) -> Vec3 {
        self * (1.0 / s)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        vec3(-self.x, -self.y, -self.z)
    }
}

/// Schlick's approximation of the Fresnel reflectance.
pub fn fresnel_schlick(cos_theta: f32, f0: f32) -> f32 {
    let m = (1.0 - cos_theta).clamp(0.0, 1.0);
    f0 + (1.0 - f0) * m.powi(5)
}

/// Linear color to sRGB-ish gamma, then to a packed 0RGB word for the framebuffer.
pub fn to_srgb_u32(c: Vec3) -> u32 {
    let enc = |v: f32| -> u32 {
        let v = v.clamp(0.0, 1.0).powf(1.0 / 2.2);
        (v * 255.0 + 0.5) as u32
    };
    (enc(c.x) << 16) | (enc(c.y) << 8) | enc(c.z)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn cross_is_right_handed() {
        let c = vec3(1.0, 0.0, 0.0).cross(vec3(0.0, 1.0, 0.0));
        assert_eq!(c, vec3(0.0, 0.0, 1.0));
    }

    #[test]
    fn normalized_has_unit_length() {
        let v = vec3(3.0, 4.0, 12.0).normalized();
        assert!(close(v.length(), 1.0));
    }

    #[test]
    fn reflect_flips_the_normal_component() {
        // Straight down onto a floor comes back straight up.
        let r = vec3(0.0, -1.0, 0.0).reflect(vec3(0.0, 1.0, 0.0));
        assert_eq!(r, vec3(0.0, 1.0, 0.0));
    }

    #[test]
    fn refract_straight_on_passes_through() {
        let d = vec3(0.0, -1.0, 0.0);
        let r = d.refract(vec3(0.0, 1.0, 0.0), 1.0 / 1.33).unwrap();
        assert!(close(r.x, 0.0) && close(r.z, 0.0) && r.y < 0.0);
    }

    #[test]
    fn refract_reports_total_internal_reflection() {
        // Leaving water at a grazing angle: past the critical angle, nothing exits.
        let d = vec3(0.999, -0.0447, 0.0).normalized();
        assert!(d.refract(vec3(0.0, 1.0, 0.0), 1.33).is_none());
    }

    #[test]
    fn fresnel_is_total_at_grazing_angle() {
        assert!(close(fresnel_schlick(0.0, 0.04), 1.0));
        assert!(close(fresnel_schlick(1.0, 0.04), 0.04));
    }
}
