//! A throw without a lock-on can be steered onto the cursor point (`Sim::set_cursor_homing`),
//! the way a locked-on throw follows its creature.

use libpikmin::captain::{CaptainPose, ThrowState};
use libpikmin::piki::PikiState;
use libpikmin::{CaptainInput, CreatureFrame, Host, Params, PikiColor, Sim, Vec3};

/// Ground that rises (or falls) with x: `slope` units of height per unit of x.
struct Flat {
    slope: f32,
}

impl Host for Flat {
    fn ground_height(&self, pos: Vec3) -> Option<f32> {
        Some(pos.x * self.slope)
    }

    fn creatures(&mut self, out: &mut CreatureFrame) {
        out.clear();
    }
}

/// The cursor sits this far to the side of him, and goes where he goes.
const AHEAD: Vec3 = Vec3::new(110.0, 0.0, 0.0);
/// Olimar running sideways across the throw at about full tilt.
const RUN: f32 = 160.0;

/// Throws one Pikmin at the cursor from a captain whose body is driven past at `RUN`, and
/// returns how far from where the cursor was at the throw (on the ground plane) it came down.
fn miss(homing: bool) -> f32 {
    miss_on(homing, 0.0)
}

/// `miss` over ground with the given slope along the throw (x).
fn miss_on(homing: bool, slope: f32) -> f32 {
    let mut flat = Flat { slope };
    let mut sim = Sim::new(Params::default(), 5);
    sim.set_cursor_homing(homing);
    let c = sim.add_captain(Vec3::ZERO);
    let id = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-20.0, 0.0, 0.0), c).unwrap();
    let mut pos = Vec3::ZERO;
    let mut input = |sim: &mut Sim, pos: &mut Vec3, throw: bool| {
        *pos += Vec3::new(0.0, 0.0, -RUN) / 30.0;
        let pose = CaptainPose { pos: *pos, vel: Vec3::new(0.0, 0.0, -RUN), face: 0.0 };
        sim.set_input(c, CaptainInput { pose: Some(pose), cursor: Some(*pos + AHEAD), throw, ..Default::default() });
        sim.step(&mut flat);
    };
    for _ in 0..60 {
        input(&mut sim, &mut pos, false);
    }
    // Hold to take a Pikmin in hand, let go to throw it.
    for _ in 0..40 {
        input(&mut sim, &mut pos, true);
        if matches!(sim.captain(c).throw, ThrowState::Wait { .. }) {
            break;
        }
    }
    input(&mut sim, &mut pos, false);
    let mut thrown = false;
    let mut aimed = Vec3::ZERO;
    for _ in 0..200 {
        if !thrown {
            aimed = sim.captain(c).cursor_world;
        }
        input(&mut sim, &mut pos, false);
        let flying = matches!(sim.piki(id).state, PikiState::Flying { .. });
        thrown |= flying;
        if thrown && !flying {
            break;
        }
    }
    assert!(thrown, "the Pikmin was thrown");
    let p = sim.piki(id).pos;
    (p - aimed).with_y(0.0).length()
}

#[test]
fn without_homing_the_captains_speed_carries_the_throw_off_target() {
    assert!(miss(false) > 30.0, "{}", miss(false));
}

#[test]
fn with_homing_the_throw_comes_down_on_the_cursor() {
    let m = miss(true);
    assert!(m < 12.0, "missed by {m}");
}

#[test]
fn with_homing_the_throw_comes_down_on_the_cursor_uphill() {
    let m = miss_on(true, 0.25);
    assert!(m < 12.0, "missed by {m} uphill");
}

#[test]
fn with_homing_the_throw_comes_down_on_the_cursor_downhill() {
    let m = miss_on(true, -0.25);
    assert!(m < 12.0, "missed by {m} downhill");
}
