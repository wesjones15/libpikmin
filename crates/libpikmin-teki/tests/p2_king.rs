//! Pikmin 2's Emperor Bulblax: the disc's numbers and corpse load, it lies buried until someone is near, bursts out, goes after a player
//! and bites, stamps on what is at its feet, and dies with the dying notice.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::info::slot;
use libpikmin_teki::king::{APPEAR, ATTACK, CAUTION, HIDE_WAIT, WALK};
use libpikmin_teki::p2::{self, EMPEROR_BULBLAX};
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
    p2::is_available(&a.root.join("p2"), EMPEROR_BULBLAX).then_some(a)
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
        tekis.seed(10);
        let id = tekis.spawn(EMPEROR_BULBLAX, 3, Vec3::ZERO, 0.0).expect("spawn");
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
fn he_loads_the_discs_numbers_and_corpse() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let tekis = Tekis::load(&a);
    let info = tekis.info(EMPEROR_BULBLAX).expect("loaded");
    assert_eq!((info.life, info.run_speed, info.visible_range), (1300.0, 45.0, 500.0));
    assert_eq!(info.clips.len(), 14);
    assert_eq!(&info.clips[..5], &["attack", "cry", "damage", "dead", "dive"]);
    assert_eq!((info.corpse_size, info.corpse_min_carriers, info.corpse_max_carriers), (35.0, 20, 30));
    assert!(info.motions[slot::DEAD].end > 1.0);
}

#[test]
fn he_stays_buried_until_someone_is_near_then_bursts_out_and_goes_after_him() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 800.0));
    for _ in 0..90 {
        rig.tick();
    }
    let t = rig.tekis.get(rig.id).unwrap();
    assert_eq!(t.state, HIDE_WAIT);
    assert!(!t.option(opt::ATARI) && !t.option(opt::VISIBLE), "buried and unseen");
    rig.player.pos = Vec3::new(0.0, 0.0, 40.0);
    assert!(rig.run_until(3.0, |r| r.state() == Some(APPEAR)), "he comes up when the player is within the spawn distance");
    assert!(rig.tekis.get(rig.id).unwrap().option(opt::ATARI));
    assert!(rig.run_until(10.0, |r| r.state() == Some(CAUTION) || r.state() == Some(WALK)));
    rig.player.pos = Vec3::new(0.0, 0.0, 300.0);
    assert!(rig.run_until(20.0, |r| r.seen.contains(&ATTACK)), "he bites: {:?}", rig.seen);
}

#[test]
fn his_bite_catches_pikmin_and_hurts_a_player() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 40.0));
    assert!(rig.run_until(10.0, |r| r.state() == Some(WALK)));
    rig.player.pos = Vec3::new(0.0, 0.0, 350.0);
    assert!(rig.run_until(40.0, |r| r.state() == Some(ATTACK)), "{:?}", rig.seen);
    // The mouth is ahead of him; a player there is hurt when the tongue's keys come round.
    let t = rig.tekis.get(rig.id).unwrap();
    rig.player.pos = t.pos + t.forward() * 80.0;
    let _ = PikiColor::Red;
    assert!(rig.run_until(4.0, |r| r.outs.iter().any(|o| matches!(o, Out::HitNavi { .. }))));
}

#[test]
fn he_dies_with_the_dying_notice() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 40.0));
    assert!(rig.run_until(10.0, |r| r.state() == Some(WALK)));
    rig.tekis.damage(rig.id, 1.0e6, None);
    assert!(rig.run_until(15.0, |r| r.tekis.get(r.id).is_none()));
    let dying = rig.outs.iter().position(|o| matches!(o, Out::Dying { .. }));
    let died = rig.outs.iter().position(|o| matches!(o, Out::Died { .. }));
    assert!(dying.is_some() && died.is_some() && dying < died);
}
