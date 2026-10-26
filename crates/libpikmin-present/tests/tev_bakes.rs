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
    for t in &view.textures.list {
        hash = fnv(hash, &t.width.to_le_bytes());
        hash = fnv(hash, &t.height.to_le_bytes());
        hash = fnv(hash, &t.wrap);
        hash = fnv(hash, &t.rgba);
    }
    println!("{} textures, hash {hash:#x}", view.textures.list.len());
    assert!(!view.textures.list.is_empty());
    assert_eq!((view.textures.list.len(), hash), BASELINE);
}

// Recorded from the evaluator before it moved to libcommon::tev.
const BASELINE: (usize, u64) = (59, 0x997693f070023756);
