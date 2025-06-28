use std::{
    cmp::{Ordering, Reverse},
    collections::BinaryHeap,
};

use util::math::{
    Bezier, CubicBezier, I16Dot16, Outline, Point2, Point2f, QuadraticBezier, Rect2, Vec2, Vec2f,
};

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
    current_x: I16Dot16,
    next_x: I16Dot16,
    kind: SegmentKind,

    prev: usize,
    next: usize,
}

impl Segment {
    fn x_at_y(&self, current_y: I16Dot16, y: I16Dot16) -> I16Dot16 {
        match self.kind {
            SegmentKind::Linear { dx } => self.bottom.x + dx * (y - self.bottom.y),
            SegmentKind::Quadratic {
                control, current_t, ..
            } => {
                newton_quadratic_x_at_y(
                    QuadraticBezier([self.bottom, control, self.top]),
                    current_t,
                    current_y,
                    y,
                )
                .1
            }
        }
    }

    fn advance_next_to_y(&mut self, current_y: I16Dot16, y: I16Dot16) {
        match self.kind {
            SegmentKind::Linear { dx } => self.next_x = self.bottom.x + dx * (y - self.bottom.y),
            SegmentKind::Quadratic {
                control,
                current_t,
                ref mut next_t,
                ..
            } => {
                let (t, x) = newton_quadratic_x_at_y(
                    QuadraticBezier([self.bottom, control, self.top]),
                    current_t,
                    current_y,
                    y,
                );
                *next_t = t;
                self.next_x = x;
            }
        }
    }

    fn advance_current_to_next(&mut self) {
        self.current_x = self.next_x;

        match self.kind {
            SegmentKind::Linear { .. } => (),
            SegmentKind::Quadratic {
                ref mut current_t,
                next_t,
                ..
            } => {
                *current_t = next_t;
            }
        }
    }

    fn stepper(&self) -> SegmentStepper {
        SegmentStepper {
            top: self.top,
            bottom: self.bottom,
            current_x: self.current_x,
            next_x: self.next_x,
            kind: self.kind.clone(),
        }
    }
}

struct SegmentStepper {
    top: Point2<I16Dot16>,
    bottom: Point2<I16Dot16>,
    current_x: I16Dot16,
    next_x: I16Dot16,
    kind: SegmentKind,
}

impl SegmentStepper {
    fn advance_inside(&mut self, current_y: I16Dot16, y: I16Dot16) -> I16Dot16 {
        match self.kind {
            SegmentKind::Linear { dx } => self.bottom.x + dx * (y - self.bottom.y),
            SegmentKind::Quadratic {
                control,
                ref mut current_t,
                ..
            } => {
                let (t, x) = newton_quadratic_x_at_y(
                    QuadraticBezier([self.bottom, control, self.top]),
                    *current_t,
                    current_y,
                    y,
                );
                *current_t = t;
                x
            }
        }
    }

    fn advance1_inside(&mut self, prev: I16Dot16, current_y: I16Dot16, y: I16Dot16) -> I16Dot16 {
        match self.kind {
            SegmentKind::Linear { dx } => prev + dx,
            SegmentKind::Quadratic {
                control,
                ref mut current_t,
                ..
            } => {
                let (t, x) = newton_quadratic_x_at_y(
                    QuadraticBezier([self.bottom, control, self.top]),
                    *current_t,
                    current_y,
                    y,
                );
                *current_t = t;
                x
            }
        }
    }
}

#[derive(Debug, Clone)]
enum SegmentKind {
    Linear {
        dx: I16Dot16,
    },
    Quadratic {
        control: Point2<I16Dot16>,
        current_t: I16Dot16,
        next_t: I16Dot16,
    },
}

