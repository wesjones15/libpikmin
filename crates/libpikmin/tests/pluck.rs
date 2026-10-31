use libpikmin::{CaptainInput, Event, Params, PikiColor, PikiState, Sim, Vec3};
use libpikmin::host::FlatHost;

#[test]
fn normal_and_chained_plucks_run_the_whole_clip_and_emit_each_cue_once() {
    let mut sim = Sim::new(Params::default(), 9);
    let c = sim.add_captain(Vec3::ZERO);
    let first = sim.plant_sprout(PikiColor::Red, Vec3::new(0.0, 0.0, 6.0));
    let second = sim.plant_sprout(PikiColor::Yellow, Vec3::new(36.0, 0.0, 6.0));
    let mut starts = Vec::new();
    let mut bursts = Vec::new();
    let mut pulled = Vec::new();
    let mut poses = [false; 2];
    for _ in 0..150 {
        sim.set_input(c, CaptainInput { throw: true, ..Default::default() });
        sim.step(&mut FlatHost);
        for e in sim.drain_events().collect::<Vec<_>>() {
            match e {
                Event::PluckStarted { piki } => {
                    assert!(matches!(sim.piki(piki).state, PikiState::Plucked { frame: 0.0, .. }));
                    starts.push(piki);
                }
                Event::PluckBurst { piki } => {
                    let PikiState::Plucked { frame, fast, .. } = sim.piki(piki).state else { panic!("burst outside clip") };
                    assert_eq!(frame, if fast { 9.0 } else { 22.0 });
                    bursts.push(piki);
                }
                Event::Plucked { piki, sprout, .. } => {
                    assert!(sprout == first || sprout == second);
                    assert!(matches!(sim.piki(piki).state, PikiState::Plucked { .. }), "keep the recovery part of the clip");
                    assert!(!sim.captain(c).plate.members.contains(&piki), "formation waits for KEY_Finished");
                    pulled.push(piki);
                }
                _ => {}
            }
        }
        for p in sim.pikis() {
            if let PikiState::Plucked { fast, .. } = p.state {
                poses[fast as usize] = true;
                assert_eq!(p.vel, Vec3::ZERO, "authored pose supplies the lift, not physics");
                assert!(p.on_ground);
            }
        }
    }
    assert_eq!(starts.len(), 2);
    assert_eq!(starts, bursts);
    assert_eq!(starts, pulled);
    assert_eq!(poses, [true, true]);
    assert_eq!(sim.captain(c).plate.len(), 2);
    assert_eq!(sim.sprouts().count(), 0);
}

#[test]
fn a_full_field_keeps_the_sprout_and_never_starts_a_phantom_pluck() {
    let mut params = Params::default();
    params.ai.max_pikis_on_field = 1;
    let mut sim = Sim::new(params, 1);
    let c = sim.add_captain(Vec3::ZERO);
    sim.spawn_piki(PikiColor::Blue, Vec3::X * 1000.0).unwrap();
    sim.plant_sprout(PikiColor::Red, Vec3::Z * 6.0);
    sim.set_input(c, CaptainInput { throw: true, ..Default::default() });
    for _ in 0..90 { sim.step(&mut FlatHost); }
    assert_eq!(sim.sprouts().count(), 1);
    assert!(!sim.drain_events().any(|e| matches!(e, Event::PluckStarted { .. } | Event::Plucked { .. } | Event::PluckBurst { .. })));
}
