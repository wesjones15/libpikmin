//! Pikmin 2's Unmarked Spectralid: the disc's numbers and corpse load, it hovers at its flight height and takes off when someone is near, ignores
//! every blow, and falls and dies when a Pikmin lands on it, or when its body dies.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{CaptainInput, Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::shijimi::{DEAD, FALL, FLY, WAIT};
use libpikmin_teki::info::slot;
use libpikmin_teki::p2::{self, SPECTRALID};
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
    p2::is_available(&a.root.join("p2"), SPECTRALID).then_some(a)
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
        tekis.seed(15);
        let id = tekis.spawn(SPECTRALID, 3, Vec3::ZERO, 0.0).expect("spawn");
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
    let info = tekis.info(SPECTRALID).expect("loaded");
    assert_eq!((info.life, info.run_speed, info.visible_range), (200.0, 150.0, 700.0));
    assert_eq!(info.clips.len(), 3);
    assert_eq!((info.corpse_min_carriers, info.corpse_max_carriers), (1, 1));
    assert_eq!((info.floats[103], info.floats[104], info.ints[0]), (70.0, 0.02, 0));
}

#[test]
fn it_hovers_at_its_flight_height_and_takes_off_when_someone_is_near() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 3000.0));
    for _ in 0..60 {
        rig.tick();
    }
    let t = rig.tekis.get(rig.id).unwrap();
    assert_eq!(t.state, WAIT);
    assert!(t.pos.y > 40.0, "it hovers above the ground: {}", t.pos.y);
    assert!(t.flying && t.option(opt::INVINCIBLE));
    rig.player.pos = t.pos.with_y(0.0) + Vec3::new(0.0, 0.0, 60.0);
    assert!(rig.run_until(2.0, |r| r.state() == Some(FLY)), "{:?}", rig.seen);
    // Blows do nothing to it.
    rig.tekis.damage(rig.id, 100.0, None);
    rig.run_until(1.0, |_| false);
    assert_eq!(rig.tekis.get(rig.id).unwrap().hp, 200.0);
}

#[test]
fn a_pikmin_on_its_back_brings_it_down() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 3000.0));
    rig.run_until(2.0, |_| false);
    // A captain below it throws a Pikmin at it.
    let creature = rig.tekis.get(rig.id).unwrap().creature;
    let captain = rig.sim.add_captain(Vec3::new(0.0, 0.0, -60.0));
    for i in 0..3 {
        rig.sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(i as f32 * 10.0, 0.0, -90.0), captain).unwrap();
    }
    rig.run_until(2.0, |_| false);
    rig.sim.set_lock_target(captain, Some(creature));
    rig.sim.set_input(captain, CaptainInput { throw: true, ..Default::default() });
    rig.run_until(1.0, |_| false);
    rig.sim.set_input(captain, CaptainInput::default());
    assert!(rig.run_until(6.0, |r| r.seen.contains(&FALL)), "{:?}", rig.seen);
    rig.tick();
    assert!(!rig.tekis.get(rig.id).is_some_and(|t| t.flying));
    assert!(rig.run_until(10.0, |r| r.tekis.get(r.id).is_none()), "{:?}", rig.seen);
    assert!(rig.seen.contains(&DEAD));
}

#[test]
fn it_dies_with_the_dying_notice_when_its_body_does() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 3000.0));
    rig.run_until(2.0, |_| false);
    rig.tekis.damage(rig.id, 1.0e6, None);
    assert!(rig.run_until(10.0, |r| r.tekis.get(r.id).is_none()));
    let dying = rig.outs.iter().position(|o| matches!(o, Out::Dying { .. }));
    let died = rig.outs.iter().position(|o| matches!(o, Out::Died { .. }));
    assert!(dying.is_some() && died.is_some() && dying < died);
}
