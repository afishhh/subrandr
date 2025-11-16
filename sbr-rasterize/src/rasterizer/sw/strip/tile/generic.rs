use util::math::{I16Dot16, Point2};

use super::{coverage_to_alpha, to_op_fixed, Tile};

pub struct GenericTileRasterizer {
    coverage_scratch_buffer: Vec<I16Dot16>,
}

impl GenericTileRasterizer {
    pub fn new() -> Self {
        Self {
            coverage_scratch_buffer: Vec::new(),
        }
    }
}

impl super::TileRasterizer for GenericTileRasterizer {
    unsafe fn rasterize(
        &mut self,
        strip_x: u16,
        tiles: &[Tile],
        initial_winding: I16Dot16,
        buffer: &mut [u8],
    ) {
        let width = buffer.len() / 4;
        self.coverage_scratch_buffer.clear();
        self.coverage_scratch_buffer
            .resize(buffer.len(), initial_winding);

        for tile in tiles {
            self.rasterize_tile(
                width,
                I16Dot16::new(4 * i32::from(tile.pos.x - strip_x)),
                tile,
            );
        }

        for (&a, b) in std::iter::zip(self.coverage_scratch_buffer.iter(), buffer.iter_mut()) {
            *b = coverage_to_alpha(a);
        }

        // for y in (0..4).rev() {
        //     for x in 0..width {
        //         eprint!("{:02X} ", buffer[y * width + x]);
        //     }
        //     eprintln!();
        // }
    }
}

impl GenericTileRasterizer {
    fn rasterize_tile(&mut self, width: usize, x: I16Dot16, tile: &Tile) {
        if tile.line.bottom_y == tile.line.top_y {
            return;
        }

        let top = Point2::new(tile.line.top_x + x, to_op_fixed(tile.line.top_y));
        let bottom = Point2::new(tile.line.bottom_x + x, to_op_fixed(tile.line.bottom_y));

        let sign = I16Dot16::new(tile.winding as i32);
        let dx = (top.x - bottom.x) / (top.y - bottom.y);
        let end_row = tile.line.top_y.floor_to_inner();
        let mut current_row = tile.line.bottom_y.floor_to_inner();
        let mut current_x = bottom.x;

        if end_row == current_row {
            self.rasterize_row(current_row, width, top.x, bottom.x, top.y - bottom.y, sign);
        } else {
            let initial_height = I16Dot16::ONE - bottom.y.fract();
            let next_x = current_x + dx * initial_height;
            self.rasterize_row(current_row, width, next_x, current_x, initial_height, sign);
            current_row += 1;
            current_x = next_x;

            while current_row < end_row {
                let next_x = current_x + dx;
                self.rasterize_row(current_row, width, next_x, current_x, I16Dot16::ONE, sign);
                current_row += 1;
                current_x = next_x;
            }

            self.rasterize_row(
                current_row,
                width,
                top.x,
                current_x,
                top.y - I16Dot16::new(current_row.into()),
                sign,
            );
        }
    }

    fn rasterize_row(
        &mut self,
        y: u16,
        width: usize,
        tx: I16Dot16,
        bx: I16Dot16,
        height: I16Dot16,
        sign: I16Dot16,
    ) {
        let row = &mut self.coverage_scratch_buffer[usize::from(y) * width..][..width];
        let (lx, rx) = if bx < tx { (bx, tx) } else { (tx, bx) };
        let mut current_xi = lx.floor_to_inner() as usize;
        let mut current_x = lx.floor();
        let end_x = rx.ceil() - 1;

        if current_x >= end_x {
            row[current_xi] += Self::cover_pixel(lx, rx, height, I16Dot16::ZERO) * sign;
            row[current_xi] += height * (I16Dot16::ONE - (rx - current_x)) * sign;
            current_xi += 1;
        } else {
            let dy = height / (rx - lx);
            let mut current_y =
                dy * (I16Dot16::ONE - if bx < tx { bx.fract() } else { tx.fract() });
            let mut next_x = current_x + 1;
            row[current_xi] += Self::cover_pixel(lx, next_x, current_y, I16Dot16::ZERO) * sign;
            current_x = next_x;
            current_xi += 1;

            while current_x < end_x {
                next_x = current_x + 1;
                row[current_xi] += Self::cover_pixel(current_x, next_x, dy, current_y) * sign;
                current_x = next_x;
                current_xi += 1;
                current_y += dy;
            }

            debug_assert_eq!(current_x, end_x);
            row[current_xi] +=
                Self::cover_pixel(current_x, rx, height - current_y, current_y) * sign;
            row[current_xi] += height * (I16Dot16::ONE - (rx - current_x)) * sign;
            current_xi += 1;
        }

        for pixel in &mut row[current_xi..width] {
            *pixel += height * sign;
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
