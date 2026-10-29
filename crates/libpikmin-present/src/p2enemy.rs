//! Pikmin 2's enemies for hosts that fight them (`libpikmin_teki::p2`): each kind's J3D body from the cache
//! `libsms::extract::p2_enemy` writes (`<assets>/p2/enemy/`), posed on its `.bca` clips by the motion index and frame
//! the host names in [`crate::teki::TekiInstance`], like the Pikmin 1 enemies.
//!
//! The model is drawn at the enemy's own scale (`mScale`, `TekiInfo::scale`) times the instance's: the Kogane family
//! grows from nothing as it pops up, which the enemy reports as its size share.

use std::path::Path;
use std::sync::Arc;

use libpikmin_teki::info::TekiInfo;
use libpikmin_teki::p2;
use libsms_present::enemy::EnemyBody;

use libpikmin_assets::model::Batch;

use crate::tev;

/// One loaded Pikmin 2 kind.
pub struct P2Teki {
    pub info: TekiInfo,
    pub body: EnemyBody,
    /// The model's index in [`crate::view::View::models`].
    pub model: u32,
    /// Where this body's textures start in [`crate::view::View::textures`].
    pub tex_base: i32,
}

impl P2Teki {
    /// The clip a motion index names (`enemyAnimMgr.txt` order).
    pub fn clip(&self, motion: u32) -> Option<&str> {
        self.info.clips.get(motion as usize).map(String::as_str)
    }

    /// The model-table entry's batches (bind pose, at the enemy's scale).
    pub fn batches(&self) -> Vec<Batch> {
        crate::pianta::batches_of(&self.body.model, &self.body.bind_world(), self.info.scale)
    }

    pub fn textures(&self) -> Vec<tev::Baked> {
        crate::pianta::textures_of(&self.body.textures)
    }
}

/// The body of Pikmin 2 kind `kind` from the cache `dir`: its model and clips, with the variant's texture.
pub fn load_body(dir: &Path, kind: u8) -> Result<EnemyBody, String> {
    let k = p2::kind(kind).ok_or("not a Pikmin 2 kind")?;
    let base = format!("enemy/data/{}", k.data);
    let textures: Vec<String> = k.textures.iter().map(|t| format!("enemy/data/{t}")).collect();
    let textures: Vec<&str> = textures.iter().map(String::as_str).collect();
    EnemyBody::load_swapped(dir, &format!("{base}/model/enemy.bmd"), &format!("{base}/anim"), &textures)
}

/// What the disc says about `kind` with the body's real bounds (the parm files give only a collision sphere), at the
/// enemy's own scale.
pub fn load_info(dir: &Path, kind: u8) -> Result<TekiInfo, String> {
    let mut info = TekiInfo::load_p2(dir, kind)?;
    fit_bounds(&mut info, &load_body(dir, kind)?);
    Ok(info)
}

fn fit_bounds(info: &mut TekiInfo, body: &EnemyBody) {
    info.lo = body.model.lo * info.scale;
    info.hi = body.model.hi * info.scale;
}

/// Loads every Pikmin 2 kind the cache `dir` (the folder holding `enemy/`) has files for; `first_model` and
/// `first_tex` are where they will sit in the view's tables, in kind order.
pub fn load(dir: &Path, first_model: u32, first_tex: i32) -> Vec<Arc<P2Teki>> {
    let mut out: Vec<Arc<P2Teki>> = Vec::new();
    let mut tex = first_tex;
    for kind in p2::FIRST..p2::FIRST + p2::KIND_TABLE.len() as u8 {
        let Some(k) = p2::kind(kind) else { continue };
        let loaded = (|| {
            let mut info = TekiInfo::load_p2(dir, kind)?;
            info.model = first_model + out.len() as u32;
            let body = load_body(dir, kind)?;
            fit_bounds(&mut info, &body);
            Ok::<_, String>(P2Teki { info, body, model: first_model + out.len() as u32, tex_base: tex })
        })();
        match loaded {
            Ok(t) => {
                tex += t.body.textures.list.len() as i32;
                out.push(Arc::new(t));
            }
            Err(e) => eprintln!("Pikmin 2 enemy {}: {e}", k.name),
        }
    }
    out
}
