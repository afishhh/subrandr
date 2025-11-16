use std::mem::MaybeUninit;

use util::math::I16Dot16;

use super::{Tile, U2Dot14};

pub trait TileRasterizer {
    fn reset(&mut self);

    fn fill_alpha(&self) -> [u8; 4];

    /// # Safety
    ///
    /// `buffer` must be aligned to an 8-byte boundary and its length must be a multiple of 16.
    /// Additionally, if `buffer`'s length is not a multiple of 32, then at least 16 extra byte
    /// must be allocated and writeable after it ends.
    // TODO: ^ reword
    unsafe fn rasterize(
        &mut self,
        strip_x: u16,
        tiles: &[Tile],
        alpha_output: *mut [MaybeUninit<u8>],
    );
}

fn coverage_to_alpha(value: I16Dot16) -> u8 {
    let value = value.unsigned_abs();
    if value >= 1 {
        u8::MAX
    } else {
        let raw = value.into_raw();
        let one = u32::from(u16::MAX) + 1;
        (((raw << 8) - raw + one / 2) >> 16) as u8
    }
}

fn to_op_fixed(v: U2Dot14) -> I16Dot16 {
    I16Dot16::from_raw(i32::from(v.into_raw()) << 2)
}

mod avx2;
pub use avx2::Avx2TileRasterizer;
mod generic;
pub use generic::GenericTileRasterizer;

pub fn init_tile_rasterizer() -> Box<dyn TileRasterizer> {
    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("avx2") {
        return Box::new(unsafe { Avx2TileRasterizer::new() });
    }

    Box::new(GenericTileRasterizer::new())
}

#[cfg(test)]
mod test {
    use util::math::I16Dot16;

    fn reference_coverage_to_alpha(value: I16Dot16) -> u8 {
        let value = value.unsigned_abs();
        if value >= 1 {
            u8::MAX
        } else {
            let one = u64::from(u16::MAX) + 1;
            ((((value.into_raw()) as u64) * u64::from(u8::MAX) + one / 2) / one) as u8
        }
    }

    #[test]
    fn coverage_to_alpha_exhaustive() {
        for r in I16Dot16::new(-2).into_raw()..=I16Dot16::new(2).into_raw() {
            let value = I16Dot16::from_raw(r);
            assert_eq!(
                super::coverage_to_alpha(value),
                reference_coverage_to_alpha(value)
            );
        }

        assert_eq!(super::coverage_to_alpha(I16Dot16::from_f32(0.75)), 191);
    }
}
