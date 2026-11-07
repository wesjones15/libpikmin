> modern/1.5.0 branch: targets RiskOfRain2_DLC4 (Unity 2021.3.45f2). Build/API validation only; in-game validation is pending. Legacy instructions below describe the previous release.

# libpikmin

A game-agnostic Rust reimplementation of Pikmin 1's Olimar, Pikmin swarm, Onion and
corpse-carrying mechanics, written from the [projectPiki decompilation](https://github.com/projectPiki/pikmin)
as a behavioral spec. The goal is to host it inside other games. The first planned host is Elden
Ring (offline only).

This repository contains no Nintendo data. Tuning values are read from your own copy of the
game at runtime.

## Layout
- `crates/libpikmin`: the simulation (fixed 30 Hz step, talks to the world through a `Host` trait)
- `crates/libpikmin-ffi`: C ABI
- `crates/viewer`: top-down 2D test host
- `docs/`: behavior spec

## Build
```
cargo test
cargo run -p viewer
```

## License and credits

Original contributions by **wesleyasaurus** are dedicated to the public domain under
[CC0 1.0 Universal](LICENSE), to the extent wesleyasaurus owns the relevant rights.
Attribution for those contributions is appreciated but not required. Third-party software
and original game content retain their respective rights and terms.
See [CREDITS.md](CREDITS.md) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
