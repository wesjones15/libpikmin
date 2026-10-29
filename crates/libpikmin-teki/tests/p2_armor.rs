//! Pikmin 2's Cloaking Burrow-nit: the disc's numbers and corpse load, it lies buried until someone is in sight, bursts out, hunts
//! and bites a player, goes home and dives when it loses him, and dies with the dying notice.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::armor::{ATTACK2, DIVE, MOVE, STAY};
use libpikmin_teki::info::slot;
use libpikmin_teki::p2::{self, BURROW_NIT};
use libpikmin_teki::{Navi, Out, TekiId, Tekis, opt};

struct Frame<'a>(&'a CreatureFrame);

impl Host for Frame<'_> {
    fn ground_height(&self, _: Vec3) -> Option<f32> {
        Some(0.0)
    }
    fn creatures(&mut self, out: &mut CreatureFrame) {
        *out = self.0.clone();
    }
}

fn assets() -> Option<Assets> {
    let a = Assets { root: Assets::default_dir() };
    p2::is_available(&a.root.join("p2"), BURROW_NIT).then_some(a)
}

struct Rig {
    sim: Sim,
    tekis: Tekis,
    id: TekiId,
    player: Navi,
    outs: Vec<Out>,
    seen: std::collections::BTreeSet<i32>,
}

impl Rig {
    fn new(a: &Assets, player_at: Vec3) -> Rig {
        let sim = Sim::new(Params::default(), 7);
        let mut tekis = Tekis::load(a);
        tekis.seed(12);
        let id = tekis.spawn(BURROW_NIT, 3, Vec3::ZERO, 0.0).expect("spawn");
        Rig { sim, tekis, id, player: Navi { id: 5, pos: player_at, size: 8.0, alive: true }, outs: Vec::new(), seen: Default::default() }
    }
    fn tick(&mut self) {
        let mut frame = CreatureFrame::default();
        self.tekis.set_ground(self.id, 0.0, false);
        self.tekis.frame(&mut frame);
        self.sim.step(&mut Frame(&frame));
        let out = self.tekis.update(&mut self.sim, &FlatHost, &[self.player], 1.0 / 30.0);
        self.outs.extend(out);
        if let Some(t) = self.tekis.get(self.id) {
            self.seen.insert(t.state);
        }
    }
    fn state(&self) -> Option<i32> {
        self.tekis.get(self.id).map(|t| t.state)
    }
    fn run_until(&mut self, seconds: f32, mut f: impl FnMut(&Rig) -> bool) -> bool {
        for _ in 0..(seconds * 30.0) as u32 {
            if f(self) {
                return true;
            }
            self.tick();
        }
        f(self)
    }
}

#[test]
fn it_loads_its_numbers_and_corpse() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let tekis = Tekis::load(&a);
    let info = tekis.info(BURROW_NIT).expect("loaded");
    assert_eq!((info.life, info.run_speed, info.visible_range), (300.0, 50.0, 200.0));
    assert_eq!(&info.clips[..4], &["dead", "appear", "dive", "move"]);
    assert_eq!((info.corpse_size, info.corpse_min_carriers, info.corpse_max_carriers), (25.0, 8, 16));
    assert!(info.motions[slot::DEAD].end > 1.0);
}

#[test]
fn it_lies_buried_then_bursts_out_hunts_bites_and_goes_back_down() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 600.0));
    for _ in 0..60 {
        rig.tick();
    }
    let t = rig.tekis.get(rig.id).unwrap();
    assert_eq!(t.state, STAY);
    assert!(!t.option(opt::ATARI));
    rig.player.pos = Vec3::new(0.0, 0.0, 150.0);
    assert!(rig.run_until(4.0, |r| r.state() == Some(MOVE)), "it bursts out and hunts: {:?}", rig.seen);
    assert!(rig.run_until(10.0, |r| r.seen.contains(&ATTACK2)), "it bites");
    assert!(rig.run_until(6.0, |r| r.outs.iter().any(|o| matches!(o, Out::HitNavi { .. }))), "and the bite hurts the player in front of it");
    // He is gone: it walks home and dives.
    rig.player.pos = Vec3::new(0.0, 0.0, 4000.0);
    assert!(rig.run_until(40.0, |r| r.seen.contains(&DIVE)), "{:?}", rig.seen);
}

#[test]
fn it_dies_with_the_dying_notice() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 150.0));
    assert!(rig.run_until(6.0, |r| r.state() == Some(MOVE)));
    rig.tekis.damage(rig.id, 1.0e6, None);
    assert!(rig.run_until(10.0, |r| r.tekis.get(r.id).is_none()));
    let dying = rig.outs.iter().position(|o| matches!(o, Out::Dying { .. }));
    let died = rig.outs.iter().position(|o| matches!(o, Out::Died { .. }));
    assert!(dying.is_some() && died.is_some() && dying < died);
}
