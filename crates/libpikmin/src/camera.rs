//! Host-independent camera definitions. Vanilla preserves the original host's fixed
//! zoom/recentre setup; Orbit is an opt-in right-stick mode, not a replacement for it.
use glam::{Vec2, Vec3};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum CameraMode { #[default] Vanilla, Orbit }

/// Distance in host metres, downward pitch in degrees, vertical field of view.
pub fn preset(zoom: u32, top: bool) -> Vec3 {
    let (distance, pitch) = if top { (24., 82.) } else {
        [(9., 38.), (14., 48.), (21., 58.)][zoom as usize % 3]
    };
    Vec3::new(distance, pitch, 55.)
}

/// yaw is radians, pitch degrees. Deadzone is radial; LB routing belongs to the host.
pub fn pan(mode: CameraMode, yaw: &mut f32, pitch: &mut f32, stick: Vec2, dt: f32) {
    if mode != CameraMode::Orbit || !stick.is_finite() || !dt.is_finite() { return; }
    let strength = stick.length().min(1.);
    if strength <= 0.15 { return; }
    let s = stick.normalize_or_zero() * ((strength - 0.15) / 0.85);
    let dt = dt.clamp(0., 0.1);
    *yaw += s.x * 2.4 * dt;
    *pitch = (*pitch - s.y * 90. * dt).clamp(10., 82.);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn vanilla_is_unchanged_and_orbit_is_bounded() {
        let (mut yaw, mut pitch) = (0., 48.);
        pan(CameraMode::Vanilla, &mut yaw, &mut pitch, Vec2::ONE, 0.1);
        assert_eq!((yaw, pitch), (0., 48.));
        for _ in 0..200 { pan(CameraMode::Orbit, &mut yaw, &mut pitch, Vec2::ONE, 0.03); }
        assert!(yaw > 0.); assert_eq!(pitch, 10.);
        assert_eq!(preset(1, false), Vec3::new(14., 48., 55.));
    }
}
