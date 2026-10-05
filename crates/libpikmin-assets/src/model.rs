//! Pikmin 1 `.mod` models.
//!
//! Chunk stream from `BaseShape::read` (sysCommon/shapeBase.cpp): each chunk is
//! `type:u32, length:u32`, then `length` bytes, every chunk starting on a 32-byte boundary.
//! Chunk ids are `BaseShapeChunk` (Shape.h). Element readers: `Joint::read`, `Mesh::read`,
//! `MtxGroup::read`, `DispList::read`, `Envelope::read` (shapeBase.cpp), `VtxMatrix::read`
//! (Shape.h), `TexImg::read`, `TexAttr::read`, `Material::read` (graphics.cpp) and the PVW
//! structures (PVW.h).
//!
//! Geometry lives in GX display lists. The vertex layout comes from `DGXGraphics::initMesh`
//! and `setupVtxDesc` (sysDolphin/dgxGraphics.cpp): an optional position/normal matrix index
//! byte (`Mesh::FeatureFlags::PosAndNrm`), an optional TEX1 matrix index byte, then 16-bit
//! indices for position, normal (always on), colour 0 (if `VtxColor`) and each enabled
//! texcoord set. Matrix index bytes are GX row addresses (slot * 3). A matrix group's
//! dependency list loads joint/envelope matrices into those slots; -1 leaves a slot holding
//! whatever the previous group loaded (`DGXGraphics::drawSingleMatpoly`).

use glam::{Vec2, Vec3};

use crate::Error;
use crate::reader::Reader;
use crate::texture::{self, Image, TexFormat};

mod chunk {
    pub const HEADER: u32 = 0x00;
    pub const VERTEX: u32 = 0x10;
    pub const NORMAL: u32 = 0x11;
    pub const NBT: u32 = 0x12;
    pub const COLOUR: u32 = 0x13;
    pub const TEXCOORD0: u32 = 0x18;
    pub const TEXCOORD7: u32 = 0x1F;
    pub const TEXTURE: u32 = 0x20;
    pub const TEX_ATTR: u32 = 0x22;
    pub const MATERIAL: u32 = 0x30;
    pub const VTX_MATRIX: u32 = 0x40;
    pub const ENVELOPE: u32 = 0x41;
    pub const MESH: u32 = 0x50;
    pub const JOINT: u32 = 0x60;
    pub const JOINT_NAME: u32 = 0x61;
    pub const END: u32 = 0xFFFF;
}

/// `Mesh::FeatureFlags` (Mesh.h).
pub mod mesh_flags {
    pub const POS_NRM_MTX: u32 = 1 << 0;
    pub const TEX1_MTX: u32 = 1 << 1;
    pub const VTX_COLOR: u32 = 1 << 2;
    pub const TEX0: u32 = 1 << 3;
    pub const VTX_NBT: u32 = 1 << 16;
}

/// `MATFLAG_*` (Material.h).
pub mod mat_flags {
    pub const PVW: u32 = 1 << 0;
    pub const OPAQUE: u32 = 1 << 8;
    pub const ALPHA_TEST: u32 = 1 << 9;
    pub const ALPHA_BLEND: u32 = 1 << 10;
    pub const INVERSE_BLEND: u32 = 1 << 15;
    pub const SKIP: u32 = 1 << 16;
}

#[derive(Debug, Clone)]
pub struct TexImage {
    pub width: u32,
    pub height: u32,
    pub format: Option<TexFormat>,
    pub data: Vec<u8>,
}

impl TexImage {
    pub fn decode(&self) -> Result<Image, Error> {
        let format = self
            .format
            .ok_or_else(|| Error::Bad("unknown texture format".into()))?;
        texture::decode(format, self.width, self.height, &self.data)
    }
}

/// `TexAttr`: which image a material samples and how it wraps.
#[derive(Debug, Clone, Copy)]
pub struct TexAttr {
    /// Index into [`Model::textures`]; bit 15 set means an external texture (unsupported).
    pub image: u16,
    /// `Texture::TEX_CLAMP_S` (1), `TEX_MIRROR_S` (2), `TEX_CLAMP_T` (0x100), `TEX_MIRROR_T` (0x200).
    pub tiling: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    Opaque,
    AlphaTest,
    AlphaBlend,
    /// `MATFLAG_InverseColorBlend` (shadows).
    Inverse,
}

