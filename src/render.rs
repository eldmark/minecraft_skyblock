//! Shading and the frame pipeline.
//!
//! One camera ray per pixel: walk the grid, sample the block's texture, perturb the
//! face normal with the texture's normal map, then light it with a sun (shadowed),
//! a two-tone sky ambient, and the material's own emission.

use crate::blocks;
use crate::camera::Camera;
use crate::material::{f0_from_ior, Material};
use crate::math::{fresnel_schlick, to_srgb_u32, vec3, Vec3};
use crate::output::Framebuffer;
use crate::parallel::{render_strips, thread_count};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use crate::scene::Scene;
use crate::world::{Face, Hit, Ray, World};

/// Rows handed out per work item. Small enough to balance sky against island,
/// large enough that queue traffic stays negligible.
const ROWS_PER_STRIP: usize = 4;
/// Fully transparent texels are skipped this many times before a ray gives up, so
/// a glancing ray through leaf cutouts still finds the ground behind them.
const MAX_ALPHA_SKIPS: usize = 6;
/// Reflection and refraction recursion budget. Four is enough for water over
/// stone with the sky in it; deeper bounces cost frames and change nothing.
const MAX_DEPTH: usize = 4;
/// Past this depth a transparent surface follows only its dominant branch instead
/// of splitting into both. Splitting at every level doubles the ray count per
/// bounce, and by the second bounce the weaker branch is no longer visible.
const SPLIT_DEPTH: usize = 1;
/// A bounce contributing less than this is dropped.
const MIN_CONTRIBUTION: f32 = 0.015;
/// Point lights farther than this are ignored, and only the nearest few are shaded.
const LIGHT_RANGE: f32 = 15.0;
const MAX_LIGHTS_PER_HIT: usize = 3;
/// A lantern contributing less than this is skipped before its shadow ray.
const MIN_LIGHT_CONTRIBUTION: f32 = 0.012;

/// Jittered samples accumulated once the camera stops moving. Past this the image
/// stops changing visibly.
pub const MAX_SAMPLES: usize = 16;

