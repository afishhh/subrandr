use crate::{
    color::BGRA8,
    math::{I32Fixed, Point2f, Vec2},
    outline::Outline,
};

const POLYGON_RASTERIZER_DEBUG_PRINT: bool = false;

type IFixed18Dot14 = I32Fixed<14>;

#[derive(Debug)]
struct Profile {
    current: IFixed18Dot14,
    step: IFixed18Dot14,
    end_y: u32,
}

#[derive(Debug)]
pub struct NonZeroPolygonRasterizer {
    queue: Vec<(u32, bool, Profile)>,
    left: Vec<Profile>,
    right: Vec<Profile>,
}

impl NonZeroPolygonRasterizer {
    pub const fn new() -> Self {
        Self {
            queue: Vec::new(),
            left: Vec::new(),
            right: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.queue.clear();
        self.left.clear();
        self.right.clear();
    }

    fn add_line(
        &mut self,
        offset: (i32, i32),
        start: &Point2f,
        end: &Point2f,
        invert_winding: bool,
    ) {
        let istart = (
            IFixed18Dot14::from_f32(start.x) + offset.0,
            IFixed18Dot14::from_f32(start.y) + offset.1,
        );
        let iend = (
            IFixed18Dot14::from_f32(end.x) + offset.0,
            IFixed18Dot14::from_f32(end.y) + offset.1,
        );

        let direction = match iend.1.cmp(&istart.1) {
            // Line is going up
            std::cmp::Ordering::Less => false ^ invert_winding,
            // Horizontal line, ignore
            std::cmp::Ordering::Equal => return,
            // Line is going down
            std::cmp::Ordering::Greater => true ^ invert_winding,
        };

        let step = if istart.0 == iend.0 {
            IFixed18Dot14::ZERO
        } else {
            (iend.0 - istart.0) / (iend.1 - istart.1)
        };

        let start_y = istart.1.round_to_inner();
        let mut start_x = istart.0;
        start_x -= (istart.1 - start_y) * step;

        let end_y = iend.1.round_to_inner();
        let mut end_x = iend.0;
        end_x -= (iend.1 - end_y) * step;

        let (mut top_y, mut bottom_y, mut init_x) = if end_y >= start_y {
            (start_y, end_y, start_x)
        } else {
            (end_y, start_y, end_x)
        };

        // FIXME: HACK: This is terrible but I tried everything and only this works
        bottom_y -= 1;
        init_x -= step;

        if top_y < 0 {
            init_x += step * -top_y;
            top_y = 0;
        }

        if top_y > bottom_y {
            return;
        }

        if POLYGON_RASTERIZER_DEBUG_PRINT {
            println!("{start_y} {end_y} {start_x} {end_x}");
            println!("{top_y} {bottom_y}");
            println!(
                "line {start:?} -- {end:?} results in top_y={top_y} direction={:?}",
                step > 0
            );
        }

        self.queue.push((
            top_y as u32,
            direction,
            Profile {
                current: init_x,
                step,
                end_y: bottom_y as u32,
            },
        ));
    }

    pub fn append_polyline(
        &mut self,
        offset: (i32, i32),
        polyline: &[Point2f],
        invert_winding: bool,
    ) {
        if polyline.is_empty() {
            return;
        }

        let mut i = 0;
        while i < polyline.len() - 1 {
            let start = &polyline[i];
            i += 1;
            let end = &polyline[i];
            self.add_line(offset, start, end, invert_winding)
        }

        let last = polyline.last().unwrap();
        if &polyline[0] != last {
            self.add_line(offset, last, &polyline[0], invert_winding)
        }
    }

    fn queue_pop_if(&mut self, cy: u32) -> Option<(u32, bool, Profile)> {
        let &(y, ..) = self.queue.last()?;

        if y <= cy {
            self.queue.pop()
        } else {
            None
        }
    }

    fn push_queue_to_lr(&mut self, cy: u32) {
        while let Some((_, d, p)) = self.queue_pop_if(cy) {
            let vec = if d { &mut self.right } else { &mut self.left };
            let idx = match vec.binary_search_by_key(&p.current, |profile| profile.current) {
                Ok(i) => i,
                Err(i) => i,
            };
            vec.insert(idx, p);
        }
    }

    fn prune_lr(&mut self, cy: u32) {
        self.left.retain(|profile| profile.end_y >= cy);
        self.right.retain(|profile| profile.end_y >= cy);
    }

    fn advance_lr_sort(&mut self) {
        for profile in self.left.iter_mut() {
            profile.current += profile.step;
        }

        for profile in self.right.iter_mut() {
            profile.current += profile.step;
        }

        self.left.sort_unstable_by_key(|profile| profile.current);
        self.right.sort_unstable_by_key(|profile| profile.current);
    }

    pub fn render(&mut self, width: u32, height: u32, mut filler: impl FnMut(u32, u32, u32)) {
        self.queue.sort_unstable_by(|(ay, ..), (by, ..)| by.cmp(ay));

        if self.queue.is_empty() {
            return;
        }

        let mut y = self.queue.last().unwrap().0;

        while (!self.queue.is_empty() || !self.left.is_empty()) && y < height {
            self.prune_lr(y);
            self.push_queue_to_lr(y);

            if POLYGON_RASTERIZER_DEBUG_PRINT {
                println!("--- POLYLINE RASTERIZER SCANLINE y={y} ---");
                println!("left: {:?}", self.left);
                println!("right: {:?}", self.right);
                assert_eq!(self.left.len(), self.right.len());
            }

            for i in 0..self.left.len() {
                let (left, right) = (&self.left[i], &self.right[i]);

                let round_clamp = |f: IFixed18Dot14| (f.round_to_inner().max(0) as u32).min(width);
                let mut x0 = round_clamp(left.current);
                let mut x1 = round_clamp(right.current);
                // TODO: is this necessary? can this be removed?
                if x0 > x1 {
                    std::mem::swap(&mut x0, &mut x1);
                }
                filler(y, x0, x1);
            }

            self.advance_lr_sort();

            y += 1;
        }
    }

    pub(super) fn render_to(
        &mut self,
        buffer: &mut [BGRA8],
        stride: usize,
        width: u32,
        height: u32,
        color: BGRA8,
    ) {
        if buffer.len() < width as usize * height as usize {
            panic!("Buffer passed to NonZeroPolygonRasterizer::render is too small")
        }

        self.render(width, height, |y, x0, x1| unsafe {
            super::sw::horizontal_line_unchecked(
                x0 as i32,
                x1 as i32,
                &mut buffer[y as usize * stride..],
                width as i32,
                color,
            )
        });
    }
}

pub fn debug_stroke_outline(
    rasterizer: &mut dyn super::Rasterizer,
    target: &mut super::RenderTarget,
    x: f32,
    y: f32,
    outline: &Outline,
    color: BGRA8,
    inverse_winding: bool,
) {
    if outline.is_empty() {
        return;
    }

    let offset = Vec2::new(x, y);
    for segments in outline.iter_contours() {
        if segments.is_empty() {
            continue;
        }

        let mut polyline = Vec::new();
        for segment in segments.iter().copied() {
            polyline.clear();
            let segment_points = outline.points_for_segment(segment);
            polyline.push(segment_points[0]);
            outline.flatten_segment(segment, 0.01, &mut polyline);
            rasterizer.stroke_polyline(target, offset, &polyline, color);
            let middle = outline.evaluate_segment(segment, 0.5);
            let start = segment_points[0];
            let end = *segment_points.last().unwrap();
            let diff = (end - start).normalize();
            let deriv = diff.normal();
            const ARROW_SCALE: f32 = 10.0;

            let f = if inverse_winding { -1.0 } else { 1.0 };
            let top = middle + diff * f * ARROW_SCALE;
            let left = middle - deriv * f * ARROW_SCALE;
            let right = middle + deriv * f * ARROW_SCALE;

            rasterizer.fill_triangle(
                target,
                &[top + offset, left + offset, right + offset],
                color,
            );
        }
    }
}
