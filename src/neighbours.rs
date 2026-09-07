//! The two smaller islands beside the main one, and the plank bridges to them.
//!
//! The main island is generated and built in its own 48x48 coordinate system;
//! `compose` stamps it into a wider world and grows the neighbours around it, so
//! neither `terrain` nor `structures` had to learn about the new geography.
//!
//! West is the nether island: netherrack, lava and a lit portal. East is the
//! farm: a plank house, a watered field, hay and a fenced pen with animals.

use crate::blocks::*;
use crate::noise::Noise;
use crate::terrain::{Island, HEIGHT, ORIGIN, SURFACE_LEVEL, WORLD_X, WORLD_Z};
use crate::world::World;

/// Where the two neighbours sit, in world coordinates. Both are on the main
/// island's centre line so the bridges run straight.
pub const NETHER: (i32, i32) = (18, 32);
pub const FARM: (i32, i32) = (110, 32);
const NEIGHBOUR_RADIUS: f32 = 12.5;

/// Ground heights of one island, indexed like the world grid.
pub struct Surface(Vec<Option<i32>>);

impl Surface {
    fn new() -> Surface {
        Surface(vec![None; WORLD_X * WORLD_Z])
    }

    fn set(&mut self, x: i32, z: i32, top: i32) {
        if let Some(cell) = self.cell_mut(x, z) {
            *cell = Some(top);
        }
    }

    pub fn get(&self, x: i32, z: i32) -> Option<i32> {
        if x < 0 || z < 0 || x >= WORLD_X as i32 || z >= WORLD_Z as i32 {
            return None;
        }
        self.0[z as usize * WORLD_X + x as usize]
    }

    fn cell_mut(&mut self, x: i32, z: i32) -> Option<&mut Option<i32>> {
        if x < 0 || z < 0 || x >= WORLD_X as i32 || z >= WORLD_Z as i32 {
            return None;
        }
        Some(&mut self.0[z as usize * WORLD_X + x as usize])
    }
}

/// What an island is made of, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Palette {
    Grass,
    Nether,
}

/// Build the whole diorama: the main island stamped in place, its two
/// neighbours, and the bridges between them.
pub fn compose(island: &Island, seed: u32) -> World {
    let mut world = World::new([WORLD_X, HEIGHT, WORLD_Z]);
    stamp(&mut world, &island.world);

    // A seed of its own, so the neighbours change with `R` but do not merely
    // repeat the main island's noise field.
    let noise = Noise::new(seed ^ 0x5EED_1E55);

    let nether_ground = land(&mut world, &noise, NETHER, Palette::Nether);
    let farm_ground = land(&mut world, &noise, FARM, Palette::Grass);

    nether_island(&mut world, &noise, &nether_ground);
    farm_island(&mut world, &noise, &farm_ground);

    bridge(&mut world, &nether_ground, NETHER, 1, NETHERRACK);
    bridge(&mut world, &farm_ground, FARM, -1, DIRT);

    world
}

/// Copy the main island into the shared world at `ORIGIN`.
fn stamp(world: &mut World, island: &World) {
    for y in 0..island.size[1] as i32 {
        for z in 0..island.size[2] as i32 {
            for x in 0..island.size[0] as i32 {
                let block = island.get(x, y, z);
                if block != AIR {
                    world.set(x + ORIGIN.0, y, z + ORIGIN.1, block);
                }
            }
        }
    }
}

/// Highest non-air block in a column, if any.
pub fn top_at(world: &World, x: i32, z: i32) -> Option<i32> {
    (0..HEIGHT as i32)
        .rev()
        .find(|&y| world.get(x, y, z) != AIR)
}

