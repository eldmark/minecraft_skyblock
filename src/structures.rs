//! Hand-placed structures on top of the generated terrain.
//!
//! The terrain is procedural; the buildings are not. Straight lines against an
//! organic landscape are what turn a heightmap into a diorama, and they are where
//! the refractive, reflective and emissive materials earn their place.
//!
//! Following the reference: a columned temple with a glowing gate on the far
//! terrace, a great tree on the rocky outcrop, an arched bridge over the stream,
//! and a small ruin of three columns on the near side.

use crate::blocks::*;
use crate::math::{vec3, Vec3};
use crate::terrain::{Island, HILL, SIZE, WATER_LEVEL};
use crate::world::World;

/// Where the temple stands, and how big its stylobate is.
const TEMPLE: (i32, i32) = (25, 12);
const TEMPLE_W: i32 = 13;
const TEMPLE_D: i32 = 11;

/// The ruin on the near side of the stream.
const RUIN: (i32, i32) = (13, 38);

pub fn place_all(island: &mut Island) {
    let base = level(island, TEMPLE.0, TEMPLE.1, TEMPLE_W, TEMPLE_D, 1);
    let ridge = temple(island, TEMPLE.0, TEMPLE.1, base);
    dragon(island, TEMPLE.0 + TEMPLE_W / 2, TEMPLE.1 + TEMPLE_D / 2, ridge + 1);
    great_tree(island);
    bridge(island);
    ruin(island);
    path(island, base);
}

/// Flatten a footprint to one height so a building does not float or sink.
/// `margin` widens the levelled area so the ground meets the walls cleanly.
fn level(island: &mut Island, x0: i32, z0: i32, w: i32, d: i32, margin: i32) -> i32 {
    let (x0, z0) = (x0 - margin, z0 - margin);
    let (w, d) = (w + margin * 2, d + margin * 2);

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
    // Never level below the water line, or the stream would flood the terrace.
    let target = (sum / count).max(WATER_LEVEL + 1);

    for z in z0..z0 + d {
        for x in x0..x0 + w {
            let Some(top) = island.surface_at(x, z) else {
                continue;
            };
            for y in target + 1..=top {
                island.world.set(x, y, z, AIR);
            }
            for y in top..=target {
                island
                    .world
                    .set(x, y, z, if y == target { GRASS } else { DIRT });
            }
            island.set_surface(x, z, target);
        }
    }
    target
}

