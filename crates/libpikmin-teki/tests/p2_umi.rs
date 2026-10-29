//! Pikmin 2's Bloysters: the disc's numbers and corpse load; the Ranging one wanders, marks a player in its search range, turns to him, creeps after him, lashes out with its tongue
//! and eats what it catches, and loses a player who goes far; the Toady one has no sight and only strikes at what is within reach.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::p2::TOADY_BLOYSTER;
use libpikmin_teki::umi::{ATTACK, EAT, FIND, LOST, SEARCH, TURN, WALK};
use libpikmin_teki::p2::{self, RANGING_BLOYSTER};
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
    p2::is_available(&a.root.join("p2"), RANGING_BLOYSTER).then_some(a)
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
        Rig::of(a, RANGING_BLOYSTER, player_at)
    }
    fn of(a: &Assets, kind: u8, player_at: Vec3) -> Rig {
        let sim = Sim::new(Params::default(), 7);
        let mut tekis = Tekis::load(a);
        tekis.seed(20);
        let id = tekis.spawn(kind, 3, Vec3::ZERO, 0.0).expect("spawn");
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
fn the_ranging_one_loads_its_numbers_and_corpse() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let tekis = Tekis::load(&a);
    let info = tekis.info(RANGING_BLOYSTER).expect("loaded");
    assert_eq!((info.life, info.run_speed, info.visible_range), (1500.0, 15.0, 700.0));
    assert_eq!(info.clips.len(), 11);
    assert!(info.motions[libpikmin_teki::info::slot::DEAD].end > 1.0);
    assert_eq!((info.floats[104], info.floats[112]), (85.0, 800.0));
    let toady = tekis.info(TOADY_BLOYSTER).expect("loaded");
    assert_eq!(toady.life, 1500.0);
    assert_eq!(toady.corpse_size, 22.0);
}

#[test]
fn the_ranging_one_marks_a_player_turns_creeps_and_lashes_out() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 3000.0));
    rig.run_until(2.0, |_| false);
    // He comes within its search distance, behind it.
    rig.player.pos = Vec3::new(-250.0, 0.0, -250.0);
    assert!(rig.run_until(10.0, |r| r.seen.contains(&FIND)), "{:?}", rig.seen);
    assert!(rig.run_until(10.0, |r| r.seen.contains(&TURN) || r.seen.contains(&SEARCH)), "{:?}", rig.seen);
    assert!(rig.run_until(40.0, |r| r.seen.contains(&ATTACK)), "it creeps up to him and lashes out: {:?}", rig.seen);
    assert!(rig.run_until(6.0, |r| r.outs.iter().any(|o| matches!(o, Out::HitNavi { .. }))), "the tongue hurts him");
}

#[test]
fn its_tongue_catches_a_pikmin_and_it_eats_it() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 3000.0));
    rig.run_until(1.0, |_| false);
    let t = rig.tekis.get(rig.id).unwrap();
    let at = t.pos + t.forward() * 120.0;
    let piki = rig.sim.spawn_piki(PikiColor::Red, at).unwrap();
    assert!(rig.run_until(40.0, |r| r.seen.contains(&EAT)), "{:?} {:?}", rig.seen, rig.sim.piki(piki).state);
    assert!(rig.run_until(10.0, |r| !r.sim.piki(piki).is_alive()), "it is eaten");
}

#[test]
fn it_loses_a_player_who_goes_far_and_goes_home() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 3000.0));
    rig.run_until(1.0, |_| false);
    rig.player.pos = Vec3::new(-250.0, 0.0, -250.0);
    assert!(rig.run_until(10.0, |r| r.seen.contains(&SEARCH)), "{:?}", rig.seen);
    rig.player.pos = Vec3::new(0.0, 0.0, 6000.0);
    assert!(rig.run_until(30.0, |r| r.seen.contains(&LOST)), "{:?}", rig.seen);
    assert!(rig.run_until(10.0, |r| r.seen.contains(&WALK)), "{:?}", rig.seen);
}

#[test]
fn the_toady_one_does_not_mark_a_player_but_strikes_at_what_is_in_reach() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::of(&a, TOADY_BLOYSTER, Vec3::new(0.0, 0.0, 3000.0));
    assert_eq!(rig.tekis.get(rig.id).unwrap().hp, 800.0, "its own life");
    rig.player.pos = Vec3::new(-200.0, 0.0, -200.0);
    rig.run_until(15.0, |_| false);
    assert!(!rig.seen.contains(&FIND) && !rig.seen.contains(&SEARCH), "it never marks him: {:?}", rig.seen);
    let t = rig.tekis.get(rig.id).unwrap();
    rig.player.pos = t.pos + t.forward() * 100.0;
    assert!(rig.run_until(10.0, |r| r.seen.contains(&ATTACK)), "{:?}", rig.seen);
}

#[test]
fn they_die_with_the_dying_notice() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    for kind in [RANGING_BLOYSTER, TOADY_BLOYSTER] {
        let mut rig = Rig::of(&a, kind, Vec3::new(0.0, 0.0, 3000.0));
        rig.run_until(2.0, |_| false);
        rig.tekis.damage(rig.id, 1.0e6, None);
        assert!(rig.run_until(15.0, |r| r.tekis.get(r.id).is_none()));
        let dying = rig.outs.iter().position(|o| matches!(o, Out::Dying { .. }));
        let died = rig.outs.iter().position(|o| matches!(o, Out::Died { .. }));
        assert!(dying.is_some() && died.is_some() && dying < died);
    }
}
