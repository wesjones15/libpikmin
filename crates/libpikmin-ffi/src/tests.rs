//! The C ABI driven the way a C host would: raw pointers and extern "C" callbacks.

use super::*;

#[test]
fn all_disc_parts_cross_the_spawn_abi_with_valid_weights_and_ids() {
    let Some(assets) = libpikmin_assets::Assets::discover(None) else { return };
    let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
    unsafe {
        let sim = pik_sim_new(dir.as_ptr(), 1);
        assert!(!sim.is_null());
        for part in 0..30 {
            let weight = pik_sim_ship_part_weight(sim, part);
            assert!(weight > 0 && weight <= 100, "part {part} weight {weight}");
            let mut name = [0; 128];
            assert!(pik_sim_ship_part_name(sim, part, name.as_mut_ptr(), 128) > 0);
            let pos = PikVec3 { x: part as f32 * 100.0, y: 0.0, z: 0.0 };
            assert_eq!(pik_sim_spawn_ship_part(sim, part, &pos, 0.0), part as i32);
            assert_eq!((*sim).sim.pellets()[part as usize].desc.ship_part, Some(part));
        }
        let mut frames = [PikPellet::default(); 30];
        assert_eq!(pik_sim_pellets(sim, frames.as_mut_ptr(), 30), 30);
        assert!(frames.iter().all(|frame| frame.has_ship_part == 1));
        assert_eq!(pik_sim_spawn_ship_part(sim, 30, &PikVec3::default(), 0.0), -1);
        assert_eq!(pik_sim_ship_part_weight(sim, u32::MAX), 0);
        pik_sim_free(sim);
    }
}

#[test]
fn the_complete_ending_music_decodes_across_the_abi_with_bounded_buffers() {
    let Some(assets) = libpikmin_assets::Assets::discover(None) else { return };
    let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
    unsafe {
        let mut rate = 0;
        let n = pik_victory_pcm(dir.as_ptr(), std::ptr::null_mut(), 0, &mut rate);
        assert!(rate >= 8000 && n > 0 && n <= rate * 120 * 2);
        let mut short = [123.0; 4];
        assert_eq!(pik_victory_pcm(dir.as_ptr(), short.as_mut_ptr(), 4, &mut rate), 0);
        assert_eq!(short, [123.0; 4]);
        let mut pcm = vec![0.0; n as usize];
        assert_eq!(pik_victory_pcm(dir.as_ptr(), pcm.as_mut_ptr(), n, &mut rate), n);
        assert!(pcm.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        assert!(pcm.iter().any(|v| v.abs() > 0.01), "victory music is not silent");
        assert_eq!(pik_victory_pcm(std::ptr::null(), std::ptr::null_mut(), 0, &mut rate), 0);
    }
}

#[test]
fn intro_cache_errors_cross_abi_without_panics_or_buffer_overruns() {
    let mut msg = [0xAB; 64];
    unsafe {
        assert_eq!(pik_render_stings(std::ptr::null(), msg.as_mut_ptr(), 4), 0);
        assert_eq!(&msg[..4], b"mis\0");
        assert!(msg[4..].iter().all(|b| *b == 0xAB));
        assert_eq!(pik_render_stings(std::ptr::null(), std::ptr::null_mut(), 0), 0);
    }
}

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
        for glyph in 0..4 {
            assert_eq!(pik_damage_suffix_texture(dir.as_ptr(), glyph, &mut info, std::ptr::null_mut(), 0), 1);
            assert_eq!((info.width, info.height), (64, 64));
            let mut rgba = vec![0; 64 * 64 * 4];
            assert_eq!(pik_damage_suffix_texture(dir.as_ptr(), glyph, &mut info, rgba.as_mut_ptr(), rgba.len() as u32), 1);
            assert!(rgba.chunks_exact(4).any(|p| p[3] != 0));
        }
    }
}