/// A smaller island: the same recipe as the main one — noisy radial falloff, a
/// rolling top and a tapering underside — at a third of the size.
fn land(world: &mut World, noise: &Noise, centre: (i32, i32), palette: Palette) -> Surface {
    let mut surface = Surface::new();
    let reach = NEIGHBOUR_RADIUS as i32 + 4;
    let phase = centre.0 as f32 * 0.37;

    for z in centre.1 - reach..=centre.1 + reach {
        for x in centre.0 - reach..=centre.0 + reach {
            let (fx, fz) = (x as f32, z as f32);
            let d = ((fx - centre.0 as f32).powi(2) + (fz - centre.1 as f32).powi(2)).sqrt();
            let wobble = noise.fbm2(fx * 0.09 + phase, fz * 0.09, 3, 2.0, 0.5) * 2.6;
            let falloff = (1.0 - d / (NEIGHBOUR_RADIUS + wobble).max(1e-3)).clamp(0.0, 1.0);
            if falloff <= 0.02 {
                continue;
            }
            let t = (falloff / 0.28).clamp(0.0, 1.0);
            let mask = t * t * (3.0 - 2.0 * t);

            let relief = noise.fbm2(fx * 0.13 + phase, fz * 0.13, 3, 2.0, 0.5) * 2.4;
            let top = (SURFACE_LEVEL + relief * mask - (1.0 - mask) * 3.0).round() as i32;

            let taper = falloff.powf(0.7);
            let depth = 2.0
                + 17.0 * taper
                + noise.fbm2(fx * 0.15 - 12.0, fz * 0.15 + 30.0, 3, 2.0, 0.55) * 4.0 * taper;
            let bottom = (top as f32 - depth).round().max(1.0) as i32;

            for y in bottom..=top {
                let from_top = top - y;
                let block = match palette {
                    Palette::Grass => {
                        if from_top == 0 {
                            GRASS
                        } else if from_top <= 2 {
                            DIRT
                        } else {
                            STONE
                        }
                    }
                    Palette::Nether => {
                        // Soul sand in patches on top, magma glowing out of the
                        // underside: the island lights itself from below.
                        let n = noise.perlin3(fx * 0.22, y as f32 * 0.22, fz * 0.22);
                        if from_top == 0 && n > 0.22 {
                            SOUL_SAND
                        } else if y == bottom && n > 0.05 {
                            MAGMA
                        } else {
                            NETHERRACK
                        }
                    }
                };
                world.set(x, y, z, block);
            }
            surface.set(x, z, top);
        }
    }
    surface
}

