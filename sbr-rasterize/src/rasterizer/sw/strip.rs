use util::math::{Fixed, FloatOutlineIterExt, I16Dot16, OutlineEvent, Point2, Point2f};

mod tile;

const DEFAULT_QUADRATIC_FLATTEN_TOLERANCE: f32 = 0.2;
const DEFAULT_CUBIC_TO_QUADRATIC_TOLERANCE: f32 = 1.0;

pub struct StripRasterizer {
    tiles: Vec<Tile>,
    tile_rasterizer: Box<dyn tile::TileRasterizer>,
}

#[derive(Debug, Clone, Copy)]
struct Tile {
    pos: Point2<u16>,
    width: u16,
    line: TileLine,
    winding: Winding,
    intersects_top: bool,
}

#[derive(Debug, Clone, Copy)]
struct TileLine {
    bottom_x: I16Dot16,
    bottom_y: U2Dot14,
    top_x: I16Dot16,
    top_y: U2Dot14,
}

#[derive(Debug, Clone, Copy)]
enum Winding {
    CounterClockwise = -1,
    Clockwise = 1,
}

type U2Dot14 = Fixed<14, u16>;

fn floor_to_tile(v: I16Dot16) -> I16Dot16 {
    I16Dot16::from_raw((v.into_raw() >> 18) << 18)
}

fn to_tile_fixed(v: I16Dot16) -> U2Dot14 {
    U2Dot14::from_raw((v.into_raw() >> 2).clamp(0, i32::from(U2Dot14::MAX.into_raw())) as u16)
}

fn tile_to_coord(v: u16) -> I16Dot16 {
    I16Dot16::from_raw(i32::from(v) << 18)
}

fn to_op_fixed(v: U2Dot14) -> I16Dot16 {
    I16Dot16::from_raw(i32::from(v.into_raw()) << 2)
}

impl StripRasterizer {
    pub fn new() -> Self {
        Self {
            tiles: Vec::new(),
            tile_rasterizer: tile::init_tile_rasterizer(),
        }
    }

    fn process_line(&mut self, start: Point2<I16Dot16>, end: Point2<I16Dot16>) {
        if start.y != end.y {
            let mut start = start;
            let mut end = end;
            let winding = if end.y > start.y {
                std::mem::swap(&mut start, &mut end);
                Winding::Clockwise
            } else {
                Winding::CounterClockwise
            };

            self.add_tiles(end, start, winding);
        } else if start != end {
            self.add_horizontal_tiles(start.x, end.x, end.y)
        }
    }

    fn process_linef(&mut self, start: Point2f, end: Point2f) {
        self.process_line(
            Point2::new(start.x.into(), start.y.into()),
            Point2::new(end.x.into(), end.y.into()),
        );
    }

    pub fn add_outline(&mut self, iter: impl Iterator<Item = OutlineEvent<f32>>) {
        self.add_outline_with(
            iter,
            DEFAULT_QUADRATIC_FLATTEN_TOLERANCE,
            DEFAULT_CUBIC_TO_QUADRATIC_TOLERANCE,
        );
    }

    pub fn add_outline_with(
        &mut self,
        iter: impl Iterator<Item = OutlineEvent<f32>>,
        quadratic_flatten_tolerance: f32,
        cubic_reduction_tolerance: f32,
    ) {
        iter.visit_flattened_with(
            |p0, p1| self.process_linef(p0, p1),
            quadratic_flatten_tolerance,
            cubic_reduction_tolerance,
        )
    }

    pub fn add_polyline(&mut self, points: &[Point2f]) {
        let Some(&(mut prev)) = points.first() else {
            return;
        };

        for &next in points {
            self.process_linef(prev, next);
            prev = next;
        }

        let &last = points.last().unwrap();
        let &first = points.first().unwrap();
        if last != first {
            self.process_linef(last, first)
        }
    }

    fn add_tile(
        &mut self,
        tile_x: u16,
        end_tile_x: u16,
        tile_y: u16,
        mut bottom_inner_x: I16Dot16,
        bottom_inner_y: U2Dot14,
        mut top_inner_x: I16Dot16,
        top_inner_y: U2Dot14,
        winding: Winding,
        intersects_top: bool,
    ) {
        debug_assert!(bottom_inner_y <= top_inner_y);

        let (pos_x, width) = if tile_x <= end_tile_x {
            debug_assert!(top_inner_x >= 0);
            (tile_x, end_tile_x - tile_x + 1)
        } else {
            debug_assert!(top_inner_x <= 0);
            let o = I16Dot16::new(4 * i32::from(tile_x - end_tile_x));
            bottom_inner_x = o + bottom_inner_x;
            top_inner_x = o + top_inner_x;
            (end_tile_x, tile_x - end_tile_x + 1)
        };

        eprintln!("y={tile_y} x={pos_x} width={width} {bottom_inner_x},{bottom_inner_y} -- {top_inner_x},{top_inner_y} {winding:?}");

        self.tiles.push(Tile {
            pos: Point2::new(pos_x, tile_y),
            width,
            line: TileLine {
                bottom_x: bottom_inner_x,
                bottom_y: bottom_inner_y,
                top_x: top_inner_x,
                top_y: top_inner_y,
            },
            winding,
            intersects_top,
        });
    }

    fn add_tiles(&mut self, mut bottom: Point2<I16Dot16>, top: Point2<I16Dot16>, winding: Winding) {
        if top.y <= 0 {
            return;
        }

        let dx = (top.x - bottom.x) / (top.y - bottom.y);
        if bottom.y < 0 {
            bottom.x += (-bottom.y) * dx;
            bottom.y = I16Dot16::ZERO;
        };

        let mut tile_x = (bottom.x.floor_to_inner() as u32 >> 2) as u16;
        let mut tile_y = (bottom.y.floor_to_inner() as u32 >> 2) as u16;
        let top_tile_x = (top.x.floor_to_inner() as u32 >> 2) as u16;
        let end_tile_y = ((top.y.ceil_to_inner() as u32 - 1) >> 2) as u16;
        let end_inner_y16 = top.y - tile_to_coord(end_tile_y);
        let end_inner_y = to_tile_fixed(end_inner_y16);
        let mut current_inner_x = bottom.x - floor_to_tile(bottom.x.floor());
        let dx4 = dx * 4;

        let bottom_inner_y = bottom.y - floor_to_tile(bottom.y);
        if tile_y == end_tile_y {
            self.add_tile(
                tile_x,
                top_tile_x,
                tile_y,
                current_inner_x,
                to_tile_fixed(bottom_inner_y),
                top.x - tile_to_coord(tile_x),
                end_inner_y,
                winding,
                end_inner_y16 == 4,
            );
            return;
        } else {
            let next_x = current_inner_x + (dx * (I16Dot16::new(4) - bottom_inner_y));
            let next_tile_x = tile_x.wrapping_add_signed((next_x.into_raw() >> 18) as i16);
            let next_inner_x = I16Dot16::from_raw(next_x.into_raw() & 0x3FFFF);
            self.add_tile(
                tile_x,
                next_tile_x,
                tile_y,
                current_inner_x,
                to_tile_fixed(bottom_inner_y),
                next_x,
                U2Dot14::MAX,
                winding,
                true,
            );
            tile_x = next_tile_x;
            current_inner_x = next_inner_x;
            tile_y += 1;
        }

        while tile_y < end_tile_y {
            let next_x = current_inner_x + dx4;
            let next_tile_x = tile_x.wrapping_add_signed((next_x.into_raw() >> 18) as i16);
            let next_inner_x = I16Dot16::from_raw(next_x.into_raw() & 0x3FFFF);

            self.add_tile(
                tile_x,
                next_tile_x,
                tile_y,
                current_inner_x,
                U2Dot14::ZERO,
                next_x,
                U2Dot14::MAX,
                winding,
                true,
            );

            tile_x = next_tile_x;
            current_inner_x = next_inner_x;
            tile_y += 1;
        }

        debug_assert_eq!(tile_y, end_tile_y);
        self.add_tile(
            tile_x,
            top_tile_x,
            tile_y,
            current_inner_x,
            U2Dot14::ZERO,
            top.x - tile_to_coord(tile_x),
            end_inner_y,
            winding,
            end_inner_y16 == 4,
        );
    }

    fn add_horizontal_tiles(&mut self, mut start_x: I16Dot16, mut end_x: I16Dot16, y: I16Dot16) {
        if y < 0 || y.fract() == 0 {
            return;
        }
        if start_x > end_x {
            std::mem::swap(&mut start_x, &mut end_x)
        }
        if start_x < 0 {
            start_x = I16Dot16::ZERO;
        }
        if start_x > end_x {
            return;
        }

        let tile_y = (y.floor_to_inner() as u32 >> 2) as u16;
        let tile_x = (start_x.floor_to_inner() as u32 >> 2) as u16;
        let end_tile_x = (end_x.floor_to_inner() as u32 >> 2) as u16;

        self.add_tile(
            tile_x,
            end_tile_x,
            tile_y,
            I16Dot16::ZERO,
            U2Dot14::ZERO,
            I16Dot16::ZERO,
            U2Dot14::ZERO,
            Winding::Clockwise,
            false,
        );
    }
}

#[derive(Debug, Clone)]
pub struct Strips {
    strips: Vec<Strip>,
    alpha_buffer: AlphaBuffer,
}

#[derive(Debug, Clone)]
struct AlphaBuffer(Vec<u64>);

impl AlphaBuffer {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn len8(&self) -> usize {
        self.0.len()
    }

    fn resize(&mut self, len8: usize) {
        self.0.resize(len8, 0);
    }

    fn as_u8(&self) -> &[u8] {
        let len8 = self.0.len();
        unsafe { std::slice::from_raw_parts(self.0.as_ptr().cast::<u8>(), len8 << 3) }
    }