#[test]
fn carry_strip_and_bottom_icons_cross_the_abi() {
    let Some(assets) = libpikmin_assets::Assets::discover(None) else { return };
    let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
    let mut info = PikTexture::default();
    unsafe {
        assert_eq!(pik_carry_digit_texture(dir.as_ptr(), &mut info, std::ptr::null_mut(), 0), 1);
        assert_eq!((info.width, info.height), (176, 16));
        let mut short = [0xAB; 4];
        assert_eq!(pik_carry_digit_texture(dir.as_ptr(), &mut info, short.as_mut_ptr(), 4), 0);
        assert_eq!(short, [0xAB; 4]);
        let mut rgba = vec![0; (info.width * info.height * 4) as usize];
        assert_eq!(pik_carry_digit_texture(dir.as_ptr(), &mut info, rgba.as_mut_ptr(), rgba.len() as u32), 1);
        assert!(rgba.chunks_exact(4).any(|p| p[3] != 0));
        for icon in 0..22 {
            assert_eq!(pik_hud_icon_texture(dir.as_ptr(), icon, &mut info, std::ptr::null_mut(), 0), 1);
            assert!(info.width > 0 && info.height > 0);
        }
        assert_eq!(pik_hud_icon_texture(dir.as_ptr(), 22, &mut info, std::ptr::null_mut(), 0), 0);
        for glyph in b"SHIP REPAIR 0123456789" {
            if *glyph != b' ' {
                assert_eq!(pik_font_glyph_texture(dir.as_ptr(), *glyph as u32, &mut info, std::ptr::null_mut(), 0), 1);
                assert_eq!((info.width, info.height), (64, 64));
            }
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
        (60, 72, 64, 44, 20, 48, 16)
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

/// Both tests make views, and the enemy models are a process-wide switch.
static VIEWS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn the_view_poses_a_sim_through_the_abi() {
    let _views = VIEWS.lock().unwrap_or_else(|e| e.into_inner());
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
fn host_can_remove_the_original_field_limit() {
    // SAFETY: the sim and positions remain live for every ABI call.
    unsafe {
        let sim = pik_sim_new(std::ptr::null(), 5);
        assert!(!sim.is_null());
        let pos = PikVec3::default();
        let captain = pik_sim_add_captain(sim, &pos);
        pik_sim_set_field_limit(sim, i32::MAX as u32);
        for _ in 0..150 {
            assert!(pik_sim_spawn_piki(sim, 0, &pos, captain) >= 0);
        }
        pik_sim_free(sim);
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
        assert_eq!(pik_sim_spawn_numbered_pellet(sim, 1, 5, &at, 0.0), -1, "no such colour");

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
        assert_eq!((pel[0].has_ship_part, pel[1].has_ship_part), (0, 0));
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
fn unified_onion_flight_allows_call_out_but_blocks_put_away() {
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
            assert_eq!((onion.colors, onion.anim, onion.stored), (7, 6, [4, 4, 4, 0, 0]));
            assert_eq!(pik_sim_onion_in_reach(sim, c), -1);
            assert_eq!(pik_sim_onion_call_out(sim, c, o, color, 1), 1);
        }
        pik_sim_set_onion_pose(sim, 999, &ground, 0);
        pik_sim_set_onion_color(sim, o, 99);
        pik_sim_set_onion_pose(sim, o, &ground, 0);
        assert_eq!(pik_sim_onion_in_reach(sim, c), o as i32);
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

#[test]
fn a_replica_follows_the_server_through_snapshots() {
    // SAFETY: every pointer below is live for its call.
    unsafe {
        let server = pik_sim_new(std::ptr::null(), 4);
        let c = pik_sim_add_captain(server, &PikVec3::default());
        for i in 0..30 {
            let at = PikVec3 { x: -20.0 - i as f32 * 2.0, y: 0.0, z: (i % 4) as f32 * 6.0 };
            pik_sim_spawn_piki(server, (i % 3) as u32, &at, c);
        }
        let mut world = World::default();
        let h = host(&mut world);
        let replica = pik_replica_new(std::ptr::null());
        let mut bytes = vec![0u8; 64 * 1024];
        let mut seen = 0;
        for tick in 0..90 {
            pik_sim_set_input(server, 0, &PikInput { stick_x: 0.5, whistle: (tick == 30) as u8, ..Default::default() });
            pik_sim_advance(server, &h, 1.0 / 30.0);
            if tick % 3 == 2 {
                let n = pik_sim_snapshot(server, bytes.as_mut_ptr(), bytes.len() as u32);
                assert!(n > 0, "a snapshot fits");
                assert_eq!(pik_replica_apply(replica, bytes.as_ptr(), n), 1);
                seen += 1;
            }
            pik_replica_update(replica, 1.0 / 30.0);
        }
        assert!(seen >= 20);
        // The replica has the server's Pikmin, within a snapshot's travel of where they are.
        let mut a = vec![PikPiki::default(); 64];
        let mut b = vec![PikPiki::default(); 64];
        let na = pik_sim_pikis(server, a.as_mut_ptr(), 64) as usize;
        let nb = pik_sim_pikis(replica, b.as_mut_ptr(), 64) as usize;
        assert_eq!((na, nb), (30, 30));
        let worst = a[..na].iter().zip(&b[..nb]).map(|(x, y)| {
            let d = (x.pos.x - y.pos.x).hypot(x.pos.z - y.pos.z);
            assert_eq!(x.color, y.color);
            d
        }).fold(0.0f32, f32::max);
        assert!(worst < 120.0, "the replica is {worst} units off");
        // Showing the captain as another sim's: a mover that has run ahead of the server.
        let mover = pik_sim_new(std::ptr::null(), 5);
        pik_sim_add_captain(mover, &PikVec3::default());
        pik_sim_set_input(mover, 0, &PikInput { stick_x: 1.0, ..Default::default() });
        for _ in 0..20 {
            pik_sim_advance(mover, &h, 1.0 / 30.0);
        }
        let mut ahead = PikCaptain::default();
        pik_sim_captain(mover, 0, &mut ahead);
        pik_replica_update(replica, 0.0);
        pik_replica_overlay_captain(replica, 0, mover, 0, std::ptr::null(), 0.0);
        let mut shown = PikCaptain::default();
        pik_sim_captain(replica, 0, &mut shown);
        assert_eq!(shown.pos.x, ahead.pos.x, "the replica's captain is where the mover has him");
        let smoothed = PikVec3 { x: ahead.pos.x + 1.0, y: ahead.pos.y, z: ahead.pos.z };
        pik_replica_overlay_captain(replica, 0, mover, 0, &smoothed, 0.5);
        pik_sim_captain(replica, 0, &mut shown);
        assert_eq!((shown.pos.x, shown.face), (smoothed.x, 0.5), "or where the host says");
        // A plain sim is left alone.
        let before = ahead.pos.x;
        pik_replica_overlay_captain(mover, 0, mover, 0, &smoothed, 0.5);
        pik_sim_captain(mover, 0, &mut ahead);
        assert_eq!(ahead.pos.x, before);
        pik_sim_free(mover);
        // A replica only takes snapshots, and refuses nonsense and a plain sim.
        assert_eq!(pik_replica_apply(replica, bytes.as_ptr(), 5), 0);
        assert_eq!(pik_replica_apply(server, bytes.as_ptr(), 10), 0, "not a replica");
        // Too small a buffer takes nothing, so the events are still there for the next try.
        let mut tiny = [0u8; 8];
        assert_eq!(pik_sim_snapshot(server, tiny.as_mut_ptr(), 8), 0);
        pik_sim_free(server);
        pik_sim_free(replica);
    }
}

#[test]
fn delayed_snapshots_do_not_rewind_the_replica_animation_clock() {
    unsafe {
        let server = pik_sim_new(std::ptr::null(), 4);
        let replica = pik_replica_new(std::ptr::null());
        let mut bytes = vec![0; 4096];
        (*server).sim.set_tick(30);
        let n = pik_sim_snapshot(server, bytes.as_mut_ptr(), bytes.len() as u32);
        assert_eq!(pik_replica_apply(replica, bytes.as_ptr(), n), 1);
        pik_replica_update(replica, 0.25);
        let before = (*replica).sim.tick();
        assert!(before > 33);
        (*server).sim.set_tick(33);
        let n = pik_sim_snapshot(server, bytes.as_mut_ptr(), bytes.len() as u32);
        assert_eq!(pik_replica_apply(replica, bytes.as_ptr(), n), 1);
        assert_eq!((*replica).sim.tick(), before);
        pik_replica_update(replica, 0.01);
        assert!((*replica).sim.tick() >= before);
        pik_sim_free(server);
        pik_sim_free(replica);
    }
}

#[test]
fn burst_snapshots_do_not_play_old_spatial_cues_on_a_recycled_pikmin() {
    unsafe {
        let replica = pik_replica_new(std::ptr::null());
        let mut source = Sim::default();
        let id = source.spawn_piki(PikiColor::Red, Vec3::ZERO).unwrap();
        source.set_tick(1);
        let ghost = Event::PikiGone { piki: id, pos: Vec3::X * 10.0 };
        let bytes = Snapshot::of(&source, &[Event::Landed { piki: id }, ghost]).encode_replica();
        assert_eq!(pik_replica_apply(replica, bytes.as_ptr(), bytes.len() as u32), 1);
        source.piki_mut(id).state = PikiState::Dead;
        source.reclaim_dead_pikis(std::iter::empty());
        source.spawn_piki(PikiColor::Blue, Vec3::X * 30.0).unwrap();
        source.set_tick(2);
        let bytes = Snapshot::of(&source, &[]).encode_replica();
        assert_eq!(pik_replica_apply(replica, bytes.as_ptr(), bytes.len() as u32), 1);
        assert_eq!((*replica).view_log, vec![ghost]);
        pik_sim_free(replica);
    }
}

#[test]
fn teki_structs_are_what_hosts_mirror() {
    // ror2-olimar's Native.TekiInfo / TekiInstance.
    let mut s = [0u32; 2];
    unsafe { crate::view::pik_view_teki_sizes(s.as_mut_ptr()) };
    assert_eq!(s, [4 * 22 + 12 * 3 + 28 * 10, 44]);
}

#[test]
fn the_view_draws_a_hosts_enemies() {
    let _views = VIEWS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(assets) = libpikmin_assets::Assets::discover(None) else {
        eprintln!("no extracted assets; skipping");
        return;
    };
    if !assets.root.join("tekis/swallow/swallow.mod").exists() {
        eprintln!("no enemy models; skipping");
        return;
    }
    let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: every pointer below is live for its call.
    unsafe {
        crate::view::pik_view_set_teki_models(1);
        let view = pik_view_new(dir.as_ptr(), 0);
        crate::view::pik_view_set_teki_models(0);
        assert!(!view.is_null());
        let sim = pik_sim_new(dir.as_ptr(), 1);
        let mut infos = [crate::view::PikTekiInfo::default(); 2];
        for k in 0..2 {
            assert_eq!(crate::view::pik_view_teki_info(view, k, &mut infos[k as usize]), 1);
        }
        assert_eq!(crate::view::pik_view_teki_info(view, 99, &mut infos[0]), 0);
        assert!(infos[1].life > infos[0].life && infos[1].model != infos[0].model);
        let list = [
            crate::view::PikTekiInstance {
                id: 5, kind: 0, motion: infos[0].motions[6].index, frame: 30.0, scale: 1.0, alpha: 1.0, flash: 0.1,
                ..Default::default()
            },
            crate::view::PikTekiInstance {
                id: 9, kind: 1, motion: infos[1].motions[2].index, frame: 3.0, scale: 0.5, alpha: 1.0,
                pos: PikVec3 { x: 100.0, y: 0.0, z: 0.0 }, yaw: 1.0, ..Default::default()
            },
        ];
        let frame = PikViewFrame {
            dt: 1.0 / 60.0,
            forward: PikVec3 { x: 0.0, y: 0.0, z: 1.0 },
            up: PikVec3 { x: 0.0, y: 1.0, z: 0.0 },
            listener: -1,
            volume: 0.0,
            ..Default::default()
        };
        crate::view::pik_view_set_tekis(view, list.as_ptr(), 2);
        pik_view_update(view, sim, &frame);
        let n = pik_view_draws(view, std::ptr::null_mut(), 0);
        let mut draws = vec![PikDraw::default(); n as usize];
        pik_view_draws(view, draws.as_mut_ptr(), n);
        let tekis: Vec<_> = draws.iter().filter(|d| d.kind == 12).collect();
        assert_eq!(tekis.len(), 2, "{draws:?}");
        assert_eq!((tekis[0].id, tekis[0].model, tekis[0].flags & 1), (5, infos[0].model, 1));
        assert_eq!((tekis[1].id, tekis[1].model), (9, infos[1].model));
        // Skinned vertices are in a draw of the model's own size and the half-size one is smaller.
        let mut m = PikModelInfo::default();
        pik_view_model(view, infos[1].model, &mut m);
        let mut count = 0u32;
        let pos = std::slice::from_raw_parts(pik_view_positions(view, &mut count), count as usize);
        let first = tekis[1].first_vertex as usize;
        let top = pos[first..first + m.vertex_count as usize].iter().map(|p| p.y).fold(f32::MIN, f32::max);
        assert!(top > 5.0 && top < infos[1].hi.y, "half-size Spotty is {top} tall of {}", infos[1].hi.y);
        // Not asked for again: gone next frame.
        pik_view_update(view, sim, &frame);
        let n = pik_view_draws(view, std::ptr::null_mut(), 0);
        let mut draws = vec![PikDraw::default(); n as usize];
        pik_view_draws(view, draws.as_mut_ptr(), n);
        assert!(draws.iter().all(|d| d.kind != 12));
        pik_sim_free(sim);
        pik_view_free(view);
    }
}

/// Ship Repair's view: the ship's parts and every enemy beside the usual models bake more textures than the
/// variant cap, which once sent the cache into endless recursion (a stack overflow when the view loaded).
#[test]
fn the_ship_repair_view_loads_the_parts_and_every_enemy() {
    let _views = VIEWS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(assets) = libpikmin_assets::Assets::discover(None) else {
        eprintln!("no extracted assets; skipping");
        return;
    };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("enemy models not all extracted; skipping");
        return;
    }
    let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
    // A thread with Unity's main-thread stack.
    let made = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            // SAFETY: the pointer is live for the call.
            unsafe {
                crate::view::pik_view_set_repaired_ship(1);
                crate::view::pik_view_set_teki_models(1);
                let view = pik_view_new(dir.as_ptr(), 0);
                crate::view::pik_view_set_teki_models(0);
                crate::view::pik_view_set_repaired_ship(0);
                !view.is_null()
            }
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(made, "the Ship Repair view did not load");
}

#[test]
fn every_enemy_kind_draws_and_sounds() {
    let _views = VIEWS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(assets) = libpikmin_assets::Assets::discover(None) else {
        eprintln!("no extracted assets; skipping");
        return;
    };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("enemy models not all extracted; skipping");
        return;
    }
    let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
    let kinds = libpikmin_present::teki::KINDS as u32;
    // SAFETY: every pointer below is live for its call.
    unsafe {
        crate::view::pik_view_set_teki_models(1);
        let view = pik_view_new(dir.as_ptr(), 0);
        crate::view::pik_view_set_teki_models(0);
        assert!(!view.is_null());
        let sim = pik_sim_new(dir.as_ptr(), 1);
        let mut models = std::collections::HashSet::new();
        let mut list = Vec::new();
        for k in 0..kinds {
            let mut info = crate::view::PikTekiInfo::default();
            assert_eq!(crate::view::pik_view_teki_info(view, k, &mut info), 1, "kind {k} not loaded");
            assert!(models.insert(info.model), "kind {k} shares a model");
            assert!(info.life > 0.0 && info.hi.y > info.lo.y);
            list.push(crate::view::PikTekiInstance {
                id: k + 1, kind: k, motion: info.motions[2].index, frame: 5.0, scale: 1.0, alpha: 1.0,
                pos: PikVec3 { x: k as f32 * 60.0, y: 0.0, z: 0.0 }, ..Default::default()
            });
        }
        let frame = PikViewFrame {
            dt: 1.0 / 30.0,
            forward: PikVec3 { x: 0.0, y: 0.0, z: 1.0 },
            up: PikVec3 { x: 0.0, y: 1.0, z: 0.0 },
            listener: -1,
            ..Default::default()
        };
        // Two frames a second apart: a looping walk passes its animation's sound keys, and the death smoke of a
        // medium enemy (EFF_Teki_DeathSmokeM = 60) is a real effect.
        for f in [5.0, 35.0] {
            for t in list.iter_mut() { t.frame = f; }
            crate::view::pik_view_set_tekis(view, list.as_ptr(), kinds);
            pik_view_update(view, sim, &frame);
        }
        let n = pik_view_draws(view, std::ptr::null_mut(), 0);
        let mut draws = vec![PikDraw::default(); n as usize];
        pik_view_draws(view, draws.as_mut_ptr(), n);
        assert_eq!(draws.iter().filter(|d| d.kind == 12).count() as u32, kinds);
        let at = PikVec3::default();
        assert_eq!(crate::view::pik_view_spawn_effect(view, 60, &at), 1);
        assert_eq!(crate::view::pik_view_spawn_effect(view, 9999, &at), 0);
        pik_sim_free(sim);
        pik_view_free(view);
    }
}

#[test]
fn teki_ai_structs_are_what_hosts_mirror() {
    // ror2-olimar's TekiAiNative.Navi / State / Out.
    let mut s = [0u32; 3];
    unsafe { crate::teki_ai::pik_tekis_sizes(s.as_mut_ptr()) };
    assert_eq!(s, [24, 60, 72]);
}

#[test]
fn an_enemy_with_its_own_ai_runs_through_the_abi() {
    use crate::teki_ai::*;
    let Some(assets) = libpikmin_assets::Assets::discover(None) else {
        eprintln!("no extracted assets; skipping");
        return;
    };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("enemy files not all extracted; skipping");
        return;
    }
    let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: every pointer below is live for its call.
    unsafe {
        let sim = pik_sim_new(dir.as_ptr(), 1);
        let t = pik_tekis_new(dir.as_ptr(), 3);
        assert!(!t.is_null());
        assert_eq!(pik_tekis_has_strategy(t, 0), 1, "Dwarf Bulborb");
        assert_eq!(pik_tekis_has_strategy(t, 18), 1, "Honeywisp");
        assert_eq!(pik_tekis_has_strategy(t, 19), 1, "Breadbug");
        let at = PikVec3::default();
        let id = pik_tekis_spawn(t, 0, 4242, &at, 0.0);
        assert!(id > 0);
        pik_tekis_damage(t, id as u32, 5.0, -1);
        let navi = PikNavi { id: 1, pos: PikVec3 { x: 0.0, y: 0.0, z: 60.0 }, size: 10.0, alive: 1, buried: 0, _pad: [0; 2] };
        for _ in 0..30 {
            pik_tekis_update(t, sim, &navi, 1, 1);
        }
        let mut st = PikTekiState::default();
        assert_eq!(pik_tekis_state(t, id as u32, &mut st), 1);
        assert_eq!(st.flags & 2, 2, "alive");
        assert!(st.hp > 0.0 && st.hp < 130.0 + 1.0);
        assert_eq!(pik_tekis_state(t, 999, &mut st), 0);
        pik_tekis_remove(t, id as u32);
        assert_eq!(pik_tekis_state(t, id as u32, &mut st), 0);
        pik_tekis_free(t);
        pik_sim_free(sim);
    }
}

/// Olimar's Pianta skin: loaded into the model table, worn by a captain, standing, walking, jumping and
/// whistling, always above the ground and about as tall as the host asked.
#[test]
fn the_pianta_skin_draws_through_the_abi() {
    let _views = VIEWS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(assets) = libpikmin_assets::Assets::discover(None) else {
        eprintln!("SKIP: no extracted assets");
        return;
    };
    let sms = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../libsms/assets");
    if !["npc/montem/mom_model.bmd", "npc/montemcommon/i_mom_mino_rgba.bti", "bmd/ma_mdl1.bmd"].iter().all(|f| sms.join(f).is_file()) {
        eprintln!("SKIP: no Sunshine assets");
        return;
    }
    let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
    let sms = CString::new(sms.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: every pointer below is live for its call.
    unsafe {
        let view = pik_view_new(dir.as_ptr(), 0);
        assert!(!view.is_null());
        assert_eq!(pik_view_load_pianta(view, sms.as_ptr(), 0, u32::MAX), 19);
        assert_eq!(pik_view_model_count(view), 20);
        let mut info = PikModelInfo::default();
        assert_eq!(pik_view_model(view, 19, &mut info), 1);
        let mut batches = vec![PikBatch::default(); info.batch_count as usize];
        assert_eq!(pik_view_batches(view, 19, batches.as_mut_ptr(), info.batch_count), info.batch_count);
        let mut idx = vec![0u32; info.index_count as usize];
        assert_eq!(pik_view_indices(view, 19, idx.as_mut_ptr(), info.index_count), info.index_count);
        assert!(idx.iter().all(|&i| i < info.vertex_count));
        for b in batches.iter().filter(|b| b.texture >= 0) {
            let mut t = PikTexture::default();
            assert_eq!(pik_view_texture(view, b.texture as u32, &mut t, std::ptr::null_mut(), 0), 1);
        }
        pik_view_set_pianta_height(view, 30.0);

        let sim = pik_sim_new(dir.as_ptr(), 1);
        let c = pik_sim_add_captain(sim, &PikVec3::default());
        pik_view_set_captain_skin(view, c as u32, 2);
        let mut world = World::default();
        let h = host(&mut world);
        let mut frame = PikViewFrame {
            dt: 1.0 / 60.0,
            forward: PikVec3 { x: 0.0, y: 0.0, z: 1.0 },
            up: PikVec3 { x: 0.0, y: 1.0, z: 0.0 },
            listener: -1,
            volume: 1.0,
            ..Default::default()
        };
        // Standing, walking, running, jumping, whistling: the Pianta's feet stay on the ground (or above it).
        let inputs = [
            PikInput::default(),
            PikInput { stick_y: 0.4, ..Default::default() },
            PikInput { stick_y: 1.0, ..Default::default() },
            PikInput { jump: 1, ..Default::default() },
            PikInput { whistle: 1, ..Default::default() },
        ];
        for input in inputs {
            pik_sim_set_input(sim, 0, &input);
            for _ in 0..40 {
                pik_sim_advance(sim, &h, 1.0 / 60.0);
                frame.lag = 1.0 - pik_sim_alpha(sim);
                pik_view_update(view, sim, &frame);
                let n = pik_view_draws(view, std::ptr::null_mut(), 0);
                let mut draws = vec![PikDraw::default(); n as usize];
                pik_view_draws(view, draws.as_mut_ptr(), n);
                let me = draws.iter().find(|d| d.kind == 0).expect("no captain");
                assert_eq!(me.model, 19, "the captain is not the Pianta");
                let mut vc = 0u32;
                let pos = std::slice::from_raw_parts(pik_view_positions(view, &mut vc), vc as usize);
                let mine = &pos[me.first_vertex as usize..(me.first_vertex + info.vertex_count) as usize];
                assert!(mine.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()));
                let (lo, hi) = mine.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p.y), h.max(p.y)));
                assert!(lo.abs() < 0.01, "feet at {lo}");
                assert!(hi > 20.0 && hi < 60.0, "stands {hi} tall");
            }
        }
        // Back to Olimar.
        pik_view_set_captain_skin(view, c as u32, 0);
        pik_view_update(view, sim, &frame);
        let n = pik_view_draws(view, std::ptr::null_mut(), 0);
        let mut draws = vec![PikDraw::default(); n as usize];
        pik_view_draws(view, draws.as_mut_ptr(), n);
        assert_eq!(draws.iter().find(|d| d.kind == 0).unwrap().model, 0);
        pik_view_free(view);
    }
}

