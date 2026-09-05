//! Textures: linear-light color, alpha, and a normal map derived at load time.
//!
//! Minecraft packs store animations as a vertical strip of square frames, so a
//! texture whose height is a multiple of its width is treated as `n` frames.

use crate::math::{vec3, Vec3};
use crate::png::Image;

/// sRGB byte to linear float. Lighting must be done in linear space.
fn srgb_to_linear(byte: u8) -> f32 {
    let c = byte as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub struct Texture {
    pub width: usize,
    pub height: usize,
    pub frames: usize,
    /// Linear RGB, `width * height * frames` entries, frame-major.
    albedo: Vec<Vec3>,
    alpha: Vec<f32>,
    /// Tangent-space normals derived from the texture's own luminance.
    normal: Vec<Vec3>,
}

impl Texture {
    pub fn from_image(image: &Image, normal_strength: f32) -> Texture {
        let width = image.width;
        let frames = if image.height >= width && image.height % width == 0 {
            image.height / width
        } else {
            1
        };
        let height = image.height / frames;

        let count = image.width * image.height;
        let mut albedo = Vec::with_capacity(count);
        let mut alpha = Vec::with_capacity(count);
        for i in 0..count {
            let p = &image.rgba[i * 4..i * 4 + 4];
            albedo.push(vec3(
                srgb_to_linear(p[0]),
                srgb_to_linear(p[1]),
                srgb_to_linear(p[2]),
            ));
            alpha.push(p[3] as f32 / 255.0);
        }

        let normal = derive_normals(&albedo, width, height, frames, normal_strength);
        Texture {
            width,
            height,
            frames,
            albedo,
            alpha,
            normal,
        }
    }

    fn index(&self, u: f32, v: f32, frame: usize) -> usize {
        // Nearest sampling on purpose: this is pixel art, bilinear would smear it.
        let x = ((u.rem_euclid(1.0)) * self.width as f32) as usize % self.width;
        let y = ((v.rem_euclid(1.0)) * self.height as f32) as usize % self.height;
        let frame = frame % self.frames;
        (frame * self.height + y) * self.width + x
    }

    pub fn sample(&self, u: f32, v: f32, frame: usize) -> Vec3 {
        self.albedo[self.index(u, v, frame)]
    }

    pub fn sample_alpha(&self, u: f32, v: f32, frame: usize) -> f32 {
        self.alpha[self.index(u, v, frame)]
    }

    /// Tangent-space normal: x along +u, y along +v, z out of the surface.
    pub fn sample_normal(&self, u: f32, v: f32, frame: usize) -> Vec3 {
        self.normal[self.index(u, v, frame)]
    }

    /// Average color, used for far-away blocks and for debugging.
    pub fn average(&self) -> Vec3 {
        let sum = self
            .albedo
            .iter()
            .fold(Vec3::ZERO, |acc, &c| acc + c);
        sum / self.albedo.len().max(1) as f32
    }
}

/// Perceived brightness, the height field a normal map is built from.
fn luminance(c: Vec3) -> f32 {
    0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z
}

/// Sobel gradient of the luminance, per frame, turned into a unit normal.
/// Bright pixels read as raised, so mortar lines in stone become grooves.
fn derive_normals(
    albedo: &[Vec3],
    width: usize,
    height: usize,
    frames: usize,
    strength: f32,
) -> Vec<Vec3> {
    let mut out = vec![vec3(0.0, 0.0, 1.0); albedo.len()];
    if strength <= 0.0 {
        return out;
    }
    for frame in 0..frames {
        let base = frame * width * height;
        // Wrapping at the edges keeps tiled blocks seamless.
        let height_at = |x: isize, y: isize| -> f32 {
            let xi = x.rem_euclid(width as isize) as usize;
            let yi = y.rem_euclid(height as isize) as usize;
            luminance(albedo[base + yi * width + xi])
        };
        for y in 0..height as isize {
            for x in 0..width as isize {
                let dx = (height_at(x + 1, y - 1) + 2.0 * height_at(x + 1, y) + height_at(x + 1, y + 1))
                    - (height_at(x - 1, y - 1) + 2.0 * height_at(x - 1, y) + height_at(x - 1, y + 1));
                let dy = (height_at(x - 1, y + 1) + 2.0 * height_at(x, y + 1) + height_at(x + 1, y + 1))
                    - (height_at(x - 1, y - 1) + 2.0 * height_at(x, y - 1) + height_at(x + 1, y - 1));
                out[base + y as usize * width + x as usize] =
                    vec3(-dx * strength, -dy * strength, 1.0).normalized();
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: usize, height: usize, color: [u8; 4]) -> Image {
        Image {
            width,
            height,
            rgba: color.iter().cloned().cycle().take(width * height * 4).collect(),
        }
    }

    #[test]
    fn detects_animation_frames_from_a_vertical_strip() {
        let tex = Texture::from_image(&solid(32, 1024, [255, 255, 255, 255]), 0.0);
        assert_eq!((tex.width, tex.height, tex.frames), (32, 32, 32));
    }

    #[test]
    fn treats_a_square_image_as_one_frame() {
        let tex = Texture::from_image(&solid(32, 32, [128, 128, 128, 255]), 0.0);
        assert_eq!(tex.frames, 1);
    }

    #[test]
    fn a_flat_texture_yields_flat_normals() {
        let tex = Texture::from_image(&solid(8, 8, [200, 100, 50, 255]), 4.0);
        assert_eq!(tex.sample_normal(0.5, 0.5, 0), vec3(0.0, 0.0, 1.0));
    }

    #[test]
    fn a_vertical_edge_tilts_the_normal_horizontally() {
        // Left half black, right half white: the gradient runs along +u only.
        let mut img = solid(8, 8, [0, 0, 0, 255]);
        for y in 0..8 {
            for x in 4..8 {
                let i = (y * 8 + x) * 4;
                img.rgba[i..i + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        let tex = Texture::from_image(&img, 4.0);
        let n = tex.sample_normal(3.5 / 8.0, 0.5, 0);
        assert!(n.x < -0.1, "normal should tilt across the edge, got {n:?}");
        assert!(n.y.abs() < 1e-4);
    }

    #[test]
    fn srgb_decoding_is_monotonic_and_anchored() {
        assert!((srgb_to_linear(0) - 0.0).abs() < 1e-6);
        assert!((srgb_to_linear(255) - 1.0).abs() < 1e-6);
        assert!(srgb_to_linear(128) < 0.5, "mid gray is darker in linear light");
    }
}