/// One colour key. The channel value is stored in the key's small integer field
/// (`PVWKeyInfoU8`/`S10::mTime`); the floats are the in/out tangents in units per frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorKey {
    pub frame: f32,
    pub value: f32,
    pub in_tan: f32,
    pub out_tan: f32,
}

/// `PVWPolygonColourInfo` / `PVWTevColReg` animation: keyed RGB and alpha channels.
#[derive(Debug, Clone, Default)]
pub struct ColorAnim {
    pub frames: u32,
    pub speed: f32,
    pub rgb: [Vec<ColorKey>; 3],
    pub alpha: Vec<ColorKey>,
}

/// `subExtract` (graphics.cpp): Hermite in frame units. One key is constant; outside every
/// key pair the first pair is used, like `PVWColourAnimInfo::extract`.
fn sample_keys(keys: &[ColorKey], frame: f32) -> Option<f32> {
    match keys {
        [] => None,
        [k] => Some(k.value),
        _ => {
            let i = (0..keys.len() - 1)
                .find(|&i| keys[i].frame <= frame && keys[i + 1].frame >= frame)
                .unwrap_or(0);
            let (k0, k1) = (keys[i], keys[i + 1]);
            let span = k1.frame - k0.frame;
            if span <= 0.0 {
                return Some(k0.value);
            }
            let a = frame - k0.frame;
            let s = a / span;
            let (s2, s3) = (s * s, s * s * s);
            Some(
                (2.0 * s3 - 3.0 * s2 + 1.0) * k0.value
                    + (-2.0 * s3 + 3.0 * s2) * k1.value
                    + a * (s2 - 2.0 * s + 1.0) * k0.out_tan
                    + a * (s2 - s) * k1.in_tan,
            )
        }
    }
}

impl ColorAnim {
    /// Samples R, G, B, A at `frame`; channels without keys come back `None`.
    pub fn sample(&self, frame: f32) -> [Option<f32>; 4] {
        [
            sample_keys(&self.rgb[0], frame),
            sample_keys(&self.rgb[1], frame),
            sample_keys(&self.rgb[2], frame),
            sample_keys(&self.alpha, frame),
        ]
    }

    pub fn is_empty(&self) -> bool {
        self.rgb.iter().all(Vec::is_empty) && self.alpha.is_empty()
    }
}

/// `PVWTevColReg` (PVW.h): a TEV colour register with signed 10-bit channels.
#[derive(Debug, Clone, Default)]
pub struct TevColReg {
    pub color: [i16; 4],
    pub anim: ColorAnim,
}

/// The parts of `PVWTevInfo` (PVW.h) that are kept: colour registers 0-2 and konst colours.
#[derive(Debug, Clone, Default)]
pub struct TevInfo {
    pub regs: [TevColReg; 3],
    pub konst: [[u8; 4]; 4],
}

/// What a renderer needs from a `Material`. TEV stages are parsed past but not kept.
#[derive(Debug, Clone)]
pub struct Material {
    pub flags: u32,
    /// Material colour (`GXSetChanMatColor`): `mColourInfo.mColour` for PVW materials,
    /// `colour()` otherwise.
    pub color: [u8; 4],
    /// First texture's [`TexAttr`] index.
    pub tex_attr: Option<usize>,
    /// `PVWLightingInfo::mCtrlFlag`; bit 0 = lit, bit 11 = vertex colour as material colour.
    pub lighting: u32,
    /// Keyed animation of `color` (`ShapeDynMaterials::animate`).
    pub color_anim: ColorAnim,
    /// Index into [`Model::tev_infos`] for PVW materials.
    pub tev_info: Option<usize>,
}

impl Material {
    pub fn blend(&self) -> Blend {
        use mat_flags::*;
        if self.flags & INVERSE_BLEND != 0 {
            Blend::Inverse
        } else if self.flags & OPAQUE != 0 {
            Blend::Opaque
        } else if self.flags & ALPHA_TEST != 0 {
            Blend::AlphaTest
        } else if self.flags & ALPHA_BLEND != 0 {
            Blend::AlphaBlend
        } else {
            Blend::Opaque
        }
    }

    pub fn lit(&self) -> bool {
        self.flags & mat_flags::PVW == 0 || self.lighting & 1 != 0
    }