/// Olimar's Louie skin: his Pikmin 2 body joins the table and draws on a captain through standing, walking,
/// running, jumping and whistling, feet on the ground, as tall as Olimar.
#[test]
fn the_louie_skin_draws_through_the_abi() {
    let _views = VIEWS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(assets) = libpikmin_assets::Assets::discover(None) else {
        eprintln!("SKIP: no extracted assets");
        return;
    };
    let p2 = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../libsms/assets/p2");
    let p2s = CString::new(p2.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: every pointer below is live for its call.
    unsafe {
        if pik_p2_complete(p2s.as_ptr()) == 0 {
            eprintln!("SKIP: no Pikmin 2 cache");
            return;
        }
        let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
        let view = pik_view_new(dir.as_ptr(), 0);
        assert!(!view.is_null());
        let model = pik_view_load_louie(view, p2s.as_ptr());
        assert_eq!(model, 19);
        assert_eq!(pik_view_model_count(view), 21);
        let mut info = PikModelInfo::default();
        assert_eq!(pik_view_model(view, 19, &mut info), 1);
        let mut idx = vec![0u32; info.index_count as usize];
        assert_eq!(pik_view_indices(view, 19, idx.as_mut_ptr(), info.index_count), info.index_count);
        assert!(idx.iter().all(|&i| i < info.vertex_count));
        let sim = pik_sim_new(dir.as_ptr(), 1);
        let c = pik_sim_add_captain(sim, &PikVec3::default());
        pik_view_set_captain_skin(view, c as u32, 1);
        let mut world = World::default();
        let h = host(&mut world);
        let mut frame = PikViewFrame {
            dt: 1.0 / 60.0,
            forward: PikVec3 { x: 0.0, y: 0.0, z: 1.0 },
            up: PikVec3 { x: 0.0, y: 1.0, z: 0.0 },
            listener: -1,
            volume: 1.0,
            ..Default::default()
        };
        let inputs = [
            PikInput::default(),
            PikInput { stick_y: 0.4, ..Default::default() },
            PikInput { stick_y: 1.0, ..Default::default() },
            PikInput { jump: 1, ..Default::default() },
            PikInput { whistle: 1, ..Default::default() },
        ];
        for input in inputs {
            pik_sim_set_input(sim, 0, &input);
            for _ in 0..40 {
                pik_sim_advance(sim, &h, 1.0 / 60.0);
                frame.lag = 1.0 - pik_sim_alpha(sim);
                pik_view_update(view, sim, &frame);
                let n = pik_view_draws(view, std::ptr::null_mut(), 0);
                let mut draws = vec![PikDraw::default(); n as usize];
                pik_view_draws(view, draws.as_mut_ptr(), n);
                let me = draws.iter().find(|d| d.kind == 0).expect("no captain");
                assert_eq!(me.model, 19, "the captain is not Louie");
                let mut vc = 0u32;
                let pos = std::slice::from_raw_parts(pik_view_positions(view, &mut vc), vc as usize);
                let mine = &pos[me.first_vertex as usize..(me.first_vertex + info.vertex_count) as usize];
                assert!(mine.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()));
                let (lo, hi) = mine.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p.y), h.max(p.y)));
                assert!(lo.abs() < 0.01, "feet at {lo}");
                assert!(hi > 10.0 && hi < 45.0, "stands {hi} tall, pos {:?} in {input:?}", mine.iter().fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |a, p| (a.0.min(p.x), a.1.min(p.z), a.2.max(p.x), a.3.max(p.z))));
                // Standing still he is about as tall as Olimar (25.1).
                if input.stick_y == 0.0 && input.jump == 0 && input.whistle == 0 {
                    assert!((20.0..31.0).contains(&hi), "idle Louie is {hi} tall");
                }
            }
        }
        // His ship: an Onion that is a ship draws as the Hocotate Ship when asked to.
        let ship = pik_sim_add_ship(sim, &PikVec3 { x: 120.0, y: 0.0, z: 0.0 }, 0.0);
        assert!(ship >= 0);
        let mut draw_ship = |on: u8| {
            pik_view_set_ship_hocotate(view, ship as u32, on);
            for _ in 0..4 {
                pik_sim_advance(sim, &h, 1.0 / 60.0);
                frame.lag = 1.0 - pik_sim_alpha(sim);
                pik_view_update(view, sim, &frame);
            }
            let n = pik_view_draws(view, std::ptr::null_mut(), 0);
            let mut draws = vec![PikDraw::default(); n as usize];
            pik_view_draws(view, draws.as_mut_ptr(), n);
            draws.iter().find(|d| d.kind == 9).map(|d| d.model)
        };
        let dolphin = draw_ship(0);
        let hocotate = draw_ship(1);
        assert!(dolphin.is_some(), "no ship drawn");
        assert!(hocotate.is_some_and(|m| m != dolphin.unwrap() && m >= 19), "{dolphin:?} {hocotate:?}");
        // His navi light burns over him: four additive glow layers among the particles.
        let mut lights = 0u32;
        pik_view_particles(view, &mut lights);
        assert!(lights >= 4, "{lights} particle quads");
        pik_view_free(view);
    }
}

