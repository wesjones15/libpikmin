//! Decode the user's HUD number textures for inspection. Never commit the output.
//! `cargo run -p libpikmin-assets --example numbers -- assets/number-preview`
use libpikmin_assets::{Assets, Image, screen::number_texture};
use std::{fs, path::PathBuf};

fn tga(path: PathBuf, image: &Image) -> std::io::Result<()> {
    let mut bytes = vec![0u8; 18];
    bytes[2] = 2;
    bytes[12..14].copy_from_slice(&(image.width as u16).to_le_bytes());
    bytes[14..16].copy_from_slice(&(image.height as u16).to_le_bytes());
    bytes[16] = 32;
    bytes[17] = 0x28; // top-left origin, 8 alpha bits
    for p in image.rgba.chunks_exact(4) {
        bytes.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
    }
    fs::write(path, bytes)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let assets = Assets::discover(None).ok_or("no extracted Pikmin assets")?;
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "assets/number-preview".into()));
    fs::create_dir_all(&out)?;
    for digit in 0..10 {
        for shadow in [false, true] {
            let image = number_texture(&assets, digit, shadow)?;
            let name = format!("{digit}{}.tga", if shadow { "-shadow" } else { "" });
            println!("{name}: {}x{}", image.width, image.height);
            tga(out.join(name), &image)?;
        }
    }
    Ok(())
}
