//! Presenting the sim with the user's own disc assets, shared by every host: which animation
//! each object plays ([`animator`]), the sounds the game plays for the sim's events
//! ([`audio`]), and for hosts that draw with their own engine, posed and skinned instances of
//! the disc's models ([`view`]) with each material baked into one texture ([`tev`]).
//!
//! The viewer draws with its own TEV shaders and uses [`animator`] and [`audio`]; game hosts
//! use [`view`] and [`audio`] through `libpikmin-ffi`.

pub mod animator;
pub mod audio;
pub mod effects;
pub mod hocotate;
pub mod louie;
pub mod navilight;
pub mod pikachu;
pub mod pianta;
pub mod p2enemy;
pub mod pikmin2;
pub mod stings;
pub mod teki;
pub mod tev;
pub mod view;

pub use audio::Audio;
pub use view::View;
