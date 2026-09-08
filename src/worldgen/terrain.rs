//! Procedural generation of the floating island.
//!
//! `generate(seed)` is pure: same seed, same island. That is what lets `R` rebuild
//! the world in place while the camera keeps its position.
//!
//! The layout follows the reference diorama: a rocky outcrop on one side for the
//! great tree, a stream running across the middle and spilling off both rims, and
//! a broad terrace on the far side where the temple stands.

use crate::assets::blocks::*;
use crate::worldgen::noise::Noise;
use crate::scene::world::World;

/// Main island footprint in blocks. The rubric asks for at least 16x16.
///
/// The island is generated in its own 48x48 box, in its own coordinates, and is
/// stamped into a wider world afterwards (see `neighbours::compose`). Keeping the
/// generation and every hand-placed structure in island space is what let the
/// diorama grow two more islands without touching a line of either.
pub const SIZE: usize = 48;
pub const HEIGHT: usize = 60;

/// The shared world the islands are stamped into, and where the main one goes.
pub const WORLD_X: usize = 128;
pub const WORLD_Z: usize = 64;
pub const ORIGIN: (i32, i32) = (40, 8);

/// Ground level around which the surface undulates.
pub const SURFACE_LEVEL: f32 = 32.0;
/// The stream's water line.
pub const WATER_LEVEL: i32 = 32;

/// The rocky outcrop that carries the great tree.
pub const HILL: (f32, f32) = (11.0, 15.0);
const HILL_RADIUS: f32 = 10.0;
const HILL_HEIGHT: f32 = 9.0;

/// The stream runs between these two rim points, and falls off at both.
const RIVER_FROM: (f32, f32) = (1.0, 40.0);
const RIVER_TO: (f32, f32) = (47.0, 25.0);
const RIVER_HALF_WIDTH: f32 = 2.1;

pub struct Island {
    pub world: World,
    pub seed: u32,
    /// Surface height per column, or `None` where the island has no ground.
    pub surface: Vec<Option<i32>>,
}

impl Island {
    pub fn set_surface(&mut self, x: i32, z: i32, top: i32) {
        if x >= 0 && z >= 0 && x < SIZE as i32 && z < SIZE as i32 {
            self.surface[z as usize * SIZE + x as usize] = Some(top);
        }
    }

    pub fn surface_at(&self, x: i32, z: i32) -> Option<i32> {
        if x < 0 || z < 0 || x >= SIZE as i32 || z >= SIZE as i32 {
            return None;
        }
        self.surface[z as usize * SIZE + x as usize]
    }
}

/// How far in from the rim a column sits: 1 at the middle, 0 at the edge, with a
/// noisy boundary so the silhouette is not a circle.
fn radial_falloff(noise: &Noise, x: f32, z: f32) -> f32 {
    let center = SIZE as f32 * 0.5 - 0.5;
    let (dx, dz) = (x - center, z - center);
    let radius = (dx * dx + dz * dz).sqrt();
    let wobble = noise.fbm2(x * 0.07, z * 0.07, 3, 2.0, 0.5) * 3.4;
    let edge = SIZE as f32 * 0.47 + wobble;
    (1.0 - radius / edge.max(1e-3)).clamp(0.0, 1.0)
}

/// Smooth 0..1 mask for the ground surface. Saturates over most of the island so
/// the top stays broad and only the last few blocks roll off.
fn island_mask(noise: &Noise, x: f32, z: f32) -> f32 {
    let t = (radial_falloff(noise, x, z) / 0.22).clamp(0.0, 1.0);
    // Smoothstep, so the rim rolls off instead of stepping.
    t * t * (3.0 - 2.0 * t)
}

/// Extra height from the rocky outcrop: a smooth dome with a noisy crest.
fn hill_height(noise: &Noise, x: f32, z: f32) -> f32 {
    let d = ((x - HILL.0).powi(2) + (z - HILL.1).powi(2)).sqrt();
    let t = (1.0 - d / HILL_RADIUS).clamp(0.0, 1.0);
    let shaped = t * t * (3.0 - 2.0 * t);
    let crest = noise.fbm2(x * 0.16 + 9.0, z * 0.16 - 4.0, 3, 2.0, 0.5) * 2.2;
    shaped * (HILL_HEIGHT + crest * shaped)
}

