# libpikmin

Goal: reimplement Pikmin 1 mechanics (Olimar, Pikmin swarm, Onion, corpse carrying) as a
game-agnostic Rust library. Host: Risk of Rain 2, 2023 build (`../ror2-olimar/`). Minecraft
Java Edition is a possible later host; Elden Ring is shelved.

## Layout
This folder is the repo root. All work happens here. Sibling folders are read-only references:
- `../pikmin-main/`: projectPiki decomp, GPIE01_01. READ-ONLY spec. It builds
  100% matching against the user's disc (`orig/GPIE01_01/*.ciso`).
- `../ror2-olimar/`: the Risk of Rain 2 host (its own git repo, BepInEx plugin + `pikmin.dll`).
- `../ror2-mario/`: the Super Mario Sunshine RoR2 host it was modelled on (another session's work).
- `../er-mario-master/`: reference for the shelved Elden Ring host (me3 DLL, libsm64, fromsoftware-rs @ 59fbd3b).
- `../pikmin2-main/`: projectPiki Pikmin 2 decomp, GPVE01. READ-ONLY secondary spec. Builds
  against the user's disc (game code 99.82% matched; gameplay code is in `src/plugProjectKandoU/`).
- `../extracted/p2/`: Pikmin 2 text parms copied off the user's disc with dtk (reference only).
- `assets/` (git-ignored, never commit): Pikmin 1 files extracted from the user's disc on first
  run, same layout as `files/dataDir/`: `parms/`, `pikis/`, `objects/{goal,pellets,pikihead,water,
  fl_water}/`, `tekis/{chappy,swallow,qurione}/`, `tekikeys/`, `tekipara/`, `SndData/`, `effects/`,
  `shapes/cursor/`, `objects/{ufo,ufoparts}/`, `screen/{eng_blo,eng_tex}/`, `bigFont.bti`. Everything at
  runtime loads from here; the disc is only read to build it.

Workspace:
- `crates/libpikmin/`: core sim. No Elden Ring or libsm64 types.
- `crates/libpikmin-assets/`: first-run extractor (pure Rust: .iso/.gcm/.ciso → `assets/`, no
  Python or dtk) and loaders for models, textures, animations and particle effects (`.pcr`, run
  by a port of `zen::particleGenerator`), the ship and its 30 parts (`ship.rs`, with a port of
  the ship's and parts' animators), and the 2D screens and font (`screen.rs`: P2D `.blo`,
  `bigFont.bti`). Renderer-agnostic: triangle batches, RGBA images, posed joint matrices. Hosts call `Assets::discover_or_extract` at startup.
- `crates/libpikmin-audio/`: Pikmin 1's sound system (JAudio 1) ported from `src/jaudio/`: the BMS
  sequencer, instruments/envelopes, 64 DSP voices and a resampling mixer, plus the game layer
  (system/Olimar SE, positional events, BGM mixes, `SeSystem`). Renders PCM; hosts own the audio
  device. Banks, sequences and the game's sound tables come from `assets/SndData/` (the extractor
  copies `pikiseq.hed`, `jaudio_tables.bin` and `sound_table.txt` out of `main.dol` by per-release
  symbol addresses). The DSP's reverb and filters aren't emulated.
