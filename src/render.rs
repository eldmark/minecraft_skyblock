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

/// Length of the running average used while the camera is still. The first frames
/// converge (weights 1, 1/2, 1/3, ...); after that the weight stops shrinking, so
/// the image keeps tracking the animated water and gate instead of freezing.
pub const SAMPLE_WINDOW: usize = 16;

/// While the camera moves, one pixel in this many is traced each frame and the
/// rest keep the colour they had. Two is a checkerboard: half the rays per frame,
/// and a pixel is never more than one frame stale, which is what keeps a fast drag
/// from breaking up into speckle.
const MOVING_INTERLEAVE: usize = 2;

/// Once the running average has settled, a pixel that is not changing is only
/// re-traced every this many frames. Water, the gate and their reflections keep
/// changing, so they stay on every frame; the still two thirds of the image do not.
const IDLE_REFRESH: u32 = 8;
/// How much a pixel has to move for it to count as still animating, in linear
/// light. Roughly one step of an 8-bit channel.
const ACTIVITY_EPSILON: f32 = 0.004;

pub struct Renderer {
    pub threads: usize,
    /// 1 = full resolution, 2 = half, and so on. Raised while the camera moves.
    pub scale: usize,
    low: Vec<u32>,
    /// Running average per pixel, for progressive refinement while idle.
    accumulator: Vec<Vec3>,
    pub samples: usize,
    /// Per pixel: does this pixel still change from frame to frame?
    active: Vec<bool>,
    /// The last complete frame shown while moving, so interleaved frames can keep
    /// the pixels they do not trace.
    display: Vec<u32>,
    /// The previous frame's sample per pixel. Comparing sample against sample (not
    /// against the running average, which is still catching up) is what makes the
    /// activity test see the animation instead of the convergence.
    previous: Vec<Vec3>,
    /// Rays actually traced in the last `accumulate`, for the idle benchmark.
    pub last_traced: usize,
    /// Skip re-tracing quiet pixels once the image has settled. On by default; the
    /// tests turn it off to compare against the full refinement.
    pub selective_refresh: bool,
    /// Which interleave phase the next moving frame traces.
    moving_phase: usize,
    /// False until a full-resolution frame has filled the buffer, so the first
    /// frame after a resize is not built on top of stale or blank pixels.
    interleave_primed: bool,
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
            active: Vec::new(),
            previous: Vec::new(),
            display: Vec::new(),
            last_traced: 0,
            selective_refresh: true,
            moving_phase: 0,
            interleave_primed: false,
        }
    }

    /// Drop the accumulated samples: the image is about to change.
    pub fn reset_accumulation(&mut self) {
        self.samples = 0;
    }

    /// Forget the interleaved frame: the next moving frame will be a full one.
    pub fn invalidate_moving(&mut self) {
        self.interleave_primed = false;
    }

    /// Mark every pixel as changing again, without throwing the image away.
    ///
    /// Used when the light moves but the geometry does not: the day/night cycle
    /// changes every pixel's brightness, so none of them may be skipped, but the
    /// converged average is still the right starting point and keeps the
    /// transition smooth instead of dropping back to a noisy frame.
    pub fn mark_all_active(&mut self) {
        self.active.iter_mut().for_each(|flag| *flag = true);
    }

    /// Fold one jittered sample per pixel into the running average and show it.
    ///
    /// Called while the camera is still. Each pass offsets the ray inside the pixel
    /// by a different low-discrepancy amount, so edges resolve into smooth
    /// gradients without ever tracing more than one ray per pixel per frame.
    ///
    /// A new sample weighs `1 / (n + 1)` while the window fills, then stays at
    /// `1 / SAMPLE_WINDOW`. That floor is what keeps the picture tracking the
    /// animated water and gate: a plain mean would freeze, and the previous fix —
    /// throwing the average away and showing a single noisy sample so the
    /// animation could move — made the image blink every sixteenth frame.
    pub fn accumulate(&mut self, frame: &mut Framebuffer, scene: &Scene, camera: &Camera) {
        let (w, h) = (frame.width, frame.height);
        if self.accumulator.len() != w * h || self.samples == 0 {
            self.accumulator.clear();
            self.accumulator.resize(w * h, Vec3::ZERO);
            self.active.clear();
            self.active.resize(w * h, true);
            self.previous.clear();
            self.previous.resize(w * h, Vec3::ZERO);
            self.samples = 0;
        }

        // While the average converges, every frame uses a fresh sub-pixel offset:
        // that is where the antialiasing comes from. Once it has converged the
        // offset is frozen, so any remaining frame-to-frame difference is the
        // animation itself — which is exactly what the activity test needs to see.
        let settled_now = self.samples >= SAMPLE_WINDOW;
        let jitter = halton_2d(if settled_now {
            SAMPLE_WINDOW
        } else {
            self.samples + 1
        });
        let threads = self.threads;
        let window = self.samples.min(SAMPLE_WINDOW - 1) as f32;
        let weight = 1.0 / (window + 1.0);
        // While the average is still converging every pixel is traced. After that,
        // only the ones that are actually changing, plus a rotating slice of the
        // rest so a pixel that starts moving again is picked up within a few frames.
        let skip_quiet = settled_now && self.selective_refresh;
        let phase = (self.samples as u32) % IDLE_REFRESH;
        let traced = AtomicUsize::new(0);

        // The accumulator is striped exactly like the framebuffer, so a worker owns
        // the same rows in both and no locking is needed.
        let accumulator = &mut self.accumulator;
        let active = &mut self.active;
        let previous = &mut self.previous;
        let strip = w * ROWS_PER_STRIP;
        let mut pairs: Vec<Strip> = accumulator
            .chunks_mut(strip)
            .zip(frame.pixels.chunks_mut(strip))
            .zip(active.chunks_mut(strip))
            .zip(previous.chunks_mut(strip))
            .map(|(((sums, pixels), flags), last)| (sums, pixels, flags, last))
            .collect();

        run_strips(
            &mut pairs,
            threads,
            |average, pixels, flags, last, first_row| {
                let mut count = 0usize;
                for (i, slot) in average.iter_mut().enumerate() {
                    let y = first_row + i / w;
                    let x = i % w;
                    // Settled and quiet: re-trace only on this pixel's turn, so a
                    // pixel that starts moving again is noticed within a few frames.
                    if skip_quiet
                        && !flags[i]
                        && ((x as u32 ^ ((y as u32) << 1)) % IDLE_REFRESH) != phase
                    {
                        continue;
                    }
                    let ray = camera.ray(x, y, w, h, jitter);
                    let sample = trace_color(scene, &ray);
                    count += 1;

                    let was_active = flags[i];
                    flags[i] = (sample - last[i]).length() > ACTIVITY_EPSILON;
                    last[i] = sample;

                    // A settled pixel that is not moving keeps the average it
                    // converged to. Folding a frozen-jitter sample into it would
                    // slowly undo the antialiasing for no reason.
                    if !settled_now || flags[i] || was_active {
                        *slot = slot.lerp(sample, weight);
                        pixels[i] = to_srgb_u32(*slot);
                    }
                }
                traced.fetch_add(count, Ordering::Relaxed);
            },
        );
        self.last_traced = traced.load(Ordering::Relaxed);
        self.samples = self.samples.saturating_add(1);
    }

    /// The frame to draw while the camera is moving.
    ///
    /// Instead of tracing a half-resolution image and stretching it — which turns
    /// every edge into stair-steps for as long as the drag lasts — this traces one
    /// pixel in two at full resolution, in a checkerboard, and fills the other
    /// half from the two neighbours traced *this* frame.
    ///
    /// Filling from the previous frame instead was cheaper still, but during a
    /// drag those pixels show a world point the camera has already left, and half
    /// an image of stale pixels reads as motion blur. Interpolating horizontally
    /// costs nothing and every pixel on screen belongs to the current frame.
    pub fn render_moving(&mut self, frame: &mut Framebuffer, scene: &Scene, camera: &Camera) {
        let (w, h) = (frame.width, frame.height);
        if !self.interleave_primed || self.display.len() != w * h {
            // Nothing trustworthy on screen yet: pay for one full frame.
            self.display.clear();
            self.display.resize(w * h, 0);
            self.render(frame, scene, camera);
            self.display.copy_from_slice(&frame.pixels);
            self.interleave_primed = true;
            self.moving_phase = 0;
            return;
        }

        let phase = self.moving_phase;
        self.moving_phase = (self.moving_phase + 1) % MOVING_INTERLEAVE;
        let threads = self.threads;
        let traced = AtomicUsize::new(0);

        // Copy forward what was on screen, then overwrite this phase's pixels.
        frame.pixels.copy_from_slice(&self.display);
        render_strips(&mut frame.pixels, w, ROWS_PER_STRIP, threads, |strip, first_row| {
            let mut count = 0usize;
            for (i, px) in strip.iter_mut().enumerate() {
                let y = first_row + i / w;
                let x = i % w;
                // Checkerboard: the two phases tile the plane, so every pixel is
                // refreshed on alternate frames wherever the camera points.
                if (x + y) % MOVING_INTERLEAVE != phase {
                    continue;
                }
                let ray = camera.ray(x, y, w, h, (0.5, 0.5));
                *px = to_srgb_u32(trace_color(scene, &ray));
                count += 1;
            }
            // Second pass: the skipped pixels. In a checkerboard the neighbours
            // to left and right always belong to the other phase, so both were
            // traced a moment ago in this same strip.
            for i in 0..strip.len() {
                let y = first_row + i / w;
                let x = i % w;
                if (x + y) % MOVING_INTERLEAVE == phase {
                    continue;
                }
                let left = if x > 0 { strip[i - 1] } else { strip[i + 1] };
                let right = if x + 1 < w { strip[i + 1] } else { left };
                strip[i] = average_srgb(left, right);
            }
            traced.fetch_add(count, Ordering::Relaxed);
        });
        self.display.copy_from_slice(&frame.pixels);
        self.last_traced = traced.load(Ordering::Relaxed);
        self.samples = 0;
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

/// Mean of two 0RGB pixels, per channel. Averaging the encoded values rather
/// than the linear colors is wrong by a hair and free; it is a fill for one
/// frame of a drag, not a shading result.
fn average_srgb(a: u32, b: u32) -> u32 {
    let mask = 0x00FF_00FF;
    // Split into two interleaved channel pairs so the whole pixel averages with
    // two shifts instead of six.
    let (ra, rb) = (a & mask, b & mask);
    let (ga, gb) = ((a >> 8) & mask, (b >> 8) & mask);
    (((ra + rb) >> 1) & mask) | ((((ga + gb) >> 1) & mask) << 8)
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
/// The four per-pixel buffers a refinement pass touches, sliced into one strip.
type Strip<'a> = (&'a mut [Vec3], &'a mut [u32], &'a mut [bool], &'a mut [Vec3]);

fn run_strips<F>(items: &mut [Strip], threads: usize, render: F)
where
    F: Fn(&mut [Vec3], &mut [u32], &mut [bool], &mut [Vec3], usize) + Sync,
{
    if threads <= 1 || items.len() <= 1 {
        for (index, (sums, pixels, flags, last)) in items.iter_mut().enumerate() {
            render(sums, pixels, flags, last, index * ROWS_PER_STRIP);
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
                let (mut sums, mut pixels, mut flags, mut last) = {
                    let mut guard = queue.lock().unwrap();
                    let (sums, pixels, flags, last) = &mut guard[index];
                    (
                        std::mem::take(sums),
                        std::mem::take(pixels),
                        std::mem::take(flags),
                        std::mem::take(last),
                    )
                };
                render(
                    &mut sums,
                    &mut pixels,
                    &mut flags,
                    &mut last,
                    index * ROWS_PER_STRIP,
                );
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

/// Where a ray restarts after passing through something: just outside the cell it
/// hit, not just past the point where it entered.
///
/// Nudging by an epsilon along the direction leaves the ray *inside* the same
/// voxel, so the next traversal starts in that cell and hits the very same block
/// again — a glass pane or a leaf would eat all six of its skips against itself
/// and the ray would give up and return sky. That is what made windows show the
/// sky instead of the room behind them.
fn past_voxel(point: Vec3, dir: Vec3, voxel: [i32; 3]) -> Vec3 {
    let mut exit = f32::MAX;
    for axis in 0..3 {
        let d = dir.axis(axis);
        if d.abs() < 1e-8 {
            continue;
        }
        let bound = if d > 0.0 {
            voxel[axis] as f32 + 1.0
        } else {
            voxel[axis] as f32
        };
        exit = exit.min((bound - point.axis(axis)) / d);
    }
    point + dir * (exit.max(0.0) + 1e-3)
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
        // Out of this cell entirely, and keep going.
        origin = past_voxel(hit.point, ray.dir, hit.voxel);
    }
    None
}

/// Shadow factor in `[0, 1]`: 1 in full sun, lower behind transparent blockers.
/// How far a shadow ray is allowed to travel. Long enough for one island to
/// shade its own structures and its neighbour across a bridge; short enough that
/// a ray does not walk the whole 128-block world to find out there is nothing
/// there. Measured: 28.6 -> 25.8 ms per frame at 900x600.
const SHADOW_RANGE: f32 = 56.0;

fn sun_visibility(scene: &Scene, point: Vec3, normal: Vec3) -> f32 {
    let world: &World = scene.world();
    let mut origin = point + normal * 1e-3;
    let mut transmission = 1.0f32;
    for _ in 0..MAX_ALPHA_SKIPS {
        let probe = Ray::new(origin, scene.sun_dir);
        let Some(hit) = world.trace(&probe, SHADOW_RANGE, |b| b != blocks::AIR) else {
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
        origin = past_voxel(hit.point, probe.dir, hit.voxel);
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

    /// These tests skip when neither the extracted textures nor the pack are
    /// there to load.
    fn scene() -> Option<Scene> {
        Scene::load(2024, &Pack::open(None).ok()?, false, 0.16).ok()
    }

    #[test]
    fn rendering_is_deterministic_across_thread_counts() {
        let Some(scene) = scene() else { return };
        let camera = Camera::new(vec3(56.0, 22.0, 24.0), 60.0);
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
        let camera = Camera::new(vec3(56.0, 22.0, 24.0), 60.0);
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
        let camera = Camera::new(vec3(56.0, 24.0, 24.0), 45.0);
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
            // The shrine's floor, in world coordinates: the main island now sits
            // at `terrain::ORIGIN` inside a wider world.
            let x = 54.0 + i as f32 * 0.05;
            let ray = Ray::new(vec3(x, 59.0, 24.0), vec3(0.0, -1.0, 0.0));
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
    fn a_ray_through_a_cutout_leaves_the_cell_it_skipped() {
        // The nudge past a transparent texel used to keep the ray inside the same
        // voxel, so a pane or a leaf ate all six skips against itself and the ray
        // gave up and returned sky: windows showed the sky instead of the room.
        let dir = vec3(-1.0, 0.0, 0.0);
        let point = vec3(116.0, 34.5, 27.5);
        let out = past_voxel(point, dir, [115, 34, 27]);
        assert!(out.x < 115.0, "the ray stayed in the cell it skipped: {out:?}");

        // And a diagonal ray leaves through whichever face it reaches first.
        let dir = vec3(-1.0, 0.3, 0.0).normalized();
        let out = past_voxel(vec3(116.0, 34.1, 27.5), dir, [115, 34, 27]);
        assert!(out.x < 115.0 || out.y > 35.0, "{out:?}");
    }

    #[test]
    fn a_window_shows_what_is_behind_it_and_not_the_sky() {
        let Some(scene) = scene() else { return };
        // Straight at the farm house's window from outside: what comes back must
        // be the lit room, not the sky the ray used to fall through to.
        let ray = Ray::new(vec3(126.0, 34.5, 27.0), vec3(-1.0, 0.0, 0.0));
        let color = trace_color(&scene, &ray);
        let sky = scene.skybox.sample(ray.dir);
        assert!(
            (color - sky).length() > 0.05,
            "the window returned the sky: {color:?} against {sky:?}"
        );
    }

    #[test]
    fn the_idle_image_never_jumps_once_it_has_settled() {
        let Some(mut scene) = scene() else { return };
        let camera = Camera::new(vec3(64.0, 34.0, 32.0), 80.0);
        let mut frame = Framebuffer::new(96, 72);
        let mut renderer = Renderer::new();

        // Let the running average fill its window.
        for _ in 0..SAMPLE_WINDOW {
            renderer.accumulate(&mut frame, &scene, &camera);
            scene.tick += 1;
        }

        // From here the picture must only drift with the animation. The bug this
        // guards against threw the average away and showed a single noisy sample,
        // which moved most of the frame at once.
        let channel = |p: u32, shift: u32| ((p >> shift) & 0xFF) as i32;
        for step in 0..24 {
            let before = frame.pixels.clone();
            renderer.accumulate(&mut frame, &scene, &camera);
            scene.tick += 1;

            let moved = before
                .iter()
                .zip(frame.pixels.iter())
                .filter(|(a, b)| {
                    [16, 8, 0]
                        .iter()
                        .any(|s| (channel(**a, *s) - channel(**b, *s)).abs() > 12)
                })
                .count();
            assert!(
                moved * 20 < before.len(),
                "frame {step} changed {moved} of {} pixels: that is a blink",
                before.len()
            );
        }
        // And the average is never thrown away while the camera sits still.
        assert!(renderer.samples >= SAMPLE_WINDOW + 24);
    }

    #[test]
    fn selective_refresh_keeps_the_image_current() {
        let Some(mut scene) = scene() else { return };
        let camera = Camera::new(vec3(64.0, 34.0, 32.0), 80.0);
        let mut refined = Framebuffer::new(120, 90);
        let mut renderer = Renderer::new();

        // Settle, then run well past the point where quiet pixels are only
        // re-traced on their turn.
        for _ in 0..SAMPLE_WINDOW + 40 {
            renderer.accumulate(&mut refined, &scene, &camera);
            scene.tick += 1;
        }
        assert!(
            renderer.last_traced < 120 * 90,
            "nothing was skipped: {} of {} pixels",
            renderer.last_traced,
            120 * 90
        );

        // The same refinement with skipping turned off: the selective image must
        // match it, or quiet pixels are going stale.
        let mut fresh = Framebuffer::new(120, 90);
        let mut reference = Renderer::new();
        reference.selective_refresh = false;
        let mut replay = Scene::load(2024, &Pack::open(None).unwrap(), false, 0.16).unwrap();
        for _ in 0..SAMPLE_WINDOW + 40 {
            reference.accumulate(&mut fresh, &replay, &camera);
            replay.tick += 1;
        }

        let channel = |p: u32, shift: u32| ((p >> shift) & 0xFF) as i32;
        let far = refined
            .pixels
            .iter()
            .zip(fresh.pixels.iter())
            .filter(|(a, b)| {
                [16, 8, 0]
                    .iter()
                    .any(|s| (channel(**a, *s) - channel(**b, *s)).abs() > 24)
            })
            .count();
        assert!(
            far * 40 < refined.pixels.len(),
            "{far} of {} pixels drifted from the fully refreshed image",
            refined.pixels.len()
        );
    }

    #[test]
    fn a_moving_frame_traces_half_and_interpolates_the_rest() {
        let Some(scene) = scene() else { return };
        let camera = Camera::new(vec3(64.0, 34.0, 32.0), 90.0);
        let (w, h) = (96usize, 72usize);
        let mut moving = Framebuffer::new(w, h);
        let mut renderer = Renderer::new();

        // Prime, then one interleaved frame with the camera held still.
        renderer.render_moving(&mut moving, &scene, &camera);
        renderer.render_moving(&mut moving, &scene, &camera);
        assert!(
            renderer.last_traced * 2 <= w * h + w,
            "an interleaved frame traced {} of {} pixels",
            renderer.last_traced,
            w * h
        );

        let mut full = Framebuffer::new(w, h);
        Renderer::new().render(&mut full, &scene, &camera);

        // Half the pixels are the real thing, and the other half is the mean of
        // its two neighbours from this same frame — never a leftover from an
        // older one, which is what used to smear during a drag.
        let mut traced = 0;
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let i = y * w + x;
                if moving.pixels[i] == full.pixels[i] {
                    traced += 1;
                    continue;
                }
                let expected = average_srgb(moving.pixels[i - 1], moving.pixels[i + 1]);
                assert_eq!(
                    moving.pixels[i], expected,
                    "pixel {x},{y} is neither traced nor interpolated"
                );
            }
        }
        assert!(
            traced > (w - 2) * (h - 2) / 3,
            "only {traced} pixels came out of the tracer"
        );
    }

    #[test]
    fn a_resize_forces_a_full_moving_frame() {
        let Some(scene) = scene() else { return };
        let camera = Camera::new(vec3(64.0, 34.0, 32.0), 90.0);
        let mut frame = Framebuffer::new(64, 48);
        let mut renderer = Renderer::new();
        renderer.render_moving(&mut frame, &scene, &camera);
        renderer.render_moving(&mut frame, &scene, &camera);

        // After a resize the old contents mean nothing, so the next frame has to
        // be a complete one or the window would show garbage.
        frame.resize(80, 60);
        renderer.invalidate_moving();
        renderer.render_moving(&mut frame, &scene, &camera);
        assert_eq!(frame.pixels.iter().filter(|&&p| p == 0).count(), 0);
    }

    #[test]
    fn the_window_loop_never_leaves_black_holes() {
        let Some(mut scene) = scene() else { return };
        let camera = Camera::new(vec3(64.0, 34.0, 32.0), 80.0);
        let mut frame = Framebuffer::new(120, 90);
        let mut renderer = Renderer::new();

        // Exactly what the window does every frame: ask for the current size, then
        // refine. When `resize` cleared unconditionally, the pixels the refinement
        // skipped stayed black and the image collapsed into scattered dots.
        for _ in 0..SAMPLE_WINDOW + 30 {
            frame.resize(120, 90);
            renderer.accumulate(&mut frame, &scene, &camera);
            scene.tick += 1;
        }

        let black = frame.pixels.iter().filter(|&&p| p == 0).count();
        assert_eq!(black, 0, "{black} pixels were left unwritten");
    }

    #[test]
    fn the_running_average_smooths_a_single_sample() {
        let Some(scene) = scene() else { return };
        let camera = Camera::new(vec3(64.0, 34.0, 32.0), 80.0);
        let mut one = Framebuffer::new(80, 60);
        let mut many = Framebuffer::new(80, 60);

        let mut a = Renderer::new();
        a.accumulate(&mut one, &scene, &camera);
        let mut b = Renderer::new();
        for _ in 0..SAMPLE_WINDOW {
            b.accumulate(&mut many, &scene, &camera);
        }

        // Averaging jittered samples softens edges: neighbouring pixels differ less
        // than in a single-sample frame.
        let contrast = |f: &Framebuffer| -> i64 {
            let mut total = 0i64;
            for y in 0..f.height {
                for x in 1..f.width {
                    let (p, q) = (f.pixels[y * f.width + x], f.pixels[y * f.width + x - 1]);
                    total += (((p >> 16) & 0xFF) as i64 - ((q >> 16) & 0xFF) as i64).abs();
                }
            }
            total
        };
        assert!(
            contrast(&many) < contrast(&one),
            "the averaged frame should be smoother"
        );
    }

    #[test]
    fn scaled_rendering_still_fills_the_framebuffer() {
        let Some(scene) = scene() else { return };
        let camera = Camera::new(vec3(56.0, 22.0, 24.0), 60.0);
        let mut frame = Framebuffer::new(64, 48);
        let mut r = Renderer::new();
        r.scale = 3;
        r.render(&mut frame, &scene, &camera);
        assert!(frame.pixels.iter().any(|&p| p != frame.pixels[0]));
    }
}