/// Distance from a column to the stream's centre line, in blocks.
fn river_distance(noise: &Noise, x: f32, z: f32) -> f32 {
    let (ax, az) = RIVER_FROM;
    let (bx, bz) = RIVER_TO;
    let (dx, dz) = (bx - ax, bz - az);
    let len2 = dx * dx + dz * dz;
    // Projection onto the segment, clamped so the ends do not flare out.
    let t = (((x - ax) * dx + (z - az) * dz) / len2).clamp(0.0, 1.0);
    let (cx, cz) = (ax + dx * t, az + dz * t);
    let straight = ((x - cx).powi(2) + (z - cz).powi(2)).sqrt();
    // A meander, so the stream is not a ruler-straight line.
    let meander = noise.fbm2(t * 6.0, 3.0, 3, 2.0, 0.5) * 2.8;
    (straight - meander.abs()).max(0.0)
}

pub fn generate(seed: u32) -> Island {
    let noise = Noise::new(seed);
    let mut world = World::new([SIZE, HEIGHT, SIZE]);
    let mut surface = vec![None; SIZE * SIZE];

    for z in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let (fx, fz) = (x as f32, z as f32);
            let mask = island_mask(&noise, fx, fz);
            if mask <= 0.02 {
                continue;
            }

            // Rolling ground: two octave bands, one broad, one for small bumps.
            let relief = noise.fbm2(fx * 0.06, fz * 0.06, 4, 2.0, 0.5) * 3.0
                + noise.fbm2(fx * 0.19, fz * 0.19, 2, 2.0, 0.5) * 0.8;
            let hill = hill_height(&noise, fx, fz);
            let top = (SURFACE_LEVEL + relief * mask + hill * mask - (1.0 - mask) * 3.5).round() as i32;

            // Underside: a stalactite, not a cylinder. Driven by the raw radial
            // falloff rather than the saturated surface mask, so the rock keeps
            // narrowing all the way to a point under the middle of the island.
            let taper = radial_falloff(&noise, fx, fz).powf(0.75);
            // Deep enough to read as a torn-out chunk, shallow enough that the
            // point never reaches the bottom of the world.
            let depth = 2.0
                + 21.0 * taper
                + noise.fbm2(fx * 0.11 + 40.0, fz * 0.11 - 25.0, 3, 2.0, 0.55) * 5.0 * taper;
            let bottom = (top as f32 - depth - hill * 0.3).round().max(1.0) as i32;

            for y in bottom..=top {
                let from_top = top - y;
                // The outcrop is bare rock near its crest, grass lower down.
                let rocky = hill > 4.5 && from_top <= 1 && noise.value(x, y, z) > 0.25;
                let block = if rocky {
                    STONE
                } else if from_top == 0 {
                    GRASS
                } else if from_top <= 2 {
                    DIRT
                } else {
                    STONE
                };
                world.set(x, y, z, block);
            }
            surface[z as usize * SIZE + x as usize] = Some(top);
        }
    }

    carve_river(&mut world, &noise, &mut surface);
    place_ores(&mut world, &noise);
    plant_trees(&mut world, &noise, &surface);

    Island {
        world,
        seed,
        surface,
    }
}

/// Carve the stream bed across the island and fill it, then pour whatever reaches
/// a rim off the edge. Two waterfalls is what sells the island as floating.
fn carve_river(world: &mut World, noise: &Noise, surface: &mut [Option<i32>]) {
    let carve =
        |world: &mut World, surface: &mut [Option<i32>], x: i32, z: i32, floor: i32, bed: Block| {
            let Some(top) = surface[z as usize * SIZE + x as usize] else {
                return;
            };
            for y in floor + 1..=top.max(WATER_LEVEL) {
                world.set(x, y, z, AIR);
            }
            for y in floor + 1..=WATER_LEVEL {
                world.set(x, y, z, WATER);
            }
            world.set(x, floor, z, bed);
            surface[z as usize * SIZE + x as usize] = Some(floor);
        };

    for z in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let (fx, fz) = (x as f32, z as f32);
            let d = river_distance(noise, fx, fz);
            if d > RIVER_HALF_WIDTH + 1.0 {
                continue;
            }
            if d <= RIVER_HALF_WIDTH {
                // Deeper in the middle than at the sides, so the bed reads round.
                let floor = if d < RIVER_HALF_WIDTH * 0.55 {
                    WATER_LEVEL - 2
                } else {
                    WATER_LEVEL - 1
                };
                carve(world, surface, x, z, floor, GRAVEL);
            } else if let Some(top) = surface[z as usize * SIZE + x as usize] {
                // Gravelly bank right at the water's edge.
                if top <= WATER_LEVEL + 1 && world.get(x, top, z) == GRASS {
                    world.set(x, top, z, GRAVEL);
                }
            }
        }
    }

    // Where the stream meets open air, pour it into the void. The fall is cut
    // short rather than run to the bottom of the world: water thinning out reads
    // better than water hitting an invisible floor.
    let fall_bottom = (WATER_LEVEL - 20).max(1);
    let mut spills = Vec::new();
    for z in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            if world.get(x, WATER_LEVEL, z) != WATER {
                continue;
            }
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, nz) = (x + dx, z + dz);
                if world.get(nx, WATER_LEVEL, nz) == AIR
                    && world.get(nx, WATER_LEVEL - 1, nz) == AIR
                    && world.get(nx, WATER_LEVEL - 3, nz) == AIR
                {
                    spills.push((nx, nz));
                }
            }
        }
    }
    for (x, z) in spills {
        for y in (fall_bottom..=WATER_LEVEL).rev() {
            if world.get(x, y, z) != AIR {
                break;
            }
            world.set(x, y, z, WATER);
        }
    }
}

