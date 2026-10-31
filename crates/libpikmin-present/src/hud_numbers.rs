//! zen/Number.h::NumberPicCallBack::invoke: half-second per-digit change pulse.
pub fn change_scale(direction: i32, elapsed: f32) -> f32 {
    if direction == 0 || !elapsed.is_finite() || elapsed >= 0.5 {
        return 1.0;
    }
    let t = elapsed.max(0.0) / 0.5;
    1.0 + direction.signum() as f32
        * ((2.0 * std::f32::consts::TAU * t).cos() + 1.0)
        * (1.0 - t)
        * 0.3
}
#[cfg(test)]
mod tests {
    #[test]
    fn exact_source_pulse_and_settle() {
        use super::change_scale as s;
        assert!((s(1, 0.0) - 1.6).abs() < 0.00001);
        assert!((s(-1, 0.0) - 0.4).abs() < 0.00001);
        assert!((s(1, 0.25) - 1.3).abs() < 0.00001);
        for t in [0.5, 1.0, f32::NAN] {
            assert_eq!(s(1, t), 1.0);
            assert_eq!(s(-1, t), 1.0);
        }
    }
}
