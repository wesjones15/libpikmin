# libpikmin

Native squad-simulation runtime used by [ror2-olimar](https://github.com/wesjones15/ror2-olimar). This workspace provides captain and squad behavior, carrying, storage and growth, asset readers, enemy state machines, presentation, audio and a C interface. The host supplies world collision, creatures, damage handling and carrying routes.

This is a source dependency. Installation and player controls belong in the [host README](https://github.com/wesjones15/ror2-olimar).

## Structure

| Crate | Responsibility |
| --- | --- |
| [`libpikmin`](crates/libpikmin/) | Fixed-step simulation, captain input, formation, throwing, combat, carrying, storage, growth and snapshots. |
| [`libpikmin-assets`](crates/libpikmin-assets/) | Disc extraction and model, texture, animation, effect, screen and sound-data readers. |
| [`libpikmin-teki`](crates/libpikmin-teki/) | Enemy state machines and outputs for hosts to apply to their own bodies. |
| [`libpikmin-audio`](crates/libpikmin-audio/) | Sequencing, voices and PCM mixing. |
| [`libpikmin-present`](crates/libpikmin-present/) | Animation selection, model posing/skinning, material textures, particles and audio presentation. |
| [`libpikmin-ffi`](crates/libpikmin-ffi/) | C interface for simulation, replicas, extraction, enemies and presentation; builds `pikmin.dll` on Windows. |
| [`viewer`](crates/viewer/) | Standalone 3D inspection host with a 2D debug view. |

The simulation has no Unity or host-game types. Implementation scope, source references, host extensions and known gaps are recorded in the [behavior spec](docs/behavior-spec.md).

## Dependencies and build

Use a Rust toolchain supporting edition 2024. The maintained branch is `modern/1.5.0`. Cargo expects these sibling checkouts:

```text
workspace/
  libpikmin/      modern/1.5.0
  libcommon/      modern/1.5.0
  libsms/         modern/1.5.0
  libssbm/        main
```

[`libcommon`](https://github.com/wesjones15/libcommon) supplies shared Rust utilities. Presentation uses additional readers and model support from [`libsms`](https://github.com/wesjones15/libsms) and [`libssbm`](https://github.com/wesjones15/libssbm); those source dependencies are needed even when optional runtime content is not selected. The C# host shares its integration code through [`ror2-common`](https://github.com/wesjones15/ror2-common).

Build the native host library from this directory:

```powershell
cargo build --release -p libpikmin-ffi
```

On Windows the output is `target/release/pikmin.dll`. A host-game installation and extracted assets are not required to compile it.

## Local data

Runtime data is read from a user-supplied disc image into an ignored local cache. Disc images, extracted assets and generated caches are not included in the repository or release packages.

The native extractor accepts ISO, GCM and CISO:

```powershell
cargo run --release -p libpikmin-assets --bin extract-assets -- "C:\path\to\disc.iso"
```

The default output is `assets/` at the repository root; a second argument selects another directory. Convert other image containers to a supported format first.

[`Assets`](crates/libpikmin-assets/src/lib.rs) provides completeness-based discovery, extraction and loading helpers. Standalone discovery supports an explicit asset directory or `PIKMIN_ASSETS`; disc discovery supports `PIKMIN_DISC` and host-supplied search directories. A complete cache can be reused without reopening the disc. Optional extraction sets are prepared separately by hosts that use them.

The current C# host resolves its paths through SharedFiles and passes them to the native library. Follow its README for that setup; the standalone discovery flow is separate.

## Viewer

After preparing the local cache:

```powershell
cargo run --release -p viewer
```

The viewer can also extract on first run from `PIKMIN_DISC` or a supported image in its search directories. It starts in 3D; `V` switches to the top-down debug view. Without usable local data it can fall back to debug drawing and code-default tuning, which does not validate asset-dependent behavior or appearance.

| Keys | Action |
| --- | --- |
| WASD / arrows | Move; hold Ctrl for light-tilt cursor control. |
| Space | Grab/release a throw; pluck when beside a sprout. |
| Shift / right mouse | Whistle. |
| IJKL / X | Swarm / dismiss. |
| Tab | Switch captain. |
| Q/E / mouse wheel | Rotate / zoom the 3D camera. |
| V / F1 | Switch view / formation debug. |
| R / M | Reset / cycle music. |

These are development-host controls, separate from the plugin layout. See [`viewer/src/main.rs`](crates/viewer/src/main.rs) for the additional inspection switches and [`viewer/examples/`](crates/viewer/examples/) for focused captures.

## Host interface

The [`Host` trait](crates/libpikmin/src/host.rs) supplies ground and wall collision, stable creature handles and collision parts, damage handling and carry paths. Hosts provide routes appropriate to their own world; the default carrying path is a straight line.

The simulation runs at **30 Hz**. [`Sim::advance`](crates/libpikmin/src/sim.rs) accumulates elapsed time and steps the world; hosts can render independently. Coordinates use the simulation's roughly centimeter units, with scale and axis conversion owned by the adapter. Load local tuning data when assessing behavior; code defaults support fallback operation and tests.

Non-Rust hosts use the [C interface](crates/libpikmin-ffi/src/lib.rs). Check `pik_abi_version` and `pik_struct_sizes` against the host bindings before creating handles. The current modern-branch ABI is **64**; the source constant is authoritative. Release owned handles and drain `pik_take_panic` for diagnostics.

For networked hosts, the authoritative simulation produces snapshots. Replicas apply and interpolate them rather than independently stepping the swarm; a local captain pose can be overlaid for responsiveness. Check `pik_snapshot_version` as well as the ABI and use matching native libraries and bindings. Transport and authority rules belong to the host.

The enemy crate exposes state-machine outputs for a host to apply while retaining its own health, collision and damage systems. Presentation consumes simulation state/events to select animation, pose models, run effects and play sound. The audio engine renders PCM; the presentation layer can use device output. Source documentation in each crate describes those contracts.

## Checks and limitations

```powershell
cargo test --release
```

The default workspace members cover the native libraries; run the viewer separately. Disc-dependent checks report a skip when their local data is absent, so a passing clean-checkout run does not establish that those asset paths were exercised.

The [behavior spec](docs/behavior-spec.md) records supported mechanics and host adaptations. Carry routing, world collision, damage conversion and final appearance depend on the adapter. Headless checks and captures establish the behavior they cover, not complete host-game or hardware validation. Feature documents may retain earlier ABI or validation notes; current source and matching bindings take precedence.

## License and credits

Original contributions by **wesleyasaurus** are dedicated to the public domain under
[CC0 1.0 Universal](LICENSE), to the extent wesleyasaurus owns the relevant rights.
Attribution for those contributions is appreciated but not required. Third-party software
and original game content retain their respective rights and terms.
See [CREDITS.md](CREDITS.md) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
