/// A cell-based analytical glyph rasterizer.
///
/// This is a cell-based rasterizer that should produce correct coverage for
/// planar non-overlapping paths. While overlapping and intersecting paths are
/// in some cases allowed in OpenType fonts, FreeType uses a similiar method
/// that results in similiar inaccuracies (although sometimes they enable 4x
/// multisampling to reduce error).
///
/// In the future I may investigate addressing the "overlapping areas" part of
/// the problem, I have some ideas for how to do that while maybe keeping
/// performance reasonable.
// TODO: Handle segments at x < 0 properly
use std::{marker::PhantomData, num::NonZeroU32, ops::Range};

use util::math::{CubicBezier, I16Dot16, Outline, Point2, Point2f, QuadraticBezier, Vec2, Vec2f};

const QUADRATIC_FLATTEN_TOLERANCE: f32 = 0.2;
const CUBIC_TO_QUADRATIC_TOLERANCE: f32 = 1.0;

#[derive(Debug, Clone, Copy)]
enum Winding {
    CounterClockwise = -1,
    Clockwise = 1,
}

#[derive(Debug, Clone)]
struct Segment {
    top: Point2<I16Dot16>,
    bottom: Point2<I16Dot16>,
    winding: Winding,
    kind: SegmentKind,
}

#[derive(Debug, Clone)]
enum SegmentKind {
    Linear { dx: I16Dot16, dy: I16Dot16 },
}

impl Segment {
    fn new_linear(mut start: Point2<I16Dot16>, mut end: Point2<I16Dot16>) -> Option<Self> {
        let winding = if end.y > start.y {
            std::mem::swap(&mut start, &mut end);
            Winding::Clockwise
        } else if end.y == start.y {
            return None;
        } else {
            Winding::CounterClockwise
        };

        Some(Segment {
            top: start,
            bottom: end,
            winding,
            kind: SegmentKind::Linear {
                dx: (start.x - end.x) / (start.y - end.y),
                // horizontal stepping is skipped in this case so this can be anything
                dy: if start.x == end.x {
                    I16Dot16::ZERO
                } else {
                    (start.y - end.y) / (start.x - end.x)
                },
            },
        })
    }

    fn y_stepper(&self, next_y: I16Dot16) -> SegmentStepper<StepAxisY> {
        let kind = self.kind.clone();
        SegmentStepper {
            current_cross: self.bottom.x,
            next_cross: match kind {
                SegmentKind::Linear { dx, .. } => self.bottom.x + dx * (next_y - self.bottom.y),
            },
            kind,
            _axis: PhantomData,
        }
    }
}

// TODO: It would be nice if this stuff was replaced by a const enum in the future :)
trait StepAxis {
    // True if we're stepping on the x axis, false if we're stepping on the y axis.
    const IS_X_AXIS: bool;
}

struct StepAxisX;

impl StepAxis for StepAxisX {
    const IS_X_AXIS: bool = true;
}

struct StepAxisY;

impl StepAxis for StepAxisY {
    const IS_X_AXIS: bool = false;
}

// The necessity of this structure is questionable since it's really simple once you
// remove the quadratic stepper that was originally here...
#[derive(Debug)]
struct SegmentStepper<A: StepAxis = StepAxisY> {
    current_cross: I16Dot16,
    next_cross: I16Dot16,
    kind: SegmentKind,
    _axis: PhantomData<A>,
}

impl SegmentStepper<StepAxisX> {
    fn new_horizontal(
        start: Point2<I16Dot16>,
        current_y: I16Dot16,
        next_x: I16Dot16,
        kind: SegmentKind,
    ) -> Self {
        Self {
            current_cross: current_y,
            next_cross: {
                match kind {
                    SegmentKind::Linear { dy, .. } => start.y + dy * (next_x - start.x),
                }
            },
            kind,
            _axis: PhantomData,
        }
    }
}

impl<A: StepAxis> SegmentStepper<A> {
    fn advance_current_to_next(&mut self) {
        self.current_cross = self.next_cross;
        match self.kind {
            SegmentKind::Linear { .. } => (),
        }
    }

