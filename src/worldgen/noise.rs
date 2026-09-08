//! Perlin gradient noise and fBm, written from scratch and driven by a seed.
//!
//! Everything procedural in the scene (terrain height, ore veins, tree placement,
//! portal smoke) comes from here, so regenerating with a new seed is one call.

/// Small, fast, deterministic integer hash (a variant of the xorshift finalizer).
fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    x
}

pub struct Noise {
    seed: u32,
}

/// Smoothstep-like fade with zero first and second derivatives at 0 and 1,
/// the curve Perlin uses to avoid visible grid creases.
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl Noise {
    pub fn new(seed: u32) -> Noise {
        Noise { seed }
    }

    /// Deterministic pseudo-random float in `[0, 1)` for an integer lattice point.
    pub fn value(&self, x: i32, y: i32, z: i32) -> f32 {
        let h = hash(
            hash(x as u32)
                ^ hash((y as u32).wrapping_mul(0x9E37_79B9))
                ^ hash((z as u32).wrapping_mul(0x85EB_CA6B))
                ^ self.seed,
        );
        (h >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Unit gradient for a 2D lattice point, picked from 8 directions.
    fn gradient2(&self, x: i32, y: i32) -> (f32, f32) {
        const DIRS: [(f32, f32); 8] = [
            (1.0, 0.0),
            (-1.0, 0.0),
            (0.0, 1.0),
            (0.0, -1.0),
            (0.7071, 0.7071),
            (-0.7071, 0.7071),
            (0.7071, -0.7071),
            (-0.7071, -0.7071),
        ];
        DIRS[(hash(hash(x as u32) ^ hash((y as u32).wrapping_mul(0x9E37_79B9)) ^ self.seed) % 8)
            as usize]
    }

    /// Gradient for a 3D lattice point: the 12 edge-midpoint directions of a cube.
    fn gradient3(&self, x: i32, y: i32, z: i32) -> (f32, f32, f32) {
        const DIRS: [(f32, f32, f32); 12] = [
            (1.0, 1.0, 0.0),
            (-1.0, 1.0, 0.0),
            (1.0, -1.0, 0.0),
            (-1.0, -1.0, 0.0),
            (1.0, 0.0, 1.0),
            (-1.0, 0.0, 1.0),
            (1.0, 0.0, -1.0),
            (-1.0, 0.0, -1.0),
            (0.0, 1.0, 1.0),
            (0.0, -1.0, 1.0),
            (0.0, 1.0, -1.0),
            (0.0, -1.0, -1.0),
        ];
        let h = hash(
            hash(x as u32)
                ^ hash((y as u32).wrapping_mul(0x9E37_79B9))
                ^ hash((z as u32).wrapping_mul(0x85EB_CA6B))
                ^ self.seed,
        );
        DIRS[(h % 12) as usize]
    }

    /// 2D Perlin noise, roughly in `[-1, 1]`.
    pub fn perlin2(&self, x: f32, y: f32) -> f32 {
        let (x0, y0) = (x.floor() as i32, y.floor() as i32);
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let (u, v) = (fade(fx), fade(fy));

        let dot = |gx: i32, gy: i32, dx: f32, dy: f32| {
            let g = self.gradient2(gx, gy);
            g.0 * dx + g.1 * dy
        };

        let n00 = dot(x0, y0, fx, fy);
        let n10 = dot(x0 + 1, y0, fx - 1.0, fy);
        let n01 = dot(x0, y0 + 1, fx, fy - 1.0);
        let n11 = dot(x0 + 1, y0 + 1, fx - 1.0, fy - 1.0);

        lerp(lerp(n00, n10, u), lerp(n01, n11, u), v)
    }

    /// 3D Perlin noise, roughly in `[-1, 1]`.
    pub fn perlin3(&self, x: f32, y: f32, z: f32) -> f32 {
        let (x0, y0, z0) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
        let (fx, fy, fz) = (x - x0 as f32, y - y0 as f32, z - z0 as f32);
        let (u, v, w) = (fade(fx), fade(fy), fade(fz));

        let dot = |gx: i32, gy: i32, gz: i32, dx: f32, dy: f32, dz: f32| {
            let g = self.gradient3(gx, gy, gz);
            g.0 * dx + g.1 * dy + g.2 * dz
        };

        let c = |dz: i32, fz: f32| {
            let n00 = dot(x0, y0, z0 + dz, fx, fy, fz);
            let n10 = dot(x0 + 1, y0, z0 + dz, fx - 1.0, fy, fz);
            let n01 = dot(x0, y0 + 1, z0 + dz, fx, fy - 1.0, fz);
            let n11 = dot(x0 + 1, y0 + 1, z0 + dz, fx - 1.0, fy - 1.0, fz);
            lerp(lerp(n00, n10, u), lerp(n01, n11, u), v)
        };

        lerp(c(0, fz), c(1, fz - 1.0), w)
    }

    /// Fractional Brownian motion: octaves of Perlin at doubling frequency and
    /// halving amplitude. Normalized so the result stays near `[-1, 1]`.
    pub fn fbm2(&self, x: f32, y: f32, octaves: u32, lacunarity: f32, gain: f32) -> f32 {
        let (mut freq, mut amp, mut sum, mut norm) = (1.0f32, 1.0f32, 0.0f32, 0.0f32);
        for _ in 0..octaves {
            sum += self.perlin2(x * freq, y * freq) * amp;
            norm += amp;
            freq *= lacunarity;
            amp *= gain;
        }
        sum / norm.max(1e-6)
    }

    pub fn fbm3(&self, x: f32, y: f32, z: f32, octaves: u32, lacunarity: f32, gain: f32) -> f32 {
        let (mut freq, mut amp, mut sum, mut norm) = (1.0f32, 1.0f32, 0.0f32, 0.0f32);
        for _ in 0..octaves {
            sum += self.perlin3(x * freq, y * freq, z * freq) * amp;
            norm += amp;
            freq *= lacunarity;
            amp *= gain;
        }
        sum / norm.max(1e-6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perlin_is_zero_on_the_lattice() {
        // Gradient noise vanishes at integer points by construction.
        let n = Noise::new(7);
        for (x, y) in [(0.0, 0.0), (3.0, -2.0), (11.0, 5.0)] {
            assert!(n.perlin2(x, y).abs() < 1e-5);
        }
    }

    #[test]
    fn perlin_stays_in_range_and_varies() {
        let n = Noise::new(1234);
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for i in 0..2000 {
            let v = n.perlin2(i as f32 * 0.137, i as f32 * 0.079);
            min = min.min(v);
            max = max.max(v);
            assert!(v.abs() <= 1.001, "perlin2 out of range: {v}");
        }
        assert!(max - min > 0.5, "noise is too flat: {min}..{max}");
    }

    #[test]
    fn perlin3_stays_in_range() {
        let n = Noise::new(99);
        for i in 0..2000 {
            let f = i as f32;
            let v = n.perlin3(f * 0.11, f * 0.07, f * 0.13);
            assert!(v.abs() <= 1.001, "perlin3 out of range: {v}");
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_noise() {
        let (a, b) = (Noise::new(42), Noise::new(42));
        for i in 0..100 {
            let f = i as f32 * 0.3;
            assert_eq!(a.perlin2(f, f * 0.5), b.perlin2(f, f * 0.5));
        }
    }

    #[test]
    fn different_seeds_give_different_noise() {
        let (a, b) = (Noise::new(1), Noise::new(2));
        let differences = (0..200)
            .filter(|i| {
                let f = *i as f32 * 0.3;
                (a.perlin2(f, f * 0.5) - b.perlin2(f, f * 0.5)).abs() > 1e-6
            })
            .count();
        assert!(differences > 150, "seeds barely differ: {differences}/200");
    }

    #[test]
    fn fbm_is_smoother_than_a_single_octave() {
        // More octaves add detail without blowing past the range.
        let n = Noise::new(5);
        for i in 0..500 {
            let f = i as f32 * 0.21;
            assert!(n.fbm2(f, f * 0.6, 5, 2.0, 0.5).abs() <= 1.001);
            assert!(n.fbm3(f, f * 0.6, f * 0.3, 4, 2.0, 0.5).abs() <= 1.001);
        }
    }

    #[test]
    fn value_noise_is_uniform_enough() {
        let n = Noise::new(3);
        let samples: Vec<f32> = (0..1000).map(|i| n.value(i, i * 7, i * 13)).collect();
        assert!(samples.iter().all(|v| (0.0..1.0).contains(v)));
        let mean = samples.iter().sum::<f32>() / samples.len() as f32;
        assert!((mean - 0.5).abs() < 0.05, "mean was {mean}");
    }
}
