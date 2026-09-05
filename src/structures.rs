//! Hand-placed structures on top of the generated terrain.
//!
//! The terrain is procedural; the shrine is not. A landmark with straight lines is
//! what gives the diorama a subject, and it is where the refractive, reflective and
//! emissive materials earn their place: a crystal gate lit from behind, framed in
//! quartz with gold and iron trim, ringed by glowstone lanterns.

use crate::blocks::*;
use crate::terrain::{Island, SIZE};
use crate::world::World;

/// Flatten a footprint to one height so the shrine does not float or sink.
fn level(island: &mut Island, x0: i32, z0: i32, w: i32, d: i32) -> i32 {
    let mut sum = 0;
    let mut count = 0;
    for z in z0..z0 + d {
        for x in x0..x0 + w {
            if let Some(top) = island.surface_at(x, z) {
                sum += top;
                count += 1;
            }
        }
    }
    if count == 0 {
        return crate::terrain::SURFACE_LEVEL as i32;
    }
    let target = sum / count;

    for z in z0..z0 + d {
        for x in x0..x0 + w {
            let Some(top) = island.surface_at(x, z) else {
                continue;
            };
            // Cut down what stands too high, fill up what sits too low.
            for y in target + 1..=top {
                island.world.set(x, y, z, AIR);
            }
            for y in top..=target {
                island.world.set(x, y, z, if y == target { DIRT } else { STONE });
            }
            island.set_surface(x, z, target);
        }
    }
    target
}

/// Build the shrine and return the block coordinates of every light it adds.
pub fn place_shrine(island: &mut Island) {
    let (x0, z0) = (SIZE as i32 / 2 - 4, SIZE as i32 / 2 - 3);
    let (w, d) = (9, 7);
    let base = level(island, x0, z0, w, d);
    let world = &mut island.world;

    // Platform: quartz floor with a polished rim.
    for z in z0..z0 + d {
        for x in x0..x0 + w {
            let edge = x == x0 || x == x0 + w - 1 || z == z0 || z == z0 + d - 1;
            world.set(x, base, z, if edge { QUARTZ_PILLAR } else { QUARTZ });
        }
    }

    // Two pillars flanking the gate, capped in gold.
    let (px, pz) = (x0 + 2, z0 + 3);
    let gate_height = 5;
    for (side, x) in [(0, px), (1, px + 4)] {
        for y in base + 1..=base + gate_height {
            world.set(x, y, pz, QUARTZ_PILLAR);
        }
        // Iron trim, offset between the two pillars so they are not identical.
        world.set(x, base + 2 + side, pz, IRON_BLOCK);
    }

    // Lintel across the top: gold at both ends and in the middle, quartz between.
    for x in px..=px + 4 {
        let gold = x == px || x == px + 2 || x == px + 4;
        world.set(
            x,
            base + gate_height + 1,
            pz,
            if gold { GOLD_BLOCK } else { QUARTZ },
        );
    }

    // The gate itself: a slab of smoky crystal, three wide and four tall.
    for x in px + 1..=px + 3 {
        for y in base + 1..=base + gate_height {
            world.set(x, y, pz, PORTAL);
        }
    }

    // Glowstone behind the gate so light pours through the crystal, plus lanterns
    // at the platform corners.
    for x in px + 1..=px + 3 {
        world.set(x, base + 2, pz + 1, GLOWSTONE);
    }
    for (x, z) in [
        (x0, z0),
        (x0 + w - 1, z0),
        (x0, z0 + d - 1),
        (x0 + w - 1, z0 + d - 1),
    ] {
        world.set(x, base + 1, z, QUARTZ_PILLAR);
        world.set(x, base + 2, z, GLOWSTONE);
    }

    // A lit glass panel in the floor in front of the gate: glowstone buried under
    // transparent blocks, so the refraction has something bright directly behind
    // it and the approach to the gate glows from below.
    for x in px + 1..=px + 3 {
        for z in pz - 2..=pz - 1 {
            world.set(x, base - 1, z, GLOWSTONE);
            world.set(x, base, z, GLASS);
        }
    }

    // A plank path running off the platform towards the pond.
    let mut x = x0 - 1;
    let mut z = z0 + d / 2;
    for _ in 0..10 {
        if let Some(top) = island.surface_at(x, z) {
            island.world.set(x, top, z, OAK_PLANKS);
        }
        x -= 1;
        if x % 3 == 0 {
            z += 1;
        }
        if x < 1 || z >= SIZE as i32 - 1 {
            break;
        }
    }
}

