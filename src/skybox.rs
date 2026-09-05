//! Skybox. Two implementations behind one `sample(dir)`:
//!
//! * a procedural dusk sky (default) — gradient, sun disc and glow, drifting cloud
//!   bands and stars that come out overhead, matching the reference diorama's
//!   twilight framing;
//! * a real cubemap built from the pack's own panorama faces (`--sky panorama`),
//!   which exercises the cube lookup against actual images.

use crate::math::{vec3, Vec3};
use crate::noise::Noise;
use crate::pack::Pack;
use crate::texture::Texture;

/// Minecraft's title-screen panorama, which every pack ships: six 768x768 faces.
const PANORAMA: [&str; 6] = [
    "assets/minecraft/textures/gui/title/background/panorama_0.png", // north, -Z
    "assets/minecraft/textures/gui/title/background/panorama_1.png", // east,  +X
    "assets/minecraft/textures/gui/title/background/panorama_2.png", // south, +Z
    "assets/minecraft/textures/gui/title/background/panorama_3.png", // west,  -X
    "assets/minecraft/textures/gui/title/background/panorama_4.png", // up,    +Y
    "assets/minecraft/textures/gui/title/background/panorama_5.png", // down,  -Y
];

pub enum Skybox {
    /// Panorama faces in file order: north, east, south, west, up, down.
    Cubemap {
        faces: Box<[Texture; 6]>,
        tint: Vec3,
        exposure: f32,
    },
    Procedural(Box<Sky>),
}

/// Resolution of the baked lookup table. 1024x512 puts a texel every ~0.006 rad,
/// finer than a star, and costs 6 MB.
const TABLE_W: usize = 1024;
const TABLE_H: usize = 512;

/// Parameters of the procedural dusk sky.
pub struct Sky {
    noise: Noise,
    pub sun_dir: Vec3,
    pub zenith: Vec3,
    pub horizon: Vec3,
    pub ground: Vec3,
    /// The sky evaluated once per direction and cached: clouds and stars cost
    /// dozens of hashes per sample, and the sky never changes during a run.
    table: Vec<Vec3>,
}

impl Sky {
    pub fn dusk(sun_dir: Vec3) -> Sky {
        let mut sky = Sky {
            noise: Noise::new(0x5EED),
            sun_dir,
            zenith: vec3(0.040, 0.075, 0.24),
            horizon: vec3(0.92, 0.60, 0.44),
            // Below the horizon the diorama floats over cool mist, as in the
            // reference: not a second warm gradient, or the island loses contrast.
            ground: vec3(0.13, 0.17, 0.30),
            table: Vec::new(),
        };
        sky.bake();
        sky
    }

    /// Fill the lat-long table by evaluating the analytic sky per texel.
    fn bake(&mut self) {
        let mut table = Vec::with_capacity(TABLE_W * TABLE_H);
        for y in 0..TABLE_H {
            // Texel centers, so the poles are not sampled exactly.
            let pitch = ((y as f32 + 0.5) / TABLE_H as f32 - 0.5) * std::f32::consts::PI;
            let (sin_p, cos_p) = pitch.sin_cos();
            for x in 0..TABLE_W {
                let yaw = ((x as f32 + 0.5) / TABLE_W as f32) * std::f32::consts::TAU;
                let (sin_y, cos_y) = yaw.sin_cos();
                let dir = vec3(cos_p * sin_y, sin_p, cos_p * cos_y);
                table.push(self.evaluate(dir));
            }
        }
        self.table = table;
    }

    /// Bilinear lookup into the baked table: two `atan2`-class calls instead of
    /// the dozens of hashes the analytic sky needs.
    pub fn sample(&self, dir: Vec3) -> Vec3 {
        if self.table.is_empty() {
            return self.evaluate(dir);
        }
        let yaw = dir.x.atan2(dir.z).rem_euclid(std::f32::consts::TAU);
        let pitch = dir.y.clamp(-1.0, 1.0).asin();

        let fx = yaw / std::f32::consts::TAU * TABLE_W as f32 - 0.5;
        let fy = (pitch / std::f32::consts::PI + 0.5) * TABLE_H as f32 - 0.5;
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);

        let wrap_x = |x: i32| x.rem_euclid(TABLE_W as i32) as usize;
        let clamp_y = |y: i32| y.clamp(0, TABLE_H as i32 - 1) as usize;
        let (x0i, x1i) = (wrap_x(x0 as i32), wrap_x(x0 as i32 + 1));
        let (y0i, y1i) = (clamp_y(y0 as i32), clamp_y(y0 as i32 + 1));

