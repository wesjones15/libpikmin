//! C ABI over `libpikmin`. Hosts that aren't Rust (or that load us as a DLL) use this.
//! The header will be generated with cbindgen once the API settles.

use std::ffi::c_void;

use libpikmin::{Host, Sim, Vec3};

/// Callbacks the host provides. `user` is passed back untouched.
#[repr(C)]
pub struct PikHost {
    pub user: *mut c_void,
    /// Writes the ground height under (x, y, z) to `out_y` and returns true, or returns false over a void.
    pub ground_height:
        Option<extern "C" fn(user: *mut c_void, x: f32, y: f32, z: f32, out_y: *mut f32) -> bool>,
}

impl Host for PikHost {
    fn ground_height(&self, pos: Vec3) -> Option<f32> {
        let f = self.ground_height?;
        let mut y = 0.0;
        f(self.user, pos.x, pos.y, pos.z, &mut y).then_some(y)
    }
}

pub struct PikSim(Sim);

#[unsafe(no_mangle)]
pub extern "C" fn pik_sim_new() -> *mut PikSim {
    Box::into_raw(Box::new(PikSim(Sim::default())))
}

/// # Safety
/// `sim` must come from `pik_sim_new` and not be used afterwards. Null is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_free(sim: *mut PikSim) {
    if !sim.is_null() {
        drop(unsafe { Box::from_raw(sim) });
    }
}

/// Advances by `dt` seconds of real time. Returns the number of 30 Hz ticks run.
///
/// # Safety
/// `sim` and `host` must be valid, non-null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_advance(sim: *mut PikSim, host: *mut PikHost, dt: f32) -> u32 {
    let (sim, host) = unsafe { (&mut (*sim).0, &mut *host) };
    sim.advance(host, dt)
}

/// # Safety
/// `sim` must be a valid, non-null pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_tick(sim: *const PikSim) -> u64 {
    unsafe { (*sim).0.tick() }
}