    pub fn vertex_color(&self) -> bool {
        self.flags & mat_flags::PVW != 0 && self.lighting & (1 << 11) != 0
    }
}

/// `VtxMatrix`: a vertex matrix is either one joint (vertices in that joint's space) or a
/// weighted envelope (vertices in model space).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skin {
    Joint(u16),
    Envelope(u16),
    /// No matrix was loaded; use the model matrix.
    Root,
}

#[derive(Debug, Clone)]
pub struct Envelope {
    pub weights: Vec<(u16, f32)>,
}

#[derive(Debug, Clone)]
pub struct DispList {
    pub flags: u32,
    pub face_count: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct MtxGroup {
    pub deps: Vec<i16>,
    pub lists: Vec<DispList>,
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub parent_joint: i32,
    pub flags: u32,
    pub groups: Vec<MtxGroup>,
}

#[derive(Debug, Clone)]
pub struct Joint {
    pub parent: i32,
    pub flags: u32,
    pub scale: Vec3,
    /// Euler XYZ in radians, applied as Rz * Ry * Rx (`Matrix4f::makeSRT`).
    pub rotation: Vec3,
    pub translation: Vec3,
    /// (material, mesh) pairs drawn with this joint.
    pub matpolys: Vec<(u16, u16)>,
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub flags: u32,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// Normal, binormal, tangent.
    pub nbt: Vec<[Vec3; 3]>,
    pub colors: Vec<[u8; 4]>,
    pub texcoords: [Vec<Vec2>; 8],
    pub textures: Vec<TexImage>,
    pub tex_attrs: Vec<TexAttr>,
    pub materials: Vec<Material>,
    pub tev_infos: Vec<TevInfo>,
    pub vtx_matrices: Vec<Skin>,
    pub envelopes: Vec<Envelope>,
    pub meshes: Vec<Mesh>,
    pub joints: Vec<Joint>,
    pub joint_names: Vec<String>,
}

fn read_colour(r: &mut Reader) -> Result<[u8; 4], Error> {
    Ok([r.u8()?, r.u8()?, r.u8()?, r.u8()?])
}

/// `PVWAnimInfo1/3<T>` with a key of `key_bytes`: `count`, then `count * (4 + n * key_bytes)`.
fn skip_anim_info(r: &mut Reader, channels: usize, key_bytes: usize) -> Result<(), Error> {
    let n = r.count()?;
    r.skip(n * (4 + channels * key_bytes))
}

/// One channel of a `PVWAnimKey1/3`: `PVWKeyInfoU8` (u8 + 3 pad) or `PVWKeyInfoS10`
/// (i16 + 2 pad), then the in and out tangents.
fn read_key_channel(r: &mut Reader, frame: f32, s10: bool) -> Result<ColorKey, Error> {
    let value = if s10 { r.i16()? as f32 } else { r.u8()? as f32 };
    r.skip(if s10 { 2 } else { 3 })?;
    Ok(ColorKey {
        frame,
        value,
        in_tan: r.f32()?,
        out_tan: r.f32()?,
    })
}

/// Frame count, speed, then `PVWAnimInfo3` (RGB) and `PVWAnimInfo1` (alpha) keys.
fn read_color_anim(r: &mut Reader, s10: bool) -> Result<ColorAnim, Error> {
    let mut anim = ColorAnim {
        frames: r.u32()?,
        speed: r.f32()?,
        ..Default::default()
    };
    for _ in 0..r.count()? {
        let frame = r.u32()? as f32;
        for ch in &mut anim.rgb {
            ch.push(read_key_channel(r, frame, s10)?);
        }
    }
    for _ in 0..r.count()? {
        let frame = r.u32()? as f32;
        anim.alpha.push(read_key_channel(r, frame, s10)?);
    }
    Ok(anim)
}

/// `PVWTevInfo::read` (PVW.h). TEV stages are skipped.
fn read_tev_info(r: &mut Reader) -> Result<TevInfo, Error> {
    let mut info = TevInfo::default();
    for reg in &mut info.regs {
        reg.color = [r.i16()?, r.i16()?, r.i16()?, r.i16()?];
        reg.anim = read_color_anim(r, true)?;
    }
    for k in &mut info.konst {
        *k = read_colour(r)?;
    }
    let stages = r.count()?;
    r.skip(stages * (8 + 12 + 12))?;
    Ok(info)
}

/// `Material::read` (graphics.cpp).
fn read_material(r: &mut Reader) -> Result<Material, Error> {
    let flags = r.u32()?;
    let texture_index = r.i32()?;
    let mut color = read_colour(r)?;
    let mut tex_attr = (texture_index >= 0).then_some(texture_index as usize);
    let mut lighting = 1;
    let mut color_anim = ColorAnim::default();
    let mut tev_info = None;
    if flags & mat_flags::PVW != 0 {
        tev_info = usize::try_from(r.i32()?).ok();
        // PVWPolygonColourInfo
        color = read_colour(r)?;
        color_anim = read_color_anim(r, false)?;
        // PVWLightingInfo
        lighting = r.u32()?;
        r.skip(4)?;
        // PVWPeInfo
        r.skip(16)?;
        // PVWTextureInfo
        r.skip(4 + 12)?; // useScale, scale
        let gens = r.count()?;
        r.skip(gens * 4)?;
        let datas = r.count()?;
        tex_attr = None;
        for i in 0..datas {
            // PVWTextureData::read
            let src = r.u32()?;
            if i == 0 {
                tex_attr = Some(src as usize);
            }
            r.skip(2 + 2 + 4 + 4 + 4 + 4 + 7 * 4)?;
            for _ in 0..3 {
                skip_anim_info(r, 3, 12)?;
            }
        }
    }
    Ok(Material {
        flags,
        color,
        tex_attr,
        lighting,
        color_anim,
        tev_info,
    })
}

impl Model {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut m = Model::default();
        let mut r = Reader::new(data);
        loop {
            let start = r.pos();
            let kind = r.u32()?;
            let len = r.u32()? as usize;
            let end = start + 8 + len;
            match kind {
                chunk::HEADER => {
                    r.align(0x20);
                    r.skip(4)?;
                    m.flags = r.u32()?;
                }
                chunk::VERTEX => {
                    let n = r.count()?;
                    r.align(0x20);
                    m.positions = (0..n).map(|_| r.vec3()).collect::<Result<_, _>>()?;
                }
                chunk::NORMAL => {
                    let n = r.count()?;
                    r.align(0x20);
                    m.normals = (0..n).map(|_| r.vec3()).collect::<Result<_, _>>()?;
                }
                chunk::NBT => {
                    let n = r.count()?;
                    r.align(0x20);
                    m.nbt = (0..n)
                        .map(|_| Ok([r.vec3()?, r.vec3()?, r.vec3()?]))
                        .collect::<Result<_, Error>>()?;
                }
                chunk::COLOUR => {
                    let n = r.count()?;
                    r.align(0x20);
                    m.colors = (0..n).map(|_| read_colour(&mut r)).collect::<Result<_, _>>()?;
                }
                chunk::TEXCOORD0..=chunk::TEXCOORD7 => {
                    let n = r.count()?;
                    r.align(0x20);
                    m.texcoords[(kind - chunk::TEXCOORD0) as usize] =
                        (0..n).map(|_| r.vec2()).collect::<Result<_, _>>()?;
                }
                chunk::TEXTURE => {
                    let n = r.count()?;
                    r.align(0x20);
                    for _ in 0..n {
                        let width = r.u16()? as u32;
                        let height = r.u16()? as u32;
                        let format = TexFormat::from_tex_img(r.u32()?);
                        r.skip(4 + 16)?; // image count, 4 unused
                        let size = r.count()?;
                        let data = r.take(size)?.to_vec();
                        m.textures.push(TexImage {
                            width,
                            height,
                            format,
                            data,
                        });
                    }
                }
                chunk::TEX_ATTR => {
                    let n = r.count()?;
                    r.align(0x20);
                    for _ in 0..n {
                        let image = r.u16()?;
                        r.skip(2)?;
                        let tiling = r.u16()?;
                        r.skip(2 + 4)?;
                        m.tex_attrs.push(TexAttr { image, tiling });
                    }
                }
                chunk::MATERIAL => {
                    let n = r.count()?;
                    let tevs = r.count()?;
                    r.align(0x20);
                    for _ in 0..tevs {
                        m.tev_infos.push(read_tev_info(&mut r)?);
                    }
                    for _ in 0..n {
                        m.materials.push(read_material(&mut r)?);
                    }
                }
                chunk::VTX_MATRIX => {
                    let n = r.count()?;
                    r.align(0x20);
                    for _ in 0..n {
                        let v = r.i16()?;
                        m.vtx_matrices.push(if v >= 0 {
                            Skin::Joint(v as u16)
                        } else {
                            Skin::Envelope((-(v as i32) - 1) as u16)
                        });
                    }
                }
                chunk::ENVELOPE => {
                    let n = r.count()?;
                    r.align(0x20);
                    for _ in 0..n {
                        let k = r.u16()? as usize;
                        let weights = (0..k)
                            .map(|_| Ok((r.u16()?, r.f32()?)))
                            .collect::<Result<_, Error>>()?;
                        m.envelopes.push(Envelope { weights });
                    }
                }
                chunk::MESH => {
                    let n = r.count()?;
                    r.align(0x20);
                    for _ in 0..n {
                        let parent_joint = r.i32()?;
                        let flags = r.u32()?;
                        let groups = r.count()?;
                        let mut mesh = Mesh {
                            parent_joint,
                            flags,
                            groups: Vec::with_capacity(groups),
                        };
                        for _ in 0..groups {
                            let nd = r.count()?;
                            let deps = (0..nd).map(|_| r.i16()).collect::<Result<_, _>>()?;
                            let nl = r.count()?;
                            let mut lists = Vec::with_capacity(nl);
                            for _ in 0..nl {
                                let flags = r.u32()?;
                                let face_count = r.u32()?;
                                let size = r.count()?;
                                r.align(0x20);
                                lists.push(DispList {
                                    flags,
                                    face_count,
                                    data: r.take(size)?.to_vec(),
                                });
                            }
                            mesh.groups.push(MtxGroup { deps, lists });
                        }
                        m.meshes.push(mesh);
                    }
                }
                chunk::JOINT => {
                    let n = r.count()?;
                    r.align(0x20);
                    for _ in 0..n {
                        let parent = r.i32()?;
                        let flags = r.u32()?;
                        r.skip(12 + 12 + 4)?; // bounds, unused float
                        let scale = r.vec3()?;
                        let rotation = r.vec3()?;
                        let translation = r.vec3()?;
                        let k = r.count()?;
                        let matpolys = (0..k)
                            .map(|_| Ok((r.u16()?, r.u16()?)))
                            .collect::<Result<_, Error>>()?;
                        m.joints.push(Joint {
                            parent,
                            flags,
                            scale,
                            rotation,
                            translation,
                            matpolys,
                        });
                    }
                }
                chunk::JOINT_NAME => {
                    let n = r.count()?;
                    r.align(0x20);
                    m.joint_names = (0..n).map(|_| r.string()).collect::<Result<_, _>>()?;
                }
                chunk::END => break,
                _ => {}
            }
            if end > data.len() {
                return Err(Error::Truncated(end));
            }
            r.seek(end);
        }
        Ok(m)
    }

