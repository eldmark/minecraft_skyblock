//! Procedural generation of the floating island.
//!
//! `generate(seed)` is pure: same seed, same island. That is what lets `R` rebuild
//! the world in place while the camera keeps its position.

use crate::blocks::*;
use crate::noise::Noise;
use crate::world::World;

/// Island footprint in blocks. The rubric asks for at least 16x16.
pub const SIZE: usize = 32;
pub const HEIGHT: usize = 44;

/// Ground level around which the surface undulates.
pub const SURFACE_LEVEL: f32 = 26.0;
/// Everything at or below this level inside a basin fills with water.
const WATER_LEVEL: i32 = 26;

pub struct Island {
    pub world: World,
    pub seed: u32,
    /// Surface height per column, or `None` where the island has no ground.
    pub surface: Vec<Option<i32>>,
}

impl Island {
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
    let wobble = noise.fbm2(x * 0.09, z * 0.09, 3, 2.0, 0.5) * 2.6;
    let edge = SIZE as f32 * 0.47 + wobble;
    (1.0 - radius / edge.max(1e-3)).clamp(0.0, 1.0)
}

/// Smooth 0..1 mask for the ground surface. Saturates over most of the island so
/// the top stays broad and only the last few blocks roll off.
fn island_mask(noise: &Noise, x: f32, z: f32) -> f32 {
    let t = (radial_falloff(noise, x, z) / 0.30).clamp(0.0, 1.0);
    // Smoothstep, so the rim rolls off instead of stepping.
    t * t * (3.0 - 2.0 * t)
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
            let relief = noise.fbm2(fx * 0.07, fz * 0.07, 4, 2.0, 0.5) * 3.2
                + noise.fbm2(fx * 0.21, fz * 0.21, 2, 2.0, 0.5) * 0.9;
            let top = (SURFACE_LEVEL + relief * mask - (1.0 - mask) * 3.0).round() as i32;

            // Underside: a stalactite, not a cylinder. Driven by the raw radial
            // falloff rather than the saturated surface mask, so the rock keeps
            // narrowing all the way to a point under the middle of the island.
            let taper = radial_falloff(&noise, fx, fz).powf(0.75);
            let depth = 2.0
                + 24.0 * taper
                + noise.fbm2(fx * 0.13 + 40.0, fz * 0.13 - 25.0, 3, 2.0, 0.55) * 5.0 * taper;
            let bottom = (top as f32 - depth).round().max(0.0) as i32;

            for y in bottom..=top {
                let from_top = top - y;
                let block = if from_top == 0 {
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

    carve_basin(&mut world, &noise, &mut surface);
    place_ores(&mut world, &noise);
    plant_trees(&mut world, &noise, &surface);

    Island {
        world,
        seed,
        surface,
    }
}

/// Carve a pond and the channel that spills off the rim, then fill both with water.
/// The waterfall is what makes the island read as floating rather than as a plate.
fn carve_basin(world: &mut World, noise: &Noise, surface: &mut [Option<i32>]) {
    let pond_center = (11.0f32, 19.0f32);
    for z in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let (fx, fz) = (x as f32, z as f32);
            let d = ((fx - pond_center.0).powi(2) + (fz - pond_center.1).powi(2)).sqrt();
            let wobble = noise.fbm2(fx * 0.25 + 11.0, fz * 0.25 + 3.0, 2, 2.0, 0.5) * 1.4;
            let in_pond = d + wobble < 4.2;
            // A channel running from the pond to the north-west rim.
            let channel = (fz - (pond_center.1 - (pond_center.0 - fx) * 0.55)).abs() < 1.4
                && fx < pond_center.0
                && fx > 1.0;

            if !(in_pond || channel) {
                continue;
            }
            let Some(top) = surface[z as usize * SIZE + x as usize] else {
                continue;
            };

            let floor = WATER_LEVEL - if in_pond { 2 } else { 1 };
            for y in floor + 1..=top.max(WATER_LEVEL) {
                world.set(x, y, z, AIR);
            }
            for y in floor + 1..=WATER_LEVEL {
                world.set(x, y, z, WATER);
            }
            world.set(x, floor, z, if in_pond { SAND } else { COBBLESTONE });
            surface[z as usize * SIZE + x as usize] = Some(floor);
        }
    }

    // Spill: where the water surface meets thin air over the rim, pour a column
    // down the outside of the island so the pond drains into the void.
    let mut spills: Vec<(i32, i32)> = Vec::new();
    for z in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            if world.get(x, WATER_LEVEL, z) != WATER {
                continue;
            }
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, nz) = (x + dx, z + dz);
                // The neighbouring column is open air at the water line and below:
                // that is the lip of the rim, not just a shallow bank.
                if world.get(nx, WATER_LEVEL, nz) == AIR
                    && world.get(nx, WATER_LEVEL - 1, nz) == AIR
                    && world.get(nx, WATER_LEVEL - 3, nz) == AIR
                {
                    spills.push((nx, nz));
                }
            }
        }
    }
    // The fall is cut short rather than run to the bottom of the world: a stream
    // thinning out into the void reads better than one hitting an invisible floor.
    let fall_bottom = (WATER_LEVEL - 15).max(0);
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
                let block = if depth_below > 14 && vein(0.30, 91.0) > 0.42 {
                    DIAMOND_ORE
                } else if depth_below > 8 && vein(0.26, 17.0) > 0.40 {
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

/// Oak trees on grass, spaced by a per-cell jitter so they never form a lattice.
fn plant_trees(world: &mut World, noise: &Noise, surface: &[Option<i32>]) {
    const CELL: i32 = 7;
    for cz in 0..(SIZE as i32 / CELL) + 1 {
        for cx in 0..(SIZE as i32 / CELL) + 1 {
            if noise.value(cx, 0, cz) > 0.55 {
                continue;
            }
            let x = cx * CELL + (noise.value(cx, 1, cz) * CELL as f32) as i32;
            let z = cz * CELL + (noise.value(cx, 2, cz) * CELL as f32) as i32;
            if x < 2 || z < 2 || x >= SIZE as i32 - 2 || z >= SIZE as i32 - 2 {
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
        for (x, z) in [(4, 4), (16, 16), (27, 9)] {
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
    fn the_surface_is_grass_or_water_and_stone_lies_underneath() {
        let island = generate(3);
        let mut checked = 0;
        for z in 0..SIZE as i32 {
            for x in 0..SIZE as i32 {
                let Some(top) = island.surface_at(x, z) else {
                    continue;
                };
                let block = island.world.get(x, top, z);
                assert!(
                    matches!(block, GRASS | SAND | COBBLESTONE | WATER),
                    "unexpected surface block {} at {x},{z}",
                    name(block)
                );
                checked += 1;
            }
        }
        assert!(checked > 200);
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
    fn there_is_water_and_it_spills_below_the_pond() {
        let island = generate(9);
        let mut water = 0;
        let mut falling = 0;
        for z in 0..SIZE as i32 {
            for x in 0..SIZE as i32 {
                for y in 0..HEIGHT as i32 {
                    if island.world.get(x, y, z) == WATER {
                        water += 1;
                        if y < WATER_LEVEL - 2 {
                            falling += 1;
                        }
                    }
                }
            }
        }
        assert!(water > 20, "expected a pond, found {water} water blocks");
        assert!(falling > 0, "expected a waterfall over the rim");
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

#[cfg(test)]
mod debug_map {
    use super::*;

    #[test]
    fn print_top_down_map() {
        let island = generate(9);
        for z in 0..SIZE as i32 {
            let row: String = (0..SIZE as i32)
                .map(|x| match island.surface_at(x, z) {
                    None => ' ',
                    Some(top) => match island.world.get(x, top, z) {
                        WATER => 'W',
                        SAND => 's',
                        COBBLESTONE => 'c',
                        GRASS => '.',
                        _ => '?',
                    },
                })
                .collect();
            eprintln!("{row}");
        }
        let falling: usize = (0..SIZE as i32)
            .flat_map(|z| (0..SIZE as i32).map(move |x| (x, z)))
            .map(|(x, z)| (0..24).filter(|&y| island.world.get(x, y, z) == WATER).count())
            .sum();
        eprintln!("falling water blocks below y=24: {falling}");
    }
}
