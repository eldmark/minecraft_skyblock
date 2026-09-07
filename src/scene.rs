//! Everything the renderer needs to shade a frame: the world, its assets, the
//! lighting setup, and the animation clock.

use crate::assets::Assets;
use crate::daylight;
use crate::math::Vec3;
use crate::pack::Pack;
use crate::blocks::{self, Block};
use crate::noise::Noise;
use crate::parallel::thread_count;
use crate::skybox::Skybox;
use crate::neighbours;
use crate::structures;
use crate::terrain::{self, Island};
use crate::world::World;

/// How finely the baked sky follows the clock. The sky is re-baked only when the
/// cycle crosses one of these steps, which keeps a smooth cycle affordable.
const SKY_STEPS: f32 = 96.0;

/// How finely the *lighting* follows the clock. Every crossing of one of these
/// steps re-shades every pixel at full resolution, so this — not the sky bake —
/// is what a running cycle costs. Between two steps the renderer is idle and
/// keeps folding jittered samples into the image, which is both cheaper and
/// cleaner than re-shading a slightly different sun sixty times a second.
const LIGHT_STEPS: f32 = 240.0;

pub struct Scene {
    /// The main island, in its own coordinates: generation and every structure
    /// still work in that box.
    pub island: Island,
    /// The whole diorama — the main island stamped into a wider world, plus its
    /// two neighbours and the bridges. This is what the renderer traces.
    world: World,
    pub assets: Assets,
    pub skybox: Skybox,
    /// Position in the day/night cycle: 0 sunrise, 0.25 noon, 0.5 sunset,
    /// 0.75 midnight.
    pub time_of_day: f32,
    /// Whether the cycle advances on its own (toggled with `D`).
    pub cycle_running: bool,
    /// Seconds one full day takes when the cycle is running.
    pub cycle_seconds: f32,
    /// Which sky step is currently baked, so the bake is skipped when it would
    /// produce the same table.
    baked_step: i32,
    /// Which lighting step the sun and the ambient are set to.
    lit_step: i32,
    /// Threads available for re-baking the sky.
    threads: usize,
    /// Direction *towards* the sun.
    pub sun_dir: Vec3,
    pub sun_color: Vec3,
    /// Sky light that reaches surfaces facing up, and its ground counterpart.
    pub sky_color: Vec3,
    pub ground_color: Vec3,
    /// Emissive blocks, as light positions at block centers.
    pub lights: Vec<(Vec3, Block)>,
    /// Drives the crystal gate's drifting smoke.
    pub effect_noise: Noise,
    /// Animation frame counter, advanced once per rendered frame.
    pub tick: usize,
}

impl Scene {
    pub fn load(seed: u32, pack: &Pack, panorama_sky: bool, time: f32) -> Result<Scene, String> {
        let threads = thread_count();
        let mut island = terrain::generate(seed);
        structures::place_all(&mut island);
        let world = neighbours::compose(&island, seed);
        let lights = structures::collect_lights(&world);
        let light = daylight::at(time);
        Ok(Scene {
            island,
            world,
            assets: Assets::load(pack)?,
            skybox: if panorama_sky {
                Skybox::panorama(pack, time, threads)
            } else {
                Skybox::procedural(time, threads)
            },
            time_of_day: time.rem_euclid(1.0),
            cycle_running: false,
            // A full day in two minutes. Half a minute looked good in a video
            // and bad in the window: at that speed the light crosses a step
            // several times a second and every one of those frames is a full
            // re-shade, so the image never got to refine and the frame rate sat
            // at the cost of a full frame the whole time.
            cycle_seconds: 120.0,
            baked_step: (time.rem_euclid(1.0) * SKY_STEPS) as i32,
            lit_step: (time.rem_euclid(1.0) * LIGHT_STEPS) as i32,
            threads,
            sun_dir: light.key_dir,
            sun_color: light.key_color,
            sky_color: light.sky_color,
            ground_color: light.ground_color,
            lights,
            effect_noise: Noise::new(0xC0FFEE),
            tick: 0,
        })
    }

    /// Move the clock. Returns true when the lighting actually changed, so the
    /// caller knows the accumulated image has to be thrown away.
    pub fn set_time(&mut self, time: f32) -> bool {
        let time = time.rem_euclid(1.0);
        if (time - self.time_of_day).abs() < 1e-6 {
            return false;
        }
        self.time_of_day = time;

        // The clock reading moves continuously; the lighting moves in steps.
        // Re-shading the whole image for a sun that has turned a thousandth of a
        // degree is what made the running cycle expensive.
        let step = (time * LIGHT_STEPS) as i32;
        if step == self.lit_step {
            return false;
        }
        self.lit_step = step;

        let light = daylight::at(time);
        self.sun_dir = light.key_dir;
        self.sun_color = light.key_color;
        self.sky_color = light.sky_color;
        self.ground_color = light.ground_color;

        // The sky costs a full bake, so it follows the clock in coarser steps.
        let step = (time * SKY_STEPS) as i32;
        if step != self.baked_step {
            self.baked_step = step;
            self.skybox.set_time(time, self.threads);
        }
        true
    }

