//! Locked onto an enemy, A throws the nearest Pikmin whatever colour was picked.

use libpikmin::captain::ThrowState;
use libpikmin::{CaptainInput, CreatureFrame, CreaturePart, Host, Params, PikiColor, PikiId, Sim, Vec3};

struct Enemy;

impl Host for Enemy {
    fn ground_height(&self, _pos: Vec3) -> Option<f32> {
        Some(0.0)
    }

    fn creatures(&mut self, out: &mut CreatureFrame) {
        out.clear();
        let part = CreaturePart { centre: Vec3::new(120.0, 15.0, 0.0), radius: 14.0, stickable: true };
        out.push(7, Vec3::new(120.0, 0.0, 0.0), 0.0, true, false, &[part]);
    }
}

/// A Red close by and a Blue a little farther, with Blue picked as the colour to throw.
fn setup() -> (Sim, libpikmin::CaptainId, PikiId, PikiId) {
    let mut sim = Sim::new(Params::default(), 5);
    let c = sim.add_captain(Vec3::ZERO);
    let red = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-20.0, 0.0, 0.0), c).unwrap();
    let blue = sim.spawn_piki_in_squad(PikiColor::Blue, Vec3::new(-45.0, 0.0, 10.0), c).unwrap();
    for _ in 0..60 {
        sim.step(&mut Enemy);
    }
    // Two presses of the colour switch: Red, then Blue (Yellow isn't in the squad).
    for _ in 0..2 {
        sim.set_input(c, CaptainInput { cycle: true, ..Default::default() });
        sim.step(&mut Enemy);
        sim.set_input(c, CaptainInput::default());
        sim.step(&mut Enemy);
    }
    assert_eq!(sim.captain(c).throw_color, Some(PikiColor::Blue));
    (sim, c, red, blue)
}

fn in_hand(sim: &Sim, c: libpikmin::CaptainId) -> Option<PikiId> {
    match sim.captain(c).throw {
        ThrowState::Wait { held, pending, .. } => held.or(pending),
        _ => None,
    }
}

fn press_throw(sim: &mut Sim, c: libpikmin::CaptainId) {
    sim.set_input(c, CaptainInput { throw: true, ..Default::default() });
    sim.step(&mut Enemy);
}

#[test]
fn unlocked_the_picked_colour_is_thrown() {
    let (mut sim, c, _, blue) = setup();
    press_throw(&mut sim, c);
    assert_eq!(in_hand(&sim, c), Some(blue));
}

#[test]
fn locked_on_the_nearest_is_thrown_and_the_pick_is_kept() {
    let (mut sim, c, red, _) = setup();
    sim.set_lock_target(c, Some(7));
    sim.step(&mut Enemy);
    press_throw(&mut sim, c);
    assert_eq!(in_hand(&sim, c), Some(red), "the nearest, not the picked Blue");
    assert_eq!(sim.captain(c).throw_color, Some(PikiColor::Blue), "the pick stays for after the lock");
}