/// The temple: stepped stylobate, a colonnade, entablature, a stepped pediment
/// roof, and the crystal gate glowing in the cella behind the columns.
///
/// Returns the height of the roof ridge, which is where the dragon perches.
fn temple(island: &mut Island, x0: i32, z0: i32, base: i32) -> i32 {
    let world = &mut island.world;
    let (w, d) = (TEMPLE_W, TEMPLE_D);
    let (x1, z1) = (x0 + w - 1, z0 + d - 1);

    // Two steps around the platform, then the floor itself.
    for (ring, y) in [(1, base), (0, base + 1)] {
        for z in z0 - ring..=z1 + ring {
            for x in x0 - ring..=x1 + ring {
                let edge = x == x0 - ring || x == x1 + ring || z == z0 - ring || z == z1 + ring;
                world.set(x, y, z, if edge { QUARTZ_BRICKS } else { QUARTZ });
            }
        }
    }
    let floor = base + 1;

    // Colonnade: fluted columns on every side, one block in from the edge.
    let column_top = floor + 6;
    let mut columns = Vec::new();
    for x in (x0 + 1..=x1 - 1).step_by(3) {
        columns.push((x, z0 + 1));
        columns.push((x, z1 - 1));
    }
    for z in (z0 + 4..=z1 - 4).step_by(3) {
        columns.push((x0 + 1, z));
        columns.push((x1 - 1, z));
    }
    for (x, z) in &columns {
        world.set(*x, floor + 1, *z, CHISELED_QUARTZ); // base
        for y in floor + 2..column_top {
            world.set(*x, y, *z, QUARTZ_PILLAR);
        }
        world.set(*x, column_top, *z, CHISELED_QUARTZ); // capital
    }

    // Entablature: a solid band resting on the capitals, with a gold frieze on the
    // front so the eye lands on the entrance.
    for z in z0 + 1..=z1 - 1 {
        for x in x0 + 1..=x1 - 1 {
            let edge = x == x0 + 1 || x == x1 - 1 || z == z0 + 1 || z == z1 - 1;
            if edge {
                world.set(x, column_top + 1, z, QUARTZ_BRICKS);
            }
        }
    }
    // The entrance faces +z, towards the bridge and the path, so that is where
    // the frieze, the steps and the glow belong.
    for x in x0 + 4..=x1 - 4 {
        world.set(x, column_top + 1, z1 - 1, GOLD_BLOCK);
    }

    // Roof: courses stepping inward along x only, so the ridge runs front to back
    // and the short faces read as pediments, the way a Greek temple does.
    // Four courses, then a flat ridge: a shallow pitch reads better at this scale
    // than a full pyramid, which would swallow the pediment.
    let courses = 3;
    for course in 0..courses {
        let y = column_top + 2 + course;
        let inset = course;
        for z in z0 + 1..=z1 - 1 {
            for x in x0 + 1 + inset..=x1 - 1 - inset {
                let edge = x == x0 + 1 + inset || x == x1 - 1 - inset;
                // The eaves of each course are brick, the field is smooth quartz.
                world.set(x, y, z, if edge { QUARTZ_BRICKS } else { QUARTZ });
            }
        }
    }
    // Ridge beam along the top, in gold, so the roofline catches the sun.
    for z in z0 + 2..=z1 - 2 {
        world.set(x0 + w / 2, column_top + 2 + courses, z, GOLD_BLOCK);
    }

    // Cella walls behind the colonnade, leaving the +z front open.
    for z in z0 + 2..=z1 - 5 {
        for y in floor + 1..=column_top {
            world.set(x0 + 3, y, z, QUARTZ);
            world.set(x1 - 3, y, z, QUARTZ);
        }
    }
    for x in x0 + 3..=x1 - 3 {
        for y in floor + 1..=column_top {
            world.set(x, y, z0 + 2, QUARTZ);
        }
    }

    // The gate: a slab of smoky crystal in an arch of chiselled quartz, lit from
    // behind so the refraction has something to carry.
    // Two blocks in from the colonnade, so the glow is visible from the front.
    let gate_z = z1 - 4;
    let (gx0, gx1) = (x0 + 5, x1 - 5);
    for x in gx0 - 1..=gx1 + 1 {
        world.set(x, floor + 6, gate_z, CHISELED_QUARTZ);
    }
    for y in floor + 1..=floor + 5 {
        world.set(gx0 - 1, y, gate_z, CHISELED_QUARTZ);
        world.set(gx1 + 1, y, gate_z, CHISELED_QUARTZ);
    }
    for x in gx0..=gx1 {
        for y in floor + 1..=floor + 5 {
            world.set(x, y, gate_z, PORTAL);
        }
        // Lit from deeper inside the cella, so the light pours out through the
        // crystal towards the viewer.
        world.set(x, floor + 2, gate_z - 1, GLOWSTONE);
        world.set(x, floor + 4, gate_z - 1, GLOWSTONE);
        world.set(x, floor + 3, gate_z - 2, GLOWSTONE);
    }

    // Braziers flanking the steps, and a lit glass panel on the threshold.
    for x in [x0 - 1, x1 + 1] {
        world.set(x, base + 1, z1 + 1, QUARTZ_BRICKS);
        world.set(x, base + 2, z1 + 1, CHISELED_QUARTZ);
        world.set(x, base + 3, z1 + 1, GLOWSTONE);
    }
    for x in gx0..=gx1 {
        world.set(x, floor - 1, z1 - 2, GLOWSTONE);
        world.set(x, floor, z1 - 2, GLASS);
    }

    column_top + 2 + courses
}