    /// Advance the cycle by `dt` seconds. No-op while the cycle is paused.
    pub fn advance_cycle(&mut self, dt: f32) -> bool {
        if !self.cycle_running {
            return false;
        }
        self.set_time(self.time_of_day + dt / self.cycle_seconds.max(0.001))
    }

    pub fn toggle_cycle(&mut self) {
        self.cycle_running = !self.cycle_running;
    }

    pub fn clock(&self) -> String {
        daylight::clock(self.time_of_day)
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// Seconds a full day takes, and the seconds between two lighting updates
    /// at that speed. Used by the status line and the tests.
    #[cfg(test)]
    pub fn seconds_between_light_updates(&self) -> f32 {
        self.cycle_seconds / LIGHT_STEPS
    }

    pub fn reseed(&mut self, seed: u32) {
        let mut island = terrain::generate(seed);
        structures::place_all(&mut island);
        self.world = neighbours::compose(&island, seed);
        self.lights = structures::collect_lights(&self.world);
        self.island = island;
    }

    /// Seconds of animation elapsed, at the nominal 60 frames per second the
    /// window targets. Used by continuous effects rather than frame-stepped ones.
    pub fn time(&self) -> f32 {
        self.tick as f32 / 60.0
    }

    /// Smoke inside the crystal gate: 3D noise drifting upward through the block,
    /// so the portal reads as thick and alive instead of as tinted glass.
    /// Returns a factor around 1.0 to scale color and emission by.
    pub fn portal_smoke(&self, point: crate::math::Vec3) -> f32 {
        let t = self.time();
        let n = self.effect_noise.fbm3(
            point.x * 1.5,
            point.y * 1.5 - t * 0.55,
            point.z * 1.5 + t * 0.20,
            3,
            2.1,
            0.55,
        );
        // Remap roughly [-1,1] to [0.45, 1.75]: dark wisps against a bright core.
        (n * 0.65 + 1.1).clamp(0.35, 1.9)
    }

    pub fn is_portal(&self, block: Block) -> bool {
        matches!(block, blocks::PORTAL | blocks::NETHER_PORTAL)
    }

    /// Frame index for an animated texture with `frames` frames. Minecraft's water
    /// advances every few ticks; matching that keeps it from strobing.
    pub fn animation_frame(&self, frames: usize) -> usize {
        if frames <= 1 {
            0
        } else {
            (self.tick / 3) % frames
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The pack is not committed; these tests skip when it is missing.
    fn scene() -> Option<Scene> {
        if !Path::new("texturepack").is_dir() {
            return None;
        }
        Scene::load(2024, &Pack::open(None).ok()?, false, 0.25).ok()
    }

    #[test]
    fn a_running_cycle_updates_the_light_a_couple_of_times_a_second() {
        let Some(mut scene) = scene() else { return };
        scene.cycle_running = true;

        // One second of frames at 60 fps.
        let updates = (0..60)
            .filter(|_| scene.advance_cycle(1.0 / 60.0))
            .count();
        let expected = 1.0 / scene.seconds_between_light_updates();
        assert!(
            (updates as f32 - expected).abs() <= 1.5,
            "{updates} light updates in a second, expected about {expected}"
        );
        // Every one of those frames costs a full re-shade, so a handful a second
        // is the whole point: the rest refine the image instead.
        assert!(updates < 6, "the cycle re-lights {updates} times a second");

        // The clock itself keeps moving smoothly, whatever the light does.
        let day = scene.time_of_day - 0.25;
        assert!(
            (day - 1.0 / scene.cycle_seconds).abs() < 1e-4,
            "a second of cycle moved the clock by {day} of a day"
        );
    }

    #[test]
    fn a_tiny_step_moves_the_clock_without_re_lighting() {
        let Some(mut scene) = scene() else { return };
        let before = scene.sun_dir;
        let changed = scene.set_time(scene.time_of_day + 1.0 / (LIGHT_STEPS * 8.0));
        assert!(!changed, "a fraction of a step should not re-light the scene");
        assert_ne!(scene.time_of_day, 0.25, "the clock should still have moved");
        assert_eq!(scene.sun_dir, before);

        // Crossing the step does re-light it.
        assert!(scene.set_time(scene.time_of_day + 1.0 / LIGHT_STEPS));
        assert_ne!(scene.sun_dir, before);
    }
}