    pub fn joint_index(&self, name: &str) -> Option<usize> {
        self.joint_names.iter().position(|n| n == name)
    }

    /// Decodes every display list into triangle batches, one per (joint, material, mesh)
    /// entry, ordered opaque → alpha test → blended like `BaseShape::recAddMatpoly` and
    /// `DGXGraphics::drawMeshes`.
    pub fn batches(&self) -> Result<Vec<Batch>, Error> {
        let mut out = Vec::new();
        for joint in &self.joints {
            for &(mat, mesh) in &joint.matpolys {
                let (Some(material), Some(m)) =
                    (self.materials.get(mat as usize), self.meshes.get(mesh as usize))
                else {
                    continue;
                };
                if material.flags & mat_flags::SKIP != 0 {
                    continue;
                }
                let mut batch = Batch {
                    material: mat as usize,
                    vertices: Vec::new(),
                    indices: Vec::new(),
                };
                self.decode_mesh(m, material, &mut batch)?;
                if !batch.indices.is_empty() {
                    out.push(batch);
                }
            }
        }
        let rank = |b: &Batch| match self.materials[b.material].blend() {
            Blend::Opaque => 0,
            Blend::AlphaTest => 1,
            Blend::AlphaBlend | Blend::Inverse => 2,
        };
        out.sort_by_key(rank);
        Ok(out)
    }