    fn advance1_inside(&mut self) {
        self.next_cross = match self.kind {
            SegmentKind::Linear { dx, dy } => {
                if A::IS_X_AXIS {
                    self.current_cross + dy
                } else {
                    self.current_cross + dx
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct PixelCell {
    x: u32,

    winding: I16Dot16,
    rcoverage: I16Dot16,

    next: Option<NonZeroU32>,
}

#[derive(Debug)]
pub struct GlyphRasterizer {
    size: Vec2<u32>,
    cells: Vec<PixelCell>,
    first_cell_for_y: Vec<Option<NonZeroU32>>,
}

impl GlyphRasterizer {
    pub fn new() -> Self {
        Self {
            size: Vec2::ZERO,
            cells: Vec::new(),
            first_cell_for_y: Vec::new(),
        }
    }

    fn process_line(&mut self, start: Point2<I16Dot16>, end: Point2<I16Dot16>) {
        if let Some(segment) = Segment::new_linear(start, end) {
            self.add_segment_cells(&segment);
        }
    }

    fn process_linef(&mut self, start: Point2f, end: Point2f) {
        self.process_line(
            Point2::new(start.x.into(), start.y.into()),
            Point2::new(end.x.into(), end.y.into()),
        );
    }

    fn process_quadraticf(&mut self, quadratic: QuadraticBezier<f32>) {
        let mut last = quadratic[0];
        for next in quadratic.flatten(QUADRATIC_FLATTEN_TOLERANCE) {
            self.process_linef(last, next);
            last = next;
        }
    }

    pub fn add_outline_with(&mut self, outline: &Outline, mapper: impl Fn(Point2f) -> Point2f) {
        for contour in outline.iter_contours() {
            for segment in contour {
                match outline.curve_for_segment(segment) {
                    util::math::SegmentCurve::Linear(line) => {
                        self.process_linef(mapper(line[0]), mapper(line[1]));
                    }
                    util::math::SegmentCurve::Quadratic(quadratic) => {
                        let mapped = QuadraticBezier(quadratic.0.map(&mapper));
                        self.process_quadraticf(mapped);
                    }
                    util::math::SegmentCurve::Cubic(cubic) => {
                        let mapped = CubicBezier(cubic.0.map(&mapper));
                        for quadratic in mapped.to_quadratics(CUBIC_TO_QUADRATIC_TOLERANCE) {
                            self.process_quadraticf(quadratic);
                        }
                    }
                }
            }
        }
    }

    pub fn add_outline(&mut self, outline: &Outline, translation: Vec2f) {
        self.add_outline_with(outline, |p| p + translation);
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

    pub fn reset(&mut self, size: Vec2<u32>) {
        self.size = size;
        self.cells.resize(
            1,
            PixelCell {
                x: 0,
                winding: I16Dot16::ZERO,
                rcoverage: I16Dot16::ZERO,
                next: None,
            },
        );
        self.first_cell_for_y.clear();
        self.first_cell_for_y.resize(size.y as usize, None);
    }

    fn insert_cell(&mut self, x: u32, y: u32) -> NonZeroU32 {
        // borrow checker momento (TODO: figure out a way to bypass, or check whether polonius alpha would allow this or smth)
        unsafe {
            self.cells.reserve(1);
            let mut current = &raw mut self.first_cell_for_y[y as usize];
            while let Some(idx) = *current {
                let cell = self.cells.as_mut_ptr().add(idx.get() as usize);
                let cell_x = (*cell).x;
                if cell_x < x {
                    current = &raw mut (*cell).next;
                } else if cell_x == x {
                    return idx;
                } else {
                    break;
                }
            }

            let idx = NonZeroU32::new_unchecked(self.cells.len() as u32);
            self.cells.push(PixelCell {
                x,
                winding: I16Dot16::ZERO,
                rcoverage: I16Dot16::ZERO,
                next: *current,
            });
            current.write(Some(idx));

            idx
        }
    }

    fn add_line_to_cell(
        &mut self,
        x: u32,
        y: u32,
        bottom: Point2<I16Dot16>,
        top: Point2<I16Dot16>,
        winding: Winding,
    ) {
        let idx = self.insert_cell(x, y);
        let cell = &mut self.cells[idx.get() as usize];

        let height = top.y - bottom.y;
        debug_assert!(height >= 0);
        let signed_height = height * (winding as i32);
        cell.winding += signed_height;
        cell.rcoverage += signed_height * (I16Dot16::new(2) - (top.x + bottom.x)) / 2;
    }

    fn add_segment_cells_horizontal(
        &mut self,
        y: u32,
        bottom: Point2<I16Dot16>,
        top: Point2<I16Dot16>,
        segment: &Segment,
        stepper_kind: SegmentKind,
    ) {
        if bottom.x == top.x && bottom.x < self.size.x as i32 {
            self.add_line_to_cell(
                bottom.x.floor_to_inner() as u32,
                y,
                Point2::new(bottom.x.fract(), bottom.y),
                Point2::new(bottom.x.fract(), top.y),
                segment.winding,
            );
            return;
        }

        let (sleft, rleft, rright) = if segment.bottom.x > segment.top.x {
            (segment.top, top, bottom)
        } else {
            (segment.bottom, bottom, top)
        };

        let mut current_x = rleft.x;
        if current_x >= self.size.x as i32 {
            return;
        }

        let mut next_x = rleft.x.floor() + 1;
        if next_x >= rright.x {
            self.add_line_to_cell(
                bottom.x.floor_to_inner() as u32,
                y,
                Point2::new(bottom.x.fract(), bottom.y),
                Point2::new(top.x.fract(), top.y),
                segment.winding,
            );
            return;
        }

        let mut stepper = SegmentStepper::new_horizontal(sleft, rleft.y, next_x, stepper_kind);

        while current_x < self.size.x as i32 {
            let mut pixel_bottom = Point2::new(current_x.fract(), stepper.current_cross);
            let mut pixel_top = Point2::new(next_x - current_x.floor(), stepper.next_cross);

            if pixel_top.y < pixel_bottom.y {
                std::mem::swap(&mut pixel_bottom, &mut pixel_top);
            }

            self.add_line_to_cell(
                current_x.floor_to_inner() as u32,
                y,
                pixel_bottom,
                pixel_top,
                segment.winding,
            );

            current_x = next_x;
            stepper.advance_current_to_next();
            next_x += I16Dot16::ONE;
            if next_x > rright.x {
                if current_x == rright.x {
                    break;
                }

                stepper.next_cross = rright.y;
                next_x = rright.x;
            } else {
                stepper.advance1_inside();
            }
        }
    }

    fn add_segment_cells(&mut self, segment: &Segment) {
        let mut current_y = segment.bottom.y;
        let mut next_y = (current_y.floor() + 1).min(segment.top.y);
        let mut stepper = segment.y_stepper(next_y);

        while current_y < self.size.y as i32 {
            // TODO: instead just skip straight to y=0 above
            if current_y >= 0 {
                self.add_segment_cells_horizontal(
                    current_y.floor_to_inner() as u32,
                    Point2::new(stepper.current_cross, current_y),
                    Point2::new(stepper.next_cross, next_y),
                    segment,
                    stepper.kind.clone(),
                );
            }

            current_y = next_y;
            stepper.advance_current_to_next();
            next_y += I16Dot16::ONE;
            if next_y > segment.top.y {
                if current_y == segment.top.y {
                    break;
                }

                stepper.next_cross = segment.top.x;
                next_y = segment.top.y;
            } else {
                stepper.advance1_inside();
            }
        }
    }

    pub fn rasterize(
        &mut self,
        mut on_span: impl FnMut(/* y: */ u32, /* xs: */ Range<u32>, /* coverage: */ u16),
    ) {
        for y in 0..self.size.y {
            let mut winding = I16Dot16::ZERO;
            let mut last = 0;

            let mut next = self.first_cell_for_y[y as usize];
            while let Some(idx) = next {
                let cell = &self.cells[idx.get() as usize];

                if winding != 0 {
                    on_span(y, last..cell.x, fixed_to_u16(winding.abs()));
                }
                last = cell.x + 1;

                let coverage = winding + cell.rcoverage;
                winding += cell.winding;

                on_span(y, cell.x..last, fixed_to_u16(coverage.abs()));

                next = cell.next;
            }

            if winding != 0 {
                on_span(y, last..self.size.x, fixed_to_u16(winding.abs()));
            }
        }
    }

    pub fn rasterize_to_vec(&mut self, out: &mut Vec<u16>, stride: usize) {
        assert!(stride >= self.size.x as usize);

        out.clear();
        out.resize(stride * self.size.y as usize, 0);

        self.rasterize(|y, xs, coverage| {
            let row = &mut out[y as usize * stride..];
            for x in xs {
                row[x as usize] = coverage;
            }
        });
    }
}

fn fixed_to_u16(value: I16Dot16) -> u16 {
    if value <= 0 {
        0
    } else if value >= 1 {
        u16::MAX
    } else {
        let raw = value.into_raw() as u32;
        (((raw << 16) - raw) >> 16) as u16
    }
}

#[cfg(test)]
mod test {
    use util::math::{I16Dot16, Outline, OutlineBuilder, Point2f, Vec2};

    use crate::sw::GlyphRasterizer;

    fn reference_fixed_to_u16(value: I16Dot16) -> u16 {
        if value <= 0 {
            0
        } else if value >= 1 {
            u16::MAX
        } else {
            (((value.into_raw()) as u64) * u64::from(u16::MAX) / (u64::from(u16::MAX) + 1)) as u16
        }
    }

    #[test]
    fn fixed_to_u16_exhaustive() {
        for r in 0..=I16Dot16::new(1).into_raw() {
            let value = I16Dot16::from_raw(r);
            assert_eq!(super::fixed_to_u16(value), reference_fixed_to_u16(value));
        }
    }

    fn compare(size: Vec2<u32>, coverage: &[u16], expected: &[u8]) {
        let coverage8: Vec<_> = coverage.iter().map(|&v| (v >> 8) as u8).collect();

        let mut matches = true;
        for y in (0..size.y as usize).rev() {
            for x in 0..size.x as usize {
                let exp = expected
                    .get((size.y as usize - y - 1) * size.x as usize + x)
                    .copied()
                    .unwrap_or(0);
                if coverage8[y * size.x as usize + x] != exp {
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

                    let v = coverage8[y * size.x as usize + x];
                    let exp = expected
                        .get((size.y as usize - y - 1) * size.x as usize + x)
                        .copied()
                        .unwrap_or(0);
                    let (pref, suff) = if v == exp {
                        ("", "")
                    } else {
                        ("\x1b[31;1m", "\x1b[0m")
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

    fn test_outline(outline: &Outline, expected: &[u8]) {
        let sizef = outline.control_box().size();
        let size = Vec2::new(sizef.x.ceil() as u32 + 1, sizef.y.ceil() as u32);
        let mut rasterizer = GlyphRasterizer::new();
        rasterizer.reset(size);
        rasterizer.add_outline(outline, Vec2::ZERO);

        let mut coverage = Vec::new();
        rasterizer.rasterize_to_vec(&mut coverage, size.x as usize);

        compare(size, &coverage, expected);
    }

    #[test]
    fn some_lines() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x00, 0x00, 0x00, 0x33, 0xCA, 0x60, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x99, 0xFF, 0xFF, 0xEC, 0x8A, 0x22, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x0C, 0xF3, 0xFF, 0xFF, 0xFF, 0xFF, 0xFD, 0xB5, 0x1C, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x66, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xD3, 0x0F, 0x00, 0x00, 0x00,
            0x00, 0x00, 0xCC, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xBF, 0x07, 0x00, 0x00,
            0x00, 0x33, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xA8, 0x01, 0x00,
            // FIXME: That 0x0E at the end seems kind of suspicous
            //        How can there even be a cell there? rounding error?
            0x00, 0x99, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x0E,
            0x0C, 0xF3, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xF3, 0xC0, 0x89, 0x52, 0x1B, 0x00,
            0x66, 0xFF, 0xFF, 0xFF, 0xFC, 0xD2, 0x9B, 0x64, 0x2D, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00,
            0xB1, 0xAD, 0x76, 0x40, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        test_outline(
            &{
                let mut builder = OutlineBuilder::new();
                builder.move_to(Point2f::new(0.0, 0.0));
                builder.line_to(Point2f::new(4.0, 10.0));
                builder.line_to(Point2f::new(10.0, 7.5));
                builder.line_to(Point2f::new(14.0, 3.0));
                builder.build()
            },
            EXPECTED,
        );
    }

    #[test]
    fn some_quadratics_and_lines() {
        // Looks about right I guess? TODO: Maybe it'd be nice to have these in a simple bitmap format instead?
        // Then you could view them with an image viewer.
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x5D, 0x94, 0xBF, 0xEA, 0xBF, 0x3F, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x33, 0xCC, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xBF, 0x3F, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x69, 0xFC, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x85, 0x00,
            0x00, 0x00, 0x31, 0xF9, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xD9, 0x51, 0x00, 0x00,
            0x00, 0x0B, 0xD8, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xE8, 0x69, 0x04, 0x00, 0x00, 0x00,
            0x00, 0x83, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xA0, 0x0E, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0xDB, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xEA, 0x50, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x2F, 0xFF, 0xFF, 0xFF, 0xFF, 0xCC, 0x66, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x82, 0xFF, 0xF3, 0x99, 0x33, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0xA3, 0x66, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        test_outline(
            &{
                let mut builder = OutlineBuilder::new();
                builder.move_to(Point2f::new(0.0, 0.0));
                builder.quad_to(Point2f::new(2.0, 10.0), Point2f::new(10.0, 10.0));
                builder.line_to(Point2f::new(15.0, 7.5));
                builder.quad_to(Point2f::new(10.0, 5.0), Point2f::new(7.5, 3.0));
                builder.build()
            },
            EXPECTED,
        );
    }

    #[test]
    fn thin_line() {
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x99, 0x00,
            0x99, 0x00,
            0x99, 0x00,
            0x99, 0x00,
            0x99, 0x00,
            0x99, 0x00,
            0x99, 0x00,
            0x99, 0x00,
            0x99, 0x00,
            0x99, 0x00
        ];

        test_outline(
            &{
                let mut builder = OutlineBuilder::new();
                builder.move_to(Point2f::new(0.2, 0.0));
                builder.line_to(Point2f::new(0.2, 10.0));
                builder.line_to(Point2f::new(0.8, 10.0));
                builder.line_to(Point2f::new(0.8, 0.0));
                builder.build()
            },
            EXPECTED,
        );
    }

    // TODO: More complex test cases
}