pub struct Renderer {
    pub threads: usize,
    /// 1 = full resolution, 2 = half, and so on. Raised while the camera moves.
    pub scale: usize,
    low: Vec<u32>,
    /// Running sum of samples per pixel, for progressive refinement while idle.
    accumulator: Vec<Vec3>,
    pub samples: usize,
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
            accumulator: Vec::new(),
            samples: 0,
        }
    }

    /// Drop the accumulated samples: the image is about to change.
    pub fn reset_accumulation(&mut self) {
        self.samples = 0;
    }

    /// True once the accumulator has nothing left to add.
    pub fn is_converged(&self) -> bool {
        self.samples >= MAX_SAMPLES
    }

    /// Add one jittered sample per pixel and show the running average.
    ///
    /// Called only while the camera is still. Each pass offsets the ray inside the
    /// pixel with a different low-discrepancy offset, so edges resolve into smooth
    /// gradients without ever tracing more than one ray per pixel per frame.
    pub fn accumulate(&mut self, frame: &mut Framebuffer, scene: &Scene, camera: &Camera) {
        let (w, h) = (frame.width, frame.height);
        if self.accumulator.len() != w * h || self.samples == 0 {
            self.accumulator.clear();
            self.accumulator.resize(w * h, Vec3::ZERO);
            self.samples = 0;
        }
        if self.samples >= MAX_SAMPLES {
            return;
        }

        let jitter = halton_2d(self.samples + 1);
        let threads = self.threads;
        let previous = self.samples as f32;
        let inv = 1.0 / (previous + 1.0);

        // The accumulator is striped exactly like the framebuffer, so a worker owns
        // the same rows in both and no locking is needed.
        let accumulator = &mut self.accumulator;
        let mut pairs: Vec<(&mut [Vec3], &mut [u32])> = accumulator
            .chunks_mut(w * ROWS_PER_STRIP)
            .zip(frame.pixels.chunks_mut(w * ROWS_PER_STRIP))
            .collect();

        run_strips(&mut pairs, threads, |sums, pixels, first_row| {
            for (i, sum) in sums.iter_mut().enumerate() {
                let y = first_row + i / w;
                let x = i % w;
                let ray = camera.ray(x, y, w, h, jitter);
                *sum += trace_color(scene, &ray);
                pixels[i] = to_srgb_u32(*sum * inv);
            }
        });
        self.samples += 1;
    }

    pub fn render(&mut self, frame: &mut Framebuffer, scene: &Scene, camera: &Camera) {
        self.samples = 0;
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

/// Halton sequence in base 2 and 3: sample offsets that fill the pixel evenly
/// instead of clumping the way random offsets do at low counts.
fn halton_2d(index: usize) -> (f32, f32) {
    fn halton(mut i: usize, base: usize) -> f32 {
        let (mut f, mut result) = (1.0f32, 0.0f32);
        while i > 0 {
            f /= base as f32;
            result += f * (i % base) as f32;
            i /= base;
        }
        result
    }
    (halton(index, 2), halton(index, 3))
}

/// Hand paired strips (accumulator sums and framebuffer pixels) to the worker
/// pool. Same dynamic queue as `render_strips`, but carrying two buffers at once.
fn run_strips<F>(items: &mut [(&mut [Vec3], &mut [u32])], threads: usize, render: F)
where
    F: Fn(&mut [Vec3], &mut [u32], usize) + Sync,
{
    if threads <= 1 || items.len() <= 1 {
        for (index, (sums, pixels)) in items.iter_mut().enumerate() {
            render(sums, pixels, index * ROWS_PER_STRIP);
        }
        return;
    }

    let next = AtomicUsize::new(0);
    let count = items.len();
    let queue = Mutex::new(items);
    let render = &render;

    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= count {
                    break;
                }
                // Swap the slices out of the queue so the borrow travels with the
                // worker: each index is claimed once, so nothing is aliased.
                let (mut sums, mut pixels) = {
                    let mut guard = queue.lock().unwrap();
                    let (sums, pixels) = &mut guard[index];
                    (std::mem::take(sums), std::mem::take(pixels))
                };
                render(&mut sums, &mut pixels, index * ROWS_PER_STRIP);
            });
        }
    });
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

    // A surface turned away from the sun needs no shadow ray: the sun contributes
    // nothing to it either way, and that skips a full grid traversal.
    let n_dot_l = normal.dot(scene.sun_dir);
    let (diffuse, specular) = if n_dot_l > 0.0 {
        let shadow = sun_visibility(scene, hit.point, hit.normal);
        // Half-vector form: cheaper than reflecting the view, just as convincing.
        let half = (scene.sun_dir - view).normalized();
        let spec_angle = normal.dot(half).max(0.0);
        (
            albedo.mul_elem(scene.sun_color) * (n_dot_l * shadow),
            scene.sun_color * (material.specular * spec_angle.powf(material.shininess) * shadow),
        )
    } else {
        (Vec3::ZERO, Vec3::ZERO)
    };

    // Sky above, bounced ground light below: cheap fill that keeps shadows readable.
    let up = normal.y * 0.5 + 0.5;
    let ambient = albedo.mul_elem(scene.ground_color.lerp(scene.sky_color, up)) * 0.55;

    diffuse + specular + ambient + albedo * material.emission + point_lights(scene, hit, normal, albedo, view)
}

