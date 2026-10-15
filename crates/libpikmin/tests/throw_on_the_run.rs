//! Olimar throws while running: a Pikmin he calls to his hand catches up with him, even a leaf
//! (slower than him at full tilt) from the far end of a lock-on's reach.

use libpikmin::piki::PikiState;
use libpikmin::{CaptainInput, CreatureFrame, CreaturePart, Host, Params, PikiColor, Sim, Vec2, Vec3};

struct Enemy;

impl Host for Enemy {
    fn ground_height(&self, _pos: Vec3) -> Option<f32> {
        Some(0.0)
    }

    fn creatures(&mut self, out: &mut CreatureFrame) {
        out.clear();
        let part = CreaturePart { centre: Vec3::new(0.0, 15.0, 3000.0), radius: 14.0, stickable: true };
        out.push(7, Vec3::new(0.0, 0.0, 3000.0), 0.0, true, false, &[part]);
    }
}

/// Runs him at full tilt, puts a leaf `behind` him, taps A and says whether it got thrown.
fn thrown_while_running(lock: bool, behind: f32) -> bool {
    let mut sim = Sim::new(Params::default(), 5);
    sim.set_squad_pace(0.93, 1.04, 1.2);
    let c = sim.add_captain(Vec3::ZERO);
    let id = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(0.0, 0.0, -20.0), c).unwrap();
    if lock {
        sim.set_lock_target(c, Some(7));
    }
    let run = |sim: &mut Sim, throw: bool| {
        sim.set_input(c, CaptainInput { stick: Vec2::new(0.0, 1.0), throw, ..Default::default() });
        sim.step(&mut Enemy);
    };
    for _ in 0..20 {
        run(&mut sim, false);
    }
    let at = sim.captain(c).pos - Vec3::new(0.0, 0.0, behind);
    sim.piki_mut(id).pos = at;
    run(&mut sim, true);
    for _ in 0..150 {
        run(&mut sim, false);
        if matches!(sim.piki(id).state, PikiState::Flying { .. }) {
            return true;
        }
    }
    false
}

#[test]
fn a_leaf_catches_up_for_a_throw_on_the_run() {
    assert!(thrown_while_running(false, 60.0), "unlocked, from just behind");
    assert!(thrown_while_running(true, 300.0), "locked on, from far behind");
}
