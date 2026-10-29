//! Pikmin 2's Segmented Crawbster: the disc's numbers and corpse load, it waits out of sight until someone is near, crashes down, walks after him and rolls
//! into a ball at him; a wall at speed throws it on its back, open to blows, then it rights itself and swings its arms.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, Host};
use libpikmin::{Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::dango::{APPEAR, ATTACK, FLICK, MOVE, RECOVER, STAY, TURN, WAIT};
use libpikmin_teki::info::slot;
use libpikmin_teki::p2::{self, SEGMENTED_CRAWBSTER};
use libpikmin_teki::{Navi, Out, TekiId, Tekis, opt};

/// Flat ground with a wall along z = `limit`.
struct Walled(f32);

impl Host for Walled {
    fn ground_height(&self, _: Vec3) -> Option<f32> {
        Some(0.0)
    }
    fn walls(&self, mut centre: Vec3, _: f32) -> Vec3 {
        centre.z = centre.z.min(self.0);
        centre
    }
}

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
    p2::is_available(&a.root.join("p2"), SEGMENTED_CRAWBSTER).then_some(a)
}

struct Rig {
    sim: Sim,
    tekis: Tekis,
    id: TekiId,
    player: Navi,
    outs: Vec<Out>,
    wall: f32,
    seen: std::collections::BTreeSet<i32>,
}

impl Rig {
    fn new(a: &Assets, player_at: Vec3) -> Rig {
        let sim = Sim::new(Params::default(), 7);
        let mut tekis = Tekis::load(a);
        tekis.seed(17);
        let id = tekis.spawn(SEGMENTED_CRAWBSTER, 3, Vec3::ZERO, 0.0).expect("spawn");
        Rig { sim, tekis, id, player: Navi { id: 5, pos: player_at, size: 8.0, alive: true }, outs: Vec::new(), wall: 1.0e6, seen: Default::default() }
    }
    fn tick(&mut self) {
        let mut frame = CreatureFrame::default();
        self.tekis.set_ground(self.id, 0.0, false);
        self.tekis.frame(&mut frame);
        self.sim.step(&mut Frame(&frame));
        let out = self.tekis.update(&mut self.sim, &Walled(self.wall), &[self.player], 1.0 / 30.0);
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
    let info = tekis.info(SEGMENTED_CRAWBSTER).expect("loaded");
    assert_eq!((info.life, info.run_speed, info.visible_range), (3000.0, 50.0, 500.0));
    assert_eq!(info.clips.len(), 9);
    assert_eq!((info.corpse_min_carriers, info.corpse_max_carriers), (20, 30));
    assert_eq!((info.floats[101], info.floats[102], info.floats[103]), (200.0, 0.03, 3.0));
}

#[test]
fn it_waits_hidden_then_crashes_down_and_goes_for_whoever_is_near() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 3000.0));
    for _ in 0..60 {
        rig.tick();
    }
    let t = rig.tekis.get(rig.id).unwrap();
    assert_eq!(t.state, STAY);
    assert!(!t.option(opt::VISIBLE) && !t.option(opt::ATARI));
    rig.player.pos = Vec3::new(0.0, 0.0, 120.0);
    assert!(rig.run_until(4.0, |r| r.state() == Some(APPEAR)), "its shadow grows, then it comes down: {:?}", rig.seen);
    assert!(rig.tekis.get(rig.id).unwrap().option(opt::VISIBLE));
    assert!(rig.run_until(8.0, |r| r.seen.contains(&WAIT) || r.seen.contains(&MOVE) || r.seen.contains(&ATTACK)), "{:?}", rig.seen);
}

#[test]
fn it_rolls_at_a_player_in_reach_and_crashes_into_a_wall() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 120.0));
    assert!(rig.run_until(20.0, |r| r.state() == Some(ATTACK)), "{:?}", rig.seen);
    assert!(rig.run_until(6.0, |r| r.tekis.get(r.id).is_some_and(|t| t.flags[0])), "it starts to roll");
    assert!(rig.run_until(6.0, |r| r.tekis.get(r.id).is_some_and(|t| t.target_vel.length() > 100.0)), "at its rolling speed");
    // The wall it meets throws it on its back.
    rig.wall = rig.tekis.get(rig.id).unwrap().pos.z + 40.0;
    assert!(rig.run_until(2.0, |r| r.state() == Some(TURN)), "{:?}", rig.seen);
    rig.tick();
    assert!(rig.run_until(6.0, |r| !r.tekis.get(r.id).unwrap().option(opt::INVINCIBLE)), "open to blows once flipped");
    assert!(rig.run_until(20.0, |r| r.seen.contains(&RECOVER)), "{:?}", rig.seen);
    assert!(rig.tekis.get(rig.id).unwrap().option(opt::INVINCIBLE), "shielded again after");
    assert!(rig.run_until(6.0, |r| r.seen.contains(&FLICK)), "{:?}", rig.seen);
}

#[test]
fn it_dies_with_the_dying_notice() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 3000.0));
    rig.run_until(2.0, |_| false);
    rig.tekis.damage(rig.id, 1.0e6, None);
    assert!(rig.run_until(15.0, |r| r.tekis.get(r.id).is_none()), "{:?} {:?}", rig.seen, rig.tekis.get(rig.id).map(|t| (t.state, t.hp, t.anim.motion, t.anim.counter, t.manual_speed)));
    let dying = rig.outs.iter().position(|o| matches!(o, Out::Dying { .. }));
    let died = rig.outs.iter().position(|o| matches!(o, Out::Died { .. }));
    assert!(dying.is_some() && died.is_some() && dying < died);
}
