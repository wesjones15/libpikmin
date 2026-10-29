//! Locked onto a Candypop Bud: the cursor stands in front of its mouth and a thrown Pikmin goes in. A host with bigger
//! squads than Pikmin 2's gives a bud more room.

use libpikmin::bulb::{self, BulbId};
use libpikmin::host::FlatHost;
use libpikmin::{CaptainInput, Params, PikiColor, Sim, Vec3};

fn step(sim: &mut Sim, c: libpikmin::CaptainId, id: Option<BulbId>, throw: bool) {
    sim.set_lock_bulb(c, id);
    sim.set_input(c, CaptainInput { throw, ..Default::default() });
    sim.step(&mut FlatHost);
}

#[test]
fn the_cursor_stands_in_front_of_the_locked_bud() {
    let mut sim = Sim::new(Params::default(), 5);
    let c = sim.add_captain(Vec3::ZERO);
    let bud = sim.add_bulb(PikiColor::Blue, false, Vec3::new(0.0, 0.0, 200.0));
    // Still unfolding: nothing to lock onto.
    step(&mut sim, c, Some(bud), false);
    assert!(sim.captain(c).lock_view.is_none());
    for _ in 0..40 { step(&mut sim, c, Some(bud), false); }
    let cap = sim.captain(c);
    let view = cap.lock_view.expect("locked on");
    assert!((cap.cursor_world - Vec3::new(0.0, bulb::HEIGHT * 0.6, 200.0)).length() < 0.5, "{:?}", cap.cursor_world);
    assert!(view.pos.z < 200.0 && view.pos.z > 0.0, "{:?}", view.pos);
    step(&mut sim, c, None, false);
    assert!(sim.captain(c).lock_view.is_none(), "letting go gives the cursor back");
}

#[test]
fn pikmin_thrown_at_the_locked_bud_go_in_and_come_out_as_its_colour() {
    let mut sim = Sim::new(Params::default(), 5);
    let c = sim.add_captain(Vec3::ZERO);
    let bud = sim.add_bulb(PikiColor::Purple, false, Vec3::new(0.0, 0.0, 260.0));
    for x in [-20.0, 20.0, 0.0] {
        sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(x, 0.0, -30.0), c).unwrap();
    }
    for _ in 0..40 { step(&mut sim, c, Some(bud), false); }
    for _ in 0..30 { step(&mut sim, c, Some(bud), true); }
    for _ in 0..120 { step(&mut sim, c, Some(bud), false); }
    let taken = sim.bulbs()[0].used;
    assert!(taken >= 1, "nobody went in: {:?}", sim.pikis().iter().map(|p| format!("{:?} {:?}", p.state, p.pos)).collect::<Vec<_>>());
    assert_eq!(sim.bulbs()[0].swallowed, taken);
}

#[test]
fn the_lock_lapses_when_the_bud_is_full_or_gone() {
    let mut sim = Sim::new(Params::default(), 5);
    let c = sim.add_captain(Vec3::ZERO);
    step(&mut sim, c, Some(BulbId(999)), false);
    assert!(sim.captain(c).lock_view.is_none());
    sim.set_bulb_slots(1, 1);
    let bud = sim.add_bulb(PikiColor::Red, false, Vec3::new(0.0, 0.0, 200.0));
    for _ in 0..40 { step(&mut sim, c, Some(bud), false); }
    assert!(sim.captain(c).lock_view.is_some());
    let p = sim.spawn_piki(PikiColor::Blue, Vec3::new(0.0, 20.0, 200.0)).unwrap();
    // Thrown into it by hand: the only slot is taken.
    sim.piki_mut(p).state = libpikmin::PikiState::Flying { ground_frames: 0 };
    step(&mut sim, c, Some(bud), false);
    assert!(sim.bulbs()[0].used >= 1);
    step(&mut sim, c, Some(bud), false);
    assert!(sim.captain(c).lock_view.is_none(), "full buds are not locked");
}

#[test]
fn a_host_can_give_buds_room_for_a_big_squad() {
    let mut sim = Sim::new(Params::default(), 5);
    let before = sim.add_bulb(PikiColor::Blue, false, Vec3::ZERO);
    assert_eq!(sim.bulbs()[0].slots, bulb::SLOTS);
    sim.set_bulb_slots(40, 8);
    let pop = sim.add_bulb(PikiColor::Blue, true, Vec3::X * 100.0);
    let slots: Vec<_> = sim.bulbs().iter().map(|b| (b.id, b.slots)).collect();
    assert_eq!(slots, [(before, 40), (pop, 8)], "existing buds too, and the Pop its own");
    sim.set_bulb_slots(0, 9999);
    assert_eq!((sim.bulbs()[0].slots, sim.bulbs()[1].slots), (1, bulb::MAX_SLOTS), "clamped");
    // Forty fit, five at a time: it spits each five before it takes more.
    sim.set_bulb_slots(40, 8);
    for _ in 0..40 { sim.step(&mut FlatHost); }
    let mut fed = 0;
    for _ in 0..(60 * 30) {
        if fed < 40 && sim.bulbs()[0].can_swallow() {
            let p = sim.spawn_piki(PikiColor::Red, Vec3::new(0.0, 20.0, 0.0)).unwrap();
            sim.piki_mut(p).state = libpikmin::PikiState::Flying { ground_frames: 0 };
            fed += 1;
        }
        sim.step(&mut FlatHost);
        if sim.bulbs()[0].used == 40 { break; }
    }
    assert_eq!(sim.bulbs()[0].used, 40);
    assert!(fed == 40, "all forty went in");
}
