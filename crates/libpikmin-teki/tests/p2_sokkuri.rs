//! Pikmin 2's Skitter Leaf: the disc's numbers and corpse load, it hides until a player comes close, skitters about and
//! dies leaving the dying notice the host makes its corpse from.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::info::slot;
use libpikmin_teki::p2::{self, SKITTER_LEAF};
use libpikmin_teki::sokkuri::{DISAPPEAR, MOVE_GROUND, STAY, WAIT};
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
    p2::is_available(&a.root.join("p2"), SKITTER_LEAF).then_some(a)
}

struct Rig {
    sim: Sim,
    tekis: Tekis,
    id: TekiId,
    player: Navi,
    outs: Vec<Out>,
}

impl Rig {
    fn new(a: &Assets, player_at: Vec3) -> Rig {
        let sim = Sim::new(Params::default(), 7);
        let mut tekis = Tekis::load(a);
        tekis.seed(9);
        let id = tekis.spawn(SKITTER_LEAF, 3, Vec3::ZERO, 0.0).expect("spawn");
        Rig { sim, tekis, id, player: Navi { id: 5, pos: player_at, size: 8.0, alive: true }, outs: Vec::new() }
    }
    fn tick(&mut self) {
        let mut frame = CreatureFrame::default();
        self.tekis.set_ground(self.id, 0.0, false);
        self.tekis.frame(&mut frame);
        self.sim.step(&mut Frame(&frame));
        let out = self.tekis.update(&mut self.sim, &FlatHost, &[self.player], 1.0 / 30.0);
        self.outs.extend(out);
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
fn it_loads_its_numbers_and_the_corpse_the_disc_says() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let tekis = Tekis::load(&a);
    let info = tekis.info(SKITTER_LEAF).expect("loaded");
    assert_eq!((info.life, info.run_speed), (120.0, 120.0));
    assert_eq!(info.clips[0], "run1");
    assert_eq!(info.clips[4], "dead1");
    // The corpse of `carcass_config.txt`: radius 5, and the dying clip is the one it lies in.
    assert_eq!(info.corpse_size, 5.0);
    assert!(info.corpse_min_carriers >= 1 && info.corpse_max_carriers >= info.corpse_min_carriers);
    assert_eq!(info.motions[slot::DEAD].index, 4);
    assert!(info.motions[slot::DEAD].end > 1.0);
    assert!(tekis.has_strategy(SKITTER_LEAF));
}

#[test]
fn it_hides_until_a_player_comes_close_then_skitters_and_goes_back() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 600.0));
    for _ in 0..60 {
        rig.tick();
    }
    let t = rig.tekis.get(rig.id).unwrap();
    assert_eq!(t.state, STAY);
    assert!(!t.option(opt::ATARI), "nothing can hit it under its leaf");
    rig.player.pos = Vec3::new(0.0, 0.0, 100.0);
    assert!(rig.run_until(2.0, |r| r.state() == Some(MOVE_GROUND)), "it pops up and runs");
    let mut states = std::collections::BTreeSet::new();
    let mut top = 0.0f32;
    let mut last = rig.tekis.get(rig.id).unwrap().pos;
    for _ in 0..(15 * 30) {
        rig.tick();
        let t = rig.tekis.get(rig.id).unwrap();
        states.insert(t.state);
        top = top.max((t.pos - last).with_y(0.0).length() * 30.0);
        last = t.pos;
    }
    assert!(states.contains(&MOVE_GROUND) && states.contains(&WAIT), "{states:?}");
    assert!(top > 100.0 && top <= 121.0, "ground speed fp06 = 120, got {top}");
    // Everyone leaves and it is back at home: it goes under its leaf again.
    rig.player.pos = Vec3::new(0.0, 0.0, 5000.0);
    let home = rig.tekis.get(rig.id).unwrap().nest;
    assert!(rig.run_until(60.0, |r| r.state() == Some(STAY) || r.state() == Some(DISAPPEAR)), "{:?} home {home:?}", rig.tekis.get(rig.id).map(|t| t.pos));
}

#[test]
fn it_dies_when_hurt_with_the_dying_notice_for_its_corpse() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 100.0));
    assert!(rig.run_until(2.0, |r| r.state() == Some(MOVE_GROUND)));
    rig.tekis.damage(rig.id, 1.0e6, None);
    assert!(rig.run_until(6.0, |r| r.tekis.get(r.id).is_none()));
    let dying = rig.outs.iter().position(|o| matches!(o, Out::Dying { .. }));
    let died = rig.outs.iter().position(|o| matches!(o, Out::Died { .. }));
    assert!(dying.is_some() && died.is_some() && dying < died);
}
