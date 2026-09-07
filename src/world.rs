//! The voxel world and the ray traversal over it.
//!
//! Everything in the scene is a unit cube on an integer grid, so instead of a BVH
//! the renderer walks the grid directly with a 3D DDA (Amanatides & Woo). Cost is
//! linear in cells crossed, there is no tree to build, and an empty cell costs a
//! couple of comparisons.

use crate::math::{vec3, Vec3};

pub type BlockId = u8;

/// How much of its voxel a block actually fills.
///
/// Everything used to be a unit cube. Slabs and fences are what a wooden bridge
/// needs to stop looking like a wall of planks, and they cost the DDA nothing
/// until a ray actually reaches one: the traversal is unchanged, and only a
/// non-full block pays for a ray/box test inside its own cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Full,
    /// Bottom half of the cell.
    Slab,
    /// A centre post, plus rails towards whichever neighbours are solid.
    Fence,
}

/// An axis-aligned box inside a voxel, in cell-local `[0, 1]` coordinates.
type SubBox = ([f32; 3], [f32; 3]);
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
#[allow(dead_code)] // `t` and `voxel` are part of the hit record the tests assert on.
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
    /// Shape per block id, copied once from `blocks::shape` so the hot loop does
    /// not call across modules per voxel.
    shapes: [Shape; 256],
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
        let mut shapes = [Shape::Full; 256];
        for (id, shape) in shapes.iter_mut().enumerate() {
            *shape = crate::blocks::shape(id as BlockId);
        }
        World {
            blocks: vec![AIR; size[0] * size[1] * size[2]],
            shapes,
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

    #[cfg(test)]
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

    #[inline]
    fn shape(&self, block: BlockId) -> Shape {
        self.shapes[block as usize]
    }

    /// The boxes a non-full block occupies inside its own cell.
    fn sub_boxes(&self, shape: Shape, voxel: [i32; 3], out: &mut [SubBox; 5]) -> usize {
        match shape {
            Shape::Full => {
                out[0] = ([0.0; 3], [1.0; 3]);
                1
            }
            Shape::Slab => {
                out[0] = ([0.0, 0.0, 0.0], [1.0, 0.5, 1.0]);
                1
            }
            Shape::Fence => {
                // Post first, then a pair of rails towards every solid neighbour,
                // which is what makes a run of fences read as a railing instead of
                // a row of sticks.
                out[0] = ([0.375, 0.0, 0.375], [0.625, 1.0, 0.625]);
                let mut n = 1;
                for (axis, dir) in [(0usize, -1i32), (0, 1), (2, -1), (2, 1)] {
                    let mut neighbour = voxel;
                    neighbour[axis] += dir;
                    if self.get(neighbour[0], neighbour[1], neighbour[2]) == AIR {
                        continue;
                    }
                    for (lo_y, hi_y) in [(0.3, 0.45), (0.6, 0.75)] {
                        if n == out.len() {
                            break;
                        }
                        let mut lo = [0.4375, lo_y, 0.4375];
                        let mut hi = [0.5625, hi_y, 0.5625];
                        if dir < 0 {
                            lo[axis] = 0.0;
                            hi[axis] = 0.5;
                        } else {
                            lo[axis] = 0.5;
                            hi[axis] = 1.0;
                        }
                        out[n] = (lo, hi);
                        n += 1;
                    }
                }
                n
            }
        }
    }

    /// Nearest intersection with a partial block inside its own cell, between
    /// `t_in` and `t_out`. Returns the distance and the face that was hit.
    fn sub_hit(
        &self,
        shape: Shape,
        voxel: [i32; 3],
        ray: &Ray,
        t_in: f32,
        t_out: f32,
    ) -> Option<(f32, Face)> {
        let mut boxes = [([0.0; 3], [0.0; 3]); 5];
        let count = self.sub_boxes(shape, voxel, &mut boxes);
        let mut best: Option<(f32, Face)> = None;
        for (lo, hi) in boxes.iter().take(count) {
            let mut t0 = t_in.max(0.0);
            let mut t1 = t_out;
            let mut axis_hit = 0usize;
            let mut inside = true;
            for axis in 0..3 {
                let origin = ray.origin.axis(axis);
                let dir = ray.dir.axis(axis);
                let lo_w = voxel[axis] as f32 + lo[axis];
                let hi_w = voxel[axis] as f32 + hi[axis];
                if dir.abs() < 1e-8 {
                    if origin < lo_w || origin > hi_w {
                        inside = false;
                        break;
                    }
                    continue;
                }
                let inv = 1.0 / dir;
                let (mut near, mut far) = ((lo_w - origin) * inv, (hi_w - origin) * inv);
                if near > far {
                    std::mem::swap(&mut near, &mut far);
                }
                if near > t0 {
                    t0 = near;
                    axis_hit = axis;
                }
                t1 = t1.min(far);
                if t0 > t1 {
                    inside = false;
                    break;
                }
            }
            if !inside {
                continue;
            }
            if best.is_none_or(|(bt, _)| t0 < bt) {
                best = Some((t0, Face::from_axis(axis_hit, ray.dir.axis(axis_hit) > 0.0)));
            }
        }
        best
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
                    // A full block is hit wherever the traversal entered its cell.
                    // A partial one has to be intersected inside the cell, and the
                    // ray simply carries on when it misses.
                    let hit = match self.shape(block) {
                        Shape::Full => Some((t.max(0.0), face)),
                        shape => {
                            let t_leave = t_max[0].min(t_max[1]).min(t_max[2]);
                            self.sub_hit(shape, voxel, ray, t, t_leave.min(t_exit))
                        }
                    };
                    if let Some((t_hit, face)) = hit {
                        let point = ray.at(t_hit.max(0.0) + 1e-5);
                        let (u, v) = face_uv(face, point);
                        return Some(Hit {
                            t: t_hit.max(0.0),
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

    /// A world holding one block of a given id, so a shape can be probed alone.
    fn shaped_world(block: BlockId) -> World {
        let mut w = World::new([8, 8, 8]);
        w.set(4, 4, 4, block);
        w
    }

    fn one_block_world() -> World {
        let mut w = World::new([8, 8, 8]);
        w.set(4, 4, 4, 1);
        w
    }

    #[test]
    fn a_slab_only_fills_the_bottom_half_of_its_cell() {
        let w = shaped_world(crate::blocks::OAK_SLAB);
        // Straight down onto the middle of the cell: the surface is at 4.5, not 5.
        let down = Ray::new(vec3(4.5, 7.0, 4.5), vec3(0.0, -1.0, 0.0));
        let hit = w.trace(&down, 100.0, |_| true).expect("the slab should be hit");
        assert!((hit.t - 2.5).abs() < 1e-2, "hit at {}", hit.t);
        assert_eq!(hit.face, Face::PosY);

        // Through the upper half of the same cell: nothing there.
        let across = Ray::new(vec3(-1.0, 4.8, 4.5), vec3(1.0, 0.0, 0.0));
        assert!(w.trace(&across, 100.0, |_| true).is_none());
        // Through the lower half: the slab is solid there.
        let low = Ray::new(vec3(-1.0, 4.2, 4.5), vec3(1.0, 0.0, 0.0));
        assert!(w.trace(&low, 100.0, |_| true).is_some());
    }

    #[test]
    fn a_fence_is_a_post_with_rails_towards_its_neighbours() {
        let mut w = shaped_world(crate::blocks::OAK_FENCE);
        // Down the middle: the post is there.
        let post = Ray::new(vec3(4.5, 7.0, 4.5), vec3(0.0, -1.0, 0.0));
        assert!(w.trace(&post, 100.0, |_| true).is_some());
        // Along the cell but off to the side: a lone post leaves the corner open.
        let corner = Ray::new(vec3(-1.0, 4.5, 4.1), vec3(1.0, 0.0, 0.0));
        assert!(w.trace(&corner, 100.0, |_| true).is_none());

        // Give it a neighbour and a rail appears between the two, at rail height.
        w.set(3, 4, 4, crate::blocks::OAK_FENCE);
        let rail = Ray::new(vec3(4.2, 7.0, 4.5), vec3(0.0, -1.0, 0.0));
        let hit = w.trace(&rail, 100.0, |_| true).expect("the rail should be hit");
        assert!((hit.t - (7.0 - 4.75)).abs() < 1e-2, "hit at {}", hit.t);
    }

    #[test]
    fn a_partial_block_does_not_stop_a_ray_that_misses_it() {
        // A slab in front of a full block: a ray through the empty upper half
        // must carry on and hit what is behind it.
        let mut w = World::new([8, 8, 8]);
        w.set(4, 4, 4, crate::blocks::OAK_SLAB);
        w.set(6, 4, 4, 1);
        let ray = Ray::new(vec3(0.0, 4.9, 4.5), vec3(1.0, 0.0, 0.0));
        let hit = w.trace(&ray, 100.0, |_| true).expect("should reach the block");
        assert_eq!(hit.voxel, [6, 4, 4]);
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
