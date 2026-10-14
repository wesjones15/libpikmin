//! The C ABI driven the way a C host would: raw pointers and extern "C" callbacks.

use super::*;

#[test]
fn hud_digits_cross_the_abi_and_short_buffers_are_rejected() {
    let Some(assets) = libpikmin_assets::Assets::discover(None) else { return };
    let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
    let mut info = PikTexture::default();
    let mut short = [0xAB; 4];
    unsafe {
        assert_eq!(pik_number_texture(std::ptr::null(), 0, 0, &mut info, std::ptr::null_mut(), 0), 0);
        assert_eq!(pik_number_texture(dir.as_ptr(), 10, 0, &mut info, std::ptr::null_mut(), 0), 0);
        assert_eq!(pik_number_texture(dir.as_ptr(), 0, 0, &mut info, short.as_mut_ptr(), 4), 0);
        assert_eq!(short, [0xAB; 4]);
        for digit in 0..10 {
            assert_eq!(pik_number_texture(dir.as_ptr(), digit, 0, &mut info, std::ptr::null_mut(), 0), 1);
            assert_eq!((info.width, info.height), (64, 64));
            let mut rgba = vec![0; (info.width * info.height * 4) as usize];
            assert_eq!(pik_number_texture(dir.as_ptr(), digit, 0, &mut info, rgba.as_mut_ptr(), rgba.len() as u32), 1);
            assert!(rgba.chunks_exact(4).any(|p| p[3] != 0));
        }
    }
}

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
        (s.captain, s.piki, s.pellet, s.onion, s.sprout, s.event, s.onion_counts),
        (60, 64, 64, 36, 20, 48, 16)
    );
    assert_eq!((s.model_info, s.batch, s.texture, s.draw, s.view_frame, s.particle), (40, 24, 12, 64, 52, 72));
}

#[test]
fn events_past_one_read_wait_for_the_next() {
    // SAFETY: every pointer below is live for its call.
    unsafe {
        let sim = pik_sim_new(std::ptr::null(), 3);
        let c = pik_sim_add_captain(sim, &PikVec3::default());
        for i in 0..20 {
            let at = PikVec3 { x: -20.0 - i as f32 * 2.0, y: 0.0, z: 0.0 };
            pik_sim_spawn_piki(sim, PikiColor::Red.index() as u32, &at, c);
        }
        let mut world = World::default();
        let h = host(&mut world);
        // Dismiss: one event per Pikmin and more, all in one tick.
        pik_sim_set_input(sim, 0, &PikInput { dismiss: 1, ..Default::default() });
        pik_sim_advance(sim, &h, 1.0 / 30.0);
        let mut buf = [PikEvent::default(); 2];
        let mut total = 0;
        loop {
            let n = pik_sim_events(sim, buf.as_mut_ptr(), 2);
            total += n;
            if n < 2 {
                break;
            }
        }
        assert!(total >= 1, "no events");
        // Read in twos, nothing may be lost compared with one big read of a replay.
        let sim2 = pik_sim_new(std::ptr::null(), 3);
        let c2 = pik_sim_add_captain(sim2, &PikVec3::default());
        for i in 0..20 {
            let at = PikVec3 { x: -20.0 - i as f32 * 2.0, y: 0.0, z: 0.0 };
            pik_sim_spawn_piki(sim2, PikiColor::Red.index() as u32, &at, c2);
        }
        pik_sim_set_input(sim2, 0, &PikInput { dismiss: 1, ..Default::default() });
        pik_sim_advance(sim2, &h, 1.0 / 30.0);
        assert_eq!(events(sim2).len() as u32, total);
        let mut big = [PikEvent::default(); 256];
        assert_eq!(pik_sim_events(sim2, big.as_mut_ptr(), 256), 0, "already drained");
        pik_sim_free(sim);
        pik_sim_free(sim2);
    }
}

