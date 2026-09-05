//! The day/night cycle: one number in `[0, 1)` drives the sun, the sky palette
//! and the ambient light, so day and night stay consistent with each other.
//!
//! `0.00` sunrise, `0.25` noon, `0.50` sunset, `0.75` midnight.

use crate::math::{vec3, Vec3};

/// Everything the lighting needs for one instant of the cycle.
#[derive(Clone, Copy, Debug)]
pub struct DayLight {
    /// Direction towards the sun, whether it is up or below the horizon.
    pub sun_dir: Vec3,
    /// Direction towards whichever body is lighting the scene right now.
    pub key_dir: Vec3,
    pub key_color: Vec3,
    /// Hemispherical ambient: sky above, bounced ground below.
    pub sky_color: Vec3,
    pub ground_color: Vec3,
    /// Sky dome colors.
    pub zenith: Vec3,
    pub horizon: Vec3,
    pub below: Vec3,
    /// 0 in daylight, 1 at night: how strongly stars show.
    pub star_strength: f32,
    /// 1 while the sun sits on the horizon: the golden hour.
    pub golden: f32,
    /// 1 in full daylight, 0 at night. Also used to dim the sun's disc.
    pub daylight: f32,
}

// Daylight palette.
const DAY_ZENITH: Vec3 = vec3(0.075, 0.22, 0.52);
const DAY_HORIZON: Vec3 = vec3(0.62, 0.74, 0.92);
const DAY_BELOW: Vec3 = vec3(0.32, 0.40, 0.55);
// The warm band that appears when the sun is near the horizon.
const DUSK_ZENITH: Vec3 = vec3(0.075, 0.09, 0.30);
const DUSK_HORIZON: Vec3 = vec3(1.05, 0.46, 0.22);
const DUSK_BELOW: Vec3 = vec3(0.20, 0.16, 0.26);
// Night.
const NIGHT_ZENITH: Vec3 = vec3(0.008, 0.012, 0.040);
const NIGHT_HORIZON: Vec3 = vec3(0.040, 0.055, 0.120);
const NIGHT_BELOW: Vec3 = vec3(0.020, 0.026, 0.055);

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Where the sun is at time `t`. It rises towards +X, crosses overhead tilted
/// slightly towards +Z, and sets towards -X.
pub fn sun_direction(t: f32) -> Vec3 {
    let angle = t.rem_euclid(1.0) * std::f32::consts::TAU;
    let (sin_a, cos_a) = angle.sin_cos();
    vec3(cos_a, sin_a, 0.35).normalized()
}

pub fn at(t: f32) -> DayLight {
    let sun_dir = sun_direction(t);
    let elevation = sun_dir.y;

    // How much of the sun's light reaches the scene. The band is wide on purpose:
    // the sun's elevation changes fastest right at the horizon, so a narrow fade
    // makes sunset flick past in a couple of frames instead of lingering.
    let daylight = smoothstep(-0.30, 0.38, elevation);
    // Peaks while the sun is low: the golden hour, on both sides of the horizon.
    let golden = (1.0 - (elevation * 2.6).abs().min(1.0)) * smoothstep(-0.30, 0.10, elevation);

    // The moon takes over as key light, opposite the sun and far dimmer.
    let moon_dir = -sun_dir;
    let key_dir = if elevation > -0.04 { sun_dir } else { moon_dir };

    let noon_sun = vec3(1.85, 1.72, 1.48);
    let golden_sun = vec3(2.10, 1.24, 0.62);
    let moonlight = vec3(0.20, 0.26, 0.44);
    // The key light reddens as it drops, then hands over to the moon.
    let key_color = noon_sun.lerp(golden_sun, golden) * daylight + moonlight * (1.0 - daylight);

    let zenith = NIGHT_ZENITH.lerp(DAY_ZENITH.lerp(DUSK_ZENITH, golden), daylight);
    let horizon = NIGHT_HORIZON.lerp(DAY_HORIZON.lerp(DUSK_HORIZON, golden), daylight);
    let below = NIGHT_BELOW.lerp(DAY_BELOW.lerp(DUSK_BELOW, golden), daylight);

    // Ambient follows the dome, but flattened: the sky fills from above, the
    // ground bounces a fraction of it back.
    let sky_color = vec3(0.045, 0.060, 0.110).lerp(vec3(0.34, 0.44, 0.68), daylight);
    let ground_color = vec3(0.030, 0.032, 0.050).lerp(vec3(0.17, 0.15, 0.16), daylight);

    DayLight {
        sun_dir,
        key_dir,
        key_color,
        sky_color,
        ground_color,
        zenith,
        horizon,
        below,
        star_strength: 1.0 - daylight,
        golden,
        daylight,
    }
}

/// Clock reading for the status line, as `HH:MM`. Noon is `t = 0.25`.
pub fn clock(t: f32) -> String {
    let hours = (t.rem_euclid(1.0) * 24.0 + 6.0) % 24.0;
    format!("{:02}:{:02}", hours as u32, ((hours.fract()) * 60.0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sun_rises_climbs_and_sets() {
        assert!(sun_direction(0.0).y.abs() < 0.05, "sunrise should be level");
        assert!(sun_direction(0.25).y > 0.9, "noon should be overhead");
        assert!(sun_direction(0.5).y.abs() < 0.05, "sunset should be level");
        assert!(sun_direction(0.75).y < -0.9, "midnight should be below");
        // It rises in one direction and sets in the other.
        assert!(sun_direction(0.05).x > 0.0 && sun_direction(0.45).x < 0.0);
    }

    #[test]
    fn noon_is_bright_and_midnight_is_not() {
        let noon = at(0.25);
        let midnight = at(0.75);
        assert!(noon.daylight > 0.99 && midnight.daylight < 0.01);
        assert!(noon.key_color.y > midnight.key_color.y * 4.0);
        assert!(noon.sky_color.y > midnight.sky_color.y * 4.0);
        assert!(noon.zenith.z > midnight.zenith.z * 4.0);
    }

    #[test]
    fn the_moon_takes_over_after_sunset() {
        let midnight = at(0.75);
        // The key light comes from above at midnight: the moon, opposite the sun.
        assert!(midnight.key_dir.y > 0.9, "the moon should be up at midnight");
        assert!(midnight.star_strength > 0.99);
    }

    #[test]
    fn sunset_is_warmer_than_noon() {
        let sunset = at(0.48);
        let noon = at(0.25);
        let warmth = |c: Vec3| c.x / c.z.max(1e-3);
        assert!(
            warmth(sunset.horizon) > warmth(noon.horizon),
            "the horizon should redden at sunset"
        );
    }

    #[test]
    fn the_cycle_wraps_without_a_jump() {
        let before = at(0.999);
        let after = at(0.001);
        assert!((before.sun_dir - after.sun_dir).length() < 0.05);
        assert!((before.daylight - after.daylight).abs() < 0.05);
    }

    #[test]
    fn the_clock_reads_noon_at_a_quarter_turn() {
        assert_eq!(clock(0.25), "12:00");
        assert_eq!(clock(0.0), "06:00");
        assert_eq!(clock(0.75), "00:00");
    }
}
