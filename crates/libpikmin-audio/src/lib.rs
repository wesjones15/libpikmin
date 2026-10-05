//! Pikmin 1's sound system (JAudio 1), reimplemented from the projectPiki decomp as a
//! behavioural spec: it plays the disc's own sequences, instruments and waves into PCM.
//!
//! - [`engine`]: the state JAudio keeps in globals (tracks, channels, voices).
//! - `seq`: the BMS sequence interpreter (jammain_2.c, seqsetup.c).
//! - `channel`: notes and envelopes (oneshot.c, driverinterface.c).
//! - `dsp`: voices and the mixer (dspdriver.c; the DSP microcode itself isn't in the decomp).
//! - [`system`]: the game-facing `Jac_*` layer and `SeSystem` (system and Olimar sounds,
//!   positional events, music).
//!
//! Nothing here contains Nintendo data: banks, sequences and the game's sound tables are read
//! from the user's extracted `assets/`.

// The port indexes parallel arrays the way the C does; iterator rewrites would obscure that.
#![allow(clippy::needless_range_loop)]

pub mod calc;
mod channel;
mod dsp;
pub mod engine;
pub mod osc;
mod seq;
pub mod system;

pub use system::SoundSystem;