/// A dragon reared up on the temple's ridge, built out of blocks.
///
/// Obsidian for the hide — dark volcanic glass, shiny enough that it picks up every
/// lantern around the temple and the sunset behind it — with an emerald spine and
/// belly, redstone in the jaw and glowstone eyes that carry it through the night.
///
/// Two things decide whether it reads as an animal at this scale. It stands
/// **across** the temple, along x, so the default view sees its whole profile;
/// an earlier version aligned with the view axis read as a totem pole. And every
/// part is a solid box, never a line of single blocks, which at this size looks
/// like a stick rather than a limb.
fn dragon(island: &mut Island, cx: i32, cz: i32, y: i32) {
    let world = &mut island.world;

    fn box_of(world: &mut World, from: (i32, i32, i32), to: (i32, i32, i32), block: Block) {
        for yy in from.1..=to.1 {
            for zz in from.2..=to.2 {
                for xx in from.0..=to.0 {
                    world.set(xx, yy, zz, block);
                }
            }
        }
    }

    // Hind legs: thick, planted on the ridge, carrying the reared-up body.
    for dz in [-1i32, 1] {
        box_of(
            world,
            (cx + 2, y, cz + dz),
            (cx + 3, y + 2, cz + dz),
            OBSIDIAN,
        );
        // Foot.
        world.set(cx + 1, y, cz + dz, OBSIDIAN);
    }

    // Body: three deep, three tall, sloping up towards the chest.
    box_of(world, (cx - 2, y + 2, cz - 1), (cx + 4, y + 4, cz + 1), OBSIDIAN);
    // Belly plates.
    box_of(world, (cx - 1, y + 2, cz - 1), (cx + 3, y + 2, cz + 1), EMERALD_BLOCK);
    // Spine along the back.
    box_of(world, (cx - 1, y + 5, cz), (cx + 3, y + 5, cz), EMERALD_BLOCK);

    // Chest and shoulders, where the neck and the wings meet the body.
    box_of(world, (cx - 4, y + 3, cz - 1), (cx - 2, y + 5, cz + 1), OBSIDIAN);

    // Front legs, shorter, tucked under the chest.
    for dz in [-1i32, 1] {
        box_of(world, (cx - 3, y + 1, cz + dz), (cx - 3, y + 2, cz + dz), OBSIDIAN);
        world.set(cx - 4, y + 1, cz + dz, OBSIDIAN);
    }

    // Neck: two thick, curving up and forward over the temple's entrance.
    // The neck climbs more than it reaches: in the reference the head sits over
    // the chest, not out in front of it.
    let neck = [(cx - 5, y + 5), (cx - 6, y + 6), (cx - 6, y + 7)];
    for (nx, ny) in neck {
        box_of(world, (nx, ny, cz - 1), (nx, ny + 1, cz + 1), OBSIDIAN);
        world.set(nx, ny + 2, cz, EMERALD_BLOCK); // crest running up the neck
    }

    // Head: a wedge with a jaw that opens forward and down.
    let (hx, hy) = (cx - 8, y + 8);
    box_of(world, (hx, hy, cz - 1), (hx + 2, hy + 1, cz + 1), OBSIDIAN);
    // Snout.
    box_of(world, (hx - 2, hy, cz - 1), (hx - 1, hy, cz + 1), OBSIDIAN);
    // Open jaw, lit from inside.
    box_of(world, (hx - 2, hy - 1, cz), (hx, hy - 1, cz), REDSTONE_BLOCK);
    // Eyes on both cheeks: what is left of the dragon after dark.
    world.set(hx + 1, hy + 1, cz - 1, GLOWSTONE);
    world.set(hx + 1, hy + 1, cz + 1, GLOWSTONE);
    // Horns sweeping back off the skull.
    for step in 0..3 {
        for dz in [-1i32, 1] {
            world.set(hx + 2 + step, hy + 2 + step / 2, cz + dz, OBSIDIAN);
        }
    }

    // Tail: leaves the hips, drops, then sweeps up and back, thinning as it goes.
    let tail = [
        (cx + 5, y + 3, 1),
        (cx + 6, y + 3, 1),
        (cx + 7, y + 4, 0),
        (cx + 8, y + 5, 0),
        (cx + 9, y + 5, 0),
    ];
    for (tx, ty, half) in tail {
        box_of(world, (tx, ty, cz - half), (tx, ty + 1, cz + half), OBSIDIAN);
        world.set(tx, ty + 2, cz, EMERALD_BLOCK);
    }

    // Wings: angular membranes off the shoulders, rising as they reach out. Two
    // courses thick at the root so they read as wings rather than as fins.
    // Wings stay small and swept back, as in the reference: big spread wings
    // covered the body from the default view and the animal disappeared behind
    // its own membranes.
    for side in [-1i32, 1] {
        for step in 1..=3 {
            let z = cz + side * (1 + step);
            let lift = y + 4 + step;
            let reach = 3 - step;
            box_of(world, (cx - 1, lift, z), (cx - 1 + reach, lift, z), OBSIDIAN);
            if step == 1 {
                box_of(world, (cx - 1, lift - 1, z), (cx + 1, lift - 1, z), OBSIDIAN);
            }
            // Red tip at the leading edge, as in the reference build.
            world.set(cx - 1, lift, z, REDSTONE_BLOCK);
        }
    }
}

