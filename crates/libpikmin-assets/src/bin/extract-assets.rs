//! Optional manual extraction (hosts normally do this on first run):
//! `cargo run -p libpikmin-assets --bin extract-assets -- "path/to/Pikmin.iso" [out]`
//! `out` defaults to `assets/` at the repository root.

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let Some(iso) = args.next().map(PathBuf::from) else {
        eprintln!("usage: extract-assets <Pikmin .iso/.ciso/.gcm> [out dir]");
        std::process::exit(2);
    };
    let out = args.next().map(PathBuf::from).unwrap_or_else(libpikmin_assets::Assets::default_dir);
    match libpikmin_assets::extract::extract_assets(&iso, &out, &mut |m| println!("{m}")) {
        Ok(r) => println!("assets in {}", r.out.display()),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
