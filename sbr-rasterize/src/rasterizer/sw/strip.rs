use std::mem::MaybeUninit;

use util::math::{Fixed, FloatOutlineIterExt, I16Dot16, OutlineEvent, Point2, Point2f, Vec2};

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

fn tile_to_coord(v: i16) -> I16Dot16 {
    I16Dot16::from_raw(i32::from(v) << 18)
}

fn to_tile_fixed(v: I16Dot16) -> U2Dot14 {
    U2Dot14::from_raw((v.into_raw() >> 2).clamp(0, i32::from(U2Dot14::MAX.into_raw())) as u16)
}

fn from_tile_fixed(v: U2Dot14) -> I16Dot16 {
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

    pub fn stroke_polyline(&mut self, mut points: impl Iterator<Item = Point2f>, half_width: f32) {
        let Some(mut current) = points.next() else {
            return;
        };

        let Some(mut next) = points.next() else {
            return;
        };

        let mut prev_offset = None;
        loop {
            let normal = (next - current).normal().normalize();
            let offset = normal * half_width;

            if let Some(prev_offset) = prev_offset {
                self.process_linef(current - prev_offset, current - offset);
                self.process_linef(current + offset, current + prev_offset);
            } else {
                self.process_linef(current + offset, current - offset);
            }

            self.process_linef(current - offset, next - offset);
            self.process_linef(next + offset, current + offset);

            current = next;
            next = match points.next() {
                Some(next_next) => next_next,
                None => {
                    self.process_linef(next - offset, next + offset);
                    break;
                }
            };
            prev_offset = Some(offset);
        }
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
        tile_x: i16,
        end_tile_x: i16,
        tile_y: u16,
        mut bottom_inner_x: I16Dot16,
        mut bottom_inner_y: U2Dot14,
        mut top_inner_x: I16Dot16,
        mut top_inner_y: U2Dot14,
        winding: Winding,
    ) {
        debug_assert!(bottom_inner_y <= top_inner_y);

        let (pos_x, width) = if tile_x <= end_tile_x {
            debug_assert!(top_inner_x >= 0);
            (tile_x, end_tile_x - tile_x + 1)
        } else {
            debug_assert!(top_inner_x <= 0);
            let o = I16Dot16::new(4 * i32::from(tile_x - end_tile_x));
            bottom_inner_x += o;
            top_inner_x += o;
            (end_tile_x, tile_x - end_tile_x + 1)
        };

        let upos_x = if let Ok(upos_x) = u16::try_from(pos_x) {
            upos_x
        } else {
            // If `pos_x < 0` then one or both of this line's points is to the left of
            // the y-axis and hence our canvas. This means we need to clip it and insert
            // a vertical line along the y-axis for the clipped part.
            let (left_fill_bottom, left_fill_top) = if pos_x + width <= 0 {
                // If `pos_x + width <= 0` then both of our points are left of the y-axis.
                // This means we can just throw away the whole line.
                let prev_top = top_inner_y;
                top_inner_y = bottom_inner_y;
                (bottom_inner_y, prev_top)
            } else {
                // Otherwise *only one* of the points is to the left of the y-axis.
                // This is trickier since we need to actually split it into a vertical
                // part and a remainder.
                let dy = (from_tile_fixed(top_inner_y) - from_tile_fixed(bottom_inner_y))
                    / (top_inner_x - bottom_inner_x);
                let bottom_abs_x = bottom_inner_x + i32::from(pos_x) * 4;
                let top_abs_x = top_inner_x + i32::from(pos_x) * 4;
                debug_assert!((bottom_abs_x < 0) ^ (top_abs_x < 0));

                if bottom_abs_x < 0 {
                    bottom_inner_x = I16Dot16::ZERO;
                    top_inner_x += i32::from(pos_x) * 4;

                    let prev_y = bottom_inner_y;
                    bottom_inner_y =
                        to_tile_fixed(from_tile_fixed(bottom_inner_y) - (dy * bottom_abs_x));
                    (prev_y, bottom_inner_y)
                } else {
                    top_inner_x = I16Dot16::ZERO;
                    bottom_inner_x += i32::from(pos_x) * 4;

                    let prev_y = top_inner_y;
                    top_inner_y = to_tile_fixed(from_tile_fixed(top_inner_y) - (dy * top_abs_x));
                    (top_inner_y, prev_y)
                }
            };

            debug_assert!(left_fill_bottom <= left_fill_top);

            self.tiles.push(Tile {
                pos: Point2::new(0, tile_y),
                width: 1,
                line: TileLine {
                    bottom_x: I16Dot16::ZERO,
                    bottom_y: left_fill_bottom,
                    top_x: I16Dot16::ZERO,
                    top_y: left_fill_top,
                },
                winding,
            });

            if bottom_inner_y >= top_inner_y {
                return;
            }

            0
        };

        self.tiles.push(Tile {
            pos: Point2::new(upos_x, tile_y),
            width: width as u16,
            line: TileLine {
                bottom_x: bottom_inner_x,
                bottom_y: bottom_inner_y,
                top_x: top_inner_x,
                top_y: top_inner_y,
            },
            winding,
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

        let mut tile_x = (bottom.x.floor_to_inner() >> 2) as i16;
        let mut tile_y = (bottom.y.floor_to_inner() as u32 >> 2) as u16;
        let top_tile_x = (top.x.floor_to_inner() >> 2) as i16;
        let end_tile_y = ((top.y.ceil_to_inner() as u32 - 1) >> 2) as u16;
        let end_inner_y16 = top.y - tile_to_coord(end_tile_y as i16);
        let end_inner_y = to_tile_fixed(end_inner_y16);
        let mut current_inner_x = bottom.x - floor_to_tile(bottom.x.floor());
        // TODO: Fixed::saturating_mul or something
        //       would be nice to be generally more careful about overflows in this file
        let dx4 = I16Dot16::from_raw(I16Dot16::into_raw(dx).saturating_mul(4));

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
            );
            return;
        } else {
            let next_x = current_inner_x + (dx * (I16Dot16::new(4) - bottom_inner_y));
            let next_tile_x = tile_x.wrapping_add((next_x.into_raw() >> 18) as i16);
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
            );
            tile_x = next_tile_x;
            current_inner_x = next_inner_x;
            tile_y += 1;
        }

        while tile_y < end_tile_y {
            let next_x = current_inner_x + dx4;
            let next_tile_x = tile_x.wrapping_add((next_x.into_raw() >> 18) as i16);
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
        );
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Strips {
    strips: Vec<Strip>,
    alpha_buffer: AlphaBuffer,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct AlphaBuffer(Vec<u64>);

impl AlphaBuffer {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn as_u8(&self) -> &[u8] {
        let len8 = self.0.len();
        unsafe { std::slice::from_raw_parts(self.0.as_ptr().cast::<u8>(), len8 << 3) }
    }

    unsafe fn push_strip(&mut self, strip_width: usize, init: impl FnOnce(*mut MaybeUninit<u8>)) {
        let start = self.0.len();
        let len = 2 * strip_width;
        let end = start + len;
        self.0.reserve(len);
        unsafe {
            let ptr = self.0.spare_capacity_mut().as_mut_ptr();
            init(ptr.cast());
            self.0.set_len(end);
        }
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
struct Strip {
    pos: Point2<u16>,
    width: u16,
    fill_previous: [u8; 4],
}

impl Strips {
    pub fn paint_iter(&self) -> StripPaintIter<'_> {
        StripPaintIter {
            iter: self.strips.iter(),
            last_x: u16::MAX,
            alpha_buffer: self.alpha_buffer.as_u8(),
        }
    }

    pub fn paint_to(&self, target: super::RenderTargetView<u8>) {
        self.blend_to(target, |out, value| *out = value)
    }

    pub fn blend_to<P: Copy>(
        &self,
        target: super::RenderTargetView<P>,
        blend_func: impl FnMut(&mut P, u8),
    ) {
        self.blend_to_at(target, blend_func, Vec2::new(0, 0));
    }

    pub fn blend_to_at<P: Copy>(
        &self,
        target: super::RenderTargetView<P>,
        mut blend_func: impl FnMut(&mut P, u8),
        offset: Vec2<i32>,
    ) {
        for op in self.paint_iter() {
            let pos = op.pos();
            let (out_off, out_pos) = {
                let out_sx = pos.x as i32 * 4 + offset.x;
                let out_sy = pos.y as i32 * 4 + offset.y;

                if out_sx <= -4 || out_sy <= -4 {
                    continue;
                }

                let out_offx = -out_sx.min(0) as u32;
                let out_offy = -out_sy.min(0) as u32;
                let out_x = out_sx.max(0) as u32;
                let out_y = out_sy.max(0) as u32;

                if out_x >= target.width || out_y >= target.height {
                    continue;
                }

                (Vec2::new(out_offx, out_offy), Point2::new(out_x, out_y))
            };

            let op_width = op.width() as u32;
            let out_width = (op_width - out_off.x).min(target.width - out_pos.x);
            let out_height = (4 - out_off.y).min(target.height - out_pos.y);
            let mut current_out = unsafe {
                target
                    .buffer
                    .as_mut_ptr()
                    .add(out_pos.y as usize * target.stride as usize + out_pos.x as usize)
            };
            let row_step = (target.stride - out_width) as usize;

            match op {
                StripPaintOp::Copy(op) => {
                    let mut current_src = unsafe {
                        op.buffer
                            .as_ptr()
                            .add(out_off.y as usize * op_width as usize + out_off.x as usize)
                    };
                    let src_row_step = (op_width - out_width) as usize;

                    for _ in 0..out_height {
                        unsafe {
                            for _ in 0..out_width {
                                blend_func(&mut *current_out, current_src.read());
                                current_out = current_out.add(1);
                                current_src = current_src.add(1);
                            }

                            current_out = current_out.wrapping_add(row_step);
                            current_src = current_src.wrapping_add(src_row_step);
                        }
                    }
                }
                StripPaintOp::Fill(op) => {
                    for &alpha in &op.alpha[out_off.y as usize..out_height as usize] {
                        unsafe {
                            for _ in 0..out_width {
                                blend_func(&mut *current_out, alpha);
                                current_out = current_out.add(1);
                            }

                            current_out = current_out.wrapping_add(row_step);
                        }
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
    Fill(StripFillOp<'a>),
}

impl StripPaintOp<'_> {
    #[inline]
    pub fn pos(&self) -> Point2<u16> {
        match self {
            Self::Copy(x) => x.pos,
            Self::Fill(x) => x.pos,
        }
    }

    #[inline]
    pub fn width(&self) -> usize {
        match self {
            StripPaintOp::Copy(x) => x.width().into(),
            StripPaintOp::Fill(x) => usize::from(x.width) * 4,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StripCopyOp<'a> {
    pub pos: Point2<u16>,
    pub buffer: &'a [u8],
}

impl<'a> StripCopyOp<'a> {
    #[inline]
    pub fn height(&self) -> u16 {
        4
    }

    #[inline]
    pub fn width(&self) -> u16 {
        (self.buffer.len() / 4) as u16
    }

    #[inline]
    pub fn to_texture(self) -> super::Texture<'a> {
        super::Texture {
            width: u32::from(self.width()),
            height: u32::from(self.height()),
            data: super::TextureData::BorrowedMono(self.buffer),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StripFillOp<'a> {
    pub pos: Point2<u16>,
    pub width: u16,
    pub alpha: &'a [u8; 4],
}

impl<'a> StripFillOp<'a> {
    #[inline]
    pub fn to_vertical_texture(self) -> super::Texture<'a> {
        super::Texture {
            width: 1,
            height: 4,
            data: super::TextureData::BorrowedMono(self.alpha),
        }
    }
}

impl<'a> Iterator for StripPaintIter<'a> {
    type Item = StripPaintOp<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.iter.as_slice().first()?;

        let last_x = self.last_x;
        self.last_x = next.pos.x + next.width;
        if last_x < next.pos.x && next.fill_previous != [0; 4] {
            return Some(StripPaintOp::Fill(StripFillOp {
                pos: Point2::new(last_x, next.pos.y),
                width: next.pos.x - last_x,
                alpha: &next.fill_previous,
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
                self.tile_rasterizer.reset();
            }

            let strip_width = strip_end - strip_pos.x;
            result.strips.push(Strip {
                pos: strip_pos,
                width: strip_width,
                fill_previous: self.tile_rasterizer.fill_alpha(),
            });

            let strip_tiles = &self.tiles[start..end];
            unsafe {
                result
                    .alpha_buffer
                    .push_strip(usize::from(strip_width), |buffer| {
                        self.tile_rasterizer.rasterize(
                            strip_pos.x,
                            strip_tiles,
                            std::ptr::slice_from_raw_parts_mut(
                                buffer,
                                16 * usize::from(strip_width),
                            ),
                        );
                    });
            }

            last_y = strip_pos.y;
            start = end;
        }

        self.tiles.clear();

        result
    }
}

#[cfg(test)]
mod test {
    use util::{
        make_static_outline,
        math::{Outline, StaticOutline, Vec2},
    };

    use crate::sw::{RenderTargetView, StripRasterizer};

    pub fn compare(size: Vec2<u32>, coverage: &[u8], expected: &[u8]) {
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
        rasterizer.add_outline(outline.iter());

        let strips = rasterizer.rasterize();
        let mut coverage = vec![0; size.x as usize * size.y as usize];
        strips.paint_to(RenderTargetView::new(&mut coverage, size.x, size.y, size.x));

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
            0xFF, 0xFF, 0x7F, 0x00,
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
            0xFF, 0x7F, 0x00,
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

    #[test]
    fn negative_x() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        test_outline(
            &make_static_outline![
                #move_to (-4.0, 2.0);
                line_to (-4.0, 12.0);
                line_to (8.0, 12.0);
                line_to (8.0, 2.0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn negative_xy() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x7F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0xFF, 0x7F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0xFF, 0xFF, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
            0xFF, 0xFF, 0xFF, 0x80, 0x00, 0x00, 0x00, 0x00,
        ];

        test_outline(
            &make_static_outline![
                #move_to (-4.0, -4.0);
                line_to (8.0, -4.0);
                line_to (-4.0, 8.0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn negative_xy_inv() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x7F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0xFF, 0x7F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0xFF, 0xFF, 0x7F, 0x00, 0x00, 0x00, 0x00, 0x00,
            0xFF, 0xFF, 0xFF, 0x80, 0x00, 0x00, 0x00, 0x00,
        ];

        test_outline(
            &make_static_outline![
                #move_to (-4.0, 8.0);
                line_to (8.0, -4.0);
                line_to (-4.0, -4.0);
            ],
            EXPECTED,
        );
    }

    #[test]
    fn negative_xy_mirror() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x00, 0x00, 0x15, 0xAA,
            0x00, 0x55, 0xEA, 0xFF,
            0xAA, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
        ];

        test_outline(
            &make_static_outline![
                #move_to (4.0, -4.0);
                line_to (-8.0, -4.0);
                line_to (4.0, 4.0);
            ],
            EXPECTED,
        );
    }
}