/// The great tree on the outcrop: a thick trunk with leaning limbs and a wide,
/// irregular canopy — the counterweight to the temple across the island.
fn great_tree(island: &mut Island) {
    let (cx, cz) = (HILL.0 as i32, HILL.1 as i32);
    let Some(ground) = island.surface_at(cx, cz) else {
        return;
    };
    let world = &mut island.world;

    // Roots spilling over the rock.
    for (dx, dz) in [(-2, 0), (2, 0), (0, -2), (0, 2), (-1, -1), (1, 1)] {
        for step in 0..2 {
            world.set(cx + dx, ground + step, cz + dz, OAK_LOG);
        }
    }

    // A 2x2 trunk, tall enough to clear the temple's roofline.
    let height = 11;
    for dy in 0..height {
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            world.set(cx + dx, ground + dy, cz + dz, OAK_LOG);
        }
    }
    let crown = ground + height;

    // Limbs: each one steps outward and upward, so the silhouette is gnarled
    // rather than a lollipop.
    let limbs: [(i32, i32); 5] = [(-1, -1), (1, -1), (-1, 1), (1, 1), (1, 0)];
    let mut tips = Vec::new();
    for (i, (dx, dz)) in limbs.iter().enumerate() {
        let (mut x, mut z) = (cx, cz);
        let mut y = crown - 5 - (i as i32 % 2);
        // Limbs reach further than they rise, which is what makes the crown wide
        // and gnarled instead of a lollipop on a stick.
        for step in 0..6 {
            x += dx;
            z += dz;
            if step % 2 == 0 {
                y += 1;
            }
            world.set(x, y, z, OAK_LOG);
            if step == 3 {
                // A short fork, so no two limbs look alike.
                world.set(x + dz, y, z - dx, OAK_LOG);
            }
        }
        tips.push((x, y, z));
    }
    tips.push((cx, crown, cz));

    // Canopy: a rounded blob per limb tip, slightly flattened and with a ragged
    // edge, so it reads as foliage rather than as a stack of discs.
    for (tx, ty, tz) in tips {
        let radius = 3i32;
        for dy in -2i32..=3 {
            for dz in -radius..=radius {
                for dx in -radius..=radius {
                    // Squashed sphere: wider than it is tall.
                    let d2 = dx * dx + dz * dz + dy * dy;
                    if d2 > radius * radius {
                        continue;
                    }
                    // Ragged rim: drop roughly a third of the outermost shell.
                    if d2 > (radius - 1) * (radius - 1)
                        && (tx * 31 + ty * 17 + tz * 7 + dx * 13 + dy * 5 + dz * 3).rem_euclid(3) == 0
                    {
                        continue;
                    }
                    let (lx, ly, lz) = (tx + dx, ty + dy + 1, tz + dz);
                    if world.get(lx, ly, lz) != AIR {
                        continue;
                    }
                    let flowering = (lx * 7 + ly * 13 + lz * 5).rem_euclid(13) == 0;
                    world.set(
                        lx,
                        ly,
                        lz,
                        if flowering { FLOWERING_LEAVES } else { OAK_LEAVES },
                    );
                }
            }
        }
    }

    // Moss and a lantern at the foot of the tree.
    for (dx, dz) in [(-3, 1), (3, -1), (1, 3), (-2, -3)] {
        if let Some(top) = island.surface_at(cx + dx, cz + dz) {
            island.world.set(cx + dx, top, cz + dz, MOSS);
        }
    }
    if let Some(top) = island.surface_at(cx + 3, cz + 2) {
        island.world.set(cx + 3, top + 1, cz + 2, STONE_BRICKS);
        island.world.set(cx + 3, top + 2, cz + 2, GLOWSTONE);
    }
}

