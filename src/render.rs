//! Shading and the frame pipeline.
//!
//! One camera ray per pixel: walk the grid, sample the block's texture, perturb the
//! face normal with the texture's normal map, then light it with a sun (shadowed),
//! a two-tone sky ambient, and the material's own emission.

use crate::blocks;
use crate::camera::Camera;
use crate::math::{to_srgb_u32, vec3, Vec3};
use crate::output::Framebuffer;
use crate::parallel::{render_strips, thread_count};
use crate::scene::Scene;
use crate::world::{Face, Hit, Ray, World};

/// Rows handed out per work item. Small enough to balance sky against island,
/// large enough that queue traffic stays negligible.
const ROWS_PER_STRIP: usize = 4;
/// Fully transparent texels are skipped this many times before a ray gives up, so
/// a glancing ray through leaf cutouts still finds the ground behind them.
const MAX_ALPHA_SKIPS: usize = 6;

pub struct Renderer {
    pub threads: usize,
    /// 1 = full resolution, 2 = half, and so on. Raised while the camera moves.
    pub scale: usize,
    low: Vec<u32>,
}

impl Default for Renderer {
    fn default() -> Renderer {
        Renderer::new()
    }
}

impl Renderer {
    pub fn new() -> Renderer {
        Renderer {
            threads: thread_count(),
            scale: 1,
            low: Vec::new(),
        }
    }

    pub fn render(&mut self, frame: &mut Framebuffer, scene: &Scene, camera: &Camera) {
        let scale = self.scale.max(1);
        let (w, h) = (frame.width, frame.height);
        let (lw, lh) = ((w / scale).max(1), (h / scale).max(1));

        if scale == 1 {
            render_into(&mut frame.pixels, w, h, scene, camera, self.threads);
            return;
        }

        self.low.resize(lw * lh, 0);
        render_into(&mut self.low, lw, lh, scene, camera, self.threads);

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
    scene: &Scene,
    camera: &Camera,
    threads: usize,
) {
    render_strips(pixels, width, ROWS_PER_STRIP, threads, |strip, first_row| {
        for (i, px) in strip.iter_mut().enumerate() {
            let y = first_row + i / width;
            let x = i % width;
            let ray = camera.ray(x, y, width, height, (0.5, 0.5));
            *px = to_srgb_u32(trace_color(scene, &ray));
        }
    });
}

/// First visible surface along a ray, skipping texels the texture marks as fully
/// transparent (leaf cutouts) so foliage does not read as solid cubes.
fn first_visible_hit(scene: &Scene, ray: &Ray) -> Option<(Hit, Vec3)> {
    let world = scene.world();
    let mut origin = ray.origin;
    for _ in 0..MAX_ALPHA_SKIPS {
        let probe = Ray {
            origin,
            dir: ray.dir,
        };
        let hit = world.trace(&probe, 1000.0, |_| true)?;
        let frame = scene.animation_frame(scene.assets.frame_count(hit.block, hit.face));
        let (color, alpha) = scene.assets.sample(hit.block, hit.face, hit.u, hit.v, frame);
        if alpha >= 0.5 {
            return Some((hit, color));
        }
        // Step just past this texel and keep going.
        origin = hit.point + ray.dir * 1e-3;
    }
    None
}

/// Shadow factor in `[0, 1]`: 1 in full sun, lower behind transparent blockers.
fn sun_visibility(scene: &Scene, point: Vec3, normal: Vec3) -> f32 {
    let world: &World = scene.world();
    let mut origin = point + normal * 1e-3;
    let mut transmission = 1.0f32;
    for _ in 0..MAX_ALPHA_SKIPS {
        let probe = Ray::new(origin, scene.sun_dir);
        let Some(hit) = world.trace(&probe, 200.0, |b| b != blocks::AIR) else {
            break;
        };
        let material = scene.assets.material(hit.block);
        if material.transparency <= 0.001 {
            return 0.0;
        }
        // Water and glass dim the light instead of blocking it outright.
        transmission *= material.transparency * 0.9;
        if transmission < 0.02 {
            return 0.0;
        }
        origin = hit.point + probe.dir * 1e-3;
    }
    transmission
}

/// Tangent frame matching the UV layout in `world::face_uv`.
fn face_tangents(face: Face) -> (Vec3, Vec3) {
    match face {
        Face::PosX => (vec3(0.0, 0.0, -1.0), vec3(0.0, -1.0, 0.0)),
        Face::NegX => (vec3(0.0, 0.0, 1.0), vec3(0.0, -1.0, 0.0)),
        Face::PosY => (vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0)),
        Face::NegY => (vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, -1.0)),
        Face::PosZ => (vec3(1.0, 0.0, 0.0), vec3(0.0, -1.0, 0.0)),
        Face::NegZ => (vec3(-1.0, 0.0, 0.0), vec3(0.0, -1.0, 0.0)),
    }
}