/// The nether island: a lava pool, a lit portal on a brick platform, and the
/// ruins of whatever built it.
fn nether_island(world: &mut World, noise: &Noise, ground: &Surface) {
    let (cx, cz) = NETHER;
    let base = ground.get(cx, cz).unwrap_or(SURFACE_LEVEL as i32);

    // Flatten the middle so the platform and the pool sit level.
    for z in cz - 6..=cz + 6 {
        for x in cx - 6..=cx + 6 {
            let Some(top) = ground.get(x, z) else { continue };
            for y in base + 1..=top {
                world.set(x, y, z, AIR);
            }
            for y in top..=base {
                world.set(x, y, z, NETHERRACK);
            }
        }
    }

    // Lava pool, sunk one block so it reads as liquid held in the rock.
    for z in cz + 1..=cz + 4 {
        for x in cx - 5..=cx - 1 {
            let edge = x == cx - 5 || x == cx - 1 || z == cz + 1 || z == cz + 4;
            if edge && noise.value(x, base, z) > 0.4 {
                continue;
            }
            world.set(x, base, z, LAVA);
            world.set(x, base - 1, z, MAGMA);
        }
    }

    // The platform, and the portal standing on it: obsidian frame, four by five,
    // with the portal plane inside it.
    let (px, pz) = (cx + 1, cz - 1);
    for z in pz - 2..=pz + 2 {
        for x in px - 3..=px + 3 {
            world.set(x, base, z, NETHER_BRICKS);
        }
    }
    for x in px - 2..=px + 1 {
        world.set(x, base + 1, pz, OBSIDIAN);
        world.set(x, base + 5, pz, OBSIDIAN);
    }
    for y in base + 1..=base + 5 {
        world.set(px - 3, y, pz, OBSIDIAN);
        world.set(px + 2, y, pz, OBSIDIAN);
    }
    for y in base + 2..=base + 4 {
        for x in px - 2..=px + 1 {
            world.set(x, y, pz, NETHER_PORTAL);
        }
    }

    // A patch of soul sand by the pool, and an arch of nether brick that the
    // portal's platform ran under.
    for z in cz - 4..=cz - 2 {
        for x in cx + 3..=cx + 5 {
            if noise.value(x, base + 1, z) > 0.35 {
                world.set(x, base, z, SOUL_SAND);
            }
        }
    }
    for y in base + 1..=base + 3 {
        world.set(cx - 6, y, cz - 3, NETHER_BRICKS);
        world.set(cx - 2, y, cz - 3, NETHER_BRICKS);
    }
    for x in cx - 6..=cx - 2 {
        world.set(x, base + 4, cz - 3, NETHER_BRICKS);
    }

    // Broken nether-brick pillars, and glowstone caught in the rock.
    for (dx, dz, height) in [(-4i32, -4i32, 4), (4, -3, 3), (5, 3, 2)] {
        for y in base + 1..=base + height {
            world.set(cx + dx, y, cz + dz, NETHER_BRICKS);
        }
        world.set(cx + dx, base + height + 1, cz + dz, MAGMA);
    }
    for z in cz - 6..=cz + 6 {
        for x in cx - 6..=cx + 6 {
            let Some(top) = ground.get(x, z) else { continue };
            if world.get(x, top, z) == NETHERRACK
                && noise.perlin3(x as f32 * 0.4, top as f32 * 0.4, z as f32 * 0.4) > 0.55
            {
                world.set(x, top, z, GLOWSTONE);
            }
        }
    }
}

/// The farm island: a plank house, a watered field of wheat, hay, and a fenced
/// pen with a cow and two sheep.
fn farm_island(world: &mut World, noise: &Noise, ground: &Surface) {
    let (cx, cz) = FARM;
    let base = ground.get(cx, cz).unwrap_or(SURFACE_LEVEL as i32);

    // One level for everything the farm stands on.
    for z in cz - 8..=cz + 8 {
        for x in cx - 8..=cx + 8 {
            let Some(top) = ground.get(x, z) else { continue };
            for y in base + 1..=top {
                world.set(x, y, z, AIR);
            }
            for y in top..=base {
                world.set(x, y, z, if y == base { GRASS } else { DIRT });
            }
        }
    }

    house(world, (cx + 2, base, cz - 4));
    field(world, (cx - 5, base, cz - 1));
    pen(world, (cx + 1, base, cz + 4));

    // Hay stacked beside the house, and one bale knocked over.
    for y in base + 1..=base + 2 {
        for z in cz - 1..=cz {
            for x in cx + 6..=cx + 7 {
                world.set(x, y, z, HAY_BLOCK);
            }
        }
    }
    world.set(cx + 5, base + 1, cz + 1, HAY_BLOCK);

    // A couple of ordinary trees, away from the buildings.
    for (dx, dz) in [(-6i32, 6i32), (-7, -6)] {
        let (x, z) = (cx + dx, cz + dz);
        let Some(top) = ground.get(x, z) else { continue };
        if world.get(x, top, z) != GRASS {
            continue;
        }
        let height = 4 + (noise.value(x, 0, z) * 2.0) as i32;
        for y in top + 1..=top + height {
            world.set(x, y, z, OAK_LOG);
        }
        for dy in -2..=1 {
            let radius = if dy <= -1 { 2 } else { 1 };
            for lz in -radius..=radius {
                for lx in -radius..=radius {
                    if lx * lx + lz * lz + dy * dy > radius * radius + 2 {
                        continue;
                    }
                    let (bx, by, bz) = (x + lx, top + height + dy, z + lz);
                    if world.get(bx, by, bz) == AIR {
                        world.set(bx, by, bz, OAK_LEAVES);
                    }
                }
            }
        }
    }
}