/// An arched bridge over the stream, on the line between the ruin and the temple.
fn bridge(island: &mut Island) {
    // Find the stream where the path wants to cross it.
    let x = 22;
    let mut span: Option<(i32, i32)> = None;
    let mut start = None;
    for z in 0..SIZE as i32 {
        let is_water = island.world.get(x, WATER_LEVEL, z) == WATER;
        match (is_water, start) {
            (true, None) => start = Some(z),
            (false, Some(s)) => {
                span = Some((s, z - 1));
                break;
            }
            _ => {}
        }
    }
    let Some((z_from, z_to)) = span else { return };

    // Land the ends one block onto each bank.
    let (z0, z1) = (z_from - 2, z_to + 2);
    let deck = WATER_LEVEL + 1;
    let mid = (z0 + z1) as f32 * 0.5;
    let half = ((z1 - z0) as f32 * 0.5).max(1.0);

    for z in z0..=z1 {
        // A shallow arch: the deck rises towards the middle of the span.
        let t = 1.0 - ((z as f32 - mid) / half).abs();
        let rise = (t * 2.0).round() as i32;
        let y = deck + rise;

        for dx in -1..=1 {
            island.world.set(x + dx, y, z, STONE_BRICKS);
        }
        // Railings, mossy where the bridge meets the water.
        let rail = if rise > 0 {
            STONE_BRICKS
        } else {
            MOSSY_STONE_BRICKS
        };
        island.world.set(x - 2, y, z, rail);
        island.world.set(x + 2, y, z, rail);
        island.world.set(x - 2, y + 1, z, rail);
        island.world.set(x + 2, y + 1, z, rail);

        // The arch's underside, down to the water.
        for below in 1..=rise + 1 {
            for dx in -1..=1 {
                let y = y - below;
                if island.world.get(x + dx, y, z) == AIR {
                    island.world.set(x + dx, y, z, MOSSY_STONE_BRICKS);
                }
            }
        }
    }

    // Moss creeping over the banks the bridge lands on.
    for z in [z0 - 1, z1 + 1] {
        for dx in -2..=2 {
            if let Some(top) = island.surface_at(x + dx, z) {
                if island.world.get(x + dx, top, z) == GRASS && (x + dx + z) % 2 == 0 {
                    island.world.set(x + dx, top, z, MOSS);
                }
            }
        }
    }

    // Lanterns on the posts at both ends.
    for z in [z0, z1] {
        for dx in [-2, 2] {
            island.world.set(x + dx, deck + 2, z, CHISELED_QUARTZ);
            island.world.set(x + dx, deck + 3, z, GLOWSTONE);
        }
    }
}

