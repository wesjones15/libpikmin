//! Prints anim and info keys for AnimMgr entries. `cargo run -p libpikmin-assets --example keys -- pikis/animMgr.bin 67 39`
fn main() {
    let a = libpikmin_assets::Assets::discover(None).unwrap();
    let mut args = std::env::args().skip(1);
    let mgr = libpikmin_assets::AnimMgr::parse(&a.read(&args.next().unwrap()).unwrap()).unwrap();
    for i in args.map(|x| x.parse::<usize>().unwrap()) {
        let info = &mgr.infos[i];
        println!("{i} {} keys {:?} info (key idx, type) {:?}", info.name, info.keys, info.info_keys);
    }
}