/// Purple and White Pikmin draw as Pikmin 2's bodies, feet on the ground, about a Pikmin tall; without the
/// Pikmin 2 files they are still drawn (tinted Pikmin 1 bodies).
#[test]
fn purple_and_white_pikmin_draw_through_the_abi() {
    let _views = VIEWS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(assets) = libpikmin_assets::Assets::discover(None) else {
        eprintln!("SKIP: no extracted assets");
        return;
    };
    let p2 = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../libsms/assets/p2");
    let p2s = CString::new(p2.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: every pointer below is live for its call.
    unsafe {
        let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
        let with_p2 = pik_p2_complete(p2s.as_ptr()) != 0;
        let view = pik_view_new(dir.as_ptr(), 0);
        assert!(!view.is_null());
        let before = pik_view_model_count(view);
        let purple_model = if with_p2 { pik_view_load_pikmin2(view, p2s.as_ptr()) } else { -1 };
        if with_p2 {
            assert_eq!(purple_model as u32, before);
            assert_eq!(pik_view_model_count(view), before + 2);
        } else {
            eprintln!("no Pikmin 2 cache: checking the fallback only");
        }
        let sim = pik_sim_new(dir.as_ptr(), 1);
        let c = pik_sim_add_captain(sim, &PikVec3::default());
        for (k, color) in [PikiColor::Blue, PikiColor::Purple, PikiColor::White].into_iter().enumerate() {
            let at = PikVec3 { x: -20.0 - k as f32 * 6.0, y: 0.0, z: 0.0 };
            assert!(pik_sim_spawn_piki(sim, color.index() as u32, &at, c) >= 0);
        }
        let mut world = World::default();
        let h = host(&mut world);
        let frame = PikViewFrame {
            dt: 1.0 / 60.0,
            forward: PikVec3 { x: 0.0, y: 0.0, z: 1.0 },
            up: PikVec3 { x: 0.0, y: 1.0, z: 0.0 },
            listener: -1,
            volume: 1.0,
            ..Default::default()
        };
        pik_sim_set_input(sim, 0, &PikInput { stick_y: 0.6, ..Default::default() });
        for _ in 0..90 {
            pik_sim_advance(sim, &h, 1.0 / 60.0);
            pik_view_update(view, sim, &frame);
        }
        let n = pik_view_draws(view, std::ptr::null_mut(), 0);
        let mut draws = vec![PikDraw::default(); n as usize];
        pik_view_draws(view, draws.as_mut_ptr(), n);
        let pikmin: Vec<_> = draws.iter().filter(|d| d.kind == 1).collect();
        assert_eq!(pikmin.len(), 3);
        if with_p2 {
            let mut vc = 0u32;
            let pos = std::slice::from_raw_parts(pik_view_positions(view, &mut vc), vc as usize);
            for model in [purple_model as u32, purple_model as u32 + 1] {
                let d = pikmin.iter().find(|d| d.model == model).unwrap_or_else(|| panic!("no Pikmin on model {model}"));
                let mut info = PikModelInfo::default();
                assert_eq!(pik_view_model(view, model, &mut info), 1);
                let mine = &pos[d.first_vertex as usize..(d.first_vertex + info.vertex_count) as usize];
                assert!(mine.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()));
                let (lo, hi) = mine.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p.y), h.max(p.y)));
                assert!(lo.abs() < 0.01, "feet at {lo}");
                assert!(hi > 4.0 && hi < 40.0, "stands {hi} tall");
            }
        }
        pik_view_free(view);
    }
}

