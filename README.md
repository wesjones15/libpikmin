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
