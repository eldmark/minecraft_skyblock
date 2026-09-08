//! Hand-placed structures on top of the generated terrain.
//!
//! The terrain is procedural; the buildings are not. Straight lines against an
//! organic landscape are what turn a heightmap into a diorama, and they are where
//! the refractive, reflective and emissive materials earn their place.
//!
//! Following the reference: a columned temple with a glowing gate on the far
//! terrace, a great tree on the rocky outcrop, an arched bridge over the stream,
//! and a small ruin of three columns on the near side.

use crate::assets::blocks::*;
use crate::math::{vec3, Vec3};
use crate::worldgen::terrain::{Island, HILL, SIZE, WATER_LEVEL};
use crate::scene::world::World;

/// Where the temple stands, and how big its stylobate is.
const TEMPLE: (i32, i32) = (25, 12);
const TEMPLE_W: i32 = 13;
const TEMPLE_D: i32 = 11;

/// The ruin on the near side of the stream.
const RUIN: (i32, i32) = (13, 38);

pub fn place_all(island: &mut Island) {
    let base = level(island, TEMPLE.0, TEMPLE.1, TEMPLE_W, TEMPLE_D, 1);
    let ridge = temple(island, TEMPLE.0, TEMPLE.1, base);
    dragon(island, TEMPLE.0 + TEMPLE_W / 2, TEMPLE.1 + TEMPLE_D / 2, base, ridge);
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
        return crate::worldgen::terrain::SURFACE_LEVEL as i32;
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

/// The dragon, coiled around the temple.
///
/// A serpentine wyrm following the reference build: white wool hide, dark red
/// crest and barbs, black spine tips. It spirals a turn and three quarters
/// around the colonnade — wide and low at the tail, tightening as it climbs —
/// and lifts its head out over the entrance, at the height of the roof ridge.
///
/// Three things make it read as an animal at this scale:
///
/// * The path is a **single spiral sampled densely and stamped with spheres**,
///   not a chain of boxes. A tube that curves around the building is what turns
///   a pile of blocks into a body; the previous dragon was axis-aligned boxes
///   and read as furniture.
/// * The **crest, spines and barbs stick out of the silhouette**. The animal is
///   recognised by its outline long before its texture, and a smooth tube in
///   white wool would just be a pipe.
/// * It only ever writes into **air**. The coil is sized to clear the temple,
///   but where it does not, the body passes behind the columns instead of
///   eating them.
fn dragon(island: &mut Island, cx: i32, cz: i32, base: i32, ridge: i32) {
    let world = &mut island.world;

    fn put(world: &mut World, x: i32, y: i32, z: i32, block: Block) {
        if world.get(x, y, z) == AIR {
            world.set(x, y, z, block);
        }
    }

    /// The head is carved out of the end of the coil, so unlike the rest of the
    /// body it is allowed to replace the dragon's own blocks — but still not the
    /// temple's.
    fn carve(world: &mut World, x: i32, y: i32, z: i32, block: Block) {
        if matches!(
            world.get(x, y, z),
            AIR | WHITE_WOOL | RED_WOOL | RED_CONCRETE | BLACK_CONCRETE | NETHERRACK
        ) {
            world.set(x, y, z, block);
        }
    }

    fn box_of(world: &mut World, from: (i32, i32, i32), to: (i32, i32, i32), block: Block) {
        for yy in from.1..=to.1 {
            for zz in from.2..=to.2 {
                for xx in from.0..=to.0 {
                    carve(world, xx, yy, zz, block);
                }
            }
        }
    }

    // Deterministic speckle: the red is mixed by position, not by a counter, so
    // the pattern does not drift when the coil is re-sampled.
    fn speckle(x: i32, y: i32, z: i32) -> u32 {
        let mut h = (x as u32)
            .wrapping_mul(0x9E37_79B9)
            .wrapping_add((y as u32).wrapping_mul(0x85EB_CA6B))
            ^ (z as u32).wrapping_mul(0xC2B2_AE35);
        h ^= h >> 13;
        h = h.wrapping_mul(0x27D4_EB2F);
        h >> 7
    }

    let tau = std::f32::consts::TAU;
    // The spiral ends on the front-right corner of the temple, and the head
    // looks out over the entrance (+z) from there — the direction the temple
    // itself faces, and a three-quarter view from the default camera rather
    // than a flat profile or a foreshortened muzzle.
    let turns = 1.5;
    let head_angle = 0.12 * tau;
    let y_tail = (base + 2) as f32;
    let y_head = (ridge + 5) as f32;

    // t = 0 at the tail tip, t = 1 at the base of the skull.
    let point = |t: f32| -> (f32, f32, f32) {
        let a = (t - 1.0) * turns * tau + head_angle;
        // The spiral tightens as it climbs, so the animal looks like it is
        // squeezing the building rather than orbiting it.
        let rx = 9.6 - 2.0 * t;
        let rz = 8.6 - 1.7 * t;
        // It stays low for most of the spiral and rears up at the end. A
        // symmetric curve put the front pass right across the lit gate, which
        // is the one thing on the facade that has to stay visible.
        let climb = t.powf(2.2);
        (
            cx as f32 + rx * a.sin(),
            y_tail + (y_head - y_tail) * climb,
            cz as f32 + rz * a.cos(),
        )
    };

    // Thickness: nothing at the two ends, thickest through the shoulders.
    let girth = |t: f32| -> f32 { 0.55 + 1.05 * (4.0 * t * (1.0 - t)).powf(0.55) };

    const SAMPLES: usize = 260;
    for i in 0..=SAMPLES {
        let t = i as f32 / SAMPLES as f32;
        let (px, py, pz) = point(t);
        let r = girth(t);
        let reach = r.ceil() as i32;

        for dy in -reach..=reach {
            for dz in -reach..=reach {
                for dx in -reach..=reach {
                    let (x, y, z) = (
                        px.round() as i32 + dx,
                        py.round() as i32 + dy,
                        pz.round() as i32 + dz,
                    );
                    let (ox, oy, oz) = (x as f32 - px, y as f32 - py, z as f32 - pz);
                    let d = (ox * ox + oy * oy + oz * oz).sqrt();
                    if d > r {
                        continue;
                    }
                    // White hide with a few red scales mixed in; the crest is
                    // painted on top afterwards so it stays one block wide
                    // instead of swallowing the whole back.
                    let block = if speckle(x, y, z) % 11 == 0 {
                        RED_WOOL
                    } else {
                        WHITE_WOOL
                    };
                    put(world, x, y, z, block);
                }
            }
        }

        // The crest: one course of red along the top of the back, with a black
        // spine standing out of it every few samples.
        let (x, z) = (px.round() as i32, pz.round() as i32);
        let top = (py + r).floor() as i32;
        put(
            world,
            x,
            top,
            z,
            if speckle(x, top, z) % 4 == 0 {
                RED_CONCRETE
            } else {
                RED_WOOL
            },
        );
        if i % 12 == 0 && (0.05..0.94).contains(&t) {
            put(world, x, top + 1, z, RED_CONCRETE);
            put(world, x, top + 2, z, BLACK_CONCRETE);
        }

        // Barbs off the flanks, alternating sides, following the reference's
        // ragged outline.
        if i % 26 == 0 && (0.08..0.90).contains(&t) {
            let side = if (i / 26) % 2 == 0 { 1 } else { -1 };
            // Outward from the temple's axis, in whichever direction the body is
            // furthest from the centre.
            let (ax, az) = (px - cx as f32, pz - cz as f32);
            let (nx, nz) = if ax.abs() > az.abs() {
                (ax.signum() as i32, 0)
            } else {
                (0, az.signum() as i32)
            };
            let (x, y, z) = (px.round() as i32, py.round() as i32, pz.round() as i32);
            // Anchored on the flank, never floating beside it: `reach` rounds
            // up and can land outside the tube.
            let flank = r.floor().max(1.0) as i32;
            let (bx, bz) = (x + nx * flank, z + nz * flank);
            put(world, bx, y + side.max(0), bz, RED_WOOL);
            put(world, bx + nx, y + side, bz + nz, BLACK_CONCRETE);
        }
    }

    // Two pairs of short clawed legs, hanging off the lower coils.
    for t in [0.26_f32, 0.58] {
        let (px, py, pz) = point(t);
        let (ax, az) = (px - cx as f32, pz - cz as f32);
        let (nx, nz) = if ax.abs() > az.abs() {
            (ax.signum() as i32, 0)
        } else {
            (0, az.signum() as i32)
        };
        // The legs run along the body, one in front of the other.
        let (tx, tz) = (-nz, nx);
        let (x, y, z) = (px.round() as i32, py.round() as i32, pz.round() as i32);
        for along in [-1i32, 2] {
            let (lx, lz) = (x + tx * along + nx, z + tz * along + nz);
            put(world, lx, y - 1, lz, RED_WOOL);
            put(world, lx + nx, y - 2, lz + nz, RED_CONCRETE);
            put(world, lx + nx * 2, y - 2, lz + nz * 2, BLACK_CONCRETE);
        }
    }

    // The head, at the end of the spiral, held out over the temple's front-right
    // corner and facing the way the building does, along +z.
    let (nx, ny, nz) = {
        let (px, py, pz) = point(1.0);
        (px.round() as i32, py.round() as i32, pz.round() as i32)
    };
    // A short neck lifts the skull clear of the last coil: at this scale a head
    // sitting straight on the body is just a lump on the tube.
    let (hx, hy, hz) = (nx, ny + 4, nz + 1);
    for step in 0..=4 {
        let x = nx - step / 3;
        let z = nz + step / 2;
        box_of(world, (x, ny + step, z), (x + 1, ny + step + 1, z), WHITE_WOOL);
        carve(world, x, ny + step + 2, z, RED_WOOL);
    }

    // Skull, three wide, with the eyes on its cheeks.
    box_of(world, (hx - 1, hy, hz - 1), (hx + 1, hy + 1, hz + 2), WHITE_WOOL);
    // Brow ridge, continuing the crest that runs down the whole back.
    box_of(world, (hx - 1, hy + 2, hz - 1), (hx + 1, hy + 2, hz + 2), RED_WOOL);
    // Muzzle, dropping as it reaches forward, with a black nose.
    box_of(world, (hx - 1, hy, hz + 3), (hx + 1, hy, hz + 4), WHITE_WOOL);
    carve(world, hx, hy, hz + 5, BLACK_CONCRETE);
    // Lower jaw, open: the gap between the two is the mouth.
    box_of(world, (hx - 1, hy - 1, hz + 1), (hx + 1, hy - 1, hz + 4), RED_CONCRETE);
    // Throat, glowing faintly through the open jaw.
    box_of(world, (hx, hy, hz + 1), (hx, hy, hz + 2), NETHERRACK);
    // Eyes: the only part of the dragon that survives after dark.
    // On the front corners of the skull, where a three-quarter view still
    // catches them: on the flat cheeks they were only visible from the side.
    carve(world, hx - 1, hy + 1, hz + 2, GLOWSTONE);
    carve(world, hx + 1, hy + 1, hz + 2, GLOWSTONE);
    // Horns sweeping back off the skull, black at the tips, and the cheek
    // frills the reference build hangs under the jaw.
    for side in [-1i32, 1] {
        for step in 0..3 {
            let block = if step == 2 { BLACK_CONCRETE } else { RED_WOOL };
            carve(world, hx + side, hy + 2 + step, hz - 1 - step, block);
        }
        carve(world, hx + side * 2, hy, hz + 1, RED_WOOL);
        carve(world, hx + side * 2, hy - 1, hz + 2, BLACK_CONCRETE);
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
    use crate::worldgen::terrain;

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
            // The dragon is the only wool in the scene, and it is big.
            assert!(count(w, WHITE_WOOL) > 200, "seed {seed}: no dragon");
            assert!(count(w, RED_WOOL) > 20, "seed {seed}: the dragon has no crest");
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
    fn the_dragon_rears_its_head_above_the_temple_roof() {
        let island = built(2024);
        let hide = |b: Block| matches!(b, WHITE_WOOL | RED_WOOL | RED_CONCRETE | BLACK_CONCRETE);

        // The roof, at the temple's centre line, and the highest block of the
        // dragon anywhere around it.
        let cx = TEMPLE.0 + TEMPLE_W / 2;
        let mut roof = 0;
        let mut head = 0;
        for y in 0..island.world.size[1] as i32 {
            for z in TEMPLE.1 - 8..TEMPLE.1 + TEMPLE_D + 8 {
                if matches!(island.world.get(cx, y, z), QUARTZ | QUARTZ_BRICKS) {
                    roof = roof.max(y);
                }
                for x in TEMPLE.0 - 8..TEMPLE.0 + TEMPLE_W + 8 {
                    if hide(island.world.get(x, y, z)) {
                        head = head.max(y);
                    }
                }
            }
        }
        assert!(
            head > roof + 2,
            "the dragon ({head}) should rear over the roof ({roof})"
        );

        // Its eyes are lit, and they are up there with the head.
        let mut eyes = Vec::new();
        for y in roof..island.world.size[1] as i32 {
            for z in TEMPLE.1 - 8..TEMPLE.1 + TEMPLE_D + 8 {
                for x in TEMPLE.0 - 8..TEMPLE.0 + TEMPLE_W + 8 {
                    if island.world.get(x, y, z) == GLOWSTONE {
                        eyes.push((x, y, z));
                    }
                }
            }
        }
        assert!(eyes.len() >= 2, "the dragon has no eyes: {eyes:?}");
    }

    #[test]
    fn the_dragon_wraps_around_all_four_sides_of_the_temple() {
        let island = built(2024);
        let (x0, z0) = TEMPLE;
        let (x1, z1) = (x0 + TEMPLE_W - 1, z0 + TEMPLE_D - 1);
        let hide = |b: Block| matches!(b, WHITE_WOOL | RED_WOOL | RED_CONCRETE | BLACK_CONCRETE);

        let mut sides = [0usize; 4];
        for y in 0..island.world.size[1] as i32 {
            for z in 0..island.world.size[2] as i32 {
                for x in 0..island.world.size[0] as i32 {
                    if !hide(island.world.get(x, y, z)) {
                        continue;
                    }
                    // Only count what is genuinely beside the building, not the
                    // head hanging over its roof.
                    match (x < x0, x > x1, z < z0, z > z1) {
                        (true, _, false, false) => sides[0] += 1,
                        (_, true, false, false) => sides[1] += 1,
                        (false, false, true, _) => sides[2] += 1,
                        (false, false, _, true) => sides[3] += 1,
                        _ => {}
                    }
                }
            }
        }
        for (i, n) in sides.iter().enumerate() {
            assert!(*n > 25, "side {i} of the temple has no dragon on it ({n})");
        }
    }

    #[test]
    fn the_dragon_never_eats_the_temple() {
        // The coil writes only into air, so the colonnade must survive it whole.
        let columns = |island: &Island| {
            let mut n = 0;
            for y in 0..island.world.size[1] as i32 {
                for z in TEMPLE.1..TEMPLE.1 + TEMPLE_D {
                    for x in TEMPLE.0..TEMPLE.0 + TEMPLE_W {
                        if island.world.get(x, y, z) == QUARTZ_PILLAR {
                            n += 1;
                        }
                    }
                }
            }
            n
        };

        let mut bare = terrain::generate(2024);
        let base = level(&mut bare, TEMPLE.0, TEMPLE.1, TEMPLE_W, TEMPLE_D, 1);
        temple(&mut bare, TEMPLE.0, TEMPLE.1, base);

        assert_eq!(columns(&bare), columns(&built(2024)));
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

