//! Enemies ("teki") for hosts that bring their own AI: what the disc says about a kind
//! (`libpikmin_teki::info`, re-exported) and the instances a host asks [`crate::View`] to draw.

pub use libpikmin_teki::info::*;
use glam::Vec3;

/// One enemy for the view to draw this frame. The host owns where it is and what it does.
#[derive(Debug, Clone, Copy)]
pub struct TekiInstance {
    /// Stable while the enemy lives; the draw's key and id.
    pub id: u32,
    pub kind: u8,
    pub pos: Vec3,
    /// Radians about +Y (the direction it faces is `(sin, 0, cos)`).
    pub yaw: f32,
    /// A `.key` AnimMgr index, usually [`TekiInfo::motions`]`[slot].index`.
    pub motion: u32,
    /// Animation frame (30 a second).
    pub frame: f32,
    /// Seconds of hit flash left.
    pub flash: f32,
    /// 1 at the disc's size; below 1 shrinks a corpse into the Onion.
    pub scale: f32,
    /// 1 opaque .. 0 gone.
    pub alpha: f32,
    /// BTeki::refresh's material-animation frame (Pellet Posy's cycling flower colour).
    pub material_frame: Option<f32>,
}