/// A small plank house: log corners, a glass window on each side, a doorway
/// facing the bridge, and a roof of slabs stepping to a ridge.
fn house(world: &mut World, at: (i32, i32, i32)) {
    let (cx, base, cz) = at;
    let (w, d) = (3, 3); // half extents: a seven by seven house
    let floor = base + 1;

    for z in cz - d..=cz + d {
        for x in cx - w..=cx + w {
            world.set(x, base, z, OAK_PLANKS);
        }
    }

    for y in floor..floor + 3 {
        for z in cz - d..=cz + d {
            for x in cx - w..=cx + w {
                let edge = x == cx - w || x == cx + w || z == cz - d || z == cz + d;
                if !edge {
                    continue;
                }
                let corner = (x == cx - w || x == cx + w) && (z == cz - d || z == cz + d);
                // The doorway faces west, towards the bridge and the main island.
                let door = x == cx - w && z == cz && y < floor + 2;
                let window = !corner
                    && y == floor + 1
                    && ((x == cx + w && z == cz) || (z == cz - d && x == cx) || (z == cz + d && x == cx));
                let block = if door {
                    AIR
                } else if corner {
                    OAK_LOG
                } else if window {
                    GLASS
                } else {
                    OAK_PLANKS
                };
                world.set(x, y, z, block);
            }
        }
    }

    // Roof: two courses of slabs stepping in, then a ridge beam.
    for (step, y) in (floor + 3..floor + 5).enumerate() {
        let inset = step as i32;
        for z in cz - d + inset..=cz + d - inset {
            for x in cx - w - 1 + inset..=cx + w + 1 - inset {
                world.set(x, y, z, OAK_SLAB);
            }
        }
    }
    for x in cx - w..=cx + w {
        world.set(x, floor + 5, cz, OAK_SLAB);
    }

    // A lantern by the door, so the farm reads at night too.
    world.set(cx - w - 1, floor + 1, cz - 1, GLOWSTONE);
}

/// A watered field: farmland around a channel, wheat on top of it.
fn field(world: &mut World, at: (i32, i32, i32)) {
    let (cx, base, cz) = at;
    for z in cz - 3..=cz + 3 {
        for x in cx - 3..=cx + 3 {
            if z == cz {
                // The channel down the middle is what keeps farmland moist.
                world.set(x, base, z, WATER);
                continue;
            }
            world.set(x, base, z, FARMLAND);
            // Every third row is left bare, so the plot reads as rows of crops
            // rather than one green mat.
            if (z - cz).rem_euclid(3) != 0 {
                world.set(x, base + 1, z, WHEAT);
            }
        }
    }
    // A fence around the plot, with a gap facing the house.
    for z in cz - 4..=cz + 4 {
        for x in cx - 4..=cx + 4 {
            let edge = x == cx - 4 || x == cx + 4 || z == cz - 4 || z == cz + 4;
            if edge && !(x == cx + 4 && z == cz) {
                world.set(x, base + 1, z, OAK_FENCE);
            }
        }
    }
}

/// A fenced pen with a cow and two sheep, all built out of blocks.
fn pen(world: &mut World, at: (i32, i32, i32)) {
    let (cx, base, cz) = at;
    for z in cz - 3..=cz + 3 {
        for x in cx - 4..=cx + 4 {
            let edge = x == cx - 4 || x == cx + 4 || z == cz - 3 || z == cz + 3;
            if edge && !(z == cz - 3 && x == cx) {
                world.set(x, base + 1, z, OAK_FENCE);
            }
        }
    }
    // Spread out: crowded together the three of them read as one pile of cubes.
    cow(world, (cx - 3, base + 1, cz - 1));
    sheep(world, (cx + 1, base + 1, cz - 2));
    sheep(world, (cx + 1, base + 1, cz + 1));
}