/// Olimar's Pikachu skin: Pikachu draws on the captain and his Pikmin draw as Ice Climbers, small, with
/// their feet on the ground.
#[test]
fn the_pikachu_skin_draws_pikachu_and_his_climbers_through_the_abi() {
    let _views = VIEWS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(assets) = libpikmin_assets::Assets::discover(None) else {
        eprintln!("SKIP: no extracted assets");
        return;
    };
    let melee = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../libssbm/assets");
    let melee_c = CString::new(melee.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: every pointer below is live for its call.
    unsafe {
        if pik_ssbm_ice_climbers_complete(melee_c.as_ptr()) == 0 || !melee.join("PlPkNr.dat").is_file() {
            eprintln!("SKIP: no Melee cache with Pikachu and the Ice Climbers");
            return;
        }
        let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
        let view = pik_view_new(dir.as_ptr(), 0);
        assert!(!view.is_null());
        assert_eq!(pik_view_load_pikachu(view, melee_c.as_ptr()), 19);
        assert_eq!(pik_view_model_count(view), 28);
        for m in 19..28 {
            let mut info = PikModelInfo::default();
            assert_eq!(pik_view_model(view, m, &mut info), 1);
            let mut idx = vec![0u32; info.index_count as usize];
            assert_eq!(pik_view_indices(view, m, idx.as_mut_ptr(), info.index_count), info.index_count);
            assert!(idx.iter().all(|&i| i < info.vertex_count), "model {m}");
            let mut batches = vec![PikBatch::default(); info.batch_count as usize];
            assert_eq!(pik_view_batches(view, m, batches.as_mut_ptr(), info.batch_count), info.batch_count);
            for b in batches.iter().filter(|b| b.texture >= 0) {
                let mut t = PikTexture::default();
                assert_eq!(pik_view_texture(view, b.texture as u32, &mut t, std::ptr::null_mut(), 0), 1);
            }
        }
        let sim = pik_sim_new(dir.as_ptr(), 1);
        let c = pik_sim_add_captain(sim, &PikVec3::default());
        // Thirty Pikmin of the three colours: the squad is dressed from every costume the disc has.
        for i in 0..30 {
            let color = PikiColor::from_index(i);
            let at = PikVec3 { x: -20.0 - (i % 10) as f32 * 5.0, y: 0.0, z: (i / 10) as f32 * 6.0 };
            pik_sim_spawn_piki(sim, color.index() as u32, &at, c);
        }
        pik_view_set_captain_skin(view, c as u32, 3);
        let mut world = World::default();
        let h = host(&mut world);
        let mut frame = PikViewFrame {
            dt: 1.0 / 60.0,
            forward: PikVec3 { x: 0.0, y: 0.0, z: 1.0 },
            up: PikVec3 { x: 0.0, y: 1.0, z: 0.0 },
            listener: -1,
            volume: 1.0,
            ..Default::default()
        };
        pik_sim_set_input(sim, 0, &PikInput { stick_y: 0.6, ..Default::default() });
        for _ in 0..90 {
            pik_sim_advance(sim, &h, 1.0 / 60.0);
            frame.lag = 1.0 - pik_sim_alpha(sim);
            pik_view_update(view, sim, &frame);
        }
        let n = pik_view_draws(view, std::ptr::null_mut(), 0);
        let mut draws = vec![PikDraw::default(); n as usize];
        pik_view_draws(view, draws.as_mut_ptr(), n);
        let me = draws.iter().find(|d| d.kind == 0).expect("no captain");
        assert_eq!(me.model, 19, "the captain is not Pikachu");
        let climbers: Vec<_> = draws.iter().filter(|d| d.kind == 1).collect();
        assert_eq!(climbers.len(), 30);
        let mut models: Vec<u32> = climbers.iter().map(|d| d.model).collect();
        models.sort();
        models.dedup();
        assert!(models.iter().all(|&m| (20..28).contains(&m)), "{models:?}");
        assert!(models.len() >= 6, "the squad wears several costumes: {models:?}");
        let mut vc = 0u32;
        let pos = std::slice::from_raw_parts(pik_view_positions(view, &mut vc), vc as usize);
        for d in me.model..=27 {
            let Some(draw) = draws.iter().find(|x| x.model == d && (x.kind == 0 || x.kind == 1)) else { continue };
            let mut info = PikModelInfo::default();
            pik_view_model(view, d, &mut info);
            let mine = &pos[draw.first_vertex as usize..(draw.first_vertex + info.vertex_count) as usize];
            assert!(mine.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()));
            let (lo, hi) = mine.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p.y), h.max(p.y)));
            assert!(lo.abs() < 0.01, "model {d}: feet at {lo}");
            let want = if d == 19 { 25.1 } else { 25.1 * 0.42 };
            assert!(hi > want * 0.6 && hi < want * 1.5, "model {d} stands {hi}, wanted about {want}");
        }
        pik_view_free(view);
    }
}