/// Three weathered columns and a broken lintel: the ruin in the foreground.
fn ruin(island: &mut Island) {
    let (x0, z0) = RUIN;
    let base = level(island, x0, z0, 5, 4, 1);
    let world = &mut island.world;

    for z in z0..z0 + 4 {
        for x in x0..x0 + 5 {
            let block = if (x + z) % 3 == 0 {
                MOSSY_STONE_BRICKS
            } else {
                STONE_BRICKS
            };
            world.set(x, base, z, block);
        }
    }

    // Columns of decreasing height, as if two had snapped.
    for (i, (x, z)) in [(x0, z0), (x0 + 4, z0), (x0, z0 + 3), (x0 + 4, z0 + 3)]
        .iter()
        .enumerate()
    {
        let height = [5, 4, 2, 3][i];
        for y in base + 1..=base + height {
            world.set(*x, y, *z, QUARTZ_PILLAR);
        }
        if height >= 4 {
            world.set(*x, base + height + 1, *z, CRACKED_STONE_BRICKS);
        }
    }
    // The surviving stretch of lintel between the two tall columns.
    for x in x0..=x0 + 4 {
        world.set(x, base + 6, z0, CRACKED_STONE_BRICKS);
    }

    // Rubble, and a lantern among it.
    for (dx, dz) in [(1, 2), (3, 1), (2, 3)] {
        world.set(x0 + dx, base + 1, z0 + dz, MOSSY_STONE_BRICKS);
    }
    world.set(x0 + 2, base + 1, z0 + 1, STONE_BRICKS);
    world.set(x0 + 2, base + 2, z0 + 1, GLOWSTONE);
}

/// A stone path from the bridge up to the temple steps.
fn path(island: &mut Island, temple_base: i32) {
    let _ = temple_base;
    let (mut x, mut z) = (22, 30);
    for step in 0..40 {
        if x >= SIZE as i32 - 1 || z < 1 {
            break;
        }
        for dx in 0..2 {
            if let Some(top) = island.surface_at(x + dx, z) {
                let block = if (x + z + step) % 5 == 0 {
                    GRAVEL
                } else {
                    STONE_BRICKS
                };
                if island.world.get(x + dx, top, z) != WATER {
                    island.world.set(x + dx, top, z, block);
                }
            }
        }
        // Walk towards the temple's front steps, which face +z.
        if x < TEMPLE.0 + TEMPLE_W / 2 {
            x += 1;
        } else if z > TEMPLE.1 + TEMPLE_D + 1 {
            z -= 1;
        } else {
            break;
        }
    }
}

