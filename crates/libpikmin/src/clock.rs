//! Fixed-step clock.
//!
//! In-game sections run at 30 Hz: `NewPikiGameSection` calls `gsys->setFrameClamp(2)` (two 60 Hz
//! retraces per frame, newPikiGame.cpp), and `System::updateSysClock` (sysDolphin/system.cpp)
//! clamps the per-frame delta to 1/30 s. We run exact 1/30 s ticks and accumulate host time.

pub const TICK_HZ: u32 = 30;
pub const TICK_DT: f32 = 1.0 / TICK_HZ as f32;

/// Upper bound on ticks per `accumulate` call so a long host stall can't spiral.
const MAX_STEPS_PER_FRAME: u32 = 4;

#[derive(Debug, Default, Clone)]
pub struct FixedClock {
    acc: f32,
}

impl FixedClock {
    /// Adds `dt` seconds and returns how many ticks are now due. Negative or NaN `dt` counts as 0.
    pub fn accumulate(&mut self, dt: f32) -> u32 {
        if dt.is_finite() && dt > 0.0 {
            self.acc += dt;
        }
        let mut steps = 0;
        while self.acc >= TICK_DT && steps < MAX_STEPS_PER_FRAME {
            self.acc -= TICK_DT;
            steps += 1;
        }
        // Drop whatever we couldn't run rather than banking it.
        if steps == MAX_STEPS_PER_FRAME {
            self.acc = self.acc.min(TICK_DT);
        }
        steps
    }

    /// Fraction of the way to the next tick, for render interpolation.
    pub fn alpha(&self) -> f32 {
        (self.acc / TICK_DT).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixty_hz_host_ticks_every_other_frame() {
        let mut c = FixedClock::default();
        let ticks: u32 = (0..60).map(|_| c.accumulate(1.0 / 60.0)).sum();
        assert!((29..=30).contains(&ticks), "{ticks}");
    }

    #[test]
    fn one_second_runs_thirty_ticks() {
        let mut c = FixedClock::default();
        let ticks: u32 = (0..300).map(|_| c.accumulate(1.0 / 300.0)).sum();
        assert!((29..=30).contains(&ticks), "{ticks}");
    }

    #[test]
    fn stall_is_capped() {
        let mut c = FixedClock::default();
        assert_eq!(c.accumulate(5.0), MAX_STEPS_PER_FRAME);
        assert!(c.accumulate(0.0) <= 1);
    }

    #[test]
    fn bad_dt_is_ignored() {
        let mut c = FixedClock::default();
        assert_eq!(c.accumulate(-1.0), 0);
        assert_eq!(c.accumulate(f32::NAN), 0);
    }
}
