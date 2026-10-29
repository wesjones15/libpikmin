//! Pikmin 2's Empress Bulblax: the disc's numbers and corpse load, she sleeps laying larvae, wakes and shakes off a mob, rolls
//! across her territory crushing a player and Pikmin, and dies with the dying notice.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::info::slot;
use libpikmin_teki::p2::{self, BULBORB_LARVA, EMPRESS_BULBLAX};
use libpikmin_teki::queen::{BORN, DAMAGE, FLICK, ROLLING, SLEEP, WAIT};
use libpikmin_teki::{Navi, Out, TekiId, Tekis};

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
    p2::is_available(&a.root.join("p2"), EMPRESS_BULBLAX).then_some(a)
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
        tekis.seed(8);
        let id = tekis.spawn(EMPRESS_BULBLAX, 3, Vec3::ZERO, 0.0).expect("spawn");
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
fn she_loads_the_discs_numbers_and_corpse() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let tekis = Tekis::load(&a);
    let info = tekis.info(EMPRESS_BULBLAX).expect("loaded");
    assert_eq!(info.life, 5000.0);
    assert_eq!(&info.clips[..3], &["dead", "sleep", "wait1"]);
    assert_eq!((info.corpse_size, info.corpse_min_carriers, info.corpse_max_carriers), (45.0, 20, 30));
    assert!(info.motions[slot::DEAD].end > 1.0);
    assert!(tekis.has_strategy(EMPRESS_BULBLAX));
}

#[test]
fn she_lays_larvae_while_she_sleeps() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 2000.0));
    assert_eq!(rig.state(), Some(SLEEP));
    assert!(rig.run_until(20.0, |r| r.outs.iter().any(|o| matches!(o, Out::Spawn { kind: BULBORB_LARVA, .. }))), "a larva is laid");
    assert!(rig.seen.contains(&BORN));
}

#[test]
fn hurt_she_wakes_shakes_off_a_mob_and_rolls_across_her_territory_crushing() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 400.0));
    rig.run_until(2.0, |_| false);
    rig.tekis.damage(rig.id, 5.0, None);
    assert!(rig.run_until(10.0, |r| r.state() != Some(SLEEP)), "a hit wakes her");
    // Enough blows to make her flick.
    for _ in 0..40 {
        rig.tekis.damage(rig.id, 5.0, None);
    }
    assert!(rig.run_until(15.0, |r| r.seen.contains(&FLICK)), "{:?}", rig.seen);
    assert!(rig.run_until(15.0, |r| r.state() == Some(ROLLING)), "{:?}", rig.seen);
    rig.player.pos = Vec3::new(100.0, 0.0, 0.0);
    // Once she is rolling in earnest, a Pikmin in reach is crushed at once.
    assert!(rig.run_until(5.0, |r| r.tekis.get(r.id).is_some_and(|t| t.flags[0])));
    let at = rig.tekis.get(rig.id).unwrap().pos;
    let piki = rig.sim.spawn_piki(PikiColor::Red, at + Vec3::new(0.0, 0.0, 60.0)).unwrap();
    rig.tick();
    rig.run_until(12.0, |_| false);
    assert!(rig.outs.iter().any(|o| matches!(o, Out::HitNavi { damage, .. } if *damage > 20.0)), "the roll crushes a player under her");
    assert!(!rig.sim.piki(piki).is_alive(), "and a Pikmin in her way");
    let _ = (WAIT, DAMAGE);
}

#[test]
fn she_dies_with_the_dying_notice() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 2000.0));
    rig.run_until(2.0, |_| false);
    rig.tekis.damage(rig.id, 1.0e6, None);
    assert!(rig.run_until(12.0, |r| r.tekis.get(r.id).is_none()));
    let dying = rig.outs.iter().position(|o| matches!(o, Out::Dying { .. }));
    let died = rig.outs.iter().position(|o| matches!(o, Out::Died { .. }));
    assert!(dying.is_some() && died.is_some() && dying < died);
}
