//! Prints what the loaders find on the user's disc. `cargo run -p libpikmin-assets --example dump`
use libpikmin_assets::{anim, paths, AnimSet, Disc, Model};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = libpikmin_assets::find_disc().ok_or("no disc found; set PIKMIN_DISC")?;
    let disc = Disc::open(&path)?;
    println!("disc {} ({})", disc.game_id, path.display());
    let mut models = vec![paths::OLIMAR, paths::ONION];
    models.extend(paths::PIKMIN);
    models.extend(paths::HAPPA);
    models.extend(paths::PELLETS);
    for p in models {
        let m = Model::parse(&disc.read(p)?)?;
        let batches = m.batches()?;
        let tris: usize = batches.iter().map(|b| b.indices.len() / 3).sum();
        println!(
            "{p}: {} verts {} joints {} envelopes {} mats {} tex {} batches {} tris",
            m.positions.len(), m.joints.len(), m.envelopes.len(), m.materials.len(), m.textures.len(), batches.len(), tris
        );
        if std::env::args().any(|a| a == "-v") {
            for (i, (j, n)) in m.joints.iter().zip(m.joint_names.iter().chain(std::iter::repeat(&String::new()))).enumerate() {
                println!("  joint {i} {n:?} parent {} t {:?}", j.parent, j.translation);
            }
            for (i, mat) in m.materials.iter().enumerate() {
                println!("  mat {i} flags {:#x} color {:?} tex {:?} light {:#x}", mat.flags, mat.color, mat.tex_attr, mat.lighting);
            }
            for t in &m.textures {
                println!("  tex {}x{} {:?} {} bytes ok={}", t.width, t.height, t.format, t.data.len(), t.decode().is_ok());
            }
        }
    }
    let set = AnimSet::load(&disc, paths::PIKI_ANIM_MGR, paths::PIKI_BUNDLE)?;
    println!("animMgr base {:?}, {} infos", set.mgr.base_path, set.mgr.infos.len());
    let bundle = anim::parse_bundle(&disc.read(paths::PIKI_BUNDLE)?)?;
    println!("bundle {} anims, first {:?}", bundle.len(), bundle.first().map(|a| (&a.name, a.joints.len(), a.frames)));
    for (i, info) in set.mgr.infos.iter().enumerate() {
        let a = set.anims[i].as_ref();
        println!("  {i:3} {:20} spd {:4} keys {:?} -> {:?}", info.name, info.speed, info.keys, a.map(|a| (a.kind, a.frames, a.joints.len())));
    }
    let goal = AnimSet::load(&disc, paths::ONION_ANIM_MGR, paths::ONION_BUNDLE)?;
    for (i, info) in goal.mgr.infos.iter().enumerate() {
        println!("  goal {i} {} -> {:?}", info.name, goal.anims[i].as_ref().map(|a| (a.kind, a.frames, a.joints.len())));
    }
    Ok(())
}