/// Light from emissive blocks (glowstone lanterns, the crystal gate).
///
/// Only the nearest few within range are shaded: each one costs a shadow ray, and
/// past three the contribution is lost in the ambient term anyway.
fn point_lights(scene: &Scene, hit: &Hit, normal: Vec3, albedo: Vec3, view: Vec3) -> Vec3 {
    let material = scene.assets.material(hit.block);
    let mut nearest: [(f32, usize); MAX_LIGHTS_PER_HIT] =
        [(f32::MAX, usize::MAX); MAX_LIGHTS_PER_HIT];

    for (i, (position, _)) in scene.lights.iter().enumerate() {
        let d2 = (*position - hit.point).length_squared();
        if d2 > LIGHT_RANGE * LIGHT_RANGE {
            continue;
        }
        // Insertion into a tiny sorted array: cheaper than sorting every light.
        if d2 < nearest[MAX_LIGHTS_PER_HIT - 1].0 {
            nearest[MAX_LIGHTS_PER_HIT - 1] = (d2, i);
            for k in (1..MAX_LIGHTS_PER_HIT).rev() {
                if nearest[k].0 < nearest[k - 1].0 {
                    nearest.swap(k, k - 1);
                }
            }
        }
    }

    let mut total = Vec3::ZERO;
    for (d2, index) in nearest {
        if index == usize::MAX {
            continue;
        }
        let (position, block) = scene.lights[index];
        let to_light = position - hit.point;
        let distance = d2.sqrt().max(1e-3);
        let dir = to_light / distance;
        let n_dot_l = normal.dot(dir);
        if n_dot_l <= 0.0 {
            continue;
        }

        let light_material = scene.assets.material(block);
        // Inverse-square falloff, softened near the source so lanterns do not blow
        // out the blocks they sit on.
        let attenuation = light_material.emission / (1.0 + 0.25 * d2);
        // Decide the contribution before paying for a shadow ray: a lantern whose
        // light would not register is not worth a grid traversal.
        if attenuation * n_dot_l < MIN_LIGHT_CONTRIBUTION {
            continue;
        }
        let color = vec3(1.0, 0.86, 0.62).mul_elem(light_material.tint);

        // Shadow ray stops short of the light block itself.
        let probe = Ray::new(hit.point + hit.normal * 1e-3, dir);
        let blocked = scene
            .world()
            .trace(&probe, distance - 0.75, |b| {
                b != blocks::AIR && !scene.assets.material(b).is_transparent()
            })
            .is_some();
        if blocked {
            continue;
        }

        let half = (dir - view).normalized();
        let spec = material.specular * normal.dot(half).max(0.0).powf(material.shininess);
        total += (albedo * n_dot_l + Vec3::ONE * spec).mul_elem(color) * attenuation;
    }
    total
}

pub fn trace_color(scene: &Scene, ray: &Ray) -> Vec3 {
    radiance(scene, ray, 0, 1.0)
}

/// Recursive shading. `weight` carries how much this ray still contributes to the
/// pixel, so faint bounces can be dropped without a visible difference.
fn radiance(scene: &Scene, ray: &Ray, depth: usize, weight: f32) -> Vec3 {
    let Some((hit, albedo)) = first_visible_hit(scene, ray) else {
        return scene.skybox.sample(ray.dir);
    };
    let frame = scene.animation_frame(scene.assets.frame_count(hit.block, hit.face));
    let normal = shaded_normal(scene, &hit, frame);
    let mut material = *scene.assets.material(hit.block);

    // The gate's smoke modulates both what it lets through and how hard it glows.
    let mut albedo = albedo;
    if scene.is_portal(hit.block) {
        let smoke = scene.portal_smoke(hit.point);
        albedo = albedo * smoke;
        material.emission *= smoke;
        material.transparency = (material.transparency / smoke.max(0.5)).clamp(0.25, 0.92);
    }
    let direct = direct_light(scene, &hit, normal, albedo, ray.dir);

    if depth >= MAX_DEPTH || weight < MIN_CONTRIBUTION {
        return direct;
    }
    if material.is_transparent() {
        transparent_shade(scene, ray, &hit, normal, albedo, &material, depth, weight)
    } else if material.reflectivity > 0.0 {
        metal_shade(scene, ray, &hit, normal, direct, &material, depth, weight)
    } else {
        direct
    }
}

/// Water, glass and the crystal gate: Fresnel splits the ray into a reflection and
/// a refraction, and the two are mixed with the surface's own shading.
#[allow(clippy::too_many_arguments)]
fn transparent_shade(
    scene: &Scene,
    ray: &Ray,
    hit: &Hit,
    normal: Vec3,
    albedo: Vec3,
    material: &Material,
    depth: usize,
    weight: f32,
) -> Vec3 {
    // A ray leaving the medium sees the flipped normal and the inverse index.
    let entering = ray.dir.dot(normal) < 0.0;
    let oriented = if entering { normal } else { -normal };
    let eta = if entering {
        1.0 / material.ior
    } else {
        material.ior
    };

    let cos_theta = (-ray.dir).dot(oriented).clamp(0.0, 1.0);
    let fresnel = fresnel_schlick(cos_theta, f0_from_ior(material.ior))
        .max(material.reflectivity * (1.0 - cos_theta));

    let reflect_weight = weight * fresnel;
    let refract_weight = weight * material.transparency * (1.0 - fresnel);
    let split = depth < SPLIT_DEPTH;

    let reflected = Ray::new(hit.point + oriented * 1e-3, ray.dir.reflect(oriented));
    let refracted = ray
        .dir
        .refract(oriented, eta)
        .map(|dir| Ray::new(hit.point - oriented * 1e-3, dir));

    // Total internal reflection: no transmitted ray exists, so all of it reflects.
    let (reflection, transmission) = match refracted {
        None => {
            let r = radiance(scene, &reflected, depth + 1, reflect_weight + refract_weight);
            (r, r)
        }
        Some(through) if split || reflect_weight >= refract_weight => {
            let r = radiance(scene, &reflected, depth + 1, reflect_weight);
            let t = if split {
                radiance(scene, &through, depth + 1, refract_weight).mul_elem(material.tint)
            } else {
                // Deep in the recursion the weaker branch reuses the stronger one's
                // color rather than tracing a second ray for it.
                r.mul_elem(material.tint)
            };
            (r, t)
        }
        Some(through) => {
            let t = radiance(scene, &through, depth + 1, refract_weight).mul_elem(material.tint);
            (t, t)
        }
    };

    let opacity = 1.0 - material.transparency;
    let surface = direct_light(scene, hit, normal, albedo, ray.dir) * opacity;
    surface
        + reflection * fresnel
        + transmission * (material.transparency * (1.0 - fresnel))
        + albedo * material.emission
}