/// Emissive blocks, merged into clusters.
///
/// Neighbouring emitters (the three glowstone blocks behind the gate, the gate's
/// own twelve crystal blocks) are indistinguishable once their light lands on a
/// surface, so they are merged into one light at their centroid. Fewer lights means
/// fewer shadow rays per shaded point, which is the dominant cost.
pub fn collect_lights(world: &World) -> Vec<(crate::math::Vec3, Block)> {
    use crate::math::vec3;

    let mut raw: Vec<(crate::math::Vec3, Block)> = Vec::new();
    for y in 0..world.size[1] as i32 {
        for z in 0..world.size[2] as i32 {
            for x in 0..world.size[0] as i32 {
                let block = world.get(x, y, z);
                if is_emissive(block) {
                    raw.push((vec3(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5), block));
                }
            }
        }
    }

    /// Emitters closer than this to a cluster's centre join it.
    const MERGE_RADIUS: f32 = 2.6;
    let mut clusters: Vec<(crate::math::Vec3, Block, f32)> = Vec::new();
    for (position, block) in raw {
        match clusters
            .iter_mut()
            .find(|(centre, kind, _)| *kind == block && (*centre - position).length() < MERGE_RADIUS)
        {
            Some((centre, _, count)) => {
                // Running mean, so the cluster sits at the centroid of its blocks.
                *count += 1.0;
                *centre = *centre + (position - *centre) / *count;
            }
            None => clusters.push((position, block, 1.0)),
        }
    }
    clusters.into_iter().map(|(p, b, _)| (p, b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain;

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
    fn the_shrine_appears_for_every_seed() {
        for seed in [1, 9, 2024, 77777] {
            let mut island = terrain::generate(seed);
            place_shrine(&mut island);
            assert!(count(&island.world, PORTAL) >= 12, "seed {seed}: no gate");
            assert!(count(&island.world, QUARTZ) > 20, "seed {seed}: no platform");
            assert!(count(&island.world, GOLD_BLOCK) >= 3, "seed {seed}: no gold");
            assert!(count(&island.world, IRON_BLOCK) >= 2, "seed {seed}: no iron");
            assert!(
                count(&island.world, GLOWSTONE) >= 7,
                "seed {seed}: not enough lanterns"
            );
            assert!(
                count(&island.world, GLASS) >= 6,
                "seed {seed}: the lit glass floor panel is missing"
            );
        }
    }

    #[test]
    fn the_platform_sits_on_level_ground() {
        let mut island = terrain::generate(5);
        place_shrine(&mut island);
        let (x0, z0) = (SIZE as i32 / 2 - 4, SIZE as i32 / 2 - 3);
        // Leveling rewrites the surface map, so every footprint column must now
        // report the same height.
        let heights: Vec<i32> = (z0..z0 + 7)
            .flat_map(|z| (x0..x0 + 9).map(move |x| (x, z)))
            .filter_map(|(x, z)| island.surface_at(x, z))
            .collect();
        assert_eq!(heights.len(), 63, "the footprint should be fully on land");
        let (min, max) = (
            *heights.iter().min().unwrap(),
            *heights.iter().max().unwrap(),
        );
        assert_eq!(min, max, "platform is not level: {min}..{max}");
    }

    #[test]
    fn lights_are_clustered_but_still_cover_every_lantern() {
        let mut island = terrain::generate(3);
        place_shrine(&mut island);
        let lights = collect_lights(&island.world);
        let emitters = count(&island.world, GLOWSTONE) + count(&island.world, PORTAL);

        assert!(lights.len() < emitters, "clustering did nothing");
        // Four corner lanterns are far apart, so they can never merge together.
        assert!(lights.len() >= 5, "clusters collapsed too far: {}", lights.len());

        // Every emissive block must have a cluster near it.
        for y in 0..island.world.size[1] as i32 {
            for z in 0..island.world.size[2] as i32 {
                for x in 0..island.world.size[0] as i32 {
                    let block = island.world.get(x, y, z);
                    if !is_emissive(block) {
                        continue;
                    }
                    let p = crate::math::vec3(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                    assert!(
                        lights
                            .iter()
                            .any(|(c, b)| *b == block && (*c - p).length() < 3.0),
                        "emitter at {x},{y},{z} has no cluster"
                    );
                }
            }
        }
    }

    #[test]
    fn the_gate_is_backlit() {
        let mut island = terrain::generate(11);
        place_shrine(&mut island);
        // Glowstone must sit directly behind the crystal, or the refraction has
        // nothing to carry.
        let mut backlit = 0;
        for y in 0..island.world.size[1] as i32 {
            for z in 0..island.world.size[2] as i32 {
                for x in 0..island.world.size[0] as i32 {
                    if island.world.get(x, y, z) == PORTAL
                        && island.world.get(x, y - 1, z + 1) == GLOWSTONE
                    {
                        backlit += 1;
                    }
                }
            }
        }
        assert!(backlit >= 3, "gate is not backlit ({backlit})");
    }
}