        let at = |x: usize, y: usize| self.table[y * TABLE_W + x];
        let top = at(x0i, y0i).lerp(at(x1i, y0i), tx);
        let bottom = at(x0i, y1i).lerp(at(x1i, y1i), tx);
        top.lerp(bottom, ty)
    }

    /// The analytic sky. Used to fill the table and by the tests.
    pub fn evaluate(&self, dir: Vec3) -> Vec3 {
        let up = dir.y.clamp(-1.0, 1.0);

        // Vertical gradient.
        // The exponent keeps the warm band tight to the horizon instead of
        // washing the whole dome orange.
        let mut color = if up >= 0.0 {
            self.horizon.lerp(self.zenith, up.powf(0.35))
        } else {
            self.horizon.lerp(self.ground, (-up).powf(0.28))
        };

        // Sun: a hard disc inside a wide glow.
        let cos_sun = dir.dot(self.sun_dir).clamp(-1.0, 1.0);
        color = color + vec3(1.0, 0.75, 0.45) * cos_sun.max(0.0).powf(64.0) * 1.1;
        if cos_sun > 0.9995 {
            color = color + vec3(6.0, 5.2, 4.2);
        }

        // Cloud bands: two octaves of noise sheared along the view direction, only
        // visible near the horizon where clouds actually read in a diorama.
        let band = (1.0 - up.abs()).powf(2.5);
        if band > 0.01 {
            let (u, v) = (dir.x / (up.abs() + 0.35), dir.z / (up.abs() + 0.35));
            let clouds = self.noise.fbm2(u * 1.6, v * 1.6, 4, 2.1, 0.55);
            let mask = ((clouds - 0.05) * 2.2).clamp(0.0, 1.0) * band;
            let lit = (cos_sun * 0.5 + 0.5).powf(2.0);
            let cloud_color = vec3(0.55, 0.45, 0.52).lerp(vec3(1.0, 0.82, 0.62), lit);
            color = color.lerp(cloud_color, mask * 0.75);
        }

        // Stars, fading in with altitude and away from the sun's glow.
        if up > 0.05 {
            let scale = 260.0;
            let cell = (
                (dir.x * scale).floor() as i32,
                (dir.y * scale).floor() as i32,
                (dir.z * scale).floor() as i32,
            );
            let r = self.noise.value(cell.0, cell.1, cell.2);
            if r > 0.9975 {
                let twinkle = 0.6 + 0.4 * self.noise.value(cell.1, cell.2, cell.0);
                let fade = ((up - 0.05) * 2.2).clamp(0.0, 1.0) * (1.0 - cos_sun.max(0.0));
                color = color + Vec3::ONE * (twinkle * fade * 1.6);
            }
        }

        color
    }
}

impl Skybox {
    /// The default sky: procedural dusk, no pack images involved.
    pub fn dusk(sun_dir: Vec3) -> Skybox {
        Skybox::Procedural(Box::new(Sky::dusk(sun_dir)))
    }

    /// Cubemap from the pack's panorama, falling back to the dusk sky.
    pub fn panorama(pack: &Pack, sun_dir: Vec3) -> Skybox {
        let mut loaded = Vec::with_capacity(6);
        for name in PANORAMA {
            match pack.decode_png(name) {
                // No normal map wanted for sky imagery, hence strength 0.
                Ok(image) => loaded.push(Texture::from_image(&image, 0.0)),
                Err(_) => return Skybox::dusk(sun_dir),
            }
        }
        let faces = <[Texture; 6]>::try_from(loaded)
            .unwrap_or_else(|_| unreachable!("exactly six faces were pushed"));

        Skybox::Cubemap {
            // Kept in file order: `cube_face` speaks the panorama's own layout.
            faces: Box::new(faces),
            // The panorama is a bright daytime scene; the diorama is lit at dusk,
            // so it is pulled towards the warm twilight palette of the reference.
            tint: vec3(0.72, 0.66, 0.82),
            exposure: 0.85,
        }
    }

    pub fn sample(&self, dir: Vec3) -> Vec3 {
        match self {
            Skybox::Procedural(sky) => sky.sample(dir),
            Skybox::Cubemap {
                faces,
                tint,
                exposure,
            } => {
                let (face, u, v) = cube_face(dir);
                faces[face].sample(u, v, 0).mul_elem(*tint) * *exposure
            }
        }
    }
}

/// Cube lookup matched to Minecraft's panorama layout.
///
/// The six files form a ring (0 north, 1 east, 2 south, 3 west) plus up (4) and
/// down (5). Adjacency was verified by comparing the images' edge pixels: face 0's
/// right column continues into face 1's left column, face 4's bottom row meets
/// face 0's top row, and so on. The mapping below reproduces exactly that layout,
/// which is why the seams line up.
fn cube_face(dir: Vec3) -> (usize, f32, f32) {
    // Which of the four horizontal faces the direction points at. Yaw is measured
    // from north (-Z) towards east (+X), matching the file order.
    let yaw = dir.x.atan2(-dir.z);
    let quarter = std::f32::consts::FRAC_PI_2;
    let k = (yaw / quarter).round().rem_euclid(4.0) as usize;
    let c = k as f32 * quarter;
    let (sin_c, cos_c) = c.sin_cos();
    let forward = dir.x * sin_c - dir.z * cos_c;
    let right = dir.x * cos_c + dir.z * sin_c;

    if dir.y > forward.abs().max(1e-6) {
        // Up: east to the right, north along the bottom row.
        let inv = 1.0 / dir.y.max(1e-6);
        return (
            4,
            clamp_uv(0.5 + 0.5 * dir.x * inv),
            clamp_uv(0.5 - 0.5 * dir.z * inv),
        );
    }
    if -dir.y > forward.abs().max(1e-6) {
        // Down: east to the right, north along the top row.
        let inv = 1.0 / (-dir.y).max(1e-6);
        return (
            5,
            clamp_uv(0.5 + 0.5 * dir.x * inv),
            clamp_uv(0.5 + 0.5 * dir.z * inv),
        );
    }

    let inv = 1.0 / forward.max(1e-6);
    (
        k,
        clamp_uv(0.5 + 0.5 * right * inv),
        clamp_uv(0.5 - 0.5 * dir.y * inv),
    )
}