fn newton_quadratic_x_at_y(
    mut quad: QuadraticBezier<I16Dot16>,
    initial_t: I16Dot16,
    initial_y: I16Dot16,
    y: I16Dot16,
) -> (I16Dot16, I16Dot16) {
    for point in quad.points_mut() {
        point.y -= y;
    }
    let derivy = quad.derivative().map(|p| p.y);
    let dd = derivy[1] - derivy[0];
    let mut t = initial_t;
    let mut p_y = initial_y - y;

    while p_y.abs() > I16Dot16::from_quotient(1, 64) {
        // TODO: It may be possible to create an edge case where the derivative is zero here
        t -= p_y / (derivy[0] + dd * t);
        p_y = quad.sample(t).y;
    }

    t = t.clamp(I16Dot16::ZERO, I16Dot16::ONE);
    (t, quad.sample(t).x)
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
pub struct PathRasterizer {
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
    value: u64,
}

impl EventPayload {
    fn new(kind: EventKind, value: u64) -> Self {
        Self {
            value: value | kind as u64,
        }
    }

    fn scanline() -> Self {
        Self::new(EventKind::Scanline, 0)
    }

    fn intersection(a: u32, b: u32) -> Self {
        Self::new(EventKind::Intersection, u64::from(a) | (u64::from(b) << 30))
    }

    fn start(value: u32) -> Self {
        Self::new(EventKind::Start, value.into())
    }

    fn end(value: u32) -> Self {
        Self::new(EventKind::End, value.into())
    }

    fn unpack(self) -> (EventKind, u32, u32) {
        const MASK: u64 = 0b11 << 62;
        let kind = unsafe { std::mem::transmute(self.value & MASK) };
        let b = (self.value >> 30) as u32;
        let a = ((self.value << 2) as u32) >> 2;
        (kind, a, b)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
enum EventKind {
    Scanline = 0b00 << 62,
    Intersection = 0b01 << 62,
    Start = 0b10 << 62,
    End = 0b11 << 62,
}

impl PathRasterizer {
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

        self.segments.push(Segment {
            top: start,
            bottom: end,
            winding,
            current_x: end.x,
            next_x: end.x,
            kind: SegmentKind::Linear {
                dx: (start.x - end.x) / (start.y - end.y),
            },
            prev: usize::MAX,
            next: usize::MAX,
        })
    }

    fn add_linef(&mut self, start: Point2f, end: Point2f) {
        self.add_line(
            Point2::new(start.x.into(), start.y.into()),
            Point2::new(end.x.into(), end.y.into()),
        );
    }

    fn add_quadratic(&mut self, quadratic: QuadraticBezier<f32>) {
        if (quadratic[0].y <= quadratic[1].y && quadratic[1].y <= quadratic[2].y)
            || (quadratic[0].y >= quadratic[1].y && quadratic[1].y >= quadratic[2].y)
        {
            self.add_monotonic_quadraticf(quadratic);
        } else {
            self.add_non_monotonic_quadratic(quadratic);
        }
    }

    fn add_quadraticf(&mut self, quadratic: QuadraticBezier<f32>) {
        self.add_quadratic(quadratic);
    }

    fn add_monotonic_quadratic(&mut self, mut quadratic: QuadraticBezier<I16Dot16>) {
        let winding = if quadratic[0].y == quadratic[1].y && quadratic[1].y == quadratic[2].y {
            return;
        } else if quadratic[0].y >= quadratic[2].y {
            quadratic.0.swap(0, 2);
            Winding::CounterClockwise
        } else {
            debug_assert!(quadratic[0].y <= quadratic[2].y);
            Winding::Clockwise
        };

        self.segments.push(Segment {
            top: quadratic[2],
            bottom: quadratic[0],
            winding,
            current_x: quadratic[0].x,
            next_x: quadratic[0].x,
            kind: SegmentKind::Quadratic {
                control: quadratic[1],
                current_t: I16Dot16::ZERO,
                next_t: I16Dot16::ZERO,
            },
            prev: usize::MAX,
            next: usize::MAX,
        })
    }

    fn add_monotonic_quadraticf(&mut self, quadratic: QuadraticBezier<f32>) {
        self.add_monotonic_quadratic(QuadraticBezier(
            quadratic
                .0
                .map(|p| Point2::new(I16Dot16::from_f32(p.x), I16Dot16::from_f32(p.y))),
        ));
    }

    fn add_non_monotonic_quadratic(&mut self, quadratic: QuadraticBezier<f32>) {
        let [dp0, dp1] = quadratic.derivative();
        let t = dp0.y / (dp0.y - dp1.y);
        let (a, b) = quadratic.split_at(t);
        self.add_monotonic_quadraticf(a);
        self.add_monotonic_quadraticf(b);
    }

    pub fn add_outline_with(
        &mut self,
        outline: &Outline,
        mapper: impl Fn(Point2f) -> Point2f,
        tolerance: f32,
    ) {
        for contour in outline.iter_contours() {
            for segment in contour {
                match outline.curve_for_segment(segment) {
                    util::math::SegmentCurve::Linear(line) => {
                        self.add_linef(mapper(line[0]), mapper(line[1]));
                    }
                    util::math::SegmentCurve::Quadratic(quadratic) => {
                        let mapped = QuadraticBezier(quadratic.0.map(&mapper));
                        self.add_quadraticf(mapped);
                    }
                    util::math::SegmentCurve::Cubic(cubic) => {
                        let mapped = CubicBezier(cubic.0.map(&mapper));
                        for quadratic in mapped.to_quadratics(tolerance) {
                            self.add_quadraticf(quadratic);
                        }
                    }
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
                start = Some(segment.stepper());
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
                let end = segment.stepper();
                self.add_trapezoid_coverage(&trapezoid, start, end);
            }
        }
    }

    fn add_trapezoid_row_coverage(&mut self, py: u32, current: &Trapezoid) {
        let pixel_left =
            (current.txl.min(current.bxl).floor_to_inner().max(0) as u32).min(self.size.x);
        let pixel_right =
            (current.txr.max(current.bxr).ceil_to_inner().max(0) as u32).min(self.size.x);

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

        let inner_left =
            (current.txl.max(current.bxl).ceil_to_inner().max(0) as u32).min(self.size.x);
        let inner_right =
            (current.txr.min(current.bxr).floor_to_inner().max(0) as u32).min(self.size.x);

        struct LineStepper {
            current_x: I16Dot16,
            next_x: I16Dot16,
            current_base: I16Dot16,
            next_base: I16Dot16,
            dbase: I16Dot16,
            end_x: I16Dot16,
            end_base: I16Dot16,
        }

        impl LineStepper {
            fn horizontal(x: I16Dot16) -> Self {
                Self {
                    end_x: I16Dot16::MAX,
                    end_base: I16Dot16::ZERO,
                    dbase: I16Dot16::ZERO,
                    current_x: x,
                    current_base: I16Dot16::ZERO,
                    next_x: x,
                    next_base: I16Dot16::ZERO,
                }
            }

            #[inline]
            fn new(
                top: Point2<I16Dot16>,
                bottom: Point2<I16Dot16>,
                fpy: I16Dot16,
                right_edge: bool,
            ) -> (Self, I16Dot16) {
                if top.x == bottom.x {
                    return (Self::horizontal(top.x), I16Dot16::ZERO);
                }

                let (left, right, over_area) = if top.x < bottom.x {
                    (top, bottom, !right_edge)
                } else {
                    (bottom, top, right_edge)
                };

                // PERF: This results in a 64-bit division on every row multiple times.
                //       I can't see a way around it short of computing ddy of bezier curves
                //       which I feel is impractical...
                //       Hoping for pipelining I guess...
                let dy = (right.y - left.y) / (right.x - left.x);
                let dbase = if over_area { dy } else { -dy };
                let next_x = (left.x.floor() + 1).min(right.x);
                let current_base = if over_area {
                    left.y - fpy
                } else {
                    (fpy + 1) - left.y
                };

                (
                    Self {
                        end_x: right.x,
                        end_base: if over_area {
                            right.y - fpy
                        } else {
                            (fpy + 1) - right.y
                        },
                        current_x: left.x,
                        next_x,
                        current_base,
                        next_base: current_base + (dbase * (next_x - left.x)),
                        dbase,
                    },
                    if right_edge {
                        (I16Dot16::ONE - right.x.fract()) * (top.y - bottom.y)
                    } else {
                        left.x.fract() * (top.y - bottom.y)
                    },
                )
            }

            #[inline]
            fn current_doubled_hit(&self) -> I16Dot16 {
                let neg_height = self.current_x - self.next_x;
                (self.next_base + self.current_base) * neg_height
            }

            fn step(&mut self) {
                self.current_x = self.next_x;
                self.current_base = self.next_base;
                self.next_x += 1;
                if self.next_x >= self.end_x {
                    self.next_x = self.end_x;
                    self.next_base = self.end_base;
                } else {
                    self.next_base += self.dbase;
                }
            }
        }

        let (mut left_stepper, mut current_left_rect_hit) = LineStepper::new(
            Point2::new(current.txl, current.top),
            Point2::new(current.bxl, current.bottom),
            fpy,
            false,
        );
        let (mut right_stepper, right_rect_hit) = LineStepper::new(
            Point2::new(current.txr, current.top),
            Point2::new(current.bxr, current.bottom),
            fpy,
            true,
        );

        let mut px = pixel_left;
        loop {
            let x_coverage_hit = {
                let mut w = I16Dot16::ZERO;

                if px < inner_left {
                    w += left_stepper.current_doubled_hit();
                    left_stepper.step();
                }

                if px >= inner_right {
                    w += right_stepper.current_doubled_hit();
                    right_stepper.step();
                }

                w /= 2;

                w -= current_left_rect_hit;

                if px == pixel_right - 1 {
                    w -= right_rect_hit;
                }

                w
            };
            let coverage = I16Dot16::ONE + x_coverage_hit + y_coverage_hit;

            // debug_assert!(coverage >= -I16Dot16::from_quotient(1, 100));
            let coverage16 = fixed_to_u16(coverage);
            // eprintln!("Coverage of pixel ({px}, {py}) = {coverage16:04X}");
            debug_assert!(px < self.size.x);
            debug_assert!(py < self.size.y);
            unsafe { self.add_coverage_at(px, py, coverage16) }

            px += 1;
            if px >= pixel_right {
                break;
            }

            current_left_rect_hit = I16Dot16::ZERO;
        }
    }

    fn add_trapezoid_coverage(
        &mut self,
        trapezoid: &Trapezoid,
        mut sleft: SegmentStepper,
        mut sright: SegmentStepper,
    ) {
        let pixel_top = trapezoid.top.ceil_to_inner() as u32;
        let pixel_bottom = trapezoid.bottom.floor_to_inner() as u32;

        let top = (trapezoid.bottom.floor() + 1).min(trapezoid.top);
        let mut current = Trapezoid {
            top,
            txl: if top == trapezoid.top {
                trapezoid.txl
            } else {
                sleft.advance_inside(trapezoid.bottom, top)
            },
            txr: if top == trapezoid.top {
                trapezoid.txr
            } else {
                sright.advance_inside(trapezoid.bottom, top)
            },
            bottom: trapezoid.bottom,
            bxl: trapezoid.bxl,
            bxr: trapezoid.bxr,
        };
        let mut py = pixel_bottom;
        loop {
            self.add_trapezoid_row_coverage(py, &current);

            py += 1;
            if py == pixel_top {
                break;
            }

            current.bottom = current.top;
            current.bxl = current.txl;
            current.bxr = current.txr;
            current.top += 1;

            if trapezoid.top <= current.top {
                current.top = trapezoid.top;
                current.txl = trapezoid.txl;
                current.txr = trapezoid.txr;
            } else {
                debug_assert!(
                    (trapezoid.bottom..=trapezoid.top).contains(&current.top),
                    "{} is not inside trapezoid {}..={}",
                    current.top,
                    trapezoid.bottom,
                    trapezoid.top
                );
                current.txl = sleft.advance1_inside(current.txl, trapezoid.bottom, current.top);
                current.txr = sright.advance1_inside(current.txr, trapezoid.bottom, current.top);
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
    fn process_intersection(&mut self, last_y: I16Dot16, li: usize, ri: usize) {
        let prev = self.segments[li].prev;
        let next = self.segments[li].next;
        if next != ri {
            return;
        }
        let next_next = self.segments[next].next;
        if prev != usize::MAX {
            self.segments[prev].next = next;
            self.check_for_intersection(last_y, prev, next);
        } else {
            self.active_head = next;
        }
        self.segments[next].prev = prev;
        self.segments[next].next = li;
        self.segments[li].prev = next;
        self.segments[li].next = next_next;
        if next_next != usize::MAX {
            self.segments[next_next].prev = li;
            self.check_for_intersection(last_y, li, next_next);
        }
    }

    fn move_left_until_sorted(&mut self, i: usize) {
        let mut target = self.segments[i].prev;
        while target != usize::MAX && self.segments[target].next_x >= self.segments[i].next_x {
            target = self.segments[target].prev;
        }
        if target != self.segments[i].prev {
            self.remove_segment(i);
            self.insert_segment_after(target, i);
        }
    }

    #[cfg(debug_assertions)]
    fn validate_linked_list(&self, print: bool) {
        let mut prev = usize::MAX;
        let mut i = self.active_head;
        let mut count = 0;
        while i != usize::MAX {
            if print {
                eprint!("  {i}: ");
                match self.segments[i].kind {
                    SegmentKind::Linear { .. } => {
                        eprint!(
                            "segment({:?}, {:?})",
                            self.segments[i].bottom, self.segments[i].top,
                        );
                    }
                    SegmentKind::Quadratic { control, .. } => {
                        eprint!(
                            "quadratic({:?}, {:?}, {:?})",
                            self.segments[i].bottom, control, self.segments[i].top,
                        );
                    }
                }
                eprintln!(" {}", self.segments[i].current_x);
            }
            assert_eq!(self.segments[i].prev, prev);
            prev = i;
            i = self.segments[i].next;
            count += 1;
        }
        assert!(
            count & 1 == 0,
            "Number of active segments must never be odd"
        );
    }

    #[allow(dead_code)]
    fn print_all_segments(&mut self) {
        for segment in &self.segments {
            match segment.kind {
                SegmentKind::Linear { dx } => {
                    eprint!("linear({:?}, {:?}) dx={dx}", segment.bottom, segment.top,);
                }
                SegmentKind::Quadratic { control, .. } => {
                    eprint!(
                        "quadratic({:?}, {control:?}, {:?})",
                        segment.bottom, segment.top,
                    );
                }
            }

            eprintln!(
                " winding={}",
                match segment.winding {
                    Winding::CounterClockwise => "ccw",
                    Winding::Clockwise => "cw",
                }
            );
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

            let (kind, value_a, value_b) = event.payload.unpack();
            match kind {
                EventKind::Intersection => {
                    self.process_intersection(last_y, value_a as usize, value_b as usize);
                }
                EventKind::Start => {
                    self.activate_segment(next_y, value_a as usize);
                }
                EventKind::End => {
                    self.deactivate_segment(next_y, value_a as usize);
                }
                EventKind::Scanline => {
                    #[cfg(debug_assertions)]
                    self.validate_linked_list(false);

                    // This whole block is basically edge case avoidance.
                    // Additional sorting and intersection checks for degenerate cases.
                    // Note that like the rest of this algorithm this is STILL susceptible to
                    // edge cases but they should be limited to very degenerate ones that probably
                    // won't change the result that much.
                    {
                        let order_y = {
                            let c = last_y + I16Dot16::HALF;
                            if c >= next_y {
                                (last_y + next_y) / 2
                            } else {
                                c
                            }
                        };
                        let mut i = self.active_head;
                        while i != usize::MAX {
                            let segment = &mut self.segments[i];
                            let next = segment.next;
                            segment.next_x = segment.x_at_y(last_y, order_y);
                            self.move_left_until_sorted(i);
                            i = next;
                        }

                        if self.events.peek().is_some_and(|Reverse(e)| e.y < next_y) {
                            self.events.push(Reverse(event));
                            continue;
                        }
                    }

                    // eprintln!("Active segments at y range {last_y}-{next_y}:");
                    #[cfg(debug_assertions)]
                    self.validate_linked_list(false);

                    {
                        let mut i = self.active_head;
                        while i != usize::MAX {
                            let segment = &mut self.segments[i];
                            segment.advance_next_to_y(last_y, next_y);
                            i = segment.next;
                        }
                    }

                    // eprintln!("Computing coverage between scan lines {next_y} -- {last_y}");
                    self.add_coverage_between(next_y, last_y);

                    for i in 0..self.segments.len() {
                        self.segments[i].advance_current_to_next();
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

        let next = self.insert_segment_after(prev, i);

        if prev != usize::MAX {
            self.check_for_intersection(current_y, prev, i);
        }

        if next != usize::MAX {
            self.check_for_intersection(current_y, i, next);
        }
    }

    fn deactivate_segment(&mut self, current_y: I16Dot16, i: usize) {
        // eprintln!("Deactivating segment {i}");

        let (prev, next) = self.remove_segment(i);

        if prev != usize::MAX && next != usize::MAX {
            self.check_for_intersection(current_y, prev, next);
        }
    }

    fn insert_segment_after(&mut self, prev: usize, i: usize) -> usize {
        if prev == usize::MAX {
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
        }
    }

    fn remove_segment(&mut self, i: usize) -> (usize, usize) {
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

        (prev, next)
    }

    // TODO: Instead do a check_for_intersection_(left|right)_of and check chains of
    //       equal / very close segments.
    fn check_for_intersection(&mut self, current_y: I16Dot16, ai: usize, bi: usize) {
        debug_assert_ne!(ai, bi);

        // eprintln!(
        //     "Checking {ai} and {bi} for intersection",
        // );
        let mut intersections = [I16Dot16::ZERO; 2];
        let mut n_intersections = 0;
        self.find_intersection_y(current_y, ai, bi, &mut intersections, &mut n_intersections);

        for &y in &intersections[..n_intersections] {
            if y <= current_y {
                return;
            }

            self.events.push(Reverse(Event {
                y,
                payload: EventPayload::intersection(ai as u32, bi as u32),
            }));
            if y > 0 && y <= self.size.y as i32 {
                self.events.push(Reverse(Event {
                    y,
                    payload: EventPayload::scanline(),
                }));
            }
        }
    }

    /// Finds the intersection point of two linear segments via ternary search.
    fn find_linear_intersection_y(
        &self,
        current_y: I16Dot16,
        a: &Segment,
        b: &Segment,
    ) -> Option<I16Dot16> {
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
            let a_x = a.x_at_y(current_y, y);
            let b_x = b.x_at_y(current_y, y);
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

        Some(intersection_y)
    }

    /// Finds the intersection point of quadratic segments (or a quadratic and linear segment) via
    /// recursive subdivision.
    ///
    /// This will also work with linear segments too but those can be handled in a simpler way
    /// and should go through [`Self::find_linear_intersection_y`] instead.
    fn find_quadratic_intersection_y(
        a: &Segment,
        b: &Segment,
        // TODO: Bring ArrayVec back
        output: &mut [I16Dot16; 2],
        output_n: &mut usize,
    ) {
        macro_rules! points_for_segment {
            (let $result: ident = $s: ident) => {
                let mut buffer = [$s.bottom, $s.top, Point2::ZERO];
                let $result = match &$s.kind {
                    SegmentKind::Linear { .. } => &buffer[..2],
                    &SegmentKind::Quadratic { control, .. } => {
                        buffer[1] = control;
                        buffer[2] = $s.top;
                        &buffer[..3]
                    }
                };
            };
        }

        points_for_segment!(let ap = a);
        points_for_segment!(let bp = b);

        let ab = Rect2::bounding_box_of_points(ap.iter().copied());
        let bb = Rect2::bounding_box_of_points(bp.iter().copied());

        intersect_curves(ap, ab, bp, bb, output, output_n)
    }

    fn find_intersection_y(
        &self,
        current_y: I16Dot16,
        ai: usize,
        bi: usize,
        output: &mut [I16Dot16; 2],
        output_n: &mut usize,
    ) {
        let a = &self.segments[ai];
        let b = &self.segments[bi];

        match (&a.kind, &b.kind) {
            // Intersecting linear segments is a lot easier than intersecting curves
            // so we handle that separately.
            (SegmentKind::Linear { .. }, SegmentKind::Linear { .. }) => {
                if let Some(intersection) = self.find_linear_intersection_y(current_y, a, b) {
                    output[0] = intersection;
                    *output_n = 1;
                }
            }
            _ => {
                Self::find_quadratic_intersection_y(a, b, output, output_n);
                let n = *output_n;
                *output_n = 0;
                for (i, v) in (*output)
                    .into_iter()
                    .take(n)
                    .filter(|&p| {
                        (p - a.bottom.y) >= I16Dot16::from_quotient(1, 4)
                            && (a.top.y - p) >= I16Dot16::from_quotient(1, 4)
                    })
                    .enumerate()
                {
                    output[i] = v;
                    *output_n = i + 1;
                }
                // eprintln!("{ai} {bi} {:?}", &output[..*output_n]);
            }
        }
    }
}

// TODO: Handle coincident curves.
//       Basically after some threshold based on the degree of our curves we should
//       assume that the curves don't actually intersect and are coincident.
fn intersect_curves(
    ap: &[Point2<I16Dot16>],
    ab: Rect2<I16Dot16>,
    bp: &[Point2<I16Dot16>],
    bb: Rect2<I16Dot16>,
    output: &mut [I16Dot16; 2],
    output_n: &mut usize,
) {
    if !ab.intersects(&bb) {
        return;
    }

    let ar = ab.signed_area();
    let br = bb.signed_area();

    debug_assert!(ar >= 0);
    debug_assert!(br >= 0);

    if *output_n > 1 {
        return;
    }

    if ar < I16Dot16::from_quotient(1, 4) && br < I16Dot16::from_quotient(1, 4) {
        let point = ab.intersection(&bb).center();
        let n = *output_n;
        if n == 1 {
            if (output[0] - point.y) > I16Dot16::from_quotient(1, 16) {
                output[1] = point.y;
                *output_n += 1;
            }
        } else {
            output[0] = point.y;
            *output_n += 1;
        }
        return;
    }

    let split = |p: &[Point2<I16Dot16>]| match p.len() {
        2 => {
            let mid = p[0].midpoint(p[1]);
            ([p[0], mid, Point2::ZERO], [mid, p[1], Point2::ZERO])
        }
        3 => {
            let ctrl1 = p[0].midpoint(p[1]);
            let ctrl2 = p[1].midpoint(p[2]);
            let mid = ctrl1.midpoint(ctrl2);
            ([p[0], ctrl1, mid], [mid, ctrl2, p[2]])
        }
        _ => unreachable!(),
    };

    let (lp, sp, sb) = if ar > br { (ap, bp, bb) } else { (bp, ap, ab) };
    let (lpb1, lpb2) = split(lp);
    let (lp1, lp2) = (&lpb1[..lp.len()], &lpb2[..lp.len()]);
    let (lb1, lb2) = (
        Rect2::bounding_box_of_points(lp1.iter().copied()),
        Rect2::bounding_box_of_points(lp2.iter().copied()),
    );

    intersect_curves(lp1, lb1, sp, sb, output, output_n);
    intersect_curves(lp2, lb2, sp, sb, output, output_n);
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
    use util::math::{I16Dot16, Outline, Point2f, Vec2};

    use crate::sw::PathRasterizer;

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
                for y in (0..size.y as usize).rev() {
                    print_row(y, false);
                    eprintln!()
                }
            }

            panic!()
        }
    }

    #[expect(dead_code)]
    fn test_outline(outline: &Outline, expected: &[u8]) {
        let sizef = outline.control_box().size();
        let size = Vec2::new(sizef.x.ceil() as u32, sizef.y.ceil() as u32);
        let mut rasterizer = PathRasterizer::new();
        rasterizer.reset(size, size.x as usize);
        rasterizer.add_outline(outline, Vec2::ZERO, 0.5);
        rasterizer.rasterize();

        compare(size, rasterizer.coverage(), expected);
    }

    #[test]
    fn coverage() {
        const SIZE: Vec2<u32> = Vec2::new(15, 10);

        let mut rasterizer = PathRasterizer::new();
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
            0x00, 0x00, 0x00, 0x33, 0xCA, 0x60, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x99, 0xFF, 0xFF, 0xEC, 0x8A, 0x22, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0xF3, 0xFF, 0xFF, 0xFF, 0xFF, 0xFD, 0x7F, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x66, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xD3, 0x0F, 0x00, 0x00, 0x00,
            0x00, 0x00, 0xCC, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xC0, 0x07, 0x00, 0x00,
            0x00, 0x33, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xA8, 0x01, 0x00,
            0x00, 0x99, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00,
            0x0C, 0xF3, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xF3, 0xC0, 0x89, 0x52, 0x1B, 0x00,
            0x66, 0xFF, 0xFF, 0xFF, 0xFC, 0xD2, 0x9B, 0x64, 0x2D, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00,
            0xB1, 0xAD, 0x76, 0x40, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        compare(SIZE, rasterizer.coverage(), EXPECTED);
    }

    // TODO: quadratic test cases
}
