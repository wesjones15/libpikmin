# Pikmin 2 enemy presentation and directed music

The adapter uses the source's particle resources from `effects/game.jpc` (JPAC2-10), decoded into libsms's shared JParticle runtime. These are particle emitters, not Melee effect models. No replacement assets are used.

| Presentation | Exact emitters | Source proof in `pikmin2-main` |
|---|---|---|
| Aboveground gas pipe | 0x2B4/2B5; cave pair 0x74/75 retained | `efxEnemy.cpp::TGasuHiba::create`, `GasHiba.cpp::startGasEffect`, `efx/TGasuHiba.h` |
| Hairy Bulborb hair / shedding | 0x283-285 / 0x294 | `YellowChappy.cpp::update` and `efx/TKechappy.h`; alpha rises 10 and falls 50 per 30 Hz tick |
| Sleeping bubbles | 0x8B; Fiery Bulblax 0x8C | `efx/THanacho.h`, `ChappyBase.cpp`, `FireChappy.cpp::setupEffect` (head matrix) |
| Fiery Bulblax | 0x1E6-1E9 body (1E9 indirect heat), 0x241 death smoke, 0x242 flick, 0x286 steam | `efx/TYaki.h`, `FireChappy.cpp::setupEffect/startBodyEffect/createDeadSteamEffect` |
| Man-at-Legs | 0x9B shell, 0x9F sight, 0x9D/9E/247 shot; 0x8D-90 ground impact, 0x97-9A water impact | `efx/THdama.h`, `HoudaiShotGun.cpp::Shell::update` and firing paths |
| Long Legs pinch smoke | 0x27 | `Damagumo.cpp` / `BigFoot.cpp::startPinchJointEffect`: three different legs' hip/knee positions; `Houdai.cpp::setupEffect`: right-front hip and knee |
| Raging Long Legs fur | 0xF2 body, 0xF3 legs | `efx/TOdama.h`, `BigFoot.cpp::setupEffect/startFurEffect`: kosi and four second leg joints |
| Beady/Raging foot lift and landing | water 0x23,28-2A; Beady 0x24/25,2B/2C; Raging 0xF1/25,F5/F6 | `efx/TDama.h`, `efx/TOdama.h`, `Damagumo.cpp` / `BigFoot.cpp::createOffGroundEffect/createOnGroundEffect` |

Body emitters follow animated matrices. Moving shells, sights and lifted feet use stable follow/stop keys; lost owners expire after eight ticks. Releasing a held effect stops emission while live particles finish. The native particle record is 104 bytes and carries all four UVs, dynamic texture versions, and a refraction blend. ABI 55 introduced this boundary. Later unrelated ABI increments may coexist.

Shared rendering preserves independent U/V samplers and implements both JPA2 alpha TEV selectors (texture alpha times primary alpha, or primary alpha alone). The selected resources use selector zero. The shared mirror regression checks both axes; no asset-wide U/V flip is applied. Heat uses the common indirect map and the host scene-distortion material. The viewer also renders a scene-refraction pass, with a grid to expose distortion; this is a host projection adaptation, not a claim of pixel-identical GX framebuffer behavior.

## Evidence

`cargo run --release -p viewer --example p2_effects -- <png> <tick> [body|gun|smoke|heat|fur|feet]` renders exact resources with IDs and source labels. Captures in the workspace `output/` include body and heat at ticks 10,30,70, and gun/smoke/fur/feet at multiple frames. The PNGs are local extracted-asset evidence and are not distributed. Tests exercise every listed emitter's finite geometry, texture boundary, release, and orphan-shell timeout. Shared particle tests cover TEV and mirrored U/V. The build has not been played in-game.

## Music

Ship Repair mixes `n_tutorial.bms`, `forest.bms`, `yakushima.bms`, and `last.bms` with the existing Pikmin 1 soundtrack. Pikmin 2 selection probability is 20% on stages 1-2, 35% on stages 3-5, and 50% later, seeded identically for late joins. Source director groups come from `user/Wakai/trackMap_Seq_W.txt` and `user/Totaka/trackMap_Seq_T.txt` (`PSM::BgmTrackMapFile::read`, `DirectorMgr_Scene::newDirector`). Nearby enemies, close combat, carrying work, nearby ship parts and squad presence drive child-track volume fades; sequence time continues through fades. Pause freezes advancement; unavailable P2 data falls back to P1.

`PSGame.cpp::PikSceneMgr::seqCpuSync` / `PSSeq.cpp::JumpBgmSeq::outputJumpRequest` require 0xFFFF for no jump (0x4001); zero restarts the score every tick. The shared audio tests render all four scores, check nonzero finite PCM, compare activity mixes, and render the nine added enemy cues. Cave AutoBgm/conductor scores are outside this selection; they require the separate source conductor machinery.

Sound IDs added: beam 0x3055, water impact 0x5805, pinch smoke 0x5844, Raging raises 0x5848-4B, Fiery flame/end 0x50B4/58B5 (`SoundID.h`). Audio extraction version 2 refreshes older caches with the two director maps.
