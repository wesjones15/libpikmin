//! Inspect/copy the exact P1 BossMgr::constructBoss Kogane assets, never a substitute.
use libpikmin_assets::{AnimMgr, Assets, Disc};
fn main() {
    let arg = std::env::args().nth(1).expect("Pikmin disc path");
    let d = Disc::open(std::path::Path::new(&arg)).unwrap();
    let a = Assets {
        root: Assets::default_dir(),
    };
    for p in d
        .paths()
        .filter(|p| p.to_ascii_lowercase().starts_with("datadir/bosses/kogane/"))
    {
        println!("{p}");
        let out = a.root.join(&p[8..]);
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(out, d.read(p).unwrap()).unwrap();
    }
    let mgr = AnimMgr::parse(&a.read("bosses/kogane/anims.bin").unwrap()).unwrap();
    println!("base: {}", mgr.base_path);
    for (i, m) in mgr.infos.iter().enumerate() {
        println!(
            "{i}: {} {:?} {:?} {:?}",
            m.name, m.keys, m.info_keys, m.event_keys
        );
    }
}
