//! Turning a camera and a world into pixels.
//!
//! Shading is still a placeholder (flat colors, one directional light); what this
//! module settles is the frame pipeline: render at a chosen resolution scale, in
//! parallel, then upscale into the framebuffer.

use crate::camera::Camera;
use crate::math::{to_srgb_u32, vec3, Vec3};
use crate::output::Framebuffer;
use crate::parallel::{render_strips, thread_count};
use crate::world::{BlockId, Ray, World};

/// Rows handed out per work item. Small enough to balance sky against island,
/// large enough that queue traffic stays negligible.
const ROWS_PER_STRIP: usize = 4;

pub struct Renderer {
    pub threads: usize,
    /// 1 = full resolution, 2 = half, and so on. Raised while the camera moves.
    pub scale: usize,
    low: Vec<u32>,
}

impl Renderer {
    pub fn new() -> Renderer {
        Renderer {
            threads: thread_count(),
            scale: 1,
            low: Vec::new(),
        }
    }

    pub fn render(&mut self, frame: &mut Framebuffer, world: &World, camera: &Camera) {
        let scale = self.scale.max(1);
        let (w, h) = (frame.width, frame.height);
        let (lw, lh) = ((w / scale).max(1), (h / scale).max(1));

        if scale == 1 {
            render_into(&mut frame.pixels, w, h, world, camera, self.threads);
            return;
        }

        self.low.resize(lw * lh, 0);
        render_into(&mut self.low, lw, lh, world, camera, self.threads);

        // Nearest upscale. Blocky while dragging, replaced the moment the camera
        // settles, and far cheaper than tracing every pixel.
        for y in 0..h {
            let sy = (y * lh / h).min(lh - 1);
            for x in 0..w {
                let sx = (x * lw / w).min(lw - 1);
                frame.pixels[y * w + x] = self.low[sy * lw + sx];
            }
        }
    }
}

fn render_into(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    world: &World,
    camera: &Camera,
    threads: usize,
) {
    render_strips(pixels, width, ROWS_PER_STRIP, threads, |strip, first_row| {
        for (i, px) in strip.iter_mut().enumerate() {
            let y = first_row + i / width;
            let x = i % width;
            let ray = camera.ray(x, y, width, height, (0.5, 0.5));
            *px = to_srgb_u32(shade(world, &ray));
        }
    });
}

/// Stand-in colors until textures are wired to blocks in the next phase.
fn debug_color(block: BlockId) -> Vec3 {
    use crate::blocks::*;
    match block {
        GRASS => vec3(0.28, 0.52, 0.18),
        DIRT => vec3(0.36, 0.26, 0.18),
        STONE => vec3(0.42, 0.42, 0.44),
        COBBLESTONE => vec3(0.36, 0.36, 0.38),
        SAND => vec3(0.76, 0.70, 0.50),
        WATER => vec3(0.15, 0.35, 0.75),
        OAK_LOG => vec3(0.35, 0.26, 0.14),
        OAK_LEAVES => vec3(0.16, 0.38, 0.12),
        OAK_PLANKS => vec3(0.55, 0.42, 0.24),
        GOLD_ORE => vec3(0.55, 0.45, 0.20),
        IRON_ORE => vec3(0.52, 0.44, 0.38),
        DIAMOND_ORE => vec3(0.35, 0.60, 0.62),
        GOLD_BLOCK => vec3(0.90, 0.72, 0.22),
        IRON_BLOCK => vec3(0.78, 0.78, 0.80),
        DIAMOND_BLOCK => vec3(0.40, 0.85, 0.85),
        QUARTZ | QUARTZ_PILLAR => vec3(0.90, 0.88, 0.84),
        GLOWSTONE => vec3(1.00, 0.85, 0.45),
        GLASS => vec3(0.80, 0.86, 0.88),
        PORTAL => vec3(0.55, 0.70, 1.00),
        _ => vec3(0.62, 0.62, 0.66),
    }
}

pub fn sky_color(dir: Vec3) -> Vec3 {
    let t = (dir.y * 0.5 + 0.5).clamp(0.0, 1.0);
    vec3(0.94, 0.62, 0.42).lerp(vec3(0.10, 0.16, 0.38), t.powf(0.7))
}

fn shade(world: &World, ray: &Ray) -> Vec3 {
    let sun = vec3(0.45, 0.8, 0.3).normalized();
    match world.trace(ray, 1000.0, |_| true) {
        Some(hit) => {
            let diffuse = hit.normal.dot(sun).max(0.0);
            let ambient = 0.25 + 0.15 * hit.normal.y;
            debug_color(hit.block) * (ambient + diffuse * 0.85)
        }
        None => sky_color(ray.dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::vec3;

    fn tiny_world() -> World {
        let mut w = World::new([8, 8, 8]);
        for x in 0..8 {
            for z in 0..8 {
                w.set(x, 2, z, 1);
            }
        }
        w
    }

    #[test]
    fn scaled_and_full_renders_agree_on_the_sky() {
        let world = tiny_world();
        let camera = Camera::new(vec3(4.0, 2.0, 4.0), 20.0);
        let mut full = Framebuffer::new(64, 48);
        let mut half = Framebuffer::new(64, 48);

        let mut r = Renderer::new();
        r.scale = 1;
        r.render(&mut full, &world, &camera);
        r.scale = 2;
        r.render(&mut half, &world, &camera);

        // A scaled pass must still fill the whole framebuffer, and the corner
        // (sky in both) must land within a shade of the full-resolution pass:
        // exact equality is not expected, the low-res ray points elsewhere.
        assert_eq!(half.pixels.len(), full.pixels.len());
        let channel = |p: u32, shift: u32| ((p >> shift) & 0xFF) as i32;
        for shift in [16, 8, 0] {
            let delta = (channel(half.pixels[0], shift) - channel(full.pixels[0], shift)).abs();
            assert!(delta <= 4, "corner differs by {delta} in channel {shift}");
        }
        assert!(half.pixels.iter().any(|&p| p != half.pixels[0]), "image is flat");
    }

    #[test]
    fn rendering_is_deterministic_across_thread_counts() {
        let world = tiny_world();
        let camera = Camera::new(vec3(4.0, 2.0, 4.0), 20.0);
        let mut a = Framebuffer::new(53, 31);
        let mut b = Framebuffer::new(53, 31);

        let mut r1 = Renderer::new();
        r1.threads = 1;
        r1.render(&mut a, &world, &camera);
        let mut r2 = Renderer::new();
        r2.threads = 16;
        r2.render(&mut b, &world, &camera);

        assert_eq!(a.pixels, b.pixels);
    }
}