- `crates/libpikmin-present/`: presentation shared by every host: `animator` (which animation
  each object plays, from sim state), `audio` (the sim's events mapped to the game's sounds, on
  a cpal stream), and for engines that draw themselves `view` (models posed and CPU-skinned
  each frame into one vertex buffer) with `tev` (each material's TEV run on the CPU per texel and
  baked into one RGBA texture, since host shaders take one texture times one colour; env-mapped
  reads count as their average; blended materials are tagged opaque, cutout or translucent),
  and `effects` (the whistle and one-shots like the dead Pikmin's ghost, on the particle
  generator port; `Flat` splits GX's additive colour modes into one-texture layers for host
  shaders). No music: hosts keep theirs. `teki` is for hosts with their own enemy AI (RoR2 monsters):
  `TekiInfo` (the disc's parms, motions and corpse config for the Dwarf and Spotty Bulborb) and
  `View::set_tekis`, which draws the instances a host names (kind, pos, yaw, `.key` motion, frame);
  off unless `set_teki_models(true)` before `View::load`.
- `crates/libpikmin-teki/`: Pikmin 1's enemies as engine-free state machines ported from the decomp's `Tai*` (plugPikiNakata) and
  `TAI*` (plugPikiYamashita) strategies: `tai.rs`/`yai.rs` the shared actions, one module per enemy, `info.rs` the per-kind
  disc facts. A host spawns an enemy for a body of its own, feeds it the ground, water and damage, and applies its `Out`s
  (`pik_tekis_*` in the ffi). The viewer's own ports of the Bulborbs and Honeywisp are older and separate.
- `crates/libpikmin-ffi/`: C ABI for hosts: `pikmin.dll`, `pik_*` functions (`pik_view_*` for
  drawing and sound), `PIK_ABI_VERSION`. ror2-olimar's `Native.cs` mirrors its structs and
  checks `pik_struct_sizes` at load; change both together (and the size test in `src/tests.rs`).
  The replica path for networked hosts: `pik_sim_snapshot` on the server (`libpikmin::Snapshot`:
  every entity field, the creatures' positions and the events since the last, encoded and decoded
  by `snapshot.rs`), `pik_replica_new` / `pik_replica_apply` / `pik_replica_update` on every other
  machine (a sim that never steps and blends between snapshots, so the view draws and sounds it as
  the real one), and `pik_replica_overlay_captain` to show a client's own captain moving as his
  local sim does. Round-trip tests compare the `Debug` output of every entity.
- `crates/viewer/`: test host (macroquad) for play-testing without a game. Uses `libpikmin-present`'s animator and audio. Its enemies (Dwarf/Spotty
  Bulborbs, Honeywisps) are host-side ports of the Pikmin 1 strategies in `viewer/src/teki.rs`. 3D with the disc's
  models and animations (CPU skinning, a GLSL TEV interpreter for PVW materials); V switches to
  the 2D top-down debug view.
- `docs/`: behavior spec written from the decomp.

## Rules
- Reimplement from the decomp as a behavioral spec. Do not copy code verbatim.
  Cite decomp files/functions in comments (e.g. `// aiTransport.cpp ActTransport::moveToWayPoint`).
- The core talks to the world only through the `Host` trait (ground/raycasts, paths, enemy
  queries, damage, corpse handles, spawn/draw events) and exposes a C ABI.
- Fixed 30 Hz step (gameplay uses `setFrameClamp(2)`, newPikiGame.cpp; dt capped at 1/30 in
  `System::updateSysClock`). Hosts pass real dt and the sim accumulates it.
- The core keeps decomp units (cm-ish). Unit/axis conversion lives in host adapters.
- **Tuning values come from the disc, not the code defaults.** The `Parm<T>` defaults in
  decomp headers are overridden by `files/dataDir/parms/*.bin`, and most differ (e.g. Olimar
  160 not 170, whistle radius 90, throw distance 140, Pikmin run 120). Pikmin 2's tuning is
  plain text in `files/user/Abe/piki/{pikiParms,naviParms}.txt` (Shift-JIS). The crate parses
  both from the user's discs at runtime and falls back to code defaults.
- No Nintendo or FromSoftware data in git: no disc images, no extracted `.bin`, models or
  textures. Assets are extracted from the user's own disc at first run into `assets/`. Users
  never need Python or dtk.
- Offline only. Never suggest running modded Elden Ring online.

## Decisions
- Olimar uses the core's own kinematic controller. Keep a seam (host-driven leader pose)
  so a host can drive him later.
- Onion location = any active Site of Grace (ER host).
- An Onion holds a set of colors, so one-per-color (P1 style) and a single shared Onion are
  both just configuration. The user leans P1 style but wants both possible.
