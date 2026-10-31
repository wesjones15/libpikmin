//! Every baked material of the user's extracted models, pinned pixel for pixel before the TEV
//! arithmetic moved to libcommon (skipped when `assets/` isn't there).

use libpikmin_assets::Assets;
use libpikmin_present::View;

fn fnv(h: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(h, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

#[test]
fn baked_textures_match_baseline() {
    let Some(assets) = Assets::discover(None) else {
        eprintln!("SKIP: no extracted assets");
        return;
    };
    let view = View::load(&assets).expect("models load");
    let mut hash = 0xcbf29ce484222325u64;
    for (i,t) in view.textures.list.iter().enumerate() {
        hash = fnv(hash, &t.width.to_le_bytes());
        hash = fnv(hash, &t.height.to_le_bytes());
        hash = fnv(hash, &t.wrap);
        hash = fnv(hash, &t.rgba);
        // The original 19-model palette remains pinned independently of
        // the newly appended falling-nectar and puddle materials.
        if i + 1 == BASELINE.0 { assert_eq!(hash, BASELINE.1); }
    }
    println!("{} textures, hash {hash:#x}", view.textures.list.len());
    assert!(!view.textures.list.is_empty());
    assert_eq!((view.textures.list.len(), hash), WITH_NECTAR);
}

// Recorded from the evaluator before it moved to libcommon::tev.
const BASELINE: (usize, u64) = (59, 0x997693f070023756);
// Exact water/fl_water materials appended by ItemMgr's shared presentation.
const WITH_NECTAR: (usize, u64) = (61, 0x49f6160fa9011236);