    fn get_subslice_mut(&mut self, start8: usize, end8: usize) -> &mut [u8] {
        let slice = &mut self.0[start8..end8];
        unsafe {
            std::slice::from_raw_parts_mut(slice.as_mut_ptr().cast::<u8>(), (end8 - start8) << 3)
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Strip {
    pos: Point2<u16>,
    width: u16,
    fill_previous: bool,
}

impl Strips {
    pub fn paint_iter(&self) -> StripPaintIter<'_> {
        StripPaintIter {
            iter: self.strips.iter(),
            last_x: u16::MAX,
            alpha_buffer: self.alpha_buffer.as_u8(),
        }
    }

    pub fn paint_to(&self, buffer: &mut [u8], width: usize, height: usize, stride: usize) {
        for op in self.paint_iter() {
            let pos = op.pos();
            let out_pos = Point2::new(usize::from(pos.x) * 4, usize::from(pos.y) * 4);
            if out_pos.y >= height || out_pos.x >= width {
                continue;
            }

            match op {
                StripPaintOp::Copy(op) => {
                    let op_width = usize::from(op.width());
                    let copy_width = op_width.min(width - out_pos.x);

                    let mut src = op.buffer;
                    let mut rows = &mut buffer[out_pos.y * stride + out_pos.x..];
                    while !rows.is_empty() && !src.is_empty() {
                        rows[..copy_width].copy_from_slice(&src[..copy_width]);
                        src = &src[op_width..];
                        rows = match rows.get_mut(stride..) {
                            Some(next_row) => next_row,
                            None => break,
                        }
                    }
                }
                StripPaintOp::Fill(op) => {
                    let fill_height = 4.min(height - out_pos.y);
                    let fill_width = (usize::from(op.width) * 4).min(width - out_pos.x);

                    let mut rows = &mut buffer[out_pos.y * stride + out_pos.x..];
                    for _ in 0..fill_height {
                        rows[..fill_width].fill(u8::MAX);
                        rows = &mut rows[stride..]
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct StripPaintIter<'a> {
    iter: std::slice::Iter<'a, Strip>,
    last_x: u16,
    alpha_buffer: &'a [u8],
}

#[derive(Debug, Clone, Copy)]
pub enum StripPaintOp<'a> {
    Copy(StripCopyOp<'a>),
    Fill(StripFillOp),
}

impl StripPaintOp<'_> {
    #[inline]
    fn pos(&self) -> Point2<u16> {
        match self {
            Self::Copy(x) => x.pos,
            Self::Fill(x) => x.pos,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StripCopyOp<'a> {
    pub pos: Point2<u16>,
    pub buffer: &'a [u8],
}

impl StripCopyOp<'_> {
    #[inline]
    pub fn height(&self) -> u16 {
        4
    }

    #[inline]
    pub fn width(&self) -> u16 {
        (self.buffer.len() / 4) as u16
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StripFillOp {
    pub pos: Point2<u16>,
    pub width: u16,
}

impl<'a> Iterator for StripPaintIter<'a> {
    type Item = StripPaintOp<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.iter.as_slice().first()?;

        let last_x = self.last_x;
        self.last_x = next.pos.x + next.width;
        if last_x < next.pos.x && next.fill_previous {
            return Some(StripPaintOp::Fill(StripFillOp {
                pos: Point2::new(last_x, next.pos.y),
                width: next.pos.x - last_x,
            }));
        }

        let slice_len = 4 * 4 * usize::from(next.width);
        // TODO: `slice_take`, stabilized in 1.87 which is after our MSRV
        let copy_buffer = &self.alpha_buffer[..slice_len];
        self.alpha_buffer = &self.alpha_buffer[slice_len..];
        self.iter.next();
        Some(StripPaintOp::Copy(StripCopyOp {
            pos: next.pos,
            buffer: copy_buffer,
        }))
    }
}

impl StripRasterizer {
    pub fn rasterize(&mut self) -> Strips {
        let mut result = Strips {
            strips: Vec::new(),
            alpha_buffer: AlphaBuffer::new(),
        };

        self.tiles
            .sort_unstable_by(|Tile { pos: a, .. }, Tile { pos: b, .. }| {
                a.y.cmp(&b.y).then(a.x.cmp(&b.x))
            });

        let mut last_y = 0;
        let mut start = 0;
        let mut strip_winding = 0;
        while start < self.tiles.len() {
            let strip_pos = self.tiles[start].pos;
            let mut strip_end = strip_pos.x + self.tiles[start].width;
            let mut end = start + 1;
            while let Some(next) = self
                .tiles
                .get(end)
                .filter(|t| t.pos.y == strip_pos.y && t.pos.x <= strip_end)
            {
                strip_end = strip_end.max(next.pos.x + next.width);
                end += 1;
            }

            if last_y != strip_pos.y {
                // NOTE: This should only happen if an invalid outline is provided,
                //       but let's safeguard ourselves from a panic in paint op
                //       construction.
                strip_winding = 0;
            }

            let strip_tiles = &self.tiles[start..end];
            let strip_width = strip_end - strip_pos.x;
            let strip_buffer_start8 = result.alpha_buffer.len8();
            let strip_buffer_end8 = strip_buffer_start8 + 2 * usize::from(strip_width);
            result.alpha_buffer.resize(strip_buffer_end8);
            let buffer = result
                .alpha_buffer
                .get_subslice_mut(strip_buffer_start8, strip_buffer_end8);

            unsafe {
                self.tile_rasterizer.rasterize(
                    strip_pos.x,
                    strip_tiles,
                    I16Dot16::new(strip_winding),
                    buffer,
                );
            }

            result.strips.push(Strip {
                pos: strip_pos,
                width: strip_width,
                fill_previous: strip_winding != 0,
            });

            for tile in strip_tiles {
                if tile.intersects_top {
                    strip_winding += tile.winding as i32;
                }
            }

            last_y = strip_pos.y;
            start = end;
        }

        result
    }
}

#[cfg(test)]
mod test {
    use util::{
        make_static_outline,
        math::{Outline, Point2, StaticOutline, Vec2},
    };

    use crate::sw::{GlyphRasterizer, StripRasterizer};

    fn compare(size: Vec2<u32>, coverage: &[u8], expected: &[u8]) {
        let mut matches = true;
        for y in (0..size.y as usize).rev() {
            for x in 0..size.x as usize {
                let exp = expected
                    .get((size.y as usize - y - 1) * size.x as usize + x)
                    .copied()
                    .unwrap_or(0);
                if coverage[y * size.x as usize + x] != exp {
                    matches = false;
                    break;
                }
            }
        }

        if !matches {
            let side_by_side = size.x < 30;
            let print_row = |y: usize, which: bool| {
                for x in 0..size.x as usize {
                    if x != 0 {
                        eprint!(" ")
                    }

                    let v = coverage[y * size.x as usize + x];
                    let exp = expected
                        .get((size.y as usize - y - 1) * size.x as usize + x)
                        .copied()
                        .unwrap_or(0);
                    let (pref, suff) = if v == exp {
                        if v != 0 {
                            ("\x1b[32;1m", "\x1b[0m")
                        } else {
                            ("", "")
                        }
                    } else if v < exp {
                        ("\x1b[31;1m", "\x1b[0m")
                    } else {
                        ("\x1b[33;1m", "\x1b[0m")
                    };

                    eprint!("{pref}{:02X}{suff}", if which { v } else { exp })
                }
            };

            for y in (0..size.y as usize).rev() {
                print_row(y, true);
                if side_by_side {
                    eprint!("    ");
                    print_row(y, false);
                }

                eprintln!()
            }

            if !side_by_side {
                eprintln!();

                for y in (0..size.y as usize).rev() {
                    print_row(y, false);
                    eprintln!()
                }
            }

            panic!()
        }
    }

    fn test_outline(outline: &StaticOutline<f32>, expected: &[u8]) {
        let size = Vec2::new(
            outline.control_box().max.x.ceil() as u32,
            outline.control_box().max.y.ceil() as u32,
        );
        let mut rasterizer = StripRasterizer::new();
        rasterizer.add_outline(outline.events());

        let strips = rasterizer.rasterize();
        let mut coverage = vec![0; size.x as usize * size.y as usize];
        strips.paint_to(
            &mut coverage,
            size.x as usize,
            size.y as usize,
            size.x as usize,
        );

        compare(size, &coverage, expected);
    }

    // FIXME: Some of these tests have small differences like 0x80 instead of 0x7F
    //        It would be nice to figure out the source of this imprecision.

    #[test]
    fn small_triangle1() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x40, 0xBF, 0xFF, 0xFF,
            0x00, 0x00, 0x40, 0xBF,
        ];

        test_outline(
            &make_static_outline![
                #move_to (0.0, 2.0);
                line_to (4.0, 0.0);
                line_to (4.0, 2.0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn small_triangle2() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x00, 0x00, 0x00, 0x7F,
            0x00, 0x00, 0x7F, 0xFF,
            0x00, 0x7F, 0xFF, 0xFF,
            0x7F, 0xFF, 0xFF, 0xFF,
        ];

        test_outline(
            &make_static_outline![
                #move_to (0.0, 0.0);
                line_to (4.0, 0.0);
                line_to (4.0, 4.0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn small_triangle3() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x7F, 0xFF, 0xFF, 0xFF,
            0x00, 0x80, 0xFF, 0xFF,
            0x00, 0x00, 0x80, 0xFF,
            0x00, 0x00, 0x00, 0x80,
        ];

        test_outline(
            &make_static_outline![
                #move_to (4.0, 4.0);
                line_to (4.0, 0.0);
                line_to (0.0, 4.0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn small_triangle4() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x7F, 0x00, 0x00, 0x00,
            0xFF, 0x7F, 0x00, 0x00,
            0xFF, 0xFF, 0x80, 0x00,
            0xFF, 0xFF, 0xFF, 0x80,
        ];

        test_outline(
            &make_static_outline![
                #move_to (0.0, 4.0);
                line_to (4.0, 0.0);
                line_to (0.0, 0.0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn small_triangle4_partial() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x7F, 0x00, 0x00,
            0xFF, 0x80, 0x00,
            0xFF, 0xFF, 0x80,
            0x00, 0x00, 0x00,
        ];

        test_outline(
            &make_static_outline![
                #move_to (0.0, 4.0);
                line_to (3.0, 1.0);
                line_to (0.0, 1.0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn tiny_triangle() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x00, 0x00, 0xFF, 0x7F,
            0x00, 0x00, 0x80, 0x00,
            0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ];

        test_outline(
            &make_static_outline![
                #move_to (2.0, 2.0);
                line_to (2.0, 4.0);
                line_to (4.0, 4.0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn medium_triangle() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x00, 0x00, 0x00, 0x40, 0x40, 0x00,
            0x00, 0x00, 0x00, 0xBF, 0xBF, 0x00,
            0x00, 0x00, 0x40, 0xFF, 0xFF, 0x40,
            0x00, 0x00, 0xBF, 0xFF, 0xFF, 0xBF,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        test_outline(
            &make_static_outline![
                #move_to (2.0, 2.0);
                line_to (4.0, 6.0);
                line_to (6.0, 2.0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn high_triangle() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x00, 0x20,
            0x00, 0x60,
            0x00, 0x9F,
            0x00, 0xDF,
            0x20, 0xFF,
            0x60, 0xFF,
            0x9F, 0xFF,
            0xDF, 0xFF,
        ];

        test_outline(
            &make_static_outline![
                #move_to (0.0, 0.0);
                line_to (2.0, 8.0);
                line_to (2.0, 0.0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn wide_triangle() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x60, 0x20,
            0xFF, 0xFF, 0xDF, 0x9F, 0x60, 0x20, 0x00, 0x00,
            0x60, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        test_outline(
            &make_static_outline![
                #move_to (8.0, 2.5);
                line_to (0.0, 0.5);
                line_to (0.0, 2.5);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn wide_quad_triangle() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x40, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x6A, 0x40, 0x15,
            0x80, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xE9, 0xBE, 0x94, 0x6A, 0x3F, 0x15, 0x00, 0x00, 0x00,
            0x3F, 0x7B, 0x76, 0x68, 0x3F, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        test_outline(
            &make_static_outline![
                #move_to (0.5, 0.5);
                quad_to (2.0, 0.0), (15.0, 2.5);
                line_to (0.5, 2.5);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn large_triangle1() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x49, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0xBF, 0xD6, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x40, 0xFF, 0xFF, 0x6D, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0xBF, 0xFF, 0xFF, 0xED, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x40, 0xFF, 0xFF, 0xFF, 0xFF, 0x92, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0xBF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFA, 0x29, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x40, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xB6, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0xBF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x49, 0x00, 0x00, 0x00,
            0x00, 0x40, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xD6, 0x05, 0x00, 0x00,
            0x00, 0xBF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x6D, 0x00, 0x00,
            0x40, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xED, 0x12, 0x00,
            0xBF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x92, 0x00,
            0x12, 0x37, 0x5B, 0x7F, 0xA4, 0xC8, 0xED, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFA, 0x29,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x12, 0x37, 0x5B, 0x7F, 0xA4, 0xC8, 0xA4,
        ];

        test_outline(
            &make_static_outline![
                #move_to (6.0, 14.0);
                line_to (14.0, 0.0);
                line_to (0.0, 2.0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn large_triangle2() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x55, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x15, 0xEA, 0xBF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xAA, 0xFF, 0xFF, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x55, 0xFF, 0xFF, 0xFF, 0xBF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x15, 0xEA, 0xFF, 0xFF, 0xFF, 0xFF, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0xAA, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xBF, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x55, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x40, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x15, 0xEA, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xBF, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0xAA, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x40, 0x00, 0x00, 0x00,
                0x00, 0x55, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xBF, 0x00, 0x00, 0x00,
                0x15, 0xEA, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x40, 0x00, 0x00,
                0xAA, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xBF, 0x00, 0x00,
                0x20, 0x60, 0x9F, 0xDF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x40, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x20, 0x60, 0x9F, 0xDF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xBF, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x60, 0x9F, 0xDF, 0xFF, 0xFF, 0xFF, 0x40,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x60, 0x9F, 0x9F,
        ];

        test_outline(
            &make_static_outline![
                #move_to (8.0, 16.0);
                line_to (16.0, 0.0);
                line_to (0.0, 4.0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn half_capital_a() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
        ];

        test_outline(
            &make_static_outline![
                #move_to (4.0, 14.0);
                line_to (20.0, 14.0);
                line_to (20.0, 6.0);
                line_to (4.0, 6.0);

                // #move_to (0.0, 0.0);
                // line_to (8, 20);
                // line_to (12, 20);
                // line_to (4, 0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn large_triangle5() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, 0x00, 0x15, 0x70, 0xCF, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x05, 0x50, 0xAF, 0xFA, 0xFF, 0xFF, 0xBF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x30, 0x8F, 0xEA, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x68, 0xF9, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xBF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x3D, 0xE9, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x1E, 0xCD, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xBF, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x0A, 0xA7, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x40, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x01, 0x78, 0xFD, 0xFF, 0xFF, 0xFF, 0xFF, 0xBF, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4A, 0xF0, 0xFF, 0xFF, 0xFF, 0xFF, 0x40, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x27, 0xD8, 0xFF, 0xFF, 0xFF, 0xBF, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0F, 0xB5, 0xFF, 0xFF, 0xFF, 0x40, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x87, 0xFE, 0xFF, 0xBF, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x58, 0xF5, 0xFF, 0x40, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x32, 0xE1, 0xBF, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x16, 0xC2, 0x40,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x58,
        ];

        test_outline(
            &make_static_outline![
                #move_to (8.0, 16.0);
                line_to (16.0, 0.0);
                line_to (0.0, 13.0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn large_rectangle() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        test_outline(
            &make_static_outline![
                #move_to (4.0, 4.0);
                line_to (4.0, 16.0);
                line_to (16.0, 16.0);
                line_to (16.0, 4.0);
            ],
            EXPECTED,
        );
    }

    static SAI: StaticOutline<f32> = make_static_outline![
        #move_to (12.1875, 4.109375);
        line_to (12.1875, 2.390625);
        line_to (27.09375, 2.390625);
        line_to (27.09375, 4.109375);
        #move_to (19.390625, 19.6875);
        line_to (19.390625, 17.734375);
        line_to (28, 17.734375);
        line_to (28, 19.6875);
        #move_to (12.1875, 0.265625);
        line_to (12.1875, 2.171875);
        line_to (27.09375, 2.171875);
        line_to (27.09375, 0.265625);
        #move_to (18.484375, 7.59375);
        line_to (18.484375, 1.046875);
        line_to (20.53125, 1.046875);
        line_to (20.53125, 7.59375);
        #move_to (15.0625, 15.859375);
        cubic_to (16.28125, 14.296875), (17.53125, 12.15625), (18.03125, 10.71875);
        line_to (19.78125, 11.5625);
        cubic_to (19.234375, 12.96875), (17.953125, 15.078125), (16.703125, 16.59375);
        #move_to (11.140625, 8.578125);
        line_to (11.140625, 3.25);
        line_to (13.34375, 3.25);
        line_to (13.34375, 6.640625);
        line_to (25.984375, 6.640625);
        line_to (25.984375, 3.125);
        line_to (28.234375, 3.125);
        line_to (28.234375, 8.578125);
        #move_to (10.546875, 19.546875);
        line_to (10.96875, 11.171875);
        line_to (13.046875, 11.171875);
        line_to (12.6875, 18.40625);
        cubic_to (13.078125, 18.46875), (13.265625, 18.625), (13.328125, 18.796875);
        #move_to (27.328125, 19.6875);
        line_to (27.328125, 19.34375);
        cubic_to (27.171875, 14.203125), (26.953125, 12.375), (26.578125, 11.90625);
        cubic_to (26.390625, 11.640625), (26.171875, 11.59375), (25.78125, 11.59375);
        cubic_to (25.421875, 11.59375), (24.5, 11.609375), (23.484375, 11.71875);
        cubic_to (23.765625, 11.1875), (23.953125, 10.34375), (24, 9.78125);
        cubic_to (25.046875, 9.703125), (26.15625, 9.71875), (26.71875, 9.765625);
        cubic_to (27.4375, 9.84375), (27.90625, 10.015625), (28.296875, 10.53125);
        cubic_to (28.90625, 11.28125), (29.140625, 13.34375), (29.34375, 18.765625);
        cubic_to (29.375, 19.0625), (29.390625, 19.6875), (29.390625, 19.6875);
        #move_to (9.25, 11.53125);
        line_to (9.859375, 9.625);
        cubic_to (12.21875, 10.375), (15.140625, 11.34375), (17.96875, 12.296875);
        line_to (17.765625, 14);
        cubic_to (14.59375, 13.046875), (11.4375, 12.09375), (9.25, 11.53125);
        #move_to (17.140625, 21.0625);
        cubic_to (15.78125, 20.203125), (13.3125, 19.3125), (11.109375, 18.703125);
        cubic_to (11.359375, 18.28125), (11.65625, 17.625), (11.765625, 17.21875);
        cubic_to (14.140625, 17.78125), (16.90625, 18.65625), (18.859375, 19.6875);
        #move_to (22.34375, 19.03125);
        cubic_to (22.046875, 15.046875), (21.109375, 11.90625), (17.6875, 10.1875);
        cubic_to (18.125, 9.828125), (18.71875, 9.109375), (18.984375, 8.640625);
        cubic_to (22.8125, 10.703125), (23.921875, 14.3125), (24.328125, 19.03125);
        #move_to (7.15625, 24.046875);
        line_to (7.15625, 21.90625);
        line_to (30.796875, 21.90625);
        line_to (30.796875, 24.046875);
        #move_to (5.96875, 24.046875);
        line_to (5.96875, 13.421875);
        cubic_to (5.96875, 8.703125), (5.59375, 2.640625), (1.9375, 1.625);
        cubic_to (2.40625, 1.9375), (3.28125, 2.78125), (3.625, 3.265625);
        cubic_to (7.59375, 1.296875), (8.203125, 8.328125), (8.203125, 13.421875);
        line_to (8.203125, 24.046875);
        #move_to (16.34375, 27.171875);
        line_to (16.34375, 22.953125);
        line_to (18.84375, 22.953125);
        line_to (18.84375, 27.171875);
        #move_to (1.515625, 20.46875);
        cubic_to (2.46875, 18.390625), (3.3125, 15.671875), (3.53125, 13.953125);
        line_to (5.453125, 14.84375);
        cubic_to (5.21875, 16.5), (4.34375, 19.140625), (3.3125, 21.203125);
        #move_to (1.046875, 8.328125);
        line_to (1.859375, 6.15625);
        cubic_to (3.546875, 7.125), (5.375, 8.25), (7.15625, 9.40625);
        line_to (6.609375, 11.296875);
        cubic_to (4.484375, 10.125), (2.484375, 9.015625), (1.046875, 8.328125);
    ];

    static SAI2: StaticOutline<f32> = make_static_outline![
              #move_to (577.5156, 259.64063);
                cubic_to
                    (593.34375, 606.84375),
                    (733.4531, 922.6406),
                    (867.09375, 923.03125);
                cubic_to
                    (927.1406, 923.03125),
                    (953.5, 883.28125),
                    (964.7969, 743.9531);
                cubic_to
                    (946.8281, 737.4375),
                    (923.34375, 724.2344),
                    (907.9375, 710.4219);
                cubic_to
                    (903.3281, 811.9844),
                    (894.09375, 856.1406),
                    (871.6094, 856.1406);
                cubic_to
                    (791.6094, 856.75),
                    (656.1719, 582.2969),
                    (647.125, 259.64063);
                line_to (577.5156, 259.64063);
                cubic_to
                    (748.5781, 325.78125),
                    (802.5469, 365.8125),
                    (828.2656, 393.67188);
                line_to (872.5625, 355.25);
                cubic_to
                    (846.2969, 328.0),
                    (791.53125, 289.75),
                    (747.3906, 263.89063);
                line_to (704.4219, 298.14063);
                cubic_to
                    (496.125, 675.3594),
                    (526.65625, 739.53125),
                    (538.09375, 781.0625);
                line_to (591.0156, 758.28125);
                cubic_to
                    (579.7969, 718.375),
                    (547.46875, 654.9844),
                    (516.3281, 608.4531);
                line_to (465.96875, 627.4375);
                cubic_to
                    (764.5156, 647.15625),
                    (650.3594, 786.28125),
                    (497.21875, 868.7344);
                cubic_to
                    (512.40625, 881.375),
                    (538.21875, 907.21875),
                    (548.46875, 921.03125);
                cubic_to
                    (702.34375, 827.28125),
                    (823.6094, 679.34375),
                    (889.0469, 482.875);
                line_to (821.65625, 467.89063);
                cubic_to
                    (247.4375, 670.78125),
                    (219.32813, 735.1719),
                    (182.84375, 780.7031);
                cubic_to
                    (196.85938, 787.8281),
                    (221.5625, 803.46875),
                    (232.40625, 812.78125);
                cubic_to
                    (268.07813, 764.6719),
                    (302.73438, 691.15625),
                    (322.78125, 620.4375);
                line_to (265.3125, 607.78125);
                line_to (158.29688, 426.0625);
                line_to (936.96875, 426.0625);
                line_to (936.96875, 364.01563);
                line_to (158.29688, 364.01563);
                line_to (230.59375, 559.0);
                line_to (563.21875, 559.0);
                line_to (563.21875, 500.29688);
                line_to (230.59375, 500.29688);
                line_to (61.1875, 272.15625);
                line_to (941.7969, 272.15625);
                line_to (941.7969, 208.9375);
                line_to (61.1875, 208.9375);
                line_to (518.2969, 126.71875);
                line_to (846.0469, 126.71875);
                line_to (846.0469, 69.96875);
                line_to (518.2969, 69.96875);
                line_to (117.953125, 533.75);
                cubic_to
                    (117.953125, 636.2969),
                    (108.296875, 775.65625),
                    (32.40625, 878.25);
                cubic_to
                    (48.0, 886.5469),
                    (77.9375, 909.0),
                    (89.5625, 922.6406);
                cubic_to
                    (171.73438, 812.3594),
                    (187.125, 649.5),
                    (187.125, 534.53125);
                line_to (187.125, 364.01563);
                line_to (117.953125, 364.01563);
                line_to (477.29688, 239.70313);
                line_to (550.5781, 239.70313);
                line_to (550.5781, 0.0);
                line_to (477.29688, 0.0);
                line_to (223.32813, 247.21875);
                line_to (293.89063, 247.21875);
                line_to (293.89063, 44.4375);
                line_to (223.32813, 44.4375);
                line_to (366.23438, 836.2031);
                cubic_to
                    (366.23438, 844.6094),
                    (364.23438, 847.21875),
                    (354.01563, 847.8281);
                cubic_to
                    (344.57813, 848.4375),
                    (316.70313, 848.4375),
                    (282.0625, 847.4375);
                cubic_to
                    (289.96875, 864.6406),
                    (299.46875, 889.15625),
                    (302.64063, 907.53125);
                cubic_to
                    (348.07813, 907.53125),
                    (381.25, 906.75),
                    (402.32813, 896.0625);
                cubic_to
                    (423.78125, 885.75),
                    (428.78125, 868.3281),
                    (428.78125, 836.7656);
                line_to (428.78125, 535.90625);
                line_to (366.23438, 535.90625);
                #move_to (704.4219, 298.14063);
                cubic_to
                    (593.34375, 606.84375),
                    (733.4531, 922.6406),
                    (867.09375, 923.03125);
                cubic_to
                    (927.1406, 923.03125),
                    (953.5, 883.28125),
                    (964.7969, 743.9531);
                cubic_to
                    (946.8281, 737.4375),
                    (923.34375, 724.2344),
                    (907.9375, 710.4219);
                cubic_to
                    (903.3281, 811.9844),
                    (894.09375, 856.1406),
                    (871.6094, 856.1406);
                cubic_to
                    (791.6094, 856.75),
                    (656.1719, 582.2969),
                    (647.125, 259.64063);
                line_to (577.5156, 259.64063);
                cubic_to
                    (748.5781, 325.78125),
                    (802.5469, 365.8125),
                    (828.2656, 393.67188);
                line_to (872.5625, 355.25);
                cubic_to
                    (846.2969, 328.0),
                    (791.53125, 289.75),
                    (747.3906, 263.89063);
                line_to (704.4219, 298.14063);
                cubic_to
                    (496.125, 675.3594),
                    (526.65625, 739.53125),
                    (538.09375, 781.0625);
                line_to (591.0156, 758.28125);
                cubic_to
                    (579.7969, 718.375),
                    (547.46875, 654.9844),
                    (516.3281, 608.4531);
                line_to (465.96875, 627.4375);
                cubic_to
                    (764.5156, 647.15625),
                    (650.3594, 786.28125),
                    (497.21875, 868.7344);
                cubic_to
                    (512.40625, 881.375),
                    (538.21875, 907.21875),
                    (548.46875, 921.03125);
                cubic_to
                    (702.34375, 827.28125),
                    (823.6094, 679.34375),
                    (889.0469, 482.875);
                line_to (821.65625, 467.89063);
                cubic_to
                    (247.4375, 670.78125),
                    (219.32813, 735.1719),
                    (182.84375, 780.7031);
                cubic_to
                    (196.85938, 787.8281),
                    (221.5625, 803.46875),
                    (232.40625, 812.78125);
                cubic_to
                    (268.07813, 764.6719),
                    (302.73438, 691.15625),
                    (322.78125, 620.4375);
                line_to (265.3125, 607.78125);
                line_to (158.29688, 426.0625);
                line_to (936.96875, 426.0625);
                line_to (936.96875, 364.01563);
                line_to (158.29688, 364.01563);
                line_to (230.59375, 559.0);
                line_to (563.21875, 559.0);
                line_to (563.21875, 500.29688);
                line_to (230.59375, 500.29688);
                line_to (61.1875, 272.15625);
                line_to (941.7969, 272.15625);
                line_to (941.7969, 208.9375);
                line_to (61.1875, 208.9375);
                line_to (518.2969, 126.71875);
                line_to (846.0469, 126.71875);
                line_to (846.0469, 69.96875);
                line_to (518.2969, 69.96875);
                line_to (117.953125, 533.75);
                cubic_to
                    (117.953125, 636.2969),
                    (108.296875, 775.65625),
                    (32.40625, 878.25);
                cubic_to
                    (48.0, 886.5469),
                    (77.9375, 909.0),
                    (89.5625, 922.6406);
                cubic_to
                    (171.73438, 812.3594),
                    (187.125, 649.5),
                    (187.125, 534.53125);
                line_to (187.125, 364.01563);
                line_to (117.953125, 364.01563);
                line_to (477.29688, 239.70313);
                line_to (550.5781, 239.70313);
                line_to (550.5781, 0.0);
                line_to (477.29688, 0.0);
                line_to (223.32813, 247.21875);
                line_to (293.89063, 247.21875);
                line_to (293.89063, 44.4375);
                line_to (223.32813, 44.4375);
                line_to (366.23438, 836.2031);
                cubic_to
                    (366.23438, 844.6094),
                    (364.23438, 847.21875),
                    (354.01563, 847.8281);
                cubic_to
                    (344.57813, 848.4375),
                    (316.70313, 848.4375),
                    (282.0625, 847.4375);
                cubic_to
                    (289.96875, 864.6406),
                    (299.46875, 889.15625),
                    (302.64063, 907.53125);
                cubic_to
                    (348.07813, 907.53125),
                    (381.25, 906.75),
                    (402.32813, 896.0625);
                cubic_to
                    (423.78125, 885.75),
                    (428.78125, 868.3281),
                    (428.78125, 836.7656);
                line_to (428.78125, 535.90625);
                line_to (366.23438, 535.90625);
                #move_to (465.96875, 627.4375);
                cubic_to
                    (593.34375, 606.84375),
                    (733.4531, 922.6406),
                    (867.09375, 923.03125);
                cubic_to
                    (927.1406, 923.03125),
                    (953.5, 883.28125),
                    (964.7969, 743.9531);
                cubic_to
                    (946.8281, 737.4375),
                    (923.34375, 724.2344),
                    (907.9375, 710.4219);
                cubic_to
                    (903.3281, 811.9844),
                    (894.09375, 856.1406),
                    (871.6094, 856.1406);
                cubic_to
                    (791.6094, 856.75),
                    (656.1719, 582.2969),
                    (647.125, 259.64063);
                line_to (577.5156, 259.64063);
                cubic_to
                    (748.5781, 325.78125),
                    (802.5469, 365.8125),
                    (828.2656, 393.67188);
                line_to (872.5625, 355.25);
                cubic_to
                    (846.2969, 328.0),
                    (791.53125, 289.75),
                    (747.3906, 263.89063);
                line_to (704.4219, 298.14063);
                cubic_to
                    (496.125, 675.3594),
                    (526.65625, 739.53125),
                    (538.09375, 781.0625);
                line_to (591.0156, 758.28125);
                cubic_to
                    (579.7969, 718.375),
                    (547.46875, 654.9844),
                    (516.3281, 608.4531);
                line_to (465.96875, 627.4375);
                cubic_to
                    (764.5156, 647.15625),
                    (650.3594, 786.28125),
                    (497.21875, 868.7344);
                cubic_to
                    (512.40625, 881.375),
                    (538.21875, 907.21875),
                    (548.46875, 921.03125);
                cubic_to
                    (702.34375, 827.28125),
                    (823.6094, 679.34375),
                    (889.0469, 482.875);
                line_to (821.65625, 467.89063);
                cubic_to
                    (247.4375, 670.78125),
                    (219.32813, 735.1719),
                    (182.84375, 780.7031);
                cubic_to
                    (196.85938, 787.8281),
                    (221.5625, 803.46875),
                    (232.40625, 812.78125);
                cubic_to
                    (268.07813, 764.6719),
                    (302.73438, 691.15625),
                    (322.78125, 620.4375);
                line_to (265.3125, 607.78125);
                line_to (158.29688, 426.0625);
                line_to (936.96875, 426.0625);
                line_to (936.96875, 364.01563);
                line_to (158.29688, 364.01563);
                line_to (230.59375, 559.0);
                line_to (563.21875, 559.0);
                line_to (563.21875, 500.29688);
                line_to (230.59375, 500.29688);
                line_to (61.1875, 272.15625);
                line_to (941.7969, 272.15625);
                line_to (941.7969, 208.9375);
                line_to (61.1875, 208.9375);
                line_to (518.2969, 126.71875);
                line_to (846.0469, 126.71875);
                line_to (846.0469, 69.96875);
                line_to (518.2969, 69.96875);
                line_to (117.953125, 533.75);
                cubic_to
                    (117.953125, 636.2969),
                    (108.296875, 775.65625),
                    (32.40625, 878.25);
                cubic_to
                    (48.0, 886.5469),
                    (77.9375, 909.0),
                    (89.5625, 922.6406);
                cubic_to
                    (171.73438, 812.3594),
                    (187.125, 649.5),
                    (187.125, 534.53125);
                line_to (187.125, 364.01563);
                line_to (117.953125, 364.01563);
                line_to (477.29688, 239.70313);
                line_to (550.5781, 239.70313);
                line_to (550.5781, 0.0);
                line_to (477.29688, 0.0);
                line_to (223.32813, 247.21875);
                line_to (293.89063, 247.21875);
                line_to (293.89063, 44.4375);
                line_to (223.32813, 44.4375);
                line_to (366.23438, 836.2031);
                cubic_to
                    (366.23438, 844.6094),
                    (364.23438, 847.21875),
                    (354.01563, 847.8281);
                cubic_to
                    (344.57813, 848.4375),
                    (316.70313, 848.4375),
                    (282.0625, 847.4375);
                cubic_to
                    (289.96875, 864.6406),
                    (299.46875, 889.15625),
                    (302.64063, 907.53125);
                cubic_to
                    (348.07813, 907.53125),
                    (381.25, 906.75),
                    (402.32813, 896.0625);
                cubic_to
                    (423.78125, 885.75),
                    (428.78125, 868.3281),
                    (428.78125, 836.7656);
                line_to (428.78125, 535.90625);
                line_to (366.23438, 535.90625);
                #move_to (821.65625, 467.89063);
                cubic_to
                    (593.34375, 606.84375),
                    (733.4531, 922.6406),
                    (867.09375, 923.03125);
                cubic_to
                    (927.1406, 923.03125),
                    (953.5, 883.28125),
                    (964.7969, 743.9531);
                cubic_to
                    (946.8281, 737.4375),
                    (923.34375, 724.2344),
                    (907.9375, 710.4219);
                cubic_to
                    (903.3281, 811.9844),
                    (894.09375, 856.1406),
                    (871.6094, 856.1406);
                cubic_to
                    (791.6094, 856.75),
                    (656.1719, 582.2969),
                    (647.125, 259.64063);
                line_to (577.5156, 259.64063);
                cubic_to
                    (748.5781, 325.78125),
                    (802.5469, 365.8125),
                    (828.2656, 393.67188);
                line_to (872.5625, 355.25);
                cubic_to
                    (846.2969, 328.0),
                    (791.53125, 289.75),
                    (747.3906, 263.89063);
                line_to (704.4219, 298.14063);
                cubic_to
                    (496.125, 675.3594),
                    (526.65625, 739.53125),
                    (538.09375, 781.0625);
                line_to (591.0156, 758.28125);
                cubic_to
                    (579.7969, 718.375),
                    (547.46875, 654.9844),
                    (516.3281, 608.4531);
                line_to (465.96875, 627.4375);
                cubic_to
                    (764.5156, 647.15625),
                    (650.3594, 786.28125),
                    (497.21875, 868.7344);
                cubic_to
                    (512.40625, 881.375),
                    (538.21875, 907.21875),
                    (548.46875, 921.03125);
                cubic_to
                    (702.34375, 827.28125),
                    (823.6094, 679.34375),
                    (889.0469, 482.875);
                line_to (821.65625, 467.89063);
                cubic_to
                    (247.4375, 670.78125),
                    (219.32813, 735.1719),
                    (182.84375, 780.7031);
                cubic_to
                    (196.85938, 787.8281),
                    (221.5625, 803.46875),
                    (232.40625, 812.78125);
                cubic_to
                    (268.07813, 764.6719),
                    (302.73438, 691.15625),
                    (322.78125, 620.4375);
                line_to (265.3125, 607.78125);
                line_to (158.29688, 426.0625);
                line_to (936.96875, 426.0625);
                line_to (936.96875, 364.01563);
                line_to (158.29688, 364.01563);
                line_to (230.59375, 559.0);
                line_to (563.21875, 559.0);
                line_to (563.21875, 500.29688);
                line_to (230.59375, 500.29688);
                line_to (61.1875, 272.15625);
                line_to (941.7969, 272.15625);
                line_to (941.7969, 208.9375);
                line_to (61.1875, 208.9375);
                line_to (518.2969, 126.71875);
                line_to (846.0469, 126.71875);
                line_to (846.0469, 69.96875);
                line_to (518.2969, 69.96875);
                line_to (117.953125, 533.75);
                cubic_to
                    (117.953125, 636.2969),
                    (108.296875, 775.65625),
                    (32.40625, 878.25);
                cubic_to
                    (48.0, 886.5469),
                    (77.9375, 909.0),
                    (89.5625, 922.6406);
                cubic_to
                    (171.73438, 812.3594),
                    (187.125, 649.5),
                    (187.125, 534.53125);
                line_to (187.125, 364.01563);
                line_to (117.953125, 364.01563);
                line_to (477.29688, 239.70313);
                line_to (550.5781, 239.70313);
                line_to (550.5781, 0.0);
                line_to (477.29688, 0.0);
                line_to (223.32813, 247.21875);
                line_to (293.89063, 247.21875);
                line_to (293.89063, 44.4375);
                line_to (223.32813, 44.4375);
                line_to (366.23438, 836.2031);
                cubic_to
                    (366.23438, 844.6094),
                    (364.23438, 847.21875),
                    (354.01563, 847.8281);
                cubic_to
                    (344.57813, 848.4375),
                    (316.70313, 848.4375),
                    (282.0625, 847.4375);
                cubic_to
                    (289.96875, 864.6406),
                    (299.46875, 889.15625),
                    (302.64063, 907.53125);
                cubic_to
                    (348.07813, 907.53125),
                    (381.25, 906.75),
                    (402.32813, 896.0625);
                cubic_to
                    (423.78125, 885.75),
                    (428.78125, 868.3281),
                    (428.78125, 836.7656);
                line_to (428.78125, 535.90625);
                line_to (366.23438, 535.90625);
                #move_to (265.3125, 607.78125);
                cubic_to
                    (593.34375, 606.84375),
                    (733.4531, 922.6406),
                    (867.09375, 923.03125);
                cubic_to
                    (927.1406, 923.03125),
                    (953.5, 883.28125),
                    (964.7969, 743.9531);
                cubic_to
                    (946.8281, 737.4375),
                    (923.34375, 724.2344),
                    (907.9375, 710.4219);
                cubic_to
                    (903.3281, 811.9844),
                    (894.09375, 856.1406),
                    (871.6094, 856.1406);
                cubic_to
                    (791.6094, 856.75),
                    (656.1719, 582.2969),
                    (647.125, 259.64063);
                line_to (577.5156, 259.64063);
                cubic_to
                    (748.5781, 325.78125),
                    (802.5469, 365.8125),
                    (828.2656, 393.67188);
                line_to (872.5625, 355.25);
                cubic_to
                    (846.2969, 328.0),
                    (791.53125, 289.75),
                    (747.3906, 263.89063);
                line_to (704.4219, 298.14063);
                cubic_to
                    (496.125, 675.3594),
                    (526.65625, 739.53125),
                    (538.09375, 781.0625);
                line_to (591.0156, 758.28125);
                cubic_to
                    (579.7969, 718.375),
                    (547.46875, 654.9844),
                    (516.3281, 608.4531);
                line_to (465.96875, 627.4375);
                cubic_to
                    (764.5156, 647.15625),
                    (650.3594, 786.28125),
                    (497.21875, 868.7344);
                cubic_to
                    (512.40625, 881.375),
                    (538.21875, 907.21875),
                    (548.46875, 921.03125);
                cubic_to
                    (702.34375, 827.28125),
                    (823.6094, 679.34375),
                    (889.0469, 482.875);
                line_to (821.65625, 467.89063);
                cubic_to
                    (247.4375, 670.78125),
                    (219.32813, 735.1719),
                    (182.84375, 780.7031);
                cubic_to
                    (196.85938, 787.8281),
                    (221.5625, 803.46875),
                    (232.40625, 812.78125);
                cubic_to
                    (268.07813, 764.6719),
                    (302.73438, 691.15625),
                    (322.78125, 620.4375);
                line_to (265.3125, 607.78125);
                line_to (158.29688, 426.0625);
                line_to (936.96875, 426.0625);
                line_to (936.96875, 364.01563);
                line_to (158.29688, 364.01563);
                line_to (230.59375, 559.0);
                line_to (563.21875, 559.0);
                line_to (563.21875, 500.29688);
                line_to (230.59375, 500.29688);
                line_to (61.1875, 272.15625);
                line_to (941.7969, 272.15625);
                line_to (941.7969, 208.9375);
                line_to (61.1875, 208.9375);
                line_to (518.2969, 126.71875);
                line_to (846.0469, 126.71875);
                line_to (846.0469, 69.96875);
                line_to (518.2969, 69.96875);
                line_to (117.953125, 533.75);
                cubic_to
                    (117.953125, 636.2969),
                    (108.296875, 775.65625),
                    (32.40625, 878.25);
                cubic_to
                    (48.0, 886.5469),
                    (77.9375, 909.0),
                    (89.5625, 922.6406);
                cubic_to
                    (171.73438, 812.3594),
                    (187.125, 649.5),
                    (187.125, 534.53125);
                line_to (187.125, 364.01563);
                line_to (117.953125, 364.01563);
                line_to (477.29688, 239.70313);
                line_to (550.5781, 239.70313);
                line_to (550.5781, 0.0);
                line_to (477.29688, 0.0);
                line_to (223.32813, 247.21875);
                line_to (293.89063, 247.21875);
                line_to (293.89063, 44.4375);
                line_to (223.32813, 44.4375);
                line_to (366.23438, 836.2031);
                cubic_to
                    (366.23438, 844.6094),
                    (364.23438, 847.21875),
                    (354.01563, 847.8281);
                cubic_to
                    (344.57813, 848.4375),
                    (316.70313, 848.4375),
                    (282.0625, 847.4375);
                cubic_to
                    (289.96875, 864.6406),
                    (299.46875, 889.15625),
                    (302.64063, 907.53125);
                cubic_to
                    (348.07813, 907.53125),
                    (381.25, 906.75),
                    (402.32813, 896.0625);
                cubic_to
                    (423.78125, 885.75),
                    (428.78125, 868.3281),
                    (428.78125, 836.7656);
                line_to (428.78125, 535.90625);
                line_to (366.23438, 535.90625);
                #move_to (158.29688, 364.01563);
                cubic_to
                    (593.34375, 606.84375),
                    (733.4531, 922.6406),
                    (867.09375, 923.03125);
                cubic_to
                    (927.1406, 923.03125),
                    (953.5, 883.28125),
                    (964.7969, 743.9531);
                cubic_to
                    (946.8281, 737.4375),
                    (923.34375, 724.2344),
                    (907.9375, 710.4219);
                cubic_to
                    (903.3281, 811.9844),
                    (894.09375, 856.1406),
                    (871.6094, 856.1406);
                cubic_to
                    (791.6094, 856.75),
                    (656.1719, 582.2969),
                    (647.125, 259.64063);
                line_to (577.5156, 259.64063);
                cubic_to
                    (748.5781, 325.78125),
                    (802.5469, 365.8125),
                    (828.2656, 393.67188);
                line_to (872.5625, 355.25);
                cubic_to
                    (846.2969, 328.0),
                    (791.53125, 289.75),
                    (747.3906, 263.89063);
                line_to (704.4219, 298.14063);
                cubic_to
                    (496.125, 675.3594),
                    (526.65625, 739.53125),
                    (538.09375, 781.0625);
                line_to (591.0156, 758.28125);
                cubic_to
                    (579.7969, 718.375),
                    (547.46875, 654.9844),
                    (516.3281, 608.4531);
                line_to (465.96875, 627.4375);
                cubic_to
                    (764.5156, 647.15625),
                    (650.3594, 786.28125),
                    (497.21875, 868.7344);
                cubic_to
                    (512.40625, 881.375),
                    (538.21875, 907.21875),
                    (548.46875, 921.03125);
                cubic_to
                    (702.34375, 827.28125),
                    (823.6094, 679.34375),
                    (889.0469, 482.875);
                line_to (821.65625, 467.89063);
                cubic_to
                    (247.4375, 670.78125),
                    (219.32813, 735.1719),
                    (182.84375, 780.7031);
                cubic_to
                    (196.85938, 787.8281),
                    (221.5625, 803.46875),
                    (232.40625, 812.78125);
                cubic_to
                    (268.07813, 764.6719),
                    (302.73438, 691.15625),
                    (322.78125, 620.4375);
                line_to (265.3125, 607.78125);
                line_to (158.29688, 426.0625);
                line_to (936.96875, 426.0625);
                line_to (936.96875, 364.01563);
                line_to (158.29688, 364.01563);
                line_to (230.59375, 559.0);
                line_to (563.21875, 559.0);
                line_to (563.21875, 500.29688);
                line_to (230.59375, 500.29688);
                line_to (61.1875, 272.15625);
                line_to (941.7969, 272.15625);
                line_to (941.7969, 208.9375);
                line_to (61.1875, 208.9375);
                line_to (518.2969, 126.71875);
                line_to (846.0469, 126.71875);
                line_to (846.0469, 69.96875);
                line_to (518.2969, 69.96875);
                line_to (117.953125, 533.75);
                cubic_to
                    (117.953125, 636.2969),
                    (108.296875, 775.65625),
                    (32.40625, 878.25);
                cubic_to
                    (48.0, 886.5469),
                    (77.9375, 909.0),
                    (89.5625, 922.6406);
                cubic_to
                    (171.73438, 812.3594),
                    (187.125, 649.5),
                    (187.125, 534.53125);
                line_to (187.125, 364.01563);
                line_to (117.953125, 364.01563);
                line_to (477.29688, 239.70313);
                line_to (550.5781, 239.70313);
                line_to (550.5781, 0.0);
                line_to (477.29688, 0.0);
                line_to (223.32813, 247.21875);
                line_to (293.89063, 247.21875);
                line_to (293.89063, 44.4375);
                line_to (223.32813, 44.4375);
                line_to (366.23438, 836.2031);
                cubic_to
                    (366.23438, 844.6094),
                    (364.23438, 847.21875),
                    (354.01563, 847.8281);
                cubic_to
                    (344.57813, 848.4375),
                    (316.70313, 848.4375),
                    (282.0625, 847.4375);
                cubic_to
                    (289.96875, 864.6406),
                    (299.46875, 889.15625),
                    (302.64063, 907.53125);
                cubic_to
                    (348.07813, 907.53125),
                    (381.25, 906.75),
                    (402.32813, 896.0625);
                cubic_to
                    (423.78125, 885.75),
                    (428.78125, 868.3281),
                    (428.78125, 836.7656);
                line_to (428.78125, 535.90625);
                line_to (366.23438, 535.90625);
                #move_to (230.59375, 500.29688);
                cubic_to
                    (593.34375, 606.84375),
                    (733.4531, 922.6406),
                    (867.09375, 923.03125);
                cubic_to
                    (927.1406, 923.03125),
                    (953.5, 883.28125),
                    (964.7969, 743.9531);
                cubic_to
                    (946.8281, 737.4375),
                    (923.34375, 724.2344),
                    (907.9375, 710.4219);
                cubic_to
                    (903.3281, 811.9844),
                    (894.09375, 856.1406),
                    (871.6094, 856.1406);
                cubic_to
                    (791.6094, 856.75),
                    (656.1719, 582.2969),
                    (647.125, 259.64063);
                line_to (577.5156, 259.64063);
                cubic_to
                    (748.5781, 325.78125),
                    (802.5469, 365.8125),
                    (828.2656, 393.67188);
                line_to (872.5625, 355.25);
                cubic_to
                    (846.2969, 328.0),
                    (791.53125, 289.75),
                    (747.3906, 263.89063);
                line_to (704.4219, 298.14063);
                cubic_to
                    (496.125, 675.3594),
                    (526.65625, 739.53125),
                    (538.09375, 781.0625);
                line_to (591.0156, 758.28125);
                cubic_to
                    (579.7969, 718.375),
                    (547.46875, 654.9844),
                    (516.3281, 608.4531);
                line_to (465.96875, 627.4375);
                cubic_to
                    (764.5156, 647.15625),
                    (650.3594, 786.28125),
                    (497.21875, 868.7344);
                cubic_to
                    (512.40625, 881.375),
                    (538.21875, 907.21875),
                    (548.46875, 921.03125);
                cubic_to
                    (702.34375, 827.28125),
                    (823.6094, 679.34375),
                    (889.0469, 482.875);
                line_to (821.65625, 467.89063);
                cubic_to
                    (247.4375, 670.78125),
                    (219.32813, 735.1719),
                    (182.84375, 780.7031);
                cubic_to
                    (196.85938, 787.8281),
                    (221.5625, 803.46875),
                    (232.40625, 812.78125);
                cubic_to
                    (268.07813, 764.6719),
                    (302.73438, 691.15625),
                    (322.78125, 620.4375);
                line_to (265.3125, 607.78125);
                line_to (158.29688, 426.0625);
                line_to (936.96875, 426.0625);
                line_to (936.96875, 364.01563);
                line_to (158.29688, 364.01563);
                line_to (230.59375, 559.0);
                line_to (563.21875, 559.0);
                line_to (563.21875, 500.29688);
                line_to (230.59375, 500.29688);
                line_to (61.1875, 272.15625);
                line_to (941.7969, 272.15625);
                line_to (941.7969, 208.9375);
                line_to (61.1875, 208.9375);
                line_to (518.2969, 126.71875);
                line_to (846.0469, 126.71875);
                line_to (846.0469, 69.96875);
                line_to (518.2969, 69.96875);
                line_to (117.953125, 533.75);
                cubic_to
                    (117.953125, 636.2969),
                    (108.296875, 775.65625),
                    (32.40625, 878.25);
                cubic_to
                    (48.0, 886.5469),
                    (77.9375, 909.0),
                    (89.5625, 922.6406);
                cubic_to
                    (171.73438, 812.3594),
                    (187.125, 649.5),
                    (187.125, 534.53125);
                line_to (187.125, 364.01563);
                line_to (117.953125, 364.01563);
                line_to (477.29688, 239.70313);
                line_to (550.5781, 239.70313);
                line_to (550.5781, 0.0);
                line_to (477.29688, 0.0);
                line_to (223.32813, 247.21875);
                line_to (293.89063, 247.21875);
                line_to (293.89063, 44.4375);
                line_to (223.32813, 44.4375);
                line_to (366.23438, 836.2031);
                cubic_to
                    (366.23438, 844.6094),
                    (364.23438, 847.21875),
                    (354.01563, 847.8281);
                cubic_to
                    (344.57813, 848.4375),
                    (316.70313, 848.4375),
                    (282.0625, 847.4375);
                cubic_to
                    (289.96875, 864.6406),
                    (299.46875, 889.15625),
                    (302.64063, 907.53125);
                cubic_to
                    (348.07813, 907.53125),
                    (381.25, 906.75),
                    (402.32813, 896.0625);
                cubic_to
                    (423.78125, 885.75),
                    (428.78125, 868.3281),
                    (428.78125, 836.7656);
                line_to (428.78125, 535.90625);
                line_to (366.23438, 535.90625);
                #move_to (61.1875, 208.9375);
                cubic_to
                    (593.34375, 606.84375),
                    (733.4531, 922.6406),
                    (867.09375, 923.03125);
                cubic_to
                    (927.1406, 923.03125),
                    (953.5, 883.28125),
                    (964.7969, 743.9531);
                cubic_to
                    (946.8281, 737.4375),
                    (923.34375, 724.2344),
                    (907.9375, 710.4219);
                cubic_to
                    (903.3281, 811.9844),
                    (894.09375, 856.1406),
                    (871.6094, 856.1406);
                cubic_to
                    (791.6094, 856.75),
                    (656.1719, 582.2969),
                    (647.125, 259.64063);
                line_to (577.5156, 259.64063);
                cubic_to
                    (748.5781, 325.78125),
                    (802.5469, 365.8125),
                    (828.2656, 393.67188);
                line_to (872.5625, 355.25);
                cubic_to
                    (846.2969, 328.0),
                    (791.53125, 289.75),
                    (747.3906, 263.89063);
                line_to (704.4219, 298.14063);
                cubic_to
                    (496.125, 675.3594),
                    (526.65625, 739.53125),
                    (538.09375, 781.0625);
                line_to (591.0156, 758.28125);
                cubic_to
                    (579.7969, 718.375),
                    (547.46875, 654.9844),
                    (516.3281, 608.4531);
                line_to (465.96875, 627.4375);
                cubic_to
                    (764.5156, 647.15625),
                    (650.3594, 786.28125),
                    (497.21875, 868.7344);
                cubic_to
                    (512.40625, 881.375),
                    (538.21875, 907.21875),
                    (548.46875, 921.03125);
                cubic_to
                    (702.34375, 827.28125),
                    (823.6094, 679.34375),
                    (889.0469, 482.875);
                line_to (821.65625, 467.89063);
                cubic_to
                    (247.4375, 670.78125),
                    (219.32813, 735.1719),
                    (182.84375, 780.7031);
                cubic_to
                    (196.85938, 787.8281),
                    (221.5625, 803.46875),
                    (232.40625, 812.78125);
                cubic_to
                    (268.07813, 764.6719),
                    (302.73438, 691.15625),
                    (322.78125, 620.4375);
                line_to (265.3125, 607.78125);
                line_to (158.29688, 426.0625);
                line_to (936.96875, 426.0625);
                line_to (936.96875, 364.01563);
                line_to (158.29688, 364.01563);
                line_to (230.59375, 559.0);
                line_to (563.21875, 559.0);
                line_to (563.21875, 500.29688);
                line_to (230.59375, 500.29688);
                line_to (61.1875, 272.15625);
                line_to (941.7969, 272.15625);
                line_to (941.7969, 208.9375);
                line_to (61.1875, 208.9375);
                line_to (518.2969, 126.71875);
                line_to (846.0469, 126.71875);
                line_to (846.0469, 69.96875);
                line_to (518.2969, 69.96875);
                line_to (117.953125, 533.75);
                cubic_to
                    (117.953125, 636.2969),
                    (108.296875, 775.65625),
                    (32.40625, 878.25);
                cubic_to
                    (48.0, 886.5469),
                    (77.9375, 909.0),
                    (89.5625, 922.6406);
                cubic_to
                    (171.73438, 812.3594),
                    (187.125, 649.5),
                    (187.125, 534.53125);
                line_to (187.125, 364.01563);
                line_to (117.953125, 364.01563);
                line_to (477.29688, 239.70313);
                line_to (550.5781, 239.70313);
                line_to (550.5781, 0.0);
                line_to (477.29688, 0.0);
                line_to (223.32813, 247.21875);
                line_to (293.89063, 247.21875);
                line_to (293.89063, 44.4375);
                line_to (223.32813, 44.4375);
                line_to (366.23438, 836.2031);
                cubic_to
                    (366.23438, 844.6094),
                    (364.23438, 847.21875),
                    (354.01563, 847.8281);
                cubic_to
                    (344.57813, 848.4375),
                    (316.70313, 848.4375),
                    (282.0625, 847.4375);
                cubic_to
                    (289.96875, 864.6406),
                    (299.46875, 889.15625),
                    (302.64063, 907.53125);
                cubic_to
                    (348.07813, 907.53125),
                    (381.25, 906.75),
                    (402.32813, 896.0625);
                cubic_to
                    (423.78125, 885.75),
                    (428.78125, 868.3281),
                    (428.78125, 836.7656);
                line_to (428.78125, 535.90625);
                line_to (366.23438, 535.90625);
                #move_to (518.2969, 69.96875);
                cubic_to
                    (593.34375, 606.84375),
                    (733.4531, 922.6406),
                    (867.09375, 923.03125);
                cubic_to
                    (927.1406, 923.03125),
                    (953.5, 883.28125),
                    (964.7969, 743.9531);
                cubic_to
                    (946.8281, 737.4375),
                    (923.34375, 724.2344),
                    (907.9375, 710.4219);
                cubic_to
                    (903.3281, 811.9844),
                    (894.09375, 856.1406),
                    (871.6094, 856.1406);
                cubic_to
                    (791.6094, 856.75),
                    (656.1719, 582.2969),
                    (647.125, 259.64063);
                line_to (577.5156, 259.64063);
                cubic_to
                    (748.5781, 325.78125),
                    (802.5469, 365.8125),
                    (828.2656, 393.67188);
                line_to (872.5625, 355.25);
                cubic_to
                    (846.2969, 328.0),
                    (791.53125, 289.75),
                    (747.3906, 263.89063);
                line_to (704.4219, 298.14063);
                cubic_to
                    (496.125, 675.3594),
                    (526.65625, 739.53125),
                    (538.09375, 781.0625);
                line_to (591.0156, 758.28125);
                cubic_to
                    (579.7969, 718.375),
                    (547.46875, 654.9844),
                    (516.3281, 608.4531);
                line_to (465.96875, 627.4375);
                cubic_to
                    (764.5156, 647.15625),
                    (650.3594, 786.28125),
                    (497.21875, 868.7344);
                cubic_to
                    (512.40625, 881.375),
                    (538.21875, 907.21875),
                    (548.46875, 921.03125);
                cubic_to
                    (702.34375, 827.28125),
                    (823.6094, 679.34375),
                    (889.0469, 482.875);
                line_to (821.65625, 467.89063);
                cubic_to
                    (247.4375, 670.78125),
                    (219.32813, 735.1719),
                    (182.84375, 780.7031);
                cubic_to
                    (196.85938, 787.8281),
                    (221.5625, 803.46875),
                    (232.40625, 812.78125);
                cubic_to
                    (268.07813, 764.6719),
                    (302.73438, 691.15625),
                    (322.78125, 620.4375);
                line_to (265.3125, 607.78125);
                line_to (158.29688, 426.0625);
                line_to (936.96875, 426.0625);
                line_to (936.96875, 364.01563);
                line_to (158.29688, 364.01563);
                line_to (230.59375, 559.0);
                line_to (563.21875, 559.0);
                line_to (563.21875, 500.29688);
                line_to (230.59375, 500.29688);
                line_to (61.1875, 272.15625);
                line_to (941.7969, 272.15625);
                line_to (941.7969, 208.9375);
                line_to (61.1875, 208.9375);
                line_to (518.2969, 126.71875);
                line_to (846.0469, 126.71875);
                line_to (846.0469, 69.96875);
                line_to (518.2969, 69.96875);
                line_to (117.953125, 533.75);
                cubic_to
                    (117.953125, 636.2969),
                    (108.296875, 775.65625),
                    (32.40625, 878.25);
                cubic_to
                    (48.0, 886.5469),
                    (77.9375, 909.0),
                    (89.5625, 922.6406);
                cubic_to
                    (171.73438, 812.3594),
                    (187.125, 649.5),
                    (187.125, 534.53125);
                line_to (187.125, 364.01563);
                line_to (117.953125, 364.01563);
                line_to (477.29688, 239.70313);
                line_to (550.5781, 239.70313);
                line_to (550.5781, 0.0);
                line_to (477.29688, 0.0);
                line_to (223.32813, 247.21875);
                line_to (293.89063, 247.21875);
                line_to (293.89063, 44.4375);
                line_to (223.32813, 44.4375);
                line_to (366.23438, 836.2031);
                cubic_to
                    (366.23438, 844.6094),
                    (364.23438, 847.21875),
                    (354.01563, 847.8281);
                cubic_to
                    (344.57813, 848.4375),
                    (316.70313, 848.4375),
                    (282.0625, 847.4375);
                cubic_to
                    (289.96875, 864.6406),
                    (299.46875, 889.15625),
                    (302.64063, 907.53125);
                cubic_to
                    (348.07813, 907.53125),
                    (381.25, 906.75),
                    (402.32813, 896.0625);
                cubic_to
                    (423.78125, 885.75),
                    (428.78125, 868.3281),
                    (428.78125, 836.7656);
                line_to (428.78125, 535.90625);
                line_to (366.23438, 535.90625);
                #move_to (117.953125, 364.01563);
                cubic_to
                    (593.34375, 606.84375),
                    (733.4531, 922.6406),
                    (867.09375, 923.03125);
                cubic_to
                    (927.1406, 923.03125),
                    (953.5, 883.28125),
                    (964.7969, 743.9531);
                cubic_to
                    (946.8281, 737.4375),
                    (923.34375, 724.2344),
                    (907.9375, 710.4219);
                cubic_to
                    (903.3281, 811.9844),
                    (894.09375, 856.1406),
                    (871.6094, 856.1406);
                cubic_to
                    (791.6094, 856.75),
                    (656.1719, 582.2969),
                    (647.125, 259.64063);
                line_to (577.5156, 259.64063);
                cubic_to
                    (748.5781, 325.78125),
                    (802.5469, 365.8125),
                    (828.2656, 393.67188);
                line_to (872.5625, 355.25);
                cubic_to
                    (846.2969, 328.0),
                    (791.53125, 289.75),
                    (747.3906, 263.89063);
                line_to (704.4219, 298.14063);
                cubic_to
                    (496.125, 675.3594),
                    (526.65625, 739.53125),
                    (538.09375, 781.0625);
                line_to (591.0156, 758.28125);
                cubic_to
                    (579.7969, 718.375),
                    (547.46875, 654.9844),
                    (516.3281, 608.4531);
                line_to (465.96875, 627.4375);
                cubic_to
                    (764.5156, 647.15625),
                    (650.3594, 786.28125),
                    (497.21875, 868.7344);
                cubic_to
                    (512.40625, 881.375),
                    (538.21875, 907.21875),
                    (548.46875, 921.03125);
                cubic_to
                    (702.34375, 827.28125),
                    (823.6094, 679.34375),
                    (889.0469, 482.875);
                line_to (821.65625, 467.89063);
                cubic_to
                    (247.4375, 670.78125),
                    (219.32813, 735.1719),
                    (182.84375, 780.7031);
                cubic_to
                    (196.85938, 787.8281),
                    (221.5625, 803.46875),
                    (232.40625, 812.78125);
                cubic_to
                    (268.07813, 764.6719),
                    (302.73438, 691.15625),
                    (322.78125, 620.4375);
                line_to (265.3125, 607.78125);
                line_to (158.29688, 426.0625);
                line_to (936.96875, 426.0625);
                line_to (936.96875, 364.01563);
                line_to (158.29688, 364.01563);
                line_to (230.59375, 559.0);
                line_to (563.21875, 559.0);
                line_to (563.21875, 500.29688);
                line_to (230.59375, 500.29688);
                line_to (61.1875, 272.15625);
                line_to (941.7969, 272.15625);
                line_to (941.7969, 208.9375);
                line_to (61.1875, 208.9375);
                line_to (518.2969, 126.71875);
                line_to (846.0469, 126.71875);
                line_to (846.0469, 69.96875);
                line_to (518.2969, 69.96875);
                line_to (117.953125, 533.75);
                cubic_to
                    (117.953125, 636.2969),
                    (108.296875, 775.65625),
                    (32.40625, 878.25);
                cubic_to
                    (48.0, 886.5469),
                    (77.9375, 909.0),
                    (89.5625, 922.6406);
                cubic_to
                    (171.73438, 812.3594),
                    (187.125, 649.5),
                    (187.125, 534.53125);
                line_to (187.125, 364.01563);
                line_to (117.953125, 364.01563);
                line_to (477.29688, 239.70313);
                line_to (550.5781, 239.70313);
                line_to (550.5781, 0.0);
                line_to (477.29688, 0.0);
                line_to (223.32813, 247.21875);
                line_to (293.89063, 247.21875);
                line_to (293.89063, 44.4375);
                line_to (223.32813, 44.4375);
                line_to (366.23438, 836.2031);
                cubic_to
                    (366.23438, 844.6094),
                    (364.23438, 847.21875),
                    (354.01563, 847.8281);
                cubic_to
                    (344.57813, 848.4375),
                    (316.70313, 848.4375),
                    (282.0625, 847.4375);
                cubic_to
                    (289.96875, 864.6406),
                    (299.46875, 889.15625),
                    (302.64063, 907.53125);
                cubic_to
                    (348.07813, 907.53125),
                    (381.25, 906.75),
                    (402.32813, 896.0625);
                cubic_to
                    (423.78125, 885.75),
                    (428.78125, 868.3281),
                    (428.78125, 836.7656);
                line_to (428.78125, 535.90625);
                line_to (366.23438, 535.90625);
                #move_to (477.29688, 0.0);
                cubic_to
                    (593.34375, 606.84375),
                    (733.4531, 922.6406),
                    (867.09375, 923.03125);
                cubic_to
                    (927.1406, 923.03125),
                    (953.5, 883.28125),
                    (964.7969, 743.9531);
                cubic_to
                    (946.8281, 737.4375),
                    (923.34375, 724.2344),
                    (907.9375, 710.4219);
                cubic_to
                    (903.3281, 811.9844),
                    (894.09375, 856.1406),
                    (871.6094, 856.1406);
                cubic_to
                    (791.6094, 856.75),
                    (656.1719, 582.2969),
                    (647.125, 259.64063);
                line_to (577.5156, 259.64063);
                cubic_to
                    (748.5781, 325.78125),
                    (802.5469, 365.8125),
                    (828.2656, 393.67188);
                line_to (872.5625, 355.25);
                cubic_to
                    (846.2969, 328.0),
                    (791.53125, 289.75),
                    (747.3906, 263.89063);
                line_to (704.4219, 298.14063);
                cubic_to
                    (496.125, 675.3594),
                    (526.65625, 739.53125),
                    (538.09375, 781.0625);
                line_to (591.0156, 758.28125);
                cubic_to
                    (579.7969, 718.375),
                    (547.46875, 654.9844),
                    (516.3281, 608.4531);
                line_to (465.96875, 627.4375);
                cubic_to
                    (764.5156, 647.15625),
                    (650.3594, 786.28125),
                    (497.21875, 868.7344);
                cubic_to
                    (512.40625, 881.375),
                    (538.21875, 907.21875),
                    (548.46875, 921.03125);
                cubic_to
                    (702.34375, 827.28125),
                    (823.6094, 679.34375),
                    (889.0469, 482.875);
                line_to (821.65625, 467.89063);
                cubic_to
                    (247.4375, 670.78125),
                    (219.32813, 735.1719),
                    (182.84375, 780.7031);
                cubic_to
                    (196.85938, 787.8281),
                    (221.5625, 803.46875),
                    (232.40625, 812.78125);
                cubic_to
                    (268.07813, 764.6719),
                    (302.73438, 691.15625),
                    (322.78125, 620.4375);
                line_to (265.3125, 607.78125);
                line_to (158.29688, 426.0625);
                line_to (936.96875, 426.0625);
                line_to (936.96875, 364.01563);
                line_to (158.29688, 364.01563);
                line_to (230.59375, 559.0);
                line_to (563.21875, 559.0);
                line_to (563.21875, 500.29688);
                line_to (230.59375, 500.29688);
                line_to (61.1875, 272.15625);
                line_to (941.7969, 272.15625);
                line_to (941.7969, 208.9375);
                line_to (61.1875, 208.9375);
                line_to (518.2969, 126.71875);
                line_to (846.0469, 126.71875);
                line_to (846.0469, 69.96875);
                line_to (518.2969, 69.96875);
                line_to (117.953125, 533.75);
                cubic_to
                    (117.953125, 636.2969),
                    (108.296875, 775.65625),
                    (32.40625, 878.25);
                cubic_to
                    (48.0, 886.5469),
                    (77.9375, 909.0),
                    (89.5625, 922.6406);
                cubic_to
                    (171.73438, 812.3594),
                    (187.125, 649.5),
                    (187.125, 534.53125);
                line_to (187.125, 364.01563);
                line_to (117.953125, 364.01563);
                line_to (477.29688, 239.70313);
                line_to (550.5781, 239.70313);
                line_to (550.5781, 0.0);
                line_to (477.29688, 0.0);
                line_to (223.32813, 247.21875);
                line_to (293.89063, 247.21875);
                line_to (293.89063, 44.4375);
                line_to (223.32813, 44.4375);
                line_to (366.23438, 836.2031);
                cubic_to
                    (366.23438, 844.6094),
                    (364.23438, 847.21875),
                    (354.01563, 847.8281);
                cubic_to
                    (344.57813, 848.4375),
                    (316.70313, 848.4375),
                    (282.0625, 847.4375);
                cubic_to
                    (289.96875, 864.6406),
                    (299.46875, 889.15625),
                    (302.64063, 907.53125);
                cubic_to
                    (348.07813, 907.53125),
                    (381.25, 906.75),
                    (402.32813, 896.0625);
                cubic_to
                    (423.78125, 885.75),
                    (428.78125, 868.3281),
                    (428.78125, 836.7656);
                line_to (428.78125, 535.90625);
                line_to (366.23438, 535.90625);
                #move_to (223.32813, 44.4375);
                cubic_to
                    (593.34375, 606.84375),
                    (733.4531, 922.6406),
                    (867.09375, 923.03125);
                cubic_to
                    (927.1406, 923.03125),
                    (953.5, 883.28125),
                    (964.7969, 743.9531);
                cubic_to
                    (946.8281, 737.4375),
                    (923.34375, 724.2344),
                    (907.9375, 710.4219);
                cubic_to
                    (903.3281, 811.9844),
                    (894.09375, 856.1406),
                    (871.6094, 856.1406);
                cubic_to
                    (791.6094, 856.75),
                    (656.1719, 582.2969),
                    (647.125, 259.64063);
                line_to (577.5156, 259.64063);
                cubic_to
                    (748.5781, 325.78125),
                    (802.5469, 365.8125),
                    (828.2656, 393.67188);
                line_to (872.5625, 355.25);
                cubic_to
                    (846.2969, 328.0),
                    (791.53125, 289.75),
                    (747.3906, 263.89063);
                line_to (704.4219, 298.14063);
                cubic_to
                    (496.125, 675.3594),
                    (526.65625, 739.53125),
                    (538.09375, 781.0625);
                line_to (591.0156, 758.28125);
                cubic_to
                    (579.7969, 718.375),
                    (547.46875, 654.9844),
                    (516.3281, 608.4531);
                line_to (465.96875, 627.4375);
                cubic_to
                    (764.5156, 647.15625),
                    (650.3594, 786.28125),
                    (497.21875, 868.7344);
                cubic_to
                    (512.40625, 881.375),
                    (538.21875, 907.21875),
                    (548.46875, 921.03125);
                cubic_to
                    (702.34375, 827.28125),
                    (823.6094, 679.34375),
                    (889.0469, 482.875);
                line_to (821.65625, 467.89063);
                cubic_to
                    (247.4375, 670.78125),
                    (219.32813, 735.1719),
                    (182.84375, 780.7031);
                cubic_to
                    (196.85938, 787.8281),
                    (221.5625, 803.46875),
                    (232.40625, 812.78125);
                cubic_to
                    (268.07813, 764.6719),
                    (302.73438, 691.15625),
                    (322.78125, 620.4375);
                line_to (265.3125, 607.78125);
                line_to (158.29688, 426.0625);
                line_to (936.96875, 426.0625);
                line_to (936.96875, 364.01563);
                line_to (158.29688, 364.01563);
                line_to (230.59375, 559.0);
                line_to (563.21875, 559.0);
                line_to (563.21875, 500.29688);
                line_to (230.59375, 500.29688);
                line_to (61.1875, 272.15625);
                line_to (941.7969, 272.15625);
                line_to (941.7969, 208.9375);
                line_to (61.1875, 208.9375);
                line_to (518.2969, 126.71875);
                line_to (846.0469, 126.71875);
                line_to (846.0469, 69.96875);
                line_to (518.2969, 69.96875);
                line_to (117.953125, 533.75);
                cubic_to
                    (117.953125, 636.2969),
                    (108.296875, 775.65625),
                    (32.40625, 878.25);
                cubic_to
                    (48.0, 886.5469),
                    (77.9375, 909.0),
                    (89.5625, 922.6406);
                cubic_to
                    (171.73438, 812.3594),
                    (187.125, 649.5),
                    (187.125, 534.53125);
                line_to (187.125, 364.01563);
                line_to (117.953125, 364.01563);
                line_to (477.29688, 239.70313);
                line_to (550.5781, 239.70313);
                line_to (550.5781, 0.0);
                line_to (477.29688, 0.0);
                line_to (223.32813, 247.21875);
                line_to (293.89063, 247.21875);
                line_to (293.89063, 44.4375);
                line_to (223.32813, 44.4375);
                line_to (366.23438, 836.2031);
                cubic_to
                    (366.23438, 844.6094),
                    (364.23438, 847.21875),
                    (354.01563, 847.8281);
                cubic_to
                    (344.57813, 848.4375),
                    (316.70313, 848.4375),
                    (282.0625, 847.4375);
                cubic_to
                    (289.96875, 864.6406),
                    (299.46875, 889.15625),
                    (302.64063, 907.53125);
                cubic_to
                    (348.07813, 907.53125),
                    (381.25, 906.75),
                    (402.32813, 896.0625);
                cubic_to
                    (423.78125, 885.75),
                    (428.78125, 868.3281),
                    (428.78125, 836.7656);
                line_to (428.78125, 535.90625);
                line_to (366.23438, 535.90625);
                #move_to (366.23438, 535.90625);
                cubic_to
                    (593.34375, 606.84375),
                    (733.4531, 922.6406),
                    (867.09375, 923.03125);
                cubic_to
                    (927.1406, 923.03125),
                    (953.5, 883.28125),
                    (964.7969, 743.9531);
                cubic_to
                    (946.8281, 737.4375),
                    (923.34375, 724.2344),
                    (907.9375, 710.4219);
                cubic_to
                    (903.3281, 811.9844),
                    (894.09375, 856.1406),
                    (871.6094, 856.1406);
                cubic_to
                    (791.6094, 856.75),
                    (656.1719, 582.2969),
                    (647.125, 259.64063);
                line_to (577.5156, 259.64063);
                cubic_to
                    (748.5781, 325.78125),
                    (802.5469, 365.8125),
                    (828.2656, 393.67188);
                line_to (872.5625, 355.25);
                cubic_to
                    (846.2969, 328.0),
                    (791.53125, 289.75),
                    (747.3906, 263.89063);
                line_to (704.4219, 298.14063);
                cubic_to
                    (496.125, 675.3594),
                    (526.65625, 739.53125),
                    (538.09375, 781.0625);
                line_to (591.0156, 758.28125);
                cubic_to
                    (579.7969, 718.375),
                    (547.46875, 654.9844),
                    (516.3281, 608.4531);
                line_to (465.96875, 627.4375);
                cubic_to
                    (764.5156, 647.15625),
                    (650.3594, 786.28125),
                    (497.21875, 868.7344);
                cubic_to
                    (512.40625, 881.375),
                    (538.21875, 907.21875),
                    (548.46875, 921.03125);
                cubic_to
                    (702.34375, 827.28125),
                    (823.6094, 679.34375),
                    (889.0469, 482.875);
                line_to (821.65625, 467.89063);
                cubic_to
                    (247.4375, 670.78125),
                    (219.32813, 735.1719),
                    (182.84375, 780.7031);
                cubic_to
                    (196.85938, 787.8281),
                    (221.5625, 803.46875),
                    (232.40625, 812.78125);
                cubic_to
                    (268.07813, 764.6719),
                    (302.73438, 691.15625),
                    (322.78125, 620.4375);
                line_to (265.3125, 607.78125);
                line_to (158.29688, 426.0625);
                line_to (936.96875, 426.0625);
                line_to (936.96875, 364.01563);
                line_to (158.29688, 364.01563);
                line_to (230.59375, 559.0);
                line_to (563.21875, 559.0);
                line_to (563.21875, 500.29688);
                line_to (230.59375, 500.29688);
                line_to (61.1875, 272.15625);
                line_to (941.7969, 272.15625);
                line_to (941.7969, 208.9375);
                line_to (61.1875, 208.9375);
                line_to (518.2969, 126.71875);
                line_to (846.0469, 126.71875);
                line_to (846.0469, 69.96875);
                line_to (518.2969, 69.96875);
                line_to (117.953125, 533.75);
                cubic_to
                    (117.953125, 636.2969),
                    (108.296875, 775.65625),
                    (32.40625, 878.25);
                cubic_to
                    (48.0, 886.5469),
                    (77.9375, 909.0),
                    (89.5625, 922.6406);
                cubic_to
                    (171.73438, 812.3594),
                    (187.125, 649.5),
                    (187.125, 534.53125);
                line_to (187.125, 364.01563);
                line_to (117.953125, 364.01563);
                line_to (477.29688, 239.70313);
                line_to (550.5781, 239.70313);
                line_to (550.5781, 0.0);
                line_to (477.29688, 0.0);
                line_to (223.32813, 247.21875);
                line_to (293.89063, 247.21875);
                line_to (293.89063, 44.4375);
                line_to (223.32813, 44.4375);
                line_to (366.23438, 836.2031);
                cubic_to
                    (366.23438, 844.6094),
                    (364.23438, 847.21875),
                    (354.01563, 847.8281);
                cubic_to
                    (344.57813, 848.4375),
                    (316.70313, 848.4375),
                    (282.0625, 847.4375);
                cubic_to
                    (289.96875, 864.6406),
                    (299.46875, 889.15625),
                    (302.64063, 907.53125);
                cubic_to
                    (348.07813, 907.53125),
                    (381.25, 906.75),
                    (402.32813, 896.0625);
                cubic_to
                    (423.78125, 885.75),
                    (428.78125, 868.3281),
                    (428.78125, 836.7656);
                line_to (428.78125, 535.90625);
                line_to (366.23438, 535.90625);
    ];

    fn bench_outline_strip(outline: &StaticOutline<f32>, scale: f32) {
        let mut rasterizer = StripRasterizer::new();
        rasterizer.add_outline(
            outline
                .events()
                .map(|event| event.map(|p| Point2::new(p.x * scale, p.y * scale))),
        );

        let strips = rasterizer.rasterize();
        std::hint::black_box(strips);
    }

    fn bench_outline_strip2(outline: &StaticOutline<f32>, scale: f32) {
        let mut rasterizer = StripRasterizer {
            tiles: Vec::new(),
            tile_rasterizer: Box::new(super::tile::GenericTileRasterizer::new()),
        };
        rasterizer.add_outline(
            outline
                .events()
                .map(|event| event.map(|p| Point2::new(p.x * scale, p.y * scale))),
        );

        let strips = rasterizer.rasterize();
        std::hint::black_box(strips);
    }

    fn bench_outline_glyph(outline: &StaticOutline<f32>, scale: f32) {
        let sizef = Vec2::new(
            outline.control_box().max.x * scale,
            outline.control_box().max.y * scale,
        );
        let size = Vec2::new(sizef.x.ceil() as u32 + 1, sizef.y.ceil() as u32);
        let mut rasterizer = GlyphRasterizer::new();
        rasterizer.reset(size);
        rasterizer.add_outline(
            outline
                .events()
                .map(|event| event.map(|p| Point2::new(p.x * scale, p.y * scale))),
        );

        let mut spans = Vec::new();

        rasterizer.rasterize(|y, xs, v| {
            spans.push((y, xs, v));
        });

        std::hint::black_box(spans);
    }

    const SCALE: f32 = 1.0;

    extern crate test;
    #[bench]
    fn xda(bencher: &mut test::Bencher) {
        bencher.iter(|| {
            bench_outline_glyph(&SAI2, SCALE);
        });
    }

    #[bench]
    fn xds_native(bencher: &mut test::Bencher) {
        bencher.iter(|| {
            bench_outline_strip(&SAI2, SCALE);
        });
    }

    #[bench]
    fn xds_generic(bencher: &mut test::Bencher) {
        bencher.iter(|| {
            bench_outline_strip2(&SAI2, SCALE);
        });
    }
}
