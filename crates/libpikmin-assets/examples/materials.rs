//! Prints the TEV programs of a model's materials, for checking the viewer's TEV emulation.
//! `cargo run -p libpikmin-assets --example materials -- pikis/nv3Model.mod 6 7`
use libpikmin_assets::Assets;
fn main() {
    let a = Assets::discover(None).unwrap();
    let mut args = std::env::args().skip(1);
    let p = args.next().unwrap();
    let m = a.model(&p).unwrap();
    for mi in args.map(|x| x.parse::<usize>().unwrap()) {
        let mat = &m.materials[mi];
        println!("mat {mi} flags {:#x} color {:?} light {:#x} gens {:?}", mat.flags, mat.color, mat.lighting, mat.tex_gens);
        if let Some(t) = mat.tev_info.map(|t| &m.tev_infos[t]) {
            println!("  konst {:?} regs {:?}", t.konst, t.regs.iter().map(|r| r.color).collect::<Vec<_>>());
            for s in &t.stages { println!("  map {} coord {} ch {} k {} {} | C {} {} {} {} op {} b {} s {} cl {} out {} | A {} {} {} {} op {} cl {} out {}", s.tex_map, s.tex_coord, s.channel, s.kcolor_sel, s.kalpha_sel, s.color.a, s.color.b, s.color.c, s.color.d, s.color.op, s.color.bias, s.color.scale, s.color.clamp, s.color.out, s.alpha.a, s.alpha.b, s.alpha.c, s.alpha.d, s.alpha.op, s.alpha.clamp, s.alpha.out); }
        }
    }
}
