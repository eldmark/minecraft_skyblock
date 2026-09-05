//! Material parameters, one set per block type.
//!
//! Kept separate from the texture atlas: a material says how a surface answers to
//! light, the atlas says what color it starts from.

use crate::math::{vec3, Vec3};

#[derive(Clone, Copy, Debug)]
pub struct Material {
    /// Multiplies the sampled texture. Grayscale pack textures (grass, leaves)
    /// are tinted here, exactly as Minecraft does at runtime.
    pub tint: Vec3,
    /// Blinn-Phong specular strength and exponent.
    pub specular: f32,
    pub shininess: f32,
    /// Mirror term, mixed in with Fresnel.
    pub reflectivity: f32,
    /// 0 = opaque, 1 = invisible. Drives the refracted ray's weight.
    pub transparency: f32,
    /// Index of refraction; 1.0 means the ray passes straight through.
    pub ior: f32,
    /// Light emitted by the surface itself, in linear units.
    pub emission: f32,
    /// How strongly the derived normal map perturbs the face normal.
    pub normal_strength: f32,
}

impl Default for Material {
    fn default() -> Material {
        Material {
            tint: Vec3::ONE,
            specular: 0.05,
            shininess: 8.0,
            reflectivity: 0.0,
            transparency: 0.0,
            ior: 1.0,
            emission: 0.0,
            normal_strength: 0.0,
        }
    }
}

impl Material {
    /// Rough, matte surfaces: soil, stone, wood.
    pub fn diffuse(normal_strength: f32) -> Material {
        Material {
            specular: 0.04,
            shininess: 6.0,
            normal_strength,
            ..Material::default()
        }
    }

    /// Polished metal: strong narrow highlight and a real mirror term.
    pub fn metal(tint: Vec3, reflectivity: f32) -> Material {
        Material {
            tint,
            specular: 0.9,
            shininess: 96.0,
            reflectivity,
            normal_strength: 1.5,
            ..Material::default()
        }
    }

    /// Transparent, refracting media: water, glass, the portal.
    pub fn refractive(transparency: f32, ior: f32, reflectivity: f32) -> Material {
        Material {
            specular: 0.6,
            shininess: 64.0,
            reflectivity,
            transparency,
            ior,
            normal_strength: 0.8,
            ..Material::default()
        }
    }

    pub fn with_tint(mut self, tint: Vec3) -> Material {
        self.tint = tint;
        self
    }

    pub fn with_emission(mut self, emission: f32) -> Material {
        self.emission = emission;
        self
    }

    pub fn is_transparent(&self) -> bool {
        self.transparency > 0.001
    }
}

/// Reflectance at normal incidence for a dielectric with the given IOR, the `f0`
/// that Schlick's approximation needs.
pub fn f0_from_ior(ior: f32) -> f32 {
    let r = (ior - 1.0) / (ior + 1.0);
    r * r
}

/// A recognisable stand-in when a texture is missing from the pack.
pub fn missing_color(u: f32, v: f32) -> Vec3 {
    let checker = ((u * 4.0) as i32 + (v * 4.0) as i32) % 2 == 0;
    if checker {
        vec3(0.8, 0.0, 0.8)
    } else {
        vec3(0.05, 0.05, 0.05)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_and_glass_have_plausible_f0() {
        // Water reflects about 2% head-on, common glass about 4%.
        assert!((f0_from_ior(1.33) - 0.02).abs() < 0.005);
        assert!((f0_from_ior(1.52) - 0.04).abs() < 0.005);
    }

    #[test]
    fn constructors_set_the_traits_they_promise() {
        assert!(Material::metal(Vec3::ONE, 0.5).specular > 0.5);
        assert!(Material::refractive(0.8, 1.33, 0.1).is_transparent());
        assert!(!Material::diffuse(1.0).is_transparent());
        assert_eq!(Material::default().tint, Vec3::ONE);
        assert!(Material::diffuse(0.0).with_emission(3.0).emission > 0.0);
    }
}