#[test]
fn the_view_poses_a_sim_through_the_abi() {
    let Some(assets) = libpikmin_assets::Assets::discover(None) else {
        eprintln!("no extracted assets; skipping");
        return;
    };
    let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: every pointer below is live for its call.
    unsafe {
        let view = pik_view_new(dir.as_ptr(), 0);
        assert!(!view.is_null());
        assert_eq!(pik_view_model_count(view), 19);
        for m in 0..19 {
            let mut info = PikModelInfo::default();
            assert_eq!(pik_view_model(view, m, &mut info), 1);
            let mut batches = vec![PikBatch::default(); info.batch_count as usize];
            assert_eq!(pik_view_batches(view, m, batches.as_mut_ptr(), info.batch_count), info.batch_count);
            let mut idx = vec![0u32; info.index_count as usize];
            assert_eq!(pik_view_indices(view, m, idx.as_mut_ptr(), info.index_count), info.index_count);
            assert!(idx.iter().all(|&i| i < info.vertex_count), "model {m}: index out of range");
            let last = batches.last().unwrap();
            assert_eq!(last.first_index + last.index_count, info.index_count);
            assert_eq!(last.first_vertex + last.vertex_count, info.vertex_count);
            let mut uv = vec![[0f32; 2]; info.vertex_count as usize];
            assert_eq!(pik_view_uvs(view, m, uv.as_mut_ptr(), info.vertex_count), info.vertex_count);
            for b in &batches {
                if b.texture < 0 {
                    continue;
                }
                let mut t = PikTexture::default();
                assert_eq!(pik_view_texture(view, b.texture as u32, &mut t, std::ptr::null_mut(), 0), 1);
                let mut px = vec![0u8; (t.width * t.height * 4) as usize];
                assert_eq!(pik_view_texture(view, b.texture as u32, &mut t, px.as_mut_ptr(), px.len() as u32), 1);
            }
        }

        let sim = pik_sim_new(dir.as_ptr(), 1);
        let c = pik_sim_add_captain(sim, &PikVec3::default());
        for i in 0..4 {
            let at = PikVec3 { x: -20.0 - i as f32 * 4.0, y: 0.0, z: 0.0 };
            pik_sim_spawn_piki(sim, PikiColor::Blue.index() as u32, &at, c);
        }
        pik_sim_add_onion(sim, &PikVec3 { x: 200.0, y: 0.0, z: 0.0 }, 0b010);
        let mut world = World::default();
        let h = host(&mut world);
        pik_sim_set_input(sim, 0, &PikInput { throw: 1, ..Default::default() });
        let mut frame = PikViewFrame {
            dt: 1.0 / 60.0,
            forward: PikVec3 { x: 0.0, y: 0.0, z: 1.0 },
            up: PikVec3 { x: 0.0, y: 1.0, z: 0.0 },
            listener: 0,
            volume: 1.0,
            ..Default::default()
        };
        for _ in 0..30 {
            pik_sim_advance(sim, &h, 1.0 / 60.0);
            frame.lag = 1.0 - pik_sim_alpha(sim);
            pik_view_update(view, sim, &frame);
        }
        let n = pik_view_draws(view, std::ptr::null_mut(), 0);
        let mut draws = vec![PikDraw::default(); n as usize];
        assert_eq!(pik_view_draws(view, draws.as_mut_ptr(), n), n);
        // Olimar, 4 Pikmin with leaves, the Onion and its light cone, cursor and marker.
        assert_eq!(n, 1 + 8 + 2 + 2, "{draws:?}");
        // The Red Onion's cone, added to the screen like light.
        let cone = draws.iter().find(|d| d.kind == 10).expect("no light cone");
        assert_eq!(cone.model, 17);
        let mut info = PikModelInfo::default();
        pik_view_model(view, cone.model, &mut info);
        let mut batches = vec![PikBatch::default(); info.batch_count as usize];
        pik_view_batches(view, cone.model, batches.as_mut_ptr(), info.batch_count);
        assert!(batches.iter().all(|b| b.blend == 3), "{batches:?}");
        let (mut vc, mut cc) = (0u32, 0u32);
        let pos = pik_view_positions(view, &mut vc);
        let col = pik_view_colors(view, &mut cc);
        assert!(!pos.is_null() && !col.is_null());
        let last = draws.last().unwrap();
        let mut info = PikModelInfo::default();
        pik_view_model(view, last.model, &mut info);
        assert_eq!(last.first_vertex + info.vertex_count, vc);
        assert_eq!(last.first_color + info.batch_count, cc);
        let mut uc = 0u32;
        assert!(!pik_view_frame_uvs(view, &mut uc).is_null());
        assert_eq!(uc, vc);
        let mut tc = 0u32;
        let ids = std::slice::from_raw_parts(pik_view_texture_ids(view, &mut tc), tc as usize);
        assert_eq!(tc, cc);
        assert!(ids.iter().any(|&i| i >= 0));
        for &i in ids.iter().filter(|&&i| i >= 0) {
            assert_eq!(pik_view_texture(view, i as u32, std::ptr::null_mut(), std::ptr::null_mut(), 0), 1);
        }
        let pos = std::slice::from_raw_parts(pos, vc as usize);
        assert!(pos.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()));
        assert!(!pik_view_status(view).is_null());

        // Whistling makes particles, each with a texture.
        pik_sim_set_input(sim, 0, &PikInput { whistle: 1, ..Default::default() });
        for _ in 0..20 {
            pik_sim_advance(sim, &h, 1.0 / 60.0);
            frame.lag = 1.0 - pik_sim_alpha(sim);
            pik_view_update(view, sim, &frame);
        }
        let mut pc = 0u32;
        let ps = pik_view_particles(view, &mut pc);
        assert!(pc > 0 && !ps.is_null(), "no whistle particles");
        let ps = std::slice::from_raw_parts(ps, pc as usize);
        for p in ps {
            let mut t = PikTexture::default();
            assert_eq!(pik_view_particle_texture(view, p.texture, &mut t, std::ptr::null_mut(), 0), 1);
            let mut px = vec![0u8; (t.width * t.height * 4) as usize];
            assert_eq!(pik_view_particle_texture(view, p.texture, &mut t, px.as_mut_ptr(), px.len() as u32), 1);
        }
        pik_sim_free(sim);
        pik_view_free(view);
    }
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
        // 7 rounds down to a 5 pellet: five to lift it, the disc's defaults otherwise.
        assert_eq!(pik_sim_spawn_numbered_pellet(sim, 7, 1, &at, 0.0), 1);
        assert_eq!(pik_sim_spawn_numbered_pellet(sim, 1, 3, &at, 0.0), -1, "no such colour");

        // A creature whose parts run past the array is kept but given no parts.
        let bad = [PikCreature { id: 1, alive: 1, parts_start: 3, parts_len: 2, ..Default::default() }];
        pik_sim_set_creatures(sim, bad.as_ptr(), 1, std::ptr::null(), 0);
        let mut world = World::default();
        let h = host(&mut world);
        pik_sim_advance(sim, &h, 0.1);
        assert_eq!(pik_sim_poisoned(sim), 0);

        let mut pel = [PikPellet::default(); 3];
        assert_eq!(pik_sim_pellets(sim, pel.as_mut_ptr(), 3), 2);
        assert_eq!((pel[0].state, pel[0].creature, pel[0].min_carriers), (0, 5, 2));
        assert_eq!((pel[1].min_carriers, pel[1].max_carriers, pel[1].radius), (5, 10, 20.0));
        let mut on = [PikOnion::default(); 2];
        assert_eq!(pik_sim_onions(sim, on.as_mut_ptr(), 2), 1);
        assert_eq!((on[0].colors, on[0].is_ship), (0b111, 0));
        assert!(!pik_sim_params_source(sim).is_null());
        pik_sim_free(sim);
    }
}

