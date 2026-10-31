//! BossProp::read / KoganeProp::read: three consecutive tagged Parameters blocks.
use crate::{
    Error,
    anim::{ParmValue, read_params},
    reader::Reader,
};
pub fn kogane_params(bytes: &[u8]) -> Result<std::collections::HashMap<[u8; 3], f32>, Error> {
    let mut r = Reader::new(bytes);
    let mut out = std::collections::HashMap::new();
    for _ in 0..3 {
        read_params(&mut r, &[], |id, v| {
            if let ParmValue::Word(Some(v)) = v {
                out.insert(*id, f32::from_bits(v));
            }
        })?;
    }
    Ok(out)
}