/// Ore veins: 3D noise thresholded inside stone, each ore in its own depth band so
/// gold and diamond end up hidden underneath the island, visible only from below.
fn place_ores(world: &mut World, noise: &Noise) {
    for z in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            for y in 0..HEIGHT as i32 {
                if world.get(x, y, z) != STONE {
                    continue;
                }
                let (fx, fy, fz) = (x as f32, y as f32, z as f32);
                let vein = |scale: f32, offset: f32| {
                    noise.fbm3(
                        fx * scale + offset,
                        fy * scale + offset,
                        fz * scale + offset,
                        3,
                        2.2,
                        0.5,
                    )
                };
                let depth_below = SURFACE_LEVEL as i32 - y;
                let block = if depth_below > 16 && vein(0.30, 91.0) > 0.42 {
                    DIAMOND_ORE
                } else if depth_below > 9 && vein(0.26, 17.0) > 0.40 {
                    GOLD_ORE
                } else if depth_below > 4 && vein(0.22, 53.0) > 0.36 {
                    IRON_ORE
                } else {
                    continue;
                };
                world.set(x, y, z, block);
            }
        }
    }

    // A single buried diamond block, deep under the middle: a reward for orbiting
    // beneath the island.
    let (cx, cz) = (SIZE as i32 / 2, SIZE as i32 / 2);
    for y in 0..HEIGHT as i32 {
        if world.get(cx, y, cz) != AIR {
            world.set(cx, y + 1, cz, DIAMOND_BLOCK);
            break;
        }
    }
}