#[test]
fn calling_pikmin_out_of_an_onion_and_putting_them_back() {
    // SAFETY: every pointer below is live for its call.
    unsafe {
        let sim = pik_sim_new(std::ptr::null(), 2);
        let c = pik_sim_add_captain(sim, &PikVec3::default()) as u32;
        let onion = pik_sim_add_onion(sim, &PikVec3 { x: 30.0, y: 0.0, z: 0.0 }, 0b010) as u32;
        let red = PikiColor::Red.index() as u32;
        let blue = PikiColor::Blue.index() as u32;
        pik_sim_store_in_onion(sim, onion, red, 0, 5);
        pik_sim_store_in_onion(sim, onion, red, 9, 5); // no such leaf stage: ignored
        pik_sim_store_in_onion(sim, 9, red, 0, 5); // no such Onion: ignored

        let mut world = World::default();
        let h = host(&mut world);
        pik_sim_advance(sim, &h, 0.1);
        assert_eq!(pik_sim_onion_in_reach(sim, c), onion as i32);
        let mut counts = PikOnionCounts::default();
        assert_eq!(pik_sim_onion_counts(sim, onion, c, red, &mut counts), 1);
        assert_eq!((counts.in_onion, counts.in_squad), (5, 0));
        assert_eq!(pik_sim_onion_counts(sim, 9, c, red, &mut counts), 0);

        assert_eq!(pik_sim_onion_call_out(sim, c, onion, blue, 3), 0, "a red Onion has no blues");
        assert_eq!(pik_sim_onion_call_out(sim, c, onion, red, 3), 3);
        for _ in 0..300 {
            pik_sim_advance(sim, &h, 1.0 / 30.0);
        }
        let mut cap = PikCaptain::default();
        pik_sim_captain(sim, c, &mut cap);
        assert_eq!(cap.squad, 3, "the three called out should have joined");

        assert_eq!(pik_sim_onion_put_away(sim, c, onion, red, 2), 2);
        for _ in 0..600 {
            pik_sim_advance(sim, &h, 1.0 / 30.0);
        }
        let mut on = [PikOnion::default(); 1];
        pik_sim_onions(sim, on.as_mut_ptr(), 1);
        assert_eq!(on[0].stored[red as usize], 4);
        assert_eq!(pik_sim_onion_in_reach(sim, 99), -1);
        assert_eq!(pik_sim_poisoned(sim), 0);
        pik_sim_free(sim);
    }
}