/// Pikmin 2's treasures cross the ABI as ship-part pellets with ids from `TREASURE_PART_BASE` (skips without the
/// Pikmin 2 treasure cache in `<assets>/p2/treasure`).
#[test]
fn treasures_cross_the_spawn_abi() {
    let Some(assets) = libpikmin_assets::Assets::discover(None) else { return };
    if !assets.root.join("p2/treasure/config/otakara_config.txt").is_file() {
        eprintln!("SKIP: no Pikmin 2 treasure cache");
        return;
    }
    let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
    unsafe {
        let sim = pik_sim_new(dir.as_ptr(), 1);
        assert!(!sim.is_null());
        assert_eq!(pik_sim_treasure_count(sim), 188);
        let mut usable = 0;
        let mut name = [0u8; 128];
        for i in 0..188 {
            let weight = pik_sim_treasure_weight(sim, i);
            if weight == 0 { continue }
            usable += 1;
            assert!(pik_sim_treasure_value(sim, i) >= 10, "treasure {i}");
            assert!(pik_sim_treasure_name(sim, i, name.as_mut_ptr(), 128) > 0, "treasure {i}");
        }
        assert_eq!(usable, 188);
        // Rubber Ugly (index 0): 8 to lift, 90 Pokos.
        assert_eq!(pik_sim_treasure_weight(sim, 0), 8);
        assert_eq!(pik_sim_treasure_value(sim, 0), 90);
        let n = pik_sim_treasure_name(sim, 0, name.as_mut_ptr(), 128) as usize;
        assert_eq!(std::str::from_utf8(&name[..n]).unwrap(), "Rubber Ugly");
        let pos = PikVec3 { x: 10.0, y: 0.0, z: 0.0 };
        let id = pik_sim_spawn_treasure(sim, 5, &pos, 0.0);
        assert!(id >= 0);
        assert_eq!((*sim).sim.pellets()[id as usize].desc.ship_part, Some(libpikmin::TREASURE_PART_BASE + 5));
        let mut frame = [PikPellet::default(); 1];
        assert_eq!(pik_sim_pellets(sim, frame.as_mut_ptr(), 1), 1);
        assert_eq!(frame[0].has_ship_part, 1);
        assert_eq!(pik_sim_spawn_treasure(sim, 188, &pos, 0.0), -1);
        assert_eq!(pik_sim_treasure_weight(sim, u32::MAX), 0);
        pik_sim_free(sim);
    }
}