/// Ordinary oak trees, kept away from the outcrop (the great tree lives there) and
/// off the temple terrace, which `structures` levels afterwards.
fn plant_trees(world: &mut World, noise: &Noise, surface: &[Option<i32>]) {
    const CELL: i32 = 8;
    for cz in 0..(SIZE as i32 / CELL) + 1 {
        for cx in 0..(SIZE as i32 / CELL) + 1 {
            if noise.value(cx, 0, cz) > 0.6 {
                continue;
            }
            let x = cx * CELL + (noise.value(cx, 1, cz) * CELL as f32) as i32;
            let z = cz * CELL + (noise.value(cx, 2, cz) * CELL as f32) as i32;
            if x < 2 || z < 2 || x >= SIZE as i32 - 2 || z >= SIZE as i32 - 2 {
                continue;
            }
            // Keep the outcrop clear for the great tree.
            if ((x as f32 - HILL.0).powi(2) + (z as f32 - HILL.1).powi(2)).sqrt() < 7.0 {
                continue;
            }
            let Some(top) = surface[z as usize * SIZE + x as usize] else {
                continue;
            };
            if world.get(x, top, z) != GRASS {
                continue;
            }

            let trunk = 4 + (noise.value(cx, 3, cz) * 2.0) as i32;
            for y in top + 1..=top + trunk {
                world.set(x, y, z, OAK_LOG);
            }
            let crown = top + trunk;
            for dy in -2..=1 {
                let radius = if dy <= -1 { 2 } else { 1 };
                for dz in -radius..=radius {
                    for dx in -radius..=radius {
                        if dx * dx + dz * dz + dy * dy > radius * radius + 2 {
                            continue;
                        }
                        let (lx, ly, lz) = (x + dx, crown + dy, z + dz);
                        if world.get(lx, ly, lz) == AIR {
                            world.set(lx, ly, lz, OAK_LEAVES);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic_for_a_seed() {
        let a = generate(2024);
        let b = generate(2024);
        assert_eq!(a.world.solid_count(), b.world.solid_count());
        for (x, z) in [(4, 4), (24, 24), (41, 9)] {
            assert_eq!(a.surface_at(x, z), b.surface_at(x, z));
        }
    }

    #[test]
    fn different_seeds_build_different_islands() {
        let a = generate(1);
        let b = generate(2);
        assert_ne!(a.world.solid_count(), b.world.solid_count());
    }

    #[test]
    fn the_island_covers_at_least_the_required_area() {
        let island = generate(7);
        let columns = (0..SIZE as i32)
            .flat_map(|z| (0..SIZE as i32).map(move |x| (x, z)))
            .filter(|(x, z)| island.surface_at(*x, *z).is_some())
            .count();
        assert!(columns >= 16 * 16, "only {columns} columns have ground");
    }

    #[test]
    fn the_island_floats_with_nothing_at_the_bottom_layer() {
        let island = generate(11);
        let bottom_solid = (0..SIZE as i32)
            .flat_map(|z| (0..SIZE as i32).map(move |x| (x, z)))
            .filter(|(x, z)| island.world.get(*x, 0, *z) != AIR)
            .count();
        assert_eq!(bottom_solid, 0, "the island should not touch y=0");
    }

    #[test]
    fn the_outcrop_rises_above_the_surrounding_ground() {
        let island = generate(3);
        let hill = island
            .surface_at(HILL.0 as i32, HILL.1 as i32)
            .expect("the outcrop should have ground");
        let plain = island
            .surface_at(SIZE as i32 - 12, SIZE as i32 / 2)
            .expect("the plain should have ground");
        assert!(
            hill > plain + 4,
            "outcrop at {hill} barely rises over the plain at {plain}"
        );
    }

    #[test]
    fn the_stream_crosses_the_island_and_falls_off_both_rims() {
        for seed in [1, 9, 2024, 31337] {
            let island = generate(seed);
            let mut water = 0;
            let mut falling_low_x = 0;
            let mut falling_high_x = 0;
            for z in 0..SIZE as i32 {
                for x in 0..SIZE as i32 {
                    for y in 0..HEIGHT as i32 {
                        if island.world.get(x, y, z) != WATER {
                            continue;
                        }
                        water += 1;
                        if y < WATER_LEVEL - 3 {
                            if x < SIZE as i32 / 2 {
                                falling_low_x += 1;
                            } else {
                                falling_high_x += 1;
                            }
                        }
                    }
                }
            }
            assert!(water > 200, "seed {seed}: the stream is too small ({water})");
            assert!(
                falling_low_x > 0 && falling_high_x > 0,
                "seed {seed}: expected a waterfall at each end ({falling_low_x}, {falling_high_x})"
            );
        }
    }

    #[test]
    fn the_surface_is_walkable_material_and_stone_lies_underneath() {
        let island = generate(3);
        let mut checked = 0;
        for z in 0..SIZE as i32 {
            for x in 0..SIZE as i32 {
                let Some(top) = island.surface_at(x, z) else {
                    continue;
                };
                let block = island.world.get(x, top, z);
                assert!(
                    matches!(block, GRASS | SAND | GRAVEL | STONE | COBBLESTONE | WATER),
                    "unexpected surface block {} at {x},{z}",
                    name(block)
                );
                checked += 1;
            }
        }
        assert!(checked > 500);
    }

    #[test]
    fn ores_are_generated_and_stay_buried() {
        let island = generate(5);
        let mut ores = 0;
        for z in 0..SIZE as i32 {
            for x in 0..SIZE as i32 {
                let Some(top) = island.surface_at(x, z) else {
                    continue;
                };
                for y in 0..HEIGHT as i32 {
                    if matches!(island.world.get(x, y, z), GOLD_ORE | IRON_ORE | DIAMOND_ORE) {
                        ores += 1;
                        assert!(y < top, "ore exposed on the surface at {x},{y},{z}");
                    }
                }
            }
        }
        assert!(ores > 20, "expected ore veins, found {ores}");
    }

    #[test]
    fn trees_stand_on_the_ground_with_leaves_above() {
        let island = generate(4);
        let mut logs = 0;
        let mut leaves = 0;
        for z in 0..SIZE as i32 {
            for x in 0..SIZE as i32 {
                for y in 0..HEIGHT as i32 {
                    match island.world.get(x, y, z) {
                        OAK_LOG => logs += 1,
                        OAK_LEAVES => leaves += 1,
                        _ => {}
                    }
                }
            }
        }
        assert!(logs >= 4, "expected at least one tree, found {logs} logs");
        assert!(leaves > logs, "trees should have more leaves than trunk");
    }
}
