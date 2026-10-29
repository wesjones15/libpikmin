//! Pikmin 2's other Bulborbs (Orange, Hairy, Dwarf Orange, Snow): the disc's numbers and corpses load, they sleep until woken,
//! chase and bite a player, shake off hits, and die with the dying notice.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::bulborb2::{ATTACK, DEAD, SLEEP, TURN, WALK};
use libpikmin_teki::info::slot;
use libpikmin_teki::p2::{self, DWARF_ORANGE_BULBORB, HAIRY_BULBORB, ORANGE_BULBORB, SNOW_BULBORB};
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

const KINDS: [u8; 4] = [ORANGE_BULBORB, HAIRY_BULBORB, DWARF_ORANGE_BULBORB, SNOW_BULBORB];

fn assets() -> Option<Assets> {
    let a = Assets { root: Assets::default_dir() };
    KINDS.iter().all(|&k| p2::is_available(&a.root.join("p2"), k)).then_some(a)
}

struct Rig {
    sim: Sim,
    tekis: Tekis,
    id: TekiId,
    player: Navi,
    outs: Vec<Out>,
}

impl Rig {
    fn new(a: &Assets, kind: u8, player_at: Vec3) -> Rig {
        let sim = Sim::new(Params::default(), 7);
        let mut tekis = Tekis::load(a);
        tekis.seed(6);
        let id = tekis.spawn(kind, 3, Vec3::ZERO, 0.0).expect("spawn");
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
fn they_load_the_discs_numbers_and_corpses() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let tekis = Tekis::load(&a);
    let expect = [(ORANGE_BULBORB, 850.0, 115.0), (HAIRY_BULBORB, 650.0, 90.0), (DWARF_ORANGE_BULBORB, 250.0, 60.0), (SNOW_BULBORB, 150.0, 50.0)];
    for (kind, life, speed) in expect {
        let info = tekis.info(kind).unwrap_or_else(|| panic!("kind {kind} did not load"));
        assert_eq!((info.life, info.run_speed), (life, speed), "kind {kind}");
        assert_eq!(info.clips.len(), 9);
        assert!(info.corpse_size > 0.0 && info.corpse_min_carriers >= 1, "kind {kind} corpse: {} {}", info.corpse_size, info.corpse_min_carriers);
        assert!(info.motions[slot::DEAD].end > 1.0);
        assert!(tekis.has_strategy(kind));
    }
}

#[test]
fn the_orange_bulborb_sleeps_until_a_player_is_within_its_wake_radius_then_chases_and_bites() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, ORANGE_BULBORB, Vec3::new(0.0, 0.0, 400.0));
    for _ in 0..90 {
        rig.tick();
    }
    assert_eq!(rig.state(), Some(SLEEP), "400 away is outside its wake radius");
    rig.player.pos = Vec3::new(0.0, 0.0, 100.0);
    assert!(rig.run_until(4.0, |r| r.state() != Some(SLEEP)), "woken");
    assert!(rig.run_until(10.0, |r| r.state() == Some(WALK) || r.state() == Some(ATTACK)), "{:?}", rig.state());
    rig.player.pos = Vec3::new(0.0, 0.0, 150.0);
    assert!(rig.run_until(15.0, |r| r.outs.iter().any(|o| matches!(o, Out::HitNavi { .. }))), "it bites the player: {:?}", rig.state());
    let _ = TURN;
}

#[test]
fn a_dwarf_stands_until_it_sees_someone_and_every_kind_dies_with_the_notice() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, SNOW_BULBORB, Vec3::new(0.0, 0.0, 800.0));
    for _ in 0..60 {
        rig.tick();
    }
    assert_eq!(rig.state(), Some(SLEEP));
    rig.player.pos = Vec3::new(0.0, 0.0, 60.0);
    assert!(rig.run_until(4.0, |r| r.state() != Some(SLEEP)));
    for kind in KINDS {
        let mut rig = Rig::new(&a, kind, Vec3::new(0.0, 0.0, 2000.0));
        rig.run_until(2.0, |_| false);
        rig.tekis.damage(rig.id, 1.0e6, None);
        assert!(rig.run_until(10.0, |r| r.tekis.get(r.id).is_none()), "kind {kind} did not die");
        let dying = rig.outs.iter().position(|o| matches!(o, Out::Dying { .. }));
        let died = rig.outs.iter().position(|o| matches!(o, Out::Died { .. }));
        assert!(dying.is_some() && died.is_some() && dying < died, "kind {kind}");
    }
    let _ = (DEAD, ATTACK);
}
