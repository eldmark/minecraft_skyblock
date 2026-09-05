//! Everything the renderer needs to shade a frame: the world, its assets, the
//! lighting setup, and the animation clock.

use crate::assets::Assets;
use crate::math::{vec3, Vec3};
use crate::pack::Pack;
use crate::skybox::Skybox;
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
    /// Animation frame counter, advanced once per rendered frame.
    pub tick: usize,
}

impl Scene {
    pub fn load(seed: u32, pack: &Pack, panorama_sky: bool) -> Result<Scene, String> {
        let sun_dir = vec3(0.55, 0.62, 0.36).normalized();
        Ok(Scene {
            island: terrain::generate(seed),
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
            tick: 0,
        })
    }

    pub fn world(&self) -> &World {
        &self.island.world
    }

    pub fn reseed(&mut self, seed: u32) {
        self.island = terrain::generate(seed);
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
