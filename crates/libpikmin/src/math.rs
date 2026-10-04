//! Angle conventions follow the decomp: a facing angle `a` points along `(sin a, 0, cos a)`,
//! and the angle of a vector is `atan2(x, z)`.

use glam::{Vec2, Vec3};
use std::f32::consts::{PI, TAU};

/// Wraps an angle into `[0, 2π)` (decomp `roundAng`).
pub fn round_ang(a: f32) -> f32 {
    let r = a.rem_euclid(TAU);
    if r >= TAU { 0.0 } else { r }
}

/// Signed shortest difference `to - from` in `(-π, π]` (decomp `angDist`).
pub fn ang_dist(to: f32, from: f32) -> f32 {
    let mut d = round_ang(to) - round_ang(from);
    if d > PI {
        d -= TAU;
    } else if d <= -PI {
        d += TAU;
    }
    d
}

/// Facing angle of a vector's XZ part.
pub fn yaw_of(v: Vec3) -> f32 {
    v.x.atan2(v.z)
}

/// Unit XZ direction for a facing angle.
pub fn dir_of(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, yaw.cos())
}

/// Rotates `v` about +Y by `yaw`, so local +Z maps to `dir_of(yaw)`.
pub fn rotate_y(v: Vec3, yaw: f32) -> Vec3 {
    let (s, c) = yaw.sin_cos();
    Vec3::new(v.x * c + v.z * s, v.y, -v.x * s + v.z * c)
}

pub fn xz(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

pub fn dist_xz(a: Vec3, b: Vec3) -> f32 {
    xz(a - b).length()
}

/// Deterministic xorshift RNG so runs replay exactly from a seed.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in `[0, 1)`.
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_matches_facing() {
        let yaw = 0.7;
        let d = rotate_y(Vec3::Z, yaw);
        assert!((d - dir_of(yaw)).length() < 1e-5);
    }

    #[test]
    fn ang_dist_wraps() {
        assert!((ang_dist(0.1, TAU - 0.1) - 0.2).abs() < 1e-5);
        assert!((ang_dist(TAU - 0.1, 0.1) + 0.2).abs() < 1e-5);
    }
}
