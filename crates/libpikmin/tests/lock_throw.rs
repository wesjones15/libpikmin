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

/// A Pikmin close by in front of him and another a little farther behind: Any takes the
/// plain nearest, not the one behind.
#[test]
fn any_takes_the_plain_nearest_even_in_front() {
    let mut sim = Sim::new(Params::default(), 5);
    let c = sim.add_captain(Vec3::ZERO);
    let behind = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-50.0, 0.0, 0.0), c).unwrap();
    let ahead = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(45.0, 0.0, 0.0), c).unwrap();
    // Hold them still where they are, and him facing +x.
    let pose = libpikmin::captain::CaptainPose { pos: Vec3::ZERO, vel: Vec3::ZERO, face: std::f32::consts::FRAC_PI_2 };
    sim.set_input(c, CaptainInput { pose: Some(pose), ..Default::default() });
    sim.step(&mut Enemy);
    sim.set_lock_target(c, Some(7));
    sim.piki_mut(behind).pos = Vec3::new(-47.0, 0.0, 0.0);
    sim.piki_mut(ahead).pos = Vec3::new(40.0, 0.0, 0.0);
    sim.set_input(c, CaptainInput { pose: Some(pose), throw: true, ..Default::default() });
    sim.step(&mut Enemy);
    assert_eq!(in_hand(&sim, c), Some(ahead), "40 away in front beats 47 away behind");
}

#[test]
fn the_colour_switch_includes_any_so_a_pick_is_never_stuck() {
    let (mut sim, c, _, _) = setup();
    // setup() stopped on Blue; the next step is Any, the nearest.
    sim.set_input(c, CaptainInput { cycle: true, ..Default::default() });
    sim.step(&mut Enemy);
    sim.set_input(c, CaptainInput::default());
    sim.step(&mut Enemy);
    assert_eq!(sim.captain(c).throw_color, None);
    // And around again: Red, then Blue (no Yellow in the squad), then Any.
    let mut seen = Vec::new();
    for _ in 0..3 {
        sim.set_input(c, CaptainInput { cycle: true, ..Default::default() });
        sim.step(&mut Enemy);
        sim.set_input(c, CaptainInput::default());
        sim.step(&mut Enemy);
        seen.push(sim.captain(c).throw_color);
    }
    assert_eq!(seen, [Some(PikiColor::Red), Some(PikiColor::Blue), None]);
}

#[test]
fn with_any_picked_the_nearest_is_thrown() {
    let (mut sim, c, red, _) = setup();
    sim.set_input(c, CaptainInput { cycle: true, ..Default::default() });
    sim.step(&mut Enemy);
    sim.set_input(c, CaptainInput::default());
    sim.step(&mut Enemy);
    press_throw(&mut sim, c);
    assert_eq!(in_hand(&sim, c), Some(red));
}

#[test]
fn locked_on_a_pikmin_two_hundred_units_off_is_called_but_not_without_the_lock() {
    for (locked, expected) in [(true, true), (false, false)] {
        let mut sim = Sim::new(Params::default(), 5);
        let c = sim.add_captain(Vec3::ZERO);
        let far = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-200.0, 0.0, 0.0), c).unwrap();
        let pose = libpikmin::captain::CaptainPose { pos: Vec3::ZERO, vel: Vec3::ZERO, face: std::f32::consts::FRAC_PI_2 };
        sim.set_input(c, CaptainInput { pose: Some(pose), ..Default::default() });
        sim.step(&mut Enemy);
        if locked {
            sim.set_lock_target(c, Some(7));
        }
        sim.piki_mut(far).pos = Vec3::new(-200.0, 0.0, 0.0);
        sim.set_input(c, CaptainInput { pose: Some(pose), throw: true, ..Default::default() });
        sim.step(&mut Enemy);
        assert_eq!(in_hand(&sim, c) == Some(far), expected, "locked: {locked}");
    }
}
