use std::{
    cmp::{Ordering, Reverse},
    collections::BinaryHeap,
};

use util::math::{I16Dot16, Outline, Point2, Point2f, Vec2, Vec2f};

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
    dx: I16Dot16,
    current_x: I16Dot16,
    next_x: I16Dot16,

    prev: usize,
    next: usize,
}

impl Segment {
    #[inline(always)]
    fn x_at_y(&self, y: I16Dot16) -> I16Dot16 {
        self.bottom.x + self.dx * (y - self.bottom.y)
    }
}

#[derive(Debug)]
struct Trapezoid {
    top: I16Dot16,
    txl: I16Dot16,
    txr: I16Dot16,
    bottom: I16Dot16,
    bxl: I16Dot16,
    bxr: I16Dot16,
}

#[derive(Debug)]
pub struct PolygonRasterizer {
    size: Vec2<u32>,
    stride: usize,
    coverage: Vec<u16>,
    segments: Vec<Segment>,
    initial_events: Vec<Reverse<Event>>,
    events: BinaryHeap<Reverse<Event>>,
    active_head: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Event {
    y: I16Dot16,
    payload: EventPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EventPayload {
    value: u32,
}

impl EventPayload {
    fn new(kind: EventKind, value: u32) -> Self {
        Self {
            value: value | kind as u32,
        }
    }

    fn scanline() -> Self {
        Self::new(EventKind::Scanline, 0)
    }

    fn intersection(value: u32) -> Self {
        Self::new(EventKind::Intersection, value)
    }

    fn start(value: u32) -> Self {
        Self::new(EventKind::Start, value)
    }

    fn end(value: u32) -> Self {
        Self::new(EventKind::End, value)
    }

    fn unpack(self) -> (EventKind, u32) {
        const MASK: u32 = 0b11 << 30;
        let kind = unsafe { std::mem::transmute(self.value & MASK) };
        let value = self.value ^ kind as u32;
        (kind, value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum EventKind {
    Scanline = 0b00 << 30,
    Intersection = 0b01 << 30,
    Start = 0b10 << 30,
    End = 0b11 << 30,
}

impl PolygonRasterizer {
    pub fn new() -> Self {
        Self {
            size: Vec2::ZERO,
            stride: 0,
            coverage: Vec::new(),
            segments: Vec::new(),
            initial_events: Vec::new(),
            events: BinaryHeap::new(),
            active_head: usize::MAX,
        }
    }

    fn add_line(&mut self, mut start: Point2<I16Dot16>, mut end: Point2<I16Dot16>) {
        let winding = if end.y > start.y {
            std::mem::swap(&mut start, &mut end);
            Winding::Clockwise
        } else if end.y == start.y {
            return;
        } else {
            Winding::CounterClockwise
        };

        let mut segment = Segment {
            top: start,
            bottom: end,
            winding,
            current_x: I16Dot16::ZERO,
            next_x: I16Dot16::ZERO,
            dx: (start.x - end.x) / (start.y - end.y),
            prev: usize::MAX,
            next: usize::MAX,
        };

        segment.current_x = segment.bottom.x;
        segment.next_x = segment.current_x;

        self.segments.push(segment)
    }

    fn add_linef(&mut self, start: Point2f, end: Point2f) {
        self.add_line(
            Point2::new(start.x.into(), start.y.into()),
            Point2::new(end.x.into(), end.y.into()),
        );
    }

    pub fn add_outline_with(
        &mut self,
        outline: &Outline,
        mapper: impl Fn(Point2f) -> Point2f,
        tolerance: f32,
    ) {
        let mut points = Vec::new();
        for contour in outline.iter_contours() {
            let Some(&last_segment) = contour.last() else {
                continue;
            };
            let mut last = mapper(*outline.points_for_segment(last_segment).last().unwrap());

            for segment in contour {
                points.clear();
                outline.flatten_segment(*segment, tolerance, &mut points);

                for &point in &points {
                    let point = mapper(point);
                    self.add_linef(last, point);
                    last = point;
                }
            }
        }
    }

    pub fn add_outline(&mut self, outline: &Outline, translation: Vec2f, tolerance: f32) {
        self.add_outline_with(outline, |p| p + translation, tolerance);
    }

    pub fn reset(&mut self, size: Vec2<u32>, stride: usize) {
        assert!(stride >= size.x as usize);

        self.size = size;
        self.stride = stride;
        self.coverage.fill(0);
        self.coverage.resize(size.x as usize * size.y as usize, 0);
    }

    fn compute_segment_endpoint_events(&mut self) {
        let max_y = I16Dot16::new(self.size.y as i32);

        macro_rules! add_fractional_scanline {
            ($y: expr) => {{
                let y = $y;
                if y > 0 && y <= max_y {
                    self.initial_events.push(Reverse(Event {
                        y: $y,
                        payload: EventPayload::scanline(),
                    }));
                }
            }};
        }

        for (index, segment) in self.segments.iter().enumerate() {
            self.initial_events.push(Reverse(Event {
                y: segment.bottom.y,
                payload: EventPayload::start(index as u32),
            }));

            self.initial_events.push(Reverse(Event {
                y: segment.top.y,
                payload: EventPayload::end(index as u32),
            }));

            add_fractional_scanline!(segment.top.y);
            add_fractional_scanline!(segment.bottom.y);
        }
    }

    fn add_coverage_between(
        &mut self,
        // next = top
        next_y: util::math::Fixed<16, i32>,
        // last = bottom
        last_y: util::math::Fixed<16, i32>,
    ) {
        let mut start = None;
        let mut winding_count = 0;
        let mut i = self.active_head;
        while i != usize::MAX {
            let segment = &self.segments[i];
            i = segment.next;

            if winding_count == 0 {
                start = Some(segment.clone());
            }
            winding_count += segment.winding as i32;
            if winding_count == 0 {
                let start = start.take().unwrap();
                let trapezoid = Trapezoid {
                    top: next_y,
                    txl: start.next_x,
                    txr: segment.next_x,
                    bottom: last_y,
                    bxl: start.current_x,
                    bxr: segment.current_x,
                };
                let end = segment.clone();
                self.add_trapezoid_coverage(&trapezoid, &start, &end);
            }
        }
    }

    fn add_trapezoid_row_coverage(&mut self, py: u32, current: &Trapezoid) {
        let pixel_left = current
            .txl
            .min(current.bxl)
            .floor_to_inner()
            .clamp(0, self.size.x as i32) as u32;
        let pixel_right = current
            .txr
            .max(current.bxr)
            .ceil_to_inner()
            .clamp(0, self.size.x as i32) as u32;

        let fpy = I16Dot16::new(py as i32);
        #[expect(clippy::int_plus_one)] // dear clippy, this is not an int.
        let y_coverage_hit = if current.top >= fpy + 1 && current.bottom <= fpy {
            I16Dot16::ZERO
        } else {
            let dtop = current.top - fpy - 1;
            let dbot = fpy - current.bottom;
            let ctop = dtop.min(I16Dot16::ZERO);
            let cbot = dbot.min(I16Dot16::ZERO);

            debug_assert!(
                (-I16Dot16::ONE..=I16Dot16::ZERO).contains(&ctop),
                "Invalid top coverage hit {ctop} on pixel row {py}"
            );

            debug_assert!(
                (-I16Dot16::ONE..=I16Dot16::ZERO).contains(&cbot),
                "Invalid bottom coverage hit {cbot} on pixel row {py}"
            );

            ctop + cbot
        };

        // eprintln!("Y coverage hit of line {py} = {y_coverage_hit}");

        let inner_left = current
            .txl
            .max(current.bxl)
            .ceil_to_inner()
            .clamp(0, self.size.x as i32) as u32;
        let inner_right = current
            .txr
            .min(current.bxr)
            .floor_to_inner()
            .clamp(0, self.size.x as i32) as u32;
        for px in pixel_left..pixel_right {
            let fpx = I16Dot16::new(px as i32);
            let x_coverage_hit = {
                let inner_h = current.top - current.bottom;
                let lhit = if px >= inner_left {
                    I16Dot16::ZERO
                } else {
                    let left = fpx;
                    let top_right = current.txl.clamp(fpx, fpx + 1);
                    let bottom_right = current.bxl.clamp(fpx, fpx + 1);
                    let neg_a = left - top_right;
                    let neg_b = left - bottom_right;
                    ((neg_a + neg_b) * inner_h) / 2
                };

                let rhit = if px < inner_right {
                    I16Dot16::ZERO
                } else {
                    let right = fpx + 1;
                    let top_left = current.txr.clamp(fpx, fpx + 1);
                    let bottom_left = current.bxr.clamp(fpx, fpx + 1);
                    let neg_a = top_left - right;
                    let neg_b = bottom_left - right;
                    ((neg_a + neg_b) * inner_h) / 2
                };

                if lhit != I16Dot16::ZERO {
                    // eprintln!("Left coverage hit of pixel ({px}, {py}) = {lhit}");
                }

                debug_assert!(
                    (-I16Dot16::ONE..=I16Dot16::ZERO).contains(&lhit),
                    "Invalid left coverage hit {lhit} on pixel ({px}, {py})"
                );

                if rhit != I16Dot16::ZERO {
                    // eprintln!("Right coverage hit of pixel ({px}, {py}) = {rhit}");
                }

                debug_assert!(
                    (-I16Dot16::ONE..=I16Dot16::ZERO).contains(&rhit),
                    "Invalid right coverage hit {rhit} on pixel ({px}, {py})"
                );

                lhit + rhit
            };
            let coverage = I16Dot16::ONE + x_coverage_hit + y_coverage_hit;

            // debug_assert!(coverage >= -I16Dot16::from_quotient(1, 100));
            let coverage16 = (((coverage.into_raw() & 0x0001FFFF) as u64) * u64::from(u16::MAX)
                / (u64::from(u16::MAX) + 1)) as u16;
            // eprintln!("Coverage of pixel ({px}, {py}) = {coverage16:04X}");
            debug_assert!(px < self.size.x);
            debug_assert!(py < self.size.y);
            unsafe { self.add_coverage_at(px, py, coverage16) }
        }
    }

    fn add_trapezoid_coverage(&mut self, trapezoid: &Trapezoid, sleft: &Segment, sright: &Segment) {
        let pixel_top = trapezoid.top.ceil_to_inner() as u32;
        let pixel_bottom = trapezoid.bottom.floor_to_inner() as u32;

        let top = (trapezoid.bottom.floor() + 1).min(trapezoid.top);
        let mut current = Trapezoid {
            top,
            txl: if top == trapezoid.top {
                trapezoid.txl
            } else {
                sleft.x_at_y(top)
            },
            txr: if top == trapezoid.top {
                trapezoid.txr
            } else {
                sright.x_at_y(top)
            },
            bottom: trapezoid.bottom,
            bxl: trapezoid.bxl,
            bxr: trapezoid.bxr,
        };
        for py in pixel_bottom..pixel_top {
            self.add_trapezoid_row_coverage(py, &current);

            if current.bottom.fract() == I16Dot16::ZERO {
                current.bottom += 1;
                current.bxl += sleft.dx;
                current.bxr += sright.dx;
            } else {
                current.bottom = current.bottom.ceil();
                current.bxl = sleft.x_at_y(current.bottom);
                current.bxr = sright.x_at_y(current.bottom);
            }

            current.top += 1;
            if trapezoid.top < current.top {
                current.top = trapezoid.top;
                current.txl = trapezoid.txl;
                current.txr = trapezoid.txr;
            } else {
                current.txl += sleft.dx;
                current.txr += sright.dx;
            }
        }
    }

    unsafe fn add_coverage_at(&mut self, x: u32, y: u32, value: u16) {
        unsafe {
            let pixel = self
                .coverage
                .get_unchecked_mut(y as usize * self.stride + x as usize);
            *pixel = pixel.saturating_add(value);
        }
    }

    /// Swap i with i->next to maintain sorted order and check for new possible
    /// intersections between the now swapped segments and their neighbors.
    fn process_intersection(&mut self, last_y: I16Dot16, i: usize) {
        let prev = self.segments[i].prev;
        let next = self.segments[i].next;
        let next_next = self.segments[next].next;
        if prev != usize::MAX {
            self.segments[prev].next = next;
            self.check_for_intersection(last_y, prev, next);
        } else {
            self.active_head = next;
        }
        self.segments[next].prev = prev;
        self.segments[next].next = i;
        self.segments[i].prev = next;
        self.segments[i].next = next_next;
        if next_next != usize::MAX {
            self.segments[next_next].prev = i;
            self.check_for_intersection(last_y, i, next_next);
        }
    }

    #[cfg(debug_assertions)]
    fn validate_linked_list(&self) {
        let mut prev = usize::MAX;
        let mut i = self.active_head;
        while i != usize::MAX {
            // eprintln!(
            //     "  {}: segment({:?}, {:?}) {}",
            //     i, self.segments[i].bottom, self.segments[i].top, self.segments[i].current_x
            // );
            assert_eq!(self.segments[i].prev, prev);
            prev = i;
            i = self.segments[i].next;
        }
    }

    pub fn rasterize(&mut self) {
        #[cfg(not(target_pointer_width = "16"))]
        assert!(
            self.segments.len() < (1u32 << 30) as usize,
            "PolygonRasterizer does not currently support more than 2^30-1 segments"
        );
        self.compute_segment_endpoint_events();
        self.events = BinaryHeap::from(std::mem::take(&mut self.initial_events));

        let mut last_y = I16Dot16::ZERO;
        let mut last = Event {
            y: I16Dot16::MIN,
            payload: EventPayload::scanline(),
        };
        while let Some(Reverse(event)) = self.events.pop() {
            if event == last {
                continue;
            }
            last = event;

            let next_y = event.y;

            let (kind, value) = event.payload.unpack();
            match kind {
                EventKind::Intersection => {
                    self.process_intersection(last_y, value as usize);
                }
                EventKind::Start => {
                    self.activate_segment(next_y, value as usize);
                }
                EventKind::End => {
                    self.deactivate_segment(next_y, value as usize);
                }
                EventKind::Scanline => {
                    // eprintln!("Active segments at y range {last_y}-{next_y}:");
                    #[cfg(debug_assertions)]
                    self.validate_linked_list();

                    let mut i = self.active_head;
                    if next_y == last_y + 1 {
                        while i != usize::MAX {
                            let segment = &mut self.segments[i];
                            segment.next_x = segment.current_x + segment.dx;
                            i = segment.next;
                        }
                    } else {
                        while i != usize::MAX {
                            let segment = &mut self.segments[i];
                            segment.next_x = segment.x_at_y(next_y);
                            i = segment.next;
                        }
                    }

                    // eprintln!("Computing coverage between scan lines {next_y} -- {last_y}");
                    self.add_coverage_between(next_y, last_y);

                    for i in 0..self.segments.len() {
                        let segment = &mut self.segments[i];
                        segment.current_x = segment.next_x;
                    }

                    last_y = next_y;
                }
            }
        }

        assert_eq!(self.active_head, usize::MAX);
        self.segments.clear();
        self.initial_events = std::mem::take(&mut self.events).into_vec();
        assert!(self.initial_events.is_empty());
    }

    pub fn coverage(&self) -> &[u16] {
        &self.coverage
    }

    fn activate_segment(&mut self, current_y: I16Dot16, i: usize) {
        // eprintln!("Activating segment {i}");

        let x = self.segments[i].current_x;
        let prev = {
            let mut prev = usize::MAX;
            let mut next = self.active_head;
            loop {
                // TODO: This looks like it's going to break tomorrow
                if next != usize::MAX
                    && self.segments[next].current_x.cmp(&x).then_with(|| {
                        let base = Point2::new(x, current_y);
                        let a = self.segments[i].top - base;
                        let b = self.segments[next].top - base;
                        b.cross(a).cmp(&I16Dot16::ZERO)
                    }) == Ordering::Less
                {
                } else {
                    break;
                }

                prev = next;
                next = self.segments[next].next;
            }
            prev
        };

        let next = if prev == usize::MAX {
            let head = self.active_head;
            self.segments[i].next = head;
            self.active_head = i;
            if head != usize::MAX {
                self.segments[head].prev = i;
                head
            } else {
                usize::MAX
            }
        } else {
            let next = self.segments[prev].next;
            self.segments[i].prev = prev;
            self.segments[i].next = next;
            self.segments[prev].next = i;
            if next != usize::MAX {
                self.segments[next].prev = i;
            }
            next
        };

        if prev != usize::MAX {
            self.check_for_intersection(current_y, prev, i);
        }

        if next != usize::MAX {
            self.check_for_intersection(current_y, i, next);
        }
    }

    fn deactivate_segment(&mut self, current_y: I16Dot16, i: usize) {
        // eprintln!("Deactivating segment {i}");

        let prev = self.segments[i].prev;
        let next = self.segments[i].next;
        if prev != usize::MAX {
            self.segments[prev].next = next;
            self.segments[i].prev = usize::MAX;
        } else {
            self.active_head = next;
        }

        if next != usize::MAX {
            self.segments[next].prev = prev;
            self.segments[i].next = usize::MAX;
        }

        if prev != usize::MAX && next != usize::MAX {
            self.check_for_intersection(current_y, prev, next);
        }
    }

    fn check_for_intersection(&mut self, current_y: I16Dot16, ai: usize, bi: usize) {
        // eprintln!("Checking {ai} and {bi} for intersection");
        if let Some(y) = self.find_intersection_y(current_y, ai, bi) {
            if y <= current_y {
                return;
            }

            self.events.push(Reverse(Event {
                y,
                payload: EventPayload::intersection(ai as u32),
            }));
            if y > 0 && y <= self.size.y as i32 {
                self.events.push(Reverse(Event {
                    y,
                    payload: EventPayload::scanline(),
                }));
            }
        }
    }

    fn find_intersection_y(
        &mut self,
        current_y: I16Dot16,
        ai: usize,
        bi: usize,
    ) -> Option<I16Dot16> {
        let a = &self.segments[ai];
        let b = &self.segments[bi];

        let bottom_relation = if a.bottom.x == b.bottom.x {
            return None;
        } else {
            a.bottom.x < b.bottom.x
        };

        let top_relation = if a.top.x == b.top.x {
            return None;
        } else {
            a.top.x < b.top.x
        };

        if bottom_relation == top_relation {
            return None;
        }

        let mut top_y = a.top.y.min(b.top.y);
        let mut bottom_y = current_y;

        let distance_at_y = |y: I16Dot16| {
            let a_x = a.x_at_y(y);
            let b_x = b.x_at_y(y);
            (a_x - b_x).abs()
        };

        while top_y - bottom_y > I16Dot16::from_quotient(1, 32) {
            let diff = (top_y - bottom_y) / 3;
            let mid_left = bottom_y + diff;
            let mid_right = top_y - diff;
            let val_left = distance_at_y(mid_left);
            let val_right = distance_at_y(mid_right);
            if val_left < val_right {
                top_y = mid_right;
            } else {
                bottom_y = mid_left;
            }
        }

        let intersection_y = (bottom_y + top_y) / 2;
        if (intersection_y - a.top.y.min(b.top.y)).abs() <= I16Dot16::from_quotient(1, 32) {
            return None;
        }

        if (intersection_y - current_y).abs() <= I16Dot16::from_quotient(1, 32) {
            return None;
        }

        // eprintln!("{ai} {bi} intersect at {intersection_y}");
        Some(intersection_y)
    }
}

#[cfg(test)]
mod test {
    use util::math::{Point2f, Vec2};

    use crate::sw::PolygonRasterizer;

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
            for y in (0..size.y as usize).rev() {
                let print_row = |which: bool| {
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

                print_row(true);
                print!("    ");
                print_row(false);

                eprintln!()
            }

            panic!()
        }
    }

    #[test]
    fn coverage() {
        const SIZE: Vec2<u32> = Vec2::new(15, 10);

        let mut rasterizer = PolygonRasterizer::new();
        rasterizer.reset(SIZE, SIZE.x as usize);
        let points = [
            Point2f::new(0.0, 0.0),
            Point2f::new(4.0, 10.0),
            Point2f::new(10.0, 7.5),
            Point2f::new(14.0, 3.0),
        ];
        let mut last = *points.last().unwrap();
        for point in points {
            rasterizer.add_linef(last, point);
            last = point;
        }
        rasterizer.rasterize();

        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            0x00, 0x00, 0x00, 0x33, 0x80, 0x7F, 0x33, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x99, 0xFF, 0xFF, 0xB3, 0x7F, 0x66, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x0C, 0xF3, 0xFF, 0xFF, 0xFF, 0xFF, 0xF3, 0xBF, 0x1C, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x66, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xB8, 0x2A, 0x00, 0x00, 0x00,
            0x00, 0x00, 0xCC, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xAA, 0x1C, 0x00, 0x00,
            0x00, 0x33, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x9C, 0x0E, 0x00,
            0x00, 0x99, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x8E, 0x00,
            0x19, 0xE6, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xAA, 0x7F, 0x7F, 0x7F, 0x7F, 0x00,
            0x66, 0xFF, 0xFF, 0xFF, 0xD5, 0x7F, 0x7F, 0x7F, 0x7F, 0x2A, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x4C, 0x7F, 0x7F, 0x7F, 0x55, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        compare(SIZE, rasterizer.coverage(), EXPECTED);
    }
}
