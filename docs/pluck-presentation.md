# Pluck presentation

Source: Pikmin 1 `src/plugPikiKando/naviState.cpp`,
`NaviNukuAdjustState::exec`, `NaviNukuState::init/procAnimMsg`;
`interactPullout.cpp::InteractPullout::actPiki`;
`pikiState.cpp::PikiNukareState::procAnimMsg`.

Exact assets (not Melee effects; no efSync cue applies):

| Asset | Identity | Source proof |
| --- | --- | --- |
| Dirt cloud | particle emitter 50, `sd_rakk1.pcr`, `hokori4.bti` | `EffectMgr.h::EFF_SD_DirtCloud`, `plugPikiYamashita/effectMgr.cpp` resource table |
| Dirt debris | particle emitter 51, `sd_rakk2.pcr`, `iwa_1_32.bti` | `EffectMgr.h::EFF_SD_DirtSpray`, same table |
| Pikmin normal | motion 6 NUKARERU, AnimInfo 29 `nukareru.dca`, dirt key 22, end 59 | `InteractPullout::actPiki`, `PaniPikiAnimator::createMotionTable`, disc AnimMgr |
| Pikmin chained | motion 83 NUKARE fast, AnimInfo 93 `nukare3.dca`, dirt key 9, end 49 | same sources |
| Sounds | 0x136 `SE_PIKI_PULLING`, 0x137 `SE_PIKI_PULLED2`, 0x147 `SE_PIKI_PULLED` | `NaviNukuState`, `SoundID.h`, `soundMgr.cpp` |

The shared simulation replaces the sprout when the captain's pull begins. The Pikmin
keeps its own pluck state through the complete clip; authored joint motion supplies
the lift instead of the former generic physics hop. Captain and Pikmin counters stay
aligned while the captain pulls. Formation, throws and whistle recruitment wait for
the recovery clip to end. The Onion's automatic plucker uses the same target state.
All state and presentation events survive snapshots (wire version 12, ABI 54).

Viewer proof, produced before C# changes, under the workspace `output/`:
`pluck-10.png`, `pluck-26.png`, `pluck-55.png`, `pluck-68.png`.
Each capture labels the motions, particle IDs and source functions. Reproduce from
`libpikmin/` with `cargo run --release -p viewer --example pluck -- ../output/pluck-26.png 26`.
The example uses the real simulation's held action input, so the second sprout
exercises the chained fast clip.

Checks: core `tests/pluck.rs` (normal/fast timing, single cues, recovery, field limit),
snapshot all-state/event round trips, presentation `tests/pluck.rs` (animated finite
vertices, both dirt textures through the host draw path, all three sounds render
finite nonzero PCM), and Olimar `NativeCheck` (state and cues through P/Invoke).
Disc-dependent checks print SKIP without the extracted cache.

Scope: grounded plucking on ordinary terrain. The source's water-bubble alternative
requires a ground-surface material query; that query is not exposed by this sim's Host.
This change does not implement the source's tutorial camera or its formation-priority
back-pull displacement. Not yet played in RoR2.
