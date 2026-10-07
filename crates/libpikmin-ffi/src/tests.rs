//! The C ABI driven the way a C host would: raw pointers and extern "C" callbacks.

use super::*;

#[derive(Default)]
struct World {
    ground_calls: u32,
    wall_calls: u32,
    hits: Vec<(u64, f32, u32)>,
}

unsafe extern "C" fn ground(user: *mut c_void, _pos: *const PikVec3, out: *mut f32) -> u8 {
    // SAFETY: `user` is the test's World; `out` is writable.
    unsafe {
        (*(user as *mut World)).ground_calls += 1;
        *out = 0.0;
    }
    1
}

/// A wall along x = 150, facing -x.
unsafe extern "C" fn walls(user: *mut c_void, c: *const PikVec3, r: f32, out: *mut PikVec3) {
    // SAFETY: as above.
    unsafe {
        (*(user as *mut World)).wall_calls += 1;
        let mut v = *c;
        v.x = v.x.min(150.0 - r);
        *out = v;
    }
}

unsafe extern "C" fn damage(user: *mut c_void, creature: u64, amount: f32, _part: i32, source: u32, _id: u32) -> u8 {
    // SAFETY: as above.
    unsafe { (*(user as *mut World)).hits.push((creature, amount, source)) };
    1
}

fn host(world: &mut World) -> PikHost {
    PikHost {
        user: world as *mut World as *mut c_void,
        ground_height: Some(ground),
        walls: Some(walls),
        damage: Some(damage),
        carry_path: None,
    }
}

fn events(sim: *mut PikSim) -> Vec<PikEvent> {
    let mut all = Vec::new();
    let mut buf = [PikEvent::default(); 16];
    loop {
        // SAFETY: `buf` holds 16.
        let n = unsafe { pik_sim_events(sim, buf.as_mut_ptr(), 16) } as usize;
        all.extend_from_slice(&buf[..n]);
        if n < 16 {
            return all;
        }
    }
}

#[test]
fn struct_sizes_are_what_hosts_mirror() {
    let mut s = PikSizes::default();
    // SAFETY: `s` is writable.
    unsafe { pik_struct_sizes(&mut s) };
    // These are the layouts ror2-olimar's Native.cs declares; change both together.
    assert_eq!(
        (s.vec3, s.host, s.input, s.creature, s.part, s.pellet_desc),
        (12, 40, 64, 40, 20, 56)
    );
    assert_eq!(
        (s.captain, s.piki, s.pellet, s.onion, s.sprout, s.event),
        (60, 64, 64, 36, 20, 48)
    );
}

#[test]
fn a_thrown_pikmin_hits_a_host_creature_through_the_abi() {
    // SAFETY: every pointer below is live for its call.
    unsafe {
        let sim = pik_sim_new(std::ptr::null(), 9);
        assert!(!sim.is_null());
        let c = pik_sim_add_captain(sim, &PikVec3::default());
        assert_eq!(c, 0);
        for i in 0..5 {
            let at = PikVec3 { x: -20.0 - i as f32 * 4.0, y: 0.0, z: 0.0 };
            assert!(pik_sim_spawn_piki(sim, PikiColor::Red.index() as u32, &at, c) >= 0);
        }

        // A creature 100 units ahead, where the cursor will be.
        let target = PikVec3 { x: 100.0, y: 0.0, z: 0.0 };
        let creature = [PikCreature { id: 77, pos: target, alive: 1, parts_start: 0, parts_len: 1, ..Default::default() }];
        let part = [PikPart { centre: PikVec3 { y: 15.0, ..target }, radius: 20.0, stickable: 1, _pad: [0; 3] }];

        let mut world = World::default();
        let h = host(&mut world);
        let step = |input: PikInput, secs: f32| {
            pik_sim_set_input(sim, 0, &input);
            for _ in 0..(secs * 60.0) as u32 {
                pik_sim_set_creatures(sim, creature.as_ptr(), 1, part.as_ptr(), 1);
                pik_sim_advance(sim, &h, 1.0 / 60.0);
            }
        };
        let aim = PikInput { use_cursor: 1, cursor: target, ..Default::default() };
        step(aim, 1.0);
        step(PikInput { throw: 1, ..aim }, 1.0);
        step(aim, 3.0);

        let ev = events(sim);
        assert!(ev.iter().any(|e| e.kind == event_kind::THROWN));
        assert!(ev.iter().any(|e| e.kind == event_kind::LATCHED && e.has_creature == 1 && e.creature == 77));
        assert!(world.hits.iter().any(|&(id, amount, src)| id == 77 && amount > 0.0 && src == PIK_SOURCE_PIKI));
        assert!(world.ground_calls > 0 && world.wall_calls > 0);

        let mut cap = PikCaptain::default();
        assert_eq!(pik_sim_captain(sim, 0, &mut cap), 1);
        assert!((cap.cursor_world.x - 100.0).abs() < 1.0, "cursor at {:?}", cap.cursor_world);

        let mut pikis = [PikPiki::default(); 8];
        assert_eq!(pik_sim_pikis(sim, pikis.as_mut_ptr(), 8), 5);
        assert!(pikis[..5].iter().all(|p| p.alive == 1 && p.color == PikiColor::Red.index() as u8));
        assert!(pikis[..5].iter().any(|p| p.state == piki_state::ATTACK && p.has_creature == 1 && p.creature == 77));
        assert_eq!(pik_sim_poisoned(sim), 0);
        pik_sim_free(sim);
    }
}

#[test]
fn pellets_onions_and_bad_arguments() {
    // SAFETY: every pointer below is live for its call.
    unsafe {
        let sim = pik_sim_new(std::ptr::null(), 1);
        let at = PikVec3 { x: 300.0, y: 0.0, z: 0.0 };
        assert_eq!(pik_sim_add_onion(sim, &at, 0), -1, "an Onion needs a colour");
        assert_eq!(pik_sim_add_onion(sim, &at, 0b111), 0);
        assert_eq!(pik_sim_spawn_piki(sim, 7, &at, -1), -1, "no such colour");
        let desc = PikPelletDesc {
            creature: 5,
            has_creature: 1,
            pos: PikVec3 { x: 50.0, y: 0.0, z: 0.0 },
            radius: 20.0,
            height: 20.0,
            min_carriers: 2,
            max_carriers: 4,
            seeds_matching: 3,
            seeds_other: 3,
            color: -1,
            ..Default::default()
        };
        assert_eq!(pik_sim_spawn_pellet(sim, &desc), 0);

        // A creature whose parts run past the array is kept but given no parts.
        let bad = [PikCreature { id: 1, alive: 1, parts_start: 3, parts_len: 2, ..Default::default() }];
        pik_sim_set_creatures(sim, bad.as_ptr(), 1, std::ptr::null(), 0);
        let mut world = World::default();
        let h = host(&mut world);
        pik_sim_advance(sim, &h, 0.1);
        assert_eq!(pik_sim_poisoned(sim), 0);

        let mut pel = [PikPellet::default(); 2];
        assert_eq!(pik_sim_pellets(sim, pel.as_mut_ptr(), 2), 1);
        assert_eq!((pel[0].state, pel[0].creature, pel[0].min_carriers), (0, 5, 2));
        let mut on = [PikOnion::default(); 2];
        assert_eq!(pik_sim_onions(sim, on.as_mut_ptr(), 2), 1);
        assert_eq!((on[0].colors, on[0].is_ship), (0b111, 0));
        assert!(!pik_sim_params_source(sim).is_null());
        pik_sim_free(sim);
    }
}
