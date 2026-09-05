//! The voxel world and the ray traversal over it.
//!
//! Everything in the scene is a unit cube on an integer grid, so instead of a BVH
//! the renderer walks the grid directly with a 3D DDA (Amanatides & Woo). Cost is
//! linear in cells crossed, there is no tree to build, and an empty cell costs a
//! couple of comparisons.

use crate::math::{vec3, Vec3};

pub type BlockId = u8;
pub const AIR: BlockId = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    NegX,
    PosX,
    NegY,
    PosY,
    NegZ,
    PosZ,
}

impl Face {
    pub fn normal(self) -> Vec3 {
        match self {
            Face::NegX => vec3(-1.0, 0.0, 0.0),
            Face::PosX => vec3(1.0, 0.0, 0.0),
            Face::NegY => vec3(0.0, -1.0, 0.0),
            Face::PosY => vec3(0.0, 1.0, 0.0),
            Face::NegZ => vec3(0.0, 0.0, -1.0),
            Face::PosZ => vec3(0.0, 0.0, 1.0),
        }
    }

    fn from_axis(axis: usize, positive_step: bool) -> Face {
        // The face hit is the one facing back along the step direction.
        match (axis, positive_step) {
            (0, true) => Face::NegX,
            (0, false) => Face::PosX,
            (1, true) => Face::NegY,
            (1, false) => Face::PosY,
            (2, true) => Face::NegZ,
            _ => Face::PosZ,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

impl Ray {
    pub fn new(origin: Vec3, dir: Vec3) -> Ray {
        Ray {
            origin,
            dir: dir.normalized(),
        }
    }

    pub fn at(&self, t: f32) -> Vec3 {
        self.origin + self.dir * t
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub t: f32,
    pub block: BlockId,
    pub face: Face,
    pub normal: Vec3,
    /// Texture coordinates on the face, both in `[0, 1)`.
    pub u: f32,
    pub v: f32,
    pub voxel: [i32; 3],
    pub point: Vec3,
}

pub struct World {
    pub size: [usize; 3],
    blocks: Vec<BlockId>,
    /// One bit per `MACRO`-sized cell: is anything solid in there at all?
    macro_size: [usize; 3],
    macro_occupied: Vec<bool>,
}

/// Side of a macro cell, in blocks. Powers of two keep the index math cheap.
const MACRO: usize = 4;

impl World {
    pub fn new(size: [usize; 3]) -> World {
        let macro_size = [
            size[0].div_ceil(MACRO),
            size[1].div_ceil(MACRO),
            size[2].div_ceil(MACRO),
        ];
        World {
            blocks: vec![AIR; size[0] * size[1] * size[2]],
            macro_occupied: vec![false; macro_size[0] * macro_size[1] * macro_size[2]],
            size,
            macro_size,
        }
    }

    #[inline]
    fn index(&self, x: usize, y: usize, z: usize) -> usize {
        (y * self.size[2] + z) * self.size[0] + x
    }

    pub fn in_bounds(&self, x: i32, y: i32, z: i32) -> bool {
        x >= 0
            && y >= 0
            && z >= 0
            && (x as usize) < self.size[0]
            && (y as usize) < self.size[1]
            && (z as usize) < self.size[2]
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> BlockId {
        if self.in_bounds(x, y, z) {
            self.blocks[self.index(x as usize, y as usize, z as usize)]
        } else {
            AIR
        }
    }

    pub fn set(&mut self, x: i32, y: i32, z: i32, block: BlockId) {
        if !self.in_bounds(x, y, z) {
            return;
        }
        let i = self.index(x as usize, y as usize, z as usize);
        self.blocks[i] = block;
        if block != AIR {
            let m = self.macro_index(x as usize, y as usize, z as usize);
            self.macro_occupied[m] = true;
        }
    }

    #[inline]
    fn macro_index(&self, x: usize, y: usize, z: usize) -> usize {
        ((y / MACRO) * self.macro_size[2] + z / MACRO) * self.macro_size[0] + x / MACRO
    }

    /// True when the macro cell containing this voxel holds nothing at all.
    #[inline]
    fn macro_empty(&self, x: i32, y: i32, z: i32) -> bool {
        if !self.in_bounds(x, y, z) {
            return true;
        }
        !self.macro_occupied[self.macro_index(x as usize, y as usize, z as usize)]
    }

    pub fn solid_count(&self) -> usize {
        self.blocks.iter().filter(|&&b| b != AIR).count()
    }

    /// Distance range over which the ray is inside the world box, or `None`.
    fn box_range(&self, ray: &Ray) -> Option<(f32, f32)> {
        let (mut t0, mut t1) = (0.0f32, f32::MAX);
        for axis in 0..3 {
            let origin = ray.origin.axis(axis);
            let dir = ray.dir.axis(axis);
            let (lo, hi) = (0.0, self.size[axis] as f32);
            if dir.abs() < 1e-8 {
                if origin < lo || origin > hi {
                    return None;
                }
                continue;
            }
            let inv = 1.0 / dir;
            let (mut near, mut far) = ((lo - origin) * inv, (hi - origin) * inv);
            if near > far {
                std::mem::swap(&mut near, &mut far);
            }
            t0 = t0.max(near);
            t1 = t1.min(far);
            if t0 > t1 {
                return None;
            }
        }
        Some((t0, t1))
    }

    /// Walk the grid and return the first voxel accepted by `accept`.
    ///
    /// The predicate lets one traversal serve every ray type: camera rays accept
    /// anything solid, shadow rays skip blocks that do not cast, and a refracted
    /// ray inside water can skip water itself.
    pub fn trace<F: Fn(BlockId) -> bool>(&self, ray: &Ray, max_t: f32, accept: F) -> Option<Hit> {
        let (t_enter, t_exit) = self.box_range(ray)?;
        if t_enter > max_t {
            return None;
        }
        let t_exit = t_exit.min(max_t);

        // Nudge inside so the starting voxel is unambiguous on a boundary.
        let start = ray.at(t_enter + 1e-4);
        let mut voxel = [
            start.x.floor() as i32,
            start.y.floor() as i32,
            start.z.floor() as i32,
        ];

        let mut step = [0i32; 3];
        let mut t_max = [f32::MAX; 3];
        let mut t_delta = [f32::MAX; 3];
        for axis in 0..3 {
            let dir = ray.dir.axis(axis);
            if dir.abs() < 1e-8 {
                continue;
            }
            let inv = 1.0 / dir.abs();
            t_delta[axis] = inv;
            if dir > 0.0 {
                step[axis] = 1;
                t_max[axis] = t_enter + (voxel[axis] as f32 + 1.0 - start.axis(axis)) * inv;
            } else {
                step[axis] = -1;
                t_max[axis] = t_enter + (start.axis(axis) - voxel[axis] as f32) * inv;
            }
        }

        let mut t = t_enter;
        let mut face = Face::PosY;
        // Which face the ray entered the box through, for the very first voxel.
        {
            let p = ray.at(t_enter + 1e-4);
            let mut best = f32::MAX;
            for (axis, f) in [
                (0usize, if ray.dir.x > 0.0 { Face::NegX } else { Face::PosX }),
                (1, if ray.dir.y > 0.0 { Face::NegY } else { Face::PosY }),
                (2, if ray.dir.z > 0.0 { Face::NegZ } else { Face::PosZ }),
            ] {
                let edge = if ray.dir.axis(axis) > 0.0 {
                    0.0
                } else {
                    self.size[axis] as f32
                };
                let d = (p.axis(axis) - edge).abs();
                if d < best {
                    best = d;
                    face = f;
                }
            }
        }

        while t <= t_exit {
            if !self.macro_empty(voxel[0], voxel[1], voxel[2]) {
                let block = self.get(voxel[0], voxel[1], voxel[2]);
                if block != AIR && accept(block) {
                    let point = ray.at(t.max(0.0) + 1e-5);
                    let (u, v) = face_uv(face, point);
                    return Some(Hit {
                        t: t.max(0.0),
                        block,
                        face,
                        normal: face.normal(),
                        u,
                        v,
                        voxel,
                        point,
                    });
                }
            }

            // Advance along whichever axis reaches its next boundary first.
            let axis = if t_max[0] < t_max[1] {
                if t_max[0] < t_max[2] {
                    0
                } else {
                    2
                }
            } else if t_max[1] < t_max[2] {
                1
            } else {
                2
            };
            t = t_max[axis];
            voxel[axis] += step[axis];
            t_max[axis] += t_delta[axis];
            face = Face::from_axis(axis, step[axis] > 0);

            if voxel[axis] < 0 || voxel[axis] >= self.size[axis] as i32 {
                break;
            }
        }
        None
    }

    /// Any-hit query for shadow rays: stops at the first blocker, no hit record.
    pub fn occluded<F: Fn(BlockId) -> bool>(&self, ray: &Ray, max_t: f32, blocks: F) -> bool {
        self.trace(ray, max_t, blocks).is_some()
    }
}

/// Texture coordinates for a point on a given face of a unit cube.
/// `v` runs downward so that row 0 of a texture is its top row.
fn face_uv(face: Face, point: Vec3) -> (f32, f32) {
    let fx = point.x - point.x.floor();
    let fy = point.y - point.y.floor();
    let fz = point.z - point.z.floor();
    match face {
        Face::PosX => (1.0 - fz, 1.0 - fy),
        Face::NegX => (fz, 1.0 - fy),
        Face::PosY => (fx, fz),
        Face::NegY => (fx, 1.0 - fz),
        Face::PosZ => (fx, 1.0 - fy),
        Face::NegZ => (1.0 - fx, 1.0 - fy),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_block_world() -> World {
        let mut w = World::new([8, 8, 8]);
        w.set(4, 4, 4, 1);
        w
    }

    #[test]
    fn hits_a_block_head_on_and_reports_the_facing_side() {
        let w = one_block_world();
        let ray = Ray::new(vec3(4.5, 4.5, -3.0), vec3(0.0, 0.0, 1.0));
        let hit = w.trace(&ray, 100.0, |_| true).expect("ray should hit");
        assert_eq!(hit.voxel, [4, 4, 4]);
        assert_eq!(hit.face, Face::NegZ);
        assert!((hit.t - 7.0).abs() < 1e-3, "t was {}", hit.t);
    }

    #[test]
    fn misses_when_nothing_is_in_the_way() {
        let w = one_block_world();
        let ray = Ray::new(vec3(0.5, 0.5, -3.0), vec3(0.0, 0.0, 1.0));
        assert!(w.trace(&ray, 100.0, |_| true).is_none());
    }

    #[test]
    fn respects_the_distance_limit() {
        let w = one_block_world();
        let ray = Ray::new(vec3(4.5, 4.5, -3.0), vec3(0.0, 0.0, 1.0));
        assert!(w.trace(&ray, 5.0, |_| true).is_none());
        assert!(w.trace(&ray, 8.0, |_| true).is_some());
    }

    #[test]
    fn a_ray_starting_inside_the_world_still_hits() {
        let w = one_block_world();
        let ray = Ray::new(vec3(4.5, 4.5, 1.0), vec3(0.0, 0.0, 1.0));
        let hit = w.trace(&ray, 100.0, |_| true).unwrap();
        assert_eq!(hit.voxel, [4, 4, 4]);
    }

    #[test]
    fn the_accept_predicate_can_see_through_blocks() {
        let mut w = World::new([8, 8, 8]);
        w.set(4, 4, 2, 7); // glass-like
        w.set(4, 4, 4, 1);
        let ray = Ray::new(vec3(4.5, 4.5, -3.0), vec3(0.0, 0.0, 1.0));
        let hit = w.trace(&ray, 100.0, |b| b != 7).unwrap();
        assert_eq!(hit.voxel, [4, 4, 4]);
    }

    #[test]
    fn diagonal_rays_land_on_the_expected_voxel() {
        let mut w = World::new([8, 8, 8]);
        w.set(6, 6, 6, 1);
        let ray = Ray::new(vec3(0.5, 0.5, 0.5), vec3(1.0, 1.0, 1.0));
        let hit = w.trace(&ray, 100.0, |_| true).unwrap();
        assert_eq!(hit.voxel, [6, 6, 6]);
    }

    #[test]
    fn face_uv_stays_inside_the_unit_square() {
        for face in [
            Face::NegX,
            Face::PosX,
            Face::NegY,
            Face::PosY,
            Face::NegZ,
            Face::PosZ,
        ] {
            let (u, v) = face_uv(face, vec3(3.25, 7.75, 1.5));
            assert!((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v));
        }
    }

    #[test]
    fn top_face_of_a_floor_is_reported_for_a_downward_ray() {
        let mut w = World::new([8, 8, 8]);
        for x in 0..8 {
            for z in 0..8 {
                w.set(x, 0, z, 1);
            }
        }
        let ray = Ray::new(vec3(3.5, 6.0, 3.5), vec3(0.0, -1.0, 0.0));
        let hit = w.trace(&ray, 100.0, |_| true).unwrap();
        assert_eq!(hit.face, Face::PosY);
        assert!((hit.t - 5.0).abs() < 1e-3);
    }
}

#[cfg(test)]
mod entry_tests {
    use super::*;

    fn slab() -> World {
        let mut w = World::new([32, 24, 32]);
        for x in 0..32 {
            for z in 0..32 {
                w.set(x, 5, z, 1);
            }
        }
        w
    }

    #[test]
    fn enters_from_every_side() {
        let w = slab();
        let target = vec3(16.0, 5.5, 16.0);
        for eye in [
            vec3(16.0, 34.0, 62.0),
            vec3(62.0, 34.0, 16.0),
            vec3(16.0, 34.0, -30.0),
            vec3(-30.0, 34.0, 16.0),
        ] {
            let ray = Ray::new(eye, target - eye);
            let hit = w.trace(&ray, 1000.0, |_| true);
            assert!(hit.is_some(), "no hit from {eye:?}");
        }
    }
}
