//! Checks against assets extracted from the user's disc (`libpikmin_assets::extract`).
//! Skipped when `assets/` is missing.

use glam::{Mat4, Vec3};
use libpikmin_assets::motion::Motion;
use libpikmin_assets::{AnimSet, Assets, Model, Skeleton, paths};

fn disc() -> Option<Assets> {
    Assets::discover(None)
}

fn bounds(
    model: &Model,
    skel: &Skeleton,
    anim: Option<(&libpikmin_assets::Anim, f32)>,
) -> (Vec3, Vec3) {
    let pose = skel.pose(Mat4::IDENTITY, anim);
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for b in model.batches().unwrap() {
        for v in &b.vertices {
            let (p, n) = pose.skin(v);
            assert!(p.is_finite() && n.is_finite());
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    (lo, hi)
}

#[test]
fn every_model_and_animation_loads() {
    let Some(disc) = disc() else { return };
    let mut all = vec![paths::OLIMAR, paths::ONION];
    all.extend(paths::PIKMIN);
    all.extend(paths::HAPPA);
    all.extend(paths::PELLETS);
    for p in all {
        let m = Model::parse(&disc.read(p).unwrap()).unwrap();
        assert!(!m.batches().unwrap().is_empty(), "{p}");
        for t in &m.textures {
            t.decode().unwrap();
        }
    }
    let set = AnimSet::load(&disc, paths::PIKI_ANIM_MGR, paths::PIKI_BUNDLE).unwrap();
    // Every motion the table names resolves to animation data.
    for i in 0..90u8 {
        if let Some(idx) = libpikmin_assets::motion::MOTION_TABLE[i as usize] {
            assert!(set.get(idx as usize).is_some(), "motion {i} -> info {idx}");
        }
    }
    let goal = AnimSet::load(&disc, paths::ONION_ANIM_MGR, paths::ONION_BUNDLE).unwrap();
    assert!(goal.anims.iter().all(Option::is_some));
}

#[test]
fn pikmin_stands_upright_at_sim_scale() {
    let Some(disc) = disc() else { return };
    let set = AnimSet::load(&disc, paths::PIKI_ANIM_MGR, paths::PIKI_BUNDLE).unwrap();
    let wait = set.get(Motion::Wait.anim_index().unwrap()).unwrap().1;
    for p in [paths::PIKMIN[0], paths::OLIMAR] {
        let m = Model::parse(&disc.read(p).unwrap()).unwrap();
        let skel = Skeleton::new(&m);
        let (lo, hi) = bounds(&m, &skel, Some((wait, 0.0)));
        let height = hi.y - lo.y;
        println!("{p}: {lo} .. {hi}");
        assert!(
            lo.y > -3.0 && (10.0..40.0).contains(&height),
            "{p}: {lo} .. {hi}"
        );
    }
}

/// The Rust extractor reproduces the existing extraction byte for byte. Needs the disc.
#[test]
fn extractor_matches_existing_assets() {
    let (Some(assets), Some(iso)) = (
        disc(),
        libpikmin_assets::extract::find_disc(&[
            Assets::default_dir().join(".."),
            Assets::default_dir().join("../../iso backups"),
            Assets::default_dir().join("../../pikmin-main/orig/GPIE01_01"),
        ]),
    ) else {
        return;
    };
    let out = std::env::temp_dir().join("libpikmin_extract_test");
    let _ = std::fs::remove_dir_all(&out);
    let report = libpikmin_assets::extract::extract_assets(&iso, &out, &mut |_| {}).unwrap();
    assert!(report.files > 20);
    for rel in libpikmin_assets::extract::REQUIRED {
        assert_eq!(
            std::fs::read(out.join(rel)).unwrap(),
            std::fs::read(assets.root.join(rel)).unwrap(),
            "{rel}"
        );
    }
    // Fresh first-run extraction includes the two jump motions inside the shared bundle.
    let fresh = Assets { root: out.clone() };
    let motions = AnimSet::load(&fresh, paths::PIKI_ANIM_MGR, paths::PIKI_BUNDLE).unwrap();
    for (motion, name) in [(Motion::OCarry, "ocarry.dca"), (Motion::RollJmp, "rolljmp.dca")] {
        let (info, anim) = motions.get(motion.anim_index().unwrap()).unwrap();
        assert_eq!(info.name.to_ascii_lowercase(), name);
        assert!(anim.frames > 10);
    }
    let _ = std::fs::remove_dir_all(&out);
}

/// Every wave in every WSYS decodes to its stated length, every instrument's waves resolve in
/// scene 0 of its wave system, and the sequences the game preloads exist.
#[test]
fn sound_bank_decodes() {
    use libpikmin_assets::sound::{SoundData, Stream};
    let Some(disc) = disc() else { return };
    let snd = SoundData::load(&disc).unwrap();
    for (wi, ws) in snd.bank.wsys.iter().enumerate() {
        let Some(ws) = ws else { continue };
        for (ai, arc) in ws.archives.iter().enumerate() {
            if arc.file.is_empty() {
                continue;
            }
            for (i, w) in arc.waves.iter().enumerate() {
                let pcm = snd.pcm(wi, ai, i).unwrap();
                assert_eq!(pcm.len(), w.end as usize, "{} wave {i}", arc.file);
            }
        }
    }
    // The sound effect bank (bank 0) against the SE wave system (0): `WaveScene_Set(0, 0)`
    // in `Jac_PlayInit` loads scene 0, whose dependencies pull in the rest.
    let se = snd.bank.banks[0].as_ref().unwrap();
    let ws = snd.bank.wsys[0].as_ref().unwrap();
    let mut found = 0;
    for p in 0..0x80u8 {
        let Some(inst) = se.inst(p) else { continue };
        for k in &inst.keys {
            for v in &k.vels {
                assert!(
                    ws.find_wave(0, v.wave_id as u16).is_some(),
                    "inst {p} wave {:#x}",
                    v.wave_id
                );
                found += 1;
            }
        }
    }
    assert!(found > 20);
    for id in [0, 1, 11, 13, 16] {
        assert!(snd.seqs.seq(id).is_some(), "sequence {id}");
    }
    let s = Stream::decode(&disc.read("SndData/piki.stx").unwrap()).unwrap();
    assert_eq!(s.stereo.len(), s.header.samples as usize * 2);
}

/// Enemy animations carry their sound cues as event keys (`ANIMEVENT_Notify`).
#[test]
fn enemy_animations_have_sound_events() {
    let Some(disc) = disc() else { return };
    for name in ["chappy", "swallow"] {
        let mgr = libpikmin_assets::AnimMgr::parse(&disc.read(&format!("tekikeys/{name}.key")).unwrap()).unwrap();
        let sounds: Vec<_> = mgr
            .infos
            .iter()
            .enumerate()
            .flat_map(|(i, info)| info.event_keys.iter().filter(|e| e.event == 0).map(move |e| (i, e.frame, e.value)))
            .collect();
        println!("{name}: {sounds:?}");
        assert!(!sounds.is_empty(), "{name} has no sound events");
    }
}

#[test]
fn every_particle_generator_parses_and_runs() {
    use libpikmin_assets::particle::{Generator, Pcr, effects};
    let Some(disc) = disc() else { return };
    let dir = disc.root.join("effects/pcr");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let path = e.unwrap().path();
        if path.extension().is_none_or(|x| x != "pcr") {
            continue;
        }
        let pcr = Pcr::parse(&std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut g = Generator::new(std::sync::Arc::new(pcr), Vec3::ZERO, 7);
        for _ in 0..90 {
            g.update(1.0, &mut |_| {});
            for q in g.quads(Vec3::X, Vec3::Y) {
                assert!(q.corners.iter().all(|c| c.is_finite()), "{}", path.display());
            }
        }
        n += 1;
    }
    assert!(n > 100, "{n} generators");
    let curs = Pcr::parse(&disc.read(effects::WHISTLE_CIRCLE.pcr).unwrap()).unwrap();
    // g_curs00: rising sparkles that live 8 frames, colours held over the life.
    assert!(curs.billboard);
    assert_eq!(curs.life, 8);
    for e in effects::NAVI_WHISTLE.iter().chain([&effects::WHISTLE_CIRCLE]) {
        libpikmin_assets::texture::decode_bti(&disc.read(e.tex).unwrap()).unwrap();
    }
    let olimar = disc.model(paths::OLIMAR).unwrap();
    let ante = olimar.coll.iter().find(|c| c.id == "ante").expect("antenna sphere");
    assert_eq!(ante.joint, 6);
}

#[test]
fn ship_parts_screens_and_font_load() {
    use libpikmin_assets::screen::{Font, Screen};
    use libpikmin_assets::ship;
    let Some(disc) = disc() else { return };
    let infos = ship::parse_pel_anim(&disc.read(ship::PEL_ANIM).unwrap()).unwrap();
    for id in ship::PART_IDS {
        let info = infos.iter().find(|i| i.id == id).unwrap_or_else(|| panic!("{id}"));
        let m = Model::parse(&disc.read(&info.model()).unwrap()).unwrap();
        assert!(!m.batches().unwrap().is_empty(), "{id}");
    }
    assert_eq!(infos.iter().find(|i| i.id == "ust1").unwrap().file, "soto1");
    let ufo = Model::parse(&disc.read(ship::SHIP_MODEL).unwrap()).unwrap();
    assert!(ufo.joint_index(&ship::ship_joint("soto1")).is_some());

    // Every English screen parses; the Bowsprit's discovery text names it.
    for e in std::fs::read_dir(disc.root.join("screen/eng_blo")).unwrap() {
        let p = e.unwrap().path();
        Screen::parse(&std::fs::read(&p).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    }
    let found = Screen::parse(&disc.read(&ship::found_screen(0)).unwrap()).unwrap();
    assert!(found.plain_text().concat().contains("Bowsprit"), "{:?}", found.plain_text());
    for i in 0..30 {
        let s = Screen::parse(&disc.read(&ship::info_screen(i)).unwrap()).unwrap();
        assert!(!s.plain_text().concat().is_empty(), "part {i}");
    }

    let (img, ..) = libpikmin_assets::texture::decode_bti(&disc.read(Font::PATH).unwrap()).unwrap();
    let font = Font::new(img, 21, 42);
    let a = font.glyph(b'A').unwrap();
    let i = font.glyph(b'i').unwrap();
    assert!(a.advance > i.advance && i.advance > 0, "A {a:?} i {i:?}");
}

#[test]
fn hud_number_textures_decode_from_the_users_disc() {
    let Some(disc) = Assets::discover(None) else { return };
    for digit in 0..10 {
        for shadow in [false, true] {
            let image = libpikmin_assets::screen::number_texture(&disc, digit, shadow).unwrap();
            assert_eq!((image.width, image.height), (64, 64));
            assert_eq!(image.rgba.len(), 64 * 64 * 4);
            assert!(image.rgba.chunks_exact(4).any(|p| p[3] > 0));
        }
    }
    assert!(libpikmin_assets::screen::number_texture(&disc, 10, false).is_err());
}

/// The ship's motions through a delivery: it opens up and stops at `kiso_t`'s Action0 key,
/// carries on when the part is in, then the part fits itself (Appear) and settles into its
/// After loop while the first upgrade plays and the lights start blinking.
#[test]
fn ship_animator_runs_a_delivery_and_a_level_up() {
    use libpikmin_assets::AnimMgr;
    use libpikmin_assets::ship::{self, ShipAnimator, ShipCue, motion, part_motion};
    let Some(disc) = disc() else { return };
    let mgr = AnimMgr::parse(&disc.read(ship::SHIP_ANIM_MGR).unwrap()).unwrap();
    let infos = ship::parse_pel_anim(&disc.read(ship::PEL_ANIM).unwrap()).unwrap();
    let parts = ship::PART_IDS
        .iter()
        .map(|id| {
            let i = infos.iter().find(|i| i.id == *id)?;
            Some((AnimMgr::parse(&disc.read(&i.anim_mgr()).ok()?).ok()?, i.override_joint != -1))
        })
        .collect();
    let mut s = ShipAnimator::new(&mgr, parts);
    let mut cues = Vec::new();
    s.start(&[], 0, &mut cues);
    assert_eq!(cues, vec![ShipCue::Sound("SE_UFO_DESTROY")]);
    let run = |s: &mut ShipAnimator, secs: f32, cues: &mut Vec<ShipCue>| {
        for _ in 0..(secs * 30.0) as usize {
            s.update(1.0 / 30.0, cues);
        }
    };
    // Standing still until a part comes.
    run(&mut s, 1.0, &mut cues);
    assert_eq!(s.context(0), (motion::WAIT_TUTORIAL, 0.0));

    s.suck_start();
    run(&mut s, 2.0, &mut cues);
    let (m, f) = s.context(0);
    assert_eq!(m, motion::WAIT_TUTORIAL);
    assert!((14.0..15.0).contains(&f), "stops at Action0: {f}");

    cues.clear();
    let engine = ship::MAIN_ENGINE;
    s.suck_finish(engine, 1, &mut cues);
    assert!(cues.iter().any(|c| matches!(c, ShipCue::Effect { lift, .. } if *lift == 60.0)));
    assert_eq!(s.part(engine).unwrap().lower.motion, part_motion::APPEAR);
    run(&mut s, 4.0, &mut cues);
    // Done shuddering, it stands up for good.
    assert_eq!(s.context(0), (motion::WAIT, 0.0));
    assert!(cues.iter().any(|c| matches!(c, ShipCue::PartEffect { part, .. } if *part == engine)));
    // henka1 has played to its end; henka1b loops.
    assert_eq!(s.context(1), (motion::HENKA1, 119.0));
    run(&mut s, 2.0, &mut cues);
    assert_eq!(s.part(engine).unwrap().lower.motion, part_motion::AFTER);
    assert!(s.light_frame(0) < 19.0);
    assert!(cues.contains(&ShipCue::Sound("SE_UFO_LIGHT")));
    // soto5f loops every 100 frames, humming as it goes round.
    run(&mut s, 3.5, &mut cues);
    assert!(cues.contains(&ShipCue::Sound("SE_UFO_ENGINE")));
}

#[test]
fn ship_takes_off_with_its_level_s_motion_and_flames() {
    use libpikmin_assets::AnimMgr;
    use libpikmin_assets::ship::{self, ShipAnimator, ShipCue, motion};
    let Some(disc) = disc() else { return };
    let mgr = AnimMgr::parse(&disc.read(ship::SHIP_ANIM_MGR).unwrap()).unwrap();
    let mut s = ShipAnimator::new(&mgr, vec![None; 30]);
    let mut cues = Vec::new();
    // Ionium Jet #1 (uf10, part 14) on board: the left thruster fires.
    let on_board = [14u32];
    s.start(&on_board, 3, &mut cues);
    cues.clear();
    s.take_off(&on_board, &mut cues);
    assert!(cues.contains(&ShipCue::StopSound("SE_UFO_IDLING")));
    assert_eq!(s.context(0).0, motion::TAKEOFF[2]);
    for _ in 0..60 {
        s.update(1.0 / 30.0, &mut cues);
    }
    assert!(cues.contains(&ShipCue::DemoSound { demo: 32, cmd: 0 }));
    let flames = cues.iter().filter(|c| matches!(c, ShipCue::Attached { .. })).count();
    assert_eq!(flames, 8, "two flames on each engine");
    assert!(cues.iter().any(|c| matches!(c, ShipCue::Effect { .. })), "ground blast");
    for _ in 0..360 {
        s.update(1.0 / 30.0, &mut cues);
    }
    assert!(cues.iter().any(|c| matches!(c, ShipCue::JointEffect { joint: 49, .. })), "left thruster");
    assert!(!cues.iter().any(|c| matches!(c, ShipCue::JointEffect { joint: 48, .. })), "no right one");
    assert!(cues.contains(&ShipCue::DemoSound { demo: 32, cmd: 1 }));

    // Landing: the take off backwards, flames on until it's down.
    cues.clear();
    s.land(&on_board, 3, &mut cues);
    assert!(!s.taking_off() && s.landing());
    assert_eq!(s.context(0), (motion::TAKEOFF[2], 398.0));
    assert_eq!(cues.iter().filter(|c| matches!(c, ShipCue::Attached { .. })).count(), 8);
    for _ in 0..30 {
        s.update(1.0 / 30.0, &mut cues);
    }
    assert!((367.0..369.0).contains(&s.context(0).1), "going backwards: {:?}", s.context(0));
    assert!(!cues.contains(&ShipCue::StopAttached));
    for _ in 0..360 {
        s.update(1.0 / 30.0, &mut cues);
    }
    assert!(cues.contains(&ShipCue::StopAttached), "flames out at touchdown");
    assert!(cues.iter().any(|c| matches!(c, ShipCue::Effect { .. })), "landing dust");
    assert!(!cues.contains(&ShipCue::Sound("SE_UFO_IDLING")), "not humming before it's down");
    for _ in 0..30 {
        s.update(1.0 / 30.0, &mut cues);
    }
    assert!(!s.landing());
    assert_eq!(s.context(0), (motion::WAIT, 0.0));
    assert!(cues.contains(&ShipCue::EndDemoSound));
    assert!(cues.contains(&ShipCue::Sound("SE_UFO_IDLING")));
}
#[test]
fn onion_startup_sound_commands_are_extracted_for_both_timelines() {
    let Some(assets) = libpikmin_assets::Assets::discover(None) else { return };
    for red in [true, false] {
        let (commands, end) = libpikmin_assets::sound::tables::onion_boot_commands(&assets, red).unwrap();
        assert_eq!(end, 280.0);
        assert_eq!(commands.first(), Some(&(0.0, 0)));
        assert_eq!(commands.last().unwrap().1, 7);
        assert!(commands.windows(2).all(|w| w[0].0 <= w[1].0));
    }
}