/// Perturb a face normal with the texture-derived normal map.
fn shaded_normal(scene: &Scene, hit: &Hit, frame: usize) -> Vec3 {
    let strength = scene.assets.material(hit.block).normal_strength;
    if strength <= 0.0 {
        return hit.normal;
    }
    let n = scene
        .assets
        .sample_normal(hit.block, hit.face, hit.u, hit.v, frame);
    let (tangent, bitangent) = face_tangents(hit.face);
    (tangent * n.x + bitangent * n.y + hit.normal * n.z).normalized()
}

/// Blinn-Phong direct lighting plus a hemispherical ambient term.
fn direct_light(scene: &Scene, hit: &Hit, normal: Vec3, albedo: Vec3, view: Vec3) -> Vec3 {
    let material = scene.assets.material(hit.block);
    let shadow = sun_visibility(scene, hit.point, hit.normal);

    let n_dot_l = normal.dot(scene.sun_dir).max(0.0);
    let diffuse = albedo.mul_elem(scene.sun_color) * (n_dot_l * shadow);

    // Half-vector form: cheaper than reflecting the view, and just as convincing.
    let half = (scene.sun_dir - view).normalized();
    let spec_angle = normal.dot(half).max(0.0);
    let lit = if n_dot_l > 0.0 { 1.0 } else { 0.0 };
    let specular =
        scene.sun_color * (material.specular * spec_angle.powf(material.shininess) * shadow * lit);

    // Sky above, bounced ground light below: cheap fill that keeps shadows readable.
    let up = normal.y * 0.5 + 0.5;
    let ambient = albedo.mul_elem(scene.ground_color.lerp(scene.sky_color, up)) * 0.55;

    diffuse + specular + ambient + albedo * material.emission
}

pub fn trace_color(scene: &Scene, ray: &Ray) -> Vec3 {
    match first_visible_hit(scene, ray) {
        Some((hit, albedo)) => {
            let frame = scene.animation_frame(scene.assets.frame_count(hit.block, hit.face));
            let normal = shaded_normal(scene, &hit, frame);
            direct_light(scene, &hit, normal, albedo, ray.dir)
        }
        None => scene.skybox.sample(ray.dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::Pack;
    use std::path::Path;

    /// The pack is not committed, so these tests skip when it is absent.
    fn scene() -> Option<Scene> {
        if !Path::new("texturepack").is_dir() {
            return None;
        }
        Scene::load(2024, &Pack::open(None).ok()?, false).ok()
    }

    #[test]
    fn rendering_is_deterministic_across_thread_counts() {
        let Some(scene) = scene() else { return };
        let camera = Camera::new(vec3(16.0, 22.0, 16.0), 60.0);
        let mut a = Framebuffer::new(53, 31);
        let mut b = Framebuffer::new(53, 31);

        let mut r1 = Renderer::new();
        r1.threads = 1;
        r1.render(&mut a, &scene, &camera);
        let mut r2 = Renderer::new();
        r2.threads = 16;
        r2.render(&mut b, &scene, &camera);

        assert_eq!(a.pixels, b.pixels);
    }

    #[test]
    fn the_island_renders_with_texture_detail() {
        let Some(scene) = scene() else { return };
        let camera = Camera::new(vec3(16.0, 22.0, 16.0), 60.0);
        let mut frame = Framebuffer::new(120, 90);
        Renderer::new().render(&mut frame, &scene, &camera);

        let unique: std::collections::HashSet<u32> = frame.pixels.iter().copied().collect();
        assert!(unique.len() > 200, "image has only {} colors", unique.len());
    }

    #[test]
    fn the_sunlit_top_is_brighter_than_the_shadowed_underside() {
        let Some(scene) = scene() else { return };
        let top = Ray::new(vec3(16.0, 60.0, 16.0), vec3(0.0, -1.0, 0.0));
        let bottom = Ray::new(vec3(16.0, -10.0, 16.0), vec3(0.0, 1.0, 0.0));
        let lit = trace_color(&scene, &top).max_component();
        let unlit = trace_color(&scene, &bottom).max_component();
        assert!(lit > unlit, "lit {lit} should exceed shadowed {unlit}");
    }

    #[test]
    fn scaled_rendering_still_fills_the_framebuffer() {
        let Some(scene) = scene() else { return };
        let camera = Camera::new(vec3(16.0, 22.0, 16.0), 60.0);
        let mut frame = Framebuffer::new(64, 48);
        let mut r = Renderer::new();
        r.scale = 3;
        r.render(&mut frame, &scene, &camera);
        assert!(frame.pixels.iter().any(|&p| p != frame.pixels[0]));
    }
}
