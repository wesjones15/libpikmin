//! Pikmin hauling a load play the carry motion (PICK) at the cadence of their pace, not at a fixed rate.

use libpikmin::carry::CarryPhase;
use libpikmin::host::FlatHost;
use libpikmin::{Params, PelletDesc, PikiColor, PikiState, Sim, Vec3};
use libpikmin_assets::motion::Motion;
use libpikmin_present::animator::{Clock, piki_look};

#[test]
fn a_carrier_on_the_move_plays_the_carry_motion_at_the_pace_of_its_walk() {
    let mut sim = Sim::new(Params::default(), 3);
    let cap = sim.add_captain(Vec3::ZERO);
    sim.add_onion(Vec3::new(700.0, 0.0, 0.0), &PikiColor::ALL);
    for k in 0..12 {
        sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(40.0 + k as f32 * 6.0, 0.0, 30.0), cap);
    }
    let pel = sim.spawn_pellet(PelletDesc::numbered(sim.params(), 5, PikiColor::Red, Vec3::new(-150.0, 0.0, 0.0), 0.0));
    sim.recruit_to_pellet(pel, None, 3000.0);
    let mut host = FlatHost;
    let mut seen = 0;
    for _ in 0..30 * 12 {
        sim.step(&mut host);
        for p in sim.pikis() {
            if matches!(p.state, PikiState::Carry(c) if c.phase == CarryPhase::Move) && p.vel.with_y(0.0).length() > 1.0 {
                let look = piki_look(sim.params(), p).expect("a carrier is drawn");
                assert_eq!(look.motion, Motion::Pick);
                let Clock::LoopFrom(fps, from) = look.clock else { panic!("{:?}", look.clock) };
                assert_eq!(from, 11.0);
                // Faster than the fixed 30 a second once it is going at a walking pace, never slower.
                let pace = p.vel.with_y(0.0).length();
                assert!(fps >= 30.0 && fps <= 60.0 && (pace < 35.0 || fps > 30.0), "{fps} at {pace}");
                seen += 1;
            }
        }
    }
    assert!(seen > 0, "no carrier was seen on the move");
}