- v1 = Pikmin 1 features with the multi-captain rule. Priority: thrown Pikmin landing on
  host creatures and attacking. Carry/Onion come next.
- Target the full swarm of 100 (`aiconst.bin` p15 = 100).
- Full Onion loop: suck → seeds → sprouts → pluck.
- Corpses are pellets (`BTeki::dieSoon` → `becomePellet`). In ER: pin kinematic and drive the
  transform from the sim; fallback is a proxy object: a Pikmin 1 numbered pellet
  (1, 5, 10, 20) sized to the corpse.
- Carry pathing goes through `Host` (Pikmin's own route graphs don't exist in other games).
- **Multiple captains.** The sim supports N captains (one per player). Each Pikmin follows
  the captain it's currently assigned to. Any captain's whistle recruits every Pikmin inside
  its radius, including ones in another captain's squad (stealing). Pikmin 1 only has one
  Navi, so this rule is ours, not the decomp's.

- **Pikmin 1 is the source of truth.** Pikmin 2 is consulted only for: Purple/White/Bulbmin,
  Candypop Buds, hazards, multi-captain logic, and to clear up murky Pikmin 1 code.
- **Types:** Red, Blue, Yellow (P1 values), plus Purple, White, Bulbmin (P2 values). Carrying
  uses P2's two-number model, which reduces to P1's formula for the original three: lift weight
  (`PikiMgr::getColorTransportScale`, Purple = 10) and speed contribution
  (`Piki::getPelletCarryPower`, plus bud/flower bonus).
- **Purple/White come from Candypop Buds** (P2 style: throw Pikmin in, get seeds of the bud's
  color). Hosts decide where buds spawn. How players get Bulbmin is still open.
- **Hazards are in v1:** fire (Red), water (Blue), electricity (Yellow), poison (White);
  Bulbmin resist all. The host reports hazards, and the core applies immunities and panic/death behavior.

## Hosts
- **Risk of Rain 2, 2023 build** (current): 1.2.4.4, Unity 2019.4.37, Mono, BepInEx 5.4.21,
  no R2API. The user plays co-op with friends on that build. `../ror2-olimar/` (OlimarTest):
  the sim runs on the server; RoR2 enemies' hurtboxes are creature parts; Pikmin hits are RoR2
  damage; corpses become pellets. Non-VR uses Pikmin controls (the sim moves Olimar, pad read
  by button position); VR (VRMod) uses aim controls with a host pose and cursor. Don't launch
  the game unasked; the user tests and Claude reads `BepInEx/LogOutput.log`.
- **Elden Ring**: shelved (the user doesn't like the game). DLL via me3, offline only.
- **Minecraft Java Edition**: a Fabric/NeoForge mod in Java that loads the `libpikmin-ffi`
  cdylib over JNI (MC 1.21 targets Java 21, where FFM is still preview). The server
  runs the sim and clients render. No Nintendo models or textures in git; build a resource
  pack from the user's disc or use placeholder models. Host-specific rules are TBD.

## Workflow
- Core is tested headless (`cargo test`). The viewer is for manual play-testing.
- ER testing (later) is manual: the user plays, and Claude reads er-mario's `logs/er_mario.log`.
- Toolchain: rustup stable (MSVC). Python 3.12 lives at
  `%LOCALAPPDATA%\Programs\Python\Python312` and is not on PATH as `python`.

## Order of work
1. `docs/behavior-spec.md` from the decomps (disc values included): follow, whistle, throw,
   attack, carry (weights, carrier counts, speed), Onion, Pikmin types, Candypop Buds, hazards,
   multiple captains.
2. Crate, Host trait, headless host, disc parm loader.
3. Follow/whistle/throw, then attack, then carry/Onion.
4. Viewer play-test.
5. RoR2 host (`../ror2-olimar/`): play-test in game, then networking (snapshots to clients),
   models/animation/sound from the user's disc, hazards.
