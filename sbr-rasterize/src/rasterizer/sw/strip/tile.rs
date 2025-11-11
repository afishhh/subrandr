use std::mem::MaybeUninit;

use util::math::I16Dot16;

use super::{Tile, U2Dot14};

pub trait TileRasterizer {
    fn reset(&mut self);

    fn fill_alpha(&self) -> [u8; 4];

    unsafe fn rasterize(
        &mut self,
        strip_x: u16,
        tiles: &[Tile],
        alpha_output: *mut [MaybeUninit<u8>],
    );
}

fn to_op_fixed(v: U2Dot14) -> I16Dot16 {
    I16Dot16::from_raw(i32::from(v.into_raw()) << 2)
}

mod generic;
pub use generic::GenericTileRasterizer;

pub fn init_tile_rasterizer() -> Box<dyn TileRasterizer> {
    Box::new(GenericTileRasterizer::new())
}
