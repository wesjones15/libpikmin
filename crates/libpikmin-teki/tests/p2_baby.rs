//! Pikmin 2's Bulborb Larva: the disc's numbers load, it hatches, chases, bites, and dies when hurt or pressed.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::baby::{ATTACK, BORN, MOVE};
use libpikmin_teki::p2::{self, BULBORB_LARVA};
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
    p2::is_available(&a.root.join("p2"), BULBORB_LARVA).then_some(a)
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
        tekis.seed(5);
        let id = tekis.spawn(BULBORB_LARVA, 3, Vec3::ZERO, 0.0).expect("spawn");
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
fn it_loads_the_discs_numbers_and_has_no_corpse() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let tekis = Tekis::load(&a);
    let info = tekis.info(BULBORB_LARVA).expect("loaded");
    assert_eq!(info.life, 5.0);
    assert_eq!(info.scale, 1.0, "no mScale of its own: drawn at the model size");
    assert_eq!((info.run_speed, info.visible_range), (40.0, 800.0));
    assert_eq!(info.clips, vec!["dead", "deadpress", "move", "attack", "attackfail", "born"]);
    assert_eq!(info.floats[p2::fp::own::BASE + 11], 0.2, "nectar chance");
    assert!(tekis.has_strategy(BULBORB_LARVA));
}

#[test]
fn it_hatches_then_chases_and_bites_a_player() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 200.0));
    assert_eq!(rig.state(), Some(BORN));
    assert!(rig.run_until(5.0, |r| r.state() == Some(MOVE)), "hatched");
    let start = rig.tekis.get(rig.id).unwrap().pos;
    assert!(rig.run_until(10.0, |r| r.state() == Some(ATTACK)), "it should close on the player and bite");
    let t = rig.tekis.get(rig.id).unwrap();
    assert!((t.pos - start).length() > 100.0, "walked at 40 a second toward him");
    assert!(rig.run_until(4.0, |r| r.outs.iter().any(|o| matches!(o, Out::HitNavi { .. }))));
    let hit = rig.outs.iter().find_map(|o| if let Out::HitNavi { damage, .. } = o { Some(*damage) } else { None }).unwrap();
    assert_eq!(hit, 2.0, "attack power fp24");
}

#[test]
fn it_dies_when_hurt_and_when_pressed() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 500.0));
    assert!(rig.run_until(5.0, |r| r.state() == Some(MOVE)));
    rig.tekis.damage(rig.id, 10.0, None);
    assert!(rig.run_until(5.0, |r| r.tekis.get(r.id).is_none()), "5 life, 10 damage");
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Dying { .. })) && rig.outs.iter().any(|o| matches!(o, Out::Died { .. })));

    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 500.0));
    assert!(rig.run_until(5.0, |r| r.state() == Some(MOVE)));
    rig.tekis.press(rig.id);
    assert!(rig.run_until(5.0, |r| r.tekis.get(r.id).is_none()));
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Sound { name: "BABY_PRESSED", .. })));
}