#[test]
fn pikmin_2_enemies_draw_and_run_through_the_abi() {
    use crate::teki_ai::*;
    let _views = VIEWS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(assets) = libpikmin_assets::Assets::discover(None) else {
        eprintln!("no extracted assets; skipping");
        return;
    };
    if !assets.root.join("tekis/.all").exists() || !libsms::extract::p2_enemy::is_complete(&assets.root.join("p2")) {
        eprintln!("SKIP: the Pikmin 1 enemy files or the Pikmin 2 enemy cache are not extracted");
        return;
    }
    let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
    let first = libpikmin_teki::p2::FIRST as u32;
    let count = libpikmin_teki::p2::KIND_TABLE.len() as u32;
    // SAFETY: every pointer below is live for its call.
    unsafe {
        crate::view::pik_view_set_teki_models(1);
        let view = pik_view_new(dir.as_ptr(), 0);
        crate::view::pik_view_set_teki_models(0);
        assert!(!view.is_null());
        let sim = pik_sim_new(dir.as_ptr(), 1);
        let t = pik_tekis_new(dir.as_ptr(), 3);
        let mut models = std::collections::HashSet::new();
        let mut list = Vec::new();
        for k in first..first + count {
            assert_eq!(pik_tekis_has_strategy(t, k), 1, "kind {k}");
            let mut info = crate::view::PikTekiInfo::default();
            assert_eq!(crate::view::pik_view_teki_info(view, k, &mut info), 1, "kind {k} not in the view");
            assert!(models.insert(info.model), "kind {k} shares a model");
            assert!(info.model >= 19 && info.life > 0.0 && info.hi.y > info.lo.y);
            // The same facts without a view.
            let mut bare = crate::view::PikTekiInfo::default();
            assert_eq!(crate::view::pik_teki_info(dir.as_ptr(), k, &mut bare), 1);
            assert_eq!(bare.life, info.life);
            list.push(crate::view::PikTekiInstance {
                id: k, kind: k, motion: 0, frame: 3.0, scale: 1.0, alpha: 1.0,
                pos: PikVec3 { x: (k - first) as f32 * 60.0, y: 0.0, z: 0.0 }, ..Default::default()
            });
        }
        let frame = PikViewFrame {
            dt: 1.0 / 30.0,
            forward: PikVec3 { x: 0.0, y: 0.0, z: 1.0 },
            up: PikVec3 { x: 0.0, y: 1.0, z: 0.0 },
            listener: -1,
            ..Default::default()
        };
        crate::view::pik_view_set_tekis(view, list.as_ptr(), count);
        pik_view_update(view, sim, &frame);
        let n = pik_view_draws(view, std::ptr::null_mut(), 0);
        let mut draws = vec![PikDraw::default(); n as usize];
        pik_view_draws(view, draws.as_mut_ptr(), n);
        let mine: Vec<_> = draws.iter().filter(|d| d.kind == 12).collect();
        assert_eq!(mine.len() as u32, count);
        assert!(mine.iter().all(|d| models.contains(&d.model)));
        // A Flint Beetle comes up when a player walks close, then is a body the view draws at its reported size.
        let at = PikVec3::default();
        let id = pik_tekis_spawn(t, first, 4242, &at, 0.0) as u32;
        let navi = PikNavi { id: 1, pos: PikVec3 { x: 0.0, y: 0.0, z: 30.0 }, size: 10.0, alive: 1, buried: 0, _pad: [0; 2] };
        for _ in 0..30 {
            pik_tekis_update(t, sim, &navi, 1, 1);
        }
        let mut st = PikTekiState::default();
        assert_eq!(pik_tekis_state(t, id, &mut st), 1);
        assert_eq!(st.flags & 3, 3, "visible and alive once someone is near");
        assert!((st.fade - 1.0).abs() < 1e-3, "full size: {}", st.fade);
        pik_tekis_free(t);
        pik_sim_free(sim);
        pik_view_free(view);
    }
}