    fn decode_mesh(&self, mesh: &Mesh, material: &Material, batch: &mut Batch) -> Result<(), Error> {
        use mesh_flags::*;
        let use_nbt = mesh.flags & VTX_NBT != 0;
        let mut slots = [Skin::Root; 10];
        for group in &mesh.groups {
            for (slot, &dep) in group.deps.iter().enumerate().take(10) {
                if dep >= 0 {
                    slots[slot] = self
                        .vtx_matrices
                        .get(dep as usize)
                        .copied()
                        .unwrap_or(Skin::Root);
                }
            }
            for list in &group.lists {
                let mut r = Reader::new(&list.data);
                while r.remaining() > 0 {
                    let cmd = r.u8()?;
                    match cmd {
                        0x00 => continue, // NOP / padding
                        0x08 => r.skip(5)?,
                        0x10 => {
                            let n = (r.u16()? as usize & 0xF) + 1;
                            r.skip(2 + n * 4)?;
                        }
                        0x20 | 0x28 | 0x30 | 0x38 => r.skip(4)?,
                        0x40 => r.skip(8)?,
                        0x48 => {}
                        0x61 => r.skip(4)?,
                        0x80..=0xBF => {
                            let prim = cmd & 0xF8;
                            let count = r.u16()? as usize;
                            let first = batch.vertices.len() as u32;
                            for _ in 0..count {
                                let slot = if mesh.flags & POS_NRM_MTX != 0 {
                                    (r.u8()? / 3) as usize
                                } else {
                                    0
                                };
                                if mesh.flags & TEX1_MTX != 0 {
                                    r.skip(1)?;
                                }
                                let pos = r.u16()? as usize;
                                let nrm = r.u16()? as usize;
                                let col = if mesh.flags & VTX_COLOR != 0 {
                                    Some(r.u16()? as usize)
                                } else {
                                    None
                                };
                                let mut uv = Vec2::ZERO;
                                for set in 0..8 {
                                    if mesh.flags & (TEX0 << set) != 0 {
                                        let i = r.u16()? as usize;
                                        if set == 0 {
                                            uv = self.texcoords[0].get(i).copied().unwrap_or_default();
                                        }
                                    }
                                }
                                let normal = if use_nbt {
                                    self.nbt.get(nrm).map(|n| n[0])
                                } else {
                                    self.normals.get(nrm).copied()
                                };
                                let color = match col {
                                    Some(i) if material.vertex_color() => {
                                        self.colors.get(i).copied().unwrap_or(material.color)
                                    }
                                    _ => material.color,
                                };
                                batch.vertices.push(BatchVertex {
                                    position: self.positions.get(pos).copied().unwrap_or_default(),
                                    normal: normal.unwrap_or(Vec3::Y),
                                    uv,
                                    color,
                                    skin: slots.get(slot).copied().unwrap_or(Skin::Root),
                                });
                            }
                            let n = count as u32;
                            let idx = &mut batch.indices;
                            match prim {
                                0x90 => idx.extend((0..n / 3 * 3).map(|i| first + i)),
                                0x98 => {
                                    for i in 0..n.saturating_sub(2) {
                                        let (a, b) = if i % 2 == 0 { (i, i + 1) } else { (i + 1, i) };
                                        idx.extend([first + a, first + b, first + i + 2]);
                                    }
                                }
                                0xA0 => {
                                    for i in 1..n.saturating_sub(1) {
                                        idx.extend([first, first + i, first + i + 1]);
                                    }
                                }
                                0x80 => {
                                    for q in 0..n / 4 {
                                        let b = first + q * 4;
                                        idx.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
                                    }
                                }
                                _ => {} // lines and points
                            }
                        }
                        _ => {
                            return Err(Error::Bad(format!(
                                "display list command {cmd:#04x} at {:#x}",
                                r.pos() - 1
                            )));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BatchVertex {
    /// In the space of `skin`: joint space for [`Skin::Joint`], model space otherwise.
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: Vec2,
    /// Material colour, or the vertex colour when the material asks for it.
    pub color: [u8; 4],
    pub skin: Skin,
}

#[derive(Debug, Clone)]
pub struct Batch {
    pub material: usize,
    pub vertices: Vec<BatchVertex>,
    pub indices: Vec<u32>,
}
