//! The area-name lettering built from Pikmin 2's course-name textures. Skipped when the Pikmin 2 cache
//! (`p2/treasure/titlefont/`) has not been extracted.

use libpikmin_assets::{Assets, title_font::{DIR, TitleFont}};

fn font() -> Option<TitleFont> {
    let assets = Assets::discover(None)?;
    let font = TitleFont::load(&assets.root.join(DIR));
    if font.is_none() {
        println!("SKIP: Pikmin 2 title lettering is not extracted");
    }
    font
}

#[test]
fn every_letter_has_a_glyph_on_one_canvas() {
    let Some(font) = font() else { return };
    for c in ('a'..='z').chain('A'..='Z') {
        let glyph = font.glyph(c).unwrap_or_else(|| panic!("missing {c}"));
        assert_eq!(glyph.height, font.height, "{c}");
        assert!(glyph.width > 4 && glyph.rgba.chunks(4).any(|p| p[3] > 128), "{c} is empty");
    }
}

/// With the floor-name letters every glyph is a real texture, so the capitals are not scaled lowercase.
#[test]
fn capitals_are_the_discs_own() {
    let Some(assets) = Assets::discover(None) else { return };
    let dir = assets.root.join(DIR);
    if !dir.join("al_l_q.bti").is_file() {
        println!("SKIP: cache has no floor-name letters");
        return;
    }
    let font = TitleFont::load(&dir).unwrap();
    let (b, q) = (font.glyph('B').unwrap(), font.glyph('Q').unwrap());
    assert!(b.width > 20 && q.width > 20);
}

/// Dumps a contact sheet to `$TITLE_FONT_DUMP` (raw RGBA, width in the file name) for a look by eye.
#[test]
fn dump_sheet() {
    let (Some(font), Some(path)) = (font(), std::env::var_os("TITLE_FONT_DUMP")) else { return };
    let text = "abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let width = 1400usize;
    let mut sheet = vec![0u8; width * font.height as usize * 4];
    let mut x = 4usize;
    for c in text.chars() {
        let Some(g) = font.glyph(c) else { x += 12; continue };
        for row in 0..g.height as usize {
            for col in 0..g.width as usize {
                if x + col < width {
                    let (from, to) = ((row * g.width as usize + col) * 4, (row * width + x + col) * 4);
                    sheet[to..to + 4].copy_from_slice(&g.rgba[from..from + 4]);
                }
            }
        }
        x += g.width as usize;
    }
    std::fs::write(path, sheet).unwrap();
    println!("sheet {width} x {}", font.height);
}
