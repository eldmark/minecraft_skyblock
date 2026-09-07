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
            // A full day in half a minute: long enough to watch, short enough to
            // demonstrate both states without waiting.
            cycle_seconds: 30.0,
            baked_step: (time.rem_euclid(1.0) * SKY_STEPS) as i32,
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

        let light = daylight::at(time);
        self.sun_dir = light.key_dir;
        self.sun_color = light.key_color;
        self.sky_color = light.sky_color;
        self.ground_color = light.ground_color;

        // The sky costs a full bake, so it only follows the clock in steps.
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