/// Polished metal: one reflected ray, tinted by the metal and weighted by a
/// Fresnel curve anchored at the material's reflectivity.
#[allow(clippy::too_many_arguments)]
fn metal_shade(
    scene: &Scene,
    ray: &Ray,
    hit: &Hit,
    normal: Vec3,
    direct: Vec3,
    material: &Material,
    depth: usize,
    weight: f32,
) -> Vec3 {
    let cos_theta = (-ray.dir).dot(normal).clamp(0.0, 1.0);
    let strength = fresnel_schlick(cos_theta, material.reflectivity);
    if weight * strength < MIN_CONTRIBUTION {
        return direct;
    }
    let bounce = Ray::new(hit.point + normal * 1e-3, ray.dir.reflect(normal));
    let reflection = radiance(scene, &bounce, depth + 1, weight * strength).mul_elem(material.tint);
    direct * (1.0 - strength) + reflection * strength
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
    fn animated_textures_change_between_ticks() {
        let Some(mut scene) = scene() else { return };
        let camera = Camera::new(vec3(16.0, 24.0, 16.0), 45.0);
        let mut a = Framebuffer::new(96, 72);
        let mut b = Framebuffer::new(96, 72);
        Renderer::new().render(&mut a, &scene, &camera);
        // Far enough for the water strip to advance a frame and the smoke to drift.
        scene.tick += 30;
        Renderer::new().render(&mut b, &scene, &camera);
        let changed = a
            .pixels
            .iter()
            .zip(b.pixels.iter())
            .filter(|(p, q)| p != q)
            .count();
        assert!(changed > 20, "only {changed} pixels animated");
    }

    #[test]
    fn the_portal_smoke_varies_in_space_and_time() {
        let Some(mut scene) = scene() else { return };
        let p = vec3(16.3, 27.4, 15.6);
        let q = vec3(16.3, 28.9, 15.6);
        assert!((scene.portal_smoke(p) - scene.portal_smoke(q)).abs() > 0.01);
        let before = scene.portal_smoke(p);
        scene.tick += 60;
        assert!((scene.portal_smoke(p) - before).abs() > 0.01);
    }

    #[test]
    fn normal_mapping_perturbs_a_flat_face() {
        let Some(scene) = scene() else { return };
        // Look straight down at the shrine's quartz floor and gather the shaded
        // normals across it: a normal-mapped surface must not return one constant.
        let mut normals = std::collections::HashSet::new();
        for i in 0..40 {
            let x = 14.0 + i as f32 * 0.05;
            let ray = Ray::new(vec3(x, 60.0, 16.0), vec3(0.0, -1.0, 0.0));
            if let Some((hit, _)) = first_visible_hit(&scene, &ray) {
                let frame = scene.animation_frame(scene.assets.frame_count(hit.block, hit.face));
                let n = shaded_normal(&scene, &hit, frame);
                normals.insert((
                    (n.x * 1000.0) as i32,
                    (n.y * 1000.0) as i32,
                    (n.z * 1000.0) as i32,
                ));
            }
        }
        assert!(
            normals.len() > 3,
            "normal map produced {} distinct normals",
            normals.len()
        );
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
