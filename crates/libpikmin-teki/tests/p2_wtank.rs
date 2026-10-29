//! Pikmin 2's Watery Blowhog: the disc's numbers and corpse load, it patrols, turns on a player in sight and blows its
//! stream at him, and dies with the dying notice.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::info::slot;
use libpikmin_teki::p2::{self, WATERY_BLOWHOG};
use libpikmin_teki::wtank::{ATTACK, CHASE_TURN, MOVE_TURN};
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
    p2::is_available(&a.root.join("p2"), WATERY_BLOWHOG).then_some(a)
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
        tekis.seed(2);
        let id = tekis.spawn(WATERY_BLOWHOG, 3, Vec3::ZERO, 0.0).expect("spawn");
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
fn it_loads_its_numbers_on_the_fiery_blowhogs_model_with_its_own_corpse() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let tekis = Tekis::load(&a);
    let info = tekis.info(WATERY_BLOWHOG).expect("loaded");
    assert_eq!((info.life, info.run_speed), (1000.0, 80.0));
    assert_eq!(info.clips[0], "dead");
    assert_eq!(info.corpse_size, 25.0);
    assert_eq!((info.corpse_min_carriers, info.corpse_max_carriers), (7, 15));
    assert!(info.motions[slot::DEAD].end > 1.0);
    assert!(tekis.has_strategy(WATERY_BLOWHOG));
}

#[test]
fn it_patrols_and_turns_on_a_player_in_sight_then_blows_water_at_him() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 1000.0));
    assert!(rig.run_until(10.0, |r| r.state() == Some(MOVE_TURN)), "with nobody about it sets out on its patrol");
    rig.player.pos = Vec3::new(0.0, 0.0, 100.0);
    rig.tekis.get_mut(rig.id).unwrap().yaw = 0.0;
    assert!(rig.run_until(20.0, |r| r.state() == Some(CHASE_TURN) || r.state() == Some(ATTACK)), "it notices him");
    assert!(rig.run_until(20.0, |r| r.state() == Some(ATTACK)), "and blows at him");
    assert!(rig.run_until(4.0, |r| r.tekis.get(r.id).is_some_and(|t| t.fire.is_some())), "a stream comes out");
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Sound { name: "WATERTANK_BREATH", .. })));
}

#[test]
fn it_dies_with_the_dying_notice() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 1000.0));
    rig.run_until(2.0, |_| false);
    rig.tekis.damage(rig.id, 1.0e6, None);
    assert!(rig.run_until(8.0, |r| r.tekis.get(r.id).is_none()));
    let dying = rig.outs.iter().position(|o| matches!(o, Out::Dying { .. }));
    let died = rig.outs.iter().position(|o| matches!(o, Out::Died { .. }));
    assert!(dying.is_some() && died.is_some() && dying < died);
}
