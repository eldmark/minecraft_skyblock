//! Everything the renderer needs to shade a frame: the world, its assets, the
//! lighting setup, and the animation clock.

use crate::assets::Assets;
use crate::math::{vec3, Vec3};
use crate::pack::Pack;
use crate::blocks::{self, Block};
use crate::noise::Noise;
use crate::skybox::Skybox;
use crate::structures;
use crate::terrain::{self, Island};
use crate::world::World;

pub struct Scene {
    pub island: Island,
    pub assets: Assets,
    pub skybox: Skybox,
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
    pub fn load(seed: u32, pack: &Pack, panorama_sky: bool) -> Result<Scene, String> {
        let sun_dir = vec3(0.55, 0.62, 0.36).normalized();
        let mut island = terrain::generate(seed);
        structures::place_shrine(&mut island);
        let lights = structures::collect_lights(&island.world);
        Ok(Scene {
            island,
            assets: Assets::load(pack)?,
            skybox: if panorama_sky {
                Skybox::panorama(pack, sun_dir)
            } else {
                Skybox::dusk(sun_dir)
            },
            // Low sun, matching the reference diorama's warm rim light.
            sun_dir,
            sun_color: vec3(1.35, 1.12, 0.86),
            sky_color: vec3(0.30, 0.40, 0.62),
            ground_color: vec3(0.16, 0.13, 0.11),
            lights,
            effect_noise: Noise::new(0xC0FFEE),
            tick: 0,
        })
    }

    pub fn world(&self) -> &World {
        &self.island.world
    }

    pub fn reseed(&mut self, seed: u32) {
        let mut island = terrain::generate(seed);
        structures::place_shrine(&mut island);
        self.lights = structures::collect_lights(&island.world);
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
        block == blocks::PORTAL
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