fn clamp_uv(v: f32) -> f32 {
    v.clamp(0.0, 0.9999)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn each_direction_selects_the_panorama_face_that_faces_it() {
        // north, east, south, west, up, down
        let faces: Vec<usize> = [
            vec3(0.0, 0.0, -1.0),
            vec3(1.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            vec3(-1.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0),
            vec3(0.0, -1.0, 0.0),
        ]
        .iter()
        .map(|d| cube_face(*d).0)
        .collect();
        assert_eq!(faces, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn the_ring_faces_meet_where_the_images_do() {
        // Face 0's right edge continues into face 1's left edge; both sit at the
        // same height, so their v must agree across the seam.
        let (fa, ua, va) = cube_face(vec3(0.999, 0.2, -1.0).normalized());
        let (fb, ub, vb) = cube_face(vec3(1.0, 0.2, -0.999).normalized());
        assert_eq!((fa, fb), (0, 1));
        assert!(ua > 0.99 && ub < 0.01, "seam u: {ua} then {ub}");
        assert!((va - vb).abs() < 0.01, "seam v jumped: {va} vs {vb}");
    }

    #[test]
    fn face_coordinates_stay_inside_the_texture() {
        for i in 0..500 {
            let f = i as f32;
            let dir = vec3(f.sin(), (f * 0.7).cos(), (f * 1.3).sin()).normalized();
            let (face, u, v) = cube_face(dir);
            assert!(face < 6);
            assert!((0.0..1.0).contains(&u) && (0.0..1.0).contains(&v));
        }
    }

    #[test]
    fn axis_directions_land_at_the_center_of_their_face() {
        for dir in [vec3(1.0, 0.0, 0.0), vec3(0.0, -1.0, 0.0), vec3(0.0, 0.0, 1.0)] {
            let (_, u, v) = cube_face(dir);
            assert!((u - 0.5).abs() < 1e-4 && (v - 0.5).abs() < 1e-4);
        }
    }

    fn dusk() -> Sky {
        Sky::dusk(vec3(0.55, 0.62, 0.36).normalized())
    }

    #[test]
    fn the_procedural_sky_is_brighter_towards_the_sun() {
        let sky = dusk();
        let sun = sky.sun_dir;
        let away = vec3(-0.55, 0.62, -0.36).normalized();
        assert!(sky.sample(sun).max_component() > sky.sample(away).max_component());
    }

    #[test]
    fn the_sky_is_warm_at_the_horizon_and_deep_overhead() {
        let sky = dusk();
        let horizon = sky.sample(vec3(0.0, 0.02, 1.0).normalized());
        let zenith = sky.sample(vec3(0.0, 1.0, 0.0));
        assert!(horizon.x > horizon.z, "horizon should be warm: {horizon:?}");
        assert!(zenith.z >= zenith.x, "zenith should be cool: {zenith:?}");
        assert!(horizon.max_component() > zenith.max_component());
    }

    #[test]
    fn the_sky_never_returns_negative_light() {
        let sky = dusk();
        for i in 0..2000 {
            let f = i as f32;
            let dir = vec3(f.sin(), (f * 0.53).cos(), (f * 1.7).sin()).normalized();
            let c = sky.sample(dir);
            assert!(c.x >= 0.0 && c.y >= 0.0 && c.z >= 0.0, "negative sky {c:?}");
        }
    }

    #[test]
    fn the_pack_provides_a_real_cubemap() {
        if !Path::new("texturepack").is_dir() {
            return;
        }
        let pack = Pack::open(None).expect("pack should open");
        let sun = vec3(0.55, 0.62, 0.36).normalized();
        assert!(
            matches!(Skybox::panorama(&pack, sun), Skybox::Cubemap { .. }),
            "expected the panorama faces to load from the pack"
        );
    }

    #[test]
    fn neighbouring_directions_sample_similar_sky() {
        if !Path::new("texturepack").is_dir() {
            return;
        }
        let sun = vec3(0.55, 0.62, 0.36).normalized();
        let sky = Skybox::panorama(&Pack::open(None).unwrap(), sun);
        // Straddling the north/east seam must not produce wildly different colors.
        let a = sky.sample(vec3(0.999, 0.1, -1.0).normalized());
        let b = sky.sample(vec3(1.0, 0.1, -0.999).normalized());
        let diff = (a - b).length();
        assert!(diff < 0.35, "seam discontinuity of {diff}");
    }
}
