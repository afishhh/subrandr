use std::mem::MaybeUninit;

use util::math::{I16Dot16, Point2};

use super::{to_op_fixed, Tile};

fn coverage_to_alpha(value: I16Dot16) -> u8 {
    let value = value.unsigned_abs();
    if value >= 1 {
        u8::MAX
    } else {
        let raw = value.into_raw();
        let half = 1 << 15;
        (((raw << 8) - raw + half) >> 16) as u8
    }
}

pub struct GenericTileRasterizer {
    coverage_scratch_buffer: Vec<I16Dot16>,
    current_winding: [I16Dot16; 4],
}

impl GenericTileRasterizer {
    pub fn new() -> Self {
        Self {
            coverage_scratch_buffer: Vec::new(),
            current_winding: [I16Dot16::ZERO; 4],
        }
    }
}

impl super::TileRasterizer for GenericTileRasterizer {
    fn reset(&mut self) {
        self.current_winding = [I16Dot16::ZERO; 4]
    }

    fn fill_alpha(&self) -> [u8; 4] {
        self.current_winding.map(coverage_to_alpha)
    }

    unsafe fn rasterize(
        &mut self,
        strip_x: u16,
        tiles: &[Tile],
        alpha_output: *mut [MaybeUninit<u8>],
    ) {
        let width = alpha_output.len() / 4;

        {
            self.coverage_scratch_buffer.clear();
            self.coverage_scratch_buffer
                .reserve_exact(alpha_output.len());
            let mut row = self.coverage_scratch_buffer.spare_capacity_mut();
            for winding in self.current_winding {
                row[..width].fill(MaybeUninit::new(winding));
                row = &mut row[width..];
            }
            self.coverage_scratch_buffer.set_len(alpha_output.len());
        }

        for tile in tiles {
            self.rasterize_tile(
                width,
                I16Dot16::new(4 * i32::from(tile.pos.x - strip_x)),
                tile,
            );
        }

        for (&a, b) in std::iter::zip(self.coverage_scratch_buffer.iter(), &mut *alpha_output) {
            b.write(coverage_to_alpha(a));
        }
    }
}

impl GenericTileRasterizer {
    fn rasterize_tile(&mut self, width: usize, x: I16Dot16, tile: &Tile) {
        if tile.line.bottom_y == tile.line.top_y {
            return;
        }

        let top = Point2::new(tile.line.top_x + x, to_op_fixed(tile.line.top_y));
        let bottom = Point2::new(tile.line.bottom_x + x, to_op_fixed(tile.line.bottom_y));

        let sign = tile.winding as i32;
        let dx = (top.x - bottom.x) / (top.y - bottom.y);
        let end_row = tile.line.top_y.floor_to_inner();
        let mut current_row = tile.line.bottom_y.floor_to_inner();
        let mut current_x = bottom.x;

        if end_row == current_row {
            let h = top.y - bottom.y;
            self.current_winding[usize::from(current_row)] += h * sign;
            self.rasterize_row(current_row, width, top.x, bottom.x, h * sign);
        } else {
            let initial_height = I16Dot16::ONE - bottom.y.fract();
            let next_x = current_x + dx * initial_height;
            self.current_winding[usize::from(current_row)] += initial_height * sign;
            self.rasterize_row(current_row, width, next_x, current_x, initial_height * sign);
            current_row += 1;
            current_x = next_x;

            while current_row < end_row {
                let next_x = current_x + dx;
                self.current_winding[usize::from(current_row)] += sign;
                self.rasterize_row(current_row, width, next_x, current_x, I16Dot16::ONE * sign);
                current_row += 1;
                current_x = next_x;
            }

            let final_height = top.y - I16Dot16::new(current_row.into());
            self.current_winding[usize::from(end_row)] += final_height * sign;
            self.rasterize_row(end_row, width, top.x, current_x, final_height * sign);
        }
    }

    fn rasterize_row(
        &mut self,
        y: u16,
        width: usize,
        tx: I16Dot16,
        bx: I16Dot16,
        signed_height: I16Dot16,
    ) {
        let row = &mut self.coverage_scratch_buffer[usize::from(y) * width..][..width];
        let (lx, rx) = if bx < tx { (bx, tx) } else { (tx, bx) };
        let mut current_xi = lx.floor_to_inner() as usize;
        let mut current_x = lx.floor();
        let end_x = rx.ceil() - 1;

        unsafe { std::hint::assert_unchecked(current_xi < width) };

        if current_x >= end_x {
            row[current_xi] += Self::cover_pixel(lx, rx, signed_height, I16Dot16::ZERO);
            row[current_xi] += signed_height * (I16Dot16::ONE - (rx - current_x));
            current_xi += 1;
        } else {
            unsafe { std::hint::assert_unchecked(rx != lx) };
            let dy = signed_height / (rx - lx);
            let mut current_y =
                dy * (I16Dot16::ONE - if bx < tx { bx.fract() } else { tx.fract() });
            let mut next_x = current_x + 1;
            row[current_xi] += Self::cover_pixel(lx, next_x, current_y, I16Dot16::ZERO);
            current_x = next_x;
            current_xi += 1;

            while current_x < end_x {
                next_x = current_x + 1;
                unsafe { std::hint::assert_unchecked(current_xi < width) };
                row[current_xi] += Self::cover_pixel(current_x, next_x, dy, current_y);
                current_x = next_x;
                current_xi += 1;
                current_y += dy;
            }

            debug_assert_eq!(current_x, end_x);
            unsafe { std::hint::assert_unchecked(current_xi < width) };
            row[current_xi] +=
                Self::cover_pixel(current_x, rx, signed_height - current_y, current_y);
            row[current_xi] += signed_height * (I16Dot16::ONE - (rx - current_x));
            current_xi += 1;
        }

        unsafe { std::hint::assert_unchecked(current_xi <= width) };
        for pixel in &mut row[current_xi..width] {
            *pixel += signed_height;
        }
    }

    #[inline]
    fn cover_pixel(
        lx: I16Dot16,
        rx: I16Dot16,
        triangle_height: I16Dot16,
        rect_height: I16Dot16,
    ) -> I16Dot16 {
        (rx - lx) * (triangle_height / 2 + rect_height)
    }
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