#[test]
fn treasure_cache_completeness_crosses_the_abi() {
    let Some(assets) = libpikmin_assets::Assets::discover(None) else { return };
    let p2 = assets.root.join("p2");
    if !p2.join("treasure/config/otakara_config.txt").is_file() {
        eprintln!("SKIP: no Pikmin 2 treasure cache");
        return;
    }
    let dir = CString::new(p2.to_string_lossy().as_bytes()).unwrap();
    let missing = CString::new(p2.join("nowhere").to_string_lossy().as_bytes()).unwrap();
    unsafe {
        assert_eq!(pik_p2_treasures_complete(dir.as_ptr()), 1);
        assert_eq!(pik_p2_treasures_complete(missing.as_ptr()), 0);
        assert_eq!(pik_p2_treasures_complete(std::ptr::null()), 0);
        // Already complete: extracting is a no-op that succeeds without a disc.
        let none = CString::new("no-such-disc.iso").unwrap();
        assert_eq!(pik_p2_extract_treasures(none.as_ptr(), dir.as_ptr()), 0);
        assert_eq!(pik_p2_extract_treasures(none.as_ptr(), missing.as_ptr()), 1);
    }
}

#[test]
fn treasure_models_join_the_view_table_through_the_abi() {
    let Some(assets) = libpikmin_assets::Assets::discover(None) else { return };
    let p2 = assets.root.join("p2");
    if !p2.join("treasure/config/otakara_config.txt").is_file() {
        eprintln!("SKIP: no Pikmin 2 treasure cache");
        return;
    }
    let dir = CString::new(assets.root.to_string_lossy().as_bytes()).unwrap();
    let cache = CString::new(p2.to_string_lossy().as_bytes()).unwrap();
    let missing = CString::new(p2.join("nowhere").to_string_lossy().as_bytes()).unwrap();
    unsafe {
        let view = pik_view_new(dir.as_ptr(), 0);
        assert!(!view.is_null());
        let before = pik_view_model_count(view);
        assert_eq!(pik_view_load_treasures(view, missing.as_ptr()), -1);
        assert_eq!(pik_view_model_count(view), before, "a failed load adds nothing");
        let first = pik_view_load_treasures(view, cache.as_ptr());
        assert_eq!(first as u32, before);
        assert_eq!(pik_view_model_count(view), before + 188);
        assert_eq!(pik_view_load_treasures(view, cache.as_ptr()), first, "again is a no-op");
        assert_eq!(pik_view_load_treasures(view, std::ptr::null()), -1);
        pik_view_free(view);
    }
}