#[test]
fn unified_onion_flight_preserves_every_colour_and_blocks_transfers() {
    unsafe {
        let sim = pik_sim_new(std::ptr::null(), 17);
        let ground = PikVec3::default();
        let c = pik_sim_add_captain(sim, &ground) as u32;
        let o = pik_sim_add_onion(sim, &ground, 7) as u32;
        for color in 0..3 { pik_sim_store_in_onion(sim, o, color, 0, 4); }
        let air = PikVec3 { x: 20.0, y: 150.0, z: 40.0 };
        for color in 0..3 {
            pik_sim_set_onion_color(sim, o, color);
            pik_sim_set_onion_pose(sim, o, &air, 1);
            let mut onion = PikOnion::default();
            assert_eq!(pik_sim_onions(sim, &mut onion, 1), 1);
            assert_eq!((onion.colors, onion.anim, onion.stored), (7, 6, [4; 3]));
            assert_eq!(pik_sim_onion_in_reach(sim, c), -1);
            assert_eq!(pik_sim_onion_call_out(sim, c, o, color, 1), 0);
        }
        pik_sim_set_onion_pose(sim, 999, &ground, 0);
        pik_sim_set_onion_color(sim, o, 99);
        pik_sim_set_onion_pose(sim, o, &ground, 0);
        assert_eq!(pik_sim_onion_in_reach(sim, c), o as i32);
        for color in 0..3 { assert_eq!(pik_sim_onion_call_out(sim, c, o, color, 1), 1); }
        let mut world = World::default();
        let h = host(&mut world);
        for _ in 0..300 { pik_sim_advance(sim, &h, 1.0 / 30.0); }
        for color in 0..3 {
            let mut counts = PikOnionCounts::default();
            pik_sim_onion_counts(sim, o, c, color, &mut counts);
            assert_eq!((counts.in_squad, counts.in_onion), (1, 3));
        }
        pik_sim_set_onion_pose(sim, o, &air, 1);
        for color in 0..3 { assert_eq!(pik_sim_onion_put_away(sim, c, o, color, 1), 0); }
        assert_eq!(pik_sim_poisoned(sim), 0);
        pik_sim_free(sim);
    }
}