/// A cow: brown body with a white patch, a black head and four black legs.
fn cow(world: &mut World, at: (i32, i32, i32)) {
    let (x, y, z) = at;
    for dz in 0..2 {
        for dx in 0..3 {
            world.set(x + dx, y + 1, z + dz, BROWN_WOOL);
        }
    }
    world.set(x + 1, y + 1, z, WHITE_WOOL);
    world.set(x + 2, y + 1, z + 1, WHITE_WOOL);
    // Head, one block forward and one higher.
    world.set(x + 3, y + 2, z, BLACK_CONCRETE);
    world.set(x + 3, y + 2, z + 1, BLACK_CONCRETE);
    for (dx, dz) in [(0, 0), (0, 1), (2, 0), (2, 1)] {
        world.set(x + dx, y, z + dz, BLACK_CONCRETE);
    }
}

/// A sheep: a white wool block of a body on black legs, with a black face.
fn sheep(world: &mut World, at: (i32, i32, i32)) {
    let (x, y, z) = at;
    for dx in 0..2 {
        world.set(x + dx, y + 1, z, WHITE_WOOL);
    }
    world.set(x + 2, y + 1, z, BLACK_CONCRETE);
    world.set(x, y, z, BLACK_CONCRETE);
    world.set(x + 1, y, z, BLACK_CONCRETE);
}

