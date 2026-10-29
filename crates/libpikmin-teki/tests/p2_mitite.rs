//! Pikmin 2's Mitite: loads with the corpse of the disc, waits underground, calls its fellows, wanders and dives away,
//! and dies to a hit leaving the dying notice and nectar.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::info::slot;
use libpikmin_teki::mitite::{APPEAR, TURN, WALK};
use libpikmin_teki::p2::{self, MITITE};
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
    p2::is_available(&a.root.join("p2"), MITITE).then_some(a)
}

struct Rig {
    sim: Sim,
    tekis: Tekis,
    id: TekiId,
    player: Navi,
    outs: Vec<Out>,
}

impl Rig {
    fn new(a: &Assets, player_at: Vec3, fellow: bool) -> Rig {
        let sim = Sim::new(Params::default(), 7);
        let mut tekis = Tekis::load(a);
        tekis.seed(3);
        let id = tekis.spawn(MITITE, 3, Vec3::ZERO, 0.0).expect("spawn");
        if fellow {
            tekis.set_count(id, 0, 1);
        }
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
fn it_loads_its_numbers_and_the_corpse() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let tekis = Tekis::load(&a);
    let info = tekis.info(MITITE).expect("loaded");
    assert_eq!((info.life, info.run_speed), (50.0, 100.0));
    assert_eq!(info.clips, vec!["dead", "dive", "move", "set", "wait", "carry"]);
    assert_eq!(info.corpse_size, 10.0, "TamagoMushi in carcass_config.txt");
    assert!(info.motions[slot::DEAD].end > 1.0);
    assert!(tekis.has_strategy(MITITE));
}

#[test]
fn a_leader_waits_underground_then_calls_ten_fellows_and_wanders() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 500.0), false);
    for _ in 0..60 {
        rig.tick();
    }
    let t = rig.tekis.get(rig.id).unwrap();
    assert_eq!(t.state, APPEAR);
    assert!(!t.option(opt::ATARI));
    assert!(!rig.outs.iter().any(|o| matches!(o, Out::Spawn { .. })));
    rig.player.pos = Vec3::new(0.0, 0.0, 60.0);
    assert!(rig.run_until(4.0, |r| r.state() == Some(WALK)), "it pops up");
    let fellows = rig.outs.iter().filter(|o| matches!(o, Out::Spawn { kind: MITITE, .. })).count();
    assert_eq!(fellows, 10);
    assert!(rig.tekis.get(rig.id).unwrap().option(opt::ATARI));
    let mut states = std::collections::BTreeSet::new();
    for _ in 0..(6 * 30) {
        rig.tick();
        states.insert(rig.state());
    }
    assert!(states.contains(&Some(WALK)) && states.contains(&Some(TURN)), "{states:?}");
    assert!(rig.tekis.get(rig.id).unwrap().pos.length() > 5.0, "it wandered");
}

#[test]
fn a_fellow_does_not_call_more_and_everyone_dives_away_when_their_time_is_up() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 5000.0), true);
    assert!(rig.run_until(6.0, |r| r.state() == Some(WALK)), "a fellow comes up at once");
    assert!(!rig.outs.iter().any(|o| matches!(o, Out::Spawn { .. })));
    // Survival time 300 ticks x 0.8..1.0 = 8..10 s.
    assert!(!rig.run_until(6.0, |r| r.tekis.get(r.id).is_none()));
    assert!(rig.run_until(8.0, |r| r.tekis.get(r.id).is_none()), "it dives away");
    assert!(!rig.outs.iter().any(|o| matches!(o, Out::Dying { .. })), "leaving is not dying");
}

#[test]
fn a_hit_kills_it_and_it_leaves_nectar_and_the_dying_notice() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 60.0), true);
    assert!(rig.run_until(6.0, |r| r.state() == Some(WALK)));
    rig.tekis.damage(rig.id, 1.0e6, None);
    let mut drops = 0;
    let mut gone = false;
    for _ in 0..(5 * 30) {
        rig.tick();
        drops = drops.max(rig.sim.nectar_drops().len());
        if rig.tekis.get(rig.id).is_none() {
            gone = true;
            break;
        }
    }
    assert!(gone);
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Dying { .. })));
    assert_eq!(drops, 1, "honey rate 1");
}