/// Emissive blocks, merged into clusters.
///
/// Neighbouring emitters are indistinguishable once their light lands on a
/// surface, so they are merged into one light at their centroid. Fewer lights
/// means fewer shadow rays per shaded point, which is the dominant cost.
pub fn collect_lights(world: &World) -> Vec<(Vec3, Block)> {
    let mut raw: Vec<(Vec3, Block)> = Vec::new();
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
    let mut clusters: Vec<(Vec3, Block, f32)> = Vec::new();
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

    fn built(seed: u32) -> Island {
        let mut island = terrain::generate(seed);
        place_all(&mut island);
        island
    }

    #[test]
    fn every_structure_appears_for_every_seed() {
        for seed in [1, 9, 2024, 77777] {
            let island = built(seed);
            let w = &island.world;
            assert!(count(w, PORTAL) >= 15, "seed {seed}: no gate");
            assert!(count(w, QUARTZ) > 100, "seed {seed}: no temple");
            assert!(count(w, QUARTZ_PILLAR) > 20, "seed {seed}: no columns");
            assert!(count(w, GOLD_BLOCK) >= 3, "seed {seed}: no frieze");
            assert!(count(w, GLOWSTONE) >= 10, "seed {seed}: too few lanterns");
            assert!(count(w, GLASS) >= 3, "seed {seed}: no lit threshold");
            assert!(count(w, STONE_BRICKS) > 20, "seed {seed}: no bridge or ruin");
            assert!(count(w, OAK_LOG) > 40, "seed {seed}: no great tree");
            // The dragon is the only obsidian in the scene, and it is big.
            assert!(count(w, OBSIDIAN) > 60, "seed {seed}: no dragon");
        }
    }

    #[test]
    fn the_temple_terrace_is_level_and_above_the_water() {
        let island = built(5);
        let heights: Vec<i32> = (TEMPLE.1..TEMPLE.1 + TEMPLE_D)
            .flat_map(|z| (TEMPLE.0..TEMPLE.0 + TEMPLE_W).map(move |x| (x, z)))
            .filter_map(|(x, z)| island.surface_at(x, z))
            .collect();
        assert_eq!(heights.len(), (TEMPLE_W * TEMPLE_D) as usize);
        let (min, max) = (
            *heights.iter().min().unwrap(),
            *heights.iter().max().unwrap(),
        );
        assert_eq!(min, max, "terrace is not level: {min}..{max}");
        assert!(min > WATER_LEVEL, "the terrace would flood");
    }

    #[test]
    fn the_gate_is_backlit() {
        let island = built(11);
        let mut backlit = 0;
        for y in 0..island.world.size[1] as i32 {
            for z in 0..island.world.size[2] as i32 {
                for x in 0..island.world.size[0] as i32 {
                    if island.world.get(x, y, z) == PORTAL
                        && island.world.get(x, y, z - 1) == GLOWSTONE
                    {
                        backlit += 1;
                    }
                }
            }
        }
        assert!(backlit >= 3, "the gate is not backlit ({backlit})");
    }

    #[test]
    fn the_bridge_spans_the_stream_without_touching_it() {
        let island = built(2024);
        let x = 22;
        let mut deck = 0;
        let mut over_water = 0;
        for z in 0..SIZE as i32 {
            for y in WATER_LEVEL + 1..WATER_LEVEL + 6 {
                if island.world.get(x, y, z) == STONE_BRICKS {
                    deck += 1;
                    if island.world.get(x, WATER_LEVEL, z) == WATER {
                        over_water += 1;
                    }
                }
            }
        }
        assert!(deck > 5, "no bridge deck found ({deck})");
        assert!(over_water > 0, "the bridge does not cross the water");
    }

    #[test]
    fn the_great_tree_towers_over_the_ordinary_ones() {
        let island = built(3);
        let (cx, cz) = (HILL.0 as i32, HILL.1 as i32);
        let ground = island.surface_at(cx, cz).unwrap();
        let mut top = ground;
        for y in ground..island.world.size[1] as i32 {
            if matches!(
                island.world.get(cx, y, cz),
                OAK_LOG | OAK_LEAVES | FLOWERING_LEAVES
            ) {
                top = y;
            }
        }
        assert!(top - ground >= 11, "the great tree is only {} tall", top - ground);
    }

    #[test]
    fn the_dragon_perches_above_the_temple_roof() {
        let island = built(2024);
        let cx = TEMPLE.0 + TEMPLE_W / 2;

        // Find the temple roof under the dragon, then the gold above it.
        let mut roof = 0;
        let mut dragon_top = 0;
        for y in 0..island.world.size[1] as i32 {
            for z in TEMPLE.1..TEMPLE.1 + TEMPLE_D {
                match island.world.get(cx, y, z) {
                    QUARTZ | QUARTZ_BRICKS => roof = roof.max(y),
                    OBSIDIAN | EMERALD_BLOCK => dragon_top = dragon_top.max(y),
                    _ => {}
                }
            }
        }
        assert!(
            dragon_top > roof,
            "the dragon ({dragon_top}) should sit above the roof ({roof})"
        );

        // It has lit eyes, somewhere above the roof: the head sits out over the
        // entrance, so the search covers the whole temple footprint.
        let mut eyes = Vec::new();
        for y in roof..island.world.size[1] as i32 {
            for z in TEMPLE.1 - 2..TEMPLE.1 + TEMPLE_D + 2 {
                for x in TEMPLE.0 - 6..=TEMPLE.0 + TEMPLE_W + 2 {
                    if island.world.get(x, y, z) == GLOWSTONE {
                        eyes.push((x, y, z));
                    }
                }
            }
        }
        assert!(eyes.len() >= 2, "the dragon has no eyes: {eyes:?}");
    }

    #[test]
    fn lights_are_clustered_but_still_cover_every_emitter() {
        let island = built(3);
        let lights = collect_lights(&island.world);
        let emitters = count(&island.world, GLOWSTONE) + count(&island.world, PORTAL);
        assert!(lights.len() < emitters, "clustering did nothing");
        assert!(lights.len() >= 5, "clusters collapsed too far: {}", lights.len());

        for y in 0..island.world.size[1] as i32 {
            for z in 0..island.world.size[2] as i32 {
                for x in 0..island.world.size[0] as i32 {
                    let block = island.world.get(x, y, z);
                    if !is_emissive(block) {
                        continue;
                    }
                    let p = vec3(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
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
}
