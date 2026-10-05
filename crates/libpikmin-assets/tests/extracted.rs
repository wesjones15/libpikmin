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