/// A plank bridge from a neighbour to the main island: a deck five wide, fence
/// railings, slab lips along the sides and posts hanging under it.
///
/// `direction` is the way to walk from the neighbour's centre to reach the main
/// island, and `fill` is what the landing ramp is packed with on the far side.
fn bridge(world: &mut World, ground: &Surface, from: (i32, i32), direction: i32, fill: Block) {
    let (cx, cz) = from;

    // The neighbour's rim: the last column of its own ground.
    let mut rim = cx;
    while ground.get(rim + direction, cz).is_some() {
        rim += direction;
    }
    // The main island's shore, walking on until the world has ground again.
    let mut shore = rim;
    let limit = WORLD_X as i32;
    loop {
        shore += direction;
        if shore <= 0 || shore >= limit {
            return;
        }
        if top_at(world, shore, cz).is_some() {
            break;
        }
    }

    let rim_top = ground.get(rim, cz).unwrap_or(SURFACE_LEVEL as i32);
    let shore_top = top_at(world, shore, cz).unwrap_or(SURFACE_LEVEL as i32);
    let deck = rim_top.max(shore_top) + 1;

    let (lo, hi) = if direction > 0 { (rim, shore) } else { (shore, rim) };
    for x in lo..=hi {
        for dz in -2..=2i32 {
            let z = cz + dz;
            // The walkway is planks; the two outer strips are slabs, so the deck
            // has a lip instead of a square edge.
            let block = if dz.abs() == 2 { OAK_SLAB } else { OAK_PLANKS };
            world.set(x, deck, z, block);
            if dz.abs() == 2 {
                world.set(x, deck + 1, z, OAK_FENCE);
            } else {
                // Clear whatever the terrain left in the way of the walkway.
                for y in deck + 1..=deck + 3 {
                    if world.get(x, y, z) != AIR {
                        world.set(x, y, z, AIR);
                    }
                }
            }
        }
        // Posts under the deck every few blocks, hanging into the void.
        if (x - lo) % 4 == 0 {
            for dz in [-2i32, 2] {
                for y in deck - 4..deck {
                    if world.get(x, y, cz + dz) == AIR {
                        world.set(x, y, cz + dz, OAK_FENCE);
                    }
                }
            }
        }
    }

    // Both ends: level the last few columns of ground so the deck lands flush
    // instead of burying itself in a slope.
    for (end, step) in [(rim, -direction), (shore, direction)] {
        for i in 0..4 {
            let x = end + step * i;
            for dz in -2..=2 {
                let z = cz + dz;
                if top_at(world, x, z).is_none() {
                    continue;
                }
                for y in deck + 1..=deck + 3 {
                    world.set(x, y, z, AIR);
                }
                let mut y = deck - 1;
                while y > 0 && world.get(x, y, z) == AIR {
                    world.set(x, y, z, fill);
                    y -= 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structures;
    use crate::terrain;

    fn built(seed: u32) -> World {
        let mut island = terrain::generate(seed);
        structures::place_all(&mut island);
        compose(&island, seed)
    }

    fn count(world: &World, block: Block) -> usize {
        let mut n = 0;
        for y in 0..world.size[1] as i32 {
            for z in 0..world.size[2] as i32 {
                for x in 0..world.size[0] as i32 {
                    if world.get(x, y, z) == block {
                        n += 1;
                    }
                }
            }
        }
        n
    }

    #[test]
    fn the_main_island_survives_the_move() {
        let mut island = terrain::generate(7);
        structures::place_all(&mut island);
        let before = island.world.solid_count();
        let world = compose(&island, 7);
        // Everything that was in the island's own box is in the shared world,
        // shifted by the origin, and the neighbours only add to it.
        assert!(world.solid_count() > before);
        for (x, z) in [(24, 24), (25, 12), (13, 38)] {
            for y in 0..terrain::HEIGHT as i32 {
                assert_eq!(
                    island.world.get(x, y, z),
                    world.get(x + ORIGIN.0, y, z + ORIGIN.1),
                    "column {x},{z} did not survive the stamp"
                );
            }
        }
    }

    #[test]
    fn both_neighbours_exist_for_every_seed() {
        for seed in [1, 9, 2024] {
            let world = built(seed);
            assert!(
                top_at(&world, NETHER.0, NETHER.1).is_some(),
                "seed {seed}: no nether island"
            );
            assert!(
                top_at(&world, FARM.0, FARM.1).is_some(),
                "seed {seed}: no farm island"
            );
            assert!(count(&world, NETHER_PORTAL) >= 12, "seed {seed}: no portal");
            assert!(count(&world, LAVA) >= 8, "seed {seed}: no lava");
            assert!(count(&world, WHEAT) >= 20, "seed {seed}: no crops");
            assert!(count(&world, HAY_BLOCK) >= 8, "seed {seed}: no hay");
            assert!(count(&world, OAK_FENCE) >= 40, "seed {seed}: no fences");
            assert!(count(&world, OAK_SLAB) >= 40, "seed {seed}: no slabs");
        }
    }

    #[test]
    fn the_islands_do_not_touch_each_other() {
        let world = built(3);
        // A gap on the centre line, at ground level, on both sides: the bridge
        // crosses open air rather than a land connection.
        for x in [NETHER.0 + 16, FARM.0 - 16] {
            let column: Vec<Block> = (0..terrain::HEIGHT as i32)
                .map(|y| world.get(x, y, NETHER.1))
                .filter(|&b| b != AIR)
                .collect();
            assert!(
                column.iter().all(|b| matches!(
                    *b,
                    OAK_PLANKS | OAK_SLAB | OAK_FENCE
                )),
                "the gap at x={x} is not open air: {column:?}"
            );
        }
    }

    #[test]
    fn each_bridge_reaches_both_shores() {
        let world = built(2024);
        let deck = |x: i32| {
            (0..terrain::HEIGHT as i32)
                .any(|y| matches!(world.get(x, y, NETHER.1), OAK_PLANKS | OAK_SLAB))
        };
        // Walk the centre line from one island to the other: no gap in the deck
        // between the two shores.
        for (from, to) in [(NETHER.0 + 10, ORIGIN.0), (ORIGIN.0 + 48, FARM.0 - 10)] {
            let missing: Vec<i32> = (from..to)
                .filter(|&x| top_at(&world, x, NETHER.1).is_none())
                .collect();
            assert!(missing.is_empty(), "gap in the crossing at {missing:?}");
            assert!(deck((from + to) / 2), "no deck in the middle of the span");
        }
    }
}
