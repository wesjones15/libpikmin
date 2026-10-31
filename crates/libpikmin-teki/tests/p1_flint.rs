use glam::Vec3;
use libpikmin::{Params, Sim, host::FlatHost};
use libpikmin_assets::Assets;
use libpikmin_teki::{Navi, Out, Tekis, flint, info::P1_FLINT, opt};
fn rig() -> Option<(Sim, Tekis, u32)> {
    let a = Assets {
        root: Assets::default_dir(),
    };
    if !a.root.join("bosses/kogane/parms.bin").exists() {
        eprintln!("SKIP: P1 Kogane assets absent");
        return None;
    }
    let mut ts = Tekis::load(&a);
    let id = ts
        .spawn(P1_FLINT, 123, Vec3::ZERO, 0.0)
        .expect("P1 Kogane loader/strategy");
    Some((Sim::new(Params::default(), 2), ts, id))
}
fn tick(s: &mut Sim, ts: &mut Tekis, near: bool) -> Vec<Out> {
    s.step(&mut FlatHost);
    ts.update(
        s,
        &FlatHost,
        &[Navi {
            id: 1,
            pos: if near {
                Vec3::ZERO
            } else {
                Vec3::splat(10000.0)
            },
            size: 8.0,
            alive: true,
        }],
        1.0 / 30.0,
    )
}
#[test]
fn immortal_hidden_appears_and_exhausts_the_exact_eight_drop_table_then_burrows() {
    let Some((mut s, mut ts, id)) = rig() else {
        return;
    };
    for _ in 0..90 {
        tick(&mut s, &mut ts, false);
    }
    let t = ts.get(id).unwrap();
    assert_eq!(t.state, flint::HIDDEN);
    assert_eq!(t.fade, 0.0);
    assert!(!t.option(opt::VISIBLE));
    tick(&mut s, &mut ts, true);
    ts.get_mut(id).unwrap().reg[0] = 300.0;
    for _ in 0..10 {
        tick(&mut s, &mut ts, true);
    }
    let hp = ts.get(id).unwrap().hp;
    ts.damage(id, f32::MAX, None);
    tick(&mut s, &mut ts, true);
    assert_eq!(ts.get(id).unwrap().hp, hp);
    assert_ne!(
        ts.get(id).unwrap().state,
        flint::FLIP,
        "damage cannot trigger loot"
    );
    let mut pellets = Vec::new();
    let mut nectar = std::collections::HashSet::new();
    let mut effects = std::collections::HashSet::new();
    let mut saw_burrow = false;
    let mut shrank = false;
    for n in 1..=8 {
        ts.press(id);
        for _ in 0..65 {
            for o in tick(&mut s, &mut ts, true) {
                match o {
                    Out::SpawnPellets { number, count, .. } => {
                        assert_eq!(count, 1);
                        pellets.push(number);
                    }
                    Out::Effect { id, .. } | Out::EffectDir { id, .. } => {
                        effects.insert(id);
                    }
                    _ => {}
                }
            }
            for d in s.nectar_drops() {
                nectar.insert(d.id);
            }
            if let Some(t) = ts.get(id) {
                assert_eq!(t.hp, hp);
                assert!(t.option(opt::INVINCIBLE));
                saw_burrow |= t.state == flint::BURROW;
                shrank |= t.state == flint::BURROW && t.fade < 1.0;
                if t.count[0] >= n && t.state != flint::FLIP {
                    break;
                }
            } else {
                break;
            }
        }
    }
    for _ in 0..30 {
        if ts.get(id).is_none() {
            break;
        }
        let t = ts.get(id).unwrap();
        saw_burrow |= t.state == flint::BURROW;
        shrank |= t.fade < 1.0;
        tick(&mut s, &mut ts, true);
    }
    assert!(ts.get(id).is_none());
    assert!(saw_burrow && shrank);
    assert_eq!(pellets, vec![0, 1, 0, 1]);
    assert_eq!(nectar.len(), 8);
    for e in [87, 88, 89, 120] {
        assert!(effects.contains(&e), "missing exact effect {e}");
    }
}
#[test]
fn unharvested_beetle_times_out_without_kill_rewards() {
    let Some((mut s, mut ts, id)) = rig() else {
        return;
    };
    tick(&mut s, &mut ts, true);
    ts.get_mut(id).unwrap().reg[0] = 0.1;
    for _ in 0..90 {
        let out = tick(&mut s, &mut ts, true);
        assert!(
            !out.iter()
                .any(|o| matches!(o, Out::SpawnPellets { .. } | Out::Dying { .. }))
        );
    }
    assert!(ts.get(id).is_none());
}
